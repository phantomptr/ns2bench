//! Run a test file against the live console and write a report.
//! Steps: everything `sequence` takes, plus navigate, ensure, expect_screen, expect_text,
//! wait_text, expect_gone, screenshot, probe, note (see the README).
//! Reports go to ~/.ns2bench/reports/<time>-<name>/: report.json, report.md, screenshots, logs.txt,
//! and on failure a screenshot, the screen text and a clip of the moments before.

use crate::bench::Bench;
use crate::ocr::{self, Line};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SEQUENCE_KEYS: &[&str] = &["press", "hold", "release", "stick", "key", "type", "mouse", "click", "wait_ms"];

pub fn reports_dir() -> PathBuf {
    crate::config::state_dir().join("reports")
}

pub fn slug(text: &str) -> String {
    let re = regex::Regex::new("[^a-z0-9]+").unwrap();
    let s: String = re.replace_all(&text.to_lowercase(), "-").trim_matches('-').chars().take(40).collect();
    if s.is_empty() { "test".into() } else { s }
}

async fn wait_for(b: &Bench, timeout_ms: u64, check: impl Fn(&[Line]) -> (bool, String)) -> Result<(bool, String)> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(300_000));
    loop {
        let lines = b.read_screen(None, false).await?;
        let (ok, detail) = check(&lines);
        if ok || Instant::now() > deadline {
            return Ok((ok, detail));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

async fn run_step(b: &Bench, step: &Value, dir: &Path, shots: &mut Vec<String>, source: &str) -> Result<(bool, String)> {
    let t = step["timeout_ms"].as_u64();
    let s = |k: &str| step[k].as_str().map(str::to_string);
    if let Some(r) = s("navigate") {
        let r = b.navigate(&r, source).await?;
        let screens: Vec<String> = crate::server::strs(&r["screens"]);
        return Ok((r["arrived"].as_bool() == Some(true), format!("screens: {}", if screens.is_empty() { "unknown".into() } else { screens.join(", ") })));
    }
    if let Some(sc) = s("ensure") {
        b.ensure(&sc, source).await?;
        return Ok((true, "ok".into()));
    }
    if let Some(want) = s("expect_screen") {
        return wait_for(b, t.unwrap_or(4000), |ls| {
            let c = b.classify(ls);
            (c.contains(&want), format!("screens: {}", if c.is_empty() { "unknown".into() } else { c.join(", ") }))
        }).await;
    }
    if let Some(text) = s("expect_text").or_else(|| s("wait_text")) {
        let re = step["regex"].as_bool().unwrap_or(false);
        let default = if step.get("expect_text").is_some() { 4000 } else { 15000 };
        let (ok, _) = wait_for(b, t.unwrap_or(default), |ls| {
            let hit = ocr::matches(ls, &text, re).is_some();
            (hit, String::new())
        }).await?;
        return Ok((ok, format!("{text:?}: {}", if ok { "found" } else { "not found" })));
    }
    if let Some(text) = s("expect_gone") {
        let (ok, _) = wait_for(b, t.unwrap_or(4000), |ls| (ocr::matches(ls, &text, false).is_none(), String::new())).await?;
        return Ok((ok, if ok { format!("{text:?} gone") } else { format!("{text:?}: still visible") }));
    }
    if step.get("screenshot").is_some() {
        let Some(f) = b.frames.latest() else { return Ok((false, "no video".into())) };
        let label = step["screenshot"].as_str().map(str::to_string).unwrap_or_else(|| step["screenshot"].to_string());
        let name = format!("{:02}-{}.jpg", shots.len() + 1, slug(&label));
        std::fs::write(dir.join(&name), &f.jpeg)?;
        shots.push(name.clone());
        return Ok((true, name));
    }
    if let Some(p) = step.get("probe") {
        let buttons = match p {
            Value::Array(_) => crate::server::strs(p),
            Value::String(x) => vec![x.clone()],
            _ => bail!("probe takes a button or a list"),
        };
        let r = crate::calib::probe(b, &buttons, step["hold_ms"].as_u64().unwrap_or(100), 1500, source).await?;
        let ok = r["changed"].as_bool() == Some(true);
        return Ok((ok, if ok { format!("latency {} ms", r["latency_ms"]) } else { "no visible change".into() }));
    }
    if let Some(n) = step.get("note") {
        return Ok((true, n.as_str().map(str::to_string).unwrap_or_else(|| n.to_string())));
    }
    if SEQUENCE_KEYS.iter().any(|k| step.get(*k).is_some()) {
        crate::server::run_sequence(b, &json!([step]), source).await?;
        return Ok((true, "ok".into()));
    }
    bail!("unknown step: {step}")
}

pub async fn run_test(b: &Bench, test: &Value, source: &str, stop_on_fail: bool) -> Result<Value> {
    let name = test["name"].as_str().unwrap_or("test").to_string();
    let steps = test["steps"].as_array().filter(|s| !s.is_empty()).cloned()
        .ok_or_else(|| anyhow::anyhow!("a test needs a non-empty \"steps\" list"))?;
    let dir = reports_dir().join(format!("{}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"), slug(&name)));
    std::fs::create_dir_all(&dir)?;
    let log_from = b.logs.seq();
    b.logs.add("bench", format!("test started: {name} ({} steps)", steps.len()));
    let (mut shots, mut results, t0) = (vec![], vec![], Instant::now());
    let mut passed = true;
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        let s0 = Instant::now();
        let (ok, detail) = match run_step(b, step, &dir, &mut shots, source).await {
            Ok(r) => r,
            Err(e) => (false, format!("error: {e}")),
        };
        results.push(json!({"n": n, "step": step, "ok": ok, "detail": detail, "ms": s0.elapsed().as_millis() as u64}));
        if !ok {
            passed = false;
            if let Some(f) = b.frames.latest() {
                let fname = format!("{:02}-failed-step-{n}.jpg", shots.len() + 1);
                let _ = std::fs::write(dir.join(&fname), &f.jpeg);
                shots.push(fname);
            }
            if let Ok((clip, _)) = b.ext.dvr.save(Some(20.0), Some(dir.join(format!("failed-step-{n}.mp4"))), "failure", Some(&b.ext.audio)).await {
                shots.push(clip.file_name().unwrap().to_string_lossy().into());
            }
            if let Ok(lines) = b.read_screen(None, false).await {
                let _ = std::fs::write(dir.join(format!("failed-step-{n}-screen.txt")), ocr::join(&lines, "\n"));
            }
            if stop_on_fail && step["optional"].as_bool() != Some(true) {
                break;
            }
        }
    }
    let logs = b.logs.query(None, log_from, 2000, None);
    let fmt_t = |t: f64| chrono::DateTime::from_timestamp(t as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()).unwrap_or_default();
    std::fs::write(dir.join("logs.txt"), logs.iter().map(|e| format!("{} [{}] {}", fmt_t(e.t), e.source, e.text)).collect::<Vec<_>>().join("\n"))?;
    let duration = (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    let at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let report = json!({"name": name, "passed": passed, "duration_s": duration, "steps": results, "screenshots": shots,
                        "dir": dir.to_string_lossy(), "at": at});
    std::fs::write(dir.join("report.json"), serde_json::to_string_pretty(&report)?)?;
    let mut md = vec![format!("# {name}: {}", if passed { "PASS" } else { "FAIL" }), String::new(),
        format!("{at} · {duration} s · {}/{} steps run", results.len(), steps.len()), String::new(),
        "| # | step | result | detail | ms |".into(), "|---|---|---|---|---|".into()];
    for r in &results {
        let st: String = r["step"].to_string().chars().take(70).collect();
        md.push(format!("| {} | `{st}` | {} | {} | {} |", r["n"], if r["ok"] == true { "ok" } else { "**FAIL**" },
            r["detail"].as_str().unwrap_or(""), r["ms"]));
    }
    md.push(String::new());
    for s in &shots {
        md.push(if s.ends_with(".jpg") { format!("![{s}]({s})") } else { format!("[{s}]({s})") });
    }
    std::fs::write(dir.join("report.md"), md.join("\n") + "\n")?;
    let msg = format!("test {}: {name} ({duration} s) → {}", if passed { "passed" } else { "FAILED" }, dir.display());
    if passed { b.logs.notify("bench", "info", "Test passed", msg) } else { b.logs.notify("bench", "error", "Test failed", msg) }
    Ok(report)
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs() {
        assert_eq!(super::slug("Settings opens!"), "settings-opens");
        assert_eq!(super::slug("!!!"), "test");
    }
}
