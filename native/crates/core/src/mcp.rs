//! MCP server on stdio (JSON-RPC 2.0, one message per line) for Claude Code, Codex, opencode
//! and other MCP clients. It talks to a running `ns2bench serve` over HTTP, so the person
//! watching the UI sees every agent action live and can block agent input at any time.

use crate::client::{query, Client};
use anyhow::{bail, Result};
use base64::Engine as _;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const INSTRUCTIONS: &str = "Drive a game console through a capture card (eyes) and a USB controller/keyboard (hands).
Typical loop: screenshot -> decide -> press/stick/key -> screenshot (use look_after_ms to combine).
Buttons: A B X Y L R ZL ZR PLUS MINUS HOME CAPTURE LSTICK RSTICK (C GL GR in Pro mode) and D-pad UP DOWN LEFT RIGHT.
Sticks take x,y in -1..1 (up/left negative). A human may be watching and can block agent input;
if a call says manual-only mode is on, stop and tell the user.";

fn int(d: &str) -> Value {
    json!({"type": "integer", "description": d})
}
fn num(d: &str) -> Value {
    json!({"type": "number", "description": d})
}
fn st(d: &str) -> Value {
    json!({"type": "string", "description": d})
}
fn boolean(d: &str) -> Value {
    json!({"type": "boolean", "description": d})
}
fn strs(d: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": d})
}
fn region() -> Value {
    json!({"type": "array", "items": {"type": "number"}, "description": "x, y, w, h as fractions of the frame, e.g. [0, 0.8, 1, 0.2]"})
}
fn look() -> Value {
    int("if set, wait this many ms after the action and return a screenshot")
}

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({"name": name, "description": desc, "inputSchema": {"type": "object", "properties": props, "required": required}})
}

pub fn tools() -> Vec<Value> {
    vec![
        tool("screenshot", "See the console screen now. max_width downsizes (full is 1920); png for lossless; region crops to part of the screen (zoom in on small text or an icon).",
            json!({"max_width": int("default 1280"), "png": boolean("lossless PNG instead of JPEG"), "region": region()}), &[]),
        tool("press", "Press and release buttons together, e.g. [\"A\"], [\"L\",\"R\"], [\"DOWN\"]. Set look_after_ms (e.g. 500) to get a screenshot taken that long after release.",
            json!({"buttons": strs("buttons to press together"), "hold_ms": int("default 100"), "look_after_ms": look()}), &["buttons"]),
        tool("hold", "Hold buttons down until release() is called.", json!({"buttons": strs("buttons to hold")}), &["buttons"]),
        tool("release", "Release the given held buttons, or everything (including sticks/keys) if omitted.",
            json!({"buttons": strs("buttons to release; omit for everything")}), &[]),
        tool("stick", "Tilt \"left\" or \"right\" stick to x,y in -1..1 (up = y -1). With hold_ms it recenters afterwards; hold_ms=null keeps it tilted until release().",
            json!({"stick": st("left or right"), "x": num("-1..1"), "y": num("-1..1, up is negative"), "hold_ms": json!({"type": ["integer", "null"], "description": "default 300; null keeps it tilted"}), "look_after_ms": look()}), &["stick", "x", "y"]),
        tool("key", "Tap one USB keyboard key (ENTER, ESC, TAB, UP, A, F1, ...) with optional modifiers (CTRL, SHIFT, ALT, META). Only useful where the console accepts a keyboard.",
            json!({"key": st("key name"), "modifiers": strs("CTRL, SHIFT, ALT, META"), "hold_ms": int("default 60"), "look_after_ms": look()}), &["key"]),
        tool("type_text", "Type ASCII text on the USB keyboard (US layout), e.g. into an on-screen text field.",
            json!({"text": st("text to type"), "delay_ms": int("between keys, default 40"), "look_after_ms": look()}), &["text"]),
        tool("mouse_move", "Move the USB mouse by dx,dy (relative; positive = right/down). buttons keeps buttons down while moving (drag): bit 1 left, 2 right, 4 middle. Only works where the console/game accepts a USB mouse and the board has a mouse.",
            json!({"dx": int("right"), "dy": int("down"), "buttons": int("held buttons while moving: 1 left, 2 right, 4 middle"), "look_after_ms": look()}), &["dx", "dy"]),
        tool("mouse_click", "Click a USB mouse button: \"left\", \"right\" or \"middle\".",
            json!({"button": st("left, right or middle"), "hold_ms": int("default 60"), "look_after_ms": look()}), &[]),
        tool("scroll", "Scroll the USB mouse wheel; positive = up.", json!({"notches": int("wheel notches"), "look_after_ms": look()}), &["notches"]),
        tool("sequence", "Run several steps in order without round-trips. Each step is one of: {\"press\": [\"A\"], \"hold_ms\": 100}, {\"wait_ms\": 500}, {\"hold\": [\"B\"]}, {\"release\": [\"B\"]}, {\"stick\": \"left\", \"x\": 0, \"y\": -1, \"hold_ms\": 400}, {\"key\": \"ENTER\"}, {\"type\": \"hello\"}, {\"mouse\": {\"dx\": 40, \"dy\": 0}}, {\"click\": \"left\"}.",
            json!({"steps": {"type": "array", "items": {"type": "object"}}, "look_after_ms": look()}), &["steps"]),
        tool("wait", "Wait (e.g. for loading), then return a screenshot.", json!({"ms": int("default 1000"), "max_width": int("default 1280")}), &[]),
        tool("logs", "Recent log lines, oldest first, each with its level and, for recognised problems, what to do (→). source: \"bench\" (actions/errors), \"api\" (HTTP calls and refused requests), \"board\" (NS2Bench Bridge), \"kernel\" (board kernel: USB enumeration), \"network\" (console DHCP/DNS), \"screen\" (text that appeared on screen), or a configured file. level: minimum level (debug, info, warn, error; \"warn\" = problems only). after: only lines after this seq (poll for new lines). contains filters by text.",
            json!({"source": st("log source"), "level": st("debug, info, warn or error"), "after": int("seq of the last line you saw"), "contains": st("filter text"), "limit": int("default 60")}), &[]),
        tool("read_screen", "Read the text on screen (OCR), in reading order, each line with its box [x, y, w, h] as fractions of the frame. region limits it to part of the screen.",
            json!({"region": region(), "fast": boolean("faster, less accurate")}), &[]),
        tool("wait_for_text", "Wait until text appears on screen (or disappears with gone=true). Case-insensitive; regex=true treats text as a regular expression. Returns what was found and how long it took.",
            json!({"text": st("text or regex"), "timeout_ms": int("default 10000"), "regex": boolean("treat text as a regex"), "gone": boolean("wait for it to disappear"), "region": region(), "fast": boolean("default true; false is slower but reads small text better")}), &["text"]),
        tool("where_am_i", "Which known screen the console shows (from the console profile), plus the screen text.", json!({}), &[]),
        tool("list_routes", "Known navigation routes and screens for this console profile.", json!({}), &[]),
        tool("navigate", "Go to a known place (e.g. \"home\", \"system_settings\", \"test_buttons\") and verify arrival.", json!({"route": st("route name")}), &["route"]),
        tool("run_test", "Run a test (steps inline or a JSON file path) and get a pass/fail report with per-step detail; screenshots and logs are saved to the report folder. Step types: press/hold/release/stick/key/type/mouse/click/wait_ms, navigate, ensure, expect_screen, expect_text, wait_text, expect_gone, screenshot, probe, note.",
            json!({"steps": {"type": "array", "items": {"type": "object"}}, "path": st("test JSON file on the bench machine"), "name": st("test name"), "stop_on_fail": boolean("default true")}), &[]),
        tool("save_clip", "Save the last N seconds of video and console audio (the bench always keeps the most recent ~30 s) to an MP4 on the bench machine, e.g. right after something went wrong.",
            json!({"seconds": num("default 20"), "label": st("file label")}), &[]),
        tool("doctor", "Check capture, NS2Bench Bridge, USB, console picture, audio, network isolation and calibration; says which part is broken and what to do. probe=true also presses RIGHT/LEFT (HOME only).",
            json!({"probe": boolean("also prove input works")}), &[]),
        tool("status", "Video fps/health, NS2Bench Bridge connection, USB state and whether agents are blocked.", json!({}), &[]),
        tool("usb_mode", "Switch what the console sees: \"standard\" (HORI controller + keyboard + mouse) or \"pro\" (Switch 2 Pro Controller with C/GL/GR, no keyboard). Takes ~8 s; the console reconnects the controller.",
            json!({"mode": st("standard or pro")}), &["mode"]),
        tool("wait_for_screen", "Wait until the screen changes from how it looks now (until=\"change\", e.g. after a press) or stops moving (until=\"stable\", e.g. after a menu animation or loading), instead of guessing a sleep. region watches only part of the screen. screenshot=true returns the screen afterwards.",
            json!({"until": st("change or stable (default change)"), "timeout_ms": int("default 5000"), "stable_ms": int("for stable: how long it must stay still, default 500"), "region": region(), "screenshot": boolean("return a screenshot afterwards")}), &[]),
        tool("find_text", "Find every line on screen that contains text (or matches regex), with its box [x, y, w, h] as fractions of the frame. Use it to see where an item is, or which one is highlighted.",
            json!({"text": st("text or regex"), "regex": boolean("treat text as a regex"), "region": region(), "fast": boolean("faster, less accurate (default false)")}), &["text"]),
        tool("health", "One quick check: is NS2Bench ready to drive (picture, input device, agents allowed)? Lists each problem with what to do. Cheaper than doctor.", json!({}), &[]),
        tool("probe", "Press buttons and measure whether and how fast the screen reacted (proves input reaches the console). Use on a screen where the press visibly changes something.",
            json!({"buttons": strs("buttons, e.g. [\"RIGHT\"]"), "hold_ms": int("default 100"), "timeout_ms": int("default 1500")}), &["buttons"]),
        tool("events", "Recent inputs and control changes, by whom (ui = the person at the bench, api, mcp). See what the person did, or confirm your own presses.",
            json!({"source": st("ui, api or mcp"), "limit": int("default 30")}), &[]),
        tool("list_clips", "Saved video clips (newest first) with their paths on the bench machine.", json!({}), &[]),
        tool("list_reports", "Saved test reports (newest first): folder, test name and whether it passed.", json!({}), &[]),
        tool("video_info", "Capture settings (device, resolution, fps, codec) and the capture devices this computer sees.", json!({}), &[]),
        tool("diagnostics", "A short Markdown summary of versions, devices, settings and recent errors (for bug reports).", json!({"lines": int("log lines to include, default 40")}), &[]),
        tool("block_agents", "Turn on Block agents (manual-only): stop all agent input, including your own, and let only the person at the bench drive. Only that person can turn it off.", json!({}), &[]),
        tool("screen_log", "Turn the screen log on or off: once a second, newly visible on-screen text is added to the logs (source \"screen\").", json!({"on": boolean("on or off")}), &["on"]),
        tool("power", "Wake the console (from a dimmed screen, Sleep Mode or, with a power-button presser set up, a crash), put it to sleep (needs HDMI-CEC), or press its power button. Use wake when screenshots are black or the Bridge is offline because the console slept. Says which steps ran and whether the picture is back.",
            json!({"action": {"type": "string", "enum": ["wake", "sleep", "button"], "description": "wake (default), sleep or button"}}), &[]),
    ]
}

enum Out {
    Text(String),
    Image(Vec<u8>, &'static str),
    /// What happened, then the screen.
    Both(String, Vec<u8>, &'static str),
}

struct Mcp {
    c: Client,
}

impl Mcp {
    async fn grab(&self, max_width: u64, png: bool) -> Result<Out> {
        self.grab_region(max_width, png, None).await
    }

    async fn grab_region(&self, max_width: u64, png: bool, region: Option<&Value>) -> Result<Out> {
        let mut p = vec![("max_width", max_width.clamp(16, 8192).to_string()), ("format", if png { "png" } else { "jpeg" }.into())];
        if let Some(r) = region.and_then(Value::as_array) {
            p.push(("region", r.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")));
        }
        let r = self.c.call("GET", &format!("/api/screenshot{}", query(&p)), None).await?;
        Ok(Out::Image(r.body, if png { "image/png" } else { "image/jpeg" }))
    }

    async fn act(&self, path: &str, mut body: Value, look: Option<u64>) -> Result<Out> {
        body["source"] = "mcp".into();
        self.c.call("POST", path, Some(&body)).await?;
        match look {
            None => Ok(Out::Text("ok".into())),
            Some(ms) => {
                tokio::time::sleep(Duration::from_millis(ms.min(10_000))).await;
                self.grab(1280, false).await
            }
        }
    }

    async fn json(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        self.c.call(method, path, body.as_ref()).await?.json()
    }

    async fn run(&self, name: &str, a: &Value) -> Result<Out> {
        let look = a["look_after_ms"].as_u64();
        let u = |k: &str, d: u64| a[k].as_u64().unwrap_or(d);
        Ok(match name {
            "screenshot" => self.grab_region(u("max_width", 1280), a["png"].as_bool().unwrap_or(false), a.get("region")).await?,
            "press" => self.act("/api/press", json!({"buttons": a["buttons"], "hold_ms": u("hold_ms", 100)}), look).await?,
            "hold" => {
                self.act("/api/hold", json!({"buttons": a["buttons"]}), None).await?;
                Out::Text("held".into())
            }
            "release" => {
                self.act("/api/release", json!({"buttons": a["buttons"]}), None).await?;
                Out::Text("released".into())
            }
            "stick" => {
                let hold = if a.get("hold_ms").is_some() { a["hold_ms"].clone() } else { json!(300) };
                self.act("/api/stick", json!({"stick": a["stick"], "x": a["x"], "y": a["y"], "hold_ms": hold}), look).await?
            }
            "key" => self.act("/api/key", json!({"key": a["key"], "modifiers": a.get("modifiers").cloned().unwrap_or(json!([])), "hold_ms": u("hold_ms", 60)}), look).await?,
            "type_text" => self.act("/api/type", json!({"text": a["text"], "delay_ms": u("delay_ms", 40)}), look).await?,
            "mouse_move" => self.act("/api/mouse", json!({"dx": a["dx"], "dy": a["dy"], "buttons": a["buttons"]}), look).await?,
            "mouse_click" => self.act("/api/click", json!({"button": a["button"].as_str().unwrap_or("left"), "hold_ms": u("hold_ms", 60)}), look).await?,
            "scroll" => self.act("/api/mouse", json!({"wheel": a["notches"]}), look).await?,
            "sequence" => self.act("/api/sequence", json!({"steps": a["steps"]}), look).await?,
            "wait" => {
                tokio::time::sleep(Duration::from_millis(u("ms", 1000).min(60_000))).await;
                self.grab(u("max_width", 1280), false).await?
            }
            "logs" => {
                let mut p = vec![("limit", u("limit", 60).to_string())];
                if let Some(s) = a["source"].as_str() { p.push(("source", s.into())); }
                if let Some(s) = a["contains"].as_str() { p.push(("contains", s.into())); }
                if let Some(s) = a["level"].as_str() { p.push(("level", s.into())); }
                if let Some(n) = a["after"].as_u64() { p.push(("after", n.to_string())); }
                let d = self.json("GET", &format!("/api/logs{}", query(&p)), None).await?;
                let lines: Vec<String> = d["entries"].as_array().into_iter().flatten().map(|e| {
                    let t = chrono::DateTime::from_timestamp(e["t"].as_f64().unwrap_or(0.0) as i64, 0)
                        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()).unwrap_or_default();
                    let hint = e["hint"].as_str().map(|h| format!(" → {h}")).unwrap_or_default();
                    format!("#{} {t} {} [{}] {}{hint}", e["seq"], e["level"].as_str().unwrap_or("info").to_uppercase(), e["source"].as_str().unwrap_or(""), e["text"].as_str().unwrap_or(""))
                }).collect();
                Out::Text(if lines.is_empty() { format!("no log lines (sources: {})", crate::server::strs(&d["sources"]).join(", ")) } else { lines.join("\n") })
            }
            "read_screen" => {
                let mut p = vec![("fast", if a["fast"].as_bool() == Some(true) { "1" } else { "0" }.to_string())];
                if let Some(r) = a["region"].as_array() {
                    p.push(("region", r.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")));
                }
                let d = self.json("GET", &format!("/api/ocr{}", query(&p)), None).await?;
                let lines: Vec<String> = d["lines"].as_array().into_iter().flatten().map(|l| {
                    let b: Vec<String> = l["box"].as_array().into_iter().flatten().map(|v| format!("{:.3}", v.as_f64().unwrap_or(0.0))).collect();
                    format!("{}  @[{}]", l["text"].as_str().unwrap_or(""), b.join(", "))
                }).collect();
                Out::Text(if lines.is_empty() { "(no text)".into() } else { lines.join("\n") })
            }
            "wait_for_text" => {
                let regex = a["regex"].as_bool() == Some(true);
                let gone = a["gone"].as_bool() == Some(true);
                let mut b = json!({"timeout_ms": u("timeout_ms", 10_000), "gone": gone, "fast": a["fast"].as_bool().unwrap_or(true)});
                b[if regex { "regex" } else { "text" }] = a["text"].clone();
                if a["region"].is_array() { b["region"] = a["region"].clone(); }
                let d = self.json("POST", "/api/wait_text", Some(b)).await?;
                let state = match (d["found"] == true, gone) {
                    (true, false) => "found",
                    (true, true) => "gone",
                    (false, false) => "NOT found (timed out)",
                    (false, true) => "STILL THERE (timed out)",
                };
                Out::Text(format!("{state} after {} ms. Screen text now:\n{}", d["elapsed_ms"], d["text"].as_str().unwrap_or("")))
            }
            "where_am_i" => {
                let d = self.json("GET", "/api/where", None).await?;
                let s = crate::server::strs(&d["screens"]);
                Out::Text(format!("screens: {}\n{}", if s.is_empty() { "unknown".into() } else { s.join(", ") }, d["text"].as_str().unwrap_or("")))
            }
            "list_routes" => Out::Text(serde_json::to_string_pretty(&self.json("GET", "/api/routes", None).await?)?),
            "navigate" => {
                let d = self.json("POST", "/api/navigate", Some(json!({"route": a["route"], "source": "mcp"}))).await?;
                let s = crate::server::strs(&d["screens"]);
                let mut t = format!("{} {} (screens: {})", if d["arrived"] == true { "arrived at" } else { "did NOT verify" },
                    a["route"].as_str().unwrap_or(""), if s.is_empty() { "unknown".into() } else { s.join(", ") });
                if let Some(n) = d["note"].as_str() { t += &format!("\nnote: {n}"); }
                Out::Text(t)
            }
            "run_test" => {
                let mut b = json!({"stop_on_fail": a["stop_on_fail"].as_bool().unwrap_or(true), "source": "mcp"});
                if let Some(p) = a["path"].as_str() {
                    // Read here (this process runs where the agent's files are) and send the test itself.
                    let full = crate::config::expand(p);
                    if full.extension().and_then(|e| e.to_str()) != Some("json") { bail!("{p}: a test file is a .json file"); }
                    let text = std::fs::read_to_string(&full).map_err(|e| anyhow::anyhow!("{p}: {e}"))?;
                    b["test"] = serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{p}: {e}"))?;
                } else {
                    b["test"] = json!({"name": a["name"].as_str().unwrap_or("agent test"), "steps": a.get("steps").cloned().unwrap_or(json!([]))});
                }
                let r = self.json("POST", "/api/test", Some(b)).await?;
                let mut lines = vec![format!("{} {} in {} s; report: {}", if r["passed"] == true { "PASS" } else { "FAIL" },
                    r["name"].as_str().unwrap_or(""), r["duration_s"], r["dir"].as_str().unwrap_or(""))];
                for s in r["steps"].as_array().into_iter().flatten() {
                    lines.push(format!("{} {}. {}: {}", if s["ok"] == true { "ok  " } else { "FAIL" }, s["n"], s["step"], s["detail"].as_str().unwrap_or("")));
                }
                Out::Text(lines.join("\n"))
            }
            "save_clip" => {
                let d = self.json("POST", "/api/clip", Some(json!({"seconds": a["seconds"].as_f64().unwrap_or(20.0), "label": a["label"].as_str().unwrap_or("clip")}))).await?;
                Out::Text(format!("saved {} s: {}", d["seconds"], d["path"].as_str().unwrap_or("")))
            }
            "doctor" => {
                let probe = a["probe"].as_bool() == Some(true);
                Out::Text(doctor_text(&self.json("POST", "/api/doctor", Some(json!({"probe": probe}))).await?))
            }
            "status" => Out::Text(serde_json::to_string_pretty(&self.json("GET", "/api/status", None).await?)?),
            "usb_mode" => {
                let m = a["mode"].as_str().unwrap_or("");
                if !["standard", "pro", "hori"].contains(&m) { bail!("mode must be standard or pro"); }
                self.json("POST", "/api/control", Some(json!({"usb_mode": m, "source": "mcp"}))).await?;
                Out::Text(format!("switching to {m}; the controller reconnects in ~8 s"))
            }
            "wait_for_screen" => {
                let mut b = json!({"until": a["until"].as_str().unwrap_or("change"), "timeout_ms": u("timeout_ms", 5000), "stable_ms": u("stable_ms", 500)});
                if a["region"].is_array() { b["region"] = a["region"].clone(); }
                let d = self.json("POST", "/api/wait_screen", Some(b)).await?;
                let text = if d["ok"] == true {
                    format!("{} after {} ms", if d["until"] == "stable" { "screen is still" } else { "screen changed" }, d["waited_ms"])
                } else {
                    format!("{} within {} ms", d["message"].as_str().unwrap_or("timed out"), d["waited_ms"])
                };
                if a["screenshot"].as_bool() == Some(true) {
                    return Ok(match self.grab(1280, false).await? { Out::Image(b, m) => Out::Both(text, b, m), o => o });
                }
                Out::Text(text)
            }
            "find_text" => {
                let regex = a["regex"].as_bool() == Some(true);
                let mut p = vec![(if regex { "regex" } else { "text" }, a["text"].as_str().unwrap_or("").to_string()),
                                 ("fast", if a["fast"].as_bool() == Some(true) { "true" } else { "false" }.into())];
                if let Some(r) = a["region"].as_array() {
                    p.push(("region", r.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")));
                }
                let d = self.json("GET", &format!("/api/find_text{}", query(&p)), None).await?;
                let lines: Vec<String> = d["matches"].as_array().into_iter().flatten().map(|l| {
                    let b: Vec<String> = l["box"].as_array().into_iter().flatten().map(|v| format!("{:.3}", v.as_f64().unwrap_or(0.0))).collect();
                    format!("{}  @[{}]", l["text"].as_str().unwrap_or(""), b.join(", "))
                }).collect();
                Out::Text(if lines.is_empty() { "not on screen".into() } else { lines.join("\n") })
            }
            "health" => {
                let d = self.json("GET", "/api/health", None).await?;
                let mut t = vec![format!("{} · picture {} · input {} · agents {}", if d["ok"] == true { "READY" } else { "NOT READY" },
                    if d["video"] == true { "ok" } else { "missing" }, if d["input"] == true { "ok" } else { "not connected" },
                    if d["agents_allowed"] == true { "allowed" } else { "blocked" })];
                for p in d["problems"].as_array().into_iter().flatten() {
                    t.push(format!("- {}: {} → {}", p["area"].as_str().unwrap_or(""), p["problem"].as_str().unwrap_or(""), p["fix"].as_str().unwrap_or("")));
                }
                Out::Text(t.join("\n"))
            }
            "probe" => {
                let d = self.json("POST", "/api/probe", Some(json!({"buttons": a["buttons"], "hold_ms": u("hold_ms", 100), "timeout_ms": u("timeout_ms", 1500), "source": "mcp"}))).await?;
                Out::Text(if d["changed"] == true { format!("the screen reacted after {} ms", d["latency_ms"]) }
                    else { format!("no visible reaction (change {} vs noise {}); the press may not have reached the console, or this screen doesn't react to it", d["diff"], d["noise"]) })
            }
            "events" => {
                let mut p = vec![("limit", u("limit", 30).to_string())];
                if let Some(s) = a["source"].as_str() { p.push(("source", s.into())); }
                let d = self.json("GET", &format!("/api/events{}", query(&p)), None).await?;
                let lines: Vec<String> = d.as_array().into_iter().flatten().map(|e| {
                    let mut e2 = e.clone();
                    let (t, src, act) = (crate::logs::local_time(e["t"].as_f64().unwrap_or(0.0)), e["source"].as_str().unwrap_or("").to_string(), e["action"].as_str().unwrap_or("").to_string());
                    if let Some(o) = e2.as_object_mut() { o.remove("t"); o.remove("source"); o.remove("action"); }
                    format!("{t} {src}: {act} {e2}")
                }).collect();
                Out::Text(if lines.is_empty() { "no inputs yet".into() } else { lines.join("\n") })
            }
            "list_clips" => Out::Text(serde_json::to_string_pretty(&self.json("GET", "/api/clips", None).await?)?),
            "list_reports" => Out::Text(serde_json::to_string_pretty(&self.json("GET", "/api/reports", None).await?)?),
            "video_info" => Out::Text(serde_json::to_string_pretty(&self.json("GET", "/api/video", None).await?)?),
            "diagnostics" => Out::Text(self.c.call("GET", &format!("/api/diagnostics?lines={}", u("lines", 40).min(500)), None).await?.text()),
            "block_agents" => {
                self.json("POST", "/api/control", Some(json!({"manual_only": true, "source": "mcp"}))).await?;
                Out::Text("agents are blocked; only the person at the NS2Bench page can turn this off".into())
            }
            "screen_log" => {
                let on = a["on"].as_bool().unwrap_or(true);
                self.json("POST", "/api/control", Some(json!({"screen_log": on, "source": "mcp"}))).await?;
                Out::Text(format!("screen log {}", if on { "on" } else { "off" }))
            }
            "power" => {
                let d = self.json("POST", "/api/power", Some(json!({"action": a["action"].as_str().unwrap_or("wake"), "source": "mcp"}))).await?;
                let mut t = d["message"].as_str().or(d["detail"].as_str()).unwrap_or(if d["ok"] == true { "done" } else { "failed" }).to_string();
                for s in d["steps"].as_array().into_iter().flatten() {
                    t += &format!("\n{} {}{}", if s["ok"] == true { "ok  " } else { "FAIL" }, s["step"].as_str().unwrap_or(""),
                        s["detail"].as_str().filter(|x| !x.is_empty()).map(|x| format!(": {x}")).unwrap_or_default());
                }
                Out::Text(t)
            }
            _ => bail!("unknown tool {name}"),
        })
    }
}

pub fn doctor_text(r: &Value) -> String {
    let mut lines = vec![format!("overall {}", r["overall"].as_str().unwrap_or("?").to_uppercase())];
    for c in r["checks"].as_array().into_iter().flatten() {
        let status = c["status"].as_str().unwrap_or("");
        let mut l = format!("{:4} {}: {}", status.to_uppercase(), c["name"].as_str().unwrap_or(""), c["detail"].as_str().unwrap_or(""));
        if matches!(status, "warn" | "fail") && !c["hint"].as_str().unwrap_or("").is_empty() {
            l += &format!("  -> {}", c["hint"].as_str().unwrap_or(""));
        }
        lines.push(l);
    }
    lines.join("\n")
}

async fn handle(m: &Mcp, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?; // notifications get no reply
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "NS2Bench", "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"].get("arguments").cloned().unwrap_or(json!({}));
            Ok(match m.run(name, &args).await {
                Ok(Out::Text(t)) => json!({"content": [{"type": "text", "text": t}], "isError": false}),
                Ok(Out::Image(b, mime)) => json!({"content": [{"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(b), "mimeType": mime}], "isError": false}),
                Ok(Out::Both(t, b, mime)) => json!({"content": [{"type": "text", "text": t}, {"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(b), "mimeType": mime}], "isError": false}),
                Err(e) => json!({"content": [{"type": "text", "text": e.to_string()}], "isError": true}),
            })
        }
        "resources/list" => Ok(json!({"resources": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        _ => Err(json!({"code": -32601, "message": format!("method not found: {method}")})),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
    })
}

/// MCP over HTTP at POST /mcp (the streamable HTTP transport, answered as plain JSON): agents
/// add the running engine by address, with nothing to install or launch. Same tools as stdio.
pub async fn http(axum::extract::State(b): axum::extract::State<crate::server::S>, axum::Json(msg): axum::Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let m = Mcp { c: Client::from_config(&b.cfg, "mcp") };
    let reply = match msg.as_array() {
        Some(batch) => {
            let mut out = vec![];
            for (i, one) in batch.iter().enumerate() {
                if i >= 50 {
                    // answered, not dropped: a client waits for every id it sent
                    if let Some(id) = one.get("id") {
                        out.push(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32600, "message": "at most 50 requests in one batch"}}));
                    }
                    continue;
                }
                if let Some(r) = handle(&m, one).await { out.push(r); }
            }
            (!out.is_empty()).then_some(Value::Array(out))
        }
        None => handle(&m, &msg).await,
    };
    match reply {
        Some(r) => axum::Json(r).into_response(),
        None => axum::http::StatusCode::ACCEPTED.into_response(), // notifications and responses get no body
    }
}

/// Serve MCP on stdin/stdout until stdin closes.
pub async fn run(cfg: crate::Config) -> Result<()> {
    let m = std::sync::Arc::new(Mcp { c: Client::from_config(&cfg, "mcp") });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(v) = rx.recv().await {
            let mut s = v.to_string();
            s.push('\n');
            if out.write_all(s.as_bytes()).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    });
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}));
                continue;
            }
        };
        // calls run concurrently (a long wait/test does not block a status call)
        let (m, tx) = (m.clone(), tx.clone());
        tokio::spawn(async move {
            if let Some(r) = handle(&m, &msg).await {
                let _ = tx.send(r);
            }
        });
    }
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_schemas_are_valid() {
        let t = super::tools();
        assert_eq!(t.len(), 35);
        let mut names: Vec<&str> = t.iter().map(|x| x["name"].as_str().unwrap()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 35);
        assert!(t.iter().all(|x| x["inputSchema"]["type"] == "object"));
    }
}
