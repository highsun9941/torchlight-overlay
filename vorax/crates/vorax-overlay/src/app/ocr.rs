//! Card titles read with Windows' own text reader (Windows.Media.Ocr): English when that language pack is
//! installed, else the first language of the user's profile (Latin letters read fine either way; the names
//! are matched loosely). Upstream reads them with Tesseract; when neither is there, the pictures decide.

use vorax_scan::pixels::Img;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

pub struct Ocr {
    engine: OcrEngine,
    pub language: String,
    cache: std::cell::RefCell<std::collections::VecDeque<(Img, String)>>,
}

impl Ocr {
    pub fn new() -> windows::core::Result<Ocr> {
        let requested = std::env::var("VORAX_LANGUAGE").unwrap_or_else(|_| "ko".into());
        Self::for_language(&requested)
    }

    pub(crate) fn for_language(requested: &str) -> windows::core::Result<Ocr> {
        let en = Language::CreateLanguage(&HSTRING::from(if requested == "ko" { "ko-KR" } else { "en-US" }))?;
        if requested == "ko" && !OcrEngine::IsLanguageSupported(&en)? {
            return Err(windows::core::Error::new(windows::core::HRESULT(0x80070032u32 as i32), "Windows Korean OCR is not installed; using card art"));
        }
        let engine = if OcrEngine::IsLanguageSupported(&en)? { OcrEngine::TryCreateFromLanguage(&en)? } else { OcrEngine::TryCreateFromUserProfileLanguages()? };
        let language = engine.RecognizerLanguage()?.LanguageTag()?.to_string();
        Ok(Ocr { engine, language, cache: Default::default() })
    }

    /// The text in a picture (dark text on white), line by line.
    pub fn read(&self, img: &Img) -> windows::core::Result<String> {
        // Exact pixels only: animated text or a changed card never reuses an old OCR result.
        if let Some((_, text)) = self.cache.borrow().iter().find(|(old, _)| old.width == img.width && old.height == img.height && old.data == img.data) {
            return Ok(text.clone());
        }
        let bmp = SoftwareBitmap::CreateWithAlpha(BitmapPixelFormat::Bgra8, img.width as i32, img.height as i32, BitmapAlphaMode::Premultiplied)?;
        let mut bgra = img.data.clone();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        bmp.CopyFromBuffer(&writer.DetachBuffer()?)?;
        let result = self.engine.RecognizeAsync(&bmp)?.get()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        let text = lines.join("\n");
        let mut cache = self.cache.borrow_mut();
        let capacity = if std::env::var("VORAX_OCR_MODE").is_ok_and(|v| v == "accurate") { 32 } else { 12 };
        if cache.len() >= capacity { cache.pop_front(); }
        cache.push_back((img.clone(), text.clone()));
        Ok(text)
    }
}
