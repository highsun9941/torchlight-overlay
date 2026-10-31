//! Watches the static screen UI as well as the game's log. Changed screens clear old advice immediately;
//! a stable screen is read once, even when the game stops writing board events.

use super::capture::Capturer;
use super::diagnostics;
use super::ocr::Ocr;
use super::sys::{find_game_window, find_log, note};
use super::window::{GameLog, ToUi, ToWorker, Waker};
use crate::pipeline::{Outcome, Pipeline};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use vorax_scan::live::{log_time, Follower, Settler, Trigger};
use vorax_scan::watch::{Change, ScreenWatch};

pub struct Options {
    pub log: Option<PathBuf>,
    pub shelley: i32,
    pub ocr: bool,
    /// --frames <dir>: every picture read is saved there, named by what came of it
    pub frames: Option<PathBuf>,
}

/// How long after a pick the board is looked at again, for the new icon to have landed in the panel.
const RELOOK: Duration = Duration::from_millis(1500);
/// A card still in flight: the board is looked at again this much later, this many times, before its marks are
/// hidden (the board they were for is gone, and no log line may come to say when the new one has landed).
const RETRY: Duration = Duration::from_millis(700);
const RETRIES: u32 = 3;
const VISUAL_POLL: Duration = Duration::from_millis(500);

/// A line the game writes only with its log on (Esc → Account Related → Enable Log): with it on, one comes at
/// least every ~10 s (pings, sounds, clicks); with it off, only a few as the game starts.
const GAME_LINE: &str = "TLLua: Display: [Game]";
/// How long the game can be open without a game line before its log counts as off.
const QUIET: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq)]
enum RetryReason { None, Animating, Uncertain }

pub fn run(opts: Options, tx: Sender<ToUi>, waker: Waker, rx: Receiver<ToWorker>) {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let send = |m: ToUi| {
        let _ = tx.send(m);
        waker.wake();
    };
    let capturer = match Capturer::new() {
        Ok(c) => c,
        Err(e) => {
            note(&format!("no screen capture: {e}"));
            return;
        }
    };
    let ocr = if opts.ocr {
        match Ocr::new() {
            Ok(o) => {
                note(&format!("card titles read with Windows OCR ({})", o.language));
                Some(o)
            }
            Err(e) => {
                note(&format!("no Windows OCR ({e}): cards read by their pictures"));
                None
            }
        }
    } else {
        None
    };
    if let Some(dir) = &opts.frames {
        note(&format!("saving every picture read to {}", dir.display()));
    }
    let mut pipe = Pipeline::new(opts.shelley);
    let mut watch = ScreenWatch::default();
    let mut last_visual_poll = Instant::now() - VISUAL_POLL;
    let mut last_heartbeat=Instant::now();
    let mut last_hovered=None;
    let mut last_hover_wait=Instant::now()-Duration::from_secs(3);
    let mut follower: Option<Follower> = None;
    let mut looked_for_log = Instant::now() - Duration::from_secs(60);
    let mut settler = Settler::default();
    let mut last_poll = Instant::now() - Duration::from_secs(1);
    let mut relook_at: Option<Instant> = None;
    // a look again at a card in flight: when, for which trigger, and how many have been made
    let mut retry: Option<(Instant, Option<&'static str>)> = None;
    let mut retries = 0;
    let mut was_over = false;
    // the game's log: when it last had a game line, since when the game is open, and what the window was told
    let mut game_line_at: Option<Instant> = None;
    let mut game_open_since: Option<Instant> = None;
    let mut game_log: Option<GameLog> = None;
    let mut last_log_check = Instant::now() - Duration::from_secs(1);
    let t0 = Instant::now();
    let now_ms = || t0.elapsed().as_secs_f64() * 1000.0;

    status("수술 준비 화면 대기", "Ctrl+Alt+X로 현재 화면을 읽을 수 있습니다.", "엔진 실행 중");
    let mut first_scan = true;
    loop {
        let command = super::sys::data_dir().join("command.txt");
        let requested = std::fs::read_to_string(&command).ok().is_some_and(|s| s.trim() == "scan");
        if requested { let _ = std::fs::remove_file(command); }
        let mut manual = requested || (first_scan && find_game_window().is_some());
        if manual { first_scan = false; }
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                ToWorker::Quit => return,
                ToWorker::Snap => manual = true,
                ToWorker::Shelley(level) => {
                    pipe.shelley = level;
                    if let Some((plan, w, h)) = pipe.readvise() {
                        send(ToUi::Show(plan, w, h));
                    }
                }
            }
        }
        if follower.is_none() && looked_for_log.elapsed() > Duration::from_secs(10) {
            looked_for_log = Instant::now();
            let found = opts.log.clone().or_else(find_log);
            match found {
                Some(p) => {
                    note(&format!("following the game's log: {}", p.display()));
                    // a game already open with its log on: known at once, not half a minute later
                    if let Some(age) = last_game_line_age(&p) {
                        game_line_at = Instant::now().checked_sub(age);
                    }
                    follower = Some(Follower::new(p));
                }
                None => note("UE_game.log not found (start the game, or pass --log <path>)"),
            }
        }
        if last_poll.elapsed() >= Duration::from_millis(100) {
            last_poll = Instant::now();
            if let Some(f) = &mut follower {
                let lines = f.poll();
                if !lines.is_empty() {
                    settler.feed(&lines, now_ms());
                    if lines.iter().any(|l| l.contains(GAME_LINE)) {
                        game_line_at = Some(Instant::now());
                    }
                }
            }
            if settler.over && !was_over {
                note("the game's Vorax screen closed");
                pipe.invalidate();
                status("수술 준비 화면 대기", "다음 화면을 기다립니다.", "보드 닫힘");
                send(ToUi::Clear);
                retry = None;
            }
            was_over = settler.over;
        }
        if last_log_check.elapsed() >= Duration::from_secs(1) {
            last_log_check = Instant::now();
            if last_heartbeat.elapsed()>=Duration::from_secs(10) {
                diagnostics::event(0,"watch_heartbeat",serde_json::json!({"capture":capturer.state(),"pendingSlots":pipe.requested_slots(),"pointerSlot":last_hovered,"foreground":super::sys::foreground().0,"lastChange":watch.last_difference,"retries":retries}));
                last_heartbeat=Instant::now();
            }
            game_open_since = if find_game_window().is_some() { game_open_since.or(Some(Instant::now())) } else { None };
            let state = match (&follower, game_open_since) {
                (None, _) => GameLog::Missing,
                (Some(f), _) if !f.path().is_file() => GameLog::Missing,
                (_, None) => GameLog::NoGame,
                _ if game_line_at.is_some_and(|t| t.elapsed() < QUIET) => GameLog::On,
                (_, Some(t)) if t.elapsed() < QUIET => GameLog::Checking,
                _ => GameLog::Off,
            };
            if game_log != Some(state) {
                match state {
                    GameLog::On => note("the game's log has game lines: log and visual change detection are active"),
                    GameLog::Off => note("no game log events for 30 s: visual change detection remains active"),
                    GameLog::Missing => note("the game's log isn't there: visual change detection remains active"),
                    _ => {}
                }
                game_log = Some(state);
                send(ToUi::GameLog(state));
            }
        }
        let due = settler.due(now_ms());
        let relook = relook_at.is_some_and(|t| Instant::now() >= t);
        let again = retry.filter(|(t, _)| Instant::now() >= *t);
        let mut visual = false;
        if last_visual_poll.elapsed() >= VISUAL_POLL && !manual && due.is_none() && !relook && again.is_none() {
            last_visual_poll = Instant::now();
            if let Some(game) = find_game_window() {
                match capturer.grab(game) {
                    Ok(Some(frame)) => {
                        let size=pipe.pending().and_then(|r|r.hand_ranks.as_ref()).map_or(3,|h|h.len().max(3));
                        let hovered=super::sys::hovered_card(game,pipe.ui(),size);
                        watch.set_hover(hovered);
                        last_hovered=hovered;
                        let mut inspecting=false;
                        let requested=pipe.requested_slots();
                        let search=if requested.is_empty() && !pipe.hover_dirty() {None} else {Some(vorax_scan::hover::inspect_tooltip(&frame))};
                        let tip=search.as_ref().and_then(|s|s.tooltip.as_ref());
                        if !requested.is_empty() {
                            if let (Some(_),Some(engine),Some(pending))=(tip.as_ref(),ocr.as_ref(),pipe.pending()) {
                                inspecting=true;
                                let phase=match pending.state.phase {vorax::advisor::ScreenPhase::Potion=>"potion",vorax::advisor::ScreenPhase::Instrument=>"instrument",_=>"protocol"};
                                let candidates:Vec<_>=requested.iter().map(|&slot|(slot,pending.hand_ranks.as_ref().and_then(|r|r.get(slot)).cloned().unwrap_or_default())).collect();
                                let hover_id=diagnostics::next_scan();
                                let mut hover_texts=Vec::new();let mut hover_crops=Vec::new();
                                let mut text=|crop:&vorax_scan::Img| {
                                    let result=engine.read(crop);
                                    let text=result.as_ref().ok().cloned().unwrap_or_default();
                                    hover_texts.push(serde_json::json!({"size":[crop.width,crop.height],"text":text,"error":result.err().map(|e|e.to_string())}));
                                    hover_crops.push(crop.clone());text
                                };
                                let result=vorax_scan::hover::recognize(&frame,phase,&[],&mut text);
                                drop(text);
                                let slot=result.as_ref().ok().and_then(|r|vorax_scan::hover::target(&r.name,&candidates,hovered));
                                diagnostics::event(hover_id,"hover_ocr",serde_json::json!({"slot":slot,"pointerSlot":hovered,"foreground":super::sys::foreground().0,"gameWindow":game.0,"result":result,"sourceCandidates":candidates,"ocr":hover_texts}));
                                if result.is_err() || slot.is_none() {diagnostics::save(hover_id,"hover_unverified",&frame,None,pipe.pending(),&hover_texts,&hover_crops,serde_json::json!({"slot":slot,"result":result}));}
                                if let (Ok(result),Some(slot))=(result,slot) {
                                    if pipe.confirm_hover(slot,&result,now_ms()) {
                                        diagnostics::event(hover_id,"hover_confirmed",serde_json::json!({"slot":slot,"name":result.name,"remaining":pipe.requested_slots()}));
                                        status("확대 인식 완료","다음 파란 카드에 마우스를 올리거나, 카드 밖으로 옮겨 추천을 갱신하세요.","확대 결과 유지 중");
                                        if let Some(plan)=pipe.hover_plan() {send(ToUi::Show(plan,frame.width,frame.height));} else {send(ToUi::Clear);}
                                    } else {status("확대 스캔 확인 중",&format!("{}번 확대 카드 내용을 한 번 더 확인합니다.",slot+1),"같은 이름 두 번 확인");}
                                } else {
                                    // Keep the last verified observation across a noisy OCR frame.
                                    // confirm_hover still requires the same slot/name within 2.5 seconds;
                                    // a missing popup or a real board change clears the candidate below.
                                    status("확대 스캔 대기","파란 카드의 확대 내용을 보여 주세요. 후보가 겹치면 원래 카드에 마우스를 유지하세요.","제목·그림·카드 위치 확인 중");
                                }
                            } else {
                                pipe.cancel_hover_candidate();
                                if last_hover_wait.elapsed()>=Duration::from_secs(3) {
                                    diagnostics::event(0,"hover_wait",serde_json::json!({"reason":if tip.is_none(){"tooltip_missing"}else{"ocr_unavailable"},"requested":requested,"pointerSlot":hovered,"foreground":super::sys::foreground().0,"gameWindow":game.0,"tooltipSearch":search}));
                                    if tip.is_none() {diagnostics::save(0,"tooltip_missing",&frame,None,pipe.pending(),&[],&[],serde_json::json!({"requested":requested,"tooltipSearch":search}));}
                                    last_hover_wait=Instant::now();
                                }
                            }
                        } else {pipe.cancel_hover_candidate();}
                        if pipe.hover_dirty() {
                            if hovered.is_some() && tip.is_some() {inspecting=true;}
                            else {visual=true;}
                        }
                        match watch.observe(&frame, pipe.ui(), now_ms()) {
                        Change::Changed => {
                            diagnostics::event(0,"visual_change",serde_json::json!({"areas":watch.last_difference}));
                            pipe.invalidate();
                            pipe.reset_hover();
                            retry = None;
                            relook_at = None;
                            status("화면 변경 감지", "화면이 안정되면 자동으로 다시 읽습니다.", "이전 추천 숨김");
                            send(ToUi::Clear);
                        }
                        Change::Ready => visual = true,
                        _ => {}
                        }
                        if inspecting && !visual {continue;}
                    },
                    _ => {
                        pipe.invalidate();
                        watch.reset();
                        status("화면 연결 대기", "게임 창을 복원하면 자동으로 다시 읽습니다.", "캡처 대기");
                        send(ToUi::Clear);
                    }
                }
            } else if watch.reset() {
                capturer.stop();
                pipe.reset_hover();
                pipe.invalidate();
                status("게임 창 연결 대기", "토치라이트를 실행하세요.", "창 없음");
                send(ToUi::Clear);
            }
        }
        if due.is_some() || manual || relook || again.is_some() || visual {
            relook_at = None;
            retry = None;
            let trigger = match due {
                Some(Trigger::Screen) => Some("screen"),
                Some(_) => Some("board"),
                None if visual => Some("visual"),
                None => again.and_then(|(_, t)| t),
            };
            if due.is_some() || manual || visual {
                retries = 0;
            }
            // the look again after a pick, to settle the new icon: the board hasn't changed, so the marks stay up
            // while it reads and only change if what it finds does
            let quiet = relook && due.is_none() && !manual && again.is_none() && !visual;
            let retry_reason = read(&capturer, ocr.as_ref(), &mut pipe, &mut watch, trigger, manual || relook, quiet, opts.frames.as_deref(), &send);
            if (retry_reason == RetryReason::Uncertain && !pipe.requested_slots().is_empty()) || retry_reason == RetryReason::None || quiet {
                retries = 0;
            } else if retries < RETRIES {
                retries += 1;
                retry = Some((Instant::now() + RETRY, trigger));
            } else {
                retries = 0;
                let reason = if retry_reason == RetryReason::Animating { "cards still moving" } else { "unresolved screen fields" };
                note(&format!("{reason} after {RETRIES} retries: advice withheld until the next scan"));
                send(ToUi::Clear);
            }
            if pipe.wants_relook() {
                relook_at = Some(Instant::now() + RELOOK);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// How long ago the log's last game line was written, from the log's last megabyte; None without one.
fn last_game_line_age(path: &Path) -> Option<Duration> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(1 << 20))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let at = String::from_utf8_lossy(&buf).lines().rev().find(|l| l.contains(GAME_LINE)).and_then(log_time)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64() * 1000.0;
    Some(Duration::from_millis((now - at).max(0.0) as u64))
}

/// Takes a picture, reads it and advises; true when a card was in flight (nothing read).
#[allow(clippy::too_many_arguments)]
fn read(capturer: &Capturer, ocr: Option<&Ocr>, pipe: &mut Pipeline, watch: &mut ScreenWatch, trigger: Option<&str>, now: bool, quiet: bool, frames: Option<&Path>, send: &dyn Fn(ToUi)) -> RetryReason {
    let scan_id=diagnostics::next_scan();
    diagnostics::event(scan_id,"scan_start",serde_json::json!({"trigger":trigger,"manual":now,"quiet":quiet,"ocrMode":std::env::var("VORAX_OCR_MODE").ok(),"threads":std::env::var("RAYON_NUM_THREADS").ok()}));
    pipe.invalidate();
    let Some(game) = find_game_window() else {
        watch.reset();
        note("the game's window isn't open");
        status("게임 창 연결 대기", "토치라이트를 실행하세요.", "창 없음");
        send(ToUi::Clear);
        return RetryReason::None;
    };
    if !quiet {
        status("화면 분석 중", "이전 추천 표시는 숨겼습니다.", "인식 및 시뮬레이션 중");
        send(ToUi::Busy);
    }
    let t = Instant::now();
    let frame = if now { capturer.grab(game) } else { capturer.grab_stable(game) };
    let frame = match frame {
        Ok(Some(f)) => f,
        Ok(None) => {
            watch.reset();
            note("no picture from the game's window");
            status("화면을 읽지 못했습니다", "게임 창을 복원한 뒤 다시 인식하세요.", "캡처 실패");
            send(ToUi::Clear);
            return RetryReason::None;
        }
        Err(e) => {
            watch.reset();
            note(&format!("capture failed: {e}"));
            status("화면을 읽지 못했습니다", "게임 창을 복원한 뒤 다시 인식하세요.", "캡처 실패");
            send(ToUi::Clear);
            return RetryReason::None;
        }
    };
    let grabbed = t.elapsed().as_millis();
    let mut ocr_attempts=Vec::new();
    let mut crops=Vec::new();
    let mut read_text=|img:&vorax_scan::pixels::Img| {
        let result=ocr.map(|o|o.read(img));
        let text=result.as_ref().and_then(|r|r.as_ref().ok()).cloned().unwrap_or_default();
        ocr_attempts.push(serde_json::json!({"size":[img.width,img.height],"text":text,"error":result.and_then(Result::err).map(|e|e.to_string())}));
        if crops.len()<4 {crops.push(img.clone());}
        text
    };
    let reader: crate::pipeline::TextReader = if ocr.is_some() { Some(&mut read_text) } else { None };
    let outcome = pipe.run(&frame, trigger, reader);
    drop(read_text);
    let kind=match &outcome {Outcome::Advice{..}=>"advice",Outcome::Uncertain{..}=>"uncertain",Outcome::Animating=>"animating",Outcome::NotGame=>"not_game",Outcome::Closing=>"closing"};
    let reading=match &outcome {Outcome::Advice{reading,..}|Outcome::Uncertain{reading}=>Some(reading.as_ref()),_=>None};
    diagnostics::event(scan_id,"recognition",serde_json::json!({"size":[frame.width,frame.height],"captureMs":grabbed,"outcome":kind,"ui":pipe.ui(),"blocking":reading.map(|r|r.blocking_fields()),"ocr":ocr_attempts}));
    if kind=="uncertain" {diagnostics::save(scan_id,kind,&frame,None,reading,&ocr_attempts,&crops,serde_json::json!({"trigger":trigger}));}
    watch.accept(&frame, pipe.ui());
    if matches!(&outcome, Outcome::Advice { .. }) {
        let size=reading.and_then(|r|r.hand_ranks.as_ref()).map_or(3,|h|h.len().max(3));
        watch.set_hover(super::sys::hovered_card(game,pipe.ui(),size));
        match capturer.grab(game) {
            Ok(Some(latest)) => {
                let diff=watch.compare(&latest,pipe.ui());
                if !diff.is_empty() {
                    pipe.invalidate();
                    diagnostics::event(scan_id,"freshness_rejected",serde_json::json!({"areas":diff,"recognized":reading.map(|r|&r.state),"before":[frame.width,frame.height],"after":[latest.width,latest.height]}));
                    diagnostics::save(scan_id,"freshness_rejected",&frame,Some(&latest),reading,&ocr_attempts,&crops,serde_json::json!({"areas":diff}));
                    status("화면 변경 감지","분석 중 카드나 수치가 바뀌어 다시 읽습니다.","이전 추천 숨김");send(ToUi::Clear);
                    return RetryReason::Animating;
                }
            }
            other => {
                pipe.invalidate();watch.reset();
                diagnostics::event(scan_id,"fresh_capture_failed",serde_json::json!({"error":other.err().map(|e|e.to_string())}));
                status("캡처 연결 대기","게임 창을 복원하면 다시 인식합니다.","캡처 확인 실패");send(ToUi::Clear);
                return RetryReason::Uncertain;
            }
        }
    }
    if pipe.resumed {
        note("the Vorax screen opened again on the game under way: it goes on (instruments kept)");
    }
    let kind = match &outcome {
        Outcome::Advice { .. } => "board",
        Outcome::Animating => "animating",
        Outcome::NotGame => "notgame",
        Outcome::Uncertain { .. } => "uncertain",
        Outcome::Closing => "closing",
    };
    let retry_reason = match &outcome {
        Outcome::Animating => RetryReason::Animating,
        Outcome::Uncertain { .. } => RetryReason::Uncertain,
        _ => RetryReason::None,
    };
    match outcome {
        Outcome::Advice { reading, advice, plan, read_ms, cached } => {
            let title = plan.pill.as_ref().map_or("추천 없음", |p| p.main.as_str());
            let detail = plan.pill.as_ref().map_or("", |p| p.sub.as_str());
            let calculation = if cached { "계산 재사용".to_owned() } else { format!("계산 {} ms", advice.ms) };
            status(title, detail, &format!("{}라운드 / 캡처 {grabbed} ms / 인식 {read_ms:.0} ms / {calculation}", reading.state.round.unwrap_or(1)));
            let mv = plan.pill.as_ref().map_or("(nothing to advise)".to_string(), |p| format!("{} · {}", p.main, p.sub));
            note(&format!(
                "{} {}x{}: picture {grabbed} ms, read {read_ms:.0} ms, advice {} ms → {mv}; board {}",
                trigger.unwrap_or("manual"),
                frame.width,
                frame.height,
                advice.ms,
                serde_json::to_string(&reading.state).unwrap_or_default()
            ));
            let (w, h) = vorax_scan::window::windowed_game(&frame).map_or((frame.width, frame.height), |a| (a.w as usize, a.h as usize));
            send(ToUi::Show(plan, w, h));
        }
        // the board changed and a card is still moving: the marks stay hidden (the pill dimmed) while it looks again,
        // rather than the old ones blinking back between looks (2-3 flashes while long effects play out)
        Outcome::Uncertain { reading } => {
            let fields = reading.blocking_fields();
            note(&format!("{}: uncertain fields {}; reading {}", trigger.unwrap_or("manual"), fields.join(", "), serde_json::to_string(&reading).unwrap_or_default()));
            let detail = fields.iter().map(|field| field_label(field)).collect::<Vec<_>>().join(", ");
            if let Some(plan)=pipe.hover_plan() {
                let size=reading.frame.as_ref().map_or((frame.width,frame.height),|f|(f.width,f.height));
                status("확대 스캔 요청",&format!("파란 카드에 마우스를 올려 주세요. 확인할 항목: {detail}"),"좌측 하단 확대 카드 인식 대기");
                send(ToUi::Show(plan,size.0,size.1));
            } else {
                status("인식 확인 필요",&format!("확인할 항목: {detail}. Ctrl+Alt+X로 재시도할 수 있습니다."),"추천 보류");send(ToUi::Clear);
            }
        }
        Outcome::Animating => {
            if pipe.hover_dirty() {
                status("확대 인식 완료","마우스를 카드 밖으로 옮기면 추천을 갱신합니다.","확대 결과 유지 중");
                if let Some(plan)=pipe.hover_plan() {send(ToUi::Show(plan,frame.width,frame.height));} else {send(ToUi::Clear);}
                return RetryReason::None;
            }
            note(&format!("{}: picture {grabbed} ms, a card was flying off: looking again", trigger.unwrap_or("manual")));
        }
        Outcome::Closing => {
            status("수술 준비 완료", "다음 판을 기다립니다.", "완료");
            note(&format!("{}: the game closing after the protocols, not a board", trigger.unwrap_or("manual")));
            send(ToUi::Clear);
        }
        Outcome::NotGame => {
            status("수술 준비 화면 대기", "수술 준비 화면에 들어가면 다시 인식합니다.", "블러드 러스트 대기");
            note(&format!("{}: not the Surgical Preparation screen", trigger.unwrap_or("manual")));
            send(ToUi::Clear);
        }
    }
    if let Some(dir) = frames {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let path = dir.join(format!("{ms}-{}-{kind}.png", trigger.unwrap_or("manual")));
        let saved = std::fs::create_dir_all(dir).map_err(|e| e.to_string()).and_then(|_| crate::pipeline::encode_png(&path, frame.width as u32, frame.height as u32, &frame.data));
        if let Err(e) = saved {
            note(&format!("frame not saved: {e}"));
        }
    }
    retry_reason
}

fn field_label(field: &str) -> String {
    match field {
        "round" => "라운드".into(),
        "refreshes" => "새로고침 횟수".into(),
        "hand" => "카드 선택지".into(),
        f if f.starts_with("hand") => f[4..].parse::<usize>().map_or("카드 이름".into(), |n| format!("{}번 카드", n + 1)),
        f if f.starts_with("tank") => {
            let (n, stat) = f[4..].split_once('.').unwrap_or(("", ""));
            let part = if stat == "activity" { "활성" } else { "수량" };
            n.parse::<usize>().map_or("탱크 수치".into(), |n| format!("{}번 탱크 {part}", n + 1))
        }
        _ => "카드 이름".into(),
    }
}


/// Local IPC snapshot for the companion window; no network service is involved.
fn status(title: &str, detail: &str, state: &str) {
    let data = serde_json::json!({ "title": title, "detail": detail, "status": state });
    let _ = std::fs::write(super::sys::data_dir().join("status.json"), data.to_string());
}
