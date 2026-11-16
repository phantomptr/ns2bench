//! Configuration: one JSON file, every section optional.
//!
//! Search order: explicit path, $NS2BENCH_CONFIG, ./bench.json, ~/.ns2bench/config.json.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub path: Option<PathBuf>,
    pub raw: Value,
    pub server: Value,
    pub video: Value,
    pub input: Value,
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `~/.ns2bench` (created on demand).
pub fn state_dir() -> PathBuf {
    let d = home().join(".ns2bench");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(p)
    }
}

fn defaults() -> Value {
    json!({
        "server": {"host": "127.0.0.1", "port": 7630, "token": null},
        "video": {"backend": "auto", "device": null, "fps": 30, "width": 1920, "height": 1080,
                  "quality": 80, "ffmpeg_input": null, "codec": "auto"},
        "input": {"backend": "none", "host": null, "port": 7631, "token_file": "~/.ns2bench/hidd.token",
                  "serial_port": null, "baud": null}
    })
}

fn merge(base: &Value, over: Option<&Value>) -> Value {
    let mut out: Map<String, Value> = base.as_object().cloned().unwrap_or_default();
    if let Some(Value::Object(o)) = over {
        for (k, v) in o {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

pub fn find(explicit: Option<&str>) -> Option<PathBuf> {
    let env = std::env::var("NS2BENCH_CONFIG").ok();
    for cand in [explicit.map(str::to_string), env, Some("bench.json".into()), Some("~/.ns2bench/config.json".into())]
        .into_iter()
        .flatten()
    {
        let p = expand(&cand);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

impl Config {
    pub fn load(explicit: Option<&str>) -> anyhow::Result<Config> {
        let path = find(explicit);
        if let (Some(e), None) = (explicit, &path) {
            anyhow::bail!("config {e} not found");
        }
        let raw: Value = match &path {
            Some(p) => serde_json::from_str(&std::fs::read_to_string(p)?)?,
            None => json!({}),
        };
        Ok(Config::from_raw(path, raw))
    }

    pub fn from_raw(path: Option<PathBuf>, raw: Value) -> Config {
        let d = defaults();
        let mut video = merge(&d["video"], raw.get("video"));
        // settings chosen in the UI win over the config file
        if let Ok(s) = std::fs::read_to_string(state_dir().join("video.json")) {
            if let Ok(saved) = serde_json::from_str::<Value>(&s) {
                video = merge(&video, Some(&saved));
            }
        }
        let mut input = merge(&d["input"], raw.get("input"));
        if let Ok(s) = std::fs::read_to_string(state_dir().join("input.json")) {
            if let Ok(saved) = serde_json::from_str::<Value>(&s) {
                if crate::settings::validate_input(&saved).is_ok() { input = merge(&input, Some(&saved)); }
            }
        }
        let mut raw = raw;
        // Safe mode (`ns2bench-app --safe`, NS2BENCH_SAFE=1): no capture, input or sound, so a
        // device or driver that crashes NS2Bench can't stop it from opening.
        if safe_mode() {
            video["backend"] = "none".into();
            input = json!({"backend": "none"});
            if !raw.is_object() { raw = json!({}); }
            raw["audio"] = json!({"enabled": false});
        }
        Config {
            server: merge(&d["server"], raw.get("server")),
            input,
            video,
            raw,
            path,
        }
    }

    pub fn url(&self) -> String {
        let host = self.server["host"].as_str().unwrap_or("127.0.0.1");
        let host = if host == "0.0.0.0" || host == "::" { "127.0.0.1" } else { host };
        let port = self.server["port"].as_u64().unwrap_or(7630);
        if host.contains(':') { format!("http://[{host}]:{port}") } else { format!("http://{host}:{port}") }
    }

    /// The API token; an empty or whitespace token means "no token", never "match nothing".
    pub fn token(&self) -> Option<String> {
        self.server["token"].as_str().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
    }
}

pub fn read_secret(path: Option<&str>) -> Option<String> {
    let p = path?;
    std::fs::read_to_string(expand(p)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn is_loopback(host: &str) -> bool {
    host == "localhost" || host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

/// Atomic, durable replace. A unique temp name keeps concurrent writers from
/// renaming each other's half-written files.
pub fn write_json(path: &Path, v: &Value) -> std::io::Result<()> {
    use std::io::Write;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}.{n}.tmp", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(v)?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Safe mode: started with `--safe` (or NS2BENCH_SAFE=1).
pub fn safe_mode() -> bool {
    std::env::var("NS2BENCH_SAFE").is_ok_and(|v| v == "1")
}
