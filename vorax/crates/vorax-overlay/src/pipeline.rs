//! A frame of the game into advice and marks: read the board, follow the instruments through the game, ask the
//! advisor, lay out the marks for the screen the overlay covers. Shared by the live app and --preview.

use crate::draw::{plan_for, Plan};
use vorax::advisor::{advise_plan, Advice};
use vorax_scan::layout::layout_at;
use vorax_scan::live::carried_round;
use vorax_scan::pixels::Img;
use vorax_scan::reading::{Problem, Reading};
use vorax_scan::titles::pictures_adaptive as pictures;
#[cfg(test)]
use vorax_scan::titles::read_titles;
use vorax_scan::track::{track_instruments, InstrumentTracker};
use vorax_scan::window::{crop_to, windowed_game};
use vorax_scan::{scan, scan_data};

pub type TextReader<'a> = Option<&'a mut dyn FnMut(&Img) -> String>;

/// Futures per option the Solver plays for the overlay's advice. More futures make close calls less a matter of
/// luck in the Solver's own playouts. On the same 3,000 seeds with the full Solver (Oct 24, 2026): the linear board
/// value at 1024 futures reached 840k in 81.5% of games, the trained trees at 256 in 79.2% and at 512 in 81.0%;
/// the linear one is about five times faster to evaluate, so 1024 of its futures take less time than 256 with the
/// trees (the hardest recorded board: about 0.2 s on 12 threads).
pub const FUTURES: u32 = 1024;
pub fn futures() -> u32 {
    std::env::var("VORAX_FUTURES").ok().and_then(|s| s.parse().ok()).filter(|n| [256, 512, 1024].contains(n)).unwrap_or(FUTURES)
}

/// Unread fields can change both the legal targets and the ranking. Suppress advice until another read.
pub fn trustworthy(reading: &Reading) -> bool {
    !reading.has(|p| matches!(p, Problem::Unreadable { .. } | Problem::UnknownArt { .. } | Problem::UnknownName { .. }))
        && !reading.state.hand.is_empty()
        && !reading.fields.iter().any(|(name, field)| name.starts_with("hand") && name[4..].parse::<usize>().is_ok() && !field.sure)
        && (reading.state.phase == vorax::advisor::ScreenPhase::Protocol || reading.fields.get("refreshes").is_some_and(|f| f.sure))
}

pub enum Outcome {
    /// not the Surgical Preparation screen
    NotGame,
    Uncertain { reading: Box<Reading> },
    /// a card flying off as it's played: the last advice still stands
    Animating,
    /// the game closing after round 13's last pick (its end animation, the rewards): not a board
    Closing,
    /// advice for the board, with its marks
    Advice { reading: Box<Reading>, advice: Box<Advice>, plan: Plan, read_ms: f64, cached: bool },
}

pub struct Pipeline {
    last_advice: Option<(String, u32, Advice)>,
    card_frames: Option<Vec<crate::card_frame::CardFrame>>,
    pub tracker: InstrumentTracker,
    pub shelley: i32,
    /// instrument rerolls left, as last read on an instrument offer this game (a potion board doesn't show them)
    instrument_refreshes: Option<i32>,
    /// the last board advised on, with the size of its picture, for advising again when a setting changes (and
    /// for its round, when the next board's can't be read)
    last: Option<(Reading, usize, usize)>,
    last_current: bool,
    pending: Option<Reading>,
    zoom_board: String,
    zoom_names: std::collections::BTreeMap<usize, String>,
    zoom_candidate: Option<(usize, String, f64)>,
    zoom_dirty: bool,
    /// this game reached the Surgical Protocols (rounds 11-13): a board after them with no cards in hand is the game
    /// closing (its end animation, the rewards). Read as a board it gave six 1횞1 monsters in "round 1"; round 13's
    /// own board may not be read first (caught mid-animation, or its number read as 12)
    protocols: bool,
    /// the last read was the Vorax screen opened again on the game under way (Escape and back, to skip long
    /// animations), not a new game
    pub resumed: bool,
}

/// Whether the screen opening is the game under way going on rather than a new one, from the round of the board
/// before (`before`) and the round now read for sure (`now`). A new game starts in round 1, and none goes on past
/// round 13. Icons in the instrument panel prove nothing: a new game can start with an item from the map in it
/// (a boss drop), read as the nearest instrument; taken for the game under way, it stayed in as an instrument.
fn resumes(before: Option<i32>, now: Option<i32>) -> bool {
    match (before, now) {
        (Some(b), Some(n)) => n > 1 && b < 13 && n >= b,
        _ => false,
    }
}

/// Instrument rerolls a game starts with, by Intern Shelley level (src/data/shelley.ts).
pub fn shelley_rerolls(level: i32) -> i32 {
    [0, 1, 1, 2, 2, 2].get((level - 1).clamp(0, 5) as usize).copied().unwrap_or(0)
}

impl Pipeline {
    pub fn new(shelley: i32) -> Pipeline {
        Pipeline { last_advice: None, card_frames: None, tracker: InstrumentTracker::default(), shelley, instrument_refreshes: None, last: None, last_current: false, pending: None, zoom_board: String::new(), zoom_names: Default::default(), zoom_candidate: None, zoom_dirty: false, protocols: false, resumed: false }
    }

    pub fn ui(&self) -> f64 { self.last.as_ref().map_or(1.0, |(r, ..)| r.ui()) }

    /// Keep history for tracking rounds, but never re-display a plan after its screen changed.
    pub fn invalidate(&mut self) { self.last_current = false; }

    pub fn pending(&self) -> Option<&Reading> { self.pending.as_ref() }
    pub fn hover_dirty(&self) -> bool { self.zoom_dirty }
    pub fn cancel_hover_candidate(&mut self) { self.zoom_candidate=None; }
    pub fn reset_hover(&mut self) {
        self.pending=None; self.zoom_names.clear(); self.zoom_board.clear(); self.zoom_candidate=None; self.zoom_dirty=false;
    }
    pub fn requested_slots(&self) -> Vec<usize> {
        self.pending.as_ref().map_or_else(Vec::new, |r| r.blocking_fields().iter().filter_map(|f|f.strip_prefix("hand").and_then(|n|n.parse().ok())).collect())
    }
    pub fn hover_plan(&self) -> Option<Plan> {
        let r=self.pending.as_ref()?;
        let frame=r.frame.as_ref()?;
        let l=layout_at(frame.width,frame.height,r.ui()).shifted_hand(r.hand_dy.unwrap_or(0.0));
        let size=r.hand_ranks.as_ref().map_or(r.hand_size.unwrap_or(3),|h|h.len().max(3));
        let mut plan=Plan::default();
        for i in self.requested_slots() {if let Some(c)=l.hand_at(size).get(i) {plan.inspect.push((*c,230.0*l.k,340.0*l.k));}}
        (!plan.inspect.is_empty()).then_some(plan)
    }
    pub fn confirm_hover(&mut self, slot:usize, read:&vorax_scan::hover::HoverRead, now:f64) -> bool {
        if !self.requested_slots().contains(&slot) {return false;}
        if self.zoom_candidate.as_ref().is_some_and(|(i,n,t)| *i==slot && n==&read.name && now-*t>=350.0 && now-*t<=2500.0) {
            self.zoom_names.insert(slot,read.name.clone());self.zoom_candidate=None;self.zoom_dirty=true;
            if let Some(r)=&mut self.pending {
                let field=format!("hand{slot}");
                r.fields.insert(field.clone(),vorax_scan::reading::FieldRead {sure:true,guess:Some(read.name.clone()),score:Some(1.0),second:None});
                r.problems.retain(|p|!matches!(p,Problem::UnknownArt{field:f,..} if f==&field));
            }
            true
        } else {self.zoom_candidate=Some((slot,read.name.clone(),now));false}
    }
    fn apply_zoom(&mut self, reading:&mut Reading) {
        let mut state=reading.state.clone(); state.hand.clear();state.shelley=None;
        let key=serde_json::to_string(&state).unwrap_or_default();
        if key!=self.zoom_board {self.zoom_board=key;self.zoom_names.clear();self.zoom_candidate=None;self.zoom_dirty=false;}
        for (i,name) in &self.zoom_names {
            let field=format!("hand{i}");
            reading.fields.insert(field.clone(),vorax_scan::reading::FieldRead {sure:true,guess:Some(name.clone()),score:Some(1.0),second:None});
            reading.problems.retain(|p|!matches!(p,Problem::UnknownArt {field:f,..} if f==&field));
        }
        if !self.zoom_names.is_empty() {
            let size=reading.hand_ranks.as_ref().map_or(reading.hand_size.unwrap_or(3),|h|h.len().max(3));
            reading.state.hand=(0..size).filter_map(|i|reading.fields.get(&format!("hand{i}")).and_then(|f|f.guess.clone())).collect();
        }
        self.pending=Some(reading.clone());
    }

    /// The board's instrument rerolls: as read on an instrument offer, else carried from the last one this game,
    /// else what the Shelley level starts a game with. (Upstream counts them as 0 on a potion board.)
    fn fill_rerolls(&mut self, reading: &mut Reading) {
        match reading.state.phase {
            vorax::advisor::ScreenPhase::Instrument => {
                if reading.fields.get("refreshes").is_some_and(|f| f.sure) {
                    self.instrument_refreshes = reading.state.instrument_refreshes;
                } else if shelley_rerolls(self.shelley) == 0
                    && reading.fields.get("refreshes").is_some_and(|f| f.guess.is_none()) {
                    // With no pet rerolls the game omits this control entirely. An absent counter is
                    // a known zero here, while a visible but unreadable digit still blocks advice.
                    reading.state.instrument_refreshes = Some(0);
                    reading.fields.insert("refreshes".into(), vorax_scan::reading::FieldRead { sure: true, guess: Some("0".into()), ..Default::default() });
                    self.instrument_refreshes = Some(0);
                }
            }
            _ => {
                if reading.state.instrument_refreshes.is_none() {
                    reading.state.instrument_refreshes = Some(self.instrument_refreshes.unwrap_or(shelley_rerolls(self.shelley)));
                }
            }
        }
    }

    /// The last board advised again (the Shelley level changed): its marks, for a picture of this size.
    pub fn readvise(&mut self) -> Option<(Plan, usize, usize)> {
        if !self.last_current { return None; }
        let (mut reading, w, h) = self.last.clone()?;
        reading.state.shelley = Some(self.shelley);
        if reading.state.phase != vorax::advisor::ScreenPhase::Instrument {
            reading.state.instrument_refreshes = None;
        }
        self.fill_rerolls(&mut reading);
        if !trustworthy(&reading) { return None; }
        let (advice, _) = self.advise(&reading.state);
        let mut plan = plan_for(&advice, &reading.state, &reading, &layout_at(w, h, reading.ui()).shifted_hand(reading.hand_dy.unwrap_or(0.0)));
        self.align_card(&mut plan, &advice);
        Some((plan, w, h))
    }

    /// `frame`: the captured game window; `trigger`: "screen" when the log says a new game opened.
    pub fn run(&mut self, frame: &Img, trigger: Option<&str>, ocr: TextReader) -> Outcome {
        let t = std::time::Instant::now();
        self.invalidate();
        self.resumed = false;
        let data = scan_data();
        let img = match windowed_game(frame) {
            Some(a) => crop_to(frame, &a),
            None => frame.clone(),
        };
        // with a text reader: the cards by their printed names, by their pictures only where a name doesn't settle
        let mut raw = match ocr {
            Some(read) => {
                // A title shape is only authoritative when every offered card is independently
                // supported by its art. These calibrated Korean titles need no OS-dependent OCR.
                let localized = std::env::var("VORAX_LANGUAGE").map_or(true, |v| v == "ko");
                let pictured = localized.then(|| scan(&img, data, true));
                let verified = pictured.as_ref().is_some_and(|pictured| !pictured.state.hand.is_empty()
                        && !pictured.has(|p| matches!(p, Problem::UnknownArt { .. } | Problem::UnknownName { .. } | Problem::NotFound | Problem::Animating))
                        && pictured.state.hand.iter().enumerate().all(|(i, _)| {
                        pictured.fields.get(&format!("hand{i}")).is_some_and(|f| f.sure && f.second.is_none() && f.score.is_some_and(|s| s >= 0.94))
                    }));
                let mut result = if verified { pictured.unwrap() } else {
                    // Preserve independently verified slots even when another card still needs OCR.
                    let names = pictured.unwrap_or_else(|| scan(&img, data, false));
                    let (dy, ui) = (names.hand_dy.unwrap_or(0.0), names.ui());
                    let pics = |phase: &str, size: usize, slots: &[usize]| pictures(&img, data, phase, size, slots, dy, ui);
                    let named = vorax_scan::titles::read_titles_styled(&img, names, data, read, Some(&pics), std::env::var("VORAX_LANGUAGE").map_or(true, |v| v == "ko"));
                    if named.state.hand.is_empty() || named.has(|p| matches!(p, Problem::UnknownArt { .. } | Problem::UnknownName { .. })) {
                        let pictured = scan(&img, data, true);
                        if !pictured.state.hand.is_empty() && !pictured.has(|p| matches!(p, Problem::UnknownArt { .. } | Problem::UnknownName { .. } | Problem::NotFound | Problem::Animating)) {
                            pictured
                        } else { named }
                    } else { named }
                };
                if let Some(field) = result.fields.get("round").filter(|f| !f.sure) {
                    let text = read(&vorax_scan::text::round_ocr_crop(&img, &layout_at(img.width, img.height, result.ui())));
                    if let Some(n) = vorax_scan::text::ocr_round(&text, field.guess.as_deref(), field.score.unwrap_or(0.0)) {
                        result.state.round = Some(n);
                        result.fields.insert("round".into(), vorax_scan::reading::FieldRead { sure: true, guess: Some(n.to_string()), ..Default::default() });
                        result.problems.retain(|p| !matches!(p, Problem::Unreadable { field } if field == "round"));
                    }
                }
                result
            }
            None => scan(&img, data, true),
        };
        if !raw.has(|p|matches!(p,Problem::Animating)) && raw.state.hand.is_empty() && raw.hand_ranks.as_ref().is_none_or(|h|h.is_empty())
            && raw.hand_pending!=Some(true) && raw.fields.get("phase").is_none_or(|f|!f.sure)
            && raw.fields.get("round").is_none_or(|f|!f.sure) {
            self.reset_hover();return Outcome::NotGame;
        }
        if raw.has(|p| matches!(p, Problem::Animating)) {
            return Outcome::Animating;
        }
        if raw.has(|p| matches!(p, Problem::NotFound)) {
            self.reset_hover();
            return Outcome::NotGame;
        }
        // the screen opened again on the game under way (Escape and back skips long animations): what's known of it
        // (instruments and their rounds, rerolls) is kept
        let round_sure = raw.fields.get("round").is_some_and(|f| f.sure);
        let before = self.last.as_ref().and_then(|(r, ..)| r.state.round);
        self.resumed = trigger == Some("screen") && resumes(before, round_sure.then_some(raw.state.round.unwrap_or(1)));
        let trigger = if self.resumed { None } else { trigger };
        if trigger == Some("screen") {
            self.instrument_refreshes = None;
            self.protocols = false;
        }
        // a new game opens with "screen" and an instrument offer: cards in hand
        let protocol = raw.state.phase == vorax::advisor::ScreenPhase::Protocol;
        if self.protocols && !protocol && raw.state.hand.is_empty() {
            return Outcome::Closing;
        }
        self.protocols |= protocol;
        let tr = track_instruments(&self.tracker, &raw, trigger);
        self.tracker = tr.tracker.clone();
        let mut reading = raw;
        reading.state.instruments = tr.instruments.clone();
        reading.state.instrument_rounds = Some(tr.rounds.clone());
        // a round not read for sure: the one before stands in, not round 1 (before any board, round 1 as upstream's
        // Solver starts). So does one that can't be: a protocol board below round 11, or a round going back when the
        // screen opened again on the game under way (still fading in, round 12 read as 2)
        let before_round = self.last.as_ref().and_then(|(r, ..)| r.state.round);
        let read_round = reading.state.round.unwrap_or(1);
        let impossible = (reading.state.phase == vorax::advisor::ScreenPhase::Protocol && read_round < 11) || (self.resumed && before_round.is_some_and(|b| read_round < b));
        if let Some(f) = reading.fields.get("round").filter(|f| !f.sure || impossible) {
            let before = self.last.as_ref().map_or(Some(1), |(r, ..)| r.state.round);
            reading.state.round = Some(carried_round(before, reading.state.phase, f.guess.as_deref()));
        }
        reading.state.shelley = Some(self.shelley);
        self.fill_rerolls(&mut reading);
        self.apply_zoom(&mut reading);
        let read_ms = t.elapsed().as_secs_f64() * 1000.0;
        if !trustworthy(&reading) { return Outcome::Uncertain { reading: Box::new(reading) }; }
        let (advice, cached) = self.advise(&reading.state);
        let l = layout_at(img.width, img.height, reading.ui()).shifted_hand(reading.hand_dy.unwrap_or(0.0));
        self.card_frames = crate::card_frame::detect(&img, &l, reading.state.hand.len());
        let mut plan = plan_for(&advice, &reading.state, &reading, &l);
        self.align_card(&mut plan, &advice);
        self.last = Some((reading.clone(), img.width, img.height));
        self.last_current = true;
        self.zoom_dirty = false;
        Outcome::Advice { reading: Box::new(reading), advice: Box::new(advice), plan, read_ms, cached }
    }

    /// A pick the tracker isn't sure of yet, first seen on this board: worth another look once the icon has landed.
    pub fn wants_relook(&self) -> bool {
        self.tracker.unsure.iter().any(|q| q.looks == 0)
    }

    fn align_card(&self, plan: &mut Plan, advice: &Advice) {
        if plan.card.is_some() {
            if let (Some(frames), vorax::advisor::Best::Card(card)) = (&self.card_frames, &advice.best) {
                plan.card = frames.get(card.index).copied();
            }
        }
    }

    fn advise(&mut self, state: &vorax::advisor::ScreenState) -> (Advice, bool) {
        let key = serde_json::to_string(state).expect("finite screen state");
        let count = futures();
        if let Some((previous, n, advice)) = &self.last_advice {
            if previous == &key && *n == count {
                let mut cached = advice.clone();
                cached.ms = 0;
                return (cached, true);
            }
        }
        let advice = advise_plan(state, Some(count), true);
        self.last_advice = Some((key, count, advice.clone()));
        (advice, false)
    }
}

pub fn decode_png(path: &std::path::Path) -> Result<Img, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(file);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = info.color_type.samples();
    let mut img = Img::new(w, h);
    for i in 0..w * h {
        let p = &buf[i * ch..i * ch + ch];
        let px = match ch {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        };
        img.data[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    Ok(img)
}

pub fn encode_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgba).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::resumes;

    #[test]
    fn an_ambiguous_name_without_an_unknown_art_problem_still_requests_enlarged_ocr() {
        use super::*;
        let frame=decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/korean/potion-round6-ultrawide.png")).unwrap();
        let img=windowed_game(&frame).map_or_else(||frame.clone(),|r|crop_to(&frame,&r));
        let mut reading=scan(&img,scan_data(),true);
        reading.problems.clear();
        reading.fields.get_mut("hand1").unwrap().sure=false;
        assert!(!trustworthy(&reading));
        assert!(reading.blocking_fields().contains(&"hand1".to_string()));
        let mut pipe=Pipeline::new(0);pipe.pending=Some(reading);
        assert_eq!(pipe.requested_slots(),[1]);
        assert_eq!(pipe.hover_plan().unwrap().inspect.len(),1);
    }

    #[test]
    fn uncertain_input_never_produces_actionable_advice() {
        use super::*;
        let frame = decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/frames/board-1080p.png")).unwrap();
        let mut reading = scan(&frame, scan_data(), true);
        reading.problems.clear();
        assert!(trustworthy(&reading));
        reading.problems.push(Problem::Unreadable { field: "tank0.quantity".into() });
        assert!(!trustworthy(&reading));
        reading.problems.clear();
        reading.problems.push(Problem::UnknownName { name: "new card".into() });
        assert!(!trustworthy(&reading));
    }

    #[test]
    fn visible_rerolls_override_pet_fallback() {
        use super::*;
        let frame = decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/frames/board-1080p.png")).unwrap();
        let mut reading = scan(&frame, scan_data(), true);
        let mut pipeline = Pipeline::new(6);
        reading.state.phase = vorax::advisor::ScreenPhase::Instrument;
        reading.state.instrument_refreshes = Some(0);
        reading.fields.insert("refreshes".into(), vorax_scan::reading::FieldRead::sure(true));
        pipeline.fill_rerolls(&mut reading);
        reading.state.phase = vorax::advisor::ScreenPhase::Potion;
        reading.state.instrument_refreshes = None;
        pipeline.fill_rerolls(&mut reading);
        assert_eq!(reading.state.instrument_refreshes, Some(0));
        assert_eq!(shelley_rerolls(1), 0);
        assert_eq!(shelley_rerolls(2), 1);
        assert_eq!(shelley_rerolls(6), 2);
        assert_eq!(shelley_rerolls(0), 0);
    }

    #[test]
    fn korean_titles_recover_gold_text_and_keep_all_three_positions() {
        use super::*;
        let frame = decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/frames/board-1080p.png")).unwrap();
        let data = scan_data();
        let names = scan(&frame, data, false);
        // Live Korean OCR: the middle gold heading needs the alternate contrast pass. No picture
        // fallback is supplied, so a missing title must never compact the hand into two positions.
        let mut text = ["비대한 늬하수체", "역! 가 최소2그룹 변이", "얼룩진 간", "오그라는 손가락"].into_iter();
        let read = read_titles(&frame, names, data, &mut |_| text.next().expect("bounded OCR retry").to_owned(), None);
        assert_eq!(read.state.hand, ["Hypertrophic Pituitary", "Prismatic Liver", "Contracted Claws"]);
        assert!(!read.has(|p| matches!(p, Problem::UnknownArt { .. })));
        assert!(text.next().is_none());
    }

    #[test]
    fn an_unread_reroll_counter_cannot_suggest_spending_it() {
        use super::*;
        let frame = decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/frames/board-1080p.png")).unwrap();
        let mut reading = scan(&frame, scan_data(), true);
        reading.problems.clear();
        reading.fields.insert("refreshes".into(), vorax_scan::reading::FieldRead::sure(false));
        assert!(!trustworthy(&reading));
    }

    #[test]
    fn unchanged_state_reuses_exact_advice_but_pet_change_recalculates() {
        use super::*;
        let frame = decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/frames/board-1080p.png")).unwrap();
        let mut pipe = Pipeline::new(0);
        let Outcome::Advice { advice: first, cached: false, .. } = pipe.run(&frame, None, None) else { panic!("first calculation") };
        let Outcome::Advice { advice: second, cached: true, .. } = pipe.run(&frame, None, None) else { panic!("cached calculation") };
        assert_eq!(first.best, second.best);
        assert_eq!(first.ranking, second.ranking);
        pipe.shelley = 6;
        assert!(matches!(pipe.run(&frame, None, None), Outcome::Advice { cached: false, .. }));
    }

    #[test]
    fn the_reported_korean_windows_produce_complete_advice_from_the_first_offer() {
        use super::*;
        for (file, round, hand, rerolls) in [
            ("instrument-no-reroll.png", 1, vec!["Incubation Sac", "Hypertrophic Pituitary", "Contracted Claws"], 0),
            ("potion-round1.png", 1, vec!["Spinal Solution - Aemberon", "Slender-Limb Powder - Pest", "Anesthetic Tincture - Aemberon"], 3),
            ("potion-round4-ultrawide.png", 4, vec!["Osteolytic Ointment", "Twin Hormone - Pest", "Potion Kit (Small)"], 1),
            ("potion-round6-ultrawide.png", 6, vec!["Digestive Enzyme Solution", "Spinal Solution - Aemberon", "Viscous Bile Solution"], 1),
        ] {
            let frame = decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/korean").join(file)).unwrap();
            let mut pipeline = Pipeline::new(0);
            match pipeline.run(&frame, None, None) {
                Outcome::Advice { reading, plan, .. } => {
                    assert_eq!(reading.state.round, Some(round), "{file}");
                    assert_eq!(reading.state.hand, hand, "{file}");
                    assert!(reading.blocking_fields().is_empty(), "{file}");
                    assert!(plan.pill.is_some(), "{file}: visible recommendation");
                    let left = if reading.state.phase == vorax::advisor::ScreenPhase::Instrument { reading.state.instrument_refreshes } else { reading.state.potion_refreshes };
                    assert_eq!(left, Some(rerolls), "{file}");
                }
                Outcome::Uncertain { reading } => panic!("{file}: {}", serde_json::to_string(&reading).unwrap()),
                _ => panic!("{file}: expected a complete board"),
            }
        }
    }

    #[test]
    fn verified_korean_titles_are_not_replaced_by_conflicting_os_ocr() {
        use super::*;
        let frame = decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/korean/potion-round4-ultrawide.png")).unwrap();
        let mut calls = 0;
        let mut text = |_: &Img| { calls += 1; "부화낭".to_string() };
        let mut pipeline = Pipeline::new(0);
        let result = pipeline.run(&frame, None, Some(&mut text));
        match result {
            Outcome::Advice { reading, .. } => assert_eq!(reading.state.hand, ["Osteolytic Ointment", "Twin Hormone - Pest", "Potion Kit (Small)"]),
            _ => panic!("expected the verified Korean offer"),
        }
        assert_eq!(calls, 0);
    }

    #[test]
    fn visual_round_transition_without_log_input_invalidates_the_previous_plan() {
        use super::*;
        use vorax_scan::watch::{Change, ScreenWatch};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/korean");
        let client = |file: &str| {
            let frame = decode_png(&root.join(file)).unwrap();
            windowed_game(&frame).map_or_else(|| frame.clone(), |r| crop_to(&frame, &r))
        };
        let a = client("potion-round4-ultrawide.png");
        let b = client("potion-round6-ultrawide.png");
        // Hold the capture size fixed: the change detector must notice the board, not just resizing.
        let same_size = vorax_scan::pixels::resize(&b, a.width, a.height);
        let mut pipe = Pipeline::new(1);
        assert!(matches!(pipe.run(&a, None, None), Outcome::Advice { .. }));
        let mut watch = ScreenWatch::default();
        watch.accept(&a, pipe.ui());
        assert_eq!(watch.observe(&same_size, pipe.ui(), 500.0), Change::Changed);
        pipe.invalidate();
        assert!(pipe.readvise().is_none(), "a settings change cannot resurrect the old recommendation");
        assert_eq!(watch.observe(&same_size, pipe.ui(), 1000.0), Change::Ready);
        let Outcome::Advice { reading, .. } = pipe.run(&b, None, None) else { panic!("new screen must read without a log event or hotkey") };
        assert_eq!(reading.state.round, Some(6));
        assert_eq!(reading.state.hand[1], "Spinal Solution - Aemberon");
        watch.accept(&b, pipe.ui());
        assert_eq!(watch.observe(&b, pipe.ui(), 1500.0), Change::Unchanged);
        assert!(pipe.readvise().is_some());
        assert!(matches!(pipe.run(&Img::new(960, 540), None, None), Outcome::NotGame));
        assert!(pipe.readvise().is_none(), "a failed read must also invalidate the visible plan");
    }

    #[test]
    fn the_screen_opened_again_mid_game_goes_on() {
        assert!(resumes(Some(6), Some(6)));
        assert!(resumes(Some(6), Some(7)));
        assert!(resumes(Some(12), Some(12)));
    }

    #[test]
    fn a_new_game_is_a_new_game() {
        // a new game starts in round 1, whatever is in the instrument panel (a boss drop from the map)
        assert!(!resumes(Some(13), Some(1)));
        assert!(!resumes(Some(6), Some(1)));
        // nothing goes on after round 13, nor back in rounds, nor when the round isn't read for sure
        assert!(!resumes(Some(13), Some(13)));
        assert!(!resumes(Some(8), Some(5)));
        assert!(!resumes(Some(6), None));
        // the overlay's first board
        assert!(!resumes(None, Some(6)));
    }
    #[test]
    fn enlarged_tooltip_is_located_and_wrong_card_association_is_rejected() {
        use super::*;
        let frame=decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hover/instrument-tooltip.png")).unwrap();
        let img=windowed_game(&frame).map_or_else(||frame.clone(),|r|crop_to(&frame,&r));
        let tip=vorax_scan::hover::tooltip(&img).expect("the game renders a large card on the left");
        assert!(tip.rect.x<img.width as f64*0.1 && tip.rect.h>img.height as f64*0.3);
        let mut text=|_:&Img|"엉겨붙은 척골".to_string();
        let read=vorax_scan::hover::recognize(&img,"instrument",&[("Adherent Metatarsals".into(),0.99)],&mut text).unwrap();
        assert_eq!(read.name,"Adherent Metatarsals");
        assert!(read.art_score>=0.92);
        assert!(vorax_scan::hover::recognize(&img,"instrument",&[("Hypertrophic Pituitary".into(),0.99)],&mut text).is_err());
        assert!(vorax_scan::hover::recognize(&Img::new(960,540),"instrument",&[],&mut text).is_err());
    }

    #[test]
    fn hover_fills_only_the_requested_slot_after_two_observations_and_expires_on_board_change() {
        use super::*;
        let frame=decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/korean/potion-round6-ultrawide.png")).unwrap();
        let img=windowed_game(&frame).map_or_else(||frame.clone(),|r|crop_to(&frame,&r));
        let mut r=scan(&img,scan_data(),true);
        r.fields.get_mut("hand0").unwrap().sure=false;
        r.problems.push(Problem::UnknownArt {field:"hand0".into(),crop:layout_at(img.width,img.height,1.0).card_face[0],hand:None,text:None});
        let mut pipe=Pipeline::new(0);pipe.apply_zoom(&mut r);
        assert_eq!(pipe.requested_slots(),[0]);
        let plan=pipe.hover_plan().unwrap();assert_eq!(plan.inspect.len(),1);assert!(plan.card.is_none());
        let read=vorax_scan::hover::HoverRead{name:"Digestive Enzyme Solution".into(),text:"소화 효소 용액".into(),title_score:12.0,art_score:0.99,tooltip:vorax_scan::hover::Tooltip{rect:Default::default(),center:vorax_scan::hover::CardPosData{x:0.0,y:0.0},scale:1.0}};
        assert!(!pipe.confirm_hover(1,&read,0.0));
        assert!(!pipe.confirm_hover(0,&read,500.0));
        assert!(pipe.confirm_hover(0,&read,1000.0));
        pipe.apply_zoom(&mut r);
        assert!(r.fields["hand0"].sure);assert!(pipe.requested_slots().is_empty());
        assert_eq!(r.state.hand,["Digestive Enzyme Solution","Spinal Solution - Aemberon","Viscous Bile Solution"]);
        r.state.round=Some(7);pipe.apply_zoom(&mut r);assert!(pipe.zoom_names.is_empty());
    }

    fn reported(file:&str) -> super::Img {
        let frame=super::decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reported").join(file)).unwrap();
        super::windowed_game(&frame).map_or_else(||frame.clone(),|r|super::crop_to(&frame,&r))
    }

    #[test]
    fn reported_red_heart_does_not_hide_the_blue_pest_and_settings_are_not_a_board() {
        use super::*;
        for file in ["hover-before.png","hover-after.png","board-before.png","board-after.png"] {
            let img=reported(file);let r=scan(&img,scan_data(),true);
            let values:Vec<_>=r.state.tanks.iter().map(|m|m.as_ref().map(|m|(m.archetype,m.rarity,m.activity,m.quantity))).collect();
            assert_eq!(values[0],Some((vorax::types::Archetype::Agito,vorax::types::Rarity::Rare,15,12)),"{file}");
            assert_eq!(values[1],Some((vorax::types::Archetype::Pest,vorax::types::Rarity::Magic,5,24)),"{file}: {}",serde_json::to_string(&r).unwrap());
            assert_eq!(values[2],values[0],"{file}");
            assert!(values[3..].iter().all(Option::is_none),"{file}");
            assert_eq!(r.state.round,Some(1));
        }
        let mut pipe=Pipeline::new(0);
        assert!(matches!(pipe.run(&reported("settings.png"),None,None),Outcome::NotGame));
    }

    #[test]
    fn recorded_flames_do_not_veto_advice_but_a_verified_number_change_does() {
        use super::*;
        use vorax_scan::watch::{Change,ScreenWatch};
        let before=reported("board-before.png");let after=reported("board-after.png");
        let mut watch=ScreenWatch::default();watch.accept(&before,1.0);
        assert!(!watch.differs(&after,1.0),"reported unchanged board: {:?}",watch.compare(&after,1.0));
        let l=layout_at(before.width,before.height,1.0);
        let mut flame=before.clone();
        // This previously watched empty-tank band is scenery. It contains no valid numeric pair.
        for r in [l.name_band[3],l.total_band[3],l.aq_band[3],l.icon[4]] {
            for y in r.y as usize..(r.y+r.h) as usize {for x in r.x as usize..(r.x+r.w) as usize {
                let at=(y*flame.width+x)*4;flame.data[at..at+3].copy_from_slice(&[220,40,60]);
            }}
        }
        assert!(!watch.differs(&flame,1.0),"empty glass cannot invalidate a correctly read board");
        let mut changed=before.clone();
        let k=l.k;
        let src=vorax_scan::pixels::crop(&before,vorax_scan::pixels::Rect::new(l.tank_cx[1]-150.0*k,l.name_band[1].y-40.0*k,300.0*k,120.0*k));
        let left=(l.tank_cx[0]-150.0*k).round() as usize;let top=(l.name_band[0].y-40.0*k).round() as usize;
        for y in 0..src.height {for x in 0..src.width {
            let to=((top+y)*changed.width+left+x)*4;let from=(y*src.width+x)*4;
            changed.data[to..to+4].copy_from_slice(&src.data[from..from+4]);
        }}
        assert!(watch.differs(&changed,1.0),"a new validated 5×24 pair must still trigger a read");
        assert_eq!(watch.observe(&changed,1.0,500.0),Change::Changed);
        assert_eq!(watch.observe(&changed,1.0,1000.0),Change::Ready);
        let read=scan(&changed,scan_data(),false);
        assert_eq!(read.state.tanks[0].as_ref().map(|m|(m.activity,m.quantity)),Some((5,24)));
    }

    #[test]
    fn captured_ocr_spelling_and_tooltip_route_work_without_a_pointer_target() {
        use super::*;
        let frame=reported("hover-before.png");
        // Actual OCR spelling captured in overlay(2).log, rather than a perfect supplied title.
        let mut text=|_:&Img|"엉켜불은 척골\n최소 2 그를 해골\n경비병 보유 시,".to_string();
        let read=vorax_scan::hover::recognize(&frame,"instrument",&[],&mut text).unwrap();
        assert_eq!(read.name,"Adherent Metatarsals");
        let candidates=vec![(0,vec![("Adherent Metatarsals".into(),0.99)])];
        assert_eq!(vorax_scan::hover::target(&read.name,&candidates,None),Some(0));
        let mut pipe=Pipeline::new(0);
        // Exercise the public production path. Calling apply_zoom directly would miss a wiring bug.
        fn recorded_titles()->impl FnMut(&Img)->String {
            let mut heading=0;
            move |img:&Img| {
                if (img.width,img.height)==(464,120) {
                    heading+=1;
                    match heading {1=>"엉켜“\n=츼골",2=>"비대한 늬하수체",_=>"오그라는 손가락"}.to_string()
                } else {"엉켜불은적골\n최소 2 그룹 해골\n경비병 보유 시,".to_string()}
            }
        }
        let mut failed_text=recorded_titles();
        let first=pipe.run(&frame,None,Some(&mut failed_text));
        assert!(matches!(first,Outcome::Uncertain{..}),"reported first title must request assistance");
        assert_eq!(pipe.requested_slots(),[0],"run must request only the failed title");
        assert!(pipe.hover_plan().is_some(),"an unresolved card needs the blue guide");
        assert!(!pipe.confirm_hover(0,&read,500.0));assert!(pipe.confirm_hover(0,&read,1000.0));
        assert!(!pipe.requested_slots().contains(&0));
        let mut failed_text=recorded_titles();
        let result=pipe.run(&reported("board-before.png"),None,Some(&mut failed_text));
        match result {
            Outcome::Advice{reading,..} => {assert_eq!(reading.state.hand[0],"Adherent Metatarsals");assert!(reading.fields["hand0"].sure);},
            Outcome::Uncertain{reading} => panic!("completed hover must reach advice: {}",serde_json::to_string(&reading).unwrap()),
            _ => panic!("completed hover must reach advice"),
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed Windows Korean OCR; CI runs this separately when available"]
    fn native_windows_korean_ocr_reads_reported_tooltip() {
        use super::*;
        unsafe {let _=windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);}
        let ocr=crate::app::ocr::Ocr::for_language("ko").expect("Windows Korean OCR capability");
        for file in ["hover-before.png","hover-after.png"] {
            let frame=reported(file);let mut text=|img:&Img|ocr.read(img).unwrap();
            let read=vorax_scan::hover::recognize(&frame,"instrument",&[],&mut text);
            assert!(matches!(&read,Ok(r) if r.name=="Adherent Metatarsals"),"{file}: {read:?}");
        }
        for i in 0..3 {
            let frame=blue_potion(i);let mut text=|img:&Img|ocr.read(img).unwrap();
            let read=vorax_scan::hover::recognize(&frame,"potion",&[],&mut text);
            assert!(matches!(&read,Ok(r) if r.name=="Gray Matter Spinal Solution"),"blue potion {i}: {read:?}");
        }
    }

    fn blue_potion(i:usize)->super::Img {
        super::decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/blue-tooltip/potion-{i}.png"))).unwrap()
    }

    // These strings are the captured ordinary-offer OCR, in order. The enlarged card is a separate
    // OCR input; a correct response here tests the geometry and routing, not the OS OCR implementation.
    fn blue_offer_ocr()->impl FnMut(&super::Img)->String {
        let mut values=["식수 가루약","활성 적수 가루약\n변이 악마가 아","한성 배육 호르몬;",
            "1 그룹 몬스터","랜엄\n활성 +41, 몬스터로","회벽한 적수 용역\n1 몬스터 41?\n한성 +41, 몬스터로"].into_iter();
        move |_|values.next().unwrap_or("").to_string()
    }

    #[test]
    fn broken_blue_borders_enter_ocr_and_only_complete_the_third_offer() {
        use super::*;
        let first=blue_potion(0);let mut pipe=Pipeline::new(0);let mut ordinary=blue_offer_ocr();
        let result=pipe.run(&first,None,Some(&mut ordinary));
        assert!(matches!(result,Outcome::Uncertain{..}),"reported third title needs assistance");
        assert_eq!(pipe.requested_slots(),[2]);assert_eq!(pipe.hover_plan().unwrap().inspect.len(),1);
        for i in 0..3 {
            let frame=blue_potion(i);let search=vorax_scan::hover::inspect_tooltip(&frame);
            let tip=search.tooltip.as_ref().unwrap_or_else(||panic!("blue {i}: {search:?}"));
            assert_eq!(search.method,Some("blue_edges"));
            assert!((tip.rect.x-44.0).abs()<10.0 && (tip.rect.y-418.0).abs()<10.0,"{search:?}");
            assert!((tip.rect.w-220.0).abs()<12.0 && (tip.rect.h-328.0).abs()<12.0,"{search:?}");
            let mut text=|_:&Img|"회백질 척수 용액".to_string();
            let read=vorax_scan::hover::recognize(&frame,"potion",&[],&mut text).unwrap();
            assert_eq!(read.name,"Gray Matter Spinal Solution");assert!(read.art_score>=0.92);
            if i<2 {
                let candidates=pipe.requested_slots().into_iter().map(|slot|(slot,pipe.pending().unwrap().hand_ranks.as_ref().unwrap()[slot].clone())).collect::<Vec<_>>();
                assert_eq!(vorax_scan::hover::target(&read.name,&candidates,None),Some(2));
                assert_eq!(pipe.confirm_hover(2,&read,500.0+i as f64*500.0),i==1);
            }
        }
        assert!(pipe.requested_slots().is_empty());let mut ordinary=blue_offer_ocr();
        match pipe.run(&blue_potion(1),None,Some(&mut ordinary)) {
            Outcome::Advice{reading,plan,..}=>{
                assert_eq!(reading.state.hand,["Fresh Spinal Powder","Viviparous Hormone","Gray Matter Spinal Solution"]);
                assert_eq!(reading.state.round,Some(2));assert_eq!(reading.state.potion_refreshes,Some(3));assert!(plan.pill.is_some());
            },
            Outcome::Uncertain{reading}=>panic!("hover must leave the loop: {}",serde_json::to_string(&reading).unwrap()),
            _=>panic!("hover must lead to advice"),
        }
        assert!(vorax_scan::hover::tooltip(&reported("settings.png")).is_none());
        assert!(vorax_scan::hover::tooltip(&reported("board-before.png")).is_none());
    }

    #[test]
    fn blue_hover_reflow_and_red_heart_do_not_reset_assistance_but_real_counters_do() {
        use super::*;
        use vorax_scan::watch::{Change,ScreenWatch};
        let mut watch=ScreenWatch::default();let first=blue_potion(0);watch.accept(&first,1.0);
        for i in 0..9 {
            let frame=blue_potion(i%3);let r=scan(&frame,scan_data(),true);
            for j in 0..4 {
                let m=r.state.tanks[j].as_ref().unwrap();
                assert_eq!((m.archetype,m.rarity,m.activity,m.quantity),(vorax::types::Archetype::Pest,vorax::types::Rarity::Normal,45,36),"frame {i}: {}",serde_json::to_string(&r).unwrap());
            }
            assert_eq!(watch.observe(&frame,1.0,500.0+i as f64*500.0),Change::Unchanged,"hover reflows the whole fan: {:?}",watch.last_difference);
            watch.accept(&frame,1.0);
        }
        let l=layout_at(first.width,first.height,1.0);
        for band in [l.round,l.refresh,l.instrument_icon[0]] {
            let mut changed=first.clone();
            for y in band.y as usize..(band.y+band.h) as usize {for x in band.x as usize..(band.x+band.w) as usize {
                let at=(y*changed.width+x)*4;changed.data[at..at+3].copy_from_slice(&[220,220,220]);
            }}
            assert!(watch.differs(&changed,1.0),"real counters/panel must invalidate advice while hovering");
        }
        let mut no_popup=first.clone();let tip=vorax_scan::hover::tooltip(&first).unwrap();
        for band in [vorax_scan::pixels::Rect::new(tip.rect.x-8.0,tip.rect.y-8.0,tip.rect.w+16.0,tip.rect.h+16.0),
            vorax_scan::pixels::Rect::new(l.cards[0].cx-60.0*l.k,l.cards[0].cy-32.0*l.k,120.0*l.k,30.0*l.k)] {
            for y in band.y as usize..(band.y+band.h) as usize {for x in band.x as usize..(band.x+band.w) as usize {
                let at=(y*no_popup.width+x)*4;no_popup.data[at..at+3].copy_from_slice(&[30,20,20]);
            }}
        }
        assert!(vorax_scan::hover::tooltip(&no_popup).is_none());
        watch.accept(&first,1.0);
        assert!(!watch.differs(&no_popup,1.0),"popup exit is fan restoration, not a new board");
        assert_eq!(watch.observe(&no_popup,1.0,6000.0),Change::Unchanged);
        assert_eq!(watch.observe(&no_popup,1.0,6500.0),Change::Unchanged);
        let mut new_title=no_popup.clone();let c=l.cards[0];
        for y in (c.cy-32.0*l.k) as usize..(c.cy-2.0*l.k) as usize {for x in (c.cx-60.0*l.k) as usize..(c.cx+60.0*l.k) as usize {
            let at=(y*new_title.width+x)*4;new_title.data[at..at+3].copy_from_slice(&[220,220,220]);
        }}
        assert!(watch.differs(&new_title,1.0),"after the one-time fan rebase, actual card changes must be watched again");
        watch.accept(&first,1.0);
        // Keep the last verified watcher value across a momentarily occluded number, then recover
        // without manufacturing a second change event solely because confidence returned.
        let mut occluded=first.clone();let band=l.aq_band[1];
        for y in (band.y-5.0) as usize..(band.y+band.h+12.0) as usize {for x in band.x as usize..(band.x+band.w) as usize {
            let at=(y*occluded.width+x)*4;occluded.data[at..at+3].copy_from_slice(&[30,20,20]);
        }}
        watch.accept(&occluded,1.0);assert!(!watch.differs(&first,1.0));
        let old=decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/korean/potion-round6-ultrawide.png")).unwrap();
        let client=windowed_game(&old).map_or_else(||old.clone(),|r|crop_to(&old,&r));let r=scan(&client,scan_data(),true);
        for i in [2,3] {assert_eq!(r.state.tanks[i].as_ref().unwrap().rarity,vorax::types::Rarity::Boss,"genuine red boss titles remain red");}
    }

}
