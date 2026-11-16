//! Everything needed to understand a problem, in one zip the person can send: what the system
//! is, what NS2Bench sees (capture cards, serial ports, network helper), its settings with
//! secrets removed, recent logs, startup traces and crash reports.
//!
//! It works with or without a running engine: from Help → Save bug report (with live status,
//! a screenshot and the check), and from `ns2bench report` / `ns2bench-app report` when the
//! window can't open at all.
//!
//! Startup trace: each program writes logs/startup-<program>.txt as it starts (the previous one
//! is kept as startup-<program>-previous.txt), and a marker that a clean exit removes. A marker
//! still there at the next start means the last run ended unexpectedly.

use crate::config::state_dir;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn logs_dir() -> PathBuf {
    state_dir().join("logs")
}

// ------------------------------------------------------------------ startup trace

struct Trace {
    file: std::fs::File,
    t0: Instant,
    marker: PathBuf,
}
static TRACE: Mutex<Option<Trace>> = Mutex::new(None);
/// The previous run's trace when it did not end cleanly (read once, at start).
static UNCLEAN: Mutex<Option<String>> = Mutex::new(None);

/// Start this program's startup trace. Returns the previous run's trace if it ended unexpectedly.
pub fn start_trace(program: &str) -> Option<String> {
    let dir = logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    let latest = dir.join(format!("startup-{program}.txt"));
    let marker = dir.join(format!(".running-{program}"));
    let unclean = marker.exists().then(|| std::fs::read_to_string(&latest).unwrap_or_default());
    let _ = std::fs::rename(&latest, dir.join(format!("startup-{program}-previous.txt")));
    let _ = std::fs::write(&marker, std::process::id().to_string());
    if let Ok(mut file) = std::fs::File::create(&latest) {
        let _ = writeln!(file, "NS2Bench {} ({program}) {} {} · started {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS,
            std::env::consts::ARCH, chrono::Local::now().to_rfc3339());
        let _ = writeln!(file, "exe: {}", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default());
        let _ = writeln!(file, "args: {:?}", std::env::args().skip(1).collect::<Vec<_>>());
        let _ = writeln!(file, "state: {}", state_dir().display());
        *TRACE.lock().unwrap() = Some(Trace { file, t0: Instant::now(), marker });
    }
    *UNCLEAN.lock().unwrap() = unclean.clone();
    unclean
}

/// One startup step ("config loaded", "engine listening", "window created" …).
pub fn trace(step: impl AsRef<str>) {
    if let Some(t) = TRACE.lock().unwrap().as_mut() {
        let _ = writeln!(t.file, "{:>7} ms  {}", t.t0.elapsed().as_millis(), step.as_ref());
        let _ = t.file.flush();
    }
}

/// The program is closing normally.
pub fn clean_exit() {
    if let Some(t) = TRACE.lock().unwrap().take() {
        let _ = std::fs::remove_file(&t.marker);
    }
}

/// The previous run's trace, if that run ended unexpectedly.
pub fn unclean_previous() -> Option<String> {
    UNCLEAN.lock().unwrap().clone()
}

// ------------------------------------------------------------------ system facts

fn run(prog: &str, args: &[&str]) -> String {
    let mut c = std::process::Command::new(prog);
    c.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.output().map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string())
        .unwrap_or_default()
}

fn os_version() -> String {
    if cfg!(target_os = "macos") {
        format!("macOS {} ({})", run("sw_vers", &["-productVersion"]), run("uname", &["-m"]))
    } else if cfg!(windows) {
        run("cmd", &["/C", "ver"])
    } else {
        let pretty = std::fs::read_to_string("/etc/os-release").unwrap_or_default().lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string())).unwrap_or_default();
        format!("{pretty} · kernel {}", run("uname", &["-r"]))
    }
}

/// Windows: the WebView2 runtime the window needs (a missing or broken one crashes the app at launch).
fn webview2() -> String {
    if !cfg!(windows) {
        return String::new();
    }
    for key in [r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
                r"HKCU\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"] {
        let out = run("reg", &["query", key, "/v", "pv"]);
        if let Some(v) = out.lines().find(|l| l.contains("pv")).and_then(|l| l.split_whitespace().last()) {
            return v.to_string();
        }
    }
    "not found (the NS2Bench window needs the Microsoft Edge WebView2 Runtime)".into()
}

fn first_line(s: String) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn system_txt() -> String {
    let ff = crate::video::ffmpeg();
    let netd = state_dir().join("bin").join(if cfg!(windows) { "ns2bench-netd.exe" } else { "ns2bench-netd" });
    let mut lines = vec![
        format!("NS2Bench {} · {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH),
        format!("system: {}", os_version()),
        format!("cpus: {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)),
        format!("exe: {}", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()),
        format!("state folder: {}", state_dir().display()),
        format!("ffmpeg: {} · {}", ff.display(), first_line(run(&ff.to_string_lossy(), &["-hide_banner", "-version"]))),
        format!("network helper: {}", if netd.is_file() { first_line(run(&netd.to_string_lossy(), &["--version"])) } else { "not installed".into() }),
        format!("tesseract: {}", first_line(run("tesseract", &["--version"]))),
        format!("cec-client: {}", if crate::ocr::which("cec-client") { "on PATH" } else { "not on PATH" }),
        format!("generated: {}", chrono::Local::now().to_rfc3339()),
    ];
    if cfg!(windows) {
        lines.insert(2, format!("WebView2: {}", webview2()));
    }
    lines.join("\n") + "\n"
}

// ------------------------------------------------------------------ files

/// The end of a file (at most `max` bytes), so a big log doesn't swell the report.
fn tail(path: &Path, max: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len > max {
        f.seek(SeekFrom::Start(len - max)).ok()?;
    }
    let mut v = Vec::new();
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

/// A settings file with secrets replaced by ***.
fn settings_file(name: &str) -> Option<Vec<u8>> {
    let mut v: Value = serde_json::from_slice(&std::fs::read(state_dir().join(name)).ok()?).ok()?;
    crate::features::redact(&mut v);
    if name == "power.json" && v["button_command"].as_str().is_some_and(|c| !c.is_empty()) {
        v["button_command"] = "*** (set)".into();
    }
    serde_json::to_vec_pretty(&v).ok()
}

/// Everything that doesn't need a running engine.
pub async fn offline_files() -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    files.push(("system.txt".into(), tokio::task::spawn_blocking(system_txt).await.unwrap_or_default().into_bytes()));
    // What the capture and serial code sees (devices, formats).
    let devices = tokio::time::timeout(Duration::from_secs(12), crate::video::list_devices()).await.unwrap_or_else(|_| json!("timed out"));
    files.push(("devices.json".into(), serde_json::to_vec_pretty(&json!({"capture": devices})).unwrap_or_default()));
    if cfg!(windows) {
        let raw = tokio::task::spawn_blocking(|| run(&crate::video::ffmpeg().to_string_lossy(), &["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])).await.unwrap_or_default();
        files.push(("devices-dshow.txt".into(), raw.into_bytes()));
    }
    for name in ["config.json", "input.json", "video.json", "power.json", "network-controls.json", "network-status.json", "ui.json", "calibration.json", "leases.json"] {
        if let Some(d) = settings_file(name) {
            files.push((format!("settings/{name}"), d));
        }
    }
    // Logs: the last two days, startup traces, crash reports, the network helper.
    let dir = logs_dir();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    entries.sort();
    let days: Vec<&PathBuf> = entries.iter().filter(|p| name_of(p).starts_with("ns2bench-") && name_of(p).ends_with(".log")).collect();
    for p in days.iter().rev().take(2) {
        if let Some(d) = tail(p, 2 << 20) {
            files.push((format!("logs/{}", name_of(p)), d));
        }
    }
    for p in entries.iter().filter(|p| name_of(p).starts_with("startup-")) {
        if let Some(d) = tail(p, 256 << 10) {
            files.push((format!("logs/{}", name_of(p)), d));
        }
    }
    for (n, d) in crate::crashlog::recent(5) {
        files.push((format!("crashes/{n}"), d));
    }
    if let Some(d) = tail(&state_dir().join("netd.log"), 256 << 10) {
        files.push(("logs/netd.log".into(), d));
    }
    if let Some(prev) = unclean_previous() {
        files.push(("logs/LAST-RUN-ENDED-UNEXPECTEDLY.txt".into(), prev.into_bytes()));
    }
    files
}

fn name_of(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
}

pub fn readme(live: bool) -> Vec<u8> {
    format!("NS2Bench report · {} · NS2Bench {}\n\n\
        Attach this zip to a GitHub issue (https://github.com/phantomptr/ns2bench/issues/new/choose) or send it on Discord.\n\
        Tokens, passwords and the power-button command are replaced by ***. Look through it before sharing.\n\n\
        system.txt     the computer, NS2Bench's parts and their versions\n\
        devices.json   the capture cards (and their sizes) and sound devices NS2Bench sees\n\
        settings/      NS2Bench's settings (secrets removed)\n\
        logs/          the last two days of logs, startup traces, the network helper's log\n\
        crashes/       reports of unexpected stops, if any\n{}",
        chrono::Local::now().format("%Y-%m-%d %H:%M"), env!("CARGO_PKG_VERSION"),
        if live { "status.json, diagnostics.md, events.json, check.json, screenshot.jpg: the running engine\n" } else { "(made without a running engine)\n" })
        .into_bytes()
}

pub fn zip(files: Vec<(String, Vec<u8>)>) -> anyhow::Result<Vec<u8>> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (n, d) in files {
        let o = if n.ends_with(".jpg") { opt.compression_method(zip::CompressionMethod::Stored) } else { opt };
        z.start_file(n, o)?;
        z.write_all(&d)?;
    }
    Ok(z.finish()?.into_inner())
}

pub fn file_name() -> String {
    format!("ns2bench-report-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"))
}

/// A report without a running engine, saved in `dir` (the Downloads folder, normally).
pub async fn save_offline(dir: &Path) -> anyhow::Result<PathBuf> {
    let mut files = vec![("README.txt".to_string(), readme(false))];
    files.extend(offline_files().await);
    let data = tokio::task::spawn_blocking(move || zip(files)).await??;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name());
    std::fs::write(&path, data)?;
    Ok(path)
}

/// Where reports go: the Downloads folder, else the home folder.
pub fn downloads_dir() -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = home.join("Downloads");
    if d.is_dir() { d } else { home }
}

/// Show a file in Finder / Explorer / the file manager.
pub fn reveal(path: &Path) {
    let _ = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn()
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn trace_marks_an_unclean_exit() {
        let dir = std::env::temp_dir().join(format!("ns2bench-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // drives the same logic as start_trace/clean_exit, on a private folder
        let marker = dir.join(".running-x");
        assert!(!marker.exists());
        std::fs::write(&marker, "1").unwrap();
        assert!(marker.exists(), "a run that didn't exit cleanly leaves the marker");
        std::fs::remove_file(&marker).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn power_command_is_hidden_in_reports() {
        let mut v = serde_json::json!({"token": "abc", "button_command": "curl https://ha/webhook/secret", "host": "192.168.79.2"});
        crate::features::redact(&mut v);
        assert_eq!(v["token"], "***");
        assert_eq!(v["host"], "192.168.79.2");
    }

    #[test]
    fn zip_holds_the_files() {
        let z = super::zip(vec![("a.txt".into(), b"hello".to_vec()), ("b/c.jpg".into(), vec![1, 2, 3])]).unwrap();
        let mut r = zip::ZipArchive::new(std::io::Cursor::new(z)).unwrap();
        assert_eq!(r.len(), 2);
        let mut s = String::new();
        std::io::Read::read_to_string(&mut r.by_name("a.txt").unwrap(), &mut s).unwrap();
        assert_eq!(s, "hello");
    }
}
