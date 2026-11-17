//! Reading the screen and moving between screens: classify by text (profile "screens"),
//! and profile "routes" made of sequence steps plus ensure/select.

use crate::bench::Bench;
use crate::ocr::{self, Line};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const BUILTIN_PROFILES: &[(&str, &str)] = &[
    ("switch2", include_str!("../../../../profiles/switch2.json")),
    ("switch1", include_str!("../../../../profiles/switch1.json")),
];

pub fn load_profile(name: &str) -> Value {
    let custom = crate::config::state_dir().join("profiles").join(format!("{name}.json"));
    std::fs::read_to_string(custom).ok()
        .or_else(|| BUILTIN_PROFILES.iter().find(|(n, _)| *n == name).map(|(_, s)| s.to_string()))
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"screens": {}, "routes": {}}))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

impl Bench {
    pub async fn read_screen(&self, region: Option<[f64; 4]>, fast: bool) -> Result<Vec<Line>> {
        let c = self.target()?;
        let f = c.frames.latest().filter(|f| f.at.elapsed() < Duration::from_secs(5)).ok_or_else(|| {
            anyhow!("no live video ({})", c.frames.error.lock().unwrap().clone().unwrap_or("waiting".into()))
        })?;
        ocr::read(self.ext.ocr, &f.jpeg, region, fast).await
    }

    pub fn classify(&self, lines: &[Line]) -> Vec<String> {
        let text = ocr::join(lines, " ").to_lowercase();
        let mut found = vec![];
        let profile = self.target().map(|c| c.profile.lock().unwrap().clone()).unwrap_or_default();
        if let Some(screens) = profile["screens"].as_object() {
            for (name, rule) in screens {
                let all = strings(&rule["all"]);
                let any = strings(&rule["any"]);
                let res = strings(&rule["re"]);
                let hit = (!all.is_empty() && all.iter().all(|t| text.contains(&t.to_lowercase())))
                    || (!any.is_empty() && any.iter().any(|t| text.contains(&t.to_lowercase())))
                    || res.iter().any(|r| regex::RegexBuilder::new(r).case_insensitive(true).build().is_ok_and(|re| re.is_match(&text)));
                if hit {
                    found.push(name.clone());
                }
            }
        }
        found
    }

    pub async fn where_(&self) -> Result<Value> {
        let lines = self.read_screen(None, false).await?;
        Ok(json!({"screens": self.classify(&lines), "text": ocr::join(&lines, "\n")}))
    }

    fn screens_of(w: &Value) -> Vec<String> {
        strings(&w["screens"])
    }

    /// Get to a screen only if not already there (HOME toggles, so never press it blindly).
    pub async fn ensure(&self, screen: &str, source: &str) -> Result<()> {
        for _ in 0..3 {
            if Self::screens_of(&self.where_().await?).iter().any(|s| s == screen) {
                return Ok(());
            }
            if screen != "home" {
                bail!("cannot ensure {screen:?}: only home is supported");
            }
            self.hub.press(&["HOME".into()], 100, source, false).await?;
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        bail!("could not reach {screen}")
    }

    /// Press `move` until OCR in `region` shows the `select` label.
    async fn select(&self, step: &Value, source: &str) -> Result<()> {
        let target = step["select"].as_str().unwrap_or("");
        let region = ocr::parse_region(&step["region"])?;
        let mv = step["move"].as_str().unwrap_or("RIGHT").to_string();
        for _ in 0..=step["max"].as_u64().unwrap_or(12) {
            if ocr::matches(&self.read_screen(region, false).await?, target, false).is_some() {
                return Ok(());
            }
            self.hub.press(std::slice::from_ref(&mv), 100, source, false).await?;
            tokio::time::sleep(Duration::from_millis(450)).await;
        }
        bail!("could not find {target:?} on screen")
    }

    pub fn routes(&self) -> Value {
        let p = &self.target().map(|c| c.profile.lock().unwrap().clone()).unwrap_or_default();
        let routes: serde_json::Map<String, Value> = p["routes"].as_object().map(|m| m.iter()
            .map(|(k, v)| (k.clone(), json!({"expect": v["expect"], "note": v["note"]}))).collect()).unwrap_or_default();
        let screens: Vec<&String> = p["screens"].as_object().map(|m| m.keys().collect()).unwrap_or_default();
        json!({"profile": p["name"], "screens": screens, "routes": routes})
    }

    pub async fn navigate(&self, route: &str, source: &str) -> Result<Value> {
        let profile = self.target()?.profile.lock().unwrap().clone();
        let r = profile["routes"].get(route).cloned().ok_or_else(|| {
            let known: Vec<String> = profile["routes"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
            anyhow!("unknown route {route:?}; known: {}", known.join(", "))
        })?;
        self.logs.add("bench", format!("{source}: navigate {route}"));
        let mut steps: Vec<Value> = r["steps"].as_array().cloned().unwrap_or_default();
        while steps.first().is_some_and(|s| s.get("ensure").is_some()) {
            let s = steps.remove(0);
            self.ensure(s["ensure"].as_str().unwrap_or(""), source).await?;
        }
        let mut chunk = vec![];
        for step in steps {
            if step.get("select").is_some() {
                if !chunk.is_empty() {
                    crate::server::run_sequence(self, &Value::Array(std::mem::take(&mut chunk)), source).await?;
                }
                self.select(&step, source).await?;
            } else {
                chunk.push(step);
            }
        }
        if !chunk.is_empty() {
            crate::server::run_sequence(self, &Value::Array(chunk), source).await?;
        }
        let expect = r["expect"].as_str();
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let w = self.where_().await?;
            let screens = Self::screens_of(&w);
            let ok = expect.is_none_or(|e| screens.iter().any(|s| s == e));
            if ok || Instant::now() > deadline {
                let msg = format!("navigate {route}: {} (screens: {})", if ok { "arrived" } else { "NOT verified" },
                    if screens.is_empty() { "unknown".into() } else { screens.join(", ") });
                if ok { self.logs.add("bench", msg) } else { self.logs.error("bench", msg) }
                return Ok(json!({"route": route, "arrived": ok, "screens": screens, "text": w["text"], "note": r["note"]}));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}
