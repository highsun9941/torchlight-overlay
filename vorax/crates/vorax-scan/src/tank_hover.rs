//! Read the player's enlarged monster tooltip, never the animated monster in a tank.
use crate::hover::{inspect_monster_tooltip, Tooltip};
use crate::pixels::{crop, resize, Img, Rect};
use crate::reading::Reading;
use vorax::types::Rarity;

#[derive(Clone, Debug, serde::Serialize)]
pub struct TankHoverRead {
    pub name: String,
    pub rarity: Rarity,
    pub total: i64,
    pub activity: i64,
    pub quantity: i64,
    pub tooltip: Tooltip,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct TankHoverFailure { pub stage: &'static str, pub texts: Vec<String> }

/// Do not repair ambiguous OCR characters (O/0, l/1) or infer a missing factor.
pub fn positive_number(text: &str) -> Option<i64> {
    let s: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() { return None; }
    let groups: Vec<_> = s.split(',').collect();
    if groups.len() > 1 && (groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3)) { return None; }
    if groups.iter().any(|g| !g.bytes().all(|b| b.is_ascii_digit())) { return None; }
    let n = groups.concat().parse().ok()?;
    (n > 0).then_some(n)
}

fn rarity(text: &str) -> Option<Rarity> {
    let s: String = text.chars().filter(|c| crate::korean::hangul(*c) || c.is_ascii_alphabetic()).flat_map(char::to_lowercase).collect();
    match s.as_str() {
        "노멀몬스터" | "일반몬스터" | "normalmonster" => Some(Rarity::Normal),
        "매직몬스터" | "magicmonster" => Some(Rarity::Magic),
        "레어몬스터" | "raremonster" => Some(Rarity::Rare),
        "보스몬스터" | "bossmonster" => Some(Rarity::Boss),
        _ => None,
    }
}

/// Fixed text boxes relative to the detected popup. Extract ink at native size before scaling;
/// applying the background opening to enlarged glyphs can erase a wide digit's centre.
pub fn text_crop(img: &Img, tip: &Tooltip, area: Rect) -> Img {
    let r = tip.rect;
    let ink = crate::titles::text_ink(&crop(img, Rect::new(r.x + area.x*r.w, r.y + area.y*r.h, area.w*r.w, area.h*r.h)));
    let text = resize(&ink, ink.width*4, ink.height*4);
    let mut out = Img::new(text.width+32, text.height+32);
    out.data.fill(255);
    for y in 0..text.height {
        let at = ((y+16)*out.width+16)*4;
        out.data[at..at+text.width*4].copy_from_slice(&text.data[y*text.width*4..(y+1)*text.width*4]);
    }
    out
}

/// Keep the centred numeric ink group, separated from side ornaments by whitespace.
/// Do not repair OCR strings or remove characters within that group: strict numeric parsing
/// and the independent product check still reject an ambiguous reading.
pub fn numeric_ink(img: &Img) -> Img {
    let counts: Vec<usize> = (0..img.width).map(|x|(0..img.height).filter(|y| {
        let (r,g,b)=img.px(x,*y);r.max(g).max(b)<180
    }).count()).collect();
    let gap=(img.height.saturating_sub(32)/5).max(3);
    let mut groups: Vec<(usize,usize)> = Vec::new();
    for (x,_) in counts.iter().enumerate().filter(|(_,n)|**n>=2) {
        if let Some(last)=groups.last_mut().filter(|g|x-g.1<=gap) {last.1=x;}
        else {groups.push((x,x));}
    }
    let centre=img.width as f64/2.0;
    let distance=|(left,right): &(usize,usize)|( *left as f64-centre).max(centre-*right as f64).max(0.0);
    let Some(&(left,right))=groups.iter().min_by(|a,b|distance(a).total_cmp(&distance(b))) else {return img.clone()};
    if distance(&(left,right))>img.width as f64*0.2 {return img.clone();}
    let pad=(img.height/24).max(4);
    let mut out=img.clone();
    for y in 0..out.height {for x in 0..out.width {
        if x<left.saturating_sub(pad) || x>right.saturating_add(pad) {
            let at=(y*out.width+x)*4;out.data[at..at+4].fill(255);
        }
    }}
    out
}

pub fn recognize(img: &Img, read: &mut dyn FnMut(&Img)->String) -> Result<TankHoverRead,TankHoverFailure> {
    let mut texts = Vec::new();
    let Some(tip) = inspect_monster_tooltip(img).tooltip else { return Err(TankHoverFailure { stage:"tooltip_missing", texts }); };
    let mut get = |area,numeric| {
        let crop=text_crop(img,&tip,area);
        let s=read(&if numeric {numeric_ink(&crop)} else {crop});texts.push(s.clone());s
    };
    // A potion footer must never qualify as a monster tooltip.
    let footer = get(Rect::new(0.15,0.887,0.70,0.073),false);
    let Some(rarity) = rarity(&footer) else { return Err(TankHoverFailure { stage:"monster_footer", texts }); };
    // The portrait ends immediately above the name. The name is diagnostic only: tank
    // association uses its already verified category, independent compact total and pointer.
    let name = get(Rect::new(0.25,0.418,0.50,0.060),false);
    let name: String = name.chars().filter(|c| crate::korean::hangul(*c) || c.is_ascii_alphabetic()).collect();
    let total = positive_number(&get(Rect::new(0.24,0.616,0.52,0.070),true));
    let activity = positive_number(&get(Rect::new(0.09,0.807,0.23,0.062),true));
    let quantity = positive_number(&get(Rect::new(0.68,0.807,0.23,0.062),true));
    let (Some(total),Some(activity),Some(quantity)) = (total,activity,quantity) else { return Err(TankHoverFailure { stage:"numbers", texts }); };
    if activity>1_000_000 || quantity>1_000_000 || activity.checked_mul(quantity)!=Some(total) {
        return Err(TankHoverFailure { stage:"product", texts });
    }
    Ok(TankHoverRead { name, rarity, total, activity, quantity, tooltip:tip })
}

/// All occupied tanks participate, including confirmed ones. A tooltip from another tank must
/// not fill the only failed tank merely because the guide currently asks for that tank.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Association {pub slot:Option<usize>,pub candidates:Vec<(usize,&'static str)>}

fn pair(text: &str) -> Option<(i64,i64)> {
    let (a,q)=text.split_once('x')?;Some((positive_number(a)?,positive_number(q)?))
}

pub fn inspect_target(read: &TankHoverRead, r: &Reading, pointer: Option<usize>) -> Association {
    let candidates: Vec<_> = r.state.tanks.iter().enumerate().map(|(i,m)| {
        let sure = |part: &str| r.fields.get(&format!("tank{i}.{part}")).is_some_and(|f|f.sure);
        let reason=(|| {
            let Some(m)=m else {return "empty"};
            if !sure("rarity") || !sure("archetype") {return "category_unknown";}
            if m.rarity!=read.rarity {return "rarity_mismatch";}
            if (sure("activity") && m.activity!=read.activity) || (sure("quantity") && m.quantity!=read.quantity) {return "verified_value_mismatch";}
            let debug=r.debug.as_ref().and_then(|d|d.tanks.get(i)).and_then(|d|d.as_ref());
            if let Some(total)=debug.and_then(|d|positive_number(&d.total)) {
                return if total==read.total {"matched_total"} else {"total_mismatch"};
            }
            // Missing total is precisely why the original reader withheld both factors.
            // An independently read, complete, high-scoring compact A×Q line can still
            // identify this tank; never use fallback state values or divide the total.
            let strong=["activity","quantity"].iter().all(|p|r.fields.get(&format!("tank{i}.{p}")).and_then(|f|f.score).is_some_and(|s|s>=0.75));
            if let Some((a,q))=debug.and_then(|d|pair(&d.aq)).filter(|_|strong) {
                return if (a,q)==(read.activity,read.quantity) && a.checked_mul(q)==Some(read.total) {"matched_activity_quantity"} else {"pair_mismatch"};
            }
            "source_numbers_unreadable"
        })();
        (i,reason)
    }).collect();
    let matching: Vec<_>=candidates.iter().filter(|(_,why)|matches!(*why,"matched_total"|"matched_activity_quantity")).map(|(i,_)|*i).collect();
    let slot=if let Some(i)=pointer {matching.contains(&i).then_some(i)} else {(matching.len()==1).then(||matching[0])};
    Association{slot,candidates}
}

pub fn target(read: &TankHoverRead, r: &Reading, pointer: Option<usize>) -> Option<usize> {inspect_target(read,r,pointer).slot}

/// A hit test over static labels and the tank itself, used only to observe the user's cursor.
pub fn pointer_tank(width: usize, height: usize, ui: f64, x: f64, y: f64) -> Option<usize> {
    let l = crate::layout::layout_at(width,height,ui);
    (0..6).filter(|i| {
        let glass = l.tank_glass[*i];
        let name = l.name_band[*i];
        let inside = |r: Rect| x>=r.x && x<=r.x+r.w && y>=r.y && y<=r.y+r.h;
        inside(glass) || inside(Rect::new(name.x,name.y-10.0*l.k,name.w,l.aq_band[*i].y+l.aq_band[*i].h-name.y+20.0*l.k))
    }).min_by(|a,b|(x-l.tank_cx[*a]).abs().total_cmp(&(x-l.tank_cx[*b]).abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_ocr_rejects_guesses_and_malformed_grouping() {
        for (s,n) in [(" 24\n",Some(24)),("1,234",Some(1234)),("0",None),("O",None),("l20",None),("12,34",None),("5x24",None),("-5",None),("",None)] { assert_eq!(positive_number(s),n,"{s}"); }
        assert_eq!(rarity("매직 포션"),None);
        assert_eq!(rarity("매직 몬스터"),Some(Rarity::Magic));
    }
}
