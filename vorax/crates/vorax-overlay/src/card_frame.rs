//! Fit the visible gold card edges. Localized cards have different widths and spacing, so an OCR
//! crop is not a reliable rectangle to draw. Searches are bounded to the already detected hand.
use vorax_scan::{layout::{CardPos, Layout}, pixels::Img};

pub type CardFrame = (CardPos, f64, f64);

fn gold_pixel(img: &Img, x: f64, y: f64) -> bool {
    let (x, y) = (x.round() as isize, y.round() as isize);
    if x < 0 || y < 0 || x >= img.width as isize || y >= img.height as isize { return false; }
    let (r, g, b) = img.px(x as usize, y as usize);
    r >= 150 && g >= 85 && g <= 240 && b < 210 && r as i32 - g as i32 >= 15 && g as i32 - b as i32 >= 25
}

fn gold(img: &Img, x: f64, y: f64) -> bool {
    [(0.0, 0.0), (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)].iter().any(|(dx, dy)| gold_pixel(img, x + dx, y + dy))
}

fn line_peak(lo: f64, hi: f64, minimum: f64, score: impl Fn(f64) -> f64) -> Option<f64> {
    let values: Vec<_> = (lo.floor() as i32..=hi.ceil() as i32).map(|n| (n as f64, score(n as f64))).collect();
    let &(at, best) = values.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    if best < minimum { return None; }
    // The glowing edge is several pixels wide. Use the middle of the strongest local band.
    let near: Vec<_> = values.iter().filter(|(x, s)| (*x - at).abs() <= 5.0 && *s >= best - 0.04).collect();
    Some(near.iter().map(|(x, _)| *x).sum::<f64>() / near.len() as f64)
}

fn vertical(img: &Img, c: &CardPos, height: f64, lo: f64, hi: f64) -> Option<f64> {
    line_peak(lo, hi, 0.75, |x| {
        (0..40).filter(|i| {
            let y = c.cy + ((*i as f64 / 39.0) - 0.5) * height * 0.6;
            let px = x - c.angle.tan() * (y - c.cy);
            gold(img, px, y)
        }).count() as f64 / 40.0
    })
}

pub fn detect(img: &Img, l: &Layout, size: usize) -> Option<Vec<CardFrame>> {
    let cards = l.hand_at(size);
    let last = cards.last()?;
    let (base_w, base_h) = (230.0 * l.k, 340.0 * l.k);
    let left = vertical(img, last, base_h, last.cx - base_w * 0.9, last.cx - base_w * 0.1)?;
    let right = vertical(img, last, base_h, last.cx + base_w * 0.1, last.cx + base_w * 1.2)?;
    let width = (right - left) * last.angle.cos();
    if !(base_w * 0.75..=base_w * 1.65).contains(&width) { return None; }
    let mut out = vec![];
    for (j, c) in cards.iter().enumerate() {
        let lx = if j + 1 == cards.len() { left } else {
            vertical(img, c, base_h, c.cx - base_w * 0.9, c.cx - base_w * 0.1)?
        };
        let mid = lx + width / (2.0 * c.angle.cos());
        // The next card covers the rightmost part of every card except the last one.
        let visible = if j + 1 == cards.len() { 0.88 } else { 0.65 };
        let horizontal = |y: f64| (0..40).filter(|i| {
            let x = lx + width * (0.1 + *i as f64 / 39.0 * (visible - 0.1));
            gold(img, x, y + c.angle.tan() * (x - mid))
        }).count() as f64 / 40.0;
        let top = line_peak(c.cy - base_h * 0.9, c.cy - base_h * 0.25, 0.75, horizontal)?;
        // The pet portrait overlaps the lower edge. Both vertical edges and the top have already
        // passed the stronger threshold; allow that localized occlusion at the bottom only.
        let bottom = line_peak(c.cy + base_h * 0.25, c.cy + base_h * 0.7, 0.45, horizontal)?;
        let height = (bottom - top) * c.angle.cos();
        if !(base_h * 0.75..=base_h * 1.65).contains(&height) { return None; }
        let cy = (top + bottom) / 2.0;
        let cx = mid - c.angle.tan() * (cy - c.cy);
        if out.last().is_some_and(|(prev, _, _): &CardFrame| cx <= prev.cx + width * 0.4) { return None; }
        out.push((CardPos { cx, cy, angle: c.angle }, width, height));
    }
    let low = out.iter().map(|(_, _, h)| *h).fold(f64::INFINITY, f64::min);
    let high = out.iter().map(|(_, _, h)| *h).fold(0.0, f64::max);
    if high - low > base_h * 0.08 { return None; }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_outline_is_invented_on_a_blank_screen() {
        let img = Img::new(1920, 1080);
        assert!(detect(&img, &vorax_scan::layout::layout(1920, 1080), 3).is_none());
    }

    #[test]
    fn localized_card_edges_override_the_smaller_ocr_layout_at_two_resolutions() {
        use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform, FillRule};
        for scale in [1.0, 0.75] {
            let (w, h) = ((1920.0 * scale) as u32, (1080.0 * scale) as u32);
            let mut pm = Pixmap::new(w, h).unwrap();
            pm.fill(Color::from_rgba8(40, 20, 15, 255));
            let positions = [(886.0, 825.0, -0.0576), (1076.0, 820.0, 0.0), (1265.0, 825.0, 0.0506)];
            for (x, y, angle) in positions {
                let mut pb = PathBuilder::new();
                pb.push_rect(tiny_skia::Rect::from_xywh(-112.5, -151.0, 225.0, 302.0).unwrap());
                let path = pb.finish().unwrap();
                let transform = Transform::from_rotate((angle as f64).to_degrees() as f32).post_translate(x as f32, y as f32).post_scale(scale as f32, scale as f32);
                let mut p = Paint::default();
                p.set_color_rgba8(45, 25, 10, 255);
                pm.fill_path(&path, &p, FillRule::Winding, transform, None);
                p.set_color_rgba8(255, 190, 65, 255);
                pm.stroke_path(&path, &p, &Stroke { width: 4.0, ..Default::default() }, transform, None);
            }
            let img = Img { width: w as usize, height: h as usize, data: pm.data().to_vec() };
            let fitted = detect(&img, &vorax_scan::layout::layout(w as usize, h as usize), 3).expect("localized gold borders");
            for ((c, cw, ch), (x, y, _)) in fitted.iter().zip(positions) {
                assert!((c.cx - x * scale).abs() < 4.0, "x: {} vs {}", c.cx, x * scale);
                assert!((c.cy - y * scale).abs() < 4.0, "y: {} vs {}", c.cy, y * scale);
                assert!((*cw - 225.0 * scale).abs() < 4.0);
                assert!((*ch - 302.0 * scale).abs() < 4.0);
            }
        }
    }
}
