//! Controller/keyboard/mouse model, the board backend, and the hub that merges human and agent
//! input. Wire model (what every backend receives): a whole-state pad report
//! [buttons(17 bits), hat 0-8, lx, ly, rx, ry] and a boot-keyboard report [mods, k1..k6].
//! Bits 14-16 (C, GL, GR) reach the console only in Pro Controller mode.

use crate::logs::LogHub;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc};

pub const CENTER: u8 = 128;
pub const HAT_NEUTRAL: u8 = 8;
pub const MAX_HOLD_MS: u64 = 10_000;
pub type Pad = [u32; 6];
pub type Kbd = [u8; 7];
pub const NEUTRAL_PAD: Pad = [0, 8, 128, 128, 128, 128];
pub const NEUTRAL_KBD: Kbd = [0; 7];

pub fn button_bit(name: &str) -> Option<u32> {
    Some(match name {
        "Y" => 0, "B" => 1, "A" => 2, "X" => 3, "L" => 4, "R" => 5, "ZL" => 6, "ZR" => 7,
        "MINUS" => 8, "PLUS" => 9, "LSTICK" => 10, "RSTICK" => 11, "HOME" => 12, "CAPTURE" => 13,
        "C" => 14, "GL" => 15, "GR" => 16,
        _ => return None,
    })
}

fn alias(n: &str) -> &str {
    match n {
        "-" | "SELECT" => "MINUS", "+" | "START" => "PLUS", "L3" | "LS" => "LSTICK", "R3" | "RS" => "RSTICK",
        "SHARE" => "CAPTURE", "GAMECHAT" => "C", other => other,
    }
}

fn dpad_bits(n: &str) -> Option<u8> {
    let mut bits = 0;
    for part in n.split('_') {
        bits |= match part { "UP" => 1, "RIGHT" => 2, "DOWN" => 4, "LEFT" => 8, _ => return None };
    }
    Some(bits)
}

/// ["A", "UP", "HOME"] -> (button mask, dpad bits)
pub fn parse_buttons(names: &[String]) -> Result<(u32, u8)> {
    let (mut mask, mut dpad) = (0u32, 0u8);
    for raw in names {
        let up = raw.trim().to_uppercase().replace(' ', "_");
        let n = up.strip_prefix("DPAD_").unwrap_or(&up);
        let n = alias(n);
        if let Some(b) = button_bit(n) {
            mask |= 1 << b;
        } else if let Some(d) = dpad_bits(n) {
            dpad |= d;
        } else {
            bail!("unknown button {raw:?}; use A B X Y L R ZL ZR PLUS MINUS HOME CAPTURE LSTICK RSTICK C GL GR UP DOWN LEFT RIGHT");
        }
    }
    Ok((mask, dpad))
}

pub fn dpad_to_hat(bits: u8) -> u8 {
    let (mut up, mut right, mut down, mut left) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
    if up && down { up = false; down = false; }
    if left && right { left = false; right = false; }
    match (up, right, down, left) {
        (true, false, false, false) => 0, (true, true, false, false) => 1, (false, true, false, false) => 2,
        (false, true, true, false) => 3, (false, false, true, false) => 4, (false, false, true, true) => 5,
        (false, false, false, true) => 6, (true, false, false, true) => 7, _ => HAT_NEUTRAL,
    }
}

/// -1.0..1.0 (up/left negative) -> 0..255
pub fn axis(v: f64) -> u8 {
    // floor: preserve the established input mapping (full left = 0, centre = 128, full right = 255)
    (CENTER as f64 + v.clamp(-1.0, 1.0) * 127.5).floor().clamp(0.0, 255.0) as u8
}

pub fn modifier_bit(name: &str) -> Option<u8> {
    Some(match name.to_uppercase().as_str() {
        "CTRL" | "LCTRL" => 0x01, "SHIFT" | "LSHIFT" => 0x02, "ALT" | "LALT" => 0x04,
        "META" | "GUI" | "CMD" | "WIN" => 0x08, "RCTRL" => 0x10, "RSHIFT" => 0x20, "RALT" => 0x40, "RMETA" => 0x80,
        _ => return None,
    })
}

/// Key name or browser KeyboardEvent.code -> HID usage.
pub fn key_code(name: &str) -> Result<u8> {
    let mut n = name.to_uppercase();
    if n.len() == 4 && n.starts_with("KEY") { n = n[3..].to_string(); }
    if n.len() == 6 && n.starts_with("DIGIT") { n = n[5..].to_string(); }
    let n = match n.as_str() {
        "ARROWUP" => "UP", "ARROWDOWN" => "DOWN", "ARROWLEFT" => "LEFT", "ARROWRIGHT" => "RIGHT",
        "NUMPADENTER" => "ENTER", other => other,
    }.to_string();
    let b = n.as_bytes();
    if b.len() == 1 && b[0].is_ascii_uppercase() { return Ok(0x04 + (b[0] - b'A')); }
    if b.len() == 1 && (b'1'..=b'9').contains(&b[0]) { return Ok(0x1E + (b[0] - b'1')); }
    if n.len() >= 2 && n.starts_with('F') {
        if let Ok(f) = n[1..].parse::<u8>() { if (1..=12).contains(&f) { return Ok(0x3A + f - 1); } }
    }
    Ok(match n.as_str() {
        "0" => 0x27, "ENTER" => 0x28, "ESCAPE" | "ESC" => 0x29, "BACKSPACE" => 0x2A, "TAB" => 0x2B, "SPACE" => 0x2C,
        "MINUS" => 0x2D, "EQUAL" => 0x2E, "BRACKETLEFT" => 0x2F, "BRACKETRIGHT" => 0x30, "BACKSLASH" => 0x31,
        "SEMICOLON" => 0x33, "QUOTE" => 0x34, "BACKQUOTE" => 0x35, "COMMA" => 0x36, "PERIOD" => 0x37, "SLASH" => 0x38,
        "CAPSLOCK" => 0x39, "PRINTSCREEN" => 0x46, "INSERT" => 0x49, "HOME" => 0x4A, "PAGEUP" => 0x4B,
        "DELETE" => 0x4C, "END" => 0x4D, "PAGEDOWN" => 0x4E, "RIGHT" => 0x4F, "LEFT" => 0x50, "DOWN" => 0x51, "UP" => 0x52,
        _ => bail!("unknown key {name:?}"),
    })
}

/// US layout: one character -> (modifiers, usage)
pub fn char_to_key(c: char) -> Result<(u8, u8)> {
    const SHIFTED: &str = "!@#$%^&*()_+{}|:\"~<>?";
    const BASE: &str = "1234567890-=[]\\;'`,./";
    let punct = |c: char| -> Option<u8> {
        Some(match c {
            '-' => 0x2D, '=' => 0x2E, '[' => 0x2F, ']' => 0x30, '\\' => 0x31, ';' => 0x33, '\'' => 0x34,
            '`' => 0x35, ',' => 0x36, '.' => 0x37, '/' => 0x38, ' ' => 0x2C, '\n' => 0x28, '\t' => 0x2B,
            _ => return None,
        })
    };
    if c.is_ascii_alphabetic() {
        let up = c.to_ascii_uppercase() as u8;
        return Ok((if c.is_ascii_uppercase() { 0x02 } else { 0 }, 0x04 + (up - b'A')));
    }
    if c.is_ascii_digit() {
        return Ok((0, key_code(&c.to_string())?));
    }
    if let Some(i) = SHIFTED.find(c) {
        let base = BASE.chars().nth(i).unwrap();
        let code = if base.is_ascii_digit() { key_code(&base.to_string())? } else { punct(base).unwrap() };
        return Ok((0x02, code));
    }
    punct(c).map(|k| (0, k)).ok_or_else(|| anyhow!("cannot type {c:?} (US layout ASCII only)"))
}

// ------------------------------------------------------------------ backend (board link)

#[derive(Debug)]
pub enum Msg {
    State(Pad, Kbd),
    Mouse([i32; 4]),
    /// A USB mode, and whether it becomes the board's boot default (a detection probe doesn't).
    UsbMode(String, bool),
    /// The mode the board starts in next time (no change now).
    BootMode(String),
    Stop(tokio::sync::oneshot::Sender<()>),
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct BackendStatus {
    pub backend: String,
    pub connected: bool,
    pub detail: Option<String>,
    pub rtt_ms: Option<f64>,
    pub usb: Option<String>,
    pub usb_mode: Option<String>,
    /// "switch2" once a Switch 2 has started the board's Pro Controller.
    pub host: Option<String>,
    /// The bridge reports `host` at all (older bridges don't: then "no Switch 2 handshake" means
    /// nothing, and detection must not run).
    pub reports_host: bool,
    /// The Bridge restarts for a mode switch until then: its disconnect is expected.
    #[serde(skip)]
    pub restarting_until: Option<Instant>,
    /// A USB mode switch is under way on the board (its devices are closed): presses would be
    /// dropped, so they're refused until it says "switched" (or this time passes).
    #[serde(skip)]
    pub switching_until: Option<Instant>,
    pub devices: Vec<String>,
    pub writes: HashMap<String, u64>,
}

pub struct Backend {
    pub status: Arc<Mutex<BackendStatus>>,
    tx: Mutex<mpsc::UnboundedSender<Msg>>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    on_connect: mpsc::UnboundedSender<()>,
}

impl Backend {
    pub fn send(&self, m: Msg) -> Result<()> {
        if !self.status.lock().unwrap().connected {
            let d = self.status.lock().unwrap().detail.clone().unwrap_or_default();
            bail!("NS2Bench Bridge not connected ({d})");
        }
        self.tx.lock().unwrap().send(m).map_err(|_| anyhow!("input backend stopped"))
    }
    pub async fn replace(&self, cfg: &Value, logs: LogHub) {
        let (done, ack) = tokio::sync::oneshot::channel();
        let _ = self.tx.lock().unwrap().send(Msg::Stop(done));
        let _ = tokio::time::timeout(Duration::from_millis(750), ack).await;
        let old = self.worker.lock().unwrap().take();
        if let Some(old) = old { old.abort(); let _ = old.await; }
        let new = spawn_backend(cfg, logs, self.on_connect.clone(), self.status.clone());
        *self.tx.lock().unwrap() = new.tx.into_inner().unwrap();
        *self.worker.lock().unwrap() = new.worker.into_inner().unwrap();
    }
    pub fn devices(&self) -> Vec<String> {
        self.status.lock().unwrap().devices.clone()
    }
}

/// Start the configured backend task; returns the handle used by the hub.
pub fn start_backend(cfg: &Value, logs: LogHub, on_connect: mpsc::UnboundedSender<()>) -> Backend {
    spawn_backend(cfg, logs, on_connect, Arc::new(Mutex::new(BackendStatus::default())))
}

fn spawn_backend(cfg: &Value, logs: LogHub, on_connect: mpsc::UnboundedSender<()>, status: Arc<Mutex<BackendStatus>>) -> Backend {
    let (tx, rx) = mpsc::unbounded_channel();
    let kind = cfg["backend"].as_str().unwrap_or("none").to_string();
    *status.lock().unwrap() = BackendStatus { backend: kind.clone(), ..Default::default() };
    let notify = on_connect.clone();
    let worker = match kind.as_str() {
        "hidd" | "serial" => {
            let link = crate::transport::Link::from_config(cfg);
            let token = crate::config::read_secret(cfg["token_file"].as_str());
            Some(tokio::spawn(run_board(link, token, rx, status.clone(), logs, on_connect)))
        }
        "ch9329" => {
            let link = match cfg["serial_port"].as_str() {
                Some(p) => crate::transport::Link::Serial { path: p.into(), baud: cfg["baud"].as_u64().unwrap_or(9600) as u32 },
                None => crate::transport::Link::None,
            };
            Some(tokio::spawn(crate::ch9329::run(link, rx, status.clone(), logs, on_connect)))
        }
        _ => {
            status.lock().unwrap().detail = Some("input disabled (view only)".into());
            None
        }
    };
    Backend { status, tx: Mutex::new(tx), worker: Mutex::new(worker), on_connect: notify }
}

async fn run_board(
    link: crate::transport::Link,
    token: Option<String>,
    mut rx: mpsc::UnboundedReceiver<Msg>,
    status: Arc<Mutex<BackendStatus>>,
    logs: LogHub,
    on_connect: mpsc::UnboundedSender<()>,
) {
    let mut delay = Duration::from_millis(300);
    let mut log_seq: u64 = 0;
    let mut up_since: Option<Instant> = None;
    let mut short_drops: u32 = 0;      // connections in a row that lasted under two minutes
    let mut quiet = false;             // after a drop: failed reconnects are details until it's back
    loop {
        let res: Result<()> = async {
            let (r, mut w) = link.connect().await?;
            let mut reader = BufReader::new(r).lines();
            let hello = json!({"hello": token.clone().unwrap_or_default()});
            board_write(&mut w, format!("{hello}\n").as_bytes()).await?;
            let line = tokio::time::timeout(Duration::from_secs(5), reader.next_line()).await??.unwrap_or_default();
            let h: Value = serde_json::from_str(&line).unwrap_or(json!({}));
            if h["ok"] != "ready" {
                if h["error"] == "auth" { bail!("NS2Bench Bridge: wrong token (this computer's token isn't the one on the board)"); }
                bail!("NS2Bench Bridge refused: {}", h.get("error").cloned().unwrap_or(h.clone()));
            }
            {
                let mut s = status.lock().unwrap();
                s.connected = true;
                s.detail = None;
                s.usb = h["udc"].as_str().map(str::to_string);
                s.usb_mode = Some(h["mode"].as_str().unwrap_or("standard").to_string());
                s.host = h["host"].as_str().map(str::to_string);
                s.reports_host = h.get("host").is_some();
                s.switching_until = None;        // (an older Bridge restarts for a mode switch: it's back)
                s.devices = h["devices"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_else(|| vec!["pad".into(), "kbd".into()]);
                let text = format!("NS2Bench Bridge connected: {} usb={} mode={} devices={}",
                    link.describe(), s.usb.clone().unwrap_or_default(), s.usb_mode.clone().unwrap_or_default(), s.devices.join(","));
                // (a notification only for a real return: while a sleeping console powers the
                // board in short bursts, each of its minute-long visits is just a log line)
                if short_drops == 0 { logs.notify("bench", "info", "Bridge connected", text); } else { logs.add("bench", text); }
            }
            log_seq = h["log_seq"].as_u64().unwrap_or(0).saturating_sub(50);
            delay = Duration::from_millis(300);
            up_since = Some(Instant::now());
            quiet = false;
            let _ = on_connect.send(());
            let mut beat = tokio::time::interval(Duration::from_millis(250));
            let mut n: u64 = 0;
            let mut sent: HashMap<u64, Instant> = HashMap::new();
            let mut last_pong = Instant::now();
            loop {
                tokio::select! {
                    m = rx.recv() => {
                        let Some(m) = m else { return Ok(()) };
                        let out = match m {
                            Msg::State(p, k) => format!("{}\n{}\n", json!({"pad": p}), json!({"kbd": k})),
                            Msg::Mouse(v) => format!("{}\n", json!({"mouse": v})),
                            Msg::Stop(done) => {
                                board_write(&mut w, format!("{}\n{}\n{}\n", json!({"pad": NEUTRAL_PAD}), json!({"kbd": NEUTRAL_KBD}), json!({"mouse": [0,0,0,0]})).as_bytes()).await?;
                                let _ = done.send(());
                                return Ok(());
                            }
                            Msg::UsbMode(mode, save) => format!("{}\n", json!({"usb_mode": mode, "save": save})),
                            Msg::BootMode(mode) => format!("{}\n", json!({"boot_mode": mode})),
                        };
                        board_write(&mut w, out.as_bytes()).await?;
                    }
                    _ = beat.tick() => {
                        // After a run of short visits (a sleeping console), a connection that has
                        // held for two minutes is the real return: said once, now.
                        if short_drops > 0 && up_since.is_some_and(|t| t.elapsed() >= Duration::from_secs(120)) {
                            short_drops = 0;
                            logs.notify("bench", "info", "Bridge connected", format!("NS2Bench Bridge is back and staying connected: {}", link.describe()));
                        }
                        if last_pong.elapsed() >= Duration::from_secs(3) {
                            bail!("NS2Bench Bridge heartbeat timed out; reconnecting");
                        }
                        n += 1;
                        sent.retain(|k, _| *k + 20 > n);
                        sent.insert(n, Instant::now());
                        let msg = if n.is_multiple_of(4) { json!({"ping": n, "logs": log_seq}) } else { json!({"ping": n}) };
                        board_write(&mut w, format!("{msg}\n").as_bytes()).await?;
                    }
                    line = reader.next_line() => {
                        let Some(line) = line? else { bail!("NS2Bench Bridge closed the connection") };
                        let m: Value = serde_json::from_str(&line).unwrap_or(json!({}));
                        if let Some(p) = m.get("pong").and_then(Value::as_u64) {
                            last_pong = Instant::now();
                            let mut s = status.lock().unwrap();
                            if let Some(t0) = sent.remove(&p) { s.rtt_ms = Some((t0.elapsed().as_secs_f64() * 10000.0).round() / 10.0); }
                            s.usb = m["udc"].as_str().map(str::to_string).or(s.usb.clone());
                            // (newer bridges say the mode really running, and who started the controller)
                            if let Some(mode) = m["mode"].as_str() { s.usb_mode = Some(mode.to_string()); }
                            if m.get("host").is_some() { s.host = m["host"].as_str().map(str::to_string); s.reports_host = true; }
                            if let Some(wr) = m["writes"].as_object() {
                                s.writes = wr.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect();
                            }
                        } else if let Some(entries) = m.get("logs").and_then(Value::as_array) {
                            // boards often have no real clock (offline, no RTC): shift their times by
                            // the difference between the board's "now" and ours
                            let ours = crate::logs::now();
                            let shift = m["now"].as_f64().map_or(0.0, |b| ours - b);
                            for e in entries {
                                let seq = e[0].as_u64().unwrap_or(0);
                                log_seq = log_seq.max(seq);
                                let src = if e[2] == "kernel" { "kernel" } else { "board" };
                                // (an older Bridge stamps with a wall clock that jumps during boot: a line that
                                // lands weeks away or in the future is given the time it arrived)
                                let t = e[1].as_f64().map(|t| t + shift).filter(|t| *t <= ours + 5.0 && *t > ours - 14.0 * 86400.0);
                                logs.add_guess(src, e[3].as_str().unwrap_or(""), t);
                            }
                        } else if m["stays"] == true {
                            // a newer Bridge switches without restarting: no disconnect to expect
                            status.lock().unwrap().restarting_until = None;
                        } else if let Some(mode) = m.get("switched").and_then(Value::as_str) {
                            // a mode switch done without dropping the connection (newer Bridges)
                            let mut s = status.lock().unwrap();
                            s.usb_mode = Some(mode.to_string());
                            if let Some(d) = m["devices"].as_array() { s.devices = d.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(); }
                            s.usb = m["udc"].as_str().map(str::to_string).or(s.usb.clone());
                            s.host = m["host"].as_str().map(str::to_string);
                            s.switching_until = None;
                            s.restarting_until = None;
                            drop(s);
                            logs.add("bench", format!("NS2Bench Bridge: now in {mode} mode (switched without reconnecting)"));
                            let _ = on_connect.send(());     // re-send what's held, as after a reconnect
                        } else if let Some(err) = m.get("error") {
                            // (a refused mode switch isn't under way: presses go through again.
                            // Other errors, like a mouse move while the devices are closed, or
                            // "already under way", say nothing of the kind)
                            let text = err.as_str().unwrap_or("").to_lowercase();
                            if !text.contains("already under way") && (text.contains("usb_mode") || text.contains("usb mode") || text.contains("mode switch")) {
                                status.lock().unwrap().switching_until = None;
                            }
                            logs.error("board", format!("error: {err}"));
                            status.lock().unwrap().detail = Some(err.to_string());
                        }
                    }
                }
            }
        }.await;
        if res.is_ok() { return; }
        let was = { let mut s = status.lock().unwrap(); let w = s.connected; s.connected = false; s.host = None; w };
        let detail = match res { Ok(()) => "stopped".to_string(), Err(e) => e.to_string() };
        let expected = status.lock().unwrap().restarting_until.is_some_and(|t| Instant::now() < t);
        if expected {
            // our own mode switch: the Bridge restarts, nothing went wrong
            if was { logs.add("bench", "NS2Bench Bridge: restarting for the USB mode change"); }
        } else if was {
            // Usually the console went to sleep (the board powers from its USB port): logged, not
            // a notification. watch_controller notifies when it stays away while a console is on.
            // A sleeping console powers its USB port in short bursts: the board starts, connects
            // and loses power again within a minute, over and over. One warning for the first
            // drop; the repeats are ordinary lines, not a wall of warnings.
            let lasted = up_since.take().map_or(0, |t| t.elapsed().as_secs());
            short_drops = if lasted < 120 { short_drops + 1 } else { 0 };
            quiet = true;
            if short_drops >= 1 {
                logs.add("bench", format!("NS2Bench Bridge: lost again after {lasted} s ({short_drops} short connections in a row). Usual while the console sleeps: its USB port gives power only in short bursts"));
            } else {
                logs.warn("bench", format!("NS2Bench Bridge: {detail}"));
            }
        } else if status.lock().unwrap().detail.as_deref() != Some(detail.as_str()) {
            if quiet { logs.debug("bench", format!("NS2Bench Bridge: {detail}")); } else { logs.warn("bench", format!("NS2Bench Bridge: {detail}")); }
        }
        // (a wrong token doesn't fix itself in half a second: asked again every 10 s)
        let wrong_token = detail.contains("wrong token");
        status.lock().unwrap().detail = Some(detail);
        if wrong_token { tokio::time::sleep(Duration::from_secs(10)).await; }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_millis(500));
    }
}

async fn board_write(w: &mut crate::transport::Writer, data: &[u8]) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), w.write_all(data))
        .await.map_err(|_| anyhow!("NS2Bench Bridge write timed out; reconnecting"))??;
    Ok(())
}

// ------------------------------------------------------------------ hub

#[derive(Clone, Default)]
struct Layer {
    buttons: u32,
    dpad: u8,
    sticks: Option<[u8; 4]>,
    kbd_mods: u8,
    kbd_keys: Vec<u8>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Event {
    pub t: f64,
    pub source: String,
    pub action: String,
    #[serde(flatten)]
    pub detail: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug)]
pub enum Push {
    Pad { pad: Pad, agent: [u32; 2] },
    Event(Event),
}

/// Runs its closure when dropped: undoes a timed press even if the future holding it is cancelled.
struct OnDrop<F: FnMut()>(F);
impl<F: FnMut()> Drop for OnDrop<F> {
    fn drop(&mut self) { (self.0)() }
}

struct HubState {
    ui: Layer,
    agent: Layer,
    last_pad: Pad,
    last_kbd: Kbd,
    /// Mouse buttons held by the person and by agents (sent as their union).
    ui_mouse: u8,
    agent_mouse: u8,
    ui_down: HashMap<(char, u32), Instant>,
    ui_want: (u32, u8, Option<[u8; 4]>, u8, Vec<u8>),
    ui_timer: bool,
}

pub struct InputHub {
    st: Mutex<HubState>,
    pub backend: Backend,
    pub manual_only: std::sync::atomic::AtomicBool,
    pub min_press_ms: std::sync::atomic::AtomicU64,
    pub events: Mutex<std::collections::VecDeque<Event>>,
    pub tx: broadcast::Sender<Push>,
    agent_lock: tokio::sync::Mutex<()>,
    logs: LogHub,
    /// The page (socket number) that last left something held: only that page going silent or
    /// closing releases the person's input, not another, idle page.
    pub ui_owner: std::sync::atomic::AtomicU64,
    /// Set while nothing may be pressed (several consoles, and which one the controller is
    /// plugged into isn't known). Letting go still goes through.
    pub paused: Mutex<Option<String>>,
    /// Bumped whenever the controller moves to another console (or its place becomes unknown).
    pub generation: std::sync::atomic::AtomicU64,
    /// When NS2Bench last switched the board's USB mode: the board drops off and comes back for
    /// a while after, which is not the cable moving.
    pub mode_switched: Mutex<Option<std::time::Instant>>,
    /// When the person last said which console the controller is on.
    pub placed: Mutex<Option<std::time::Instant>>,
    /// The last time the controller's cable could have moved: the board came (back) on, or its
    /// USB cable was out and in again. No such moment since it was placed, with the board there
    /// all along, means it is still in the console it was placed on.
    pub cable_event: Mutex<Option<std::time::Instant>>,
    /// Whether the last placement was more than a guess: the person said where the cable is, it
    /// was found, or the cable was seen to move after a console was picked. Following the screen
    /// with nothing seen is a guess: it must not later count as proof of where the cable is.
    pub placed_sure: std::sync::atomic::AtomicBool,
    /// The console the person picked on screen while the cable was provably still in another
    /// one (and when): the controller goes there once the cable is seen to move.
    pub pending_follow: Mutex<Option<(String, std::time::Instant)>>,
    /// The person picked the board's mode by hand: automatic mode changes leave it alone until
    /// the controller is next placed on a console.
    pub manual_mode: std::sync::atomic::AtomicBool,
    /// Automatic switches to the console's mode since it was placed (at most 2: a board that
    /// can't reach a mode must not be switched over and over).
    pub mode_tries: std::sync::atomic::AtomicU8,
    /// Set after every connect and finished mode switch: the watcher then makes sure the board
    /// is in the mode its console needs (a placement or choice made while a switch was still
    /// running was lost: the board said "already switching" and nothing asked again).
    pub mode_check: std::sync::atomic::AtomicBool,
}

tokio::task_local! {
    /// The controller generation an input request started with: if it changes while the request
    /// still runs (a long sequence or test), that request may press nothing more.
    pub static RUN_GENERATION: Option<u64>;
}

impl InputHub {
    pub fn new(backend: Backend, logs: LogHub) -> Arc<InputHub> {
        let (tx, _) = broadcast::channel(512);
        Arc::new(InputHub {
            st: Mutex::new(HubState {
                ui: Layer::default(), agent: Layer::default(), last_pad: NEUTRAL_PAD, last_kbd: NEUTRAL_KBD,
                ui_mouse: 0, agent_mouse: 0, ui_down: HashMap::new(), ui_want: (0, 0, None, 0, vec![]), ui_timer: false,
            }),
            backend,
            manual_only: false.into(),
            min_press_ms: crate::calib::DEFAULT_MIN_PRESS_MS.into(),
            events: Mutex::new(Default::default()),
            tx,
            agent_lock: tokio::sync::Mutex::new(()),
            logs,
            ui_owner: 0.into(),
            paused: Mutex::new(None),
            generation: 0.into(),
            mode_switched: Mutex::new(None),
            placed: Mutex::new(None), cable_event: Mutex::new(None), placed_sure: false.into(), pending_follow: Mutex::new(None),
            manual_mode: false.into(),
            mode_tries: 0.into(), mode_check: false.into(),
        })
    }

    pub fn configuration_guard(&self) -> Result<tokio::sync::MutexGuard<'_, ()>> {
        self.agent_lock.try_lock().map_err(|_| anyhow!("Agent input is active; wait for it to finish before changing the input device"))
    }

    pub fn min_press(&self) -> u64 {
        self.min_press_ms.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn merged(st: &HubState) -> (Pad, Kbd) {
        let (ui, ag) = (&st.ui, &st.agent);
        let buttons = ui.buttons | ag.buttons;
        let hat = dpad_to_hat(if ui.dpad != 0 { ui.dpad } else { ag.dpad });
        let sticks = ui.sticks.or(ag.sticks).unwrap_or([CENTER; 4]);
        let kl = if ui.kbd_mods != 0 || !ui.kbd_keys.is_empty() { ui } else { ag };
        let mut kbd = [0u8; 7];
        kbd[0] = kl.kbd_mods;
        for (i, k) in kl.kbd_keys.iter().take(6).enumerate() {
            kbd[i + 1] = *k;
        }
        ([buttons, hat as u32, sticks[0] as u32, sticks[1] as u32, sticks[2] as u32, sticks[3] as u32], kbd)
    }

    fn flush(&self) -> Result<()> {
        // One flush at a time, from reading the state to sending it: two running side by side
        // (an agent's release and the page's own state) could send the older state last, leaving
        // a button held on the console that everything here shows as released.
        static ORDER: Mutex<()> = Mutex::new(());
        let _one = ORDER.lock().unwrap_or_else(|e| e.into_inner());
        let (pad, kbd, changed, agent) = {
            let mut st = self.st.lock().unwrap();
            let (pad, kbd) = Self::merged(&st);
            let changed = pad != st.last_pad;
            st.last_pad = pad;
            st.last_kbd = kbd;
            (pad, kbd, changed, [st.agent.buttons, st.agent.dpad as u32])
        };
        if pad != NEUTRAL_PAD || kbd != NEUTRAL_KBD {
            if let Some(why) = self.paused.lock().unwrap().clone() {
                bail!(Conflict(why));
            }
            self.check_switching()?;
            self.check_generation()?;
        }
        if changed {
            let _ = self.tx.send(Push::Pad { pad, agent });
        }
        self.backend.send(Msg::State(pad, kbd))
    }

    /// After a reconnect: the device starts neutral, so send what is held now, mouse included.
    pub fn resend(&self) {
        let _ = self.flush();
        let b = { let st = self.st.lock().unwrap(); st.ui_mouse | st.agent_mouse };
        if self.backend.devices().iter().any(|d| d == "mouse") {
            let _ = self.backend.send(Msg::Mouse([b as i32, 0, 0, 0]));
        }
    }

    /// Let go of everything a source holds, without the Block-agents check: cleanup after a
    /// stopped test or sequence must always work.
    pub fn release_source(&self, source: &str) {
        if source == "ui" {
            { let mut st = self.st.lock().unwrap(); st.ui = Layer::default(); st.ui_down.clear(); }
            let _ = self.flush();
            self.mouse_up(true, false);
        } else {
            self.clear_agent();
        }
    }

    pub fn log(&self, source: &str, action: &str, detail: Value) {
        let detail = detail.as_object().cloned().unwrap_or_default();
        let ev = Event { t: crate::logs::now(), source: source.into(), action: action.into(), detail: detail.clone() };
        let pretty: Vec<String> = detail.iter().map(|(k, v)| format!("{k}={v}")).collect();
        if (source == "api" || source == "mcp") && !self.logs.throttled(&format!("agent-active-{source}"), 60.0) {
            self.logs.notify("bench", "info", "An agent is driving", format!("{source}: an agent started driving the console (Block agents in the Driving card stops it)"));
        }
        self.logs.add("bench", format!("{source}: {action} {}", pretty.join(" ")));
        let mut evs = self.events.lock().unwrap();
        evs.push_back(ev.clone());
        if evs.len() > 200 {
            evs.pop_front();
        }
        let _ = self.tx.send(Push::Event(ev));
    }

    fn check_agent(&self, source: &str) -> Result<()> {
        if source != "ui" && self.manual_only.load(std::sync::atomic::Ordering::Relaxed) {
            bail!(Locked);
        }
        Ok(())
    }

    fn layer<'a>(st: &'a mut HubState, source: &str) -> &'a mut Layer {
        if source == "ui" { &mut st.ui } else { &mut st.agent }
    }

    /// Human realtime state. Buttons/keys released sooner than the calibrated minimum press
    /// stay down until it has passed (the console drops shorter presses).
    pub fn set_ui_state(self: &Arc<Self>, buttons: u32, dpad: u8, sticks: Option<[u8; 4]>, mods: u8, keys: Vec<u8>) -> Result<()> {
        let now = Instant::now();
        let hold = Duration::from_millis(self.min_press());
        let (want_b, want_d, keys6) = (buttons & 0x1FFFF, dpad & 0xF, keys.into_iter().take(6).collect::<Vec<u8>>());
        let mut early_wait: Option<Duration> = None;
        {
            let mut st = self.st.lock().unwrap();
            let mut down = std::mem::take(&mut st.ui_down);
            let held_now = |k: &(char, u32)| match k.0 {
                'b' => want_b >> k.1 & 1 == 1,
                'd' => want_d >> k.1 & 1 == 1,
                _ => keys6.contains(&(k.1 as u8)),
            };
            for i in 0..17 { if want_b >> i & 1 == 1 { down.entry(('b', i)).or_insert(now); } }
            for i in 0..4 { if want_d >> i & 1 == 1 { down.entry(('d', i)).or_insert(now); } }
            for k in &keys6 { down.entry(('k', *k as u32)).or_insert(now); }
            let (mut b, mut d, mut ks) = (want_b, want_d, keys6.clone());
            down.retain(|k, t| {
                if held_now(k) { return true; }
                let age = now.duration_since(*t);
                if age < hold {
                    match k.0 { 'b' => b |= 1 << k.1, 'd' => d |= 1 << k.1, _ => if ks.len() < 6 && !ks.contains(&(k.1 as u8)) { ks.push(k.1 as u8) } }
                    let left = hold - age;
                    early_wait = Some(early_wait.map_or(left, |e: Duration| e.min(left)));
                    true
                } else { false }
            });
            st.ui_down = down;
            st.ui_want = (buttons, dpad, sticks, mods, keys6.clone());
            st.ui = Layer {
                buttons: b, dpad: d,
                sticks: sticks.filter(|s| *s != [CENTER; 4]),
                kbd_mods: mods, kbd_keys: ks,
            };
            if early_wait.is_some() && st.ui_timer { early_wait = None; } else if early_wait.is_some() { st.ui_timer = true; }
        }
        let r = self.flush();
        if let Some(w) = early_wait {
            let hub = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(w + Duration::from_millis(2)).await;
                let want = { let mut st = hub.st.lock().unwrap(); st.ui_timer = false; st.ui_want.clone() };
                let _ = hub.set_ui_state(want.0, want.1, want.2, want.3, want.4);
            });
        }
        r
    }

    pub async fn press(&self, names: &[String], hold_ms: u64, source: &str, exact: bool) -> Result<()> {
        let (mask, dpad) = parse_buttons(names)?;
        let floor = if exact { 8 } else { self.min_press() };
        let hold = hold_ms.clamp(floor, MAX_HOLD_MS);
        let _g = self.agent_lock.lock().await;
        self.check_agent(source)?;
        if source != "calibrate" {
            self.log(source, "press", json!({"buttons": names, "hold_ms": hold}));
        }
        {
            let mut st = self.st.lock().unwrap();
            let l = Self::layer(&mut st, source);
            l.buttons |= mask;
            l.dpad |= dpad;
        }
        // Released by the guard, also when the caller goes away mid-hold (request cancelled).
        let release = OnDrop(|| {
            {
                let mut st = self.st.lock().unwrap();
                let l = Self::layer(&mut st, source);
                l.buttons &= !mask;
                l.dpad &= !dpad;
            }
            let _ = self.flush();
        });
        let r = self.flush();
        // (a press that couldn't be sent isn't held: the caller hears why right away)
        if r.is_ok() { tokio::time::sleep(Duration::from_millis(hold)).await; }
        drop(release);
        r
    }

    pub fn hold(&self, names: &[String], source: &str) -> Result<()> {
        let (mask, dpad) = parse_buttons(names)?;
        self.check_agent(source)?;
        self.log(source, "hold", json!({"buttons": names}));
        let before = { let mut st = self.st.lock().unwrap(); let l = Self::layer(&mut st, source); let b = (l.buttons, l.dpad); l.buttons |= mask; l.dpad |= dpad; b };
        self.flush().inspect_err(|_| {
            // Not sent (Bridge offline): don't let it take effect later on reconnect.
            let mut st = self.st.lock().unwrap();
            let l = Self::layer(&mut st, source);
            (l.buttons, l.dpad) = before;
        })
    }

    pub fn release(&self, names: Option<&[String]>, source: &str) -> Result<()> {
        if source != "ui" { self.check_agent(source)?; }
        {
            let mut st = self.st.lock().unwrap();
            let l = Self::layer(&mut st, source);
            match names {
                Some(n) if !n.is_empty() => { let (m, d) = parse_buttons(n)?; l.buttons &= !m; l.dpad &= !d; }
                _ => *l = Layer::default(),
            }
        }
        self.log(source, "release", json!({"buttons": names.map(|n| n.to_vec()).unwrap_or(vec!["all".into()])}));
        // "everything" includes mouse buttons held with mouse_move {buttons}
        if names.is_none_or(|n| n.is_empty()) { self.mouse_up(source == "ui", source != "ui"); }
        self.flush()
    }

    pub async fn stick(&self, which: &str, x: f64, y: f64, hold_ms: Option<u64>, source: &str) -> Result<()> {
        self.check_agent(source)?;
        let i = match which.to_lowercase().as_str() {
            "left" | "l" => 0,
            "right" | "r" => 2,
            _ => anyhow::bail!("stick must be \"left\" or \"right\", not {which:?}"),
        };
        self.log(source, "stick", json!({"stick": which, "x": x, "y": y, "hold_ms": hold_ms}));
        let set = |x: u8, y: u8| {
            let mut st = self.st.lock().unwrap();
            let l = Self::layer(&mut st, source);
            let before = l.sticks;
            let mut cur = before.unwrap_or([CENTER; 4]);
            cur[i] = x;
            cur[i + 1] = y;
            l.sticks = if cur == [CENTER; 4] { None } else { Some(cur) };
            before
        };
        let before = set(axis(x), axis(y));
        if let Err(e) = self.flush() {
            // Not sent (Bridge offline): don't let it take effect later on reconnect.
            let mut st = self.st.lock().unwrap();
            Self::layer(&mut st, source).sticks = before;
            return Err(e);
        }
        if let Some(h) = hold_ms {
            // Centred by the guard, also when the caller goes away mid-hold.
            let _center = OnDrop(|| { set(CENTER, CENTER); let _ = self.flush(); });
            tokio::time::sleep(Duration::from_millis(h.clamp(16, MAX_HOLD_MS))).await;
        }
        Ok(())
    }

    async fn tap(&self, source: &str, mods: u8, code: u8, hold_ms: u64) -> Result<()> {
        { let mut st = self.st.lock().unwrap(); let l = Self::layer(&mut st, source); l.kbd_mods = mods; l.kbd_keys = vec![code]; }
        let release = OnDrop(|| {
            { let mut st = self.st.lock().unwrap(); let l = Self::layer(&mut st, source); l.kbd_mods = 0; l.kbd_keys.clear(); }
            let _ = self.flush();
        });
        let r = self.flush();
        if r.is_ok() { tokio::time::sleep(Duration::from_millis(hold_ms.clamp(16, 2000))).await; }
        drop(release);
        r
    }

    pub async fn key(&self, name: &str, modifiers: &[String], hold_ms: u64, source: &str) -> Result<()> {
        let code = key_code(name)?;
        let mut mods = 0;
        for m in modifiers { mods |= modifier_bit(m).ok_or_else(|| anyhow!("unknown modifier {m}"))?; }
        let _g = self.agent_lock.lock().await;
        self.check_agent(source)?;
        self.log(source, "key", json!({"key": name, "modifiers": modifiers}));
        self.tap(source, mods, code, hold_ms.max(self.min_press())).await
    }

    pub async fn type_text(&self, text: &str, delay_ms: u64, source: &str) -> Result<()> {
        if text.chars().count() > 2000 { bail!("text too long (max 2000 characters)"); }
        let strokes: Vec<(u8, u8)> = text.chars().map(char_to_key).collect::<Result<_>>()?;
        let _g = self.agent_lock.lock().await;
        self.check_agent(source)?;
        // the length only: typed text can be a password, and logs end up in bug reports
        self.log(source, "type", json!({"characters": text.chars().count()}));
        // delay_ms is the time per character: half held, half released (each at least the minimum press)
        let delay_ms = delay_ms.clamp(16, 2000);
        let hold = (delay_ms / 2).max(self.min_press()).min(1000);
        // released long enough that a doubled letter ("ll") is two presses
        let gap = delay_ms.saturating_sub(hold).max(40);
        for (m, c) in strokes {
            // Block agents stops typing between keys, not only before the first one.
            self.check_agent(source)?;
            self.tap(source, m, c, hold).await?;
            tokio::time::sleep(Duration::from_millis(gap)).await;
        }
        Ok(())
    }

    pub fn mouse(&self, dx: i32, dy: i32, wheel: i32, buttons: Option<u8>, source: &str) -> Result<()> {
        self.check_agent(source)?;
        self.mouse_send(dx, dy, wheel, buttons.map(|b| (0x1F, b)), source)
    }

    /// Send a mouse report. `buttons` is (mask, value): the bits of this source's buttons to change.
    fn mouse_send(&self, dx: i32, dy: i32, wheel: i32, buttons: Option<(u8, u8)>, source: &str) -> Result<()> {
        if !self.backend.devices().iter().any(|d| d == "mouse") {
            bail!("NS2Bench Bridge has no USB mouse in this mode");
        }
        if dx != 0 || dy != 0 || wheel != 0 || buttons.is_some_and(|(m, v)| v & m != 0) {
            if let Some(why) = self.paused.lock().unwrap().clone() {
                bail!(Conflict(why));
            }
            self.check_switching()?;
            self.check_generation()?;
        }
        let (b, before) = {
            let mut st = self.st.lock().unwrap();
            let mine = if source == "ui" { &mut st.ui_mouse } else { &mut st.agent_mouse };
            let before = *mine;
            if let Some((mask, v)) = buttons { *mine = (*mine & !mask) | (v & mask & 0x1F); }
            (st.ui_mouse | st.agent_mouse, before)
        };
        self.backend.send(Msg::Mouse([b as i32, dx.clamp(-4096, 4096), dy.clamp(-4096, 4096), wheel.clamp(-64, 64)]))
            .inspect_err(|_| {
                // A press that wasn't sent isn't down on the console: forget it. A release that
                // wasn't sent stays released here (the device is neutral after it reconnects).
                let mut st = self.st.lock().unwrap();
                let mine = if source == "ui" { &mut st.ui_mouse } else { &mut st.agent_mouse };
                *mine &= before;
            })
    }

    pub async fn click(&self, button: &str, hold_ms: u64, source: &str) -> Result<()> {
        let bit = match button { "left" => 1, "right" => 2, "middle" => 4, _ => bail!("button is left, right or middle") };
        let _g = self.agent_lock.lock().await;
        self.check_agent(source)?;
        self.log(source, "click", json!({"button": button}));
        // Released by the guard (not subject to Block agents), also when the caller goes away mid-hold.
        let release = OnDrop(|| { let _ = self.mouse_send(0, 0, 0, Some((bit, 0)), source); });
        let r = self.mouse_send(0, 0, 0, Some((bit, bit)), source);
        if r.is_ok() {
            tokio::time::sleep(Duration::from_millis(hold_ms.clamp(self.min_press().min(2000), 2000))).await;
        }
        drop(release);
        r
    }

    /// Let go of mouse buttons: the person's (`ui`), agents' (`agent`), or both.
    fn mouse_up(&self, ui: bool, agent: bool) {
        let (was, now) = {
            let mut st = self.st.lock().unwrap();
            let was = st.ui_mouse | st.agent_mouse;
            if ui { st.ui_mouse = 0; }
            if agent { st.agent_mouse = 0; }
            (was, st.ui_mouse | st.agent_mouse)
        };
        if was != now {
            let _ = self.backend.send(Msg::Mouse([now as i32, 0, 0, 0]));
        }
    }

    /// Is the person at the page holding anything (a button, key, stick or mouse button)?
    pub fn ui_active(&self) -> bool {
        let st = self.st.lock().unwrap();
        let l = &st.ui;
        l.buttons != 0 || l.dpad != 0 || l.kbd_mods != 0 || !l.kbd_keys.is_empty() || st.ui_mouse != 0
            || l.sticks.is_some_and(|s| s.iter().any(|v| v.abs_diff(CENTER) > 8))
    }

    /// The page went away: release what the person was holding.
    pub fn release_ui(self: &Arc<Self>) {
        let _ = self.set_ui_state(0, 0, None, 0, vec![]);
        self.mouse_up(true, false);
    }

    /// Switch the board's USB mode (it rebuilds its USB side and reconnects).
    pub fn switch_usb_mode(&self, mode: &str) -> Result<()> {
        self.switch_usb_mode_as(mode, true)
    }

    /// `save`: the board starts in this mode next time (false for a quick look, like detection's).
    pub fn switch_usb_mode_as(&self, mode: &str, save: bool) -> Result<()> {
        // (nothing is marked when it can't be sent: a board that's away isn't switching)
        self.backend.send(Msg::UsbMode(mode.to_string(), save))?;
        self.release_all();
        let now = std::time::Instant::now();
        *self.mode_switched.lock().unwrap() = Some(now);
        let mut s = self.backend.status.lock().unwrap();
        // the Bridge may restart for it: its disconnect is expected, not a problem to report
        s.restarting_until = Some(now + Duration::from_secs(45));
        s.switching_until = Some(now + Duration::from_secs(27));    // (the board gives a switch 25 s; "switched" ends this early)
        Ok(())
    }

    /// Refuse a press while the board switches USB mode (it would be acknowledged and lost).
    fn check_switching(&self) -> Result<()> {
        if self.backend.status.lock().unwrap().switching_until.is_some_and(|t| Instant::now() < t) {
            bail!(Conflict("the controller is changing mode (a few seconds): try again in a moment".into()));
        }
        Ok(())
    }

    /// Within this long after a mode switch, a board that drops off and comes back did so because
    /// of the switch.
    pub fn switching_mode(&self) -> bool {
        // (over as soon as a Bridge that stays connected says "switched")
        let s = self.backend.status.lock().unwrap();
        let pending = s.switching_until.is_some() || s.restarting_until.is_some_and(|t| Instant::now() < t);
        pending && self.mode_switched.lock().unwrap().is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(45))
    }

    /// Refuse a press from a request that started before the controller moved.
    fn check_generation(&self) -> Result<()> {
        if let Ok(Some(started)) = RUN_GENERATION.try_with(|g| *g) {
            if started != self.generation.load(std::sync::atomic::Ordering::SeqCst) {
                bail!(Conflict("the controller was moved to another console while this was running: stopped".into()));
            }
        }
        Ok(())
    }

    pub fn release_all(&self) {
        { let mut st = self.st.lock().unwrap(); st.ui = Layer::default(); st.agent = Layer::default(); st.ui_down.clear(); }
        let _ = self.flush();
        self.mouse_up(true, true);
    }

    pub fn clear_agent(&self) {
        self.st.lock().unwrap().agent = Layer::default();
        let _ = self.flush();
        self.mouse_up(false, true);
    }

    pub fn status(&self) -> Value {
        let b = self.backend.status.lock().unwrap().clone();
        let st = self.st.lock().unwrap();
        json!({
            "backend": b.backend, "connected": b.connected, "detail": b.detail, "rtt_ms": b.rtt_ms, "usb": b.usb,
            "usb_mode": b.usb_mode, "host": b.host, "reports_host": b.reports_host, "devices": b.devices, "writes": b.writes,
            "agent_busy": self.agent_lock.try_lock().is_err(),
            "manual_only": self.manual_only.load(std::sync::atomic::Ordering::Relaxed),
            "min_press_ms": self.min_press(), "pad": st.last_pad, "kbd": st.last_kbd,
        })
    }
}

/// Marker error: agent input refused because manual-only mode is on (HTTP 409).
#[derive(Debug)]
pub struct Locked;
impl std::fmt::Display for Locked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "manual-only mode is on: agent input is blocked in the UI")
    }
}
impl std::error::Error for Locked {}

/// Input refused for a reason the person can fix (the controller is on another console, or
/// where it is isn't known): HTTP 409 with this text.
#[derive(Debug)]
pub struct Conflict(pub String);
impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}
impl std::error::Error for Conflict {}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[tokio::test]
    async fn silent_board_connection_is_replaced_after_power_loss() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::unbounded_channel();
        let (connected, mut events) = mpsc::unbounded_channel();
        let status = Arc::new(Mutex::new(BackendStatus::default()));
        let task = tokio::spawn(run_board(
            crate::transport::Link::Tcp { host: "127.0.0.1".into(), port },
            None, rx, status, LogHub::new(None), connected,
        ));
        // A powered-off board can leave a half-open TCP connection. Keep the
        // first socket open but stop answering, then emulate its next boot.
        let (mut old, _) = listener.accept().await.unwrap();
        old.write_all(b"{\"ok\":\"ready\",\"udc\":\"configured\"}\n").await.unwrap();
        events.recv().await.unwrap();
        let replacement = tokio::time::timeout(Duration::from_secs(7), listener.accept()).await;
        assert!(replacement.is_ok(), "silent old connection prevented reconnection");
        let (mut replacement, _) = replacement.unwrap().unwrap();
        replacement.write_all(b"{\"ok\":\"ready\",\"udc\":\"configured\"}\n").await.unwrap();
        let recovered = tokio::time::timeout(Duration::from_secs(1), events.recv()).await;
        task.abort();
        drop(tx);
        assert!(matches!(recovered, Ok(Some(()))), "replacement board did not become ready");
    }

    #[test]
    fn buttons_and_hat() {
        let (m, d) = parse_buttons(&["A".into(), "home".into(), "+".into(), "UP".into(), "dpad_left".into()]).unwrap();
        assert_eq!(m, (1 << 2) | (1 << 12) | (1 << 9));
        assert_eq!(dpad_to_hat(d), 7);
        assert_eq!(dpad_to_hat(1 | 4), HAT_NEUTRAL);
        assert!(parse_buttons(&["TURBO".into()]).is_err());
        assert_eq!(parse_buttons(&["C".into(), "GL".into(), "GR".into()]).unwrap().0, 0x1C000);
    }

    #[test]
    fn keys() {
        assert_eq!((axis(-1.0), axis(0.0), axis(1.0)), (0, 128, 255));
        assert_eq!(char_to_key('A').unwrap(), (2, 0x04));
        assert_eq!(char_to_key('!').unwrap(), (2, 0x1E));
        assert_eq!(char_to_key('/').unwrap(), (0, 0x38));
        assert_eq!(key_code("ArrowUp").unwrap(), 0x52);
        assert_eq!(key_code("KeyQ").unwrap(), 0x14);
        assert_eq!(key_code("Digit0").unwrap(), 0x27);
        assert_eq!(key_code("F12").unwrap(), 0x45);
        assert!(char_to_key('é').is_err());
    }
    pub(crate) fn fake() -> (Arc<InputHub>, mpsc::UnboundedReceiver<Msg>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (on_connect, _) = mpsc::unbounded_channel();
        let backend = Backend { on_connect, tx: Mutex::new(tx), worker: Mutex::new(None), status: Arc::new(Mutex::new(BackendStatus {
            connected: true, ..Default::default()
        })) };
        let hub = InputHub::new(backend, LogHub::new(None));
        hub.min_press_ms.store(0, std::sync::atomic::Ordering::Relaxed);
        (hub, rx)
    }

    #[tokio::test]
    async fn changing_backend_neutralizes_old_link_and_stops_it() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut lines = BufReader::new(r).lines();
            lines.next_line().await.unwrap();
            w.write_all(b"{\"ok\":\"ready\",\"udc\":\"configured\"}\n").await.unwrap();
            let mut pad = false; let mut keyboard = false; let mut mouse = false;
            while let Some(line) = lines.next_line().await.unwrap() {
                let v: Value = serde_json::from_str(&line).unwrap();
                pad |= v["pad"] == json!(NEUTRAL_PAD);
                keyboard |= v["kbd"] == json!(NEUTRAL_KBD);
                mouse |= v["mouse"] == json!([0,0,0,0]);
            }
            assert!(pad && keyboard && mouse, "all devices must be released before disconnect");
        });
        let (notify, mut ready) = mpsc::unbounded_channel();
        let backend = start_backend(&json!({"backend":"hidd","host":"127.0.0.1","port":port}), LogHub::new(None), notify);
        tokio::time::timeout(Duration::from_secs(2), ready.recv()).await.unwrap();
        backend.replace(&json!({"backend":"none"}), LogHub::new(None)).await;
        assert_eq!(backend.status.lock().unwrap().backend, "none");
        assert!(!backend.status.lock().unwrap().connected);
        tokio::time::timeout(Duration::from_secs(2), server).await.unwrap().unwrap();
        assert!(backend.send(Msg::State(NEUTRAL_PAD,NEUTRAL_KBD)).is_err());
    }

    #[tokio::test]
    async fn press_releases_and_manual_only_preserves_human_input() {
        let (hub, mut rx) = fake();
        hub.press(&["A".into()], 16, "mcp", false).await.unwrap();
        assert!(matches!(rx.try_recv().unwrap(), Msg::State([4, ..], _)));
        assert!(matches!(rx.try_recv().unwrap(), Msg::State(NEUTRAL_PAD, _)));
        hub.manual_only.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(hub.press(&["A".into()], 16, "mcp", false).await.unwrap_err().is::<Locked>());
        assert!(rx.try_recv().is_err());
        hub.press(&["A".into()], 16, "ui", false).await.unwrap();
        assert_eq!(hub.status()["pad"], json!(NEUTRAL_PAD));
    }

    #[tokio::test]
    async fn human_and_agent_merge_and_release() {
        let (hub, _rx) = fake();
        hub.set_ui_state(2, 0, None, 0, vec![]).unwrap();
        hub.hold(&["A".into()], "mcp").unwrap();
        assert_eq!(hub.status()["pad"][0], 6);
        hub.stick("left", 0.0, -1.0, None, "mcp").await.unwrap();
        hub.set_ui_state(2, 0, Some([10, 20, 128, 128]), 2, vec![4]).unwrap();
        assert_eq!(hub.status()["pad"], json!([6, 8, 10, 20, 128, 128]));
        assert_eq!(hub.status()["kbd"], json!([2, 4, 0, 0, 0, 0, 0]));
        hub.release_all();
        assert_eq!(hub.status()["pad"], json!(NEUTRAL_PAD));
        assert_eq!(hub.status()["kbd"], json!(NEUTRAL_KBD));
    }

    #[tokio::test]
    async fn short_ui_tap_is_stretched_and_text_validates_before_sending() {
        let (hub, mut rx) = fake();
        assert!(hub.type_text("ok é", 16, "mcp").await.is_err());
        assert!(rx.try_recv().is_err());
        hub.min_press_ms.store(60, std::sync::atomic::Ordering::Relaxed);
        hub.set_ui_state(4, 0, None, 0, vec![]).unwrap();
        hub.set_ui_state(0, 0, None, 0, vec![]).unwrap();
        assert_eq!(hub.status()["pad"][0], 4);
        tokio::time::timeout(Duration::from_secs(2), async {
            while hub.status()["pad"][0] != 0 { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.unwrap();
    }

    #[tokio::test]
    async fn short_key_tap_is_released_after_the_minimum_press() {
        let (hub, _rx) = fake();
        hub.min_press_ms.store(60, std::sync::atomic::Ordering::Relaxed);
        hub.set_ui_state(0, 0, None, 0, vec![4]).unwrap();
        hub.set_ui_state(0, 0, None, 0, vec![]).unwrap();
        assert_eq!(hub.status()["kbd"][1], 4);
        tokio::time::timeout(Duration::from_secs(2), async {
            while hub.status()["kbd"][1] != 0 { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_press_and_blocked_click_let_go() {
        let (hub, mut rx) = fake();
        hub.backend.status.lock().unwrap().devices = vec!["mouse".into()];
        // the caller goes away mid-hold (request cancelled)
        let h = hub.clone();
        let t = tokio::spawn(async move { h.press(&["A".into()], 5000, "api", false).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_ne!(hub.status()["pad"][0], 0);
        t.abort();
        let _ = t.await;
        assert_eq!(hub.status()["pad"][0], 0);
        // Block agents during a click: the button still comes up, and a held one is released
        let h = hub.clone();
        let t = tokio::spawn(async move { h.click("left", 200, "api").await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        hub.manual_only.store(true, std::sync::atomic::Ordering::Relaxed);
        t.await.unwrap().unwrap();
        hub.manual_only.store(false, std::sync::atomic::Ordering::Relaxed);
        hub.mouse(0, 0, 0, Some(1), "api").unwrap();
        hub.clear_agent();
        let mut last = None;
        while let Ok(m) = rx.try_recv() { if let Msg::Mouse(r) = m { last = Some(r[0]); } }
        assert_eq!(last, Some(0));
    }

    #[tokio::test]
    async fn hold_that_was_not_sent_does_not_apply_on_reconnect() {
        let (hub, _rx) = fake();
        hub.backend.status.lock().unwrap().connected = false;
        assert!(hub.hold(&["A".into()], "api").is_err());
        assert!(hub.stick("left", 1.0, 0.0, None, "api").await.is_err());
        hub.backend.status.lock().unwrap().connected = true;
        hub.resend();
        assert_eq!(hub.status()["pad"][0], 0);
        assert_eq!(hub.status()["pad"][2], 128);
    }

}
