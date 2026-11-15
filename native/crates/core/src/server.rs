//! HTTP server: browser UI, MJPEG + H.264 live view, JSON API, realtime WebSocket.
//! Shared routes and JSON contracts for the UI, MCP and scripts.

use crate::bench::Bench;
use crate::inputs::{key_code, Locked, Push};
use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(RustEmbed)]
#[folder = "../../../web/"]
struct Web;

pub type S = Arc<Bench>;

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        if e.downcast_ref::<Locked>().is_some() {
            return ApiError(StatusCode::CONFLICT, e.to_string());
        }
        let m = e.to_string();
        let code = if m.contains("not connected") || m.contains("no live video") || m.contains("no OCR") || m.contains("stopped") {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_REQUEST
        };
        ApiError(code, m)
    }
}
pub fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
pub type R<T> = std::result::Result<T, ApiError>;

/// Secret the desktop app gives its own window (kept in memory only). When set, only
/// requests that carry it count as the person at the NS2Bench page, so a local script
/// or an agent with a shell can't pose as the page by faking an Origin header.
static UI_KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn set_ui_key(key: String) {
    let _ = UI_KEY.set(key);
}

fn ui_key_ok(given: Option<&str>) -> bool {
    match UI_KEY.get() {
        Some(k) => given.is_some_and(|g| same(g.as_bytes(), k.as_bytes())),
        None => true,
    }
}

/// True only for the NS2Bench page: a same-origin browser request (`guard` has already
/// rejected any other Origin), carrying the app's UI key when the app set one.
pub fn from_browser(headers: &HeaderMap) -> bool {
    headers.contains_key(header::ORIGIN)
        && ui_key_ok(headers.get("x-ns2bench-ui").and_then(|v| v.to_str().ok()))
}

/// Reject cross-site browser requests and DNS-rebinding host names. A page on
/// another site must never drive the console, even when no token is configured.
pub fn guard_request(headers: &HeaderMap, path: &str) -> std::result::Result<(), &'static str> {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let name = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.rsplit_once(':').map_or(host, |(n, _)| n)
    };
    if !host.is_empty() && name != "localhost" && name.parse::<std::net::IpAddr>().is_err() {
        return Err("host name not allowed: open NS2Bench by its IP address or localhost");
    }
    match headers.get(header::ORIGIN).map(|o| o.to_str().unwrap_or("")) {
        Some(o) if o != format!("http://{host}") => Err("cross-origin request refused"),
        None if path == "/ws" => Err("the UI socket is for the NS2Bench page only"),
        _ => Ok(()),
    }
}

pub fn source_of(headers: &HeaderMap, body: &Value) -> String {
    let s = body["source"].as_str().map(str::to_string)
        .or_else(|| headers.get("x-ns2bench-source").and_then(|v| v.to_str().ok()).map(str::to_string))
        .unwrap_or_else(|| "api".into())
        .to_lowercase();
    // Only the NS2Bench page itself may act as the person at the controls.
    if s == "ui" && !from_browser(headers) { return "api".into(); }
    if ["api", "mcp", "ui"].contains(&s.as_str()) { s } else { "api".into() }
}

pub fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

pub fn router(bench: S) -> Router {
    let token = bench.cfg.token();
    let bench_logs = bench.logs.clone();
    let mut r = Router::new()
        .route("/", get(index))
        .route("/static/{*path}", get(asset))
        .route("/stream.mjpg", get(stream))
        .route("/ws", get(ws_ui))
        .route("/video", get(crate::h264::ws_video))
        .route("/audio", get(crate::audio::ws_audio))
        .route("/api/integrations", get(crate::settings::integrations))
        .route("/api/input", get(crate::settings::get_input).post(crate::settings::set_input))
        .route("/api/network", get(crate::settings::get_network).post(crate::settings::set_network))
        .route("/api/status", get(status))
        .route("/api/events", get(events))
        .route("/api/logs", get(logs))
        .route("/api/screenshot", get(screenshot))
        .route("/api/control", post(control))
        .route("/api/press", post(press))
        .route("/api/hold", post(hold))
        .route("/api/release", post(release))
        .route("/api/stick", post(stick))
        .route("/api/key", post(key))
        .route("/api/type", post(type_text))
        .route("/api/mouse", post(mouse))
        .route("/api/click", post(click))
        .route("/api/sequence", post(sequence))
        .route("/api/video", get(get_video).post(post_video));
    r = crate::features::routes(r);
    let activity = bench.clone();
    let r = r.with_state(bench).layer(axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
        let bench = activity.clone();
        async move {
            struct Operation(Option<S>);
            impl Drop for Operation {
                fn drop(&mut self) { if let Some(b) = &self.0 { b.active_operations.fetch_sub(1, std::sync::atomic::Ordering::Relaxed); } }
            }
            let busy = req.method() == axum::http::Method::POST;
            let path = req.uri().path().to_string();
            if !busy {
                let res = next.run(req).await;
                if res.status().is_server_error() && path.starts_with("/api/") {
                    bench.logs.warn("api", format!("GET {path} → {}", res.status()));
                }
                return res;
            }
            let who = if from_browser(req.headers()) { "ui".to_string() } else {
                req.headers().get("x-ns2bench-source").and_then(|v| v.to_str().ok()).unwrap_or("api").chars().take(16).collect()
            };
            let logs = bench.logs.clone();
            bench.active_operations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let operation = Operation(Some(bench));
            let t0 = std::time::Instant::now();
            // Run to completion even if the client goes away: a timed press must
            // always reach its release, whatever happens to the HTTP connection.
            // Read-only waits stop when the client goes away; anything that drives input runs
            // to completion (detached), so a timed press always reaches its release.
            let res = if matches!(path.as_str(), "/api/wait_text" | "/api/wait_screen") {
                let _operation = operation;
                next.run(req).await
            } else {
                tokio::spawn(async move { let _operation = operation; next.run(req).await })
                    .await
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            };
            let line = format!("{who}: POST {path} → {} in {} ms", res.status().as_u16(), t0.elapsed().as_millis());
            if res.status().is_client_error() || res.status().is_server_error() { logs.warn("api", line) } else { logs.debug("api", line) }
            res
        }
    }));
    let token_logs = bench_logs.clone();
    let r = match token {
        Some(t) => r.layer(axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
            let t = t.clone();
            let logs = token_logs.clone();
            async move {
                let p = req.uri().path().to_string();
                let public = p == "/" || p.starts_with("/static/") || p == "/favicon.ico";
                let q = Query::<HashMap<String, String>>::try_from_uri(req.uri()).map(|q| q.0).unwrap_or_default();
                let given = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
                    .map(|v| v.trim_start_matches("Bearer ").trim().to_string())
                    .or_else(|| q.get("token").cloned())
                    .unwrap_or_default();
                if !public && !same(given.as_bytes(), t.as_bytes()) {
                    if !logs.throttled("api-token", 10.0) {
                        logs.warn("api", format!("refused {p}: wrong or missing API token (unauthorized)"));
                    }
                    return (StatusCode::UNAUTHORIZED, "token required").into_response();
                }
                next.run(req).await
            }
        })),
        None => r,
    };
    let guard_logs = bench_logs;
    r.layer(axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
      let logs = guard_logs.clone();
      async move {
        if let Err(e) = guard_request(req.headers(), req.uri().path()) {
            if !logs.throttled(&format!("guard-{e}"), 10.0) {
                let origin = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()).unwrap_or("-").chars().take(80).collect::<String>();
                logs.warn("api", format!("refused {} {}: {e} (origin {origin}). Another web page tried to use NS2Bench; it was blocked.", req.method(), req.uri().path()));
            }
            return (StatusCode::FORBIDDEN, e).into_response();
        }
        let mut res = next.run(req).await;
        // No other site may frame the UI (clickjacking the driving controls).
        let h = res.headers_mut();
        h.insert(header::X_FRAME_OPTIONS, header::HeaderValue::from_static("DENY"));
        h.insert(header::CONTENT_SECURITY_POLICY, header::HeaderValue::from_static("frame-ancestors 'none'"));
        h.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
        h.insert(header::REFERRER_POLICY, header::HeaderValue::from_static("no-referrer"));
        res
      }
    }))
}

/// Constant-time comparison for tokens.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn no_cache(mut r: Response) -> Response {
    r.headers_mut().insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    r
}

async fn index() -> Response {
    asset_named("index.html")
}

fn asset_named(path: &str) -> Response {
    match Web::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            no_cache(([(header::CONTENT_TYPE, mime.as_ref().to_string())], f.data.into_owned()).into_response())
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    asset_named(&path)
}

async fn status(State(b): State<S>) -> Json<Value> {
    Json(b.status())
}

async fn events(State(b): State<S>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let after: f64 = q.get("after").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let limit: usize = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(200).clamp(1, 200);
    let source = q.get("source").filter(|s| !s.is_empty());
    let v: Vec<_> = b.hub.events.lock().unwrap().iter().filter(|e| e.t > after && source.is_none_or(|s| &e.source == s)).cloned().collect();
    Json(serde_json::to_value(&v[v.len().saturating_sub(limit)..]).unwrap())
}

async fn logs(State(b): State<S>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let entries = b.logs.query_level(q.get("source").map(String::as_str).filter(|s| !s.is_empty()),
        q.get("after").and_then(|v| v.parse().ok()).unwrap_or(0),
        q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(200),
        q.get("contains").map(String::as_str).filter(|s| !s.is_empty()),
        q.get("level").map(String::as_str).filter(|s| crate::logs::LEVELS.contains(s)));
    Json(json!({"sources": b.logs.sources(), "levels": b.logs.level_counts(), "entries": entries}))
}

pub fn encode_image(jpeg: &[u8], fmt: &str, max_width: Option<u32>) -> Result<(Vec<u8>, &'static str)> {
    if fmt != "png" && max_width.is_none() {
        return Ok((jpeg.to_vec(), "image/jpeg"));
    }
    let mut img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)?;
    if let Some(w) = max_width {
        if img.width() > w {
            let h = (img.height() as f64 * w as f64 / img.width() as f64).round() as u32;
            img = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
        }
    }
    let mut out = std::io::Cursor::new(vec![]);
    if fmt == "png" {
        img.write_to(&mut out, image::ImageFormat::Png)?;
        Ok((out.into_inner(), "image/png"))
    } else {
        let rgb = img.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&rgb)?;
        Ok((out.into_inner(), "image/jpeg"))
    }
}

async fn screenshot(State(b): State<S>, Query(q): Query<HashMap<String, String>>) -> R<Response> {
    let f = b.frames.latest().filter(|f| f.at.elapsed() < Duration::from_secs(5))
        .ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, format!("no live video ({})",
            b.frames.error.lock().unwrap().clone().unwrap_or("waiting".into()))))?;
    let fmt = q.get("format").map(|s| s.to_lowercase()).unwrap_or("jpeg".into());
    if !["jpeg", "jpg", "png"].contains(&fmt.as_str()) {
        return Err(bad("format must be jpeg or png"));
    }
    let mw: Option<u32> = q.get("max_width").and_then(|v| v.parse().ok());
    if mw.is_some_and(|w| !(16..=8192).contains(&w)) {
        return Err(bad("max_width must be 16-8192"));
    }
    let region = crate::ocr::parse_region(&q.get("region").map(|s| Value::String(s.clone())).unwrap_or(Value::Null))?;
    let jpeg = f.jpeg.clone();
    let (data, ct) = tokio::task::spawn_blocking(move || -> anyhow::Result<(Vec<u8>, &'static str)> {
        let src = match region { Some(r) => crate::ocr::crop(&jpeg, r)?, None => jpeg.to_vec() };
        let fmt = if region.is_some() && fmt != "png" && mw.is_none() { "jpeg".to_string() } else { fmt };
        encode_image(&src, &fmt, mw)
    }).await.map_err(|e| bad(e.to_string()))??;
    Ok(([(header::CONTENT_TYPE, ct.to_string()), (header::CACHE_CONTROL, "no-store".into()),
         (header::HeaderName::from_static("x-frame-seq"), f.seq.to_string()),
         (header::HeaderName::from_static("x-frame-age-ms"), f.at.elapsed().as_millis().to_string())], data).into_response())
}

async fn stream(State(b): State<S>) -> Response {
    let rx = b.frames.latest.subscribe();
    let s = futures::stream::unfold(rx, |mut rx| async move {
        rx.changed().await.ok()?;
        let f = rx.borrow_and_update().clone()?;
        // Header, the shared frame (no copy per viewer) and the trailer as separate chunks.
        let head = Bytes::from(format!("--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n", f.jpeg.len()));
        let parts = [head, f.jpeg.clone(), Bytes::from_static(b"\r\n")].map(Ok::<Bytes, std::io::Error>);
        Some((futures::stream::iter(parts), rx))
    }).flatten();
    Response::builder()
        .header(header::CONTENT_TYPE, "multipart/x-mixed-replace; boundary=frame")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(s))
        .unwrap()
}

async fn control(State(b): State<S>, h: HeaderMap, Json(body): Json<Value>) -> R<Json<Value>> {
    let who = source_of(&h, &body);
    let blocked = who != "ui" && b.hub.manual_only.load(std::sync::atomic::Ordering::Relaxed);
    if let Some(m) = body["usb_mode"].as_str() {
        if !["standard", "hori", "pro"].contains(&m) {
            return Err(bad("usb_mode must be standard, hori or pro"));
        }
        if blocked {
            return Err(ApiError(StatusCode::CONFLICT, "manual-only mode is on: only the person at the NS2Bench page can switch the USB mode".into()));
        }
        // Not in the middle of an agent's input: the controller disconnects during the switch.
        let _quiet = b.hub.configuration_guard().map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
        b.hub.backend.send(crate::inputs::Msg::UsbMode(m.into()))?;
        b.logs.add("bench", format!("board USB mode -> {m} (the console sees the controller reconnect)"));
    }
    if let Some(on) = body["screen_log"].as_bool() {
        if blocked {
            return Err(ApiError(StatusCode::CONFLICT, "manual-only mode is on: only the person at the NS2Bench page can change this".into()));
        }
        b.screen_log.store(on, std::sync::atomic::Ordering::Relaxed);
        b.logs.add("bench", format!("screen log {}", if on { "on" } else { "off" }));
    }
    if let Some(on) = body["manual_only"].as_bool() {
        if !on && !from_browser(&h) {
            return Err(bad("only the person at the NS2Bench page can turn off manual-only mode"));
        }
        b.hub.manual_only.store(on, std::sync::atomic::Ordering::Relaxed);
        b.hub.log(&source_of(&h, &body), "manual_only", json!({"on": on}));
        if on { b.hub.clear_agent(); }
    }
    Ok(Json(b.status()))
}

fn done(b: &Bench) -> Json<Value> {
    Json(json!({"ok": true, "input": b.hub.status()}))
}

async fn press(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    b.hub.press(&strs(&v["buttons"]), v["hold_ms"].as_u64().unwrap_or(100), &source_of(&h, &v), false).await?;
    Ok(done(&b))
}
async fn hold(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    b.hub.hold(&strs(&v["buttons"]), &source_of(&h, &v))?;
    Ok(done(&b))
}
async fn release(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    let names = strs(&v["buttons"]);
    b.hub.release(if names.is_empty() { None } else { Some(&names) }, &source_of(&h, &v))?;
    Ok(done(&b))
}
async fn stick(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    b.hub.stick(v["stick"].as_str().unwrap_or("left"), v["x"].as_f64().unwrap_or(0.0), v["y"].as_f64().unwrap_or(0.0),
        v["hold_ms"].as_u64(), &source_of(&h, &v)).await?;
    Ok(done(&b))
}
async fn key(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    b.hub.key(v["key"].as_str().ok_or_else(|| bad("key required"))?, &strs(&v["modifiers"]), v["hold_ms"].as_u64().unwrap_or(60), &source_of(&h, &v)).await?;
    Ok(done(&b))
}
async fn type_text(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    b.hub.type_text(v["text"].as_str().ok_or_else(|| bad("text required"))?, v["delay_ms"].as_u64().unwrap_or(40), &source_of(&h, &v)).await?;
    Ok(done(&b))
}
async fn mouse(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    b.hub.mouse(v["dx"].as_i64().unwrap_or(0) as i32, v["dy"].as_i64().unwrap_or(0) as i32, v["wheel"].as_i64().unwrap_or(0) as i32,
        v["buttons"].as_u64().map(|x| x as u8), &source_of(&h, &v))?;
    Ok(done(&b))
}
async fn click(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    b.hub.click(v["button"].as_str().unwrap_or("left"), v["hold_ms"].as_u64().unwrap_or(60), &source_of(&h, &v)).await?;
    Ok(done(&b))
}

pub async fn run_sequence(b: &Bench, steps: &Value, source: &str) -> Result<()> {
    let steps = steps.as_array().ok_or_else(|| anyhow::anyhow!("steps must be a list"))?;
    if steps.len() > 200 { anyhow::bail!("at most 200 steps"); }
    for s in steps {
        if let Some(w) = s["wait_ms"].as_u64() {
            tokio::time::sleep(Duration::from_millis(w.min(30_000))).await;
        } else if s.get("press").is_some() {
            b.hub.press(&strs(&s["press"]), s["hold_ms"].as_u64().unwrap_or(100), source, false).await?;
        } else if s.get("hold").is_some() {
            b.hub.hold(&strs(&s["hold"]), source)?;
        } else if s.get("release").is_some() {
            let n = strs(&s["release"]);
            b.hub.release(if n.is_empty() { None } else { Some(&n) }, source)?;
        } else if let Some(st) = s["stick"].as_str() {
            b.hub.stick(st, s["x"].as_f64().unwrap_or(0.0), s["y"].as_f64().unwrap_or(0.0), s["hold_ms"].as_u64(), source).await?;
        } else if let Some(k) = s["key"].as_str() {
            b.hub.key(k, &strs(&s["modifiers"]), s["hold_ms"].as_u64().unwrap_or(60), source).await?;
        } else if let Some(t) = s["type"].as_str() {
            b.hub.type_text(t, s["delay_ms"].as_u64().unwrap_or(40), source).await?;
        } else if let Some(m) = s.get("mouse") {
            b.hub.mouse(m["dx"].as_i64().unwrap_or(0) as i32, m["dy"].as_i64().unwrap_or(0) as i32, m["wheel"].as_i64().unwrap_or(0) as i32,
                m["buttons"].as_u64().map(|x| x as u8), source)?;
        } else if let Some(c) = s["click"].as_str() {
            b.hub.click(c, s["hold_ms"].as_u64().unwrap_or(60), source).await?;
        } else {
            anyhow::bail!("unknown step {s}");
        }
    }
    Ok(())
}

async fn sequence(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    run_sequence(&b, &v["steps"], &source_of(&h, &v)).await?;
    Ok(done(&b))
}

async fn get_video(State(b): State<S>) -> Json<Value> {
    let c = b.video_cfg.lock().unwrap().clone();
    Json(json!({"settings": {"backend": c["backend"], "device": c["device"], "width": c["width"], "height": c["height"],
        "fps": c["fps"], "quality": c["quality"], "codec": c["codec"], "audio_device": c["audio_device"]},
        // Listing can be slow the first time (macOS builds its capture helper): never hang the caller.
        "devices": tokio::time::timeout(Duration::from_secs(8), crate::video::list_devices()).await.unwrap_or_else(|_| json!([]))}))
}

async fn post_video(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    if source_of(&h, &v) != "ui" && b.hub.manual_only.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(ApiError(StatusCode::CONFLICT, "manual-only mode is on: only the person at the NS2Bench page can change video settings".into()));
    }
    let mut v = v;
    if let Some(o) = v.as_object_mut() { o.remove("source"); }
    b.set_video(v).await?;
    Ok(Json(json!({"ok": true})))
}

async fn ws_ui(State(b): State<S>, Query(q): Query<HashMap<String, String>>, ws: WebSocketUpgrade) -> Response {
    // Without the app's UI key the socket still shows status and logs, but can't drive.
    let drive = ui_key_ok(q.get("ui").map(String::as_str));
    ws.on_upgrade(move |sock| ui_socket(b, sock, drive))
}

async fn ui_socket(b: S, sock: WebSocket, drive: bool) {
    b.logs.debug("bench", "a NS2Bench page connected");
    struct Gone(S);
    impl Drop for Gone { fn drop(&mut self) { self.0.logs.debug("bench", "a NS2Bench page disconnected"); } }
    let _gone = Gone(b.clone());
    let (mut tx, mut rx) = sock.split();
    let mut pushes = b.hub.tx.subscribe();
    let mut logs = b.logs.tx.subscribe();
    let b2 = b.clone();
    let sender = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            let msg = tokio::select! {
                p = pushes.recv() => match p {
                    Ok(Push::Pad { pad, agent }) => json!({"t": "pad", "pad": pad, "agent": agent}),
                    Ok(Push::Event(e)) => json!({"t": "event", "event": e}),
                    Err(_) => continue,
                },
                l = logs.recv() => match l { Ok(e) => json!({"t": "log", "entry": e}), Err(_) => continue },
                _ = tick.tick() => json!({"t": "status", "status": b2.status()}),
            };
            if tx.send(Message::Text(msg.to_string().into())).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(m)) = rx.next().await {
        let Message::Text(t) = m else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
        if !drive { continue; }
        let r = match v["t"].as_str() {
            Some("state") => {
                let keys: Vec<u8> = strs(&v["keynames"]).iter().filter_map(|k| key_code(k).ok()).collect();
                let sticks = v["sticks"].as_array().filter(|a| a.len() == 4)
                    .map(|a| [0, 1, 2, 3].map(|i| a[i].as_u64().unwrap_or(128).min(255) as u8));
                b.hub.set_ui_state(v["buttons"].as_u64().unwrap_or(0) as u32, v["dpad"].as_u64().unwrap_or(0) as u8,
                    sticks, v["mods"].as_u64().unwrap_or(0) as u8, keys)
            }
            Some("mouse") => b.hub.mouse(v["dx"].as_i64().unwrap_or(0) as i32, v["dy"].as_i64().unwrap_or(0) as i32,
                v["wheel"].as_i64().unwrap_or(0) as i32, v["buttons"].as_u64().map(|x| x as u8), "ui"),
            _ => Ok(()),
        };
        let _ = r; // errors surface through status (input offline) rather than per message
    }
    sender.abort();
    if drive { let _ = b.hub.set_ui_state(0, 0, None, 0, vec![]); } // tab closed: release human input
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use tower::ServiceExt;

    #[test]
    fn cross_site_and_rebinding_requests_are_refused() {
        let h = |pairs: &[(&str, &str)]| {
            let mut m = HeaderMap::new();
            for (k, v) in pairs { m.insert(axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(), v.parse().unwrap()); }
            m
        };
        assert!(guard_request(&h(&[("host", "127.0.0.1:7630")]), "/api/press").is_ok());
        assert!(guard_request(&h(&[("host", "localhost:7630"), ("origin", "http://localhost:7630")]), "/ws").is_ok());
        assert!(guard_request(&h(&[("host", "[::1]:7630"), ("origin", "http://[::1]:7630")]), "/ws").is_ok());
        assert!(guard_request(&h(&[("host", "127.0.0.1:7630"), ("origin", "https://evil.example")]), "/api/press").is_err());
        assert!(guard_request(&h(&[("host", "127.0.0.1:7630"), ("origin", "null")]), "/api/press").is_err());
        assert!(guard_request(&h(&[("host", "rebind.evil.example:7630")]), "/api/status").is_err());
        assert!(guard_request(&h(&[("host", "127.0.0.1:7630")]), "/ws").is_err());
        // Only the page itself is "ui"; scripts claiming it are agents.
        assert_eq!(source_of(&h(&[]), &json!({"source":"ui"})), "api");
        assert_eq!(source_of(&h(&[("origin", "http://127.0.0.1:7630")]), &json!({"source":"ui"})), "ui");
    }

    #[tokio::test]
    async fn cancelled_press_still_releases() {
        let b = Bench::new(crate::Config::from_raw(None, json!({"input":{"backend":"none"},"video":{"backend":"none"}})));
        let app = router(b.clone());
        let request = Request::builder().method("POST").uri("/api/press").header("host", "127.0.0.1:7630")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"buttons":["A"],"hold_ms":300}"#)).unwrap();
        // The client gives up after 50 ms, mid-press.
        let _ = tokio::time::timeout(Duration::from_millis(50), app.oneshot(request)).await;
        assert_ne!(b.hub.status()["pad"][0], json!(0), "A should be down mid-press");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(b.hub.status()["pad"][0], json!(0), "A must be released after the hold");
    }

    #[tokio::test]
    async fn active_api_operation_is_visible_until_it_finishes() {
        let b = Bench::new(crate::Config::from_raw(None,json!({"input":{"backend":"none"},"video":{"backend":"none"}})));
        let app = router(b.clone());
        let request = Request::builder().method("POST").uri("/api/sequence").header("content-type","application/json")
            .body(Body::from(r#"{"steps":[{"wait_ms":300}]}"#)).unwrap();
        let task=tokio::spawn(app.oneshot(request));
        tokio::time::timeout(Duration::from_secs(1),async {
            while b.status()["active_operations"] != 1 { tokio::task::yield_now().await; }
        }).await.unwrap();
        assert_eq!(task.await.unwrap().unwrap().status(),StatusCode::OK);
        assert_eq!(b.status()["active_operations"],0);
    }

    #[tokio::test]
    async fn assets_status_screenshot_auth_and_input_contract() {
        let cfg = crate::config::Config::from_raw(None, json!({
            "server":{"token":"synthetic-api-token"}, "video":{"backend":"none"},
            "input":{"backend":"none"}, "audio":{"enabled":false}
        }));
        let mut bench = Bench::new(cfg);
        let (hub, mut rx) = crate::inputs::tests::fake();
        Arc::get_mut(&mut bench).unwrap().hub = hub;
        let mut jpeg=Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode(&[32,64,96], 1, 1, image::ExtendedColorType::Rgb8).unwrap();
        bench.frames.push(Bytes::from(jpeg.clone()));
        let app=router(bench.clone());
        let call = |method: &str, path: &str, body: Value, auth: bool| {
            let mut req=Request::builder().method(method).uri(path).header("content-type","application/json");
            if auth { req=req.header("authorization","Bearer synthetic-api-token"); }
            app.clone().oneshot(req.body(Body::from(body.to_string())).unwrap())
        };
        let home=call("GET","/",Value::Null,false).await.unwrap();
        assert_eq!(home.status(),StatusCode::OK);
        assert!(to_bytes(home.into_body(),1024*1024).await.unwrap().len()>100);
        assert_eq!(call("GET","/api/status",Value::Null,false).await.unwrap().status(),StatusCode::UNAUTHORIZED);
        let status=call("GET","/api/status",Value::Null,true).await.unwrap();
        assert_eq!(status.status(),StatusCode::OK);
        let data:Value=serde_json::from_slice(&to_bytes(status.into_body(),1024*1024).await.unwrap()).unwrap();
        assert_eq!(data["engine"],"native");
        for endpoint in ["/api/integrations", "/api/input", "/api/network"] {
            assert_eq!(call("GET",endpoint,Value::Null,false).await.unwrap().status(),StatusCode::UNAUTHORIZED);
            let response=call("GET",endpoint,Value::Null,true).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            let body=to_bytes(response.into_body(),1024*1024).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("synthetic-api-token"));
        }
        let shot=call("GET","/api/screenshot",Value::Null,true).await.unwrap();
        assert_eq!(shot.status(),StatusCode::OK);
        assert_eq!(&to_bytes(shot.into_body(),1024*1024).await.unwrap()[..2],&[0xff,0xd8]);
        assert_eq!(call("POST","/api/press",json!({"buttons":["NOPE"]}),true).await.unwrap().status(),StatusCode::BAD_REQUEST);
        assert_eq!(call("POST","/api/press",json!({"buttons":["A"],"hold_ms":16}),true).await.unwrap().status(),StatusCode::OK);
        assert!(matches!(rx.try_recv().unwrap(),crate::inputs::Msg::State([4,..],_)));
        assert!(matches!(rx.try_recv().unwrap(),crate::inputs::Msg::State(crate::inputs::NEUTRAL_PAD,_)));
        assert_eq!(call("POST","/api/control",json!({"manual_only":true}),true).await.unwrap().status(),StatusCode::OK);
        assert_eq!(call("POST","/api/press",json!({"buttons":["A"],"source":"mcp"}),true).await.unwrap().status(),StatusCode::CONFLICT);
    }
}
