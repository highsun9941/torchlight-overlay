//! Bounded local diagnostics: native capture pixels, OCR inputs and the exact rejection stage.
use std::sync::atomic::{AtomicU64,Ordering};
use vorax_scan::{pixels::Img,reading::Reading};
use super::sys::{data_dir,note};

pub fn next_scan()->u64 { static NEXT:AtomicU64=AtomicU64::new(1);NEXT.fetch_add(1,Ordering::Relaxed) }
pub fn event(scan:u64,stage:&str,details:serde_json::Value) {
    note(&format!("diagnostic {}",serde_json::json!({"scan":scan,"pid":std::process::id(),"stage":stage,"details":details})));
}

/// Keep at most three diagnostic groups and save no more than once per 15 s. Never transmitted.
pub fn save(scan:u64,kind:&str,frame:&Img,after:Option<&Img>,reading:Option<&Reading>,ocr:&[serde_json::Value],crops:&[Img],extra:serde_json::Value) {
    static LAST:AtomicU64=AtomicU64::new(0);
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs());
    let previous=LAST.load(Ordering::Relaxed);
    if now.saturating_sub(previous)<15 {return;}
    LAST.store(now,Ordering::Relaxed);
    let dir=data_dir().join("diagnostics");
    let result=(||->Result<(),String>{
        std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
        let stem=format!("failure-{}",(now/15)%3);
        for file in std::fs::read_dir(&dir).map_err(|e|e.to_string())?.flatten() {
            if file.file_name().to_string_lossy().starts_with(&stem) {let _=std::fs::remove_file(file.path());}
        }
        let write=|suffix:&str,img:&Img|crate::pipeline::encode_png(&dir.join(format!("{stem}-{suffix}.png")),img.width as u32,img.height as u32,&img.data);
        write("before",frame)?;
        if let Some(img)=after {write("after",img)?;}
        for (i,img) in crops.iter().take(4).enumerate() {write(&format!("ocr{i}"),img)?;}
        let value=serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"scan":scan,"pid":std::process::id(),"kind":kind,"nativeSize":[frame.width,frame.height],"reading":reading,"ocr":ocr,"details":extra});
        std::fs::write(dir.join(format!("{stem}.json")),serde_json::to_vec_pretty(&value).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        Ok(())
    })();
    event(scan,"diagnostic_saved",serde_json::json!({"directory":dir,"result":result.err()}));
}
