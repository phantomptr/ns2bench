//! Rolling recorder: always keeps the last N seconds of video (and console audio) so the
//! moments *before* a problem can be saved after it happens.
//! Config "dvr": {"seconds": 30, "fps": 20, "max_mb": 100}. Frames are the live-view JPEGs (no
//! extra encoding while recording). Saving encodes an MP4 (H.264 + AAC) with ffmpeg; without
//! ffmpeg the frames go into a zip of JPEGs (plus audio.wav).

use crate::audio::{AudioHub, CHANNELS, RATE};
use crate::config::state_dir;
use crate::video::FrameHub;
use anyhow::{bail, Result};
use bytes::Bytes;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

pub fn clips_dir() -> PathBuf {
    state_dir().join("clips")
}

pub struct Dvr {
    pub seconds: f64,
    pub fps: f64,
    max_bytes: f64,
    buf: Mutex<(VecDeque<(f64, Bytes)>, usize)>,
}

impl Dvr {
    pub fn new(cfg: Option<&Value>) -> Arc<Dvr> {
        let g = |k: &str, d: f64| cfg.and_then(|c| c[k].as_f64()).unwrap_or(d);
        Arc::new(Dvr { seconds: g("seconds", 30.0), fps: g("fps", 20.0), max_bytes: g("max_mb", 100.0) * 1e6, buf: Mutex::default() })
    }

    pub async fn run(self: Arc<Self>, frames: Arc<FrameHub>) {
        let mut rx = frames.latest.subscribe();
        let interval = 1.0 / self.fps;
        let mut next_at = 0.0;
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            let Some(f) = rx.borrow_and_update().clone() else { continue };
            let now = crate::logs::now();
            if now < next_at {
                continue;
            }
            next_at = now + interval * 0.9;
            let mut b = self.buf.lock().unwrap();
            b.1 += f.jpeg.len();
            b.0.push_back((now, f.jpeg));
            while b.0.front().is_some_and(|(t, _)| now - t > self.seconds) || b.1 as f64 > self.max_bytes {
                let Some((_, j)) = b.0.pop_front() else { break };
                b.1 -= j.len();
            }
        }
    }

    pub fn status(&self) -> Value {
        let b = self.buf.lock().unwrap();
        let span = match (b.0.front(), b.0.back()) {
            (Some(a), Some(z)) => z.0 - a.0,
            _ => 0.0,
        };
        json!({"seconds_kept": (span * 10.0).round() / 10.0, "frames": b.0.len(), "mb": (b.1 as f64 / 1e5).round() / 10.0,
               "fps": self.fps, "limit_s": self.seconds, "limit_mb": (self.max_bytes / 1e6).round()})
    }

    /// Write the last `seconds` (default: everything kept) to an MP4 (or zip). Returns (path, span).
    pub async fn save(&self, seconds: Option<f64>, path: Option<PathBuf>, label: &str, audio: Option<&AudioHub>) -> Result<(PathBuf, f64)> {
        let mut frames: Vec<(f64, Bytes)> = self.buf.lock().unwrap().0.iter().cloned().collect();
        if let Some(s) = seconds.filter(|s| *s > 0.0) {
            let cutoff = crate::logs::now() - s;
            frames.retain(|f| f.0 >= cutoff);
        }
        if frames.len() < 2 {
            bail!("nothing recorded yet");
        }
        std::fs::create_dir_all(clips_dir())?;
        let stem = format!("{}-{label}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let path = path.unwrap_or_else(|| clips_dir().join(format!("{stem}.mp4")));
        let (t0, t1) = (frames[0].0, frames[frames.len() - 1].0);
        let span = t1 - t0;
        let fps = if span > 0.0 { ((frames.len() - 1) as f64 / span).max(1.0) } else { self.fps };
        let pcm = audio.map(|a| place_pcm(a, t0, t1 + 1.0 / fps)).filter(|p| p.iter().any(|b| *b != 0));
        let span_r = (span * 10.0).round() / 10.0;

        let ff = crate::video::ffmpeg();
        if ff.is_file() || crate::ocr::which("ffmpeg") {
            let wav = pcm.as_ref().map(|p| -> Result<PathBuf> {
                let w = path.with_extension("tmp.wav");
                std::fs::write(&w, wav_bytes(p))?;
                Ok(w)
            }).transpose()?;
            let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-f", "image2pipe", "-framerate"]
                .iter().map(|s| s.to_string()).collect();
            args.extend([format!("{fps:.3}"), "-i".into(), "-".into()]);
            if let Some(w) = &wav {
                args.extend(["-i".into(), w.to_string_lossy().into(), "-c:a".into(), "aac".into(), "-b:a".into(), "160k".into(), "-shortest".into()]);
            }
            args.extend(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                "-movflags", "+faststart"].iter().map(|s| s.to_string()));
            args.push(path.to_string_lossy().into());
            let mut child = tokio::process::Command::new(crate::video::ffmpeg()).args(&args).stdin(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped()).kill_on_drop(true).spawn()
                .map_err(|_| anyhow::anyhow!("saving clips needs ffmpeg: {}", crate::video::ffmpeg_hint()))?;
            let mut stdin = child.stdin.take().unwrap();
            let fr = frames.clone();
            let feed = tokio::spawn(async move {
                for (_, j) in fr {
                    if stdin.write_all(&j).await.is_err() {
                        break;
                    }
                }
            });
            let out = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output()).await;
            let _ = feed.await;
            if let Some(w) = &wav {
                let _ = std::fs::remove_file(w);
            }
            if matches!(&out, Ok(Ok(o)) if o.status.success()) {
                return Ok((path, span_r));
            }
        }
        let zpath = path.with_extension("zip");
        let zp = zpath.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zp)?);
            let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (i, (t, j)) in frames.iter().enumerate() {
                z.start_file(format!("{i:05}-{t:.3}.jpg"), opt)?;
                z.write_all(j)?;
            }
            if let Some(p) = pcm {
                z.start_file("audio.wav", opt)?;
                z.write_all(&wav_bytes(&p))?;
            }
            z.finish()?;
            Ok(())
        }).await??;
        Ok((zpath, span_r))
    }
}

/// PCM covering [from, to], each captured chunk at its own time, silence where nothing was captured.
fn place_pcm(a: &AudioHub, from: f64, to: f64) -> Vec<u8> {
    let bps = (RATE * CHANNELS * 2) as f64;
    let frame = CHANNELS * 2;
    let total = (((to - from) * bps) as usize) / frame * frame;
    let mut out = vec![0u8; total];
    let chunk_s = crate::audio::CHUNK as f64 / bps;
    for (end, d) in a.chunks_between(from - chunk_s, to + chunk_s) {
        let start = end - d.len() as f64 / bps;
        let off = ((start - from) * bps) as i64 / frame as i64 * frame as i64;
        for (i, b) in d.iter().enumerate() {
            let p = off + i as i64;
            if p >= 0 && (p as usize) < total {
                out[p as usize] = *b;
            }
        }
    }
    out
}

fn wav_bytes(pcm: &[u8]) -> Vec<u8> {
    let mut h = Vec::with_capacity(44 + pcm.len());
    let byte_rate = (RATE * CHANNELS * 2) as u32;
    h.extend(b"RIFF");
    h.extend((36 + pcm.len() as u32).to_le_bytes());
    h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend((CHANNELS as u16).to_le_bytes());
    h.extend((RATE as u32).to_le_bytes());
    h.extend(byte_rate.to_le_bytes());
    h.extend(((CHANNELS * 2) as u16).to_le_bytes());
    h.extend(16u16.to_le_bytes());
    h.extend(b"data");
    h.extend((pcm.len() as u32).to_le_bytes());
    h.extend_from_slice(pcm);
    h
}

pub fn list_clips() -> Value {
    let mut v: Vec<(String, u64)> = std::fs::read_dir(clips_dir()).map(|d| d.filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.metadata().map(|m| m.len()).unwrap_or(0))).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    Value::Array(v.into_iter().take(50).map(|(n, s)| json!({"name": n, "url": format!("/clips/{n}"), "mb": (s as f64 / 1e5).round() / 10.0})).collect())
}

pub fn clip_file(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().count() != 1 || name.starts_with('.') {
        return None;
    }
    let f = clips_dir().join(p);
    f.is_file().then_some(f)
}

#[cfg(test)]
mod tests {
    #[test]
    fn wav_header() {
        let w = super::wav_bytes(&[0u8; 8]);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 52);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
    }
}
