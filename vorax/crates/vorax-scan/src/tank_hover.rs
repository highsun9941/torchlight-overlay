//! Read the player's enlarged monster tooltip, never the animated monster in a tank.
use crate::hover::{tooltip, Tooltip};
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

pub fn recognize(img: &Img, read: &mut dyn FnMut(&Img)->String) -> Result<TankHoverRead,TankHoverFailure> {
    let mut texts = Vec::new();
    let Some(tip) = tooltip(img) else { return Err(TankHoverFailure { stage:"tooltip_missing", texts }); };
    let mut get = |area| { let s = read(&text_crop(img,&tip,area)); texts.push(s.clone()); s };
    // A potion footer must never qualify as a monster tooltip.
    let footer = get(Rect::new(0.15,0.887,0.70,0.073));
    let Some(rarity) = rarity(&footer) else { return Err(TankHoverFailure { stage:"monster_footer", texts }); };
    let name = get(Rect::new(0.12,0.395,0.76,0.075));
    let name: String = name.chars().filter(|c| crate::korean::hangul(*c) || c.is_ascii_alphabetic()).collect();
    if name.chars().count() < 2 { return Err(TankHoverFailure { stage:"monster_name", texts }); }
    let total = positive_number(&get(Rect::new(0.24,0.604,0.52,0.083)));
    let activity = positive_number(&get(Rect::new(0.09,0.807,0.23,0.062)));
    let quantity = positive_number(&get(Rect::new(0.68,0.807,0.23,0.062)));
    let (Some(total),Some(activity),Some(quantity)) = (total,activity,quantity) else { return Err(TankHoverFailure { stage:"numbers", texts }); };
    if activity>1_000_000 || quantity>1_000_000 || activity.checked_mul(quantity)!=Some(total) {
        return Err(TankHoverFailure { stage:"product", texts });
    }
    Ok(TankHoverRead { name, rarity, total, activity, quantity, tooltip:tip })
}

/// All occupied tanks participate, including confirmed ones. A tooltip from another tank must
/// not fill the only failed tank merely because the guide currently asks for that tank.
pub fn target(read: &TankHoverRead, r: &Reading, pointer: Option<usize>) -> Option<usize> {
    let candidates: Vec<_> = r.state.tanks.iter().enumerate().filter_map(|(i,m)| {
        let m = m.as_ref()?;
        let sure = |part: &str| r.fields.get(&format!("tank{i}.{part}")).is_some_and(|f|f.sure);
        if !sure("rarity") || !sure("archetype") || m.rarity != read.rarity { return None; }
        let total = r.debug.as_ref()?.tanks.get(i)?.as_ref().and_then(|d|positive_number(&d.total))?;
        if total != read.total || (sure("activity") && m.activity != read.activity) || (sure("quantity") && m.quantity != read.quantity) { return None; }
        Some(i)
    }).collect();
    if let Some(i) = pointer { return candidates.contains(&i).then_some(i); }
    (candidates.len()==1).then(||candidates[0])
}

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
