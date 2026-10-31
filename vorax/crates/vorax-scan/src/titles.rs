//! The cards' printed names (src/solver/scan/titles.ts), read by a text reader (OCR) and matched to the known
//! names. A clearly read name outranks the picture match; when two names fit about as well, the picture picks
//! between them; when nothing is read, the picture's answer stands.

use crate::cards::read_at;
use crate::data::ScanData;
use crate::layout::{card_face, layout_at, CardPos};
use crate::pixels::{clamp_u8, dot, patch, resize, signature, Img, Rect};
use crate::reading::{FieldRead, Fields, Problem, Reading};
use std::collections::HashMap;

const MIN_Q: f64 = 6.0;
const GAP: f64 = 3.0;

fn letters(t: &str) -> Vec<char> {
    t.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || crate::korean::hangul(*c)).collect()
}

/// For each start of p, the fewest edits that match it somewhere inside t.
fn prefix_costs(p: &[char], t: &[char]) -> Vec<usize> {
    let mut prev = vec![0usize; t.len() + 1];
    let mut best = vec![0];
    for i in 1..=p.len() {
        let mut row = vec![i; t.len() + 1];
        for j in 1..=t.len() {
            row[j] = (prev[j] + 1).min(row[j - 1] + 1).min(prev[j - 1] + usize::from(p[i - 1] != t[j - 1]));
        }
        best.push(*row.iter().min().unwrap());
        prev = row;
    }
    best
}

/// How much of a name the text shows: its letters matched, less two per mistake, less a half per missing letter at its end.
pub fn name_fit(name: &str, text: &str) -> f64 {
    // A Hangul syllable represents more than a single Latin letter. Doubling both sides preserves edit
    // distances proportionally, allowing short exact Korean names without lowering Latin thresholds.
    if text.chars().any(crate::korean::hangul) {
        let local = crate::korean::name(name);
        if !local.chars().any(crate::korean::hangul) { return f64::NEG_INFINITY; }
        let expand = |s: &str| letters(s).into_iter().flat_map(|c| [c, c]).collect::<Vec<_>>();
        let (p, t) = (expand(local), expand(text));
        let costs = prefix_costs(&p, &t);
        return (p.len().min(6)..=p.len()).map(|i| i as f64 - 2.0 * costs[i] as f64 - 0.5 * (p.len() - i) as f64).fold(f64::NEG_INFINITY, f64::max);
    }
    let p = letters(name);
    let t = letters(text);
    if t.is_empty() {
        return f64::NEG_INFINITY;
    }
    let c = prefix_costs(&p, &t);
    let mut q = f64::NEG_INFINITY;
    for i in p.len().min(6)..=p.len() {
        q = q.max(i as f64 - 2.0 * c[i] as f64 - 0.5 * (p.len() - i) as f64);
    }
    q
}

const TITLE_AREA: Rect = Rect::new(-140.0, -110.0, 280.0, 150.0);
const ZOOM: f64 = 2.0;
const BLUR: usize = 8;

/// A compact, high-confidence title-shape supplement for the reported Korean font. It can only
/// disambiguate a name already supported by the card art; unknown titles continue through OCR.
fn localized_refs() -> &'static std::collections::BTreeMap<String, Vec<Vec<f64>>> {
    use std::sync::OnceLock;
    static TITLES: OnceLock<std::collections::BTreeMap<String, Vec<Vec<f64>>>> = OnceLock::new();
    TITLES.get_or_init(|| serde_json::from_str(include_str!("../../../data/scan/korean-titles.json")).expect("Korean title samples"))
}

pub fn localized_hand_geometry(img: &Img, l: &crate::layout::Layout) -> bool {
    let candidates: Vec<_> = localized_refs().keys().map(|name| (name.clone(), 1.0)).collect();
    l.cards.iter().filter(|card| localized_title(img, card, l.k, &candidates).is_some()).take(2).count() >= 2
}

pub fn localized_title(img: &Img, c: &CardPos, k: f64, candidates: &[(String, f64)]) -> Option<(String, f64)> {
    let refs = localized_refs();
    if !candidates.iter().any(|(name, score)| *score >= ART_SUPPORT && refs.contains_key(name)) { return None; }
    let mut scores: std::collections::BTreeMap<&String, f64> = std::collections::BTreeMap::new();
    for (dx, dy) in [(0.0, 0.0), (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
        let mut at = *c;
        at.cx += dx * k;
        at.cy += dy * k;
        let crop = title_crop(img, &at, k);
        let value = signature(&crop, Rect::new(30.0, 160.0, 500.0, 96.0), 64, 16);
        for (name, samples) in refs {
            let score = samples.iter().map(|v| dot(v, &value)).fold(f64::NEG_INFINITY, f64::max);
            let old = scores.entry(name).or_insert(f64::NEG_INFINITY);
            *old = old.max(score);
        }
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (name, score) = ranked.first()?;
    let second = ranked.get(1).map_or(0.0, |x| x.1);
    (*score >= 0.94 && *score - second >= 0.06
        && candidates.iter().any(|(n, s)| n == *name && *s >= ART_SUPPORT))
        .then(|| ((*name).clone(), *score))
}

const ART_SUPPORT: f64 = 0.92;

/// A card's title area, straightened and enlarged, as dark text on white.
pub fn title_crop(img: &Img, c: &CardPos, k: f64) -> Img {
    let r = Rect::new(TITLE_AREA.x * ZOOM, TITLE_AREA.y * ZOOM, TITLE_AREA.w * ZOOM, TITLE_AREA.h * ZOOM);
    let p = patch(img, c.cx, c.cy, c.angle, r, k / ZOOM);
    contrast(&p)
}

/// Read the heading alone: an icon or the brighter description must not determine its contrast or OCR lines.
/// White padding helps Windows OCR keep the clipped right edge and short Hangul names on one line.
fn legacy_heading_crop(img: &Img, c: &CardPos, k: f64) -> Img {
    let r = Rect::new(-216.0, -76.0, 432.0, 88.0);
    let p = contrast(&patch(img, c.cx, c.cy, c.angle, r, k / ZOOM));
    let mut out = Img::new(p.width + 32, p.height + 32);
    out.data.fill(255);
    for y in 0..p.height {
        let at = ((y + 16) * out.width + 16) * 4;
        out.data[at..at + p.width * 4].copy_from_slice(&p.data[y * p.width * 4..(y + 1) * p.width * 4]);
    }
    out
}

fn filter_line(p:&[u8],w:usize,h:usize,horizontal:bool,maximum:bool)->Vec<u8> {
    let mut out=vec![0;w*h];
    for y in 0..h {for x in 0..w {
        let at=if horizontal{x}else{y};let bound=if horizontal{w}else{h};
        let mut value:u8=if maximum{0}else{255};
        for q in at.saturating_sub(8)..=(at+8).min(bound-1) {
            let v=p[if horizontal{y*w+q}else{q*w+x}];
            value=if maximum{value.max(v)}else{value.min(v)};
        }
        out[y*w+x]=value;
    }}out
}

fn ink_threshold(p:&[u8])->u8 {
    let mut hist=[0.0;256];for &v in p {hist[v as usize]+=1.0;}
    let total: f64=hist.iter().enumerate().map(|(i,n)|i as f64*n).sum();
    let (mut count,mut sum,mut best,mut at)=(0.0,0.0,0.0,0);
    for (i,n) in hist.iter().enumerate() {
        count+=n;sum+=i as f64*n;let rest=p.len() as f64-count;
        if count==0.0 || rest==0.0 {continue;}
        let delta=sum/count-(total-sum)/rest;let score=count*rest*delta*delta;
        if score>best {best=score;at=i;}
    }(at as f64*0.7).max(8.0) as u8
}

/// Locate a text row on each captured frame. Hover raises a card and slides its neighbours, so the
/// old fixed heading can land on a description. Opening removes the smooth background without
/// subtracting the dense middle strokes of Hangul as the old local mean did.
pub fn located_heading(img:&Img,c:&CardPos,k:f64,enlarged:bool)->(Img,Rect) {
    let r=if enlarged{Rect::new(-216.0,-100.0,432.0,88.0)}else{Rect::new(-320.0,-210.0,640.0,340.0)};
    let p=patch(img,c.cx,c.cy,c.angle,r,k/2.0);let (w,h)=(p.width,p.height);
    let gray:Vec<u8>=p.data.chunks_exact(4).map(|v|v[0].min(v[1])).collect();
    let opened=filter_line(&filter_line(&filter_line(&filter_line(&gray,w,h,true,false),w,h,false,false),w,h,true,true),w,h,false,true);
    let lift:Vec<u8>=gray.iter().zip(opened).map(|(v,b)|v.saturating_sub(b)).collect();
    let threshold=ink_threshold(&lift);
    let mut rows=Vec::new();let (mut start,mut last,mut active)=(0,0,false);
    for y in 0..h {
        // Rim pixels at either side must not join all text rows into one component.
        let hits=(w/5..w*4/5).filter(|&x|lift[y*w+x]>=threshold).count();
        if (hits as f64)<(w as f64*0.025).max(10.0) {continue;}
        if !active || y-last>4 {if active {rows.push((start,last+1));} start=y;active=true;}
        last=y;
    }
    if active {rows.push((start,last+1));}
    let target=if enlarged{-48.0}else{-80.0}-r.y;
    rows.sort_by(|(a,b),(aa,bb)| (((a+b) as f64/2.0-target).abs()).total_cmp(&(((aa+bb) as f64/2.0-target).abs())));
    let mut found=None;
    for (top,bottom) in rows {
        let center=r.y+(top+bottom) as f64/2.0;
        if !(8..=70).contains(&(bottom-top)) || (!enlarged && !(-150.0..=70.0).contains(&center)) {continue;}
        let (top,bottom)=(top.saturating_sub(4),(bottom+4).min(h));
        let mut columns=Vec::new();let (mut start,mut last,mut active)=(0,0,false);
        for x in 0..w {
            if !(top..bottom).any(|y|lift[y*w+x]>=threshold) {continue;}
            if !active || (x-last) as f64>((bottom-top) as f64*0.9).max(6.0) {
                if active {columns.push((start,last+1));}start=x;active=true;
            }
            last=x;
        }
        if active {columns.push((start,last+1));}
        let word=columns.into_iter().filter(|(a,b)| {
            if ((b-a) as f64)<(bottom-top) as f64*1.6 {return false;}
            let (mut groups,mut previous,mut pixels)=(0,false,0usize);
            for x in *a..*b {
                let count=(top..bottom).filter(|&y|lift[y*w+x]>=threshold).count();
                let current=count>0;if current && !previous {groups+=1;}previous=current;pixels+=count;
            }
            groups>=3 && pixels as f64/((b-a)*(bottom-top)) as f64>=0.06
        }).min_by_key(|(a,b)|(a+b).abs_diff(w));
        if let Some((left,right))=word {found=Some((left.saturating_sub(12),top,(right+12).min(w),bottom));break;}
    }
    let Some((left,top,right,bottom))=found else {return (legacy_heading_crop(img,c,k),Rect::new(c.cx-108.0*k,c.cy-38.0*k,216.0*k,44.0*k));};
    let mut values=Vec::new();for y in top..bottom {values.extend_from_slice(&lift[y*w+left..y*w+right]);}
    let mut ranked=values.clone();ranked.sort_unstable();let high=ranked[ranked.len()*95/100].max(16) as f64;
    let mut text=Img::new(right-left,bottom-top);
    for (i,&v) in values.iter().enumerate() {
        let pixel=if (v as f64)<threshold as f64*0.5 {255}else{clamp_u8(255.0-v as f64*400.0/high)};
        text.data[i*4..i*4+4].copy_from_slice(&[pixel,pixel,pixel,255]);
    }
    let scale=(432.0/text.width as f64).min(88.0/text.height as f64);
    let text=resize(&text,(text.width as f64*scale).round().max(1.0) as usize,(text.height as f64*scale).round().max(1.0) as usize);
    let mut out=Img::new(464,120);out.data.fill(255);
    let (ox,oy)=((out.width-text.width)/2,(out.height-text.height)/2);
    for y in 0..text.height {
        let to=((oy+y)*out.width+ox)*4;out.data[to..to+text.width*4].copy_from_slice(&text.data[y*text.width*4..(y+1)*text.width*4]);
    }
    let local=Rect::new((r.x+left as f64)*k/2.0,(r.y+top as f64)*k/2.0,(right-left) as f64*k/2.0,(bottom-top) as f64*k/2.0);
    (out,Rect::new(c.cx+local.x*c.angle.cos()-local.y*c.angle.sin(),c.cy+local.x*c.angle.sin()+local.y*c.angle.cos(),local.w,local.h))
}

pub fn heading_crop(img:&Img,c:&CardPos,k:f64)->Img {located_heading(img,c,k,false).0}

fn contrast(p: &Img) -> Img {
    let (w, h) = (p.width, p.height);
    let g: Vec<f32> = (0..w * h).map(|i| p.data[i * 4].max(p.data[i * 4 + 1]).max(p.data[i * 4 + 2]) as f32).collect();
    let mut sum = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            sum[(y + 1) * (w + 1) + x + 1] = g[y * w + x] as f64 + sum[y * (w + 1) + x + 1] + sum[(y + 1) * (w + 1) + x] - sum[y * (w + 1) + x];
        }
    }
    let mut lift = vec![0f32; w * h];
    let mut top = 1.0f64;
    for y in 0..h {
        for x in 0..w {
            let (x0, x1, y0, y1) = (x.saturating_sub(BLUR), w.min(x + BLUR + 1), y.saturating_sub(BLUR), h.min(y + BLUR + 1));
            let mean = (sum[y1 * (w + 1) + x1] - sum[y0 * (w + 1) + x1] - sum[y1 * (w + 1) + x0] + sum[y0 * (w + 1) + x0]) / ((x1 - x0) * (y1 - y0)) as f64;
            lift[y * w + x] = (g[y * w + x] as f64 - mean).max(0.0) as f32;
            top = top.max(lift[y * w + x] as f64);
        }
    }
    let mut out = Img::new(w, h);
    for i in 0..w * h {
        let v = clamp_u8(255.0 - (lift[i] as f64 / top * 400.0).min(255.0));
        out.data[i * 4] = v;
        out.data[i * 4 + 1] = v;
        out.data[i * 4 + 2] = v;
        out.data[i * 4 + 3] = 255;
    }
    out
}

/// Gold Korean headings have little red-channel contrast against the brown card. The blue channel
/// separates the heading from that background without sharpening away thin Hangul strokes.
pub fn title_crop_gold(img: &Img, c: &CardPos, k: f64, threshold: u8) -> Img {
    let r = Rect::new(TITLE_AREA.x * ZOOM, TITLE_AREA.y * ZOOM, TITLE_AREA.w * ZOOM, TITLE_AREA.h * ZOOM);
    let mut out = patch(img, c.cx, c.cy, c.angle, r, k / ZOOM);
    for p in out.data.chunks_exact_mut(4) {
        let v = if p[2] >= threshold { 0 } else { 255 };
        p[0] = v; p[1] = v; p[2] = v; p[3] = 255;
    }
    out
}

fn names_for<'a>(data: &'a ScanData, phase: &str) -> &'a [String] {
    match phase {
        "potion" => &data.names.potion,
        "instrument" => &data.names.instrument,
        _ => &data.names.protocol,
    }
}

fn ranked(data: &ScanData, phase: &str, text: &str) -> Vec<(String, f64)> {
    let mut r: Vec<(String, f64)> = names_for(data, phase).iter().map(|n| (n.clone(), name_fit(n, text))).collect();
    r.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    r
}

fn settled(r: &[(String, f64)]) -> bool {
    r[0].1 >= MIN_Q && r[0].1 - r.get(1).map_or(f64::NEG_INFINITY, |x| x.1) >= GAP
}

/// Some of a hand's cards matched by their pictures: per position, the best few cards, its field, and whether
/// no picture was close enough.
#[derive(Clone, Debug)]
pub struct PictureRead {
    pub ranks: Vec<(String, f64)>,
    pub field: Option<FieldRead>,
    pub unknown: bool,
}

/// The pictures of the given card positions, read as the reading's own would be.
/// `hand_dy`: the reading's hand_dy (0 where the layout puts the hand); `ui`: its interface scale.
pub fn pictures(img: &Img, data: &ScanData, phase: &str, size: usize, slots: &[usize], hand_dy: f64, ui: f64) -> HashMap<usize, PictureRead> {
    let l = layout_at(img.width, img.height, ui).shifted_hand(hand_dy);
    let r = read_at(img, &l, size, phase, data, Some(slots));
    slots
        .iter()
        .map(|&j| {
            let key = format!("hand{j}");
            let unknown = r.problems.iter().any(|p| matches!(p, Problem::UnknownArt { field, .. } if *field == key));
            (j, PictureRead { ranks: r.ranks[j].clone(), field: r.fields.get(&key).cloned(), unknown })
        })
        .collect()
}

/// Production fallback for translated cards; original `pictures` stays available to the trace reader.
pub fn pictures_adaptive(img: &Img, data: &ScanData, phase: &str, size: usize, slots: &[usize], hand_dy: f64, ui: f64) -> HashMap<usize, PictureRead> {
    let l = layout_at(img.width, img.height, ui).shifted_hand(hand_dy);
    let read = crate::cards::read_at_art(img, &l, size, phase, data, Some(slots));
    slots.iter().map(|&j| {
        let key = format!("hand{j}");
        let unknown = read.problems.iter().any(|p| matches!(p, Problem::UnknownArt { field, .. } if *field == key));
        (j, PictureRead { ranks: read.ranks[j].clone(), field: read.fields.get(&key).cloned(), unknown })
    }).collect()
}

const PHASES: [&str; 3] = ["potion", "instrument", "protocol"];

/// The reading with each card's printed name added. `read_text` reads a title crop (OCR); `pictures` matches
/// the cards whose names can't be settled, when the reading left them out.
pub fn read_titles(img: &Img, reading: Reading, data: &ScanData, read_text: &mut dyn FnMut(&Img) -> String, pictures_of: Option<&dyn Fn(&str, usize, &[usize]) -> HashMap<usize, PictureRead>>) -> Reading {
    read_titles_styled(img, reading, data, read_text, pictures_of, false)
}

/// Korean's gold headings are fastest and clearest in the blue-channel mask. The original adaptive
/// contrast remains the fallback, and the default entry point preserves the upstream reading contract.
pub fn read_titles_styled(img: &Img, reading: Reading, data: &ScanData, read_text: &mut dyn FnMut(&Img) -> String, pictures_of: Option<&dyn Fn(&str, usize, &[usize]) -> HashMap<usize, PictureRead>>, gold_first: bool) -> Reading {
    read_titles_styled_known(img,reading,data,read_text,pictures_of,gold_first,&Default::default())
}

/// Names already confirmed on this offer are supplied explicitly, without inventing OCR scores.
pub fn read_titles_styled_known(img: &Img, reading: Reading, data: &ScanData, read_text: &mut dyn FnMut(&Img) -> String, pictures_of: Option<&dyn Fn(&str, usize, &[usize]) -> HashMap<usize, PictureRead>>, gold_first: bool, known: &std::collections::BTreeMap<usize,String>) -> Reading {
    let read = reading.hand_ranks.as_ref().map_or(0, |r| r.len());
    if (read == 0 && reading.hand_pending != Some(true)) || reading.has(|p| matches!(p, Problem::NotFound | Problem::Animating)) {
        return reading;
    }
    let l = layout_at(img.width, img.height, reading.ui()).shifted_hand(reading.hand_dy.unwrap_or(0.0));
    let header = match reading.state.phase {
        vorax::advisor::ScreenPhase::Potion => "potion",
        vorax::advisor::ScreenPhase::Instrument => "instrument",
        vorax::advisor::ScreenPhase::Protocol => "protocol",
    };
    let phases: Vec<&str> = if reading.fields.get("phase").is_some_and(|f| f.sure) { vec![header] } else { PHASES.to_vec() };
    let size = if read > 0 { read } else { reading.hand_size.unwrap_or(3) };
    let texts: Vec<String> = l.hand_at(size).iter().enumerate().map(|(j, c)| {
        if let Some(name) = known.get(&j) { return if gold_first {crate::korean::name(name).to_string()} else {name.clone()}; }
        if gold_first {
            if let Some(name) = reading.fields.get(&format!("hand{j}"))
                .filter(|f| f.sure && f.second.is_none() && f.score.is_some_and(|s| s >= 0.94))
                .and_then(|f| f.guess.as_deref()) {
                return crate::korean::name(name).to_string();
            }
            let heading = read_text(&heading_crop(img, c, l.k));
            if phases.iter().any(|phase| settled(&ranked(data, phase, &heading))) { return heading; }
        }
        let text = read_text(&if gold_first { title_crop_gold(img, c, l.k, 70) } else { title_crop(img, c, l.k) });
        if (gold_first || text.chars().any(crate::korean::hangul)) && !phases.iter().any(|phase| settled(&ranked(data, phase, &text))) {
            let other = read_text(&if gold_first { title_crop(img, c, l.k) } else { title_crop_gold(img, c, l.k, 70) });
            if phases.iter().any(|phase| settled(&ranked(data, phase, &other))) { return other; }
        }
        if std::env::var("VORAX_OCR_MODE").is_ok_and(|v| v == "accurate")
            && !phases.iter().any(|phase| settled(&ranked(data, phase, &text))) {
            let crop = title_crop(img, c, l.k);
            for scale in [1.5, 2.0] {
                let enlarged = resize(&crop, (crop.width as f64 * scale) as usize, (crop.height as f64 * scale) as usize);
                let candidate = read_text(&enlarged);
                if phases.iter().any(|phase| settled(&ranked(data, phase, &candidate))) { return candidate; }
            }
            for threshold in [50, 90] {
                let candidate = read_text(&title_crop_gold(img, c, l.k, threshold));
                if phases.iter().any(|phase| settled(&ranked(data, phase, &candidate))) { return candidate; }
            }
        }
        text
    }).collect();
    let mut best: Option<(&str, Vec<Vec<(String, f64)>>, f64)> = None;
    for phase in phases {
        let slots: Vec<Vec<(String, f64)>> = texts.iter().map(|t| ranked(data, phase, t)).collect();
        let fit = slots.iter().fold(0.0, |s, r| s + r[0].1.max(0.0));
        if best.as_ref().is_none_or(|b| fit > b.2) {
            best = Some((phase, slots, fit));
        }
    }
    let (phase, slots, _) = best.unwrap();
    let pos = l.hand_at(size);

    let need: Vec<usize> = slots.iter().enumerate().filter(|(_, r)| !settled(r)).map(|(j, _)| j).collect();
    let from_reading = read == size && phase == header;
    let pics: HashMap<usize, PictureRead> = if from_reading {
        need.iter()
            .map(|&j| {
                let key = format!("hand{j}");
                let unknown = reading.problems.iter().any(|p| matches!(p, Problem::UnknownArt { field, .. } if *field == key));
                (j, PictureRead { ranks: reading.hand_ranks.as_ref().unwrap()[j].clone(), field: reading.fields.get(&key).cloned(), unknown })
            })
            .collect()
    } else if !need.is_empty() {
        pictures_of.map(|f| f(phase, size, &need)).unwrap_or_default()
    } else {
        HashMap::new()
    };

    let mut fields: Fields = reading.fields.iter().filter(|(k, _)| !(k.len() == 5 && k.starts_with("hand") && k.as_bytes()[4].is_ascii_digit())).map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut hand = Vec::new();
    let mut unknown = Vec::new();
    for (j, r) in slots.iter().enumerate() {
        let (top, second) = (&r[0], r.get(1));
        let key = format!("hand{j}");
        let pic = pics.get(&j);
        let mut name: Option<String> = None;
        if settled(r) {
            name = Some(top.0.clone());
            fields.insert(key, FieldRead { sure: true, guess: name.clone(), score: Some(top.1), second: second.map(|s| s.1) });
        } else if top.1 >= MIN_Q {
            let close: Vec<&String> = r.iter().filter(|(_, q)| *q >= top.1 - GAP).map(|(n, _)| n).collect();
            let n = pic.and_then(|p| p.ranks.iter().find(|(n, _)| close.contains(&n)).map(|x| x.0.clone())).unwrap_or_else(|| top.0.clone());
            let sure = pic.is_some_and(|p| p.ranks.first().is_some_and(|x| x.0 == n) && p.field.as_ref().is_some_and(|f| f.sure));
            fields.insert(key, FieldRead { sure, guess: Some(n.clone()), score: Some(top.1), second: second.map(|s| s.1) });
            name = Some(n);
        } else if let Some(p) = pic.filter(|p| !p.unknown && p.field.as_ref().is_some_and(|f| f.guess.is_some())) {
            name = p.field.as_ref().unwrap().guess.clone();
            fields.insert(key, p.field.clone().unwrap());
        } else {
            fields.insert(key, pic.and_then(|p| p.field.clone()).unwrap_or(FieldRead::sure(false)));
        }
        match name {
            Some(n) => hand.push(n),
            None => unknown.push(j),
        }
    }
    let mut problems: Vec<Problem> = reading.problems.iter().filter(|p| !matches!(p, Problem::UnknownArt { .. })).cloned().collect();
    for &j in &unknown {
        let text = texts[j].trim();
        problems.push(Problem::UnknownArt { field: format!("hand{j}"), crop: card_face(&pos[j], l.k), hand: (size == 5).then_some(5), text: (!text.is_empty()).then(|| text.to_string()) });
    }
    if phase != header {
        fields.insert("phase".into(), FieldRead { sure: unknown.is_empty(), guess: Some(phase.into()), ..Default::default() });
    }
    let mut state = reading.state.clone();
    state.phase = match phase {
        "potion" => vorax::advisor::ScreenPhase::Potion,
        "instrument" => vorax::advisor::ScreenPhase::Instrument,
        _ => vorax::advisor::ScreenPhase::Protocol,
    };
    state.hand = hand;
    // a refresh count read under the header's kind of pick belongs to the one the names show
    if phase != header && phase != "protocol" {
        let left = if header == "potion" { reading.state.potion_refreshes } else { reading.state.instrument_refreshes };
        if phase == "potion" {
            state.potion_refreshes = left;
        } else {
            state.instrument_refreshes = left;
        }
    }
    Reading { state, fields, problems, hand_ranks: Some((0..pos.len()).map(|j| pics.get(&j).map(|p| p.ranks.clone()).unwrap_or_default()).collect()), hand_pending: None, ..reading }
}
