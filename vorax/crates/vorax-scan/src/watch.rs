//! Cheap visual change detection, independent of the game's log. Only static UI areas are sampled;
//! flames and moving monsters do not keep restarting OCR. Time is supplied by the caller for replay tests.
use crate::layout::{card_face, layout_at};
use crate::pixels::{Img, Rect};

#[derive(Clone)]
struct Stamp {
    size: (usize, usize),
    ui: f64,
    areas: Vec<Vec<f32>>,
}

impl Stamp {
    fn new(img: &Img, ui: f64) -> Self {
        let l = layout_at(img.width, img.height, ui);
        let mut areas = vec![l.round, l.refresh];
        areas.extend(l.cards.iter().map(|c| card_face(c, l.k)));
        areas.extend([card_face(&l.hand5[0], l.k), card_face(&l.hand5[4], l.k)]);
        areas.extend(l.total_band);
        areas.extend(l.aq_band);
        areas.extend(l.instrument_icon);
        Self { size: (img.width, img.height), ui, areas: areas.iter().map(|r| sample(img, r)).collect() }
    }

    fn differs(&self, other: &Self, threshold: f32) -> bool {
        self.size != other.size || self.ui != other.ui || self.areas.iter().zip(&other.areas).any(|(a, b)| {
            a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len() as f32 > threshold
        })
    }
}

/// Small colour thumbnails retain a changed digit or swapped potion without scanning full images.
fn sample(img: &Img, r: &Rect) -> Vec<f32> {
    let mut out = Vec::with_capacity(24 * 16 * 3);
    for y in 0..16 {
        for x in 0..24 {
            let mut rgb = [0.0; 3];
            for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let xx = (r.x + (x as f64 + dx) * r.w / 24.0).max(0.0).min(img.width.saturating_sub(1) as f64) as usize;
                let yy = (r.y + (y as f64 + dy) * r.h / 16.0).max(0.0).min(img.height.saturating_sub(1) as f64) as usize;
                let (rr, gg, bb) = img.px(xx, yy);
                for (v, c) in rgb.iter_mut().zip([rr, gg, bb]) { *v += c as f32 / 4.0; }
            }
            out.extend(rgb);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change { Unchanged, Changed, Waiting, Ready }

#[derive(Default)]
pub struct ScreenWatch {
    accepted: Option<Stamp>,
    pending: Option<(Stamp, f64)>,
}

impl ScreenWatch {
    /// Hide old advice on Changed; read on Ready, after at least two stable observations 350 ms apart.
    pub fn observe(&mut self, img: &Img, ui: f64, now_ms: f64) -> Change {
        let stamp = Stamp::new(img, ui);
        if let Some((previous, since)) = &mut self.pending {
            if previous.differs(&stamp, 1.5) { *previous = stamp; *since = now_ms; }
            return if now_ms - *since >= 350.0 { Change::Ready } else { Change::Waiting };
        }
        if self.accepted.as_ref().is_none_or(|s| s.differs(&stamp, 3.0)) {
            self.pending = Some((stamp, now_ms));
            Change::Changed
        } else { Change::Unchanged }
    }

    /// Remember the exact frame read, including failed reads, so an unchanged failure cannot spin OCR.
    pub fn accept(&mut self, img: &Img, ui: f64) {
        self.accepted = Some(Stamp::new(img, ui));
        self.pending = None;
    }

    /// A final capture before publishing advice catches a selection made while OCR was running.
    pub fn differs(&self, img: &Img, ui: f64) -> bool {
        self.accepted.as_ref().is_none_or(|s| s.differs(&Stamp::new(img, ui), 3.0))
    }

    pub fn reset(&mut self) -> bool {
        let had_frame = self.accepted.is_some() || self.pending.is_some();
        *self = Self::default();
        had_frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(img: &mut Img, r: Rect, value: u8) {
        for y in r.y.max(0.0) as usize..((r.y + r.h) as usize).min(img.height) {
            for x in r.x.max(0.0) as usize..((r.x + r.w) as usize).min(img.width) {
                img.data[(y * img.width + x) * 4..][..4].copy_from_slice(&[value, value, value, 255]);
            }
        }
    }

    #[test]
    fn transitions_without_log_events_hide_then_read_once_after_settling() {
        let a = Img::new(960, 540);
        let mut b = a.clone();
        fill(&mut b, layout_at(960, 540, 1.0).card_face[1], 140);
        let mut watch = ScreenWatch::default();
        watch.accept(&a, 1.0);
        assert_eq!(watch.observe(&a, 1.0, 0.0), Change::Unchanged);
        assert_eq!(watch.observe(&b, 1.0, 500.0), Change::Changed);
        assert!(watch.differs(&b, 1.0), "old advice must not be published over the new card");
        assert_eq!(watch.observe(&b, 1.0, 700.0), Change::Waiting);
        assert_eq!(watch.observe(&b, 1.0, 1000.0), Change::Ready);
        watch.accept(&b, 1.0);
        assert_eq!(watch.observe(&b, 1.0, 1500.0), Change::Unchanged);
        assert!(!watch.differs(&b, 1.0));
        assert_eq!(watch.observe(&a, 1.0, 2000.0), Change::Changed);
        assert_eq!(watch.observe(&b, 1.0, 2250.0), Change::Waiting);
        assert_eq!(watch.observe(&b, 1.0, 2600.0), Change::Ready, "returning to the old frame must restore cleared advice");
    }

    #[test]
    fn digits_rerolls_and_panel_changes_are_watched_but_scene_animation_is_not() {
        let a = Img::new(960, 540);
        let l = layout_at(960, 540, 1.0);
        for area in [l.round, l.refresh, l.total_band[1], l.aq_band[2], l.instrument_icon[0]] {
            let mut b = a.clone();
            fill(&mut b, area, 100);
            let mut watch = ScreenWatch::default();
            watch.accept(&a, 1.0);
            assert_eq!(watch.observe(&b, 1.0, 500.0), Change::Changed, "{area:?}");
        }
        let mut scene = a.clone();
        fill(&mut scene, Rect::new(380.0, 45.0, 90.0, 70.0), 255);
        let mut watch = ScreenWatch::default();
        watch.accept(&a, 1.0);
        assert_eq!(watch.observe(&scene, 1.0, 500.0), Change::Unchanged);
        assert!(watch.reset());
        assert_eq!(watch.observe(&scene, 1.0, 1000.0), Change::Changed);
    }
}
