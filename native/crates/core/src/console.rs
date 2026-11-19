//! Several consoles on one bench (docs/design/multi-console.md).
//!
//! A console has a profile (name, model, colour, network address) and its own capture card, with
//! its own picture, sound and clip recorder running all the time. The controller is shared: it is
//! plugged into one console at a time, and only the person knows which, so the bench keeps that
//! location and sends input only to that console.
//!
//! A request picks its console with `?console=<id or name>` or the `X-NS2Bench-Console` header;
//! without one it is the console the controller is on (so single-console calls work as before).

use crate::audio::AudioHub;
use crate::config::{state_dir, write_json};
use crate::dvr::Dvr;
use crate::logs::LogHub;
use crate::video::FrameHub;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;

/// Models and what they set: the menu profile and the controller mode that works on them.
pub const MODELS: &[(&str, &str, &str)] = &[
    // (id, name, controller mode)
    ("switch2", "Switch 2", "pro"),
    ("switch1", "Switch 1", "pro1"),
];

/// The board mode for a console's type and its controller choice: "pro" is that type's own Pro
/// Controller (Switch 1: pro1, Switch 2: pro), "keyboard" the wired pad + keyboard + mouse.
pub fn mode_for(model: &str, controller: &str) -> &'static str {
    match (model, controller) {
        (_, "keyboard") => "standard",
        ("switch1", _) => "pro1",
        _ => "pro",
    }
}

/// The controller choice a board mode means ("pro" or "keyboard").
pub fn controller_of(mode: &str) -> &'static str {
    if mode == "pro" || mode == "pro1" { "pro" } else { "keyboard" }
}

/// The video settings a console keeps (the same keys as video.json).
pub const VIDEO_KEYS: [&str; 8] = ["device", "width", "height", "fps", "quality", "backend", "codec", "audio_device"];

pub struct Console {
    pub id: String,
    pub name: Mutex<String>,
    pub model: Mutex<String>,
    pub color: Mutex<String>,
    pub network_ip: Mutex<Option<String>>,
    /// It uses the bench's shared controller (the board): picking it can move the controller here.
    pub uses_controller: std::sync::atomic::AtomicBool,
    /// "pro" (its type's Pro Controller) or "keyboard" (wired pad + keyboard + mouse).
    pub controller: Mutex<String>,
    /// The menu routes and screen names for this console's model.
    pub profile: Mutex<Value>,
    pub frames: Arc<FrameHub>,
    pub video_cfg: Arc<Mutex<Value>>,
    video_task: Mutex<Option<JoinHandle<()>>>,
    /// Its recorder, sound, picture watch and screen log: all stopped with the console.
    pub tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Removed: nothing may start for it again.
    pub stopped: std::sync::atomic::AtomicBool,
    pub audio: Arc<AudioHub>,
    pub dvr: Arc<Dvr>,
}

impl Console {
    /// `entry`: one console from consoles.json. `base`: the bench's own video settings (each
    /// console starts from them; its own entries win).
    pub fn new(entry: &Value, base_video: &Value, raw: &Value) -> Arc<Console> {
        let model = entry["model"].as_str().unwrap_or("switch2").to_string();
        let mut video = base_video.clone();
        if let (Some(v), Some(own)) = (video.as_object_mut(), entry["video"].as_object()) {
            for (k, x) in own { v.insert(k.clone(), x.clone()); }
        }
        let dvr = Dvr::new(raw.get("dvr"));
        Arc::new(Console {
            id: entry["id"].as_str().unwrap_or("console").to_string(),
            name: Mutex::new(entry["name"].as_str().unwrap_or("Console").to_string()),
            profile: Mutex::new(crate::nav::load_profile(&model)),
            model: Mutex::new(model),
            color: Mutex::new(entry["color"].as_str().unwrap_or("#e60012").to_string()),
            network_ip: Mutex::new(entry["network_ip"].as_str().map(str::to_string)),
            uses_controller: entry["uses_controller"].as_bool().unwrap_or(true).into(),
            controller: Mutex::new(entry["controller"].as_str().filter(|c| *c == "keyboard").unwrap_or("pro").to_string()),
            frames: FrameHub::new(),
            video_cfg: Arc::new(Mutex::new(video)),
            video_task: Mutex::new(None),
            tasks: Mutex::new(vec![]),
            stopped: false.into(),
            audio: AudioHub::new(raw.get("audio"), dvr.seconds + 2.0),
            dvr,
        })
    }

    pub fn name(&self) -> String {
        self.name.lock().unwrap().clone()
    }

    /// What consoles.json keeps for this console.
    pub fn entry(&self) -> Value {
        let v = self.video_cfg.lock().unwrap().clone();
        let mut video: serde_json::Map<String, Value> = VIDEO_KEYS.iter().filter_map(|k| v.get(*k).filter(|x| !x.is_null()).map(|x| (k.to_string(), x.clone()))).collect();
        // ("no card" is written down: left out, the console would take the bench's first card
        // at the next start, and the two would fight over it)
        if v["device"].is_null() { video.insert("device".into(), Value::Null); }
        json!({"id": self.id, "name": self.name(), "model": *self.model.lock().unwrap(), "color": *self.color.lock().unwrap(),
            "network_ip": *self.network_ip.lock().unwrap(), "video": video,
            "uses_controller": self.uses_controller.load(std::sync::atomic::Ordering::Relaxed),
            "controller": *self.controller.lock().unwrap()})
    }

    /// For status and the page: the entry plus the live picture's state.
    pub fn info(&self, controller_here: bool) -> Value {
        let mut e = self.entry();
        e["controller_here"] = json!(controller_here);
        e["video_status"] = self.frames.status();
        // what its network traffic says it is (crate::netseen), to check the type against
        e["network_model"] = json!(self.network_ip.lock().unwrap().as_deref().and_then(crate::netseen::model_at));
        e["model_name"] = json!(MODELS.iter().find(|m| Some(m.0) == e["model"].as_str()).map(|m| m.1).unwrap_or("?"));
        e
    }

    /// Start (or restart) this console's capture. `save` keeps a card picked automatically.
    pub fn restart_video(self: &Arc<Self>, logs: &LogHub, save: Arc<dyn Fn() + Send + Sync>, taken: crate::video::Taken) {
        {
            // one lock across stop and start: two restarts at once must not leave two captures
            let mut task = self.video_task.lock().unwrap();
            // removed meanwhile (a settings change racing the removal): start nothing
            if self.stopped.load(std::sync::atomic::Ordering::SeqCst) { return; }
            let restarting = task.take().map(|t| t.abort()).is_some();
            let (cfg, frames, logs) = (self.video_cfg.clone(), self.frames.clone(), logs.clone());
            *task = Some(tokio::spawn(async move {
                // the old ffmpeg is being killed, not yet gone: give it a moment to let go of the
                // card (Windows refuses a second open with "I/O error" meanwhile)
                if restarting { tokio::time::sleep(Duration::from_millis(500)).await; }
                crate::video::supervise(cfg, frames, logs, Some(save), Some(taken)).await
            }));
        }
        self.audio.restart();
    }

    /// Stop everything this console runs (it is being removed).
    pub fn stop(&self) {
        let mut video = self.video_task.lock().unwrap();
        self.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(t) = video.take() { t.abort(); }
        // (dropping the sound task's ffmpeg kills it, so the card is free again)
        for t in self.tasks.lock().unwrap().drain(..) { t.abort(); }
    }
}

/// A short id from a name: "Switch 1 OLED" -> "switch-1-oled".
pub fn slug(name: &str) -> String {
    let s: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "console".into() } else { s.chars().take(32).collect() }
}

pub fn file() -> std::path::PathBuf {
    state_dir().join("consoles.json")
}

/// consoles.json, or the single console this bench had before (its video settings, the
/// configured menu profile), holding the controller.
pub fn load(raw: &Value) -> (Vec<Value>, Option<String>) {
    let bytes = std::fs::read(file()).ok();
    let parsed = bytes.as_ref().and_then(|d| serde_json::from_slice::<Value>(d).ok());
    if bytes.is_some() && parsed.is_none() {
        // unreadable: keep it (its consoles would be lost at the next save) and start from one
        let keep = file().with_extension(format!("json.broken-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        let _ = std::fs::rename(file(), keep);
    }
    if let Some(v) = parsed {
        let list: Vec<Value> = v["consoles"].as_array().cloned().unwrap_or_default().into_iter()
            .filter(|c| c["id"].as_str().is_some_and(|s| !s.is_empty())).collect();
        if !list.is_empty() {
            FOLLOW_VIEW.store(v["controller_follows_view"].as_bool().unwrap_or(true), std::sync::atomic::Ordering::Relaxed);
            set_active(v["active"].as_str().or(v["controller_on"].as_str()).map(str::to_string));
            return (list, v["controller_on"].as_str().map(str::to_string));
        }
    }
    let model = raw["profile"].as_str().unwrap_or("switch2");
    let name = MODELS.iter().find(|m| m.0 == model).map(|m| m.1).unwrap_or("Switch");
    let id = slug(name);
    (vec![json!({"id": id, "name": name, "model": model, "color": "#e60012"})], Some(id))
}

/// Switching to a console (on the page) moves the shared controller there too.
pub static FOLLOW_VIEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The console the person picked last (the one on screen). With the controller following the
/// console on screen, that's where the controller is: after the board restarts, at start, always.
static ACTIVE: Mutex<Option<String>> = Mutex::new(None);
#[cfg(test)]
thread_local! { static TEST_ACTIVE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) }; }
/// The console last picked as the controller's. (In unit tests each test has its own: they run
/// side by side, and one test's pick showed up in another's bench.)
pub fn active() -> Option<String> {
    #[cfg(test)]
    { TEST_ACTIVE.with(|a| a.borrow().clone()) }
    #[cfg(not(test))]
    { ACTIVE.lock().unwrap().clone() }
}
pub fn set_active(id: Option<String>) {
    #[cfg(test)]
    { let _ = &ACTIVE; TEST_ACTIVE.with(|a| *a.borrow_mut() = id) }
    #[cfg(not(test))]
    { *ACTIVE.lock().unwrap() = id; }
}

pub fn save(consoles: &[Arc<Console>], controller_on: Option<&str>) -> std::io::Result<()> {
    // unit tests never write the person's consoles.json
    if cfg!(test) { return Ok(()); }
    write_json(&file(), &json!({"consoles": consoles.iter().map(|c| c.entry()).collect::<Vec<_>>(), "controller_on": controller_on,
        "controller_follows_view": FOLLOW_VIEW.load(std::sync::atomic::Ordering::Relaxed),
        "active": active()}))
}

tokio::task_local! {
    /// The console a request names (`?console=` or the X-NS2Bench-Console header), if any.
    pub static SELECTED: Option<String>;
}

/// The console a request is about, as named by it (None: it named none).
pub fn selected() -> Option<String> {
    SELECTED.try_with(|s| s.clone()).ok().flatten()
}

/// The console a request names: `?console=` or the X-NS2Bench-Console header.
pub fn requested(req: &axum::extract::Request) -> Option<String> {
    let from_query = req.uri().query().and_then(|q| {
        q.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == "console")
            .map(|(_, v)| percent_decode(v))
    });
    // (percent-encoded by clients, so any name fits in a header)
    let from_header = req.headers().get("x-ns2bench-console").and_then(|v| v.to_str().ok()).map(percent_decode);
    from_query.or(from_header).filter(|s| !s.trim().is_empty())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'+', _, _) => { out.push(b' '); i += 1; }
            (b'%', Some(h), Some(l)) => { out.push((h * 16 + l) as u8); i += 3; }
            (c, _, _) => { out.push(c); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slugs_and_names() {
        assert_eq!(slug("Switch 1 OLED"), "switch-1-oled");
        assert_eq!(slug("  ??  "), "console");
        assert_eq!(percent_decode("Switch%201"), "Switch 1");
        assert_eq!(percent_decode("a+b%2"), "a b%2");
        // what client.rs sends for any name comes back as the same name
        let name = "Switch 1 é\nX";
        let enc: String = name.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        assert!(!enc.contains(['\n', ' ']));
        assert_eq!(percent_decode(&enc), name);
        assert_eq!((mode_for("switch1", "pro"), mode_for("switch2", "pro"), mode_for("switch1", "keyboard")), ("pro1", "pro", "standard"));
        assert_eq!((controller_of("pro1"), controller_of("pro"), controller_of("hori")), ("pro", "pro", "keyboard"));
    }
}
