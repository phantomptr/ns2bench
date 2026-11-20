//! Everything built on video + input: OCR, screen log, probe/calibration, routes, tests,
//! recorder + clips, audio, doctor, diagnostics and the report bundle.

use crate::dvr;
use crate::ocr::{self, Engine};
use crate::server::{bad, source_of, strs, ApiError, R, S};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What the consoles share: the OCR engine. (Each console has its own recorder, sound and
/// menu profile: crate::console::Console.)
pub struct Ext {
    pub ocr: Option<Engine>,
}

impl Ext {
    pub fn new(cfg: &crate::Config) -> Ext {
        Ext { ocr: ocr::make_engine(cfg.raw.get("ocr")) }
    }
}

pub fn start(b: Arc<crate::Bench>) {
    tokio::spawn(crate::shared_net::watch(b.clone()));
    tokio::spawn(watch_controller(b));
}

/// One console's background work: its recorder, its sound, its picture's health, its screen log.
/// Each runs with that console selected, so everything it calls works on that console.
pub fn start_console(b: Arc<crate::Bench>, c: Arc<crate::console::Console>) {
    let tasks = vec![
        tokio::spawn(c.dvr.clone().run(c.frames.clone())),
        tokio::spawn(c.audio.clone().run(c.video_cfg.clone(), b.logs.clone())),
        tokio::spawn(crate::console::SELECTED.scope(Some(c.id.clone()), watch_health(b.clone(), c.clone()))),
        tokio::spawn(crate::console::SELECTED.scope(Some(c.id.clone()), watch_screen(b.clone(), c.clone()))),
        tokio::spawn(watch_power(b, c.clone())),
    ];
    // owned by the console: removing it stops all of them
    c.tasks.lock().unwrap().extend(tasks);
}

/// Is the console on or asleep? Told by its picture: a sleeping console sends no picture, and its
/// capture card then shows black or its own "No Signal" screen (frames still come). Dark for 8
/// seconds is asleep (a fade to black in a game is shorter); any picture is on. A console that is
/// switched off looks the same as one asleep. Every 2 seconds, from a small grey copy of the
/// latest frame (no colour, no full decode). The controller's console's state also goes to the
/// Bridge connection, which then knows a lost Bridge is the console sleeping, not a fault.
async fn watch_power(b: Arc<crate::Bench>, c: Arc<crate::console::Console>) {
    let mut dark_since: Option<std::time::Instant> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let now = std::time::Instant::now();
        let current = c.power.lock().unwrap().0;
        let state = match c.frames.latest().filter(|f| f.at.elapsed() < Duration::from_secs(3)) {
            None => { dark_since = None; "unknown" }
            Some(f) => match crate::calib::small(&f).await {
                Ok(img) if crate::power::dark(&img) => {
                    let d = *dark_since.get_or_insert(now);
                    if now.duration_since(d) >= Duration::from_secs(8) { "asleep" } else { current }
                }
                Ok(_) => { dark_since = None; "on" }
                Err(_) => current,
            },
        };
        if state != current {
            *c.power.lock().unwrap() = (state, now);
            match (current, state) {
                (_, "asleep") => b.logs.add("bench", format!("{}: asleep (no picture from the console)", c.name())),
                ("asleep", "on") => b.logs.add("bench", format!("{}: awake", c.name())),
                _ => {}
            }
        }
        // the Bridge's console: is it asleep (since when)?
        if b.controller_console().is_some_and(|x| x.id == c.id) {
            let since = c.power.lock().unwrap().1;
            b.hub.backend.status.lock().unwrap().console_asleep = (state == "asleep").then(|| (c.name(), since));
        }
    }
}

/// The controller's USB cable moved: with several consoles, which one it's on now isn't known, so
/// pause input and ask. Two signs: the board saw its USB cable out ("not attached", not the
/// "suspended" of a console asleep) and then in again; or the board itself came back (moving its
/// cable can restart a board the console powers).
async fn watch_controller(b: Arc<crate::Bench>) {
    let mut was_connected: Option<bool> = None;
    let mut unplugged = false;
    let mut down_since: Option<std::time::Instant> = None;
    let mut detector: Option<crate::detect::Detector> = None;
    let mut gave_up = false;
    // the board gone while a console is on (its picture live): worth one notification an outage
    let mut away_since: Option<std::time::Instant> = None;
    let mut told_away = false;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        detect_step(&b, &mut detector, &mut gave_up);
        let st = b.hub.status();
        let connected = st["connected"] == true;
        let usb = st["usb"].as_str().unwrap_or("");
        let uses_board = st["backend"].as_str().is_some_and(|k| k != "none");
        if connected || !uses_board {
            away_since = None;
            told_away = false;
        } else {
            let since = *away_since.get_or_insert_with(std::time::Instant::now);
            if !told_away && since.elapsed() >= Duration::from_secs(20) {
                // said once an outage: a console really on (a picture, not "No Signal": the card
                // keeps sending frames while the console sleeps) with the board gone is a problem.
                // Looked at again every 20 s: the console may wake while the board stays away.
                away_since = Some(std::time::Instant::now());
                let mut console_on = false;
                for c in b.list() {
                    if crate::power::showing_picture(&b, &c.id).await { console_on = true; break; }
                }
                if !console_on { continue; }
                told_away = true;
                b.logs.notify("bench", "warn", "Bridge not answering",
                    "NS2Bench Bridge: no answer for 20 s while a console is on. Check the board's network cable and its USB cable to the console (it powers the board)");
            }
        }
        // (a switch just finished, or the board just connected: is it what its console needs?)
        if connected && b.hub.mode_check.swap(false, std::sync::atomic::Ordering::SeqCst) && !b.hub.switching_mode() { b.apply_mode(); }
        let mut moved = false;
        if was_connected == Some(true) && !connected { down_since = Some(std::time::Instant::now()); }
        if was_connected == Some(false) && connected {
            // The board reconnected: anything could have happened meanwhile, unless the person
            // already said where it is while it was away (moved the cable, then clicked).
            // ...told while it was away, and only just: the person moved the cable and said so a
            // moment ago. An old answer (the board away for minutes, the cable maybe moved again
            // since) proves nothing: find out again.
            let told = matches!((*b.hub.placed.lock().unwrap(), down_since), (Some(p), Some(d))
                if p >= d && p.elapsed() < Duration::from_secs(60) && d.elapsed() < Duration::from_secs(90));
            // back from our own mode switch: make sure it's the mode its console needs (the
            // person may have placed it while a detection switch was under way)
            if told || b.hub.switching_mode() { b.apply_mode(); } else { moved = true; }
            down_since = None;
        }
        if connected {
            if usb == "not attached" { unplugged = true; }
            else if unplugged && usb.starts_with("configured") { unplugged = false; moved = true; }
        }
        // first sight of the board since start, or one console's board back: the mode its console needs
        if connected && (was_connected.is_none() || (moved && b.list().len() == 1)) { b.apply_mode(); }
        was_connected = Some(connected);
        // The board's own word wins: a Switch 2 started it as its Pro Controller, so it is in that
        // Switch 2, whatever we were told. If we have it on a console that isn't a Switch 2, we're
        // wrong: say so, and find out (detection settles it at once from the handshake).
        if connected && st["host"] == "switch2" && b.list().len() > 1 {
            if let Some(c) = b.controller_console() {
                if *c.model.lock().unwrap() != "switch2" {
                    let _ = b.set_controller_on(None);
                    b.logs.notify("bench", "warn", "The controller is in a Switch 2",
                        format!("controller: a Switch 2 is driving the board as its Pro Controller, so it isn't on {}. Finding where it is", c.name()));
                    continue;
                }
            }
        }
        // our own mode switch makes the board drop off and come back: that's not the cable
        if b.hub.switching_mode() { moved = false; unplugged = false; }
        if moved { *b.hub.cable_event.lock().unwrap() = Some(std::time::Instant::now()); }
        if moved && b.list().len() > 1 && b.controller_console().is_some() {
            // The person picked another console on screen while the cable was still in this one,
            // and now the cable moved: that is where it went.
            if let Some(c) = b.take_pending_follow() {
                if b.set_controller_on(Some(&c.id)).is_ok() {
                    b.logs.add("bench", format!("controller: its cable moved; now on {} (the console picked on screen)", c.name()));
                    continue;
                }
            }
            // Following the console on screen: the controller is where the person pointed it
            // (the board restarting is the console's power, not the cable moving). The Switch 2
            // check below still corrects it if a Switch 2 turns out to be driving it.
            if let Some(a) = b.active_console() {
                if b.controller_console().is_some_and(|c| c.id == a.id) {
                    // (picked on screen, and then the cable was seen to move or the board came
                    // back: from here on it counts as being there)
                    *b.hub.placed.lock().unwrap() = Some(std::time::Instant::now());
                    b.hub.placed_sure.store(true, std::sync::atomic::Ordering::SeqCst);
                    b.apply_mode();
                } else {
                    let _ = b.set_controller_on(Some(&a.id));
                }
                b.logs.add("bench", format!("controller: the board came back; it stays on {} (the console picked on screen)", a.name()));
                continue;
            }
            let _ = b.set_controller_on(None);
            b.logs.notify("bench", "warn", "Which console is the controller on?",
                "controller: its USB cable (or the board) was unplugged and came back. Input is paused: pick the console it's plugged into now (under the picture)");
        }
    }
}

/// While the controller's place is unknown, find it by itself when the consoles allow it (exactly
/// one Switch 2 and one other console): see crate::detect. Never sends input.
fn detect_step(b: &Arc<crate::Bench>, detector: &mut Option<crate::detect::Detector>, gave_up: &mut bool) {
    use crate::detect::{Detector, Seen, Step};
    use std::sync::atomic::Ordering::Relaxed;
    let list = b.list();
    let (twos, others): (Vec<_>, Vec<_>) = list.iter().partition(|c| *c.model.lock().unwrap() == "switch2");
    // (an older bridge doesn't say whether a Switch 2 started it: without that, nothing can be told)
    let reports = b.hub.status()["reports_host"] == true;
    let unknown = list.len() > 1 && b.controller_console().is_none() && reports;
    // The one Switch 2 saying "I started this controller" settles it, however many other consoles
    // there are, and also after detection gave up (a console asleep for an hour, then woken).
    if unknown && twos.len() == 1 && b.hub.status()["host"] == "switch2" && b.hub.status()["connected"] == true {
        let c = twos[0].clone();
        if detector.take().is_some() { b.detecting.store(false, Relaxed); }
        *gave_up = false;
        if b.set_controller_on(Some(&c.id)).is_ok() {
            b.logs.notify("bench", "info", &format!("Controller found on {}", c.name()),
                format!("controller: found on {} (the Switch 2 started it as its Pro Controller). Wrong? Pick the right console under the picture", c.name()));
        }
        return;
    }
    let can = unknown && twos.len() == 1 && others.len() == 1;
    if !can {
        if detector.take().is_some() { b.detecting.store(false, Relaxed); }
        // placed (by the person or by us): the next unknown place may be detected again
        if b.controller_console().is_some() { *gave_up = false; }
        return;
    }
    if *gave_up { return; }
    let st = b.hub.status();
    let seen = Seen {
        connected: st["connected"] == true,
        mode: st["usb_mode"].as_str().map(str::to_string),
        usb: st["usb"].as_str().unwrap_or("").to_string(),
        switch2: st["host"] == "switch2",
    };
    let now = std::time::Instant::now();
    let d = detector.get_or_insert_with(|| {
        b.logs.add("bench", "controller: finding which console it's plugged into (no buttons are pressed)");
        Detector::new(now)
    });
    b.detecting.store(true, Relaxed);
    match d.step(now, &seen) {
        Step::Wait => {}
        Step::TryPro => {
            b.logs.add("bench", "controller: a console is there; trying the Pro Controller for a moment to see if it's the Switch 2");
            let _ = b.hub.switch_usb_mode_as("pro", false);     // a quick look: not the boot mode
        }
        Step::Found { switch2, why } => {
            let c = if switch2 { twos[0].clone() } else { others[0].clone() };
            *detector = None;
            b.detecting.store(false, Relaxed);
            if b.set_controller_on(Some(&c.id)).is_ok() {
                b.logs.notify("bench", "info", &format!("Controller found on {}", c.name()),
                    format!("controller: found on {} ({why}). Wrong? Pick the right console under the picture", c.name()));
            }
        }
        Step::GiveUp => {
            *detector = None;
            *gave_up = true;
            b.detecting.store(false, Relaxed);
            b.logs.warn("bench", "controller: couldn't tell which console it's on (asleep, or the cable not in?). Pick it under the picture");
        }
    }
}

/// Watch the picture and say when it stops, comes back or runs slow, in plain words.
async fn watch_health(b: Arc<crate::Bench>, c: Arc<crate::console::Console>) {
    let mut stalled = false;
    let mut slow_since: Option<std::time::Instant> = None;
    let mut slow_logged = false;
    let mut seen_frame = false;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        // with several consoles, say which one
        let who = if b.list().len() > 1 { format!("{}: ", c.name()) } else { String::new() };
        let v = c.frames.status();
        let Some(age) = v["age_s"].as_f64() else { continue };
        if !seen_frame { seen_frame = true; b.logs.add("bench", format!("{who}video: first picture received")); }
        if age > 5.0 && !stalled {
            stalled = true;
            b.logs.notify("bench", "warn", "No picture", format!("{who}video: no new picture for {age:.0} s. The console may be asleep, or the HDMI cable or capture card was unplugged; it comes back by itself"));
        } else if age < 1.0 && stalled {
            stalled = false;
            b.logs.notify("bench", "info", "Picture is back", format!("{who}video: the picture is back"));
        }
        let want = c.video_cfg.lock().unwrap()["fps"].as_f64().unwrap_or(30.0);
        let fps = v["fps"].as_f64().unwrap_or(0.0);
        if !stalled && fps > 0.0 && fps < want * 0.5 {
            let since = *slow_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed() > Duration::from_secs(10) && !slow_logged {
                slow_logged = true;
                b.logs.warn("bench", format!("{who}video is slow: {fps:.0} of {want:.0} fps. Close other apps using the capture card or CPU, or pick a lower resolution or FPS in the Video panel"));
            }
        } else {
            if slow_logged { b.logs.add("bench", format!("{who}video speed is back to normal ({fps:.0} fps)")); }
            slow_since = None;
            slow_logged = false;
        }
    }
}

pub fn status(b: &crate::Bench, target: Option<&crate::console::Console>, s: &mut Value) {
    s["ocr"] = json!({"engine": b.ext.ocr.map(|e| e.name()), "screen_log": b.screen_log.load(Ordering::Relaxed)});
    if let Some(c) = target {
        s["dvr"] = c.dvr.status();
        s["audio"] = c.audio.status();
    }
}

/// Screen as a log source: text that newly appears on screen becomes a 'screen' log line.
async fn watch_screen(b: Arc<crate::Bench>, c: Arc<crate::console::Console>) {
    let mut recent: Vec<HashSet<String>> = vec![];
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if !b.screen_log.load(Ordering::Relaxed) || b.ext.ocr.is_none() || c.frames.latest().is_none() {
            recent.clear();
            continue;
        }
        let lines = match b.read_screen(None, true).await {
            Ok(l) => l,
            Err(e) => {
                b.logs.error("bench", format!("screen log: {e}"));
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let norm = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
        let now: HashSet<String> = lines.iter().map(|l| norm(&l.text)).filter(|t| t.trim().len() > 2).collect();
        let seen: HashSet<&String> = recent.iter().flatten().collect();
        // lines are already in reading order
        let mut emitted = HashSet::new();
        for l in &lines {
            let t = norm(&l.text);
            if now.contains(&t) && !seen.contains(&t) && emitted.insert(t.clone()) {
                b.logs.add("screen", if b.list().len() > 1 { format!("{}: {t}", c.name()) } else { t });
            }
        }
        recent.push(now);
        if recent.len() > 3 {
            recent.remove(0);
        }
    }
}

pub fn routes(r: Router<S>) -> Router<S> {
    r.route("/api/ocr", get(get_ocr))
        .route("/api/wait_text", post(post_wait_text))
        .route("/api/where", get(get_where))
        .route("/api/routes", get(get_routes))
        .route("/api/navigate", post(post_navigate))
        .route("/api/probe", post(post_probe))
        .route("/api/calibrate", post(post_calibrate))
        .route("/api/test", post(post_test))
        .route("/api/clip", post(post_clip))
        .route("/api/clips", get(get_clips))
        .route("/clips/{name}", get(get_clip_file))
        .route("/api/doctor", get(get_doctor).post(post_doctor))
        .route("/api/diagnostics", get(get_diagnostics))
        .route("/api/bundle", get(get_bundle))
        .route("/api/wait_screen", post(post_wait_screen))
        .route("/api/find_text", get(get_find_text))
        .route("/api/health", get(get_health))
        .route("/api/reports", get(get_reports))
        .route("/api/prefs", get(get_prefs).post(post_prefs))
        .route("/api/power", get(crate::power::get_power).post(crate::power::post_power))
        .route("/api/power/settings", post(crate::power::set_power))
        .route("/api/changelog", get(|| async { ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], CHANGELOG) }))
        .route("/api/serial_ports", get(|| async { Json(json!({"ports": tokio::task::spawn_blocking(serial_ports).await.unwrap_or_default()})) }))
}

/// Wait for the screen to react ("change") or to settle ("stable").
async fn post_wait_screen(State(b): State<S>, Json(v): Json<Value>) -> R<Json<Value>> {
    let until = v["until"].as_str().unwrap_or("change");
    let timeout = Duration::from_millis(v["timeout_ms"].as_u64().unwrap_or(5000).clamp(100, 120_000));
    let stable_ms = v["stable_ms"].as_u64().unwrap_or(500).clamp(100, 10_000);
    let region = ocr::parse_region(&v["region"])?;
    Ok(Json(crate::calib::wait_screen(&b.target()?.frames, until, timeout, stable_ms, region).await
        .map_err(|e| if e.to_string().contains("no live video") { unavailable(e) } else { bad(e.to_string()) })?))
}

/// Every on-screen line that matches, with its box: where to look, or which item is highlighted.
async fn get_find_text(State(b): State<S>, Query(q): Query<HashMap<String, String>>) -> R<Json<Value>> {
    // ?regex=pattern or ?text=pattern&regex=true
    let flag = matches!(q.get("regex").map(String::as_str), Some("1" | "true")) && q.contains_key("text");
    let regex = flag || q.get("regex").is_some_and(|r| !r.is_empty());
    let pattern = if flag { q.get("text") } else { q.get("regex").filter(|r| !r.is_empty()).or(q.get("text")) }.cloned().unwrap_or_default();
    if pattern.is_empty() {
        return Err(bad("text or regex required"));
    }
    if regex { regex::Regex::new(&pattern).map_err(|e| bad(format!("invalid regex: {e}")))?; }
    let region = ocr::parse_region(&q.get("region").map(|s| Value::String(s.clone())).unwrap_or(Value::Null))?;
    let fast = matches!(q.get("fast").map(String::as_str), Some("1" | "true"));
    let lines = b.read_screen(region, fast).await.map_err(unavailable)?;
    let hits: Vec<&ocr::Line> = lines.iter().filter(|l| ocr::matches(std::slice::from_ref(*l), &pattern, regex).is_some()).collect();
    Ok(Json(json!({"found": !hits.is_empty(), "matches": hits, "count": hits.len()})))
}

/// One cheap call that says whether NS2Bench is ready to drive, and if not, why and what to do.
async fn get_health(State(b): State<S>) -> Json<Value> {
    Json(health(&b))
}

pub fn health(b: &crate::Bench) -> Value {
    let st = b.status();
    let mut problems: Vec<Value> = vec![];
    let video_ok = st["video"]["age_s"].as_f64().is_some_and(|a| a < 5.0);
    if !video_ok {
        let why = st["video"]["error"].as_str().unwrap_or("no picture yet").to_string();
        problems.push(json!({"area": "video", "problem": why,
            "fix": crate::logs::hint_for("bench", "no video data").unwrap_or_default()}));
    }
    let backend = st["input"]["backend"].as_str().unwrap_or("none");
    let input_ok = backend != "none" && st["input"]["connected"].as_bool().unwrap_or(false);
    if backend == "none" {
        problems.push(json!({"area": "input", "problem": "no input device is set up, so nothing can press buttons",
            "fix": "Pick one in Settings → Devices (the NS2Bench Bridge, a Pico or a CH9329)."}));
    } else if !input_ok {
        let why = st["input"]["detail"].as_str().unwrap_or("not connected").to_string();
        problems.push(json!({"area": "input", "problem": why.clone(),
            "fix": crate::logs::hint_for("bench", &format!("bridge {why}")).unwrap_or("Check the input device is plugged in and powered, and its settings in Settings → Devices.")}));
    }
    let manual = st["input"]["manual_only"].as_bool().unwrap_or(false);
    if manual {
        problems.push(json!({"area": "input", "problem": "Block agents (manual-only) is on",
            "fix": crate::logs::hint_for("bench", "manual-only").unwrap_or_default()}));
    }
    json!({"ok": problems.is_empty(), "video": video_ok, "input": input_ok, "agents_allowed": !manual,
           "ocr": b.ext.ocr.is_some(), "problems": problems, "version": env!("CARGO_PKG_VERSION")})
}

/// Saved test reports, newest first.
async fn get_reports() -> Json<Value> {
    let dir = crate::testrun::reports_dir();
    let d = dir.clone();
    // (off the async workers, and only the newest 200 are opened: months of runs add up)
    let v = tokio::task::spawn_blocking(move || {
        let mut names: Vec<std::path::PathBuf> = std::fs::read_dir(&d).into_iter().flatten().flatten()
            .map(|e| e.path()).filter(|p| p.is_dir()).collect();
        names.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        names.truncate(200);
        names.into_iter().map(|p| {
            let report: Value = std::fs::read_to_string(p.join("report.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            json!({"name": p.file_name().map(|n| n.to_string_lossy().into_owned()), "path": p, "passed": report["passed"], "test": report["name"]})
        }).collect::<Vec<Value>>()
    }).await.unwrap_or_default();
    Json(json!({"dir": dir, "reports": v}))
}

fn unavailable(e: anyhow::Error) -> ApiError {
    ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string())
}
/// What changed in each version, for the app's "What's new" (the same file as on GitHub).
const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// Serial devices an input board could be on, for the Devices form's suggestions.
fn serial_ports() -> Vec<String> {
    #[cfg(unix)]
    {
        let wanted = |n: &str| ["cu.usb", "cu.wch", "cu.SLAB", "ttyACM", "ttyUSB", "ttyAMA", "ttyS0"].iter().any(|p| n.starts_with(p));
        let mut v: Vec<String> = std::fs::read_dir("/dev").into_iter().flatten().flatten()
            .filter_map(|e| e.file_name().into_string().ok()).filter(|n| wanted(n)).map(|n| format!("/dev/{n}")).collect();
        v.sort();
        v
    }
    #[cfg(windows)]
    {
        // The ports Windows has right now (the same list Device Manager shows).
        let mut c = std::process::Command::new("reg");
        std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000);
        let out = c.args(["query", r"HKLM\HARDWARE\DEVICEMAP\SERIALCOMM"]).output();
        out.map(|o| String::from_utf8_lossy(&o.stdout).lines()
            .filter_map(|l| l.split_whitespace().last().filter(|w| w.starts_with("COM")).map(str::to_string)).collect())
            .unwrap_or_default()
    }
}

/// The page's own preferences (theme, text size, language, notification choices…), kept in
/// ~/.ns2bench/ui.json so they are the same in the app and in a browser, and outlive both.
async fn get_prefs() -> Json<Value> {
    let v: Value = std::fs::read(crate::config::state_dir().join("ui.json")).ok()
        .and_then(|d| serde_json::from_slice(&d).ok()).filter(Value::is_object).unwrap_or(json!({}));
    Json(v)
}

async fn post_prefs(h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    if !crate::server::from_browser(&h) {
        return Err(ApiError(StatusCode::FORBIDDEN, "only the NS2Bench page saves its preferences".into()));
    }
    // Changes only, merged into what is saved: {"ns2bench.key": "value" | null (remove)}.
    // Two open windows then don't undo each other's choices.
    let change = v.as_object().ok_or_else(|| bad("preferences must be an object"))?;
    let ok = change.iter().all(|(k, x)| k.starts_with("ns2bench.") && k.len() <= 80 && (x.is_null() || x.as_str().is_some_and(|x| x.len() <= 2000)));
    if !ok {
        return Err(bad("preferences are short \"ns2bench.*\" text values (null removes one)"));
    }
    let _one = PREFS.lock().await;
    let path = crate::config::state_dir().join("ui.json");
    let mut saved: serde_json::Map<String, Value> = std::fs::read(&path).ok()
        .and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
    for (k, x) in change {
        if x.is_null() { saved.remove(k); } else { saved.insert(k.clone(), x.clone()); }
    }
    if saved.len() > 200 {
        return Err(bad("at most 200 preferences"));
    }
    crate::config::write_json(&path, &Value::Object(saved)).map_err(anyhow::Error::from)?;
    Ok(Json(json!({"ok": true})))
}
static PREFS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn not_manual_only(b: &S) -> R<()> {
    if b.hub.manual_only.load(Ordering::Relaxed) {
        return Err(ApiError(StatusCode::CONFLICT, "manual-only mode is on".into()));
    }
    Ok(())
}

async fn get_ocr(State(b): State<S>, Query(q): Query<HashMap<String, String>>) -> R<Json<Value>> {
    let region = ocr::parse_region(&q.get("region").map(|s| Value::String(s.clone())).unwrap_or(Value::Null))?;
    let fast = matches!(q.get("fast").map(String::as_str), Some("1" | "true"));
    let lines = b.read_screen(region, fast).await.map_err(unavailable)?;
    Ok(Json(json!({"engine": b.ext.ocr.map(|e| e.name()), "text": ocr::join(&lines, "\n"), "lines": lines})))
}

async fn post_wait_text(State(b): State<S>, Json(v): Json<Value>) -> R<Json<Value>> {
    // {"regex": "pattern"} or {"text": "pattern", "regex": true}
    let regex = v["regex"].as_str().is_some() || v["regex"].as_bool() == Some(true);
    let pattern = v["regex"].as_str().or(v["text"].as_str()).unwrap_or("").to_string();
    if pattern.is_empty() {
        return Err(bad("text or regex required"));
    }
    if regex { regex::Regex::new(&pattern).map_err(|e| bad(format!("invalid regex: {e}")))?; }
    let timeout = Duration::from_secs_f64((v["timeout_ms"].as_f64().unwrap_or(10_000.0) / 1000.0).clamp(0.2, 300.0));
    let region = ocr::parse_region(&v["region"])?;
    let gone = v["gone"].as_bool().unwrap_or(false);
    let fast = v["fast"].as_bool().unwrap_or(true);
    let t0 = Instant::now();
    loop {
        let lines = b.read_screen(region, fast).await.map_err(unavailable)?;
        let hit = ocr::matches(&lines, &pattern, regex);
        let ms = t0.elapsed().as_millis() as u64;
        if hit.is_none() == gone {
            return Ok(Json(json!({"found": true, "match": hit, "elapsed_ms": ms, "text": ocr::join(&lines, "\n")})));
        }
        if t0.elapsed() > timeout {
            return Ok(Json(json!({"found": false, "elapsed_ms": ms, "text": ocr::join(&lines, "\n")})));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn get_where(State(b): State<S>) -> R<Json<Value>> {
    Ok(Json(b.where_().await.map_err(unavailable)?))
}

async fn get_routes(State(b): State<S>) -> Json<Value> {
    Json(b.routes())
}

async fn post_navigate(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    Ok(Json(b.navigate(v["route"].as_str().unwrap_or(""), &source_of(&h, &v)).await?))
}

async fn post_probe(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    not_manual_only(&b)?;
    let buttons = strs(&v["buttons"]);
    if buttons.is_empty() {
        return Err(bad("buttons required"));
    }
    let r = crate::calib::probe(&b, &buttons, v["hold_ms"].as_u64().unwrap_or(100), v["timeout_ms"].as_u64().unwrap_or(1500).clamp(100, 10_000), &source_of(&h, &v)).await?;
    b.logs.add("bench", format!("probe {r}"));
    Ok(Json(r))
}

async fn post_calibrate(State(b): State<S>, Json(v): Json<Value>) -> R<Json<Value>> {
    not_manual_only(&b)?;
    b.logs.add("bench", format!("calibration started: {v}"));
    let or = |k: &str, d: &[&str]| { let s = strs(&v[k]); if s.is_empty() { d.iter().map(|x| x.to_string()).collect() } else { s } };
    let holds: Vec<u64> = v["holds"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).filter(|h: &Vec<u64>| !h.is_empty())
        .unwrap_or_else(|| vec![16, 24, 33, 50, 66, 100]);
    // Bounded so one call cannot hold the agent lock for an unbounded time.
    if holds.len() > 12 || holds.iter().any(|&h| h > 1000) {
        return Err(bad("at most 12 hold values, each up to 1000 ms"));
    }
    if v["repeats"].as_u64().is_some_and(|r| r == 0 || r > 20) {
        return Err(bad("repeats must be 1-20"));
    }
    let out = crate::calib::calibrate(&b, &or("forward", &["RIGHT"]), &or("back", &["LEFT"]), &holds,
        v["repeats"].as_u64().unwrap_or(4) as u32).await.map_err(unavailable)?;
    b.hub.min_press_ms.store(crate::calib::clamp_min_press(out["min_press_ms"].as_u64().unwrap_or(crate::calib::DEFAULT_MIN_PRESS_MS)), Ordering::Relaxed);
    *b.calibration.lock().unwrap() = out.clone();
    b.logs.add("bench", format!("calibration done: min press {} ms, latency median {} ms", out["min_press_ms"], out["latency_ms_median"]));
    Ok(Json(out))
}

async fn post_test(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    not_manual_only(&b)?;
    let test = match (&v["test"], v["path"].as_str()) {
        (t, _) if t.is_object() => t.clone(),
        (_, Some(p)) => {
            // Only test files in the NS2Bench folder or the engine's working folder.
            let full = std::fs::canonicalize(crate::config::expand(p)).map_err(|e| bad(format!("{p}: {e}")))?;
            let root = std::fs::canonicalize(crate::config::state_dir()).ok();
            if !root.is_some_and(|r| full.starts_with(r)) || full.extension().and_then(|e| e.to_str()) != Some("json") {
                return Err(bad("path must be a .json test under ~/.ns2bench; or send the test itself as \"test\" (the CLI and MCP do)"));
            }
            serde_json::from_str(&std::fs::read_to_string(&full).map_err(|e| bad(format!("{p}: {e}")))?).map_err(|e| bad(format!("{p}: {e}")))?
        }
        _ => return Err(bad("test or path required")),
    };
    Ok(Json(crate::testrun::run_test(&b, &test, &source_of(&h, &v), v["stop_on_fail"].as_bool().unwrap_or(true)).await?))
}

async fn post_clip(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    crate::settings::agents_blocked(&b, &h, &v)?;
    let label = crate::testrun::slug(v["label"].as_str().unwrap_or("clip"));
    let c = b.target()?;
    let label = if b.list().len() > 1 { format!("{}-{label}", c.id) } else { label };
    let (path, span) = c.dvr.save(v["seconds"].as_f64(), None, &label, Some(&c.audio)).await.map_err(unavailable)?;
    b.logs.notify("bench", "info", "Clip saved", format!("clip saved ({span} s): {}", path.display()));
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    Ok(Json(json!({"path": path, "seconds": span, "url": format!("/clips/{name}")})))
}

async fn get_clips() -> Json<Value> {
    Json(dvr::list_clips())
}

async fn get_clip_file(Path(name): Path<String>) -> Response {
    let Some(p) = dvr::clip_file(&name) else { return (StatusCode::NOT_FOUND, "not found").into_response() };
    match tokio::fs::read(&p).await {
        Ok(data) => {
            let mime = mime_guess::from_path(&p).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string())], data).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

/// Read-only checks. The input probe presses buttons, so it needs POST (a GET can be
/// triggered by any web page through an image tag).
async fn get_doctor(State(b): State<S>) -> Json<Value> {
    Json(crate::doctor::run(&b, false).await)
}
async fn post_doctor(State(b): State<S>, Json(v): Json<Value>) -> R<Json<Value>> {
    let probe = v["probe"].as_bool() == Some(true);
    // (only the probe presses buttons: the checks themselves run whatever the controller is on)
    if probe { b.check_input_target()?; }
    Ok(Json(crate::doctor::run(&b, probe).await))
}

fn local(t: f64, f: &str) -> String {
    chrono::DateTime::from_timestamp(t as i64, ((t.fract()) * 1e9) as u32)
        .map(|d| d.with_timezone(&chrono::Local).format(f).to_string()).unwrap_or_default()
}

pub fn diagnostics_md(b: &crate::Bench, lines: usize) -> String {
    let st = b.status();
    let (v, i, cal) = (&st["video"], &st["input"], &st["calibration"]);
    let vc = b.target().map(|c| c.video_cfg.lock().unwrap().clone()).unwrap_or_default();
    let all = b.logs.all();
    let errors: Vec<_> = all.iter().filter(|e| e.level == "error").collect();
    let errors = &errors[errors.len().saturating_sub(15)..];
    let recent = &all[all.len().saturating_sub(lines)..];
    let fmt = |e: &crate::logs::Entry| format!("{} [{}] {}", local(e.t, "%H:%M:%S"), e.source, e.text);
    let s = |x: &Value| match x { Value::String(s) => s.clone(), Value::Null => "-".into(), o => o.to_string() };
    let mut out = vec![
        format!("## NS2Bench diagnostics — {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z")), String::new(),
        format!("- **NS2Bench** {} (native) · {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH),
        format!("- **Video** {} · {} · {}x{}@{} · measured {} fps · {}", s(&v["source"]), vc["device"].as_str().unwrap_or("default"),
            s(&vc["width"]), s(&vc["height"]), s(&vc["fps"]), s(&v["fps"]),
            v["error"].as_str().map_or("ok".into(), |e| format!("error: {e}"))),
        format!("- **Input** {} · {} · rtt {} ms · usb {} · devices {} · min press {} ms · manual-only {}", s(&i["backend"]),
            if i["connected"] == true { "connected".into() } else { format!("DISCONNECTED: {}", s(&i["detail"])) },
            s(&i["rtt_ms"]), s(&i["usb"]), strs(&i["devices"]).join(", "), s(&i["min_press_ms"]), s(&i["manual_only"])),
        format!("- **Audio** {} · level {} dB · {}", s(&st["audio"]["device"]), s(&st["audio"]["level_db"]), st["audio"]["error"].as_str().unwrap_or("ok")),
        format!("- **Calibration** shortest reliable press {} ms · latency median {} ms · measured {}",
            s(&cal["shortest_reliable_press_ms"]), s(&cal["latency_ms_median"]), cal["measured_at"].as_str().unwrap_or("never")),
        format!("- **Uptime** {} s", s(&st["uptime_s"])), String::new(),
    ];
    if !errors.is_empty() {
        out.extend(["### Recent errors".into(), "```text".into()]);
        out.extend(errors.iter().map(|e| fmt(e)));
        out.extend(["```".into(), String::new()]);
    }
    out.extend([format!("### Last {} log lines", recent.len()), "```text".into()]);
    out.extend(recent.iter().map(fmt));
    out.extend(["```".into(), String::new()]);
    out.join("\n")
}

async fn get_diagnostics(State(b): State<S>, Query(q): Query<HashMap<String, String>>) -> Response {
    let n = q.get("lines").and_then(|v| v.parse().ok()).unwrap_or(60);
    ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], diagnostics_md(&b, n)).into_response()
}

/// Mask every secret-looking string anywhere in the config (paths such as
/// `token_file` stay: they help debugging and hold no secret themselves).
pub fn redact(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                let k = k.to_ascii_lowercase();
                let secret = ["token", "secret", "password", "key"].iter().any(|w| k.contains(w)) && !k.ends_with("_file");
                if secret && x.is_string() {
                    *x = "***".into();
                } else if let Some(u) = x.as_str().filter(|u| u.contains("://") && u.contains('@')) {
                    // http://user:pass@host -> http://***@host
                    if let Some((scheme, rest)) = u.split_once("://") {
                        if let Some((_, host)) = rest.split_once('@') {
                            *x = format!("{scheme}://***@{host}").into();
                        }
                    }
                } else {
                    redact(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(redact),
        _ => {}
    }
}

/// Everything needed to report a problem, in one zip (no secrets).
async fn get_bundle(State(b): State<S>) -> R<Response> {
    // The live engine's view, plus everything a report has without one (crate::report).
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("README.txt".into(), crate::report::readme(true)),
        ("diagnostics.md".into(), diagnostics_md(&b, 200).into_bytes()),
        ("status.json".into(), serde_json::to_vec_pretty(&b.status()).unwrap()),
        ("logs/live.txt".into(), b.logs.all().iter().map(|e| format!("{} [{}] {} {}", local(e.t, "%Y-%m-%dT%H:%M:%S"), e.source, e.level, e.text))
            .collect::<Vec<_>>().join("\n").into_bytes()),
        ("events.json".into(), serde_json::to_vec_pretty(&b.hub.events.lock().unwrap().iter().cloned().collect::<Vec<_>>()).unwrap()),
    ];
    // The check, without pressing anything.
    if let Ok(check) = tokio::time::timeout(Duration::from_secs(15), crate::doctor::run(&b, false)).await {
        files.push(("check.json".into(), serde_json::to_vec_pretty(&check).unwrap()));
    }
    files.extend(crate::report::offline_files().await);
    for c in b.list() {
        if let Some(f) = c.frames.latest() {
            files.push((format!("screenshot-{}.jpg", c.id), f.jpeg.to_vec()));
        }
    }
    let data = tokio::task::spawn_blocking(move || crate::report::zip(files)).await.map_err(|e| bad(e.to_string()))?.map_err(|e| bad(e.to_string()))?;
    let name = format!("ns2bench-report-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    Ok(([(header::CONTENT_TYPE, "application/zip".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], data).into_response())
}

#[cfg(test)]
mod redact_tests {
    #[test]
    fn secrets_are_masked_anywhere_paths_stay() {
        let mut v = serde_json::json!({"server":{"token":"abc"},"input":{"token":"t","token_file":"~/.ns2bench/hidd.token"},
            "network":{"helper":{"api_key":"k"}},"list":[{"password":"p"}]});
        super::redact(&mut v);
        assert_eq!(v["server"]["token"], "***");
        assert_eq!(v["input"]["token"], "***");
        assert_eq!(v["input"]["token_file"], "~/.ns2bench/hidd.token");
        assert_eq!(v["network"]["helper"]["api_key"], "***");
        assert_eq!(v["list"][0]["password"], "***");
        let mut u = serde_json::json!({"network":{"helper":{"url":"http://me:pw@10.0.0.2:7632"}}});
        super::redact(&mut u);
        assert_eq!(u["network"]["helper"]["url"], "http://***@10.0.0.2:7632");
    }
}
