//! Everything we can observe in one place: bench actions/errors, the NS2Bench Bridge daemon and its kernel
//! log, the offline network's DHCP/DNS, and any configured log files. Pushed live to every UI.

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

const MAX_ENTRIES: usize = 8000;

/// Levels, lowest first. "debug" is detail (every API call, viewer connects); "info" is
/// normal operation; "warn" is something to look at that may fix itself; "error" needs action.
pub const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

pub fn rank(level: &str) -> usize {
    LEVELS.iter().position(|l| *l == level).unwrap_or(1)
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub seq: u64,
    pub t: f64,
    pub source: String,
    pub level: String,
    pub text: String,
    /// What it means and what to do, for problems NS2Bench recognises.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Worth a desktop notification: its short title (the app decides whether to show it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
}

/// Level of a line from a source that doesn't say (log files, the board, its kernel).
pub fn guess_level(text: &str) -> &'static str {
    let t = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["error", "failed", "failure", "panic", "refus", "denied", "cannot ", "can't ", "could not", "fatal", "over-current"]) {
        "error"
    } else if has(&["warn", "retry", "retrying", "timed out", "timeout", "lost", "disconnect", "reconnect", "not ready", "dropped", "stall", "slow"]) {
        "warn"
    } else {
        "info"
    }
}

/// A plain-language explanation and next step for problems NS2Bench recognises.
pub fn hint_for(source: &str, text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    let has = |w: &str| t.contains(w);
    let hint = if has("ffmpeg") && (has("not found") || has("no such file")) {
        "ffmpeg isn't installed. Install it (macOS: brew install ffmpeg · Windows: winget install Gyan.FFmpeg · Linux: sudo apt install ffmpeg), then restart NS2Bench."
    } else if has("could not run graph") || (has("dshow") && (has("i/o error") || has("access is denied"))) {
        "Windows can't open the capture card. Close other apps that use it (OBS, Camera), and turn on Settings → Privacy & security → Camera → Let desktop apps access your camera."
    } else if has("/dev/video") && (has("permission denied") || has("eacces")) {
        "Linux won't let NS2Bench open the capture card. Add yourself to the video group (sudo usermod -aG video $USER), then sign out and back in."
    } else if (has("camera") || has("video")) && (has("not authorized") || has("permission") || has("tcc")) {
        "macOS hasn't allowed the camera. Open System Settings → Privacy & Security → Camera and turn on NS2Bench, then restart it."
    } else if has("no video data") || (source == "bench" && has("video (") && (has("error") || has("retrying") || has("failed"))) {
        "No picture from the capture card. Check the HDMI cable goes into the card's input and the console is docked and awake. Close OBS or other apps if they hold the card, and pick the card in the Video panel."
    } else if has("delivers only") && has("frames a second") {
        "The capture card itself sends fewer pictures than asked, before NS2Bench gets them. Usual causes: two capture cards on one USB hub or port group (give each its own port on the computer), a USB 2 cable or port, or the console sending a lower rate."
    } else if has("could not keep up") && has("frames a second") {
        "This computer was too busy to take every picture from the capture card. Close other heavy apps, lower the frame rate or size under Picture & sound, or (on Windows and Linux) check that a hardware encoder is used."
    } else if has("heartbeat timed out") || (has("bridge") && (has("closed the connection") || has("connection reset"))) {
        "The Bridge stopped answering. It powers from the console's USB port, so this is normal while the console sleeps; it reconnects by itself when the console wakes. Otherwise check its cable."
    } else if has("bridge") && (has("refused") || has("timed out") || has("unreachable") || has("no route") || has("host is down")) {
        "This computer can't reach the NS2Bench Bridge board. Check it's powered (the console must be awake), its network cable, and its address in Settings → Devices."
    } else if (has("token") || has("unauthorized") || has("authentication")) && (has("reject") || has("bad") || has("invalid") || has("wrong") || has("unauthorized")) {
        "The access token doesn't match. For the Bridge, reinstall it from Settings → Install → Bridge board; for the API, use the token from Settings → API & MCP."
    } else if has("forwarding") {
        "The network helper won't serve the console while this computer forwards traffic (internet sharing). Turn sharing off: NS2Bench keeps the console offline on purpose."
    } else if has("address already in use") || has("addrinuse") {
        "Another program uses this port. Quit the other NS2Bench copy (or the other program), or change the port in the config."
    } else if (has("serial") || has("tty") || has("com")) && (has("busy") || has("access is denied") || has("permission denied")) {
        "The serial port is busy or not allowed. Close other programs using it; on Linux add yourself to the dialout group (sudo usermod -aG dialout $USER) and sign in again."
    } else if has("no controller") && has("ch9329") {
        "A CH9329 is a keyboard and mouse only. Drive with Keyboard & mouse, or use the Bridge or a Pico for controller buttons."
    } else if source == "kernel" && (has("error -71") || has("error -110") || has("over-current") || has("reset high-speed")) {
        "The board's USB link to the console is unstable. Try another cable or port, and avoid unpowered hubs."
    } else if has("manual-only") || has("manual only") {
        "Block agents is on, so only the person at the NS2Bench page can drive. Turn it off in the Driving card to let agents drive."
    } else if has("not ready") && has("interface") {
        "The console's network adapter isn't ready. Plug it in and give it the address from Settings → Network."
    } else {
        return None;
    };
    Some(hint)
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

struct Inner {
    seq: u64,
    entries: VecDeque<Entry>,
    counts: HashMap<String, u64>,
    levels: [u64; 4],
    throttle: HashMap<String, f64>,
    disk: Option<std::sync::mpsc::Sender<Entry>>,
}

#[derive(Clone)]
pub struct LogHub {
    inner: Arc<Mutex<Inner>>,
    pub tx: broadcast::Sender<Entry>,
    files: Vec<(String, PathBuf)>,
}

impl LogHub {
    pub fn new(files_cfg: Option<&Value>) -> LogHub {
        let mut files = vec![("network".to_string(), crate::config::expand("~/.ns2bench/netd.log"))];
        if let Some(Value::Object(m)) = files_cfg {
            for (k, v) in m {
                if let Some(p) = v.as_str() {
                    files.retain(|(n, _)| n != k);
                    files.push((k.clone(), crate::config::expand(p)));
                }
            }
        }
        let (tx, _) = broadcast::channel(1024);
        LogHub {
            inner: Arc::new(Mutex::new(Inner { seq: 0, entries: VecDeque::new(), counts: HashMap::new(), levels: [0; 4], throttle: HashMap::new(), disk: None })),
            tx,
            files,
        }
    }

    pub fn add(&self, source: &str, text: impl Into<String>) {
        self.add_full(source, text, None, "info")
    }

    pub fn debug(&self, source: &str, text: impl Into<String>) {
        self.add_full(source, text, None, "debug")
    }

    pub fn warn(&self, source: &str, text: impl Into<String>) {
        self.add_full(source, text, None, "warn")
    }

    pub fn error(&self, source: &str, text: impl Into<String>) {
        self.add_full(source, text, None, "error")
    }

    /// A line whose source doesn't give a level: guessed from its words.
    pub fn add_guess(&self, source: &str, text: impl Into<String>, t: Option<f64>) {
        let text = text.into();
        let level = guess_level(&text);
        self.add_full(source, text, t, level)
    }

    /// A line from a followed log file. The network helper's per-lookup lines (every name a
    /// console asks for, every connection test) are details: debug, not info, so they don't
    /// bury everything else.
    fn add_file_line(&self, name: &str, line: &str) {
        let detail = name == "network" && line.trim_start().starts_with('{')
            && serde_json::from_str::<Value>(line).ok().is_some_and(|v| matches!(v["event"].as_str(), Some("dns" | "ctest")));
        if detail { self.add_full(name, pretty(line), None, "debug") } else { self.add_guess(name, pretty(line), None) }
    }

    /// Also write every line to ~/.ns2bench/logs/ns2bench-YYYY-MM-DD.log (kept 7 days),
    /// so a problem can be looked at after NS2Bench restarts.
    pub fn persist(&self, dir: PathBuf) {
        let (tx, rx) = std::sync::mpsc::channel::<Entry>();
        self.inner.lock().unwrap().disk = Some(tx);
        std::thread::spawn(move || disk_writer(dir, rx));
    }

    /// True at most once per `secs` for `key`: for problems that can repeat many times a second.
    pub fn throttled(&self, key: &str, secs: f64) -> bool {
        let mut g = self.inner.lock().unwrap();
        let t = now();
        match g.throttle.get(key) {
            Some(last) if t - last < secs => true,
            _ => { g.throttle.insert(key.to_string(), t); false }
        }
    }

    pub fn level_counts(&self) -> Value {
        let g = self.inner.lock().unwrap();
        serde_json::json!({"debug": g.levels[0], "info": g.levels[1], "warn": g.levels[2], "error": g.levels[3]})
    }

    pub fn add_full(&self, source: &str, text: impl Into<String>, t: Option<f64>, level: &str) {
        self.push(source, text.into(), t, level, None)
    }

    /// A line the desktop app may also show as a notification titled `title`.
    pub fn notify(&self, source: &str, level: &str, title: &str, text: impl Into<String>) {
        self.push(source, text.into(), None, level, Some(title.to_string()))
    }

    fn push(&self, source: &str, mut text: String, t: Option<f64>, level: &str, notify: Option<String>) {
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
        if text.is_empty() {
            return;
        }
        if text.len() > 2000 {
            let mut cut = 2000;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        let e = {
            let mut g = self.inner.lock().unwrap();
            g.seq += 1;
            let level = LEVELS[rank(level)];
            let hint = if rank(level) >= 2 { hint_for(source, &text).map(str::to_string) } else { None };
            let e = Entry { seq: g.seq, t: t.unwrap_or_else(now), source: source.into(), level: level.into(), text, hint, notify };
            g.levels[rank(level)] += 1;
            if let Some(d) = &g.disk {
                let _ = d.send(e.clone());
            }
            g.entries.push_back(e.clone());
            if g.entries.len() > MAX_ENTRIES {
                g.entries.pop_front();
            }
            *g.counts.entry(source.to_string()).or_insert(0) += 1;
            e
        };
        let _ = self.tx.send(e);
    }

    pub fn seq(&self) -> u64 {
        self.inner.lock().unwrap().seq
    }

    pub fn query(&self, source: Option<&str>, after: u64, limit: usize, contains: Option<&str>) -> Vec<Entry> {
        self.query_level(source, after, limit, contains, None)
    }

    /// `min_level`: only entries at this level or above ("warn" = warnings and errors).
    pub fn query_level(&self, source: Option<&str>, after: u64, limit: usize, contains: Option<&str>, min_level: Option<&str>) -> Vec<Entry> {
        let min = min_level.map(rank).unwrap_or(0);
        let g = self.inner.lock().unwrap();
        let contains = contains.map(|c| c.to_lowercase());
        // newest first, stopping at `limit`: only the entries returned are copied
        let n = limit.clamp(1, 2000);
        let mut v: Vec<Entry> = g
            .entries
            .iter()
            .rev()
            .take_while(|e| e.seq > after)
            .filter(|e| source.is_none_or(|s| e.source == s))
            .filter(|e| rank(&e.level) >= min)
            .filter(|e| contains.as_ref().is_none_or(|c| e.text.to_lowercase().contains(c)))
            .take(n)
            .cloned()
            .collect();
        v.reverse();
        v
    }

    pub fn all(&self) -> Vec<Entry> {
        self.inner.lock().unwrap().entries.iter().cloned().collect()
    }

    pub fn sources(&self) -> Vec<String> {
        let g = self.inner.lock().unwrap();
        let mut s: Vec<String> = g.counts.keys().cloned().collect();
        for (n, _) in &self.files {
            s.push(n.clone());
        }
        for n in ["bench", "board", "kernel"] {
            s.push(n.into());
        }
        s.sort();
        s.dedup();
        s
    }

    /// Follow each configured file from its current end (and across rotation).
    pub async fn tail_files(self) {
        let mut pos: HashMap<String, u64> = HashMap::new();
        for (name, path) in &self.files {
            if let Ok(data) = std::fs::read(path) {
                let start = data.len().saturating_sub(16384);
                let text = String::from_utf8_lossy(&data[start..]).to_string();
                let lines: Vec<&str> = text.lines().collect();
                if name == "network" {
                    for l in &lines { crate::netseen::observe(l, false); }
                }
                for l in &lines[lines.len().saturating_sub(30)..] {
                    self.add_file_line(name, l);
                }
                pos.insert(name.clone(), data.len() as u64);
            }
        }
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            for (name, path) in &self.files {
                let Ok(meta) = std::fs::metadata(path) else { continue };
                let mut off = *pos.get(name).unwrap_or(&0);
                if meta.len() < off {
                    off = 0; // rotated or truncated
                }
                if meta.len() > off {
                    // Read only what's new (at most 1 MB a second), not the whole file.
                    let want = (meta.len() - off).min(1 << 20) as usize;
                    let read = || -> std::io::Result<Vec<u8>> {
                        use std::io::{Read, Seek};
                        let mut f = std::fs::File::open(path)?;
                        f.seek(std::io::SeekFrom::Start(off))?;
                        let mut chunk = vec![0; want];
                        let n = f.read(&mut chunk)?;
                        chunk.truncate(n);
                        Ok(chunk)
                    };
                    if let Ok(chunk) = read() {
                        // keep a partial last line for the next round
                        let end = chunk.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
                        let end = if end == 0 && chunk.len() == want && want == 1 << 20 { chunk.len() } else { end };
                        for l in String::from_utf8_lossy(&chunk[..end]).lines() {
                            if name == "network" { crate::netseen::observe(l, true); }
                            self.add_file_line(name, l);
                        }
                        off += end as u64;
                    }
                }
                pos.insert(name.clone(), off);
            }
        }
    }
}

fn disk_writer(dir: PathBuf, rx: std::sync::mpsc::Receiver<Entry>) {
    use std::io::Write;
    use std::sync::mpsc::RecvTimeoutError;
    let _ = std::fs::create_dir_all(&dir);
    let mut day = String::new();
    let mut out: Option<std::io::BufWriter<std::fs::File>> = None;
    loop {
        let e = match rx.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(e) => e,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(o) = out.as_mut() { let _ = o.flush(); }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let local = chrono::DateTime::from_timestamp(e.t as i64, 0).map(|d| d.with_timezone(&chrono::Local)).unwrap_or_else(chrono::Local::now);
        // (the file is the day the line arrived: a board's replayed older lines keep their own
        // time in the text, without reopening and pruning files line by line)
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if today != day {
            if let Some(o) = out.as_mut() { let _ = o.flush(); }
            day = today;
            out = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("ns2bench-{day}.log"))).ok().map(std::io::BufWriter::new);
            prune_logs(&dir, 7);
        }
        if let Some(o) = out.as_mut() {
            let hint = e.hint.as_deref().map(|h| format!("  → {h}")).unwrap_or_default();
            let _ = writeln!(o, "{} {:<5} {:<8} {}{hint}", local.format("%Y-%m-%dT%H:%M:%S"), e.level.to_uppercase(), e.source, e.text.replace('\n', " ⏎ "));
        }
    }
    if let Some(o) = out.as_mut() { let _ = o.flush(); }
}

/// Keep the newest `keep` daily log files.
fn prune_logs(dir: &std::path::Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("ns2bench-") && n.ends_with(".log")))
        .collect();
    files.sort();
    let n = files.len().saturating_sub(keep);
    for f in &files[..n] {
        let _ = std::fs::remove_file(f);
    }
}

/// Compact JSON-lines logs (like netd's) into readable text.
pub fn pretty(line: &str) -> String {
    let line = line.trim();
    if line.starts_with('{') {
        if let Ok(Value::Object(mut m)) = serde_json::from_str::<Value>(line) {
            let ev = m.remove("event").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            m.remove("t");
            let rest: Vec<String> = m.iter().map(|(k, v)| format!("{k}={}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect();
            return format!("{ev} {}", rest.join(" "));
        }
    }
    line.to_string()
}

/// Local time stamp for file names: 20261114-203012
pub fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Local wall-clock time (HH:MM:SS) of a log timestamp.
pub fn local_time(t: f64) -> String {
    chrono::DateTime::from_timestamp(t as i64, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels_hints_and_filtering() {
        let h = LogHub::new(None);
        h.debug("bench", "api: GET /api/status");
        h.add("bench", "started");
        h.warn("bench", "NS2Bench Bridge heartbeat timed out; reconnecting");
        h.error("bench", "video (avfoundation): no video data for 10s");
        h.add_full("bench", "odd", None, "loud");
        let all = h.query(None, 0, 100, None);
        assert_eq!(all.len(), 5);
        assert_eq!(all[4].level, "info"); // unknown level -> info
        assert!(all[0].hint.is_none() && all[1].hint.is_none());
        assert!(all[2].hint.as_deref().unwrap().contains("sleeps"));
        assert!(all[3].hint.as_deref().unwrap().contains("HDMI"));
        assert_eq!(h.query_level(None, 0, 100, None, Some("warn")).len(), 2);
        assert_eq!(h.query_level(None, 0, 100, None, Some("error")).len(), 1);
        assert_eq!(h.level_counts()["debug"], 1);
        assert!(hint_for("bench", "video (ffmpeg): ffmpeg exited: Could not run graph").unwrap().contains("Let desktop apps"));
        assert!(hint_for("bench", "open /dev/video0: Permission denied").unwrap().contains("video group"));
        assert_eq!(guess_level("usb 1-1: device descriptor read/64, error -71"), "error");
        assert_eq!(guess_level("link lost, retrying"), "warn");
        assert_eq!(guess_level("lease 192.168.80.10"), "info");
    }
    #[test]
    fn disk_log_is_written_and_pruned() {
        let dir = std::env::temp_dir().join(format!("ns2bench-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for d in 1..=9 { std::fs::write(dir.join(format!("ns2bench-2000-01-0{d}.log")), "x").unwrap(); }
        let h = LogHub::new(None);
        h.persist(dir.clone());
        h.error("bench", "video (x): no video data for 10s");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(files.len(), 7);
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let text = std::fs::read_to_string(dir.join(format!("ns2bench-{today}.log"))).unwrap();
        assert!(text.contains("ERROR bench") && text.contains("→ No picture"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
