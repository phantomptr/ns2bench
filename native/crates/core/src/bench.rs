//! The bench: config + video + input + logs + everything built on them.

use crate::config::{state_dir, write_json, Config};
use crate::inputs::{start_backend, InputHub};
use crate::logs::LogHub;
use crate::video::FrameHub;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::JoinHandle;

pub struct Bench {
    pub cfg: Config,
    pub logs: LogHub,
    pub frames: Arc<FrameHub>,
    pub hub: Arc<InputHub>,
    pub input_cfg: Mutex<Value>,
    pub settings_lock: tokio::sync::Mutex<()>,
    pub video_cfg: Arc<Mutex<Value>>,
    video_task: Mutex<Option<JoinHandle<()>>>,
    pub screen_log: AtomicBool,
    pub active_operations: std::sync::atomic::AtomicUsize,
    pub calibration: Mutex<Value>,
    pub ext: crate::features::Ext,
    started: Instant,
}

impl Bench {
    pub fn new(cfg: Config) -> Arc<Bench> {
        let logs = LogHub::new(cfg.raw.get("logs"));
        // Daily log files in ~/.ns2bench/logs (not from unit tests).
        if !cfg!(test) && cfg.raw["log_files"].as_bool() != Some(false) {
            logs.persist(state_dir().join("logs"));
        }
        let (conn_tx, mut conn_rx) = tokio::sync::mpsc::unbounded_channel();
        let backend = start_backend(&cfg.input, logs.clone(), conn_tx);
        let hub = InputHub::new(backend, logs.clone());
        let calibration: Value = std::fs::read_to_string(state_dir().join("calibration.json")).ok()
            .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
        hub.min_press_ms.store(crate::calib::clamp_min_press(calibration["min_press_ms"].as_u64().unwrap_or(crate::calib::DEFAULT_MIN_PRESS_MS)), std::sync::atomic::Ordering::Relaxed);
        let screen_log = cfg.raw["ocr"]["screen_log"].as_bool().unwrap_or(false);
        let bench = Arc::new(Bench {
            input_cfg: Mutex::new(cfg.input.clone()), settings_lock: tokio::sync::Mutex::new(()),
            video_cfg: Arc::new(Mutex::new(cfg.video.clone())),
            ext: crate::features::Ext::new(&cfg),
            cfg, logs, frames: FrameHub::new(), hub, video_task: Mutex::new(None),
            active_operations: 0.into(), screen_log: screen_log.into(), calibration: Mutex::new(calibration), started: Instant::now(),
        });
        let h = bench.hub.clone();
        tokio::spawn(async move { while conn_rx.recv().await.is_some() { h.resend(); } });
        bench
    }

    pub fn start(self: &Arc<Self>) {
        self.logs.add("bench", format!("NS2Bench {} started (native): video={} input={}", env!("CARGO_PKG_VERSION"),
            crate::video::backend_name(&self.video_cfg.lock().unwrap()), self.cfg.input["backend"].as_str().unwrap_or("none")));
        self.restart_video();
        tokio::spawn(self.logs.clone().tail_files());
        crate::features::start(self.clone());
    }

    pub fn restart_video(self: &Arc<Self>) {
        {
            // One lock across stop and start: two restarts at once must not leave two captures.
            let mut task = self.video_task.lock().unwrap();
            if let Some(t) = task.take() { t.abort(); }
            *task = Some(tokio::spawn(crate::video::supervise(self.video_cfg.clone(), self.frames.clone(), self.logs.clone())));
        }
        self.ext.audio.restart();
    }

    pub async fn set_video(self: &Arc<Self>, changes: Value) -> Result<()> {
        let Some(obj) = changes.as_object() else { bail!("JSON object required") };
        let allowed = ["device", "width", "height", "fps", "quality", "backend", "codec", "audio_device"];
        if let Some(k) = obj.keys().find(|k| !allowed.contains(&k.as_str())) { bail!("unknown video setting: {k}"); }
        if obj.get("fps").and_then(Value::as_u64).is_some_and(|f| !(1..=120).contains(&f)) { bail!("fps must be 1-120"); }
        {
            let mut v = self.video_cfg.lock().unwrap();
            for (k, x) in obj { v[k] = x.clone(); }
            let saved: serde_json::Map<String, Value> = allowed.iter().filter_map(|k| v.get(*k).map(|x| (k.to_string(), x.clone()))).collect();
            let _ = write_json(&state_dir().join("video.json"), &Value::Object(saved));
        }
        // new settings: ask the card for them again (it may accept these)
        crate::video::USE_DEVICE_DEFAULT.store(false, std::sync::atomic::Ordering::Relaxed);
        self.logs.add("bench", format!("video settings changed: {changes}"));
        self.restart_video();
        Ok(())
    }

    pub fn status(&self) -> Value {
        let mut s = json!({
            "video": self.frames.status(),
            "input": self.hub.status(),
            "calibration": *self.calibration.lock().unwrap(),
            "uptime_s": self.started.elapsed().as_secs(),
            "active_operations": self.active_operations.load(std::sync::atomic::Ordering::Relaxed),
            "engine": "native",
            "version": env!("CARGO_PKG_VERSION"),
            "safe_mode": crate::config::safe_mode(),
            "last_run_unclean": crate::report::unclean_previous().is_some(),
        });
        crate::features::status(self, &mut s);
        s
    }
}
