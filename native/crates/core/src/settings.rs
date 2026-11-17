//! Configuration and integration information shared by the browser and local API.
use crate::config::{state_dir, write_json};
use crate::server::{bad, R, S};
use axum::{extract::State, Json};
use serde_json::{json, Value};

pub fn validate_input(v: &Value) -> anyhow::Result<()> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("input must be an object"))?;
    let allowed = [
        "backend",
        "host",
        "port",
        "serial_port",
        "baud",
        "token_file",
    ];
    anyhow::ensure!(
        obj.keys().all(|k| allowed.contains(&k.as_str())),
        "unknown input setting"
    );
    let kind = v["backend"].as_str().unwrap_or("");
    anyhow::ensure!(
        ["none", "hidd", "serial", "ch9329"].contains(&kind),
        "unsupported input transport"
    );
    if kind == "hidd" {
        anyhow::ensure!(
            v["host"]
                .as_str()
                .is_some_and(|h| !h.trim().is_empty() && h.len() < 254),
            "Bridge host required"
        );
        anyhow::ensure!(
            v["port"].as_u64().is_some_and(|p| (1..=65535).contains(&p)),
            "Bridge port must be 1-65535"
        );
    }
    if let Some(t) = v.get("token_file").filter(|t| !t.is_null()) {
        // The token is sent to the Bridge host: only files in the NS2Bench state
        // directory may be used, never arbitrary files such as SSH keys.
        let t = t.as_str().ok_or_else(|| anyhow::anyhow!("token_file must be a path"))?;
        // Only *.token files directly in ~/.ns2bench (or ~/.ns2bench/tokens), and no
        // links: never the config, the signing password or another secret.
        let p = crate::config::expand(t);
        let dir = p.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let named = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".token") && !n.starts_with('.'));
        let linked = std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink());
        anyhow::ensure!(
            named && !linked && (dir == state_dir() || dir == state_dir().join("tokens"))
                && p.components().all(|c| !matches!(c, std::path::Component::ParentDir)),
            "token_file must be a .token file in ~/.ns2bench (for example ~/.ns2bench/hidd.token)"
        );
    }
    if kind == "serial" || kind == "ch9329" {
        anyhow::ensure!(
            v["serial_port"].as_str().is_some_and(serial_device),
            "serial port must be a device such as /dev/ttyACM0, /dev/cu.usbmodem1 or COM3"
        );
        anyhow::ensure!(
            v["baud"]
                .as_u64()
                .is_some_and(
                    |b| [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600].contains(&b)
                ),
            "unsupported baud rate"
        );
    }
    Ok(())
}

/// A serial device name, never an ordinary file (the port is opened read-write).
pub fn serial_device(s: &str) -> bool {
    let unix = s.strip_prefix("/dev/").is_some_and(|rest| {
        !rest.is_empty() && !rest.contains("..")
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
    });
    let com = s.trim_start_matches(r"\\.\");
    let windows = com.len() > 3 && com.is_ascii() && com[..3].eq_ignore_ascii_case("COM") && com[3..].chars().all(|c| c.is_ascii_digit());
    unix || windows
}

pub async fn integrations(State(b): State<S>) -> Json<Value> {
    let exe = std::env::current_exe().ok();
    let mut server = json!({"command": exe, "args": ["mcp"]});
    if let Some(path) = &b.cfg.path {
        server["env"] =
            json!({"NS2BENCH_CONFIG": std::fs::canonicalize(path).unwrap_or(path.clone())});
    }
    Json(
        json!({"api_url": b.cfg.url(), "mcp_url": format!("{}/mcp", b.cfg.url()), "token_required": b.cfg.token().is_some(),
        "mcp": {"mcpServers": {"ns2bench": server}}, "tools": crate::mcp::tools()}),
    )
}

pub async fn get_input(State(b): State<S>) -> Json<Value> {
    Json(json!({"settings": *b.input_cfg.lock().unwrap(), "status": b.hub.status()}))
}
/// While Block agents is on, only the person at the NS2Bench page may change things.
pub fn agents_blocked(b: &S, h: &axum::http::HeaderMap, v: &Value) -> R<()> {
    if crate::server::source_of(h, v) != "ui" && b.hub.manual_only.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(crate::server::ApiError(axum::http::StatusCode::CONFLICT,
            "manual-only mode is on: only the person at the NS2Bench page can change this".into()));
    }
    Ok(())
}

pub async fn set_input(State(b): State<S>, h: axum::http::HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    agents_blocked(&b, &h, &v)?;
    let mut v = v;
    if let Some(o) = v.as_object_mut() { o.remove("source"); }
    validate_input(&v)?;
    // The Bridge token is sent to the configured host: only the person at the page may point
    // it somewhere new, or an agent could have it sent to a listener of its own.
    // (also without a token_file: the engine would still connect to that address on its own)
    if !crate::server::from_browser(&h) {
        let cur = b.input_cfg.lock().unwrap().clone();
        if ["backend", "host", "port", "serial_port", "token_file"].iter().any(|k| v[*k] != cur[*k]) {
            return Err(crate::server::ApiError(axum::http::StatusCode::FORBIDDEN,
                "only the person at the NS2Bench page can change the Bridge address or its token file (Settings → Input)".into()));
        }
    }
    let _lock = b.settings_lock.lock().await;
    let _agent = b.hub.configuration_guard()?;
    // Persist before touching the working connection; a write failure keeps it running.
    write_json(&state_dir().join("input.json"), &v).map_err(anyhow::Error::from)?;
    b.hub.release_all();
    b.hub.backend.replace(&v, b.logs.clone()).await;
    let what = match v["backend"].as_str().unwrap_or("none") {
        "hidd" => format!("NS2Bench Bridge at {}:{}", v["host"].as_str().unwrap_or("?"), v["port"]),
        "serial" => format!("Pico on {}", v["serial_port"].as_str().unwrap_or("?")),
        "ch9329" => format!("CH9329 on {}", v["serial_port"].as_str().unwrap_or("?")),
        other => other.to_string(),
    };
    *b.input_cfg.lock().unwrap() = v;
    b.logs.add("bench", format!("input device set to: {what}"));
    Ok(Json(json!({"ok": true})))
}

/// A remote Linux helper (`network.helper = {"url","token_file"}`) for hosts that
/// cannot run netd themselves (Windows). Absent = the local helper's state files.
fn helper(b: &S) -> Option<crate::client::Client> {
    let h = &b.cfg.raw["network"]["helper"];
    let url = h["url"].as_str()?.trim_end_matches('/').to_string();
    Some(crate::client::Client {
        base: url,
        token: crate::config::read_secret(h["token_file"].as_str()),
        source: "engine",
    })
}
async fn helper_call(c: &crate::client::Client, method: &str, path: &str, body: Option<&Value>)
    -> anyhow::Result<Value> {
    let r = tokio::time::timeout(std::time::Duration::from_secs(4), c.request(method, path, body))
        .await
        .map_err(|_| anyhow::anyhow!("network helper at {} did not answer", c.base))?
        .map_err(|_| anyhow::anyhow!("network helper at {} is unreachable", c.base))?;
    let v = r.json().unwrap_or(Value::Null);
    if r.status >= 400 {
        anyhow::bail!("network helper: {}", v["error"].as_str().unwrap_or("request failed"));
    }
    Ok(v)
}

pub async fn get_network(State(b): State<S>) -> Json<Value> {
    let blocking = "Always enforced: no upstream DNS, forwarding, NAT or proxy";
    if let Some(c) = helper(&b) {
        // The helper reports the status age itself: its clock may differ from ours.
        return Json(match helper_call(&c, "GET", "/status", None).await {
            Ok(v) => json!({"settings": b.cfg.raw["network"], "controls": v["controls"],
                "daemon": v["daemon"], "leases": v["leases"], "helper": c.base, "blocking": blocking,
                "online": v["age_s"].as_f64().is_some_and(|s| (0.0..5.0).contains(&s))}),
            Err(e) => json!({"settings": b.cfg.raw["network"], "controls": {}, "daemon": {},
                "leases": {}, "helper": c.base, "online": false, "error": e.to_string(),
                "blocking": blocking}),
        });
    }
    // the helper's own folder (its config's state_dir), the one it reads and writes
    let dir = crate::shared_net::helper_state_dir(&b);
    let status: Value = std::fs::read(dir.join("network-status.json"))
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or(json!({}));
    let age = status["time"].as_f64().map(|t| crate::logs::now() - t);
    let controls = read_network_controls(&dir);
    let settings = crate::shared_net::network_settings(&b);
    let leases: Value = std::fs::read(dir.join("leases.json"))
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or(json!({}));
    Json(
        json!({"settings": settings, "controls": controls,
        "daemon": status, "online": age.is_some_and(|s| (0.0..5.0).contains(&s)),
        "leases": leases, "blocking": blocking, "shared": crate::shared_net::report(&b)}),
    )
}
pub fn read_network_controls(dir: &std::path::Path) -> Value {
    std::fs::read(dir.join("network-controls.json"))
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or(json!({"dhcp":true,"dns":true,"connectivity":true,"topology":"direct"}))
}
pub async fn set_network(State(b): State<S>, h: axum::http::HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    agents_blocked(&b, &h, &v)?;
    let mut v = v;
    if let Some(o) = v.as_object_mut() { o.remove("source"); }
    ns2bench_netd::validate_controls(&v).map_err(bad)?;
    // the settings say this is a shared home network: the router hands out addresses there
    if crate::shared_net::is_shared(&b) && (v["dhcp"] == true || v["topology"] != "shared") {
        return Err(bad("this computer's network helper is set up for a shared home network: use the shared setup, or change it in Settings → Network first (DHCP stays off on a home network)"));
    }
    if let Some(c) = helper(&b) {
        helper_call(&c, "POST", "/controls", Some(&v)).await.map_err(|e| bad(e.to_string()))?;
    } else {
        write_json(&crate::shared_net::helper_state_dir(&b).join("network-controls.json"), &v).map_err(anyhow::Error::from)?;
    }
    let onoff = |k: &str| if v[k].as_bool().unwrap_or(false) { "on" } else { "off" };
    b.logs.add("bench", format!("console network: {} · DHCP {} · DNS {} · connectivity page {}",
        v["topology"].as_str().unwrap_or("direct"), onoff("dhcp"), onoff("dns"), onoff("connectivity")));
    Ok(Json(json!({"ok":true,"controls":v})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_profiles_are_validated() {
        assert!(validate_input(
            &json!({"backend":"serial","serial_port":"/dev/ttyUSB0","baud":921600})
        )
        .is_ok());
        assert!(
            validate_input(&json!({"backend":"hidd","host":"192.168.1.3","port":7631})).is_ok()
        );
        assert!(validate_input(&json!({"backend":"hidd","host":"","port":7631})).is_err());
        assert!(validate_input(&json!({"backend":"hidd","host":"board","port":65536})).is_err());
        assert!(validate_input(&json!({"backend":"serial","serial_port":"-x","baud":1})).is_err());
        assert!(validate_input(&json!({"backend":"none","shell":"x"})).is_err());
        assert!(validate_input(&json!({"backend":"ch9329","serial_port":"~/.zshrc","baud":9600})).is_err());
        assert!(validate_input(&json!({"backend":"serial","serial_port":"/dev/../etc/passwd","baud":9600})).is_err());
        assert!(validate_input(&json!({"backend":"serial","serial_port":"COM3","baud":9600})).is_ok());
        assert!(validate_input(&json!({"backend":"hidd","host":"evil","port":80,"token_file":"~/.ssh/id_ed25519"})).is_err());
        assert!(validate_input(&json!({"backend":"hidd","host":"b","port":7631,"token_file":"~/.ns2bench/../.ssh/id"})).is_err());
        assert!(validate_input(&json!({"backend":"hidd","host":"b","port":7631,"token_file":"~/.ns2bench/config.json"})).is_err());
        assert!(validate_input(&json!({"backend":"hidd","host":"b","port":7631,"token_file":"~/.ns2bench/signing/password"})).is_err());
        assert!(validate_input(&json!({"backend":"hidd","host":"b","port":7631,"token_file":"~/.ns2bench/hidd.token"})).is_ok());
        assert!(validate_input(&json!({"backend":"hidd","host":"b","port":7631,"token_file":"~/.ns2bench/tokens/lab.token"})).is_ok());
    }
    #[tokio::test]
    async fn cannot_turn_off_isolation_or_run_dhcp_on_shared_lan() {
        let b = crate::Bench::new(crate::Config::from_raw(None, json!({"input":{"backend":"none"},"video":{"backend":"none"}})));
        assert!(set_network(State(b.clone()), axum::http::HeaderMap::new(), Json(json!({"blocking":false}))).await.is_err());
        assert!(set_network(State(b), axum::http::HeaderMap::new(), Json(
            json!({"topology":"router","dhcp":true,"dns":false,"connectivity":false})
        ))
        .await
        .is_err());
    }
    #[tokio::test]
    async fn remote_helper_is_proxied_with_its_token() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut seen = vec![];
            for _ in 0..2 {
                let (mut s, _) = l.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if req.starts_with("GET /status") {
                    r#"{"age_s":1.0,"daemon":{"dhcp":true},"controls":{"topology":"direct"},"leases":{}}"#
                } else { r#"{"ok":true}"# };
                s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                seen.push(req);
            }
            seen
        });
        let dir = std::env::temp_dir().join(format!("ns2bench-helper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tok"), "helper-secret-123\n").unwrap();
        let b = crate::Bench::new(crate::Config::from_raw(None, json!({"input":{"backend":"none"},"video":{"backend":"none"},
            "network":{"helper":{"url":format!("http://{addr}"),"token_file":dir.join("tok").to_string_lossy()}}})));
        let got = get_network(State(b.clone())).await.0;
        assert_eq!((got["online"].as_bool(), got["daemon"]["dhcp"].as_bool()), (Some(true), Some(true)));
        let v = json!({"topology":"direct","dhcp":false,"dns":true,"connectivity":true});
        assert!(set_network(State(b), axum::http::HeaderMap::new(), Json(v)).await.is_ok());
        let seen = server.await.unwrap();
        assert!(seen.iter().all(|r| r.contains("Authorization: Bearer helper-secret-123")));
        assert!(seen[1].starts_with("POST /controls") && seen[1].contains(r#""dhcp":false"#));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
