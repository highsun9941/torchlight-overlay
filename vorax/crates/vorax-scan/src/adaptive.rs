//! Small, bounded searches around the measured layout. Only number pairs whose product agrees with
//! the independently printed total are accepted; the original reader remains the upstream contract.

use crate::data::ScanData;
use crate::layout::Layout;
use crate::pixels::{Img, Rect};
use crate::tanks::{read_tank, reconcile, stats, TankRead};
use crate::text::{read_text, TextKind, TextRead};
use indexmap::IndexMap;
use std::sync::OnceLock;

fn shifted(r: Rect, dy: f64) -> Rect { Rect::new(r.x, r.y + dy, r.w, r.h) }

fn tank_sure(r: &TankRead, i: usize) -> bool {
    r.fields.get(&format!("tank{i}.activity")).is_some_and(|f| f.sure)
        && r.fields.get(&format!("tank{i}.quantity")).is_some_and(|f| f.sure)
}

pub fn tank(img: &Img, g: &[f32], l: &Layout, i: usize, data: &ScanData) -> TankRead {
    let first = read_tank(img, g, l, i, data, true);
    if tank_sure(&first, i) { return first; }
    let refs = &data.digits["label"];
    // Physical pixels, nearest first: at small window sizes a single row from the preceding number
    // joins the multiplication sign to its neighbours. Moving the AQ band independently fixes that.
    let reach = (24.0 * l.k).ceil().clamp(4.0, 32.0) as i32;
    for step in 0..=reach * 2 {
        let offset = if step == 0 { 0 } else if step % 2 == 1 { (step + 1) / 2 } else { -step / 2 };
        let dy = offset as f64;
        let total = read_text(img, g, &shifted(l.total_band[i], dy), TextKind::Label, refs);
        let Some(total) = total.filter(|r| r.score >= 0.75) else { continue };
        for inset in [0.0, 2.0 * l.k, 4.0 * l.k] {
            let aq = read_text(img, g, &shifted(l.aq_band[i], dy + inset), TextKind::Label, refs);
            let Some(aq) = aq.filter(|r| r.score >= 0.75) else { continue };
            // The heart can add a leading glyph (545×36). Apply the same independently checked
            // reconciliation as read_tank before rejecting an otherwise clear shifted number pair.
            if !stats(Some(&total.text), Some(&aq.text)).2 && reconcile(&total.alts,&aq.alts).is_none() { continue; }
            let mut at = l.clone();
            at.name_band[i].y += dy;
            at.total_band[i].y += dy;
            at.aq_band[i].y += dy + inset;
            at.icon[i].y += dy;
            // Projection between 16:9 and ultrawide can move a tiny class icon horizontally too.
            at.icon[i].x -= 16.0 * l.k;
            at.icon[i].w += 32.0 * l.k;
            at.icon[i].y -= 8.0 * l.k;
            at.icon[i].h += 16.0 * l.k;
            let found = read_tank(img, g, &at, i, data, true);
            if tank_sure(&found, i) { return found; }
        }
    }
    first
}

fn valid_round(r: &TextRead, minimum: f64) -> bool {
    r.score >= minimum && r.text.parse::<i32>().is_ok_and(|n| (1..=13).contains(&n))
}

/// Extra glyph samples supplement the old font only after its read fails. Keeping them out of
/// ScanData preserves the recorded upstream reader and all of its exact floating-point scores.
fn round_refs(data: &ScanData) -> &'static IndexMap<String, Vec<Vec<f64>>> {
    static REFS: OnceLock<IndexMap<String, Vec<Vec<f64>>>> = OnceLock::new();
    REFS.get_or_init(|| {
        let mut refs = data.digits["round"].clone();
        let extra: IndexMap<String, Vec<Vec<f64>>> = serde_json::from_str(include_str!("../../../data/scan/korean-round.json")).expect("round glyph samples");
        for (digit, samples) in extra { refs.entry(digit).or_default().extend(samples); }
        refs
    })
}

pub fn round(img: &Img, g: &[f32], l: &Layout, data: &ScanData) -> Option<TextRead> {
    let first = read_text(img, g, &l.round, TextKind::Round, &data.digits["round"]);
    if first.as_ref().is_some_and(|r| valid_round(r, 0.75)) { return first; }
    for dy in [0.0, -2.0 * l.k, 2.0 * l.k, -4.0 * l.k, 4.0 * l.k] {
        if let Some(read) = read_text(img, g, &shifted(l.round, dy), TextKind::Round, round_refs(data)) {
            if valid_round(&read, 0.80) && read.score - read.second >= 0.06 { return Some(read); }
        }
    }
    first
}
