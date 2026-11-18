//! Power and wake: bring the console back from a dimmed screen, Sleep Mode, or a crash.
//!
//! Three ways, tried in this order by "wake":
//!   1. HDMI-CEC: a USB-CEC adapter in the HDMI line sends the "power on" a TV would send
//!      (the console's "Match TV Power State" setting must be on). Uses libcec's `cec-client`,
//!      or the Linux kernel's `cec-ctl`. Nothing mechanical, nothing changed on the console.
//!   2. A power-button command the person sets: for a button presser (SwitchBot), a Home
//!      Assistant webhook, a relay. The only way back from a crash or power-off.
//!   3. A press through the Bridge: wakes a dimmed screen (the console is still on).
//!
//! The settings live in ~/.ns2bench/power.json. Only the person at the NS2Bench page can change
//! them (the command is run on this computer); agents can only ask for wake / sleep / button.

use crate::config::{state_dir, write_json};
use crate::server::{bad, ApiError, R, S};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const RUN_LIMIT: Duration = Duration::from_secs(20);

fn path() -> std::path::PathBuf {
    state_dir().join("power.json")
}

/// The saved settings, with defaults filled in.
pub fn settings() -> Value {
    let v: Value = std::fs::read(path()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or(json!({}));
    json!({
        "cec": v["cec"].as_bool().unwrap_or(false),
        "cec_port": v["cec_port"].as_str().unwrap_or(""),
        // the console as a CEC playback device; 4 is the first playback address
        "cec_address": v["cec_address"].as_u64().filter(|a| (1..=14).contains(a)).unwrap_or(4),
        "button_command": v["button_command"].as_str().unwrap_or(""),
        "wake_with_button": v["wake_with_button"].as_bool().unwrap_or(false),
    })
}

fn validate(v: &Value) -> anyhow::Result<Value> {
    let o = v.as_object().ok_or_else(|| anyhow::anyhow!("power settings must be an object"))?;
    let allowed = ["cec", "cec_port", "cec_address", "button_command", "wake_with_button"];
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        anyhow::bail!("unknown power setting: {k}");
    }
    let port = v["cec_port"].as_str().unwrap_or("").trim().to_string();
    anyhow::ensure!(port.len() <= 200 && !port.starts_with('-') && !port.chars().any(char::is_control), "CEC adapter port is not a device name");
    let addr = v["cec_address"].as_u64().unwrap_or(4);
    anyhow::ensure!((1..=14).contains(&addr), "CEC address must be 1-14");
    let cmd = v["button_command"].as_str().unwrap_or("").trim().to_string();
    anyhow::ensure!(cmd.len() <= 2000 && !cmd.contains('\0'), "the power-button command is too long");
    Ok(json!({
        "cec": v["cec"].as_bool().unwrap_or(false), "cec_port": port, "cec_address": addr,
        "button_command": cmd, "wake_with_button": v["wake_with_button"].as_bool().unwrap_or(false),
    }))
}

/// Where programs are looked for: the app's own PATH (short when started from Finder) plus the
/// places Homebrew, package managers and the Pulse-Eight installer put them.
fn search_path() -> std::ffi::OsString {
    let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(windows) {
        for d in [r"C:\Program Files (x86)\Pulse-Eight\USB-CEC Adapter", r"C:\Program Files\Pulse-Eight\USB-CEC Adapter"] {
            dirs.push(d.into());
        }
    } else {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            dirs.push(d.into());
        }
        if let Some(h) = std::env::var_os("HOME") {
            dirs.push(std::path::PathBuf::from(h).join(".local/bin"));
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

fn find(name: &str) -> Option<std::path::PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::split_paths(&search_path()).map(|d| d.join(&exe)).find(|p| p.is_file())
}

/// Which CEC tool this computer has, and where.
fn cec_tool() -> Option<(&'static str, std::path::PathBuf)> {
    find("cec-client").map(|p| ("cec-client", p))
        .or_else(|| if cfg!(target_os = "linux") { find("cec-ctl").map(|p| ("cec-ctl", p)) } else { None })
}

/// Run a program with a time limit; returns (ok, last lines of output). The program gets the
/// wider PATH; on a time-out its whole process group is stopped (a hung `curl` under `sh` too),
/// and output is collected only briefly after it exits (a helper it started in the background
/// may keep the pipes open).
async fn run(mut c: Command, stdin: Option<&str>) -> (bool, String) {
    c.env("PATH", search_path())
        .stdin(if stdin.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    c.process_group(0);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => return (false, e.to_string()),
    };
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes()).await;
        drop(pipe);
    }
    let collect = |pipe: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>| {
        tokio::spawn(async move {
            let mut out = Vec::new();
            if let Some(mut p) = pipe {
                use tokio::io::AsyncReadExt;
                let _ = p.read_to_end(&mut out).await;
            }
            out
        })
    };
    let out = collect(child.stdout.take().map(|p| Box::new(p) as _));
    let err = collect(child.stderr.take().map(|p| Box::new(p) as _));
    let status = match tokio::time::timeout(RUN_LIMIT, child.wait()).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return (false, e.to_string()),
        Err(_) => {
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                // SAFETY: signals the group this child leads (process_group(0) above).
                unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
            }
            // Windows: cmd.exe and whatever it started (a curl), as a tree.
            #[cfg(windows)]
            if let Some(pid) = child.id() {
                let mut k = std::process::Command::new("taskkill");
                std::os::windows::process::CommandExt::creation_flags(&mut k, 0x0800_0000);
                let _ = k.args(["/T", "/F", "/PID", &pid.to_string()]).status();
            }
            let _ = child.kill().await;
            out.abort();
            err.abort();
            return (false, format!("no answer within {} s", RUN_LIMIT.as_secs()));
        }
    };
    let grab = |h: tokio::task::JoinHandle<Vec<u8>>| async move {
        let abort = h.abort_handle();
        let r = tokio::time::timeout(Duration::from_secs(1), h).await.ok().and_then(Result::ok).unwrap_or_default();
        abort.abort(); // a helper the command left running keeps the pipe: stop reading
        r
    };
    let text = format!("{}{}", String::from_utf8_lossy(&grab(out).await), String::from_utf8_lossy(&grab(err).await));
    let tail: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    (status.success(), tail[tail.len().saturating_sub(3)..].join(" · "))
}

/// Send "power on" (or "standby") to the console over HDMI-CEC.
async fn cec(s: &Value, on: bool) -> (bool, String) {
    let addr = s["cec_address"].as_u64().unwrap_or(4);
    let port = s["cec_port"].as_str().unwrap_or("");
    match cec_tool() {
        Some(("cec-client", path)) => {
            let mut c = Command::new(path);
            c.args(["-s", "-d", "1"]);
            if !port.is_empty() {
                c.arg(port);
            }
            // cec-client reads the logical address as hexadecimal (11 is "b")
            run(c, Some(&format!("{} {addr:x}\n", if on { "on" } else { "standby" }))).await
        }
        Some((_, path)) => {
            let mut c = Command::new(path);
            if !port.is_empty() {
                c.args(["-d", port]);
            }
            c.args(["--playback", "--to", &addr.to_string()]);
            if on {
                c.args(["--user-control-pressed", "ui-cmd=power-on-function", "--user-control-released"]);
            } else {
                c.arg("--standby");
            }
            run(c, None).await
        }
        None => (false, "no CEC tool: install libcec (cec-client), or on Linux v4l-utils (cec-ctl)".into()),
    }
}

/// The person's power-button command, run by their shell.
async fn button(s: &Value) -> (bool, String) {
    let cmd = s["button_command"].as_str().unwrap_or("");
    if cmd.is_empty() {
        return (false, "no power-button command set (Settings → Devices → Power and wake)".into());
    }
    #[cfg(windows)]
    let c = {
        // cmd.exe reads its command line as typed: passed as a normal argument, Rust would
        // escape the quotes in it (\"…\"), which cmd doesn't understand.
        let mut c = Command::new("cmd");
        c.arg("/C").raw_arg(cmd);
        c
    };
    #[cfg(not(windows))]
    let c = {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", cmd]);
        c
    };
    run(c, None).await
}

/// What the capture card shows.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Seen {
    /// No frames: capture stopped, card unplugged, or another app owns it. Nothing is known
    /// about the console, so the power button is never pressed on this.
    NoCapture,
    /// Frames, but black or the card's "No Signal" screen: the console is asleep or off.
    NoSignal,
    Picture,
}

async fn seen(b: &S) -> Seen {
    let Some(f) = b.target().ok().and_then(|c| c.frames.latest()).filter(|f| f.at.elapsed() < Duration::from_secs(3)) else { return Seen::NoCapture };
    if let Ok(img) = crate::calib::small(&f).await {
        let px: Vec<f64> = img.pixels().map(|p| p.0[0] as f64).collect();
        let mean = px.iter().sum::<f64>() / px.len().max(1) as f64;
        let var = px.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / px.len().max(1) as f64;
        if mean < 14.0 && var.sqrt() < 6.0 {
            return Seen::NoSignal;
        }
    }
    if b.ext.ocr.is_some() {
        if let Ok(lines) = b.read_screen(None, true).await {
            if crate::ocr::join(&lines, " ").to_lowercase().contains("no signal") {
                return Seen::NoSignal;
            }
        }
    }
    Seen::Picture
}

/// This console shows a real picture (not black, not the card's "No Signal" screen): it's on.
pub async fn showing_picture(b: &S, id: &str) -> bool {
    crate::console::SELECTED.scope(Some(id.to_string()), async { matches!(seen(b).await, Seen::Picture) }).await
}

/// The console has its USB up for the Bridge: it is on, whatever the picture says.
fn usb_up(b: &S) -> bool {
    let s = b.hub.status();
    s["connected"] == true && s["usb"] == "configured"
}

async fn wait_picture(b: &S, limit: Duration) -> bool {
    let t0 = Instant::now();
    loop {
        if seen(b).await == Seen::Picture {
            return true;
        }
        if t0.elapsed() >= limit {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// One power action at a time: two wakes at once could press the button twice (on, then off).
static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Wake the console: HDMI-CEC, then (only when the card shows no signal and the console's USB is
/// down) the power-button command if allowed, then a press for a dimmed screen.
pub async fn wake(b: &S, source: &str, full: bool) -> Value {
    let s = settings();
    let mut steps = vec![];
    let before = seen(b).await;
    if before != Seen::Picture && s["cec"] == true {
        let (ok, out) = cec(&s, true).await;
        steps.push(json!({"step": "HDMI-CEC power on", "ok": ok, "detail": out}));
        if ok && before == Seen::NoSignal {
            wait_picture(b, Duration::from_secs(30)).await;
        }
    }
    let button_set = s["wake_with_button"] == true && !s["button_command"].as_str().unwrap_or("").is_empty();
    // Before pressing the power button, be sure: a dark loading screen also looks black for a
    // moment, and a press would put a running console to sleep. No signal for 6 s in a row.
    let mut now = seen(b).await;
    if now == Seen::NoSignal && button_set {
        for _ in 0..6 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            now = seen(b).await;
            if now != Seen::NoSignal { break; }
        }
    }
    if now == Seen::NoSignal && button_set {
        if usb_up(b) {
            steps.push(json!({"step": "power button", "ok": false, "detail": "not pressed: the console's USB is up, so it is on (a press would put it to sleep)"}));
        } else {
            let (ok, out) = button(&s).await;
            steps.push(json!({"step": "power button", "ok": ok, "detail": if full { out } else if ok { String::new() } else { "the command failed (details in Settings → Devices → Power and wake)".into() }}));
            if ok {
                wait_picture(b, Duration::from_secs(30)).await;
            }
        }
    }
    // A dimmed screen wakes with any press; B does nothing on the HOME menu.
    if b.hub.status()["connected"] == true {
        let r = b.hub.press(&["B".into()], 100, source, false).await;
        steps.push(json!({"step": "press B (wakes a dimmed screen)", "ok": r.is_ok(), "detail": r.err().map(|e| e.to_string()).unwrap_or_default()}));
    }
    let after = seen(b).await;
    let awake = after == Seen::Picture;
    let msg = match after {
        Seen::Picture => "the console is awake",
        Seen::NoCapture => "no picture from the capture card, so NS2Bench can't tell whether the console is on and didn't press its power button: check the capture card (and that no other app is using it)",
        Seen::NoSignal if steps.is_empty() => "the capture card shows no signal, and no way to wake the console is set up: turn on HDMI-CEC or set a power-button command in Settings → Devices → Power and wake",
        Seen::NoSignal => "the capture card still shows no signal: check the steps below, or press the console's power button",
    };
    b.logs.add("bench", format!("{source}: wake: {msg}"));
    json!({"ok": awake, "picture": awake, "was_awake": before == Seen::Picture, "message": msg, "steps": steps})
}

pub async fn get_power(State(_b): State<S>, h: HeaderMap) -> Json<Value> {
    let mut s = settings();
    let set = !s["button_command"].as_str().unwrap_or("").is_empty();
    // The command may hold a secret (a webhook address): only the page sees it.
    if !crate::server::from_browser(&h) {
        s["button_command"] = Value::Null;
    }
    s["button_command_set"] = set.into();
    Json(json!({"settings": s, "cec_tool": cec_tool().map(|(n, _)| n),
        "can_wake": s["cec"] == true || (s["wake_with_button"] == true && set),
        "can_sleep": s["cec"] == true, "can_press_button": set}))
}

pub async fn set_power(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    // The command runs on this computer: never something an agent may set.
    if !crate::server::from_browser(&h) {
        return Err(ApiError(StatusCode::FORBIDDEN, "only the person at the NS2Bench page can change power settings".into()));
    }
    let v = validate(&v).map_err(|e| bad(e.to_string()))?;
    write_json(&path(), &v).map_err(anyhow::Error::from)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path(), std::fs::Permissions::from_mode(0o600));
    }
    b.logs.add("bench", format!("power settings saved: HDMI-CEC {}, power-button command {}",
        if v["cec"] == true { "on" } else { "off" }, if v["button_command"].as_str().unwrap_or("").is_empty() { "not set" } else { "set" }));
    Ok(Json(json!({"ok": true, "settings": v})))
}

/// POST /api/power {"action": "wake" | "sleep" | "button"}
pub async fn post_power(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    crate::settings::agents_blocked(&b, &h, &v)?;
    let source = crate::server::source_of(&h, &v);
    // The command's output can include what the command holds: only the page sees it.
    let full = crate::server::from_browser(&h);
    let Ok(_one) = BUSY.try_lock() else {
        return Err(ApiError(StatusCode::CONFLICT, "a power action is already running".into()));
    };
    let s = settings();
    match v["action"].as_str().unwrap_or("wake") {
        "wake" => Ok(Json(wake(&b, &source, full).await)),
        "sleep" => {
            if s["cec"] != true {
                return Err(bad("putting the console to sleep needs HDMI-CEC (Settings → Devices → Power and wake)"));
            }
            let (ok, out) = cec(&s, false).await;
            b.logs.add("bench", format!("{source}: sleep over HDMI-CEC: {}", if ok { "sent" } else { "failed" }));
            Ok(Json(json!({"ok": ok, "detail": out})))
        }
        "button" => {
            let (ok, out) = button(&s).await;
            b.logs.add("bench", format!("{source}: power button: {}", if ok { "pressed" } else { "the command failed" }));
            if !ok {
                return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, if full { out } else { "the power-button command failed".into() }));
            }
            Ok(Json(json!({"ok": true, "detail": if full { out } else { String::new() }})))
        }
        other => Err(bad(format!("action must be wake, sleep or button, not {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_checked() {
        assert!(validate(&json!({"cec": true, "cec_address": 4, "button_command": "curl -X POST http://ha.local/api/webhook/x"})).is_ok());
        assert!(validate(&json!({"cec_address": 15})).is_err());
        assert!(validate(&json!({"cec_port": "--evil"})).is_err());
        assert!(validate(&json!({"shell": "x"})).is_err());
        assert_eq!(validate(&json!({"button_command": "  x  "})).unwrap()["button_command"], "x");
    }
}
