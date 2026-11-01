//! User-assisted OCR of the game's enlarged card, with no mouse or keyboard automation.
use crate::layout::CardPos;
use crate::pixels::{Img, Rect};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Tooltip { pub rect: Rect, pub center: CardPosData, pub scale: f64 }
#[derive(Clone, Debug, serde::Serialize)]
pub struct CardPosData { pub x: f64, pub y: f64 }

/// Find a connected bright card border only in the left tooltip area. No fixed screenshot hash or name.
fn connected_border(img: &Img) -> Option<Rect> {
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
    best.map(|(_,r)|r)
}

#[derive(Clone,Debug,serde::Serialize)]
pub struct TooltipSearch {
    pub tooltip:Option<Tooltip>,pub method:Option<&'static str>,
    pub blue_sides:usize,pub blue_pairs:usize,pub edge_score:f64,
}

/// Blue glow is broken by the background and need not form a connected component. Fit both long
/// sides and independently verify top and bottom; lowering a global colour threshold joins scenery.
pub fn inspect_tooltip(img:&Img)->TooltipSearch {
    let k=img.height as f64/1440.0;
    let make=|r:Rect|Tooltip{rect:r,center:CardPosData{x:r.x+r.w/2.0,y:r.y+r.h/2.0},scale:(r.h-12.0*k)/340.0};
    // Gold glow can connect to the illustration or split along the left edge. Fit the four
    // frame edges before trusting the bounding box of that connected glow.
    let gold=fit_edges(img,BorderColor::Gold);
    if gold.tooltip.is_some() {return gold;}
    if let Some(r)=connected_border(img) {return TooltipSearch{tooltip:Some(make(r)),method:Some("connected_border"),blue_sides:0,blue_pairs:0,edge_score:1.0};}
    let blue=fit_edges(img,BorderColor::Blue);
    if blue.tooltip.is_some() {return blue;}
    let neutral=fit_edges(img,BorderColor::Neutral);
    if neutral.tooltip.is_some() {neutral} else {blue}
}

/// Normal potions have a dim grey frame. Fit all four edges independently, using the same
/// size/aspect checks as the blue frame; scenery colour alone never identifies a card.
#[derive(Clone,Copy)]
enum BorderColor {Blue,Neutral,Gold}
fn fit_edges(img:&Img,color:BorderColor)->TooltipSearch {
    let k=img.height as f64/1440.0;
    let make=|r:Rect|Tooltip{rect:r,center:CardPosData{x:r.x+r.w/2.0,y:r.y+r.h/2.0},scale:(r.h-12.0*k)/340.0};
    let (w,h,y0)=(img.width*27/100,img.height*70/100,img.height/4);
    let mut result=TooltipSearch{tooltip:None,method:None,blue_sides:0,blue_pairs:0,edge_score:0.0};
    if w<3 || h<3 {return result;}
    let mut mask=vec![false;w*h];
    for y in 0..h {for x in 0..w {
        let (r,g,b)=img.px(x,y+y0);
        mask[y*w+x]=match color {
            BorderColor::Neutral=>r.min(g).min(b)>=60 && r.max(g).max(b)-r.min(g).min(b)<=48,
            BorderColor::Blue=>b>=115 && g>=60 && b as i32-r as i32>=40 && b as i32-g as i32>=25,
            BorderColor::Gold=>r>145 && g>80 && b<150 && r as f64>g as f64*1.1,
        };
    }}
    let on=|x:usize,y:usize|(x.saturating_sub(1)..=(x+1).min(w-1)).any(|xx|mask[y*w+xx]);
    let gap=(5.0*k).round().max(3.0) as usize;
    let mut sides=Vec::new();
    for x in 0..w {
        let (mut start,mut last,mut hits)=(0,0,0usize);
        let mut keep=|start:usize,last:usize,hits:usize| {
            let height=(last-start+1) as f64;
            if hits>0 && height>=260.0*k && height<=700.0*k && hits as f64/height>=0.85 {
                sides.push((x,start+y0,last+y0,hits as f64/height));
            }
        };
        for y in 0..h {
            if !on(x,y) {continue;}
            if hits==0 || y-last>gap {keep(start,last,hits);start=y;hits=0;}
            last=y;hits+=1;
        }
        keep(start,last,hits);
    }
    result.blue_sides=sides.len();
    if sides.len()>96 {return result;}
    let horizontal=|left:usize,right:usize,lo:f64,hi:f64| {
        let mut values=Vec::new();let mut best=0.0f64;
        for y in (lo.floor().max(y0 as f64) as usize)..=(hi.ceil().min((y0+h-1) as f64) as usize) {
            let at=y-y0;
            let score=(0..40).filter(|i| {
                let x=(left as f64+(right-left) as f64*(0.08+*i as f64/39.0*0.84)).round() as usize;
                (at.saturating_sub(2)..=(at+2).min(h-1)).any(|yy|mask[yy*w+x])
            }).count() as f64/40.0;
            best=best.max(score);values.push((y as f64,score));
        }
        let near:Vec<_>=values.into_iter().filter(|(_,s)|*s>=best-0.025).collect();
        let y=near.iter().map(|(y,_)|*y).sum::<f64>()/near.len().max(1) as f64;
        (y,best)
    };
    let mut rank=0.0;
    for (i,&(lx,lt,lb,ls)) in sides.iter().enumerate() {for &(rx,rt,rb,rs) in &sides[i+1..] {
        let width=(rx-lx) as f64;
        if width<180.0*k || width>500.0*k || lt.abs_diff(rt) as f64>200.0*k || lb.abs_diff(rb) as f64>200.0*k {continue;}
        let (top,ts)=horizontal(lx,rx,lt.min(rt) as f64-12.0*k,lt.max(rt) as f64+12.0*k);
        let (bottom,bs)=horizontal(lx,rx,lb.min(rb) as f64-12.0*k,lb.max(rb) as f64+12.0*k);
        let height=bottom-top;
        if height<260.0*k || height>700.0*k || !(0.5..0.85).contains(&(width/height))
            || ((lb-lt+1).min(rb-rt+1) as f64)<height*0.6 {continue;}
        result.blue_pairs+=1;result.edge_score=result.edge_score.max(ts.min(bs));
        if ts>=0.7 && bs>=0.7 && ts+bs+ls+rs>rank {
            rank=ts+bs+ls+rs;result.tooltip=Some(make(Rect::new(lx as f64,top,width,height)));result.method=Some(match color {BorderColor::Neutral=>"neutral_edges",BorderColor::Blue=>"blue_edges",BorderColor::Gold=>"gold_edges"});
        }
    }}
    result
}

pub fn tooltip(img:&Img)->Option<Tooltip> {inspect_tooltip(img).tooltip}

#[derive(Clone, Debug, serde::Serialize)]
pub struct HoverRead {pub name:String,pub text:String,pub title_score:f64,pub art_score:f64,pub tooltip:Tooltip,pub title_area:Rect,pub match_kind:String}

#[derive(Clone,Debug,serde::Serialize)]
pub struct HoverFailure {pub stage:String,pub texts:Vec<String>,pub art:Vec<(String,f64)>,pub tooltip:Option<Tooltip>,pub title_area:Option<Rect>,pub title_ranks:Vec<(String,f64)>,pub fragments:Vec<String>}

/// A damaged middle may be recovered only with two exact Hangul syllables at each end, no more
/// than two edits, and a unique name in the entire offer catalog. Generic description text and
/// short fragments such as "용액" cannot qualify. Artwork and slot association remain mandatory.
fn fragment_matches(local:&str,text:&str)->bool {
    let name:Vec<char>=local.chars().filter(|c|crate::korean::hangul(*c)).collect();
    if name.len()<5 {return false;}
    for line in text.lines() {
        let letters:Vec<char>=line.chars().filter(|c|crate::korean::hangul(*c)).collect();
        for start in 0..letters.len().saturating_sub(1) {
            if letters[start..start+2]!=name[..2] {continue;}
            for end in start+4..=letters.len().min(start+name.len()+2) {
                if letters[end-2..end]!=name[name.len()-2..] {continue;}
                let segment=&letters[start..end];let mut row:Vec<usize>=(0..=segment.len()).collect();
                for (i,ch) in name.iter().enumerate() {
                    let mut next=vec![i+1;segment.len()+1];
                    for j in 1..=segment.len() {next[j]=(row[j]+1).min(next[j-1]+1).min(row[j-1]+usize::from(ch!=&segment[j-1]));}
                    row=next;
                }
                if row[segment.len()]<=2 {return true;}
            }
        }
    }false
}

fn fragment_names<'a>(names:impl Iterator<Item=(&'a str,&'a str)>,text:&str)->Vec<&'a str> {
    names.filter(|(_,local)|fragment_matches(local,text)).map(|(name,_)|name).collect()
}

/// Short Hangul names have too few syllables for the normal fragment rule. Permit one damaged
/// suffix syllable only with an exact two-syllable prefix, an otherwise complete heading, and a
/// unique catalog match. The caller additionally requires the leading source art and popup art.
fn short_title_names<'a>(names:&'a [String],text:&str)->Vec<&'a str> {
    names.iter().filter(|name| {
        if crate::family::is_variant(name) {return false;}
        let local:Vec<_>=crate::korean::name(name).chars().filter(|c|crate::korean::hangul(*c)).collect();
        if !(4..=6).contains(&local.len()) {return false;}
        text.lines().any(|line| {
            if line.chars().any(|c|c.is_alphanumeric() && !crate::korean::hangul(c)) {return false;}
            let letters:Vec<_>=line.chars().filter(|c|crate::korean::hangul(*c)).collect();
            letters.len()==local.len() && letters[..2]==local[..2]
                && letters.iter().zip(&local).filter(|(a,b)|a!=b).count()==1
        })
    }).map(String::as_str).collect()
}

pub fn recognize(img:&Img, phase:&str, original:&[(String,f64)], read:&mut dyn FnMut(&Img)->String)->Result<HoverRead,HoverFailure> {
    let tip=tooltip(img).ok_or_else(||HoverFailure{stage:"tooltip_missing".into(),texts:vec![],art:vec![],tooltip:None,title_area:None,title_ranks:vec![],fragments:vec![]})?;
    let mut texts=Vec::new();
    let c=CardPos {cx:tip.center.x,cy:tip.center.y,angle:0.0};
    let data=crate::scan_data();
    let art=crate::cards::rank_card_art(img,&c,tip.scale,phase,data);
    let names=match phase {"instrument"=>&data.names.instrument,"protocol"=>&data.names.protocol,_=>&data.names.potion};
    let (heading,title_area)=if phase=="instrument" {crate::titles::gold_heading(img,&c,tip.scale,true)} else {crate::titles::located_heading(img,&c,tip.scale,true)};
    let mut attempts=vec![heading,crate::titles::family_crop(img,&c,tip.scale),crate::titles::title_crop(img,&c,tip.scale)];
    let mut title_ranks=Vec::new();let mut fragments=Vec::new();
    for crop in attempts.drain(..) {
        let part=read(&crop);texts.push(part.clone());
        let text=if texts.len()==2 {format!("{}\n{part}",texts[0])} else {part};
        let mut ranked:Vec<_>=names.iter().map(|n|(n.clone(),crate::titles::name_fit(n,&text))).collect();
        crate::family::rerank(&mut ranked,&text,names);
        let mut ranks:Vec<_>=ranked.iter().map(|(n,q)|(n,*q)).collect();
        ranks.sort_by(|a,b|b.1.total_cmp(&a.1));
        let Some((name,q))=ranks.first() else {continue;};
        title_ranks=ranks.iter().take(3).map(|(n,q)|((*n).clone(),*q)).collect();
        let score=art.iter().find(|(n,_)|n==*name).map_or(0.0,|x|x.1);
        let supported=original.is_empty() || original.iter().all(|(_,s)|*s<0.90) || original.iter().any(|(n,s)|n==*name&&*s>=0.90);
        if *q>=6.0 && *q-ranks.get(1).map_or(f64::NEG_INFINITY,|x|x.1)>=3.0 && score>=0.92 && supported && crate::family::verified(name,&text,names) {
            return Ok(HoverRead {name:(*name).clone(),text,title_score:*q,art_score:score,tooltip:tip,title_area,match_kind:"complete_title".into()});
        }
        if !(*q>=6.0 && *q-ranks.get(1).map_or(f64::NEG_INFINITY,|x|x.1)>=3.0) {
            let partial=fragment_names(names.iter().map(|n|(n.as_str(),crate::korean::name(n))),&text);
            fragments=partial.iter().map(|s|s.to_string()).collect();
            if let [name]=partial.as_slice() {
                let name=*name;
                let score=art.iter().find(|(n,_)|n.as_str()==name).map_or(0.0,|x|x.1);
                let supported=original.is_empty() || original.iter().all(|(_,s)|*s<0.90) || original.iter().any(|(n,s)|n.as_str()==name&&*s>=0.90);
                if score>=0.92 && supported && crate::family::verified(name,&text,names) {
                    return Ok(HoverRead{name:name.to_string(),title_score:crate::titles::name_fit(name,&text),text,art_score:score,tooltip:tip,title_area,match_kind:"unique_fragments".into()});
                }
            }
        }
    }
    // A four-syllable title such as 무쇠 뼈톱 may repeatedly lose just its last syllable.
    // A bare prefix, multiple damaged syllables, or a different leading source card is insufficient.
    let short=texts.first().map(|t|short_title_names(names,t)).unwrap_or_default();
    if let [name]=short.as_slice() {
        let score=art.iter().find(|(n,_)|n.as_str()==*name).map_or(0.0,|x|x.1);
        if original.first().is_some_and(|(n,s)|n.as_str()==*name && *s>=0.95) && score>=0.95 {
            return Ok(HoverRead{name:(*name).to_string(),text:texts[0].clone(),title_score:crate::titles::name_fit(name,&texts[0]),art_score:score,tooltip:tip,title_area,match_kind:"short_title_and_source_art".into()});
        }
    }
    Err(HoverFailure{stage:"title_or_art_not_verified".into(),texts,art:art.into_iter().take(5).collect(),tooltip:Some(tip),title_area:Some(title_area),title_ranks,fragments})
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
    #[test]
    fn damaged_hangul_requires_exact_ends_a_bounded_gap_and_catalog_uniqueness() {
        use super::{fragment_matches,fragment_names};
        assert!(fragment_matches("푸른 담즙 용액","푸른 {』'言 용액"));
        assert!(fragment_matches("푸른 담즙 용액","푸른 {』'죠 용액"));
        for bad in ["용액","담즙 용액","푸른 기타 다른 용액","1~2 그룹 몬스터를 선택해 랜덤 매직 몬스터로 변이시키고"] {
            assert!(!fragment_matches("푸른 담즙 용액",bad),"{bad}");
        }
        let names=[("a","푸른 담즙 용액"),("b","푸른 농축 용액")];
        assert_eq!(fragment_names(names.into_iter(),"푸른 용액").len(),2,"ambiguous fragments must never be treated as unique");
    }
    #[test]
    fn a_short_title_needs_a_complete_unique_line_and_exact_prefix() {
        use super::short_title_names;
        let names=&crate::scan_data().names.instrument;
        for text in ["무쇠뼈롭","무쇠 뼈를"] {assert_eq!(short_title_names(names,text),["Pig Iron Bone Saw"]);}
        for text in ["무쇠","무쇄뼈롭","무소l떠롭","라운드 종료 시 무쇠뼈롭","무쇠뼈롭 150"] {assert!(short_title_names(names,text).is_empty(),"{text}");}
        let ambiguous=vec!["Pig Iron Bone Saw".to_string(),"무쇠뼈봉".to_string()];
        assert_eq!(short_title_names(&ambiguous,"무쇠뼈롭").len(),2);
    }
}
