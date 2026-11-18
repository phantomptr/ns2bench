//! Check the whole chain and say which part is broken and what to do.
//! Each check is {"name", "status": ok|warn|fail|skip, "detail", "hint"}. Nothing is pressed
//! unless probe=true, and then only on the HOME menu (where a cursor move is harmless).

use crate::bench::Bench;
use serde_json::{json, Value};

fn check(name: &str, status: &str, detail: impl Into<String>, hint: &str) -> Value {
    json!({"name": name, "status": status, "detail": detail.into(), "hint": hint})
}

fn free_gb() -> Option<f64> {
    if cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("df").args(["-k", &crate::config::home().to_string_lossy()]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let kb: f64 = s.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb * 1024.0 / 1e9)
}

pub async fn run(b: &Bench, probe: bool) -> Value {
    let mut out = vec![];
    let st = b.status();
    let (v, i) = (&st["video"], &st["input"]);

    // video
    let frames = v["frames"].as_u64().unwrap_or(0);
    let err = v["error"].as_str().unwrap_or("");
    match v["age_s"].as_f64() {
        _ if frames == 0 => out.push(check("video", "fail", if err.is_empty() { "no frames" } else { err },
            "Is the capture card plugged in? Run `ns2bench devices` and check the Video card.")),
        Some(age) if age > 2.0 => out.push(check("video", "fail", format!("last frame {age} s ago ({err})"),
            "The capture stream stalled: replug the card or re-apply the Video settings.")),
        _ => {
            let want = b.target().map(|c| c.video_cfg.lock().unwrap()["fps"].as_f64().unwrap_or(30.0)).unwrap_or(30.0);
            let fps = v["fps"].as_f64().unwrap_or(0.0);
            let ok = fps >= want * 0.75;
            out.push(check("video", if ok { "ok" } else { "warn" }, format!("{} {fps} fps (asked {want})", v["source"].as_str().unwrap_or("?")),
                if ok { "" } else { "Another app may own the card format (OBS), or the machine is busy." }));
        }
    }

    // what the console shows
    if b.ext.ocr.is_some() && frames > 0 {
        match b.read_screen(None, true).await {
            Ok(lines) => {
                let text = crate::ocr::join(&lines, " ").to_lowercase();
                if text.contains("no signal") {
                    out.push(check("console picture", "fail", "capture card shows \"No Signal\"",
                        "The console is off, asleep, or HDMI/dock is disconnected."));
                } else {
                    let w = b.classify(&lines);
                    out.push(check("console picture", "ok", format!("screen: {}", if w.is_empty() { "unrecognised".into() } else { w.join(", ") }), ""));
                }
            }
            Err(e) => out.push(check("console picture", "warn", format!("could not read the screen: {e}"), "")),
        }
    }
    let a = &st["audio"];
    if let Some(e) = a["error"].as_str() {
        out.push(check("audio", "warn", e, "Set \"video\": {\"audio_device\": \"...\"} if pairing by USB location fails."));
    } else if let Some(d) = a["device"].as_str() {
        let heard = a["last_sound_s"].as_f64().map_or("no sound heard yet".into(), |s| format!("last sound {s} s ago"));
        out.push(check("audio", "ok", format!("{d}: {heard}"), ""));
    }
    out.push(match b.ext.ocr {
        Some(e) => check("ocr", "ok", e.name(), ""),
        None => check("ocr", "warn", "no OCR engine", "macOS: install Xcode command line tools; elsewhere install tesseract."),
    });

    // input board
    let connected = i["connected"].as_bool() == Some(true);
    if !connected {
        out.push(check("NS2Bench Bridge", "fail", i["detail"].as_str().unwrap_or("not connected"),
            "Check the Bridge connection (ping it), that NS2Bench Bridge runs (ns2bench-hidd.sh status) and the token."));
    } else {
        let rtt = i["rtt_ms"].as_f64().unwrap_or(0.0);
        let devices = crate::server::strs(&i["devices"]).join(", ");
        out.push(check("NS2Bench Bridge", if rtt < 20.0 { "ok" } else { "warn" },
            format!("{} rtt {rtt} ms, mode {}, devices {devices}", i["backend"].as_str().unwrap_or("?"), i["usb_mode"].as_str().unwrap_or("?")),
            if rtt < 20.0 { "" } else { "High round trip: check the Bridge network connection." }));
        let usb = i["usb"].as_str().unwrap_or("unknown");
        out.push(check("console USB", if usb == "configured" { "ok" } else { "fail" }, format!("gadget state {usb}"),
            if usb == "configured" { "" } else { "The console has not configured the board: check the USB cable/dock port." }));
        if i["manual_only"].as_bool() == Some(true) {
            out.push(check("agent input", "warn", "manual-only mode is on", "Agents cannot press anything until it is off."));
        }
    }

    // console responds to input (optional, HOME only)
    if probe {
        if !connected {
            out.push(check("console input", "skip", "NS2Bench Bridge offline", ""));
        } else {
            let where_: Vec<String> = if b.ext.ocr.is_some() {
                b.where_().await.map(|w| crate::server::strs(&w["screens"])).unwrap_or_default()
            } else { vec![] };
            if !where_.iter().any(|s| s == "home") {
                out.push(check("console input", "skip", format!("not on HOME ({}); probe only runs where a cursor move is harmless",
                    if where_.is_empty() { "unknown".into() } else { where_.join(", ") }), ""));
            } else {
                let _ = b.hub.press(&["B".into()], 100, "doctor", false).await; // wake a dimmed screen (no-op on HOME)
                let r1 = crate::calib::probe(b, &["RIGHT".into()], 100, 1500, "doctor").await;
                let r2 = crate::calib::probe(b, &["LEFT".into()], 100, 1500, "doctor").await;
                let desc = |r: &anyhow::Result<Value>| match r {
                    Ok(v) if v["changed"] == true => format!("{} ms", v["latency_ms"]),
                    Ok(_) => "no change".into(),
                    Err(e) => e.to_string(),
                };
                let ok = [&r1, &r2].iter().all(|r| matches!(r, Ok(v) if v["changed"] == true));
                out.push(check("console input", if ok { "ok" } else { "fail" }, format!("RIGHT {}, LEFT {}", desc(&r1), desc(&r2)),
                    if ok { "" } else { "Presses did not change the screen: the console may not have assigned this controller (open Change Grip/Order and press L+R), or the screen was busy." }));
            }
        }
    }

    // calibration
    let cal = b.calibration.lock().unwrap().clone();
    match cal["measured_at"].as_str() {
        Some(at) => out.push(check("calibration", "ok", format!("min press {} ms, latency {} ms, measured {at}", cal["min_press_ms"], cal["latency_ms_median"]), "")),
        None => out.push(check("calibration", "warn", "never measured", "Go to the HOME menu, then Settings → Devices → Calibrate (or POST /api/calibrate).")),
    }

    // waking from sleep or a crash (optional: a dimmed screen wakes with any press)
    let p = crate::power::settings();
    let ways: Vec<&str> = [(p["cec"] == true, "HDMI-CEC"), (!p["button_command"].as_str().unwrap_or("").is_empty(), "power-button command")]
        .into_iter().filter(|(on, _)| *on).map(|(_, n)| n).collect();
    out.push(if ways.is_empty() {
        check("power and wake", "skip", "nothing set up: NS2Bench can wake a dimmed screen, not Sleep Mode or a crash",
            "Settings → Devices → Power and wake: HDMI-CEC or a power-button command.")
    } else {
        check("power and wake", "ok", ways.join(" + "), "")
    });

    // offline console network, if configured
    if !b.cfg.raw["network"].is_null() {
        if cfg!(target_os = "macos") {
            let fwd = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "net.inet.ip.forwarding", "net.inet6.ip6.forwarding"]).output()
                .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
            if !fwd.is_empty() && fwd.iter().any(|x| x != "0") {
                out.push(check("console network isolation", "fail", format!("IP forwarding is on ({fwd:?})"), "Turn forwarding off: the console must not reach the internet."));
            } else if !fwd.is_empty() {
                out.push(check("console network isolation", "ok", "forwarding off", ""));
            }
        }
        // Shared home network: consoles have fixed addresses and get no leases (DHCP stays off on
        // purpose). What tells there is whether the helper has seen each console's lookups.
        if crate::shared_net::is_shared(b) {
            let consoles: Vec<(String, Option<String>)> = b.list().iter().map(|c| (c.name(), c.network_ip.lock().unwrap().clone())).collect();
            let seen: Vec<String> = consoles.iter().filter_map(|(n, ip)| ip.as_deref().filter(|ip| crate::netseen::model_at(ip).is_some()
                || crate::netseen::tested_within(ip, std::time::Duration::from_secs(24 * 3600))).map(|ip| format!("{n} ({ip})"))).collect();
            let missing: Vec<String> = consoles.iter().filter(|(n, ip)| ip.is_some() && !seen.iter().any(|s| s.starts_with(n.as_str()))).map(|(n, _)| n.clone()).collect();
            let no_ip: Vec<String> = consoles.iter().filter(|(_, ip)| ip.is_none()).map(|(n, _)| n.clone()).collect();
            let (state, detail, fix) = if !missing.is_empty() {
                ("warn", format!("not seen on the network yet: {}", missing.join(", ")), "Check that console's network settings: its DNS must be this computer's address (Settings → Network shows it). It shows up once it looks a name up.".to_string())
            } else if seen.is_empty() {
                ("warn", "no console address set".to_string(), "Give each console its network address in Settings → Consoles.".to_string())
            } else {
                ("ok", format!("seen: {}", seen.join(", ")), String::new())
            };
            out.push(check("console network (shared)", state, if no_ip.is_empty() { detail } else { format!("{detail}; no address set for {}", no_ip.join(", ")) }, &fix));
        } else {
        match std::fs::read_to_string(crate::config::state_dir().join("leases.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
            Some(leases) => {
                let now = crate::logs::now();
                let live: Vec<String> = leases.as_object().map(|m| m.iter().filter(|(_, l)| l["expires"].as_f64().unwrap_or(0.0) > now)
                    .map(|(mac, l)| format!("{} ({mac})", l["ip"].as_str().unwrap_or("?"))).collect()).unwrap_or_default();
                let any = !live.is_empty();
                out.push(check("console network", if any { "ok" } else { "warn" }, if any { live.join(", ") } else { "no live DHCP lease".into() },
                    if any { "" } else { "Is the console connected to the bench network interface?" }));
            }
            None => out.push(check("console network", "warn", "no lease data", "")),
        }
        }
    }

    if let Some(free) = free_gb() {
        out.push(check("disk", if free > 5.0 { "ok" } else { "warn" }, format!("{free:.0} GB free"), if free > 5.0 { "" } else { "Clips and reports need space." }));
    }

    let has = |s: &str| out.iter().any(|c| c["status"] == s);
    let worst = if has("fail") { "fail" } else if has("warn") { "warn" } else { "ok" };
    let msg = format!("doctor: {worst} ({}/{} ok)", out.iter().filter(|c| c["status"] == "ok").count(), out.len());
    if worst == "fail" { b.logs.error("bench", msg) } else { b.logs.add("bench", msg) }
    json!({"overall": worst, "checks": out, "at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()})
}
