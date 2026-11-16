//! Read the console screen as text.
//!   vision     macOS: Apple Vision through the capture helper (fast, accurate, built in)
//!   tesseract  any OS with the `tesseract` CLI installed
//! Lines are {"text", "conf" 0..1, "box": [x, y, w, h]} normalised to the full frame.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Line {
    pub text: String,
    pub conf: Option<f64>,
    #[serde(rename = "box")]
    pub bbox: Option<[f64; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Engine {
    Vision,
    Tesseract,
}

impl Engine {
    pub fn name(&self) -> &'static str {
        match self {
            Engine::Vision => "vision",
            Engine::Tesseract => "tesseract",
        }
    }
}

pub fn which(cmd: &str) -> bool {
    let exe = if cfg!(windows) { format!("{cmd}.exe") } else { cmd.to_string() };
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(&exe).is_file()))
}

/// tesseract on PATH, or where the Windows installer puts it (it doesn't add itself to PATH).
fn tesseract() -> Option<String> {
    if which("tesseract") {
        return Some("tesseract".into());
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var) {
                let base = std::path::PathBuf::from(base);
                for p in [base.join("Tesseract-OCR").join("tesseract.exe"), base.join("Programs").join("Tesseract-OCR").join("tesseract.exe")] {
                    if p.is_file() {
                        return Some(p.display().to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn make_engine(cfg: Option<&Value>) -> Option<Engine> {
    let want = cfg.and_then(|c| c["engine"].as_str()).unwrap_or("auto");
    if matches!(want, "vision" | "auto") && cfg!(target_os = "macos")
        && (crate::video::bundled_helper().is_some() || which("swiftc")) {
        return Some(Engine::Vision);
    }
    if matches!(want, "tesseract" | "auto") && tesseract().is_some() {
        return Some(Engine::Tesseract);
    }
    None
}

async fn pipe(cmd: &str, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    let mut child = crate::video::quiet(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::piped()).kill_on_drop(true).spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let data = input.to_vec();
    let w = tokio::spawn(async move {
        let _ = stdin.write_all(&data).await;
    });
    // a stuck OCR process must not hold up navigation or the power button for good
    let out = tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output()).await
        .map_err(|_| anyhow!("{cmd} gave no answer within 30 s"))??;
    let _ = w.await;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!("{}", if msg.is_empty() { format!("{cmd} failed") } else { msg });
    }
    Ok(out.stdout)
}

async fn engine_read(engine: Engine, jpeg: &[u8], fast: bool) -> Result<Vec<Line>> {
    match engine {
        Engine::Vision => {
            let helper = crate::video::build_helper().await?;
            let args: Vec<&str> = if fast { vec!["ocr", "fast"] } else { vec!["ocr"] };
            let out = pipe(helper.to_str().unwrap(), &args, jpeg).await?;
            let v: Value = serde_json::from_slice(&out)?;
            Ok(serde_json::from_value(v["lines"].clone())?)
        }
        Engine::Tesseract => {
            // the size from the header (decoding the whole frame here would stall the runtime)
            let (w, h) = image::ImageReader::new(std::io::Cursor::new(jpeg)).with_guessed_format()?.into_dimensions()
                .map(|(w, h)| (w as f64, h as f64))?;
            let exe = tesseract().ok_or_else(|| anyhow!("tesseract is not installed"))?;
            let out = pipe(&exe, &["stdin", "stdout", "--psm", "11", "tsv"], jpeg).await?;
            // (block, paragraph, line) -> words as (left, top, width, height, conf, text)
            type Word = (f64, f64, f64, f64, f64, String);
            let mut words: BTreeMap<(String, String, String), Vec<Word>> = BTreeMap::new();
            for row in String::from_utf8_lossy(&out).lines().skip(1) {
                let f: Vec<&str> = row.split('\t').collect();
                if f.len() < 12 || f[11].trim().is_empty() {
                    continue;
                }
                let conf: f64 = f[10].parse().unwrap_or(-1.0);
                if conf < 0.0 {
                    continue;
                }
                let n = |i: usize| f[i].parse::<f64>().unwrap_or(0.0);
                words.entry((f[2].into(), f[3].into(), f[4].into())).or_default()
                    .push((n(6), n(7), n(8), n(9), conf / 100.0, f[11].to_string()));
            }
            Ok(words.into_values().map(|ws| {
                let x0 = ws.iter().map(|w| w.0).fold(f64::MAX, f64::min);
                let y0 = ws.iter().map(|w| w.1).fold(f64::MAX, f64::min);
                let x1 = ws.iter().map(|w| w.0 + w.2).fold(0.0, f64::max);
                let y1 = ws.iter().map(|w| w.1 + w.3).fold(0.0, f64::max);
                Line {
                    text: ws.iter().map(|w| w.5.as_str()).collect::<Vec<_>>().join(" "),
                    conf: Some(ws.iter().map(|w| w.4).sum::<f64>() / ws.len() as f64),
                    bbox: Some([x0 / w, y0 / h, (x1 - x0) / w, (y1 - y0) / h]),
                }
            }).collect())
        }
    }
}

pub fn crop(jpeg: &[u8], r: [f64; 4]) -> Result<Vec<u8>> {
    let im = image::load_from_memory(jpeg)?;
    let (w, h) = (im.width() as f64, im.height() as f64);
    // a region from a request: clamp to the frame (no underflow on a negative or oversized box)
    let px = |v: f64, size: f64| (v.clamp(0.0, 1.0) * size).round() as u32;
    let (x, y) = (px(r[0], w), px(r[1], h));
    let cw = px(r[0] + r[2], w).saturating_sub(x);
    let ch = px(r[1] + r[3], h).saturating_sub(y);
    let c = im.crop_imm(x, y, cw.max(1), ch.max(1)).to_rgb8();
    let mut out = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95).encode_image(&c)?;
    Ok(out)
}

/// OCR a frame (optionally a region of it); lines in reading order, boxes in frame coordinates.
pub async fn read(engine: Option<Engine>, jpeg: &[u8], region: Option<[f64; 4]>, fast: bool) -> Result<Vec<Line>> {
    let engine = engine.ok_or_else(|| anyhow!("no OCR engine: on macOS install Xcode command line tools; elsewhere install tesseract"))?;
    let data = match region {
        Some(r) => {
            let j = jpeg.to_vec();
            tokio::task::spawn_blocking(move || crop(&j, r)).await??
        }
        None => jpeg.to_vec(),
    };
    let mut lines: Vec<Line> = engine_read(engine, &data, fast).await?.into_iter()
        .filter(|l| l.conf.unwrap_or(1.0) >= 0.3 && !l.text.trim().is_empty()).collect();
    if let Some([x, y, w, h]) = region {
        for l in &mut lines {
            if let Some(b) = &mut l.bbox {
                *b = [x + b[0] * w, y + b[1] * h, b[2] * w, b[3] * h];
            }
        }
    }
    let key = |l: &Line| l.bbox.map_or((0, 0.0), |b| (((b[1] * 100.0).round()) as i64, b[0]));
    lines.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
    Ok(lines)
}

/// First line containing `pattern` (case-insensitive) or matching it as a regex; else the joined text.
pub fn matches(lines: &[Line], pattern: &str, regex: bool) -> Option<Line> {
    let re = if regex { regex::RegexBuilder::new(pattern).case_insensitive(true).build().ok() } else { None };
    let lower = pattern.to_lowercase();
    let hit = |t: &str| match &re {
        Some(r) => r.is_match(t),
        None => !regex && t.to_lowercase().contains(&lower),
    };
    if let Some(l) = lines.iter().find(|l| hit(&l.text)) {
        return Some(l.clone());
    }
    let joined = join(lines, " ");
    hit(&joined).then_some(Line { text: joined, conf: None, bbox: None })
}

pub fn join(lines: &[Line], sep: &str) -> String {
    lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(sep)
}

pub fn parse_region(v: &Value) -> Result<Option<[f64; 4]>> {
    let nums: Vec<f64> = match v {
        Value::Null => return Ok(None),
        Value::String(s) if s.is_empty() => return Ok(None),
        Value::String(s) => s.split(',').map(|x| x.trim().parse::<f64>()).collect::<std::result::Result<_, _>>()
            .map_err(|_| anyhow!("region is x,y,w,h in 0..1 of the frame"))?,
        Value::Array(a) => a.iter().map(|x| x.as_f64().unwrap_or(-1.0)).collect(),
        _ => bail!("region is x,y,w,h in 0..1 of the frame"),
    };
    if nums.len() != 4 || !nums.iter().all(|x| (0.0..=1.0).contains(x)) {
        bail!("region is x,y,w,h in 0..1 of the frame");
    }
    Ok(Some([nums[0], nums[1], nums[2], nums[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn l(t: &str) -> Line {
        Line { text: t.into(), conf: Some(0.9), bbox: Some([0.0, 0.0, 0.1, 0.1]) }
    }
    #[test]
    fn matching() {
        let ls = vec![l("System Settings"), l("Airplane Mode")];
        assert_eq!(matches(&ls, "airplane", false).unwrap().text, "Airplane Mode");
        assert!(matches(&ls, "settings airplane", false).is_some()); // across lines
        assert!(matches(&ls, "^air.*mode$", true).is_some());
        assert!(matches(&ls, "wifi", false).is_none());
        assert_eq!(parse_region(&Value::String("0,0.5,1,0.5".into())).unwrap(), Some([0.0, 0.5, 1.0, 0.5]));
        assert!(parse_region(&Value::String("0,0,2,1".into())).is_err());
    }
}
