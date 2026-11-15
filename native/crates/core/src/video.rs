//! Video: a shared latest-frame hub and capture sources.
//!   macOS   the native AVFoundation helper (embedded Swift source, built on first use): shares the
//!           card with OBS, applies the chosen format, hardware JPEG and H.264 (VideoToolbox)
//!   Linux   ffmpeg v4l2 (MJPEG passthrough)        Windows  ffmpeg dshow
//! Every source feeds JPEG frames (screenshots, OCR, MJPEG view, recorder). H.264 for the
//! hardware-decoded live view is produced alongside where the platform supports it.

use crate::logs::LogHub;
use anyhow::{anyhow, bail, Result};
use bytes::Bytes;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{broadcast, watch};

pub const HELPER_SWIFT: &str = include_str!("../../../../capture/ns2capture.swift");

#[derive(Clone)]
pub struct Frame {
    pub seq: u64,
    pub at: Instant,
    pub jpeg: Bytes,
}

pub struct FrameHub {
    pub latest: watch::Sender<Option<Frame>>,
    pub h264: broadcast::Sender<Bytes>,
    pub h264_config: Mutex<Option<Bytes>>, // last SPS/PPS + IDR access unit, for late joiners
    times: Mutex<VecDeque<Instant>>,
    pub error: Mutex<Option<String>>,
    pub source: Mutex<Option<String>>,
    seq: Mutex<u64>,
}

impl FrameHub {
    pub fn new() -> Arc<FrameHub> {
        let (latest, _) = watch::channel(None);
        let (h264, _) = broadcast::channel(256);
        Arc::new(FrameHub {
            latest, h264, h264_config: Mutex::new(None), times: Mutex::new(VecDeque::new()),
            error: Mutex::new(None), source: Mutex::new(None), seq: Mutex::new(0),
        })
    }

    pub fn push(&self, jpeg: Bytes) {
        let now = Instant::now();
        {
            let mut t = self.times.lock().unwrap();
            t.push_back(now);
            while t.front().is_some_and(|x| now.duration_since(*x) > Duration::from_secs(2)) {
                t.pop_front();
            }
        }
        let seq = { let mut s = self.seq.lock().unwrap(); *s += 1; *s };
        *self.error.lock().unwrap() = None;
        // send_replace: plain send() drops the value when nobody is subscribed yet
        self.latest.send_replace(Some(Frame { seq, at: now, jpeg }));
    }

    pub fn latest(&self) -> Option<Frame> {
        self.latest.borrow().clone()
    }

    pub fn status(&self) -> Value {
        let fps = self.times.lock().unwrap().len() as f64 / 2.0;
        let f = self.latest();
        json!({
            "source": *self.source.lock().unwrap(), "fps": fps, "frames": f.as_ref().map_or(0, |f| f.seq),
            "age_s": f.as_ref().map(|f| (f.at.elapsed().as_secs_f64() * 100.0).round() / 100.0),
            "error": *self.error.lock().unwrap(),
            "h264_viewers": self.h264.receiver_count(),
        })
    }
}

// ------------------------------------------------------------------ ffmpeg

/// Where ffmpeg is: $NS2BENCH_FFMPEG, next to the app, common install places
/// (apps started from Finder or the Start menu don't get a shell's PATH), then PATH.
pub fn ffmpeg() -> PathBuf {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = std::env::var_os("NS2BENCH_FFMPEG").map(PathBuf::from).filter(|p| p.is_file()) {
        return p;
    }
    let mut places: Vec<PathBuf> = vec![];
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        places.push(dir.join(exe));
    }
    if cfg!(target_os = "macos") {
        places.extend(["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"].map(PathBuf::from));
    } else if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            places.push(PathBuf::from(local).join("Microsoft/WinGet/Links/ffmpeg.exe"));
        }
        places.push(PathBuf::from("C:/ProgramData/chocolatey/bin/ffmpeg.exe"));
        if let Some(home) = std::env::var_os("USERPROFILE") {
            places.push(PathBuf::from(home).join("scoop/shims/ffmpeg.exe"));
        }
    } else {
        places.extend(["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/snap/bin/ffmpeg"].map(PathBuf::from));
    }
    places.into_iter().find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(exe))
}

/// One line telling the user how to get ffmpeg on this system.
pub fn ffmpeg_hint() -> &'static str {
    if cfg!(windows) {
        "install it with `winget install Gyan.FFmpeg` (then restart NS2Bench), or put ffmpeg.exe next to NS2Bench"
    } else if cfg!(target_os = "macos") {
        "install it with `brew install ffmpeg`"
    } else {
        "install it with your package manager, e.g. `sudo apt install ffmpeg`"
    }
}

// ------------------------------------------------------------------ macOS helper

pub fn helper_path() -> PathBuf {
    crate::config::state_dir().join("bin").join(if cfg!(windows) { "ns2capture.exe" } else { "ns2capture" })
}

/// The prebuilt helper shipped inside the desktop app (NS2Bench.app/Contents/MacOS),
/// so installed copies need no Xcode tools.
pub fn bundled_helper() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("ns2capture");
    p.is_file().then_some(p)
}

/// Use the bundled helper, else build it from the embedded Swift source if it is
/// missing or out of date (source checkouts).
pub async fn build_helper() -> Result<PathBuf> {
    if let Some(p) = bundled_helper() {
        return Ok(p);
    }
    // Video, sound and text reading all ask at startup: build once, not three times at once.
    static BUILD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = BUILD.lock().await;
    let dir = crate::config::state_dir().join("bin");
    std::fs::create_dir_all(&dir)?;
    let src = dir.join("ns2capture.swift");
    let bin = helper_path();
    let current = std::fs::read_to_string(&src).ok();
    if current.as_deref() != Some(HELPER_SWIFT) || !bin.exists() {
        std::fs::write(&src, HELPER_SWIFT)?;
        let out = Command::new("swiftc").args(["-O", src.to_str().unwrap(), "-o", bin.to_str().unwrap()]).output().await
            .map_err(|e| anyhow!("swiftc not found (install Xcode command line tools): {e}"))?;
        if !out.status.success() {
            bail!("building the capture helper failed: {}", String::from_utf8_lossy(&out.stderr).chars().rev().take(400).collect::<String>().chars().rev().collect::<String>());
        }
    }
    Ok(bin)
}

pub fn has_helper() -> bool {
    cfg!(target_os = "macos")
}

// ------------------------------------------------------------------ sources

async fn capture_read<R: AsyncRead + Unpin>(source: &mut R, buf: &mut [u8], limit: Duration) -> Result<usize> {
    tokio::time::timeout(limit, source.read(buf)).await
        .map_err(|_| anyhow!("no video data for {}s; check capture permission, device and HDMI signal", limit.as_secs()))?
        .map_err(Into::into)
}

async fn capture_exact<R: AsyncRead + Unpin>(source: &mut R, mut buf: &mut [u8]) -> Result<()> {
    while !buf.is_empty() {
        let n = capture_read(source, buf, Duration::from_secs(10)).await?;
        if n == 0 { bail!("capture process stopped; see capture logs for details"); }
        buf = &mut buf[n..];
    }
    Ok(())
}

pub fn backend_name(cfg: &Value) -> &'static str {
    match cfg["backend"].as_str().unwrap_or("auto") {
        "none" => "none",
        "avf" => "avf",
        "ffmpeg" => "ffmpeg",
        _ if has_helper() && cfg["ffmpeg_input"].is_null() => "avf",
        _ => "ffmpeg",
    }
}

fn split_jpegs(buf: &mut Vec<u8>) -> Vec<Bytes> {
    let mut out = vec![];
    loop {
        let Some(start) = buf.windows(2).position(|w| w == [0xFF, 0xD8]) else { buf.clear(); return out };
        let Some(end) = buf[start + 2..].windows(2).position(|w| w == [0xFF, 0xD9]) else {
            buf.drain(..start);
            return out;
        };
        let end = start + 2 + end + 2;
        out.push(Bytes::copy_from_slice(&buf[start..end]));
        buf.drain(..end);
    }
}

pub fn ffmpeg_input_args(cfg: &Value) -> Vec<String> {
    if let Some(a) = cfg["ffmpeg_input"].as_array() {
        return a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    }
    let size = format!("{}x{}", cfg["width"].as_u64().unwrap_or(1920), cfg["height"].as_u64().unwrap_or(1080));
    let fps = cfg["fps"].as_u64().unwrap_or(30).to_string();
    let dev = cfg["device"].as_str().map(str::to_string);
    if cfg!(target_os = "macos") {
        vec!["-f".into(), "avfoundation".into(), "-framerate".into(), fps, "-video_size".into(), size, "-i".into(), format!("{}:none", dev.unwrap_or("0".into()))]
    } else if cfg!(windows) {
        vec!["-f".into(), "dshow".into(), "-rtbufsize".into(), "256M".into(), "-video_size".into(), size, "-framerate".into(), fps, "-i".into(), format!("video={}", dev.unwrap_or_default())]
    } else {
        vec!["-f".into(), "v4l2".into(), "-input_format".into(), "mjpeg".into(), "-video_size".into(), size, "-framerate".into(), fps, "-i".into(), dev.unwrap_or("/dev/video0".into())]
    }
}

async fn run_avf(cfg: &Value, hub: &FrameHub, logs: &LogHub) -> Result<()> {
    let helper = build_helper().await?;
    let mut args = vec![
        "stream2".to_string(), cfg["device"].as_str().unwrap_or("").to_string(),
        cfg["fps"].as_u64().unwrap_or(30).to_string(), cfg["quality"].as_u64().unwrap_or(80).to_string(),
    ];
    if let (Some(w), Some(h)) = (cfg["width"].as_u64(), cfg["height"].as_u64()) {
        args.push(w.to_string());
        args.push(h.to_string());
    }
    let mut child = Command::new(&helper).args(&args).stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).kill_on_drop(true).spawn()?;
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let l2 = logs.clone();
    tokio::spawn(async move {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s).await;
        for line in s.lines() { l2.add("bench", line); }
    });
    // stream2: [u8 kind][u32 len][data]; 1 = JPEG, 2 = H.264 Annex B access unit
    loop {
        let mut hdr = [0u8; 5];
        capture_exact(&mut out, &mut hdr).await?;
        let n = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
        if n > 64 * 1024 * 1024 { bail!("invalid capture packet length: {n}"); }
        let mut buf = vec![0u8; n];
        capture_exact(&mut out, &mut buf).await?;
        match hdr[0] {
            1 => hub.push(Bytes::from(buf)),
            2 => crate::h264::push(hub, Bytes::from(buf)),
            _ => {}
        }
    }
}

/// Split an Annex B stream (with AUD NALs from h264_metadata=aud=insert) into access units.
fn split_aus(buf: &mut Vec<u8>) -> Vec<Bytes> {
    let aud = |b: &[u8], i: usize| b.len() > i + 4 && b[i] == 0 && b[i + 1] == 0 && b[i + 2] == 0 && b[i + 3] == 1 && b[i + 4] & 0x1F == 9;
    let starts: Vec<usize> = (0..buf.len().saturating_sub(4)).filter(|&i| aud(buf, i)).collect();
    let mut out = vec![];
    for w in starts.windows(2) {
        out.push(Bytes::copy_from_slice(&buf[w[0]..w[1]]));
    }
    if let Some(&last) = starts.last() {
        buf.drain(..last);
    }
    out
}

/// One ffmpeg reads the device once (V4L2/DirectShow devices usually allow a single reader) and
/// writes MJPEG to stdout for the live view, screenshots and OCR, plus, when "h264_encoder" is set
/// (libx264, h264_vaapi, h264_nvenc, h264_qsv, h264_mf, h264_videotoolbox ...), H.264 with AUD
/// delimiters to a local socket for the hardware-decoded live view.
async fn run_ffmpeg(cfg: &Value, hub: &Arc<FrameHub>) -> Result<()> {
    let q = (31.0 - cfg["quality"].as_f64().unwrap_or(80.0) * 0.29).round().clamp(2.0, 31.0) as u32;
    let passthrough = cfg!(target_os = "linux") && cfg["ffmpeg_input"].is_null();
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into()];
    args.extend(ffmpeg_input_args(cfg));
    args.push("-an".into());
    if passthrough { args.extend(["-c:v".into(), "copy".into()]); } else { args.extend(["-q:v".into(), q.to_string()]); }
    args.extend(["-f".into(), "mjpeg".into(), "pipe:1".into()]);
    let h264 = match cfg["h264_encoder"].as_str() {
        Some(enc) => {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
            let port = listener.local_addr()?.port();
            args.extend(["-map".into(), "0:v".into(), "-an".into(), "-c:v".into(), enc.to_string()]);
            if enc == "libx264" {
                args.extend(["-preset".into(), "ultrafast".into(), "-tune".into(), "zerolatency".into()]);
            }
            if enc.ends_with("_vaapi") {
                args.extend(["-vf".into(), "format=nv12,hwupload".into()]);
            } else {
                args.extend(["-pix_fmt".into(), "yuv420p".into()]);
            }
            let fps = cfg["fps"].as_u64().unwrap_or(30);
            args.extend(["-g".into(), fps.to_string(), "-bf".into(), "0".into(), "-bsf:v".into(), "h264_metadata=aud=insert".into(),
                "-f".into(), "h264".into(), format!("tcp://127.0.0.1:{port}")]);
            let hub2 = hub.clone();
            Some(tokio::spawn(async move {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let mut buf = Vec::with_capacity(1 << 20);
                let mut chunk = vec![0u8; 1 << 16];
                while let Ok(n) = sock.read(&mut chunk).await {
                    if n == 0 { break; }
                    buf.extend_from_slice(&chunk[..n]);
                    for au in split_aus(&mut buf) { crate::h264::push(&hub2, au); }
                }
            }))
        }
        None => None,
    };
    struct Abort(Option<tokio::task::JoinHandle<()>>);
    impl Drop for Abort { fn drop(&mut self) { if let Some(h) = &self.0 { h.abort(); } } }
    let _h264 = Abort(h264);
    let mut child = Command::new(ffmpeg()).args(&args).stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).kill_on_drop(true).spawn()
        .map_err(|_| anyhow!("video capture needs ffmpeg: {}", ffmpeg_hint()))?;
    let mut out = child.stdout.take().unwrap();
    // Keep reading ffmpeg's messages while it runs (a full pipe would stall it); keep the tail.
    let tail = Arc::new(Mutex::new(String::new()));
    let reader = child.stderr.take().map(|mut err| {
        let tail = tail.clone();
        tokio::spawn(async move {
            let mut b = [0u8; 4096];
            while let Ok(n) = err.read(&mut b).await {
                if n == 0 { break; }
                let mut t = tail.lock().unwrap();
                t.push_str(&String::from_utf8_lossy(&b[..n]));
                if t.len() > 4096 { let cut = t.len() - 2048; let cut = (cut..t.len()).find(|i| t.is_char_boundary(*i)).unwrap_or(0); t.drain(..cut); }
            }
        })
    });
    let _stderr = Abort(reader);
    let mut buf = Vec::with_capacity(1 << 20);
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        let n = capture_read(&mut out, &mut chunk, Duration::from_secs(10)).await?;
        if n == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let e = tail.lock().unwrap().clone();
            bail!("ffmpeg exited: {}", e.trim());
        }
        buf.extend_from_slice(&chunk[..n]);
        for f in split_jpegs(&mut buf) { hub.push(f); }
    }
}

/// Run the configured source forever, restarting with backoff. `cfg` is re-read on each start.
pub async fn supervise(cfg: Arc<Mutex<Value>>, hub: Arc<FrameHub>, logs: LogHub) {
    let mut delay = Duration::from_secs(1);
    loop {
        let c = cfg.lock().unwrap().clone();
        let name = backend_name(&c);
        *hub.source.lock().unwrap() = Some(name.to_string());
        if name == "none" {
            *hub.error.lock().unwrap() = Some("video disabled".into());
            return;
        }
        logs.add("bench", format!("video ({name}): {} {}x{}@{} q{}", c["device"].as_str().unwrap_or("default"),
            c["width"], c["height"], c["fps"], c["quality"]));
        let started = Instant::now();
        let r = if name == "avf" { run_avf(&c, &hub, &logs).await } else { run_ffmpeg(&c, &hub).await };
        if let Err(e) = r {
            *hub.error.lock().unwrap() = Some(e.to_string());
            logs.error("bench", format!("video ({name}): {e}; retrying in {}s", delay.as_secs()));
        }
        if started.elapsed() > Duration::from_secs(30) { delay = Duration::from_secs(1); }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(15));
    }
}

/// Capture devices and their formats (macOS helper; best effort elsewhere).
pub async fn list_devices() -> Value {
    if has_helper() {
        if let Ok(h) = build_helper().await {
            if let Ok(out) = Command::new(h).arg("formats").output().await {
                if let Ok(mut devs) = serde_json::from_slice::<Value>(&out.stdout) {
                    for d in devs.as_array_mut().into_iter().flatten() {
                        // keep one entry per size: best fps, prefer 420v
                        let mut best: Vec<Value> = vec![];
                        for f in d["formats"].as_array().cloned().unwrap_or_default() {
                            match best.iter_mut().find(|b| b["width"] == f["width"] && b["height"] == f["height"]) {
                                Some(b) if f["max_fps"].as_f64() > b["max_fps"].as_f64()
                                    || (f["max_fps"] == b["max_fps"] && f["pixel"] == "420v") => *b = f,
                                Some(_) => {}
                                None => best.push(f),
                            }
                        }
                        best.sort_by(|a, b| {
                            let pa = a["width"].as_f64().unwrap_or(0.0) * a["height"].as_f64().unwrap_or(0.0);
                            let pb = b["width"].as_f64().unwrap_or(0.0) * b["height"].as_f64().unwrap_or(0.0);
                            pb.partial_cmp(&pa).unwrap()
                        });
                        d["formats"] = Value::Array(best);
                    }
                    return devs;
                }
            }
        }
        return json!([]);
    }
    let out = if cfg!(windows) {
        Command::new(ffmpeg()).args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"]).output().await
    } else {
        Command::new("sh").args(["-c", "for d in /dev/video*; do echo \"== $d $(cat /sys/class/video4linux/$(basename $d)/name 2>/dev/null)\"; ffmpeg -hide_banner -f v4l2 -list_formats all -i $d 2>&1 | tail -20; done"]).output().await
    };
    match out {
        Ok(o) => json!([{"id": null, "name": "see raw", "formats": [], "raw": format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))}]),
        Err(e) => json!([{"id": null, "name": "ffmpeg missing", "formats": [], "raw": e.to_string()}]),
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn stalled_capture_returns_an_error_for_supervisor_recovery() {
        let (mut reader, _writer) = tokio::io::duplex(64);
        let mut bytes = [0; 5];
        let result = tokio::time::timeout(std::time::Duration::from_millis(200),
            super::capture_read(&mut reader, &mut bytes, std::time::Duration::from_millis(20))).await;
        assert!(result.is_ok(), "a silent capture must not hang forever");
        assert!(result.unwrap().unwrap_err().to_string().contains("no video data"));
    }

    #[tokio::test]
    async fn capture_read_preserves_frames_and_disconnects() {
        use tokio::io::AsyncWriteExt;
        let (mut reader, mut writer) = tokio::io::duplex(64);
        writer.write_all(b"frame").await.unwrap();
        drop(writer);
        let mut bytes = [0; 5];
        assert_eq!(super::capture_read(&mut reader, &mut bytes, std::time::Duration::from_secs(1)).await.unwrap(), 5);
        assert_eq!(&bytes, b"frame");
        assert_eq!(super::capture_read(&mut reader, &mut bytes, std::time::Duration::from_secs(1)).await.unwrap(), 0);
    }
    #[test]
    fn split() {
        let mut b = b"junk\xff\xd8one\xff\xd9\xff\xd8tw".to_vec();
        let f = super::split_jpegs(&mut b);
        assert_eq!(f.len(), 1);
        assert_eq!(&f[0][..], b"\xff\xd8one\xff\xd9");
        assert_eq!(b, b"\xff\xd8tw");
    }
}

#[cfg(test)]
mod aud_tests {
    #[test]
    fn splits_access_units_on_aud() {
        let aud = [0u8, 0, 0, 1, 0x09, 0xF0];
        let mut buf = vec![];
        buf.extend(aud);
        buf.extend([0, 0, 0, 1, 0x67, 1, 2]); // SPS
        buf.extend(aud);
        buf.extend([0, 0, 0, 1, 0x41, 3]); // slice
        buf.extend(aud); // start of a third, unfinished AU
        buf.extend([0, 0, 0, 1, 0x41]);
        let aus = super::split_aus(&mut buf);
        assert_eq!(aus.len(), 2);
        assert_eq!(&aus[0][6..11], &[0, 0, 0, 1, 0x67]);
        assert_eq!(&buf[..5], &[0, 0, 0, 1, 0x09]); // the unfinished AU stays buffered
    }
}
