//! Compare named UI regions by shape, with hover motion and the left-hand tooltip excluded.
use crate::layout::{card_face, layout_at};
use crate::pixels::{gray, Img, Rect};

#[derive(Clone)]
struct Area { name: String, values: Vec<f32>, ignored: bool }
#[derive(Clone)]
struct Stamp { size: (usize, usize), ui: f64, present: usize, tooltip:bool, areas: Vec<Area>, tanks:Vec<Option<String>> }

#[derive(Clone, Debug, serde::Serialize)]
pub struct Difference {
    pub area:String,pub similarity:f64,pub mean_delta:f64,
    #[serde(skip_serializing_if="Option::is_none")] pub before:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] pub after:Option<String>,
}

impl Stamp {
    fn new(img: &Img, ui: f64, hovered: Option<usize>) -> Self {
        let l = layout_at(img.width, img.height, ui);
        let tooltip=crate::hover::tooltip(img).map(|t|t.rect);
        let moving=tooltip.is_some() || hovered.is_some();
        let mut areas = Vec::new();
        let mut add = |name: String, r: Rect, text: bool, ignored: bool| {
            let covered=tooltip.is_some_and(|t|r.x<t.x+t.w && r.x+r.w>t.x && r.y<t.y+t.h && r.y+r.h>t.y);
            areas.push(Area { name, values: sample(img, &r, text), ignored:ignored||covered });
        };
        add("round".into(), l.round, true, false);
        add("refreshes".into(), l.refresh, true, false);
        // Unused fan positions contain animated scenery on a three-card offer.
        let size = crate::cards::fan_size(img, &l);
        for (i, c) in l.hand_at(size).iter().enumerate() {
            // Enlarging one offer also slides its neighbours. Round, refreshes, validated tanks and
            // the instrument panel still detect actual actions while this temporary fan motion is masked.
            add(format!("hand{i}.title"), Rect::new(c.cx-100.0*l.k,c.cy-32.0*l.k,200.0*l.k,38.0*l.k), true, moving);
            add(format!("hand{i}.art"), Rect::new(c.cx-38.0*l.k,c.cy-145.0*l.k,76.0*l.k,100.0*l.k), false, moving);
        }
        for (i, r) in l.instrument_icon.iter().enumerate() { add(format!("instrument{i}"), *r, false, false); }
        let count=crate::cards::cards_present(img,&l);
        let present=if count<3 && tooltip.is_some() {3} else {count};
        let g=gray(img);
        let tanks=(0..6).map(|i| {
            let r=crate::adaptive::tank(img,&g,&l,i,crate::scan_data());
            r.monster.filter(|_|r.fields.get(&format!("tank{i}.activity")).is_some_and(|f|f.sure)
                && r.fields.get(&format!("tank{i}.quantity")).is_some_and(|f|f.sure))
                .and_then(|m|serde_json::to_string(&m).ok())
        }).collect();
        Self { size:(img.width,img.height),ui,present,tooltip:moving,areas,tanks }
    }
    fn differences(&self, other: &Self) -> Vec<Difference> {
        if self.size != other.size || self.ui != other.ui || self.areas.len()!=other.areas.len() {
            return vec![Difference { area:"capture.size_or_layout".into(),similarity:0.0,mean_delta:255.0,before:None,after:None }];
        }
        if self.present!=other.present {return vec![Difference{area:"hand.animation".into(),similarity:0.0,mean_delta:255.0,before:None,after:None}];}
        let mut diffs:Vec<Difference>=self.areas.iter().zip(&other.areas).filter(|(_,b)| !b.ignored && !(self.tooltip && !other.tooltip && b.name.starts_with("hand"))).filter_map(|(a,b)| {
            let delta=a.values.iter().zip(&b.values).map(|(x,y)|(x-y).abs() as f64).sum::<f64>()/a.values.len() as f64;
            let similarity=correlation(&a.values,&b.values);
            (similarity < 0.90 && delta > if b.name.starts_with("tank") || b.name=="round" || b.name=="refreshes" {0.8} else {3.0}).then(|| Difference {area:b.name.clone(),similarity,mean_delta:delta,before:None,after:None})
        }).collect();
        for (i,(before,after)) in self.tanks.iter().zip(&other.tanks).enumerate() {
            // Missing text is not proof that a monster vanished. Card/round changes still invalidate
            // the screen; only a newly verified or changed numeric state can veto advice by itself.
            if after.is_some() && before!=after {diffs.push(Difference{area:format!("tank{i}.verified_state"),similarity:0.0,mean_delta:0.0,before:before.clone(),after:after.clone()});}
        }
        diffs
    }
}

fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let mut best = -1.0f64;
    for (dx,dy) in [(0,0),(-1,0),(1,0),(0,-1),(0,1)] {
        let mut aa=0.0; let mut bb=0.0; let mut ab=0.0; let mut sa=0.0; let mut sb=0.0; let mut n=0.0;
        for y in 1..15 { for x in 1..47 { for c in 0..3 {
            let av=a[(y*48+x)*3+c] as f64;
            let bv=b[(((y as i32+dy) as usize)*48+(x as i32+dx) as usize)*3+c] as f64;
            sa+=av; sb+=bv; aa+=av*av; bb+=bv*bv; ab+=av*bv; n+=1.0;
        }}}
        let va=(aa-sa*sa/n).max(0.0); let vb=(bb-sb*sb/n).max(0.0);
        let score=if va*vb>1e-6 {(ab-sa*sb/n)/(va*vb).sqrt()} else if va<1e-6 && vb<1e-6 && (sa-sb).abs()/n<8.0 {1.0} else {0.0};
        best=best.max(score);
    }
    best.clamp(-1.0,1.0)
}

fn sample(img:&Img,r:&Rect,text:bool)->Vec<f32> {
    let mut out=Vec::with_capacity(48*16*3);
    for y in 0..16 { for x in 0..48 {
        let mut rgb=[0.0;3];
        for (dx,dy) in [(0.25,0.25),(0.75,0.25),(0.25,0.75),(0.75,0.75)] {
            let xx=(r.x+(x as f64+dx)*r.w/48.0).clamp(0.0,img.width.saturating_sub(1) as f64) as usize;
            let yy=(r.y+(y as f64+dy)*r.h/16.0).clamp(0.0,img.height.saturating_sub(1) as f64) as usize;
            let (rr,gg,bb)=img.px(xx,yy);
            let ink=rr.max(gg).max(bb)>80 && !(rr as i32>gg as i32+60 && rr as i32>bb as i32+50);
            for (v,c) in rgb.iter_mut().zip([rr,gg,bb]) { *v+=if text && !ink {0.0} else {c as f32/4.0}; }
        }
        out.extend(rgb);
    }} out
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Change { Unchanged, Changed, Waiting, Ready }
#[derive(Default)]
pub struct ScreenWatch { accepted:Option<Stamp>, pending:Option<(Stamp,f64)>, hovered:Option<usize>, had_hover:bool, pub last_difference:Vec<Difference> }
impl ScreenWatch {
    pub fn set_hover(&mut self, slot:Option<usize>) { self.hovered=slot; }
    pub fn observe(&mut self,img:&Img,ui:f64,now_ms:f64)->Change {
        let stamp=Stamp::new(img,ui,self.hovered);
        let leaving=self.had_hover && !stamp.tooltip;self.had_hover=stamp.tooltip;
        if let Some(old)=&mut self.accepted {
            // Leaving the popup restores the entire fan. Rebase only those pixels, once, so the
            // worker keeps confirmed names for its finishing read; genuine counters still compare.
            if leaving {
                for area in &mut old.areas {if area.name.starts_with("hand") {
                    if let Some(now)=stamp.areas.iter().find(|a|a.name==area.name) {*area=now.clone();}
                }}
                old.tooltip=false;
            }
        }
        if let Some((before,since))=&mut self.pending {
            let diff=before.differences(&stamp);
            if !diff.is_empty() { *before=stamp; *since=now_ms; self.last_difference=diff; }
            return if now_ms-*since>=350.0 {Change::Ready} else {Change::Waiting};
        }
        let diff=self.accepted.as_ref().map_or_else(||vec![Difference {area:"first-frame".into(),similarity:0.0,mean_delta:255.0,before:None,after:None}],|s|s.differences(&stamp));
        if diff.is_empty() {Change::Unchanged} else {self.last_difference=diff;self.pending=Some((stamp,now_ms));Change::Changed}
    }
    pub fn accept(&mut self,img:&Img,ui:f64) {
        let mut stamp=Stamp::new(img,ui,self.hovered);
        self.had_hover=stamp.tooltip;
        if let Some(old)=&self.accepted {
            for a in &mut stamp.areas { if a.ignored { if let Some(b)=old.areas.iter().find(|b|b.name==a.name) {a.values=b.values.clone();} } }
            for (new,before) in stamp.tanks.iter_mut().zip(&old.tanks) {if new.is_none() {*new=before.clone();}}
        }
        self.accepted=Some(stamp); self.pending=None;
    }
    pub fn compare(&self,img:&Img,ui:f64)->Vec<Difference> {
        self.accepted.as_ref().map_or_else(||vec![Difference {area:"no-reference".into(),similarity:0.0,mean_delta:255.0,before:None,after:None}],|s|s.differences(&Stamp::new(img,ui,self.hovered)))
    }
    pub fn differs(&self,img:&Img,ui:f64)->bool { !self.compare(img,ui).is_empty() }
    pub fn reset(&mut self)->bool {let had=self.accepted.is_some()||self.pending.is_some();*self=Self::default();had}
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
        for area in [l.round, l.refresh, l.instrument_icon[0]] {
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

    #[test]
    fn a_small_heading_suffix_change_is_not_diluted_by_unchanged_card_art() {
        let mut a = Img::new(960, 540);
        a.data.fill(40);
        let mut b = a.clone();
        let l = layout_at(960, 540, 1.0);
        fill(&mut b, Rect::new(l.cards[1].cx + 15.0 * l.k, l.cards[1].cy - 24.0 * l.k, 22.0 * l.k, 16.0 * l.k), 150);
        let mut watch = ScreenWatch::default();
        watch.accept(&a, 1.0);
        assert_eq!(watch.observe(&b, 1.0, 500.0), Change::Changed);
        let mut noise = a.clone();
        noise.data.fill(42);
        watch.accept(&a, 1.0);
        assert_eq!(watch.observe(&noise, 1.0, 1000.0), Change::Unchanged);
    }
    #[test]
    fn unused_fan_positions_and_hovered_cards_do_not_restart_ocr() {
        let a=Img::new(960,540);let l=layout_at(960,540,1.0);
        let mut scene=a.clone();
        fill(&mut scene,Rect::new(l.hand5[0].cx-30.0,370.0,50.0,90.0),180);
        let mut watch=ScreenWatch::default();watch.accept(&a,1.0);
        assert_eq!(watch.observe(&scene,1.0,500.0),Change::Unchanged);
        let mut hover=a.clone();fill(&mut hover,l.card_face[1],180);
        watch.set_hover(Some(1));
        assert_eq!(watch.observe(&hover,1.0,1000.0),Change::Unchanged);
        assert!(!watch.differs(&hover,1.0),"a hover animation cannot veto completed OCR");
        watch.accept(&hover,1.0);watch.set_hover(None);
        assert_eq!(watch.observe(&a,1.0,1500.0),Change::Unchanged,"keep the normal card reference while hovered");
    }

}
