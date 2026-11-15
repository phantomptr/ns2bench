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

/// Which CEC tool this computer has.
fn cec_tool() -> Option<&'static str> {
    if crate::ocr::which("cec-client") {
        Some("cec-client")
    } else if cfg!(target_os = "linux") && crate::ocr::which("cec-ctl") {
        Some("cec-ctl")
    } else {
        None
    }
}

/// Run a program with a time limit; returns (ok, last lines of output).
async fn run(mut c: Command, stdin: Option<&str>) -> (bool, String) {
    c.stdin(if stdin.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
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
    match tokio::time::timeout(RUN_LIMIT, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let tail: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            (out.status.success(), tail[tail.len().saturating_sub(3)..].join(" · "))
        }
        Ok(Err(e)) => (false, e.to_string()),
        Err(_) => (false, format!("no answer within {} s", RUN_LIMIT.as_secs())),
    }
}

/// Send "power on" (or "standby") to the console over HDMI-CEC.
async fn cec(s: &Value, on: bool) -> (bool, String) {
    let addr = s["cec_address"].as_u64().unwrap_or(4);
    let port = s["cec_port"].as_str().unwrap_or("");
    match cec_tool() {
        Some("cec-client") => {
            let mut c = Command::new("cec-client");
            c.args(["-s", "-d", "1"]);
            if !port.is_empty() {
                c.arg(port);
            }
            run(c, Some(&format!("{} {addr}\n", if on { "on" } else { "standby" }))).await
        }
        Some(_) => {
            let mut c = Command::new("cec-ctl");
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
    let c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", cmd]);
        c
    };
    run(c, None).await
}

fn picture(b: &S) -> bool {
    b.frames.latest().is_some_and(|f| f.at.elapsed() < Duration::from_secs(3))
}

/// Wait up to `limit` for a live picture.
async fn wait_picture(b: &S, limit: Duration) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if picture(b) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    picture(b)
}

/// Wake the console: CEC, then the power-button command if allowed, then a press for a dimmed
/// screen. Each step says what it did.
pub async fn wake(b: &S, source: &str) -> Value {
    let s = settings();
    let mut steps = vec![];
    let had_picture = picture(b);
    if !had_picture && s["cec"] == true {
        let (ok, out) = cec(&s, true).await;
        steps.push(json!({"step": "HDMI-CEC power on", "ok": ok, "detail": out}));
        if ok {
            wait_picture(b, Duration::from_secs(15)).await;
        }
    }
    if !picture(b) && s["wake_with_button"] == true && !s["button_command"].as_str().unwrap_or("").is_empty() {
        let (ok, out) = button(&s).await;
        steps.push(json!({"step": "power button", "ok": ok, "detail": out}));
        if ok {
            wait_picture(b, Duration::from_secs(30)).await;
        }
    }
    // A dimmed screen wakes with any press; B does nothing on the HOME menu.
    if b.hub.status()["connected"] == true {
        let r = b.hub.press(&["B".into()], 100, source, false).await;
        steps.push(json!({"step": "press B (wakes a dimmed screen)", "ok": r.is_ok(), "detail": r.err().map(|e| e.to_string()).unwrap_or_default()}));
    }
    let awake = picture(b);
    let msg = if awake {
        "the console is awake"
    } else if steps.is_empty() {
        "no picture, and no way to wake the console is set up: turn on HDMI-CEC or set a power-button command in Settings → Devices → Power and wake"
    } else {
        "still no picture: check the steps below, or press the console's power button"
    };
    b.logs.add("bench", format!("{source}: wake: {msg}"));
    json!({"ok": awake, "picture": awake, "was_awake": had_picture, "message": msg, "steps": steps})
}

pub async fn get_power(State(_b): State<S>) -> Json<Value> {
    let s = settings();
    Json(json!({"settings": s, "cec_tool": cec_tool(),
        "can_wake": s["cec"] == true || (s["wake_with_button"] == true && !s["button_command"].as_str().unwrap_or("").is_empty()),
        "can_sleep": s["cec"] == true, "can_press_button": !s["button_command"].as_str().unwrap_or("").is_empty()}))
}

pub async fn set_power(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    // The command runs on this computer: never something an agent may set.
    if !crate::server::from_browser(&h) {
        return Err(ApiError(StatusCode::FORBIDDEN, "only the person at the NS2Bench page can change power settings".into()));
    }
    let v = validate(&v).map_err(|e| bad(e.to_string()))?;
    write_json(&path(), &v).map_err(anyhow::Error::from)?;
    b.logs.add("bench", format!("power settings saved: HDMI-CEC {}, power-button command {}",
        if v["cec"] == true { "on" } else { "off" }, if v["button_command"].as_str().unwrap_or("").is_empty() { "not set" } else { "set" }));
    Ok(Json(json!({"ok": true, "settings": v})))
}

/// POST /api/power {"action": "wake" | "sleep" | "button"}
pub async fn post_power(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    crate::settings::agents_blocked(&b, &h, &v)?;
    let source = crate::server::source_of(&h, &v);
    let s = settings();
    match v["action"].as_str().unwrap_or("wake") {
        "wake" => Ok(Json(wake(&b, &source).await)),
        "sleep" => {
            if s["cec"] != true {
                return Err(bad("putting the console to sleep needs HDMI-CEC (Settings → Devices → Power and wake)"));
            }
            let (ok, out) = cec(&s, false).await;
            b.logs.add("bench", format!("{source}: sleep over HDMI-CEC: {}", if ok { "sent" } else { &out }));
            Ok(Json(json!({"ok": ok, "detail": out})))
        }
        "button" => {
            let (ok, out) = button(&s).await;
            b.logs.add("bench", format!("{source}: power button: {}", if ok { "pressed" } else { &out }));
            if !ok {
                return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, out));
            }
            Ok(Json(json!({"ok": true, "detail": out})))
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
