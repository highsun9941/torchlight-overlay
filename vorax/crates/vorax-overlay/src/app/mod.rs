//! The live overlay on Windows: one window thread (the overlay, tray icon and hotkeys) and one worker thread
//! (the log, the pictures, the reading and the advice).

mod capture;
mod diagnostics;
mod menu;
mod logview;
pub(crate) mod ocr;
mod sys;
mod update;
mod window;
mod worker;

use crate::arg;
use std::path::PathBuf;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

pub fn run(args: &[String]) {
    // Windows' shared COM area (the MTA) kept alive for the whole run, so a short-lived thread that used it can't
    // take it down under the factories the windows crate keeps cached
    unsafe {
        let _ = windows::Win32::System::Com::CoIncrementMTAUsage();
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // a crash's message into the debug log (a release build has no console to show it)
    std::panic::set_hook(Box::new(|info| {
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        sys::note(&format!("CRASH: {msg}{at}"));
    }));
    // a release build has no console of its own: print into the one it was started from
    if args.iter().any(|a| a == "--selftest" || a == "--console" || a == "--demo") {
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--demo") {
        return demo(args.get(i + 1).filter(|a| !a.starts_with("--")).map(PathBuf::from), arg(args, "--seconds").unwrap_or(15));
    }
    // the runs window drawn from the debug log into a picture
    if let Some(i) = args.iter().position(|a| a == "--runs-preview") {
        let Some(out) = args.get(i + 1) else { return };
        let Some(fonts) = crate::draw::Fonts::system() else { return };
        let log = std::fs::read_to_string(sys::data_dir().join("overlay.log")).unwrap_or_default();
        let pm = logview::preview(&log, std::rc::Rc::new(fonts), 1.0, 760.0);
        let mut rgba = pm.data().to_vec();
        for p in rgba.chunks_exact_mut(4) {
            let a = p[3] as f32 / 255.0;
            for c in 0..3 {
                p[c] = (p[c] as f32 + 70.0 * (1.0 - a)).round() as u8;
            }
            p[3] = 255;
        }
        let _ = crate::pipeline::encode_png(std::path::Path::new(out), pm.width(), pm.height(), &rgba);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--menu-preview") {
        let Some(out) = args.get(i + 1) else { return };
        let Some(fonts) = crate::draw::Fonts::system() else { return };
        let rows = vec![
            menu::Row::Header { title: "블러드 러스트".into(), status: format!("v{} 쨌 odds of {}", update::current(), vorax::data().odds) },
            menu::Row::Warn("게임 로그 꺼짐 / 수동 인식만 가능".into()),
            menu::Row::Action { id: 0, label: "게임 로그 켜는 방법".into(), shortcut: String::new() },
            menu::Row::Separator,
            menu::Row::Switch { id: 0, label: "오버레이 표시".into(), on: true },
            menu::Row::Action { id: 0, label: "지금 인식".into(), shortcut: "Ctrl+Alt+X".into() },
            menu::Row::Levels { base: 0, level: 6, hint: |l| format!("{} instrument rerolls", crate::pipeline::shelley_rerolls(l)) },
            menu::Row::Colors { base: 0, selected: 0 },
            menu::Row::Separator,
            menu::Row::Action { id: 0, label: "플레이 기록".into(), shortcut: "로컬 기록".into() },
            menu::Row::Separator,
            menu::Row::Action { id: 0, label: "종료".into(), shortcut: "Ctrl+Alt+Q".into() },
        ];
        // the menu as it usually is: the game's log on
        let plain = args.iter().any(|a| a == "--plain");
        let rows = if plain {
            let mut head = vec![
                menu::Row::Header { title: "블러드 러스트".into(), status: format!("v{} 쨌 odds of {}", update::current(), vorax::data().odds) },
                menu::Row::Text("게임 로그 연결됨 / 자동 인식".into()),
            ];
            head.extend(rows.into_iter().skip(3));
            head
        } else {
            rows
        };
        // the steps shown when the game's log is off
        let rows = if args.iter().any(|a| a == "--log-card") {
            vec![
                menu::Row::Header { title: "게임 로그 켜기".into(), status: "로그를 켜면 자동 인식할 수 있습니다.".into() },
                menu::Row::Text("1. 게임에서 Esc를 누르세요.".into()),
                menu::Row::Text("2. 계정 관련 설정을 여세요.".into()),
                menu::Row::Text("3. 로그 활성화를 켜세요.".into()),
                menu::Row::Text("4. 재접속하세요.".into()),
                menu::Row::Text("Ctrl+Alt+X로 수동 인식할 수 있습니다.".into()),
                menu::Row::Buttons { primary: (0, "확인".into()), secondary: None },
            ]
        } else {
            rows
        };
        let hover = if plain { None } else { Some((6, Some(4))) };
        let pm = menu::preview(rows, std::rc::Rc::new(fonts), 1.5, hover);
        // onto a mid-grey, as a desktop would show it
        let mut rgba = pm.data().to_vec();
        for p in rgba.chunks_exact_mut(4) {
            let a = p[3] as f32 / 255.0;
            for c in 0..3 {
                p[c] = (p[c] as f32 + 70.0 * (1.0 - a)).round() as u8;
            }
            p[3] = 255;
        }
        let _ = crate::pipeline::encode_png(std::path::Path::new(out), pm.width(), pm.height(), &rgba);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        return selftest(args.get(i + 1).map(PathBuf::from).unwrap_or_else(std::env::temp_dir));
    }
    if let Some(i) = args.iter().position(|a| a == "--inspect") {
        if let (Some(input), Some(out)) = (args.get(i + 1), args.get(i + 2)) {
            return inspect_frame(&PathBuf::from(input), &PathBuf::from(out));
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--benchmark-frame") {
        if let (Some(input), Some(out)) = (args.get(i + 1), args.get(i + 2)) {
            return benchmark_frame(&PathBuf::from(input), &PathBuf::from(out), arg(args, "--shelley").unwrap_or(0));
        }
    }
    if args.iter().any(|a| a == "--console") {
        std::env::set_var("VORAX_CONSOLE", "1");
    }
    if args.iter().any(|a| a == "--version") {
        return println!("vorax-overlay {}", update::current());
    }
    unsafe {
        // one overlay at a time (after an update, the one before is still closing: wait for it)
        let waits = if args.iter().any(|a| a == "--after-update") { 50 } else { 0 };
        let mut n = 0;
        loop {
            let h = CreateMutexW(None, true, w!("Local\\TorchlightHelperVoraxKO"));
            if GetLastError() != ERROR_ALREADY_EXISTS {
                break;
            }
            if let Ok(h) = h {
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
            n += 1;
            if n > waits {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        // physical pixels everywhere, so the overlay lines up with the captured picture at any display scaling
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let mut settings = sys::Settings::load();
    if let Some(s) = arg::<i32>(args, "--shelley") {
        settings.shelley = s.clamp(0, 6);
        settings.save();
    }
    if args.iter().any(|a| a == "--no-ocr") {
        settings.ocr = false;
    }
    // The personal build never updates itself from the upstream release channel.
    sys::note(&format!("Vorax overlay v{} started (Intern Shelley level {}, odds of {})", update::current(), settings.shelley, vorax::data().odds));
    let Some(fonts) = crate::draw::Fonts::system() else {
        sys::note("no Segoe UI or Arial font found");
        return;
    };
    let (to_ui, from_worker) = std::sync::mpsc::channel();
    let (to_worker, from_ui) = std::sync::mpsc::channel();
    let waker = match window::create(fonts, from_worker, to_ui.clone(), to_worker, settings.clone(), args.to_vec()) {
        Ok(w) => w,
        Err(e) => {
            sys::note(&format!("overlay window: {e}"));
            return;
        }
    };
    let opts = worker::Options { log: arg::<String>(args, "--log").map(PathBuf::from), shelley: settings.shelley, ocr: settings.ocr, frames: arg::<String>(args, "--frames").map(PathBuf::from) };
    waker.wake();
    let worker = std::thread::spawn(move || worker::run(opts, to_ui, waker, from_ui));
    window::run_loop();
    let _ = worker.join();
    sys::note("Vorax overlay closed");
}

/// Checks each piece on this PC without the overlay: the log, the game window, one picture of it, the text
/// reader on a known card title, and reading the picture. Writes the picture to `out`.
fn inspect_frame(input: &std::path::Path, out: &std::path::Path) {
    unsafe { let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED); }
    let frame = crate::pipeline::decode_png(input).expect("read diagnostic screenshot");
    let img = vorax_scan::window::game_view(&frame).map_or_else(|| frame.clone(), |r| vorax_scan::window::crop_to(&frame, &r));
    std::fs::create_dir_all(out).expect("create diagnostic directory");
    let _ = crate::pipeline::encode_png(&out.join("client.png"), img.width as u32, img.height as u32, &img.data);
    let ocr = ocr::Ocr::new().ok();
    let mut reader = |crop: &vorax_scan::pixels::Img| ocr.as_ref().and_then(|o| o.read(crop).ok()).unwrap_or_default();
    let mut pipeline = crate::pipeline::Pipeline::new(0);
    let result = pipeline.run(&img, None, ocr.as_ref().map(|_| &mut reader as &mut dyn FnMut(&vorax_scan::pixels::Img) -> String));
    let (kind, reading) = match result {
        crate::pipeline::Outcome::Advice { reading, .. } => ("advice", Some(reading)),
        crate::pipeline::Outcome::Uncertain { reading } => ("uncertain", Some(reading)),
        crate::pipeline::Outcome::Animating => ("animating", None),
        crate::pipeline::Outcome::NotGame => ("notgame", None),
        crate::pipeline::Outcome::Closing => ("closing", None),
    };
    let value = serde_json::json!({
        "outcome": kind,
        "ocrLanguage": ocr.as_ref().map(|o| &o.language),
        "blockingFields": reading.as_ref().map(|r| r.blocking_fields()),
        "reading": reading,
    });
    std::fs::write(out.join("reading.json"), serde_json::to_vec_pretty(&value).unwrap()).expect("save diagnostic result");
}

fn benchmark_frame(input: &std::path::Path, out: &std::path::Path, shelley: i32) {
    unsafe { let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED); }
    let img = crate::pipeline::decode_png(input).unwrap();
    let ocr = ocr::Ocr::new().unwrap();
    let mut pipe = crate::pipeline::Pipeline::new(shelley);
    let mut rows = vec![];
    for n in 0..5 {
        let start = std::time::Instant::now();
        let mut reader = |crop: &vorax_scan::pixels::Img| ocr.read(crop).unwrap_or_default();
        let crate::pipeline::Outcome::Advice { read_ms, advice, cached, .. } = pipe.run(&img, None, Some(&mut reader)) else { panic!("benchmark requires a readable board") };
        rows.push(serde_json::json!({ "run": n, "read_ms": read_ms, "advice_ms": advice.ms, "cached": cached, "total_ms": start.elapsed().as_secs_f64() * 1000.0, "best": advice.best }));
    }
    std::fs::write(out, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
}

fn selftest(out: PathBuf) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    // the displays' colour mode: HDR changes the colours a capture gets
    unsafe {
        use windows::core::Interface;
        use windows::Win32::Graphics::Dxgi::*;
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut a = 0;
            while let Ok(adapter) = factory.EnumAdapters1(a) {
                let mut o = 0;
                while let Ok(output) = adapter.EnumOutputs(o) {
                    if let Ok(o6) = output.cast::<IDXGIOutput6>() {
                        if let Ok(d) = o6.GetDesc1() {
                            let name = String::from_utf16_lossy(&d.DeviceName).trim_end_matches('\0').to_string();
                            let hdr = d.ColorSpace == windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
                            println!("display {name}: {}x{}, HDR {}, colour space {}, max {} nits", d.DesktopCoordinates.right - d.DesktopCoordinates.left, d.DesktopCoordinates.bottom - d.DesktopCoordinates.top, if hdr { "ON" } else { "off" }, d.ColorSpace.0, d.MaxLuminance);
                        }
                    }
                    o += 1;
                }
                a += 1;
            }
        }
    }
    println!("log: {}", sys::find_log().map_or("not found".into(), |p| p.display().to_string()));
    let game = sys::find_game_window();
    println!("game window: {}", game.map_or("not open".into(), |g| format!("{:?}, picture at {:?}, in front: {}", g, sys::client_rect(g), sys::foreground() == g)));
    match ocr::Ocr::new() {
        Ok(o) => {
            println!("OCR language: {}", o.language);
            let frame = crate::pipeline::decode_png(&PathBuf::from("fixtures/frames/board-1080p.png"));
            match frame {
                Ok(f) => {
                    let l = vorax_scan::layout::layout(f.width, f.height);
                    for (j, c) in l.cards.iter().enumerate() {
                        let crop = vorax_scan::titles::title_crop(&f, c, l.k);
                        let t = std::time::Instant::now();
                        let text = o.read(&crop).unwrap_or_else(|e| format!("error {e}"));
                        println!("OCR ({}) card {j}: {:?} in {} ms", o.language, text.lines().next().unwrap_or(""), t.elapsed().as_millis());
                    }
                }
                Err(e) => println!("OCR: no test frame ({e})"),
            }
        }
        Err(e) => println!("OCR: not available ({e})"),
    }
    let Some(game) = game else { return };
    let cap = match capture::Capturer::new() {
        Ok(c) => c,
        Err(e) => return println!("capture: {e}"),
    };
    let t = std::time::Instant::now();
    match cap.grab(game) {
        Ok(Some(img)) => {
            println!("capture: {}x{} in {} ms", img.width, img.height, t.elapsed().as_millis());
            let path = out.join("selftest-capture.png");
            let _ = crate::pipeline::encode_png(&path, img.width as u32, img.height as u32, &img.data);
            println!("saved {}", path.display());
            let mut p = crate::pipeline::Pipeline::new(0);
            let ocr = ocr::Ocr::new().ok();
            let mut read_title = |crop: &vorax_scan::pixels::Img| ocr.as_ref().and_then(|o| o.read(crop).ok()).unwrap_or_default();
            match p.run(&img, None, Some(&mut read_title)) {
                crate::pipeline::Outcome::Advice { advice, plan, read_ms, .. } => println!("read in {read_ms:.0} ms, advice in {} ms: {:?}", advice.ms, plan.pill.map(|p| p.main)),
                crate::pipeline::Outcome::Animating => println!("read: a card is moving"),
                crate::pipeline::Outcome::Uncertain { .. } => println!("read: uncertain fields; advice withheld"),
                crate::pipeline::Outcome::NotGame => println!("read: not the Surgical Preparation screen (expected outside Vorax)"),
                crate::pipeline::Outcome::Closing => println!("read: the game closing after round 13"),
            }
        }
        Ok(None) => println!("capture: no frame"),
        Err(e) => println!("capture: {e}"),
    }
}

/// The overlay with a saved board's advice, over the game for 15 seconds (switch to the game to see it): checks
/// that it lines up on this screen without playing a Vorax round.
fn demo(frame: Option<PathBuf>, seconds: u64) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let path = frame.unwrap_or_else(|| PathBuf::from("upstream/tests/fixtures/frames/board-1080p.png"));
    let img = match crate::pipeline::decode_png(&path) {
        Ok(i) => i,
        Err(e) => return println!("{e}"),
    };
    let mut p = crate::pipeline::Pipeline::new(6);
    let crate::pipeline::Outcome::Advice { plan, .. } = p.run(&img, None, None) else { return println!("{}: no advice on that picture", path.display()) };
    println!("demo: {} over the game for {seconds} s", plan.pill.as_ref().map_or(String::new(), |p| format!("{} 쨌 {}", p.main, p.sub)));
    let Some(fonts) = crate::draw::Fonts::system() else { return };
    let (to_ui, from_worker) = std::sync::mpsc::channel();
    let (to_worker, _from_ui) = std::sync::mpsc::channel();
    let Ok(waker) = window::create(fonts, from_worker, to_ui.clone(), to_worker, sys::Settings { shown: true, ..sys::Settings::default() }, vec![]) else { return };
    let _ = to_ui.send(window::ToUi::Show(plan, img.width, img.height));
    waker.wake();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        let _ = to_ui.send(window::ToUi::Quit);
        waker.wake();
    });
    window::run_loop();
}
