//! Everything built on video + input: OCR, screen log, probe/calibration, routes, tests,
//! recorder + clips, audio, doctor, diagnostics and the report bundle.

use crate::audio::AudioHub;
use crate::dvr::{self, Dvr};
use crate::ocr::{self, Engine};
use crate::server::{bad, source_of, strs, ApiError, R, S};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Ext {
    pub ocr: Option<Engine>,
    pub profile: Value,
    pub dvr: Arc<Dvr>,
    pub audio: Arc<AudioHub>,
}

impl Ext {
    pub fn new(cfg: &crate::Config) -> Ext {
        let dvr = Dvr::new(cfg.raw.get("dvr"));
        Ext {
            ocr: ocr::make_engine(cfg.raw.get("ocr")),
            profile: crate::nav::load_profile(cfg.raw["profile"].as_str().unwrap_or("switch2")),
            audio: AudioHub::new(cfg.raw.get("audio"), dvr.seconds + 2.0),
            dvr,
        }
    }
}

pub fn start(b: Arc<crate::Bench>) {
    tokio::spawn(b.ext.dvr.clone().run(b.frames.clone()));
    tokio::spawn(b.ext.audio.clone().run(b.video_cfg.clone(), b.logs.clone()));
    tokio::spawn(watch_health(b.clone()));
    tokio::spawn(watch_screen(b));
}

/// Watch the picture and say when it stops, comes back or runs slow, in plain words.
async fn watch_health(b: Arc<crate::Bench>) {
    let mut stalled = false;
    let mut slow_since: Option<std::time::Instant> = None;
    let mut slow_logged = false;
    let mut seen_frame = false;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let v = b.frames.status();
        let Some(age) = v["age_s"].as_f64() else { continue };
        if !seen_frame { seen_frame = true; b.logs.add("bench", "video: first picture received"); }
        if age > 5.0 && !stalled {
            stalled = true;
            b.logs.notify("bench", "warn", "No picture", format!("video: no new picture for {age:.0} s. The console may be asleep, or the HDMI cable or capture card was unplugged; it comes back by itself"));
        } else if age < 1.0 && stalled {
            stalled = false;
            b.logs.notify("bench", "info", "Picture is back", "video: the picture is back");
        }
        let want = b.video_cfg.lock().unwrap()["fps"].as_f64().unwrap_or(30.0);
        let fps = v["fps"].as_f64().unwrap_or(0.0);
        if !stalled && fps > 0.0 && fps < want * 0.5 {
            let since = *slow_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed() > Duration::from_secs(10) && !slow_logged {
                slow_logged = true;
                b.logs.warn("bench", format!("video is slow: {fps:.0} of {want:.0} fps. Close other apps using the capture card or CPU, or pick a lower resolution or FPS in the Video panel"));
            }
        } else {
            if slow_logged { b.logs.add("bench", format!("video speed is back to normal ({fps:.0} fps)")); }
            slow_since = None;
            slow_logged = false;
        }
    }
}

pub fn status(b: &crate::Bench, s: &mut Value) {
    s["ocr"] = json!({"engine": b.ext.ocr.map(|e| e.name()), "screen_log": b.screen_log.load(Ordering::Relaxed)});
    s["dvr"] = b.ext.dvr.status();
    s["audio"] = b.ext.audio.status();
}

/// Screen as a log source: text that newly appears on screen becomes a 'screen' log line.
async fn watch_screen(b: Arc<crate::Bench>) {
    let mut recent: Vec<HashSet<String>> = vec![];
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if !b.screen_log.load(Ordering::Relaxed) || b.ext.ocr.is_none() || b.frames.latest().is_none() {
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
                b.logs.add("screen", t);
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
}

/// Wait for the screen to react ("change") or to settle ("stable").
async fn post_wait_screen(State(b): State<S>, Json(v): Json<Value>) -> R<Json<Value>> {
    let until = v["until"].as_str().unwrap_or("change");
    let timeout = Duration::from_millis(v["timeout_ms"].as_u64().unwrap_or(5000).clamp(100, 120_000));
    let stable_ms = v["stable_ms"].as_u64().unwrap_or(500).clamp(100, 10_000);
    let region = ocr::parse_region(&v["region"])?;
    Ok(Json(crate::calib::wait_screen(&b.frames, until, timeout, stable_ms, region).await
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
    let mut v: Vec<Value> = std::fs::read_dir(&dir).into_iter().flatten().flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let p = e.path();
            let report: Value = std::fs::read_to_string(p.join("report.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            json!({"name": e.file_name().to_string_lossy(), "path": p, "passed": report["passed"], "test": report["name"]})
        })
        .collect();
    v.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    v.truncate(200);
    Json(json!({"dir": dir, "reports": v}))
}

fn unavailable(e: anyhow::Error) -> ApiError {
    ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string())
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
    let ok = v.as_object().is_some_and(|o| o.len() <= 200
        && o.iter().all(|(k, x)| k.starts_with("ns2bench.") && k.len() <= 80 && x.as_str().is_some_and(|x| x.len() <= 2000)));
    if !ok {
        return Err(bad("preferences are up to 200 short \"ns2bench.*\" text values"));
    }
    crate::config::write_json(&crate::config::state_dir().join("ui.json"), &v).map_err(anyhow::Error::from)?;
    Ok(Json(json!({"ok": true})))
}

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
    let (path, span) = b.ext.dvr.save(v["seconds"].as_f64(), None, &label, Some(&b.ext.audio)).await.map_err(unavailable)?;
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
async fn post_doctor(State(b): State<S>, Json(v): Json<Value>) -> Json<Value> {
    Json(crate::doctor::run(&b, v["probe"].as_bool() == Some(true)).await)
}

fn local(t: f64, f: &str) -> String {
    chrono::DateTime::from_timestamp(t as i64, ((t.fract()) * 1e9) as u32)
        .map(|d| d.with_timezone(&chrono::Local).format(f).to_string()).unwrap_or_default()
}

pub fn diagnostics_md(b: &crate::Bench, lines: usize) -> String {
    let st = b.status();
    let (v, i, cal) = (&st["video"], &st["input"], &st["calibration"]);
    let vc = b.video_cfg.lock().unwrap().clone();
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
    let mut cfg = b.cfg.raw.clone();
    redact(&mut cfg);
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("diagnostics.md", diagnostics_md(&b, 200).into_bytes()),
        ("status.json", serde_json::to_vec_pretty(&b.status()).unwrap()),
        ("logs.txt", b.logs.all().iter().map(|e| format!("{} [{}] {} {}", local(e.t, "%Y-%m-%dT%H:%M:%S"), e.source, e.level, e.text))
            .collect::<Vec<_>>().join("\n").into_bytes()),
        ("events.json", serde_json::to_vec_pretty(&b.hub.events.lock().unwrap().iter().cloned().collect::<Vec<_>>()).unwrap()),
        ("config.json", serde_json::to_vec_pretty(&cfg).unwrap()),
        ("calibration.json", serde_json::to_vec_pretty(&*b.calibration.lock().unwrap()).unwrap()),
    ];
    let shot = b.frames.latest().map(|f| f.jpeg.to_vec());
    let data = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (n, d) in files {
            z.start_file(n, opt)?;
            z.write_all(&d)?;
        }
        if let Some(j) = shot {
            z.start_file("screenshot.jpg", opt.compression_method(zip::CompressionMethod::Stored))?;
            z.write_all(&j)?;
        }
        Ok(z.finish()?.into_inner())
    }).await.map_err(|e| bad(e.to_string()))??;
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
