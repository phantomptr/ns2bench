//! What the network helper saw of each console (its JSON log lines): which kind of console it is,
//! from the names it looks up, and when it last ran its connection test (a console that just woke
//! runs one within seconds). Read only: nothing here is sent anywhere.
//!
//! Names seen on this bench (2026-11-17): a Switch 2 looks up `*.p01.*` names
//! (ctest.p01.ctest.srv, dauth.p01.lp1.ndas.srv, beach.p01.lp1.eshop); a Switch 1 looks up
//! `*.hac.*` names (val.hac.lp1.penne, sun.hac.lp1.d4c) and ctest.cdn.nintendo.net.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default, Clone, Copy)]
struct Peer {
    switch1: u32,
    switch2: u32,
    last_test: Option<Instant>,
}

static PEERS: Mutex<Option<HashMap<String, Peer>>> = Mutex::new(None);

/// The kind of console a looked-up name belongs to, if it tells.
pub fn model_of_name(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.contains(".p01.") {
        Some("switch2")
    } else if n.contains(".hac.") || n == "ctest.cdn.nintendo.net" {
        Some("switch1")
    } else {
        None
    }
}

/// One line of the network helper's log, as it is read (`fresh`: read as it happened, not from
/// the history at start, so a connection test counts as "just now").
pub fn observe(line: &str, fresh: bool) {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return };
    let (Some(ev), Some(peer)) = (v["event"].as_str(), v["peer"].as_str()) else { return };
    let mut g = PEERS.lock().unwrap();
    let p = g.get_or_insert_with(HashMap::new).entry(peer.to_string()).or_default();
    match ev {
        "dns" => match v["name"].as_str().and_then(model_of_name) {
            Some("switch2") => p.switch2 += 1,
            Some(_) => p.switch1 += 1,
            None => {}
        },
        "ctest" if fresh => p.last_test = Some(Instant::now()),
        _ => {}
    }
}

/// The kind of console at this address, when its lookups clearly say so (at least 3, 90% one kind).
pub fn model_at(ip: &str) -> Option<&'static str> {
    let p = *PEERS.lock().unwrap().as_ref()?.get(ip)?;
    let total = p.switch1 + p.switch2;
    if total < 3 {
        return None;
    }
    if p.switch2 * 10 >= total * 9 {
        Some("switch2")
    } else if p.switch1 * 10 >= total * 9 {
        Some("switch1")
    } else {
        None
    }
}

/// Did the console at this address run its connection test within `within`?
pub fn tested_within(ip: &str, within: Duration) -> bool {
    PEERS.lock().unwrap().as_ref().and_then(|m| m.get(ip)).and_then(|p| p.last_test).is_some_and(|t| t.elapsed() <= within)
}

#[cfg(test)]
pub fn reset() {
    *PEERS.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_tests() {
        reset();
        assert_eq!(model_of_name("ctest.p01.ctest.srv.nintendo.net"), Some("switch2"));
        assert_eq!(model_of_name("val.hac.lp1.penne.srv.nintendo.net"), Some("switch1"));
        assert_eq!(model_of_name("ctest.cdn.nintendo.net"), Some("switch1"));
        assert_eq!(model_of_name("bcat-list-lp1.cdn.nintendo.net"), None);
        for n in ["ctest.p01.ctest.srv.nintendo.net", "dauth.p01.lp1.ndas.srv.nintendo.net", "beach.p01.lp1.eshop.nintendo.net"] {
            observe(&format!(r#"{{"event":"dns","name":"{n}","peer":"10.0.0.2"}}"#), false);
        }
        assert_eq!(model_at("10.0.0.2"), Some("switch2"));
        assert_eq!(model_at("10.0.0.3"), None);
        // history at start doesn't count as "just tested"
        observe(r#"{"event":"ctest","ok":true,"peer":"10.0.0.2"}"#, false);
        assert!(!tested_within("10.0.0.2", Duration::from_secs(60)));
        observe(r#"{"event":"ctest","ok":true,"peer":"10.0.0.2"}"#, true);
        assert!(tested_within("10.0.0.2", Duration::from_secs(60)));
    }
}
