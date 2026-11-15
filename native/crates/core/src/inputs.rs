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
    UsbMode(String),
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
    let mut delay = Duration::from_secs(1);
    let mut log_seq: u64 = 0;
    loop {
        let res: Result<()> = async {
            let (r, mut w) = link.connect().await?;
            let mut reader = BufReader::new(r).lines();
            let hello = json!({"hello": token.clone().unwrap_or_default()});
            board_write(&mut w, format!("{hello}\n").as_bytes()).await?;
            let line = tokio::time::timeout(Duration::from_secs(5), reader.next_line()).await??.unwrap_or_default();
            let h: Value = serde_json::from_str(&line).unwrap_or(json!({}));
            if h["ok"] != "ready" {
                bail!("NS2Bench Bridge refused: {}", h.get("error").cloned().unwrap_or(h.clone()));
            }
            {
                let mut s = status.lock().unwrap();
                s.connected = true;
                s.detail = None;
                s.usb = h["udc"].as_str().map(str::to_string);
                s.usb_mode = Some(h["mode"].as_str().unwrap_or("standard").to_string());
                s.devices = h["devices"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_else(|| vec!["pad".into(), "kbd".into()]);
                logs.add("bench", format!("NS2Bench Bridge connected: {} usb={} mode={} devices={}",
                    link.describe(), s.usb.clone().unwrap_or_default(), s.usb_mode.clone().unwrap_or_default(), s.devices.join(",")));
            }
            log_seq = h["log_seq"].as_u64().unwrap_or(0).saturating_sub(50);
            delay = Duration::from_secs(1);
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
                            Msg::UsbMode(mode) => format!("{}\n", json!({"usb_mode": mode})),
                        };
                        board_write(&mut w, out.as_bytes()).await?;
                    }
                    _ = beat.tick() => {
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
                            if let Some(wr) = m["writes"].as_object() {
                                s.writes = wr.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect();
                            }
                        } else if let Some(entries) = m.get("logs").and_then(Value::as_array) {
                            // boards often have no real clock (offline, no RTC): shift their times by
                            // the difference between the board's "now" and ours
                            let shift = m["now"].as_f64().map_or(0.0, |b| crate::logs::now() - b);
                            for e in entries {
                                let seq = e[0].as_u64().unwrap_or(0);
                                log_seq = log_seq.max(seq);
                                let src = if e[2] == "kernel" { "kernel" } else { "board" };
                                logs.add_full(src, e[3].as_str().unwrap_or(""), e[1].as_f64().map(|t| t + shift), "info");
                            }
                        } else if let Some(err) = m.get("error") {
                            logs.error("board", format!("error: {err}"));
                            status.lock().unwrap().detail = Some(err.to_string());
                        }
                    }
                }
            }
        }.await;
        if res.is_ok() { return; }
        let was = { let mut s = status.lock().unwrap(); let w = s.connected; s.connected = false; w };
        let detail = match res { Ok(()) => "stopped".to_string(), Err(e) => e.to_string() };
        if was || status.lock().unwrap().detail.as_deref() != Some(detail.as_str()) {
            logs.error("bench", format!("NS2Bench Bridge: {detail}"));
        }
        status.lock().unwrap().detail = Some(detail);
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(2));
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

struct HubState {
    ui: Layer,
    agent: Layer,
    last_pad: Pad,
    last_kbd: Kbd,
    mouse_buttons: u8,
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
}

impl InputHub {
    pub fn new(backend: Backend, logs: LogHub) -> Arc<InputHub> {
        let (tx, _) = broadcast::channel(512);
        Arc::new(InputHub {
            st: Mutex::new(HubState {
                ui: Layer::default(), agent: Layer::default(), last_pad: NEUTRAL_PAD, last_kbd: NEUTRAL_KBD,
                mouse_buttons: 0, ui_down: HashMap::new(), ui_want: (0, 0, None, 0, vec![]), ui_timer: false,
            }),
            backend,
            manual_only: false.into(),
            min_press_ms: crate::calib::DEFAULT_MIN_PRESS_MS.into(),
            events: Mutex::new(Default::default()),
            tx,
            agent_lock: tokio::sync::Mutex::new(()),
            logs,
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
        let (pad, kbd, changed, agent) = {
            let mut st = self.st.lock().unwrap();
            let (pad, kbd) = Self::merged(&st);
            let changed = pad != st.last_pad;
            st.last_pad = pad;
            st.last_kbd = kbd;
            (pad, kbd, changed, [st.agent.buttons, st.agent.dpad as u32])
        };
        if changed {
            let _ = self.tx.send(Push::Pad { pad, agent });
        }
        self.backend.send(Msg::State(pad, kbd))
    }

    pub fn resend(&self) {
        let _ = self.flush();
    }

    pub fn log(&self, source: &str, action: &str, detail: Value) {
        let detail = detail.as_object().cloned().unwrap_or_default();
        let ev = Event { t: crate::logs::now(), source: source.into(), action: action.into(), detail: detail.clone() };
        let pretty: Vec<String> = detail.iter().map(|(k, v)| format!("{k}={v}")).collect();
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
            st.ui_want = (buttons, dpad, sticks, mods, ks.clone());
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
        let r = self.flush();
        tokio::time::sleep(Duration::from_millis(hold)).await;
        {
            let mut st = self.st.lock().unwrap();
            let l = Self::layer(&mut st, source);
            l.buttons &= !mask;
            l.dpad &= !dpad;
        }
        self.flush()?;
        r
    }

    pub fn hold(&self, names: &[String], source: &str) -> Result<()> {
        let (mask, dpad) = parse_buttons(names)?;
        self.check_agent(source)?;
        self.log(source, "hold", json!({"buttons": names}));
        { let mut st = self.st.lock().unwrap(); let l = Self::layer(&mut st, source); l.buttons |= mask; l.dpad |= dpad; }
        self.flush()
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
        {
            let mut st = self.st.lock().unwrap();
            let l = Self::layer(&mut st, source);
            let mut cur = l.sticks.unwrap_or([CENTER; 4]);
            cur[i] = axis(x);
            cur[i + 1] = axis(y);
            l.sticks = if cur == [CENTER; 4] { None } else { Some(cur) };
        }
        self.flush()?;
        if let Some(h) = hold_ms {
            tokio::time::sleep(Duration::from_millis(h.clamp(16, MAX_HOLD_MS))).await;
            {
                let mut st = self.st.lock().unwrap();
                let l = Self::layer(&mut st, source);
                let mut cur = l.sticks.unwrap_or([CENTER; 4]);
                cur[i] = CENTER;
                cur[i + 1] = CENTER;
                l.sticks = if cur == [CENTER; 4] { None } else { Some(cur) };
            }
            self.flush()?;
        }
        Ok(())
    }

    async fn tap(&self, source: &str, mods: u8, code: u8, hold_ms: u64) -> Result<()> {
        { let mut st = self.st.lock().unwrap(); let l = Self::layer(&mut st, source); l.kbd_mods = mods; l.kbd_keys = vec![code]; }
        let r = self.flush();
        tokio::time::sleep(Duration::from_millis(hold_ms.clamp(16, 2000))).await;
        { let mut st = self.st.lock().unwrap(); let l = Self::layer(&mut st, source); l.kbd_mods = 0; l.kbd_keys.clear(); }
        self.flush()?;
        r
    }

    pub async fn key(&self, name: &str, modifiers: &[String], hold_ms: u64, source: &str) -> Result<()> {
        let code = key_code(name)?;
        let mut mods = 0;
        for m in modifiers { mods |= modifier_bit(m).ok_or_else(|| anyhow!("unknown modifier {m}"))?; }
        let _g = self.agent_lock.lock().await;
        self.check_agent(source)?;
        self.log(source, "key", json!({"key": name, "modifiers": modifiers}));
        self.tap(source, mods, code, hold_ms).await
    }

    pub async fn type_text(&self, text: &str, delay_ms: u64, source: &str) -> Result<()> {
        if text.chars().count() > 2000 { bail!("text too long (max 2000 characters)"); }
        let strokes: Vec<(u8, u8)> = text.chars().map(char_to_key).collect::<Result<_>>()?;
        let _g = self.agent_lock.lock().await;
        self.check_agent(source)?;
        let shown: String = text.chars().take(80).collect();
        self.log(source, "type", json!({"text": shown}));
        for (m, c) in strokes {
            self.tap(source, m, c, delay_ms).await?;
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        Ok(())
    }

    pub fn mouse(&self, dx: i32, dy: i32, wheel: i32, buttons: Option<u8>, source: &str) -> Result<()> {
        if source != "ui" { self.check_agent(source)?; }
        if !self.backend.devices().iter().any(|d| d == "mouse") {
            bail!("NS2Bench Bridge has no USB mouse in this mode");
        }
        let b = { let mut st = self.st.lock().unwrap(); if let Some(b) = buttons { st.mouse_buttons = b & 0x1F; } st.mouse_buttons };
        self.backend.send(Msg::Mouse([b as i32, dx.clamp(-4096, 4096), dy.clamp(-4096, 4096), wheel.clamp(-64, 64)]))
    }

    pub async fn click(&self, button: &str, hold_ms: u64, source: &str) -> Result<()> {
        let bit = match button { "left" => 1, "right" => 2, "middle" => 4, _ => bail!("button is left, right or middle") };
        let _g = self.agent_lock.lock().await;
        self.log(source, "click", json!({"button": button}));
        let cur = self.st.lock().unwrap().mouse_buttons;
        self.mouse(0, 0, 0, Some(cur | bit), source)?;
        tokio::time::sleep(Duration::from_millis(hold_ms.clamp(16, 2000))).await;
        let cur = self.st.lock().unwrap().mouse_buttons;
        self.mouse(0, 0, 0, Some(cur & !bit), source)
    }

    pub fn release_all(&self) {
        { let mut st = self.st.lock().unwrap(); st.ui = Layer::default(); st.agent = Layer::default(); st.ui_down.clear(); }
        let _ = self.flush();
    }

    pub fn clear_agent(&self) {
        self.st.lock().unwrap().agent = Layer::default();
        let _ = self.flush();
    }

    pub fn status(&self) -> Value {
        let b = self.backend.status.lock().unwrap().clone();
        let st = self.st.lock().unwrap();
        json!({
            "backend": b.backend, "connected": b.connected, "detail": b.detail, "rtt_ms": b.rtt_ms, "usb": b.usb,
            "usb_mode": b.usb_mode, "devices": b.devices, "writes": b.writes,
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

}
