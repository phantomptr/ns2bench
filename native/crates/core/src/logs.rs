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

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub seq: u64,
    pub t: f64,
    pub source: String,
    pub level: String,
    pub text: String,
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

struct Inner {
    seq: u64,
    entries: VecDeque<Entry>,
    counts: HashMap<String, u64>,
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
            inner: Arc::new(Mutex::new(Inner { seq: 0, entries: VecDeque::new(), counts: HashMap::new() })),
            tx,
            files,
        }
    }

    pub fn add(&self, source: &str, text: impl Into<String>) {
        self.add_full(source, text, None, "info")
    }

    pub fn error(&self, source: &str, text: impl Into<String>) {
        self.add_full(source, text, None, "error")
    }

    pub fn add_full(&self, source: &str, text: impl Into<String>, t: Option<f64>, level: &str) {
        let mut text: String = text.into();
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
            let e = Entry { seq: g.seq, t: t.unwrap_or_else(now), source: source.into(), level: level.into(), text };
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
        let g = self.inner.lock().unwrap();
        let contains = contains.map(|c| c.to_lowercase());
        let v: Vec<Entry> = g
            .entries
            .iter()
            .filter(|e| e.seq > after)
            .filter(|e| source.is_none_or(|s| e.source == s))
            .filter(|e| contains.as_ref().is_none_or(|c| e.text.to_lowercase().contains(c)))
            .cloned()
            .collect();
        let n = limit.clamp(1, 2000);
        v[v.len().saturating_sub(n)..].to_vec()
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
                for l in &lines[lines.len().saturating_sub(30)..] {
                    self.add(name, pretty(l));
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
                    if let Ok(data) = std::fs::read(path) {
                        let chunk = &data[off as usize..data.len().min(off as usize + (1 << 20))];
                        for l in String::from_utf8_lossy(chunk).lines() {
                            self.add(name, pretty(l));
                        }
                        off += chunk.len() as u64;
                    }
                }
                pos.insert(name.clone(), off);
            }
        }
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
