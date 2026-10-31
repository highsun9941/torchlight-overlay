//! User-assisted OCR of the game's enlarged card, with no mouse or keyboard automation.
use crate::layout::CardPos;
use crate::pixels::{Img, Rect};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Tooltip { pub rect: Rect, pub center: CardPosData, pub scale: f64 }
#[derive(Clone, Debug, serde::Serialize)]
pub struct CardPosData { pub x: f64, pub y: f64 }

/// Find a connected bright card border only in the left tooltip area. No fixed screenshot hash or name.
pub fn tooltip(img: &Img) -> Option<Tooltip> {
    let (w,h)=(img.width*27/100,img.height*70/100);
    let y0=img.height/4;
    if w<2 || h<2 {return None;}
    let mut mask=vec![false;w*h];
    for y in 0..h {for x in 0..w {
        let (r,g,b)=img.px(x,y+y0);
        mask[y*w+x]=(r>145 && g>80 && b<150 && r as f64>g as f64*1.1)
            || (b>145 && g>85 && b as f64>r as f64*1.25);
    }}
    let k=img.height as f64/1440.0;
    let mut best:Option<(usize,Rect)>=None;
    for seed in 0..mask.len() {
        if !mask[seed] {continue;} mask[seed]=false;
        let mut queue=vec![seed]; let mut at=0;
        let (mut x0,mut x1,mut top,mut bottom)=(w,0,h,0);
        while at<queue.len() {
            let i=queue[at];at+=1;let (x,y)=(i%w,i/w);
            x0=x0.min(x);x1=x1.max(x);top=top.min(y);bottom=bottom.max(y);
            for dy in -1i32..=1 {for dx in -1i32..=1 {
                let (xx,yy)=(x as i32+dx,y as i32+dy);
                if xx<0||yy<0||xx>=w as i32||yy>=h as i32 {continue;}
                let j=yy as usize*w+xx as usize;
                if mask[j] {mask[j]=false;queue.push(j);}
            }}
        }
        let (ww,hh)=((x1-x0+1) as f64,(bottom-top+1) as f64);
        if ww>=180.0*k && ww<=500.0*k && hh>=260.0*k && hh<=700.0*k && (0.5..0.85).contains(&(ww/hh)) {
            let area=(ww*hh) as usize;
            if best.as_ref().is_none_or(|b|area>b.0) {best=Some((area,Rect::new(x0 as f64,(top+y0) as f64,ww,hh)));}
        }
    }
    best.map(|(_,r)| {
        let scale=(r.h-12.0*k)/340.0;
        Tooltip {rect:r,center:CardPosData{x:r.x+r.w/2.0,y:r.y+r.h/2.0},scale}
    })
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct HoverRead {pub name:String,pub text:String,pub title_score:f64,pub art_score:f64,pub tooltip:Tooltip}

#[derive(Clone,Debug,serde::Serialize)]
pub struct HoverFailure {pub stage:String,pub texts:Vec<String>,pub art:Vec<(String,f64)>}

pub fn recognize(img:&Img, phase:&str, original:&[(String,f64)], read:&mut dyn FnMut(&Img)->String)->Result<HoverRead,HoverFailure> {
    let tip=tooltip(img).ok_or_else(||HoverFailure{stage:"tooltip_missing".into(),texts:vec![],art:vec![]})?;
    let mut texts=Vec::new();
    let c=CardPos {cx:tip.center.x,cy:tip.center.y,angle:0.0};
    let data=crate::scan_data();
    let art=crate::cards::rank_card_art(img,&c,tip.scale,phase,data);
    let names=match phase {"instrument"=>&data.names.instrument,"protocol"=>&data.names.protocol,_=>&data.names.potion};
    let mut attempts=vec![crate::titles::heading_crop(img,&c,tip.scale),crate::titles::title_crop(img,&c,tip.scale)];
    for crop in attempts.drain(..) {
        let text=read(&crop);
        texts.push(text.clone());
        let mut ranks:Vec<_>=names.iter().map(|n|(n,crate::titles::name_fit(n,&text))).collect();
        ranks.sort_by(|a,b|b.1.total_cmp(&a.1));
        let Some((name,q))=ranks.first() else {continue;};
        let score=art.iter().find(|(n,_)|n==*name).map_or(0.0,|x|x.1);
        let supported=original.is_empty() || original.iter().all(|(_,s)|*s<0.90) || original.iter().any(|(n,s)|n==*name&&*s>=0.90);
        if *q>=6.0 && *q-ranks.get(1).map_or(f64::NEG_INFINITY,|x|x.1)>=3.0 && score>=0.92 && supported {
            return Ok(HoverRead {name:(*name).clone(),text,title_score:*q,art_score:score,tooltip:tip});
        }
    }
    Err(HoverFailure{stage:"title_or_art_not_verified".into(),texts,art:art.into_iter().take(5).collect()})
}

/// A visible enlarged card may be read while the helper has focus or the pointer has already moved.
/// Use its name/art to find a unique unresolved slot. Pointer geometry only breaks a real ambiguity.
pub fn target(name:&str,candidates:&[(usize,Vec<(String,f64)>)],pointer:Option<usize>)->Option<usize> {
    let matching:Vec<_>=candidates.iter().filter(|(_,r)|r.is_empty() || r.iter().all(|(_,s)|*s<0.90)
        || r.iter().any(|(n,s)|n==name&&*s>=0.90)).map(|(i,_)|*i).collect();
    if matching.len()==1 {matching.first().copied()}
    else {pointer.filter(|i|matching.contains(i))}
}

#[cfg(test)]
mod target_tests {
    use super::target;
    #[test]
    fn visible_tooltip_does_not_require_foreground_or_pointer_but_ambiguous_slots_do() {
        let a=vec![(0,vec![("foot".into(),0.99)]),(1,vec![("brain".into(),0.99)])];
        assert_eq!(target("foot",&a,None),Some(0));
        assert_eq!(target("foot",&a,Some(1)),Some(0));
        assert_eq!(target("claw",&a,None),None);
        let b=vec![(0,vec![("potion".into(),0.99)]),(1,vec![("potion".into(),0.99)])];
        assert_eq!(target("potion",&b,None),None);
        assert_eq!(target("potion",&b,Some(1)),Some(1));
        assert_eq!(target("foot",&[(2,vec![])],None),Some(2));
    }
}
