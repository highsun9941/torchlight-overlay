//! Saved-image integration with the actual bundled model, never supplied OCR strings.
use std::path::Path;
use serde_json::{json,Value};
use vorax_scan::pixels::Img;
use super::ocr::Ocr;

fn load(root:&Path,file:&str)->Result<Img,String> {
    let frame=crate::pipeline::decode_png(&root.join(file))?;
    Ok(vorax_scan::window::game_view(&frame).map_or_else(||frame.clone(),|r|vorax_scan::window::crop_to(&frame,&r)))
}

pub fn run(root:&Path,out:&Path)->Result<(),String> {
    std::fs::create_dir_all(out).map_err(|e|e.to_string())?;
    let ocr=Ocr::new().map_err(|e|e.to_string())?;
    if ocr.backend!="paddleocr-v5-korean-cpu" {return Err("validation requires the actual CPU PaddleOCR backend".into());}
    let mut attempts=Vec::new();let mut cases=Vec::new();let mut index=0usize;
    let mut failed=Vec::new();
    for (file,phase,expected) in [
        ("gold-saw/tooltip.png","instrument","Pig Iron Bone Saw"),
        ("reported/hover-before.png","instrument","Adherent Metatarsals"),
        ("title-motion/tooltip.png","potion","Green Bile Solution"),
        ("round-memory/normal-tooltip.png","potion","Twin Hormone - Pest"),
    ] {
        let frame=load(root,file)?;
        let source=if expected=="Pig Iron Bone Saw" {
            vorax_scan::scan(&load(root,"gold-saw/raised.png")?,vorax_scan::scan_data(),true).hand_ranks
                .and_then(|r|r.first().cloned()).unwrap_or_default()
        } else {vec![]};
        let mut reader=|img:&Img| {
            let name=format!("crop-{index:03}.png");index+=1;
            let _=crate::pipeline::encode_png(&out.join(&name),img.width as u32,img.height as u32,&img.data);
            let result=ocr.read(img);
            attempts.push(json!({"file":file,"crop":name,"width":img.width,"height":img.height,"text":result.as_ref().ok(),"error":result.as_ref().err().map(|e|e.to_string()),"model":ocr.details()}));
            result.unwrap_or_default()
        };
        let result=vorax_scan::hover::recognize(&frame,phase,&source,&mut reader);
        let ok=result.as_ref().is_ok_and(|r|r.name==expected);
        if !ok {failed.push(file.to_string());}
        cases.push(json!({"file":file,"expected":expected,"passed":ok,"result":result}));
    }
    let mut pipe=crate::pipeline::Pipeline::new(0);
    let mut reader=|img:&Img|ocr.read(img).unwrap_or_default();
    let first=pipe.run(&load(root,"gold-saw/raised.png")?,None,Some(&mut reader));
    let direct=matches!(first,crate::pipeline::Outcome::Advice{..});
    let before=pipe.requested_slots();
    if !direct {
        let candidates=pipe.hover_candidates();
        let source=candidates.iter().find(|(i,_)|*i==0).map(|(_,r)|r.as_slice()).unwrap_or(&[]);
        if let Ok(read)=vorax_scan::hover::recognize(&load(root,"gold-saw/tooltip.png")?,"instrument",source,&mut reader) {
            let _=pipe.confirm_hover(0,&read,500.0);let _=pipe.confirm_hover(0,&read,1000.0);
        }
    }
    let result=pipe.run(&load(root,"gold-saw/rest.png")?,None,Some(&mut reader));
    let (passed,reading)=match result {
        crate::pipeline::Outcome::Advice{reading,..} => (reading.state.hand.first().is_some_and(|n|n=="Pig Iron Bone Saw"),serde_json::to_value(reading).unwrap()),
        crate::pipeline::Outcome::Uncertain{reading} => (false,serde_json::to_value(reading).unwrap()),
        _ => (false,Value::Null),
    };
    if !passed {failed.push("gold-saw/pipeline".into());}
    let report=json!({"backend":ocr.backend,"model":ocr.details(),"cases":cases,"attempts":attempts,
        "sawPipeline":{"passed":passed,"direct":direct,"requestedBefore":before,"reading":reading},"failures":failed});
    std::fs::write(out.join("model-validation.json"),serde_json::to_vec_pretty(&report).unwrap()).map_err(|e|e.to_string())?;
    println!("PADDLE_VALIDATION {}",serde_json::to_string(&report).unwrap());
    if failed.is_empty() {Ok(())} else {Err(format!("PaddleOCR validation failed: {}",failed.join(", ")))}
}
