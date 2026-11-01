//! Korean text uses the bundled CPU PaddleOCR worker by default.
//! Windows OCR remains explicitly selectable and is used for non-Korean languages.

use vorax_scan::pixels::Img;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

pub struct Ocr {
    reader: Reader,
    pub language: String,
    pub backend: String,
    cache: std::cell::RefCell<std::collections::VecDeque<(Img, String)>>,
}
enum Reader {Windows(OcrEngine),Paddle(std::cell::RefCell<super::paddle::Paddle>)}
fn worker_error(message:String)->windows::core::Error {
    windows::core::Error::new(windows::core::HRESULT(0x80004005u32 as i32),message)
}

impl Ocr {
    pub fn new() -> windows::core::Result<Ocr> {
        let requested = std::env::var("VORAX_LANGUAGE").unwrap_or_else(|_| "ko".into());
        let backend=std::env::var("VORAX_OCR_BACKEND").unwrap_or_else(|_|if requested=="ko" {"paddle".into()} else {"windows".into()});
        if backend=="paddle" {
            if requested!="ko" {return Err(worker_error("the bundled PaddleOCR worker currently supports Korean".into()));}
            let worker=super::paddle::Paddle::new().map_err(worker_error)?;
            return Ok(Ocr{reader:Reader::Paddle(std::cell::RefCell::new(worker)),language:"ko-KR".into(),backend:"paddleocr-v5-korean-cpu".into(),cache:Default::default()});
        }
        if backend!="windows" {return Err(worker_error(format!("unsupported OCR backend: {backend}")));}
        Self::for_language(&requested)
    }

    pub(crate) fn for_language(requested: &str) -> windows::core::Result<Ocr> {
        let en = Language::CreateLanguage(&HSTRING::from(if requested == "ko" { "ko-KR" } else { "en-US" }))?;
        if requested == "ko" && !OcrEngine::IsLanguageSupported(&en)? {
            return Err(windows::core::Error::new(windows::core::HRESULT(0x80070032u32 as i32), "Windows Korean OCR is not installed; using card art"));
        }
        let engine = if OcrEngine::IsLanguageSupported(&en)? { OcrEngine::TryCreateFromLanguage(&en)? } else { OcrEngine::TryCreateFromUserProfileLanguages()? };
        let language = engine.RecognizerLanguage()?.LanguageTag()?.to_string();
        Ok(Ocr { reader:Reader::Windows(engine), language,backend:"windows-media-ocr".into(), cache: Default::default() })
    }

    pub fn details(&self)->serde_json::Value {
        match &self.reader {
            Reader::Paddle(worker)=>{let worker=worker.borrow();serde_json::json!({"backend":self.backend,"startup":worker.startup,"last":worker.last})},
            Reader::Windows(_)=>serde_json::json!({"backend":self.backend,"language":self.language}),
        }
    }

    /// The text in a picture (dark text on white), line by line.
    pub fn read(&self, img: &Img) -> windows::core::Result<String> {
        // Exact pixels only: animated text or a changed card never reuses an old OCR result.
        if let Some((_, text)) = self.cache.borrow().iter().find(|(old, _)| old.width == img.width && old.height == img.height && old.data == img.data) {
            return Ok(text.clone());
        }
        let text=match &self.reader {
            Reader::Paddle(worker)=>worker.borrow_mut().read(img).map_err(worker_error)?,
            Reader::Windows(engine)=>{
        let bmp = SoftwareBitmap::CreateWithAlpha(BitmapPixelFormat::Bgra8, img.width as i32, img.height as i32, BitmapAlphaMode::Premultiplied)?;
        let mut bgra = img.data.clone();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        bmp.CopyFromBuffer(&writer.DetachBuffer()?)?;
        let result = engine.RecognizeAsync(&bmp)?.get()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        lines.join("\n")
            }
        };
        let mut cache = self.cache.borrow_mut();
        let capacity = if std::env::var("VORAX_OCR_MODE").is_ok_and(|v| v == "accurate") { 32 } else { 12 };
        const MAX_CACHE_BYTES:usize=8*1024*1024;
        while !cache.is_empty() && (cache.len()>=capacity || cache.iter().map(|(i,_)|i.data.len()).sum::<usize>()+img.data.len()>MAX_CACHE_BYTES) {cache.pop_front();}
        if img.data.len()<=MAX_CACHE_BYTES {cache.push_back((img.clone(), text.clone()));}
        Ok(text)
    }
}
