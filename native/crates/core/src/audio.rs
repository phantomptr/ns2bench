//! Console audio from the capture card: 48 kHz stereo s16le, with levels, a browser stream
//! (/audio WebSocket) and a rolling buffer so saved clips include sound.
//! The audio device is paired with the video device by USB location (identical cards can share
//! a serial number). Override with "video": {"audio_device": "..."}; "audio": {"enabled": false}.
//! Capture: macOS through the helper; Linux/Windows through ffmpeg when "video.audio_device" is set
//! (an ALSA/PulseAudio or DirectShow name).

use crate::logs::LogHub;
use crate::server::S;
use anyhow::{bail, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::broadcast;

pub const RATE: usize = 48000;
pub const CHANNELS: usize = 2;
pub const CHUNK: usize = RATE * CHANNELS * 2 / 50; // 20 ms

#[derive(Default)]
struct St {
    device: Option<String>,
    level_db: Option<f64>,
    peak_db: Option<f64>,
    last_sound: Option<f64>,
    error: Option<String>,
}

pub struct AudioHub {
    pub enabled: bool,
    pub tx: broadcast::Sender<Bytes>,
    st: Mutex<St>,
    /// (unix time of the chunk's end, PCM) for the last `keep_s` seconds
    ring: Mutex<VecDeque<(f64, Bytes)>>,
    keep_s: f64,
    /// Bumped when the capture settings change: the running capture stops and starts again.
    generation: std::sync::atomic::AtomicU64,
}

impl AudioHub {
    pub fn new(cfg: Option<&Value>, keep_s: f64) -> Arc<AudioHub> {
        let (tx, _) = broadcast::channel(64);
        Arc::new(AudioHub {
            enabled: cfg.and_then(|c| c["enabled"].as_bool()).unwrap_or(true),
            tx, st: Mutex::default(), ring: Mutex::default(), keep_s, generation: 0.into(),
        })
    }

    pub fn status(&self) -> Value {
        let s = self.st.lock().unwrap();
        json!({"device": s.device, "level_db": s.level_db, "peak_db": s.peak_db,
               "last_sound_s": s.last_sound.map(|t| ((crate::logs::now() - t) * 10.0).round() / 10.0),
               "listeners": self.tx.receiver_count(), "error": s.error})
    }

    /// Captured chunks (end time, PCM) whose end falls in [from, to] (for clips).
    pub fn chunks_between(&self, from: f64, to: f64) -> Vec<(f64, Bytes)> {
        self.ring.lock().unwrap().iter().filter(|(t, _)| *t >= from && *t <= to).cloned().collect()
    }

    /// Follow a change of capture card or settings.
    pub fn restart(&self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn error(&self, e: impl Into<String>) {
        self.st.lock().unwrap().error = Some(e.into());
    }

    pub async fn run(self: Arc<Self>, video_cfg: Arc<Mutex<Value>>, logs: LogHub) {
        if !self.enabled {
            return self.error("disabled");
        }
        let mut delay = 2u64;
        let mut last = String::new();
        loop {
            let started = std::time::Instant::now();
            let r = self.clone().once(&video_cfg, &logs).await;
            if started.elapsed() > Duration::from_secs(30) || r.is_ok() { delay = 2; }
            if let Err(e) = r {
                let msg = e.to_string();
                // say it once per distinct problem, not on every retry
                if msg != last { logs.warn("bench", format!("audio: {msg}; retrying")); last = msg.clone(); }
                self.error(msg);
            }
            tokio::time::sleep(Duration::from_secs(delay)).await;
            delay = (delay * 2).min(30);
        }
    }

    async fn once(self: Arc<Self>, video_cfg: &Arc<Mutex<Value>>, logs: &LogHub) -> Result<()> {
        // Before the settings are read: a change made while starting up is then noticed below.
        let gen = self.generation.load(std::sync::atomic::Ordering::Relaxed);
        let vc = video_cfg.lock().unwrap().clone();
        let mut child = if cfg!(target_os = "macos") {
            let helper = crate::video::build_helper().await?;
            let device = match vc["audio_device"].as_str() {
                Some(d) => d.to_string(),
                None => pair(vc["device"].as_str().unwrap_or(""), &list(&helper).await?)
                    .ok_or_else(|| anyhow::anyhow!("no audio device paired with the video device"))?,
            };
            self.st.lock().unwrap().device = Some(device.clone());
            Command::new(helper).args(["audio", &device]).stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null()).kill_on_drop(true).spawn()?
        } else {
            // Windows: the card's sound device usually carries its name ("Game Capture 4K Pro").
            let found = if cfg!(windows) && vc["audio_device"].as_str().is_none() {
                let (_, audio) = crate::video::dshow_devices_names().await;
                pair_by_name(vc["device"].as_str().unwrap_or(""), &audio)
            } else {
                None
            };
            let Some(device) = vc["audio_device"].as_str().map(str::to_string).or(found) else {
                bail!("no sound device matched the capture card: pick it under Video → Sound")
            };
            let device = device.as_str();
            self.st.lock().unwrap().device = Some(device.to_string());
            let input: Vec<String> = if cfg!(windows) {
                vec!["-f".into(), "dshow".into(), "-i".into(), format!("audio={device}")]
            } else {
                vec!["-f".into(), "alsa".into(), "-i".into(), device.into()]
            };
            crate::video::quiet(crate::video::ffmpeg()).args(["-hide_banner", "-loglevel", "error", "-nostdin"]).args(&input)
                .args(["-ac", "2", "-ar", "48000", "-f", "s16le", "-"]).stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null()).kill_on_drop(true).spawn()?
        };
        logs.add("bench", format!("audio: {}", self.st.lock().unwrap().device.clone().unwrap_or_default()));
        self.st.lock().unwrap().error = None;
        let mut out = child.stdout.take().unwrap();
        let mut win: Vec<i16> = Vec::with_capacity(RATE * CHANNELS / 5);
        loop {
            if self.generation.load(std::sync::atomic::Ordering::Relaxed) != gen {
                return Ok(()); // settings changed: start again with the new card
            }
            let mut buf = vec![0u8; CHUNK];
            // A capture that goes silent without exiting is restarted too.
            match tokio::time::timeout(Duration::from_secs(5), out.read_exact(&mut buf)).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) => bail!("audio capture stopped"),
                Err(_) if self.generation.load(std::sync::atomic::Ordering::Relaxed) != gen => return Ok(()),
                Err(_) => bail!("audio capture stalled"),
            }
            let now = crate::logs::now();
            let data = Bytes::from(buf);
            let _ = self.tx.send(data.clone()); // a slow browser lags and skips; capture never stalls
            {
                let mut r = self.ring.lock().unwrap();
                r.push_back((now, data.clone()));
                while r.front().is_some_and(|(t, _)| now - t > self.keep_s) {
                    r.pop_front();
                }
            }
            win.extend(data.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes([c[0], c[1]])));
            if win.len() >= RATE * CHANNELS / 5 {
                // levels every 200 ms
                let peak = win.iter().map(|x| (*x as i32).abs()).max().unwrap_or(0);
                let rms = (win.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / win.len() as f64).sqrt();
                let db = |v: f64| if v > 0.0 { (20.0 * (v / 32768.0).log10() * 10.0).round() / 10.0 } else { -120.0 };
                let mut s = self.st.lock().unwrap();
                s.level_db = Some(db(rms));
                s.peak_db = Some(db(peak as f64));
                if peak > 300 {
                    s.last_sound = Some(now);
                }
                win.clear();
            }
        }
    }
}

pub struct Dev {
    pub kind: String,
    pub id: String,
    pub name: String,
}

async fn list(helper: &std::path::Path) -> Result<Vec<Dev>> {
    let out = Command::new(helper).arg("list").output().await?;
    Ok(String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| {
        let p: Vec<&str> = l.split('\t').collect();
        (p.len() >= 3).then(|| Dev { kind: p[0].into(), id: p[1].into(), name: p[2].into() })
    }).collect())
}

/// Windows: the sound device that belongs to a capture card, by name. The same name, or one that
/// contains the other ("Game Capture 4K Pro" / "Digital Audio Interface (Game Capture 4K Pro)"),
/// else the one sharing the most words with it (at least two).
pub fn pair_by_name(video: &str, audio: &[String]) -> Option<String> {
    let v = video.to_lowercase();
    if v.is_empty() {
        return None;
    }
    if let Some(a) = audio.iter().find(|a| a.to_lowercase() == v || a.to_lowercase().contains(&v) || v.contains(&a.to_lowercase())) {
        return Some(a.clone());
    }
    let words = |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 1).map(str::to_string).collect::<std::collections::HashSet<_>>();
    let vw = words(video);
    audio.iter().map(|a| (vw.intersection(&words(a)).count(), a)).filter(|(n, _)| *n >= 2).max_by_key(|(n, _)| *n).map(|(_, a)| a.clone())
}

/// Audio device for a video device: same USB location, else the one no other card owns.
pub fn pair(video_id: &str, devs: &[Dev]) -> Option<String> {
    let video = devs.iter().find(|d| d.kind == "video" && d.id == video_id)?;
    if !video_id.starts_with("0x") || video_id.len() < 10 {
        return None;
    }
    let name = match video.name.rsplit_once(' ') {
        Some((head, tail)) if tail.contains('x') => head.to_string(),
        _ => video.name.clone(),
    };
    let loc = video_id[2..10].to_lowercase();
    let same: Vec<&Dev> = devs.iter().filter(|d| d.kind == "audio" && d.name == name).collect();
    if let Some(d) = same.iter().find(|d| d.id.to_lowercase().ends_with(&format!(":{loc}:3"))) {
        return Some(d.id.clone());
    }
    let others: Vec<String> = devs.iter().filter(|d| d.kind == "video" && d.name.starts_with(&name) && d.id.len() >= 10)
        .map(|d| d.id[2..10].to_lowercase()).collect();
    let rest: Vec<&&Dev> = same.iter().filter(|d| !others.iter().any(|o| d.id.to_lowercase().ends_with(&format!(":{o}:3")))).collect();
    (rest.len() == 1).then(|| rest[0].id.clone())
}

/// Console audio for the browser: a JSON header, then binary 20 ms chunks of s16le PCM.
pub async fn ws_audio(State(b): State<S>, ws: WebSocketUpgrade) -> Response {
    // which console: decided now, while the request still names it
    match b.target() {
        Ok(c) => ws.on_upgrade(move |sock| audio_socket(c, sock)),
        Err(e) => axum::response::IntoResponse::into_response((axum::http::StatusCode::NOT_FOUND, e.to_string())),
    }
}

async fn audio_socket(c: std::sync::Arc<crate::console::Console>, sock: WebSocket) {
    let (mut tx, mut rx) = sock.split();
    let mut chunks = c.audio.tx.subscribe();
    let reader = tokio::spawn(async move { while let Some(Ok(_)) = rx.next().await {} });
    let hello = json!({"rate": RATE, "channels": CHANNELS, "format": "s16le"});
    if tx.send(Message::Text(hello.to_string().into())).await.is_ok() {
        loop {
            match chunks.recv().await {
                Ok(c) => {
                    if tx.send(Message::Binary(c)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    }
    reader.abort();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(kind: &str, id: &str, name: &str) -> Dev {
        Dev { kind: kind.into(), id: id.into(), name: name.into() }
    }
    #[test]
    fn pairs_by_name_on_windows() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pair_by_name("Game Capture 4K Pro", &a(&["Microphone (Realtek)", "Game Capture 4K Pro"])).as_deref(), Some("Game Capture 4K Pro"));
        assert_eq!(pair_by_name("Game Capture 4K Pro", &a(&["Digital Audio Interface (Game Capture 4K Pro)"])).as_deref(), Some("Digital Audio Interface (Game Capture 4K Pro)"));
        assert_eq!(pair_by_name("USB3.0 Capture", &a(&["Digital Audio Interface (USB3.0 Capture)", "Microphone"])).as_deref(), Some("Digital Audio Interface (USB3.0 Capture)"));
        assert_eq!(pair_by_name("Cam Link 4K", &a(&["Microphone (Realtek)"])), None);
    }

    #[test]
    fn pairs_by_usb_location() {
        let devs = vec![
            d("video", "0x112233002b895389", "UGREEN 15389 1920x1080"),
            d("video", "0x445566002b895389", "UGREEN 15389 1920x1080"),
            d("audio", "AppleUSBAudioEngine:MACROSILICON:UGREEN 15389:11223300:3", "UGREEN 15389"),
            d("audio", "AppleUSBAudioEngine:MACROSILICON:UGREEN 15389:44556600:3", "UGREEN 15389"),
        ];
        assert_eq!(pair("0x112233002b895389", &devs).unwrap(), "AppleUSBAudioEngine:MACROSILICON:UGREEN 15389:11223300:3");
        assert!(pair("nope", &devs).is_none());
    }
}
