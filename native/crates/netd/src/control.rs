//! Optional management endpoint so a host that cannot run netd itself (Windows)
//! can read status and change the same three service controls over a private
//! management link. It never listens on the console network, requires a token,
//! and cannot change isolation.
use crate::validate_controls;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Control {
    pub listen: SocketAddrV4,
    token: String,
}

/// `network.control = {"listen": "ip:port", "token_file": "path"}`; absent = disabled.
pub fn from_config(
    data: &Value,
    cfg: &crate::Config,
    expand: impl Fn(&str) -> PathBuf,
) -> Result<Option<Control>, String> {
    let c = &data["network"]["control"];
    if c.is_null() {
        return Ok(None);
    }
    let listen: SocketAddrV4 = c["listen"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("network.control.listen must be an IPv4 address:port")?;
    let ip = *listen.ip();
    if ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() {
        return Err("network.control.listen must name one management address".into());
    }
    if cfg.contains(ip) || ip == cfg.address {
        return Err("network.control.listen must not be on the console network".into());
    }
    let path = c["token_file"]
        .as_str()
        .ok_or("network.control.token_file required")?;
    let token = fs::read_to_string(expand(path))
        .map_err(|e| format!("network.control.token_file: {e}"))?
        .trim()
        .to_string();
    if token.len() < 16 {
        return Err("network.control token must be at least 16 characters".into());
    }
    Ok(Some(Control { listen, token }))
}

impl Control {
    pub fn bind(&self) -> Result<TcpListener, String> {
        let l = TcpListener::bind(self.listen).map_err(|e| format!("control {}: {e}", self.listen))?;
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(l)
    }
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

pub struct Request {
    pub method: String,
    pub path: String,
    pub token: Option<String>,
    pub body: Vec<u8>,
}

/// Bounded HTTP/1.1 request parse: 8 KiB of headers, 4 KiB body.
pub fn parse(raw: &[u8]) -> Option<Result<Request, &'static str>> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..end]).ok();
    let Some(head) = head else { return Some(Err("bad request")) };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let (mut token, mut length) = (None, 0usize);
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "authorization" => token = v.trim().strip_prefix("Bearer ").map(|t| t.trim().to_string()),
            "content-length" => match v.trim().parse() {
                Ok(n) if n <= 4096 => length = n,
                _ => return Some(Err("body too large")),
            },
            _ => {}
        }
    }
    let body = &raw[end + 4..];
    if body.len() < length {
        return None;
    }
    Some(Ok(Request { method, path, token, body: body[..length].to_vec() }))
}

fn read_json(path: &Path, fallback: Value) -> Value {
    fs::read(path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or(fallback)
}

pub fn write_controls(state: &Path, v: &Value) -> io::Result<()> {
    let tmp = state.join("network-controls.tmp");
    let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(v.to_string().as_bytes())?;
    f.sync_all()?;
    fs::rename(tmp, state.join("network-controls.json"))
}

/// Route one authenticated request. Returns (status, json body).
pub fn respond(req: &Request, token: &str, state: &Path, status: Value, now: f64) -> (u16, Value) {
    if !req.token.as_deref().is_some_and(|t| same(t.as_bytes(), token.as_bytes())) {
        return (401, json!({"error":"unauthorized"}));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/status") => {
            let age = status["time"].as_f64().map(|t| now - t);
            (200, json!({
                "daemon": status, "age_s": age,
                "controls": read_json(&state.join("network-controls.json"),
                    json!({"dhcp":true,"dns":true,"connectivity":true,"topology":"direct"})),
                "leases": read_json(&state.join("leases.json"), json!({})),
            }))
        }
        ("POST", "/controls") => {
            let v: Value = match serde_json::from_slice(&req.body) {
                Ok(v) => v,
                Err(_) => return (400, json!({"error":"invalid JSON"})),
            };
            if let Err(e) = validate_controls(&v) {
                return (400, json!({"error": e}));
            }
            match write_controls(state, &v) {
                Ok(()) => (200, json!({"ok": true, "controls": v})),
                Err(e) => (500, json!({"error": e.to_string()})),
            }
        }
        _ => (404, json!({"error":"not found"})),
    }
}

/// Serve one management connection with a short deadline.
pub fn serve(mut stream: TcpStream, peer: SocketAddr, control: &Control, state: &Path,
    status: Value, now: f64) -> io::Result<(String, u16)> {
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut raw = Vec::new();
    let parsed = loop {
        if let Some(p) = parse(&raw) {
            break p;
        }
        if raw.len() > 8192 + 4096 {
            break Err("request too large");
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok((peer.to_string(), 408));
        }
        stream.set_read_timeout(Some(remaining))?;
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok((peer.to_string(), 400));
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let (code, body) = match parsed {
        Ok(req) => respond(&req, &control.token, state, status, now),
        Err(e) => (400, json!({"error": e})),
    };
    let body = body.to_string();
    let reason = match code { 200 => "OK", 401 => "Unauthorized", 404 => "Not Found", 408 => "Timeout", 500 => "Error", _ => "Bad Request" };
    write!(stream, "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())?;
    Ok((peer.to_string(), code))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn req(raw: &str) -> Request {
        parse(raw.as_bytes()).unwrap().unwrap()
    }
    #[test]
    fn requires_token_and_validates_controls() {
        let dir = std::env::temp_dir().join(format!("ns2bench-netd-control-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let token = "0123456789abcdef";
        let status = json!({"time": 100.0});
        let r = req("GET /status HTTP/1.1\r\n\r\n");
        assert_eq!(respond(&r, token, &dir, status.clone(), 101.0).0, 401);
        let r = req("GET /status HTTP/1.1\r\nAuthorization: Bearer 0123456789abcdeX\r\n\r\n");
        assert_eq!(respond(&r, token, &dir, status.clone(), 101.0).0, 401);
        let r = req("GET /status HTTP/1.1\r\nAuthorization: Bearer 0123456789abcdef\r\n\r\n");
        let (code, body) = respond(&r, token, &dir, status.clone(), 101.5);
        assert_eq!((code, body["age_s"].as_f64()), (200, Some(1.5)));
        let bad = r#"{"topology":"router","dhcp":true,"dns":false,"connectivity":false}"#;
        let r = req(&format!("POST /controls HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{bad}", bad.len()));
        assert_eq!(respond(&r, token, &dir, status.clone(), 0.0).0, 400);
        let blocking = r#"{"topology":"direct","dhcp":true,"dns":true,"connectivity":true,"blocking":false}"#;
        let r = req(&format!("POST /controls HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{blocking}", blocking.len()));
        assert_eq!(respond(&r, token, &dir, status.clone(), 0.0).0, 400);
        let good = r#"{"topology":"direct","dhcp":false,"dns":true,"connectivity":true}"#;
        let r = req(&format!("POST /controls HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{good}", good.len()));
        assert_eq!(respond(&r, token, &dir, status, 0.0).0, 200);
        let saved: Value = serde_json::from_slice(&fs::read(dir.join("network-controls.json")).unwrap()).unwrap();
        assert_eq!(crate::runtime::service_controls(&saved), [false, true, true]);
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn serves_over_tcp() {
        let dir = std::env::temp_dir().join(format!("ns2bench-netd-serve-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let c = Control { listen: "127.0.0.1:1".parse().unwrap(), token: "0123456789abcdef".into() };
        let client = std::thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            // headers and body split across writes
            s.write_all(b"GET /status HTTP/1.1\r\nAuthori").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            s.write_all(b"zation: Bearer 0123456789abcdef\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        });
        let (stream, peer) = l.accept().unwrap();
        let (_, code) = serve(stream, peer, &c, &dir, json!({"time": 5.0}), 6.0).unwrap();
        let out = client.join().unwrap();
        assert_eq!(code, 200);
        assert!(out.starts_with("HTTP/1.1 200 OK") && out.contains(r#""age_s":1.0"#), "{out}");
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn bounded_parse() {
        assert!(parse(b"GET / HTTP/1.1\r\n").is_none());
        assert!(matches!(parse(b"POST / HTTP/1.1\r\nContent-Length: 99999\r\n\r\n"), Some(Err(_))));
        assert!(parse(b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\nab").is_none());
    }
    #[test]
    fn refuses_console_side_or_wildcard_listen() {
        let fixture: Value = serde_json::from_str(include_str!("../tests/fixtures/reference.json")).unwrap();
        let mut data = json!({"network": fixture["config"]["network"].clone()});
        let cfg = crate::Config::from_value(&data).unwrap();
        let dir = std::env::temp_dir().join(format!("ns2bench-netd-token-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("t"), "0123456789abcdef\n").unwrap();
        let tok = dir.join("t").to_string_lossy().into_owned();
        let expand = |p: &str| PathBuf::from(p);
        for listen in ["0.0.0.0:7632", &format!("{}:7632", cfg.address)] {
            data["network"]["control"] = json!({"listen": listen, "token_file": tok});
            assert!(from_config(&data, &cfg, expand).is_err(), "{listen}");
        }
        data["network"]["control"] = json!({"listen": "10.255.0.1:7632", "token_file": tok});
        assert!(from_config(&data, &cfg, expand).unwrap().is_some());
        fs::write(dir.join("t"), "short").unwrap();
        assert!(from_config(&data, &cfg, expand).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
