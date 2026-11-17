//! Offline console on a shared home network (design: docs/design/shared-network.md).
//!
//! The console keeps its own manual settings (gateway = its own address, DNS = this computer),
//! and the network helper answers only its DNS lookups and connection test. Here:
//! - the adapters the person can pick from,
//! - the guided setup that writes the helper's settings,
//! - the leak watch: a listed console on the network that no longer asks NS2Bench may be back on
//!   automatic settings, and so online.

use crate::server::{bad, R, S};
use axum::{extract::State, http::HeaderMap, Json};
use serde_json::{json, Value};
use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::Duration;

/// A console that hasn't asked NS2Bench for this long, while on the network, is a warning.
const QUIET_S: f64 = 600.0;
/// How often the leak watch looks.
const EVERY: Duration = Duration::from_secs(30);

/// The last leak-watch result, shown by the Network tab.
static WATCH: Mutex<Option<Value>> = Mutex::new(None);

/// The file the helper reads (the one this engine was started with, as installed by the app).
fn config_path(b: &S) -> std::path::PathBuf {
    b.cfg.path.clone().filter(|p| p.is_file()).unwrap_or_else(|| crate::config::state_dir().join("config.json"))
}

/// The helper's network settings as they are on disk now (they change without a restart).
pub fn network_settings(b: &S) -> Value {
    std::fs::read(config_path(b)).ok().and_then(|d| serde_json::from_slice::<Value>(&d).ok())
        .map(|v| v["network"].clone()).filter(|v| !v.is_null()).unwrap_or_else(|| b.cfg.raw["network"].clone())
}

/// macOS: "en10" -> "Belkin USB-C LAN" (networksetup's hardware port names).
#[cfg(unix)]
fn friendly_names() -> std::collections::HashMap<String, String> {
    if !cfg!(target_os = "macos") {
        return Default::default();
    }
    let text = std::process::Command::new("/usr/sbin/networksetup").arg("-listallhardwareports").output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    parse_hardware_ports(&text)
}

/// "Hardware Port: Belkin USB-C LAN" followed by "Device: en10" -> en10 => Belkin USB-C LAN.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_hardware_ports(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut port: Option<String> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("Hardware Port: ") {
            port = Some(p.trim().to_string());
        } else if let Some(d) = line.strip_prefix("Device: ") {
            if let Some(p) = port.take() { out.insert(d.trim().to_string(), p); }
        }
    }
    out
}

fn mask(prefix: u32) -> Ipv4Addr {
    Ipv4Addr::from(if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) })
}

/// The adapters with a private IPv4 address (the only kind a console network can use).
pub fn interfaces() -> Vec<Value> {
    #[cfg(unix)]
    {
        let names = friendly_names();
        ns2bench_netd::runtime::ipv4_interfaces().into_iter().filter(|(_, ip, p)| ip.is_private() && (8..=30).contains(p))
            .map(|(name, ip, prefix)| {
                let net = Ipv4Addr::from(u32::from(ip) & u32::from(mask(prefix)));
                json!({"name": name, "label": names.get(&name), "address": ip.to_string(), "prefix": prefix,
                    "subnet": format!("{net}/{prefix}"), "mask": mask(prefix).to_string()})
            }).collect()
    }
    #[cfg(not(unix))]
    vec![]
}

pub async fn get_interfaces() -> Json<Value> {
    let list = tokio::task::spawn_blocking(interfaces).await.unwrap_or_default();
    Json(json!({"interfaces": list, "supported": cfg!(unix),
        "forwarding_off": forwarding_off()}))
}

fn forwarding_off() -> bool {
    #[cfg(unix)]
    { ns2bench_netd::runtime::forwarding_off() }
    #[cfg(not(unix))]
    { false }
}

/// The values to type on each console.
fn console_settings(consoles: &[Ipv4Addr], dns: Ipv4Addr, prefix: u32) -> Vec<Value> {
    consoles.iter().map(|c| json!({"console": c.to_string(), "ip": c.to_string(), "mask": mask(prefix).to_string(),
        "gateway": c.to_string(), "dns": dns.to_string()})).collect()
}

/// POST /api/network/shared {"interface": "en10", "consoles": ["192.168.86.251", ...]}
/// Only the person at the page (not agents or scripts): it decides who may reach this computer.
pub async fn set_shared(State(b): State<S>, h: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    crate::settings::agents_blocked(&b, &h, &v)?;
    if !crate::server::from_browser(&h) {
        return Err(crate::server::ApiError(axum::http::StatusCode::FORBIDDEN,
            "only the person at the NS2Bench page can set up the console network".into()));
    }
    if !cfg!(unix) {
        return Err(bad("the network helper runs on macOS and Linux"));
    }
    if !b.cfg.raw["network"]["helper"].is_null() {
        return Err(bad("this NS2Bench uses a network helper on another computer: set up the shared network there"));
    }
    let name = v["interface"].as_str().unwrap_or("");
    let list = tokio::task::spawn_blocking(interfaces).await.unwrap_or_default();
    let iface = list.iter().find(|i| i["name"] == name)
        .ok_or_else(|| bad(format!("pick one of this computer's network adapters (not {name:?})")))?;
    let address: Ipv4Addr = iface["address"].as_str().and_then(|s| s.parse().ok()).ok_or_else(|| bad("adapter without an address"))?;
    let prefix = iface["prefix"].as_u64().unwrap_or(24) as u32;
    let mut consoles: Vec<Ipv4Addr> = vec![];
    for c in v["consoles"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let ip: Ipv4Addr = c.as_str().map(str::trim).and_then(|s| s.parse().ok())
            .ok_or_else(|| bad(format!("{c} is not an IPv4 address")))?;
        if !consoles.contains(&ip) { consoles.push(ip); }
    }
    if consoles.is_empty() {
        return Err(bad("add at least one console address"));
    }
    if !forwarding_off() {
        return Err(bad("IP forwarding or internet sharing is on, or can't be checked: turn it off first (it would let consoles through this computer)"));
    }
    let path = config_path(&b);
    let mut doc: Value = std::fs::read(&path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_else(|| json!({}));
    if !doc.is_object() { doc = json!({}); }
    let mut net = doc["network"].as_object().cloned().unwrap_or_default();
    for k in ["pool_start", "pool_end", "reservations", "service"] { net.remove(k); }
    net.insert("interface".into(), json!(name));
    net.insert("address".into(), json!(address.to_string()));
    net.insert("subnet".into(), iface["subnet"].clone());
    net.insert("consoles".into(), json!(consoles.iter().map(|c| c.to_string()).collect::<Vec<_>>()));
    net.insert("service".into(), json!("Shared home network (DNS and connection test only)"));
    doc["network"] = Value::Object(net);
    // The helper's own checks (subnet, consoles on it, not this computer) before anything is written.
    ns2bench_netd::Config::from_value(&doc).map_err(bad)?;
    let _lock = b.settings_lock.lock().await;
    if path.is_file() {
        let backup = path.with_extension(format!("json.bak-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        std::fs::copy(&path, &backup).map_err(|e| bad(format!("could not back up {}: {e}", path.display())))?;
    }
    crate::config::write_json(&path, &doc).map_err(|e| bad(format!("could not save {}: {e}", path.display())))?;
    let controls = json!({"topology": "shared", "dhcp": false, "dns": true, "connectivity": true});
    ns2bench_netd::validate_controls(&controls).map_err(bad)?;
    let state = crate::config::expand(doc["state_dir"].as_str().unwrap_or("~/.ns2bench"));
    crate::config::write_json(&state.join("network-controls.json"), &controls).map_err(anyhow::Error::from)?;
    b.logs.add("bench", format!("console network: shared home network on {name} ({address}), consoles {}",
        consoles.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")));
    Ok(Json(json!({"ok": true, "config": path.display().to_string(), "interface": name, "address": address.to_string(),
        "settings": console_settings(&consoles, address, prefix), "restart_helper": true})))
}

/// Is `ip` on the network right now? A ping refreshes the neighbour table (consoles may not
/// answer pings, but the address lookup before it still happens); then the table is read.
fn present(ip: Ipv4Addr) -> bool {
    let ip_s = ip.to_string();
    let quiet = |c: &mut std::process::Command| c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).stdin(std::process::Stdio::null()).status();
    if cfg!(target_os = "macos") {
        let _ = quiet(std::process::Command::new("/sbin/ping").args(["-c", "1", "-t", "1", &ip_s]));
        let out = std::process::Command::new("/usr/sbin/arp").args(["-n", &ip_s]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        out.contains(" at ") && !out.contains("incomplete")
    } else if cfg!(target_os = "linux") {
        let _ = quiet(std::process::Command::new("ping").args(["-c", "1", "-W", "1", &ip_s]));
        let out = std::process::Command::new("ip").args(["neigh", "show", &ip_s]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        out.contains("lladdr") && !out.contains("FAILED") && !out.contains("INCOMPLETE")
    } else {
        false
    }
}

/// One console's state from the helper's record and whether it's on the network.
/// protected: it uses NS2Bench. warning: on the network but not asking NS2Bench. away: not seen.
pub fn judge(rec: &Value, on_network: bool, now: f64, helper_up_s: f64) -> &'static str {
    let ago = |k: &str| rec[k].as_f64().map(|t| now - t);
    let asked = ago("last_dns").is_some_and(|a| a < QUIET_S) || ago("last_ctest").is_some_and(|a| a < QUIET_S);
    if asked {
        "protected"
    } else if on_network && helper_up_s >= QUIET_S {
        // (just after the helper starts, nobody has asked yet: no warning then)
        "warning"
    } else if on_network {
        "waiting"
    } else {
        "away"
    }
}

/// The leak watch. Runs for the life of the engine; idle unless the network is shared.
pub async fn watch(b: S) {
    let mut warned: std::collections::HashSet<String> = Default::default();
    let mut up_since: Option<f64> = None;
    loop {
        tokio::time::sleep(EVERY).await;
        let net = network_settings(&b);
        let state = crate::config::expand(
            std::fs::read(config_path(&b)).ok().and_then(|d| serde_json::from_slice::<Value>(&d).ok())
                .and_then(|v| v["state_dir"].as_str().map(str::to_string)).as_deref().unwrap_or("~/.ns2bench"));
        let status: Value = std::fs::read(state.join("network-status.json")).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or(json!({}));
        let now = crate::logs::now();
        let fresh = status["time"].as_f64().is_some_and(|t| (now - t).abs() < 5.0);
        if status["topology"] != "shared" || !fresh {
            *WATCH.lock().unwrap() = None;
            up_since = None;
            continue;
        }
        let up = *up_since.get_or_insert(now);
        let consoles: Vec<Ipv4Addr> = net["consoles"].as_array().into_iter().flatten()
            .filter_map(|c| c.as_str().and_then(|s| s.parse().ok())).collect();
        let mut rows = vec![];
        for ip in consoles {
            let on = tokio::task::spawn_blocking(move || present(ip)).await.unwrap_or(false);
            let rec = &status["consoles"][ip.to_string()];
            let state = judge(rec, on, now, now - up);
            let key = ip.to_string();
            if state == "warning" && warned.insert(key.clone()) {
                b.logs.notify("network", "error", "Console may be online",
                    format!("console {ip} is on the network but hasn't used NS2Bench for 10 minutes: it may be back on automatic settings and online. Check its gateway and DNS (Settings → Network)"));
            } else if state == "protected" && warned.remove(&key) {
                b.logs.notify("network", "info", "Console protected again", format!("console {ip} uses NS2Bench again"));
            }
            rows.push(json!({"console": key, "state": state, "on_network": on,
                "last_dns": rec["last_dns"], "last_ctest": rec["last_ctest"], "lookups": rec["lookups"], "refused": rec["refused"]}));
        }
        *WATCH.lock().unwrap() = Some(json!({"checked": now, "consoles": rows}));
    }
}

/// For GET /api/network: the leak watch's last look, and what to type on each console.
pub fn report(b: &S) -> Value {
    let net = network_settings(b);
    let consoles: Vec<Ipv4Addr> = net["consoles"].as_array().into_iter().flatten()
        .filter_map(|c| c.as_str().and_then(|s| s.parse().ok())).collect();
    let prefix = net["subnet"].as_str().and_then(|s| s.split_once('/')).and_then(|(_, p)| p.parse().ok()).unwrap_or(24);
    let dns = net["address"].as_str().and_then(|s| s.parse().ok()).unwrap_or(Ipv4Addr::UNSPECIFIED);
    json!({"interface": net["interface"], "address": net["address"], "consoles": consoles.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "settings": console_settings(&consoles, dns, prefix), "watch": WATCH.lock().unwrap().clone()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapter_names() {
        let m = parse_hardware_ports("\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: f4:d4\n\nHardware Port: Belkin USB-C LAN\nDevice: en10\nEthernet Address: 58:ef\n");
        assert_eq!(m.get("en10").map(String::as_str), Some("Belkin USB-C LAN"));
        assert_eq!(m.get("en0").map(String::as_str), Some("Wi-Fi"));
    }
    #[test]
    fn leak_watch_states() {
        let now = 10_000.0;
        let asked = json!({"last_dns": now - 30.0});
        let quiet = json!({"last_dns": now - 3600.0});
        assert_eq!(judge(&asked, true, now, 7200.0), "protected");
        assert_eq!(judge(&quiet, true, now, 7200.0), "warning", "on the network, not using NS2Bench");
        assert_eq!(judge(&json!({}), true, now, 7200.0), "warning");
        assert_eq!(judge(&json!({}), true, now, 60.0), "waiting", "helper just started");
        assert_eq!(judge(&quiet, false, now, 7200.0), "away");
    }
    #[test]
    fn console_settings_point_at_nothing_but_this_computer() {
        let s = console_settings(&["192.168.86.251".parse().unwrap()], "192.168.86.199".parse().unwrap(), 24);
        assert_eq!(s[0], json!({"console": "192.168.86.251", "ip": "192.168.86.251", "mask": "255.255.255.0",
            "gateway": "192.168.86.251", "dns": "192.168.86.199"}));
    }
}
