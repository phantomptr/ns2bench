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
        let fps = {
            // count only the last 2 s, so a stopped capture reads 0 and not its last rate
            let mut t = self.times.lock().unwrap();
            let now = Instant::now();
            while t.front().is_some_and(|x| now.duration_since(*x) > Duration::from_secs(2)) { t.pop_front(); }
            t.len() as f64 / 2.0
        };
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

/// A program started without a console window: the Windows app has none of its own, and each
/// ffmpeg or helper would otherwise pop one up (every capture retry, every clip, every OCR read).
pub fn quiet(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    // never the parent's stdin (a GUI app on Windows has none); callers that feed one set it again
    c.stdin(std::process::Stdio::null());
    // (an AppImage's bundled libraries are for this app, not for system programs it starts)
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() { c.env_remove("LD_LIBRARY_PATH"); }
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
}

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

/// Most a splitter keeps while it waits for the end of a frame (a broken stream, not a frame).
const SPLIT_LIMIT: usize = 32 << 20;

/// Cuts a byte stream into JPEG frames (FFD8 .. FFD9). Each byte is scanned once however the
/// pipe chunks the stream, and a frame leaves the buffer without being copied again.
#[derive(Default)]
pub(crate) struct JpegSplitter {
    buf: bytes::BytesMut,
    /// Where the current frame starts (buf[0]), once its FFD8 has been seen.
    in_frame: bool,
    /// Bytes already searched for the marker being looked for.
    scanned: usize,
}

fn find2(b: &[u8], from: usize, m: [u8; 2]) -> Option<usize> {
    // jump between second-byte hits (memchr-style), then check the byte before
    let mut i = from;
    loop {
        let j = i + 1 + b.get(i + 1..)?.iter().position(|&x| x == m[1])?;
        if b[j - 1] == m[0] { return Some(j - 1); }
        i = j;
    }
}

impl JpegSplitter {
    pub(crate) fn push(&mut self, data: &[u8], mut out: impl FnMut(Bytes)) {
        self.buf.extend_from_slice(data);
        loop {
            if !self.in_frame {
                match find2(&self.buf, self.scanned, [0xFF, 0xD8]) {
                    Some(p) => { let _ = self.buf.split_to(p); self.in_frame = true; self.scanned = 2; }
                    None => {
                        // keep a trailing 0xFF: it may be half of the next marker
                        let keep = usize::from(self.buf.last() == Some(&0xFF));
                        let _ = self.buf.split_to(self.buf.len() - keep);
                        self.scanned = 0;
                        return;
                    }
                }
            }
            match find2(&self.buf, self.scanned.max(2), [0xFF, 0xD9]) {
                Some(p) => {
                    out(self.buf.split_to(p + 2).freeze());
                    self.in_frame = false;
                    self.scanned = 0;
                }
                None => {
                    self.scanned = self.buf.len().saturating_sub(1).max(2);
                    if self.buf.len() > SPLIT_LIMIT { *self = Self::default(); }
                    return;
                }
            }
        }
    }
}

pub fn ffmpeg_input_args(cfg: &Value) -> Vec<String> {
    if let Some(a) = cfg["ffmpeg_input"].as_array() {
        return a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    }
    let size = format!("{}x{}", cfg["width"].as_u64().unwrap_or(1920), cfg["height"].as_u64().unwrap_or(1080));
    let fps = cfg["fps_exact"].as_str().map(str::to_string).unwrap_or_else(|| cfg["fps"].as_u64().unwrap_or(30).to_string());
    let dev = cfg["device"].as_str().map(str::to_string);
    if cfg!(target_os = "macos") {
        vec!["-f".into(), "avfoundation".into(), "-framerate".into(), fps, "-video_size".into(), size, "-i".into(), format!("{}:none", dev.unwrap_or("0".into()))]
    } else if cfg!(windows) {
        let mut a: Vec<String> = vec!["-f".into(), "dshow".into(), "-rtbufsize".into(), "256M".into()];
        // (this console's card refused the size it was asked for: let it use its own default)
        if cfg["_device_default"] != true {
            a.extend(["-video_size".into(), size, "-framerate".into(), fps]);
        }
        a.extend(["-i".into(), format!("video={}", dev.unwrap_or_default())]);
        a
    } else {
        let mut a: Vec<String> = vec!["-f".into(), "v4l2".into()];
        // (a card with no MJPEG output, raw only: its frames are encoded here instead)
        if cfg["_raw"] != true { a.extend(["-input_format".into(), "mjpeg".into()]); }
        a.extend(["-video_size".into(), size, "-framerate".into(), fps, "-i".into(), dev.unwrap_or("/dev/video0".into())]);
        a
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
    let err = child.stderr.take().unwrap();
    let l2 = logs.clone();
    // its messages as they come (line by line, nothing kept): a chatty helper costs no memory
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(err).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() { l2.add("bench", line.chars().take(2000).collect::<String>()); }
        }
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

/// Cuts an Annex B stream (with AUD NALs from h264_metadata=aud=insert) into access units, each
/// byte scanned once. Without AUDs (an encoder that ignores the filter) the buffer is capped.
#[derive(Default)]
pub(crate) struct AuSplitter {
    buf: bytes::BytesMut,
    /// buf starts at an AUD
    aligned: bool,
    scanned: usize,
}

impl AuSplitter {
    fn next_aud(&self, from: usize) -> Option<usize> {
        let b = &self.buf;
        let mut i = from;
        while i + 4 < b.len() {
            if b[i + 3] > 1 { i += 4; continue; }
            if b[i] == 0 && b[i + 1] == 0 && b[i + 2] == 0 && b[i + 3] == 1 && b[i + 4] & 0x1F == 9 { return Some(i); }
            i += 1;
        }
        None
    }

    pub(crate) fn push(&mut self, data: &[u8], mut out: impl FnMut(Bytes)) {
        self.buf.extend_from_slice(data);
        loop {
            if !self.aligned {
                match self.next_aud(self.scanned) {
                    Some(p) => { let _ = self.buf.split_to(p); self.aligned = true; self.scanned = 5; }
                    None => {
                        let keep = self.buf.len().min(4);
                        let _ = self.buf.split_to(self.buf.len() - keep);
                        self.scanned = 0;
                        return;
                    }
                }
            }
            match self.next_aud(self.scanned.max(5)) {
                Some(p) => { out(self.buf.split_to(p).freeze()); self.scanned = 5; }
                None => {
                    self.scanned = self.buf.len().saturating_sub(4).max(5);
                    if self.buf.len() > SPLIT_LIMIT { *self = Self::default(); }
                    return;
                }
            }
        }
    }
}

/// One ffmpeg reads the device once (V4L2/DirectShow devices usually allow a single reader) and
/// writes MJPEG to stdout for the live view, screenshots and OCR, plus, when "h264_encoder" is set
/// (libx264, h264_vaapi, h264_nvenc, h264_qsv, h264_mf, h264_videotoolbox ...), H.264 with AUD
/// delimiters to a local socket for the hardware-decoded live view.
async fn run_ffmpeg(cfg: &Value, hub: &Arc<FrameHub>) -> Result<()> {
    // Windows: ask the card only for a size and frame rate it offers (internal cards such as an
    // Elgato 4K Pro often don't offer exactly what was asked for, and DirectShow then refuses).
    let mut cfg = cfg.clone();
    if cfg!(windows) && cfg["ffmpeg_input"].is_null() {
        if let Some(dev) = cfg["device"].as_str().filter(|d| !d.is_empty()) {
            let formats = dshow_formats(dev).await;
            let (w, h, f) = pick_format(&formats, cfg["width"].as_u64().unwrap_or(1920), cfg["height"].as_u64().unwrap_or(1080), cfg["fps"].as_u64().unwrap_or(60));
            cfg["width"] = w.into(); cfg["height"] = h.into(); cfg["fps_exact"] = f.into();
        }
    }
    let cfg = &cfg;
    let q = (31.0 - cfg["quality"].as_f64().unwrap_or(80.0) * 0.29).round().clamp(2.0, 31.0) as u32;
    let passthrough = cfg!(target_os = "linux") && cfg["ffmpeg_input"].is_null() && cfg["_raw"] != true;
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
                let mut split = AuSplitter::default();
                let mut chunk = vec![0u8; 1 << 16];
                while let Ok(n) = sock.read(&mut chunk).await {
                    if n == 0 { break; }
                    split.push(&chunk[..n], |au| crate::h264::push(&hub2, au));
                }
            }))
        }
        None => None,
    };
    struct Abort(Option<tokio::task::JoinHandle<()>>);
    impl Drop for Abort { fn drop(&mut self) { if let Some(h) = &self.0 { h.abort(); } } }
    let _h264 = Abort(h264);
    let mut child = quiet(ffmpeg()).args(&args).stdout(std::process::Stdio::piped())
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
    let mut split = JpegSplitter::default();
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        let n = capture_read(&mut out, &mut chunk, Duration::from_secs(10)).await?;
        if n == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let e = tail.lock().unwrap().clone();
            // Only a refused format switches to the card's default; "in use" (OBS) or an unplugged
            // card is not the format's fault.
            if cfg!(windows) && (e.contains("Could not set video options") || e.contains("Could not find output pin")) {
                bail!("{REFUSED}the capture card refused {}x{} at {} fps; trying its own default next: {}", cfg["width"], cfg["height"], cfg["fps_exact"].as_str().unwrap_or(""), e.trim());
            }
            bail!("ffmpeg exited: {}", e.trim());
        }
        split.push(&chunk[..n], |f| hub.push(f));
    }
}

/// Run the configured source forever, restarting with backoff. `cfg` is re-read on each start.
/// `save`: keeps a capture card picked automatically (the console's settings).
/// Capture cards other consoles use: an automatic pick never takes one of them.
pub type Taken = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

pub async fn supervise(cfg: Arc<Mutex<Value>>, hub: Arc<FrameHub>, logs: LogHub, save: Option<Arc<dyn Fn() + Send + Sync>>, taken: Option<Taken>) {
    let mut delay = Duration::from_secs(1);
    let mut h264_failures = 0u32;
    loop {
        let c = cfg.lock().unwrap().clone();
        let name = backend_name(&c);
        *hub.source.lock().unwrap() = Some(name.to_string());
        if name == "none" {
            *hub.error.lock().unwrap() = Some("video disabled".into());
            return;
        }
        // No card chosen yet (a first start, or settings from another computer): pick the most
        // likely capture card, and keep it (the sound pairs with it, the Video panel shows it).
        if name == "ffmpeg" && !cfg!(target_os = "macos") && c["ffmpeg_input"].is_null() && c["device"].as_str().unwrap_or("").is_empty() {
            if !ffmpeg().is_file() && !crate::ocr::which("ffmpeg") {
                *hub.error.lock().unwrap() = Some(format!("video capture needs ffmpeg: {}", ffmpeg_hint()));
                logs.error("bench", format!("video: ffmpeg is missing: {}", ffmpeg_hint()));
                tokio::time::sleep(Duration::from_secs(15)).await;
                continue;
            }
            // never a card another console already shows (one console picks at a time: two
            // starting together would not see each other's pick and take the same card)
            static PICKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
            let _one = PICKING.lock().await;
            let mut found = list_devices().await;
            if let (Some(taken), Some(list)) = (&taken, found.as_array_mut()) {
                let used = taken();
                list.retain(|d| !d["id"].as_str().is_some_and(|id| used.iter().any(|u| u == id)));
            }
            match pick_capture_card(&found) {
                Some(d) => {
                    logs.add("bench", format!("video: no capture card was chosen; using {d}"));
                    cfg.lock().unwrap()["device"] = d.clone().into();
                    if let Some(save) = &save { save(); }
                    continue;
                }
                None => {
                    *hub.error.lock().unwrap() = Some("no capture card found".into());
                    logs.error("bench", "video: no capture card found. Connect one (and close OBS or other apps using it), then pick it in the Video panel");
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    continue;
                }
            }
        }
        logs.add("bench", format!("video ({name}): {} {}x{}@{} q{}", c["device"].as_str().unwrap_or("default"),
            c["width"], c["height"], c["fps"], c["quality"]));
        let started = Instant::now();
        // A hardware H.264 encoder this computer lacks (nvenc without an NVIDIA card) stops the
        // one ffmpeg that also makes the picture: after quick failures, go on without it.
        let mut c = c;
        if h264_failures >= 3 && !c["h264_encoder"].is_null() {
            logs.warn("bench", format!("video: the H.264 encoder {} doesn't work here: continuing without it (the live view uses MJPEG)", c["h264_encoder"]));
            c["h264_encoder"] = Value::Null;
        }
        let r = if name == "avf" { run_avf(&c, &hub, &logs).await } else { run_ffmpeg(&c, &hub).await };
        if let Err(e) = r {
            if refused(&e.to_string()) { cfg.lock().unwrap()["_device_default"] = true.into(); }
            // Linux asks the card for MJPEG and passes it on untouched. A card that only gives
            // raw frames fails that at once, every time: try the other way next (and back, if
            // that fails too: the card may simply be busy or unplugged).
            if cfg!(target_os = "linux") && name == "ffmpeg" && c["ffmpeg_input"].is_null() && started.elapsed() < Duration::from_secs(8) {
                let mut g = cfg.lock().unwrap();
                let raw = g["_raw"] != true;
                g["_raw"] = raw.into();
                if raw { logs.add("bench", "video: no MJPEG picture from this card; trying its raw output next"); }
            }
            *hub.error.lock().unwrap() = Some(e.to_string());
            logs.error("bench", format!("video ({name}): {e}; retrying in {}s", delay.as_secs()));
            if !c["h264_encoder"].is_null() && started.elapsed() < Duration::from_secs(10) { h264_failures += 1; }
        } else {
            h264_failures = 0;
        }
        if started.elapsed() > Duration::from_secs(30) { delay = Duration::from_secs(1); }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(15));
    }
}

// ------------------------------------------------------------------ Windows (DirectShow) and Linux devices

/// `ffmpeg -list_devices true -f dshow -i dummy`: (video names, audio names). Handles both the
/// current `"Name" (video)` lines and older ffmpeg's "DirectShow video devices" sections.
pub fn parse_dshow_devices(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut video, mut audio) = (vec![], vec![]);
    let mut section = "";
    for line in text.lines() {
        let l = line.split_once(']').map_or(line, |(_, r)| r).trim();
        if l.starts_with("DirectShow video devices") { section = "video"; continue; }
        if l.starts_with("DirectShow audio devices") { section = "audio"; continue; }
        if l.starts_with("Alternative name") || !l.starts_with('"') { continue; }
        let Some(end) = l[1..].find('"') else { continue };
        let name = l[1..1 + end].to_string();
        // "(video)", "(audio)", "(video, audio)" (one device with both pins), "(none)"; older
        // ffmpeg: no parentheses, the section says which.
        let rest = l[1 + end + 1..].trim();
        let kinds: Vec<&str> = match rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            Some(list) => list.split(',').map(str::trim).collect(),
            None => vec![section],
        };
        if kinds.contains(&"video") && !video.contains(&name) { video.push(name.clone()); }
        if kinds.contains(&"audio") && !audio.contains(&name) { audio.push(name); }
    }
    (video, audio)
}

/// `ffmpeg -f dshow -list_options true -i video=NAME`: one entry per size with its best frame rate.
pub fn parse_dshow_options(text: &str) -> Vec<Value> {
    // per size: (width, height, min fps, max fps, pixel format)
    let mut best: Vec<(u64, u64, f64, f64, String)> = vec![];
    for line in text.lines() {
        let l = line.split_once(']').map_or(line, |(_, r)| r).trim();
        let pixel = l.split_whitespace().next().unwrap_or("").replace("pixel_format=", "").replace("vcodec=", "");
        // "... min s=1280x720 fps=5 max s=1920x1080 fps=60"
        let mut found: Vec<(u64, u64, f64)> = vec![];
        let mut it = l.split_whitespace().peekable();
        while let Some(tok) = it.next() {
            let Some(size) = tok.strip_prefix("s=") else { continue };
            let Some(fps) = it.peek().and_then(|f| f.strip_prefix("fps=")).and_then(|f| f.parse::<f64>().ok()) else { continue };
            let Some((w, h)) = size.split_once('x').and_then(|(w, h)| Some((w.parse::<u64>().ok()?, h.parse::<u64>().ok()?))) else { continue };
            found.push((w, h, fps));
        }
        // the line's min and max: one size (most cards) with a range of rates, or two sizes
        let lo = found.iter().map(|f| f.2).fold(f64::INFINITY, f64::min);
        for &(w, h, fps) in &found {
            let min = if found.iter().all(|f| f.0 == w && f.1 == h) { lo } else { fps };
            match best.iter_mut().find(|b| b.0 == w && b.1 == h) {
                Some(b) => {
                    if fps > b.3 { b.3 = fps; b.4 = pixel.clone(); }
                    if min < b.2 { b.2 = min; }
                }
                None => best.push((w, h, min, fps, pixel.clone())),
            }
        }
    }
    best.sort_by_key(|b| std::cmp::Reverse(b.0 * b.1));
    best.into_iter().map(|(w, h, lo, hi, p)| json!({"width": w, "height": h, "min_fps": lo, "max_fps": hi, "pixel": p})).collect()
}

/// The size and frame rate to ask a card for: what was asked if the card offers it, otherwise
/// 1920×1080 if offered, otherwise the largest size. The rate is the asked one when the card's
/// range allows it, else the card's own nearest rate exactly ("59.9402"): DirectShow refuses a
/// rate outside a format's range.
/// The device most likely to be the console's capture card: never a virtual camera; a name that
/// sounds like a capture card first; otherwise the first one.
pub fn pick_capture_card(devices: &Value) -> Option<String> {
    let virt = ["virtual", "obs ", "snap camera", "nvidia broadcast", "camo", "droidcam", "iriun", "xsplit", "manycam"];
    let capture = ["capture", "hdmi", "elgato", "cam link", "avermedia", "live gamer", "macrosilicon", "ugreen", "usb video", "magewell", "game"];
    let list: Vec<(String, String)> = devices.as_array().into_iter().flatten()
        .filter(|d| d["kind"] == "video")
        .filter_map(|d| Some((d["id"].as_str()?.to_string(), d["name"].as_str().unwrap_or("").to_lowercase())))
        .filter(|(_, n)| !virt.iter().any(|w| n.contains(w)))
        .collect();
    list.iter().find(|(_, n)| capture.iter().any(|w| n.contains(w))).or(list.first()).map(|(id, _)| id.clone())
}

pub fn pick_format(formats: &[Value], width: u64, height: u64, fps: u64) -> (u64, u64, String) {
    let has = |w: u64, h: u64| formats.iter().find(|f| f["width"] == w && f["height"] == h);
    let f = has(width, height).or_else(|| has(1920, 1080)).or_else(|| formats.first());
    match f {
        Some(f) => {
            let hi = f["max_fps"].as_f64().unwrap_or(fps as f64);
            let lo = f["min_fps"].as_f64().unwrap_or(hi);
            let want = fps as f64;
            let rate = if want > hi + 0.01 { hi } else if want < lo - 0.01 { lo } else { want };
            let rate = if (rate - rate.round()).abs() < 0.001 { format!("{}", rate.round() as u64) } else { format!("{rate}") };
            (f["width"].as_u64().unwrap_or(width), f["height"].as_u64().unwrap_or(height), rate)
        }
        None => (width, height, fps.to_string()),
    }
}

async fn ffmpeg_text(args: &[&str], limit: Duration) -> String {
    let mut c = Command::new(ffmpeg());
    c.args(args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    match tokio::time::timeout(limit, c.output()).await {
        Ok(Ok(o)) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        _ => String::new(),
    }
}

/// DirectShow device names only (video, audio): quick, bounded.
pub async fn dshow_devices_names() -> (Vec<String>, Vec<String>) {
    parse_dshow_devices(&ffmpeg_text(&["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"], Duration::from_secs(5)).await)
}

/// DirectShow capture and sound devices, each video device with the sizes it offers.
pub async fn dshow_devices() -> (Vec<Value>, Vec<String>) {
    let (video, audio) = parse_dshow_devices(&ffmpeg_text(&["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"], Duration::from_secs(5)).await);
    // All cards at once: the Video panel waits at most 8 s for the whole list.
    let out = futures::future::join_all(video.into_iter().map(|name| async move {
        let formats = dshow_formats(&name).await;
        json!({"id": name, "name": name, "kind": "video", "formats": formats})
    })).await;
    (out, audio)
}

pub async fn dshow_formats(name: &str) -> Vec<Value> {
    parse_dshow_options(&ffmpeg_text(&["-hide_banner", "-f", "dshow", "-list_options", "true", "-i", &format!("video={name}")], Duration::from_secs(3)).await)
}

/// `ffmpeg -f v4l2 -list_formats all`: sizes listed after the format names.
pub fn parse_v4l2_formats(text: &str) -> Vec<Value> {
    let mut sizes: Vec<(u64, u64)> = vec![];
    for line in text.lines().filter(|l| l.contains("Compressed") || l.contains("Raw")) {
        for tok in line.rsplit(':').next().unwrap_or("").split_whitespace() {
            if let Some((w, h)) = tok.split_once('x').and_then(|(w, h)| Some((w.parse::<u64>().ok()?, h.parse::<u64>().ok()?))) {
                if !sizes.contains(&(w, h)) { sizes.push((w, h)); }
            }
        }
    }
    sizes.sort_by_key(|s| std::cmp::Reverse(s.0 * s.1));
    // V4L2 doesn't say the frame rate here: offer up to 60.
    sizes.into_iter().map(|(w, h)| json!({"width": w, "height": h, "max_fps": 60.0})).collect()
}

/// When a card refuses the size and frame rate (ffmpeg: "Could not set video options"), the next
/// start lets it use its own default, as OBS does. Cleared when video settings change.
/// Marks the error for a card that refused the asked format (supervise then asks for its default).
const REFUSED: &str = "";
fn refused(e: &str) -> bool { e.contains("the capture card refused") }

/// Capture devices and their formats (macOS helper; best effort elsewhere).
pub async fn list_devices() -> Value {
    if has_helper() {
        if let Ok(h) = build_helper().await {
            if let Ok(out) = Command::new(h).arg("formats").output().await {
                if let Ok(mut devs) = serde_json::from_slice::<Value>(&out.stdout) {
                    for d in devs.as_array_mut().into_iter().flatten() {
                        // (the helper lists only cameras here: say so, as the other platforms do;
                        // without it Settings listed no cards and automatic picks found none)
                        if d.get("kind").is_none() { d["kind"] = "video".into(); }
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
    if cfg!(windows) {
        let (mut video, audio) = dshow_devices().await;
        video.extend(audio.into_iter().map(|a| json!({"id": a, "name": a, "kind": "audio", "formats": []})));
        return Value::Array(video);
    }
    // Linux: every /dev/videoN that offers capture sizes, with the card's name.
    let mut out = vec![];
    let mut nodes: Vec<String> = std::fs::read_dir("/dev").into_iter().flatten().flatten()
        .filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.starts_with("video")).map(|n| format!("/dev/{n}")).collect();
    // by number: /dev/video2 before /dev/video10
    nodes.sort_by_key(|n| n.trim_start_matches("/dev/video").parse::<u32>().unwrap_or(u32::MAX));
    let found = futures::future::join_all(nodes.into_iter().map(|dev| async move {
        let base = dev.trim_start_matches("/dev/").to_string();
        let name = std::fs::read_to_string(format!("/sys/class/video4linux/{base}/name")).map(|s| s.trim().to_string()).unwrap_or_else(|_| dev.clone());
        let formats = parse_v4l2_formats(&ffmpeg_text(&["-hide_banner", "-f", "v4l2", "-list_formats", "all", "-i", &dev], Duration::from_secs(3)).await);
        (!formats.is_empty()).then(|| json!({"id": dev, "name": name, "kind": "video", "formats": formats}))
    })).await;
    out.extend(found.into_iter().flatten());
    Value::Array(out)
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
    fn dshow_lists_and_options() {
        let new = "[dshow @ 0001] \"Game Capture 4K Pro\" (video)\n[dshow @ 0001]   Alternative name \"@device_pnp_\\\\?\\pci#ven_1cd7\"\n[dshow @ 0001] \"Game Capture 4K Pro\" (audio)\n[dshow @ 0001] \"Microphone (Realtek)\" (audio)\n[dshow @ 0001] \"OBS Virtual Camera\" (none)\n";
        let (v, a) = super::parse_dshow_devices(new);
        assert_eq!(v, ["Game Capture 4K Pro"]);
        assert_eq!(a, ["Game Capture 4K Pro", "Microphone (Realtek)"]);
        let old = "[dshow @ 0x1] DirectShow video devices (some may be both video and audio devices)\n[dshow @ 0x1]  \"USB Video\"\n[dshow @ 0x1]     Alternative name \"@device_pnp_x\"\n[dshow @ 0x1] DirectShow audio devices\n[dshow @ 0x1]  \"Digital Audio Interface (USB Digital Audio)\"\n";
        let (v, a) = super::parse_dshow_devices(old);
        assert_eq!(v, ["USB Video"]);
        assert_eq!(a, ["Digital Audio Interface (USB Digital Audio)"]);
        let opts = "[dshow @ 0x2] DirectShow video device options (from video devices)\n[dshow @ 0x2]  Pin \"Capture\" (alternative pin name \"0\")\n[dshow @ 0x2]   pixel_format=nv12  min s=1920x1080 fps=30 max s=1920x1080 fps=59.9402\n[dshow @ 0x2]   pixel_format=yuyv422  min s=1280x720 fps=60 max s=1280x720 fps=60\n[dshow @ 0x2]   unknown compression type 0x30313050  min s=3840x2160 fps=59.94 max s=3840x2160 fps=59.94\n";
        let f = super::parse_dshow_options(opts);
        assert_eq!(f[0]["width"], 3840);
        assert_eq!(f.iter().find(|x| x["width"] == 1920).unwrap()["max_fps"], 59.9402);
        // asked 1080p60: the card offers 1080p up to 59.94 -> 1920x1080 at 59
        assert_eq!(super::pick_format(&f, 1920, 1080, 60), (1920, 1080, "59.9402".to_string()));
        // asked a size it doesn't offer -> 1080p
        assert_eq!(super::pick_format(&f, 2560, 1440, 60), (1920, 1080, "59.9402".to_string()));
        assert_eq!(super::pick_format(&f, 1920, 1080, 30), (1920, 1080, "30".to_string()));
        // nothing known -> as asked
        assert_eq!(super::pick_format(&[], 1920, 1080, 60), (1920, 1080, "60".to_string()));
        // a device with both pins, and picking the capture card over a webcam and a virtual camera
        let (v, a) = super::parse_dshow_devices("[dshow @ 1] \"Game Capture 4K Pro\" (video, audio)\n");
        assert_eq!((v.len(), a.len()), (1, 1));
        let devs = serde_json::json!([{"id": "OBS Virtual Camera", "name": "OBS Virtual Camera", "kind": "video"},
            {"id": "Integrated Webcam", "name": "Integrated Webcam", "kind": "video"},
            {"id": "Game Capture 4K Pro", "name": "Game Capture 4K Pro", "kind": "video"}]);
        assert_eq!(super::pick_capture_card(&devs).as_deref(), Some("Game Capture 4K Pro"));
    }

    #[test]
    fn v4l2_sizes() {
        let t = "[video4linux2,v4l2 @ 0x5] Compressed:       mjpeg :          Motion-JPEG : 1920x1080 1280x720 640x480\n[video4linux2,v4l2 @ 0x5] Raw       :     yuyv422 :           YUYV 4:2:2 : 1920x1080 640x480\n";
        let f = super::parse_v4l2_formats(t);
        assert_eq!(f.len(), 3);
        assert_eq!(f[0]["width"], 1920);
    }

    #[test]
    fn split() {
        let stream = b"junk\xff\xd8one\xff\xd9\xff\xd8two\xff\xd9\xff\xd8th";
        // whole, and one byte at a time (markers cut in half across reads)
        for step in [stream.len(), 1, 3] {
            let mut s = super::JpegSplitter::default();
            let mut f = vec![];
            for c in stream.chunks(step) { s.push(c, |x| f.push(x)); }
            assert_eq!(f.len(), 2, "step {step}");
            assert_eq!(&f[0][..], b"\xff\xd8one\xff\xd9");
            assert_eq!(&f[1][..], b"\xff\xd8two\xff\xd9");
            assert_eq!(&s.buf[..], b"\xff\xd8th");
        }
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
        let mut lead = vec![7u8, 0, 0]; // junk before the first AUD
        lead.extend(&buf);
        for step in [lead.len(), 1, 2, 5] {
            let mut s = super::AuSplitter::default();
            let mut aus = vec![];
            for c in lead.chunks(step) { s.push(c, |x| aus.push(x)); }
            assert_eq!(aus.len(), 2, "step {step}");
            assert_eq!(&aus[0][6..11], &[0, 0, 0, 1, 0x67]);
            assert_eq!(&aus[1][..5], &[0, 0, 0, 1, 0x09]);
            assert_eq!(&s.buf[..5], &[0, 0, 0, 1, 0x09]); // the unfinished AU stays buffered
        }
        // no AUDs at all: the buffer stays bounded
        let mut s = super::AuSplitter::default();
        for _ in 0..1000 { s.push(&[0x41u8; 65536], |_| {}); }
        assert!(s.buf.len() < 8);
    }
}
