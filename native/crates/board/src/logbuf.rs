//! In-memory log ring the host fetches with {"logs": after_seq}; also printed to stdout.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

struct Ring {
    seq: u64,
    entries: VecDeque<(u64, f64, String, String)>,
}

static RING: Mutex<Ring> = Mutex::new(Ring { seq: 0, entries: VecDeque::new() });

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn clock() -> String {
    let s = now() as i64;
    format!("{:02}:{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60, s % 60)
}

pub fn remember(source: &str, text: String) {
    let mut r = RING.lock().unwrap();
    r.seq += 1;
    let seq = r.seq;
    r.entries.push_back((seq, (now() * 1000.0).round() / 1000.0, source.into(), text));
    while r.entries.len() > 500 {
        r.entries.pop_front();
    }
}

/// A daemon message: stdout (UTC clock, like the board's own log files) and the ring.
pub fn log(text: impl Into<String>) {
    let text = text.into();
    println!("{} {text}", clock());
    remember("hidd", text);
}

pub fn seq() -> u64 {
    RING.lock().unwrap().seq
}

pub fn after(seq: u64) -> serde_json::Value {
    let r = RING.lock().unwrap();
    let v: Vec<_> = r.entries.iter().filter(|e| e.0 > seq).map(|e| serde_json::json!([e.0, e.1, e.2, e.3])).collect();
    let skip = v.len().saturating_sub(200);
    serde_json::Value::Array(v.into_iter().skip(skip).collect())
}
