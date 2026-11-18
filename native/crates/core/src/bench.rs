//! The bench: config + consoles + the controller + logs + everything built on them.

use crate::config::{state_dir, write_json, Config};
use crate::console::Console;
use crate::inputs::{start_backend, InputHub};
use crate::logs::LogHub;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

pub struct Bench {
    pub cfg: Config,
    pub logs: LogHub,
    /// The controller (one, shared by the consoles: it is plugged into one at a time).
    pub hub: Arc<InputHub>,
    pub input_cfg: Mutex<Value>,
    pub settings_lock: tokio::sync::Mutex<()>,
    /// Every console on this bench, each with its own capture running.
    pub consoles: RwLock<Vec<Arc<Console>>>,
    /// The console the controller is plugged into; None: unknown (input is paused then).
    pub controller_on: Mutex<Option<String>>,
    /// Finding out by itself which console the controller is on (crate::detect).
    pub detecting: std::sync::atomic::AtomicBool,
    /// One change to the consoles (or the controller's place) at a time, saved before the next.
    changes: Mutex<()>,
    pub screen_log: AtomicBool,
    pub active_operations: std::sync::atomic::AtomicUsize,
    pub calibration: Mutex<Value>,
    pub ext: crate::features::Ext,
    started: Instant,
}

impl Bench {
    pub fn new(cfg: Config) -> Arc<Bench> {
        let logs = LogHub::new(cfg.raw.get("logs"));
        // Daily log files in ~/.ns2bench/logs (not from unit tests).
        if !cfg!(test) && cfg.raw["log_files"].as_bool() != Some(false) {
            logs.persist(state_dir().join("logs"));
        }
        let (conn_tx, mut conn_rx) = tokio::sync::mpsc::unbounded_channel();
        let backend = start_backend(&cfg.input, logs.clone(), conn_tx);
        let hub = InputHub::new(backend, logs.clone());
        let calibration: Value = std::fs::read_to_string(state_dir().join("calibration.json")).ok()
            .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
        hub.min_press_ms.store(crate::calib::clamp_min_press(calibration["min_press_ms"].as_u64().unwrap_or(crate::calib::DEFAULT_MIN_PRESS_MS)), std::sync::atomic::Ordering::Relaxed);
        let screen_log = cfg.raw["ocr"]["screen_log"].as_bool().unwrap_or(false);
        // Unit tests never read the person's consoles.json.
        let (entries, controller_on) = if cfg!(test) {
            (vec![json!({"id": "switch-2", "name": "Switch 2", "model": "switch2"})], Some("switch-2".to_string()))
        } else {
            crate::console::load(&cfg.raw)
        };
        let consoles: Vec<Arc<Console>> = entries.iter().map(|e| Console::new(e, &cfg.video, &cfg.raw)).collect();
        // Two consoles can't read one card: if the file says so (it happened), the later one gets
        // none and is asked for its own, instead of both fighting over the card.
        let mut seen_cards: Vec<String> = vec![];
        for c in &consoles {
            let dev = c.video_cfg.lock().unwrap()["device"].as_str().map(str::to_string);
            if let Some(d) = dev {
                if seen_cards.contains(&d) {
                    logs.warn("bench", format!("{}: its capture card {d} is also another console's; pick its own in Settings → Consoles", c.name()));
                    c.video_cfg.lock().unwrap()["device"] = Value::Null;
                } else {
                    seen_cards.push(d);
                }
            }
        }
        // Several consoles: where the controller is isn't known at start (its cable may have moved
        // while NS2Bench was closed). The page asks; one console needs no answer.
        let controller_on = controller_on.filter(|id| consoles.iter().any(|c| &c.id == id)).filter(|_| consoles.len() < 2 || cfg!(test));
        let bench = Arc::new(Bench {
            input_cfg: Mutex::new(cfg.input.clone()), settings_lock: tokio::sync::Mutex::new(()),
            ext: crate::features::Ext::new(&cfg),
            consoles: RwLock::new(consoles), controller_on: Mutex::new(controller_on), changes: Mutex::new(()), detecting: false.into(),
            cfg, logs, hub,
            active_operations: 0.into(), screen_log: screen_log.into(), calibration: Mutex::new(calibration), started: Instant::now(),
        });
        let h = bench.hub.clone();
        tokio::spawn(async move { while conn_rx.recv().await.is_some() { h.resend(); } });
        bench.apply_pause();
        bench
    }

    pub fn start(self: &Arc<Self>) {
        let names: Vec<String> = self.list().iter().map(|c| c.name()).collect();
        self.logs.add("bench", format!("NS2Bench {} started (native): consoles {} · input={}", env!("CARGO_PKG_VERSION"),
            names.join(", "), self.cfg.input["backend"].as_str().unwrap_or("none")));
        for c in self.list() {
            self.restart_video(&c);
            crate::features::start_console(self.clone(), c);
        }
        tokio::spawn(self.logs.clone().tail_files());
        crate::features::start(self.clone());
    }

    // ------------------------------------------------------------ consoles

    pub fn list(&self) -> Vec<Arc<Console>> {
        self.consoles.read().unwrap().clone()
    }

    /// A console by id or name (names match without regard to case).
    pub fn console(&self, sel: &str) -> Option<Arc<Console>> {
        let s = sel.trim();
        self.list().into_iter().find(|c| c.id == s || c.name().eq_ignore_ascii_case(s))
    }

    /// The console the controller is plugged into, if known.
    pub fn controller_console(&self) -> Option<Arc<Console>> {
        let on = self.controller_on.lock().unwrap().clone()?;
        self.console(&on)
    }

    /// The console a request is about: the one it names, else the one with the controller, else
    /// the first.
    pub fn target(&self) -> Result<Arc<Console>> {
        match crate::console::selected() {
            Some(sel) => self.console(&sel).ok_or_else(|| {
                let names: Vec<String> = self.list().iter().map(|c| format!("{} ({})", c.name(), c.id)).collect();
                anyhow!("no console named {sel:?}: this bench has {}", names.join(", "))
            }),
            None => self.controller_console().or_else(|| self.list().into_iter().next()).ok_or_else(|| anyhow!("no console set up")),
        }
    }

    /// Input may go to the console a request names only if the controller is plugged into it.
    pub fn check_input_target(&self) -> Result<()> {
        if self.list().len() < 2 {
            return Ok(());
        }
        let on = self.controller_console();
        let Some(on) = on else {
            bail!(crate::inputs::Conflict("the controller's console isn't known (its cable was moved?): pick where it is in the console bar".into()))
        };
        if let Some(sel) = crate::console::selected() {
            let want = self.console(&sel).ok_or_else(|| anyhow!("no console named {sel:?}"))?;
            if want.id != on.id {
                bail!(crate::inputs::Conflict(format!("the controller is plugged into {}, not {}: move its cable, then pick {} in the console bar",
                    on.name(), want.name(), want.name())));
            }
        }
        Ok(())
    }

    /// Input is paused while the controller's console is unknown (several consoles only).
    fn apply_pause(&self) {
        let unknown = self.list().len() > 1 && self.controller_console().is_none();
        *self.hub.paused.lock().unwrap() = unknown.then(|| "the controller's console isn't known: pick where it is plugged in".to_string());
    }

    pub fn save_consoles(&self) -> Result<()> {
        let on = self.controller_on.lock().unwrap().clone();
        crate::console::save(&self.list(), on.as_deref())?;
        Ok(())
    }

    /// The controller is now plugged into `id` (None: unknown). Releases everything first, then
    /// sets the board's USB mode to what the console's model needs.
    pub fn set_controller_on(self: &Arc<Self>, id: Option<&str>) -> Result<()> {
        let _one = self.changes.lock().unwrap();
        let target = match id {
            Some(i) => Some(self.console(i).ok_or_else(|| anyhow!("no console {i:?}"))?),
            None => None,
        };
        self.hub.release_all();
        let new = target.as_ref().map(|c| c.id.clone());
        let moved = *self.controller_on.lock().unwrap() != new;
        *self.controller_on.lock().unwrap() = new;
        // anything still running for the old place (a sequence, a test) stops pressing
        if moved { self.hub.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
        self.apply_pause();
        self.save_consoles()?;
        match &target {
            Some(c) => {
                *self.hub.placed.lock().unwrap() = Some(std::time::Instant::now());
                // placed again: the mode follows this console (a hand-picked mode was for the last one)
                self.hub.manual_mode.store(false, std::sync::atomic::Ordering::SeqCst);
                self.hub.mode_tries.store(0, std::sync::atomic::Ordering::SeqCst);
                self.logs.add("bench", format!("controller: now on {} ({})", c.name(), c.id));
                // (a board that is restarting gets it when it's back: see watch_controller)
                self.apply_mode();
            }
            None => self.logs.warn("bench", "controller: where it is plugged in is unknown; input is paused until you pick the console"),
        }
        Ok(())
    }

    /// The person switched the page to this console: with "the shared controller follows the
    /// console I watch" on and the console using it, the controller moves there (and its mode with
    /// it). Refused when the board shows it's still in another console: a Switch 2 is driving it
    /// as its Pro Controller, and this console isn't that Switch 2.
    pub fn follow_view(self: &Arc<Self>, id: &str) -> Result<bool> {
        use std::sync::atomic::Ordering::Relaxed;
        let c = self.console(id).ok_or_else(|| anyhow!("no console {id:?}"))?;
        if !crate::console::FOLLOW_VIEW.load(Relaxed) || !c.uses_controller.load(Relaxed) || self.list().len() < 2 {
            return Ok(false);
        }
        let on = self.controller_console();
        if on.as_ref().is_some_and(|o| o.id == c.id) { return Ok(false); }
        let st = self.hub.status();
        if st["host"] == "switch2" && *c.model.lock().unwrap() != "switch2" {
            let there = on.map(|o| o.name()).unwrap_or_else(|| "a Switch 2".into());
            bail!(crate::inputs::Conflict(format!("the controller is still plugged into {there} (it's driving it as its Pro Controller): move its cable to {} first, then pick it again", c.name())));
        }
        self.set_controller_on(Some(&c.id))?;
        self.logs.add("bench", format!("controller: follows the console on screen: {}", c.name()));
        Ok(true)
    }

    /// Put the board in the USB mode the controller's console needs, if it's connected and in
    /// another one.
    pub fn apply_mode(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        let Some(c) = self.controller_console() else { return };
        if self.hub.manual_mode.load(SeqCst) { return; }
        let want = crate::console::model_mode(&c.model.lock().unwrap());
        let st = self.hub.status();
        // next time the board powers up (the console woke), it starts as what this console needs,
        // instead of starting as the other and switching over (seconds, and a reconnect)
        if st["connected"] == true { let _ = self.hub.backend.send(crate::inputs::Msg::BootMode(want.to_string())); }
        if st["connected"] == true && st["usb_mode"].as_str().is_some_and(|m| m != want) {
            if self.hub.mode_tries.fetch_add(1, SeqCst) >= 2 {
                if self.hub.mode_tries.load(SeqCst) == 3 {
                    self.logs.warn("bench", format!("controller: the board didn't reach {want} mode for {} after two tries; pick it under Controller", c.name()));
                }
                return;
            }
            self.logs.add("bench", format!("controller: switching the board to {want} mode for {}", c.name()));
            let _ = self.hub.switch_usb_mode(want);
        }
    }

    /// Add a console (its capture card must not be in use by another console).
    pub fn add_console(self: &Arc<Self>, v: &Value) -> Result<Arc<Console>> {
        let _one = self.changes.lock().unwrap();
        let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| anyhow!("a console needs a name"))?;
        if let Some(ip) = v["network_ip"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
            ip.parse::<std::net::Ipv4Addr>().map_err(|_| anyhow!("{ip} is not an IPv4 address"))?;
        }
        if self.list().len() >= 8 { bail!("at most 8 consoles"); }
        if self.console(name).is_some() { bail!("there is already a console called {name}"); }
        let model = v["model"].as_str().unwrap_or("switch2");
        if !crate::console::MODELS.iter().any(|m| m.0 == model) { bail!("unknown model {model:?}"); }
        let mut id = crate::console::slug(name);
        let base = id.clone();
        let mut n = 2;
        while self.console(&id).is_some() { id = format!("{base}-{n}"); n += 1; }
        // Only the name is needed: the card, address and colour can come later. Without a card it
        // never inherits another console's (the bench's video settings carry the first one's card):
        // it starts with none and takes a card no other console uses once one is connected.
        let device = v["video"]["device"].as_str().unwrap_or("");
        self.check_device_free(device, None)?;
        let mut video = v["video"].as_object().cloned().unwrap_or_default();
        video.insert("device".into(), if device.is_empty() { Value::Null } else { device.into() });
        let color = v["color"].as_str().filter(|c| c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())).unwrap_or("#0ab9e6");
        let ip = v["network_ip"].as_str().map(str::trim).filter(|s| !s.is_empty());
        let entry = json!({"id": id, "name": name, "model": model, "color": color, "network_ip": ip, "video": video});
        let c = Console::new(&entry, &self.cfg.video, &self.cfg.raw);
        self.consoles.write().unwrap().push(c.clone());
        self.apply_pause();
        self.save_consoles()?;
        self.restart_video(&c);
        crate::features::start_console(self.clone(), c.clone());
        self.logs.add("bench", format!("console added: {name} ({id}), model {model}"));
        Ok(c)
    }

    /// Two consoles can't read one capture card (sharing a card is not supported yet).
    fn check_device_free(&self, device: &str, except: Option<&str>) -> Result<()> {
        if device.is_empty() { return Ok(()); }
        if let Some(other) = self.list().into_iter().find(|c| Some(c.id.as_str()) != except
            && c.video_cfg.lock().unwrap()["device"].as_str() == Some(device)) {
            bail!("that capture card is already used by {}: each console needs its own card", other.name());
        }
        Ok(())
    }

    /// Rename, change model, colour or network address.
    pub fn update_console(&self, id: &str, v: &Value) -> Result<()> {
        let _one = self.changes.lock().unwrap();
        let c = self.console(id).ok_or_else(|| anyhow!("no console {id:?}"))?;
        if let Some(name) = v["name"].as_str().map(str::trim) {
            if name.is_empty() { bail!("a console needs a name"); }
            if self.console(name).is_some_and(|o| o.id != c.id) { bail!("there is already a console called {name}"); }
            *c.name.lock().unwrap() = name.to_string();
        }
        if let Some(model) = v["model"].as_str() {
            if !crate::console::MODELS.iter().any(|m| m.0 == model) { bail!("unknown model {model:?}"); }
            *c.model.lock().unwrap() = model.to_string();
            *c.profile.lock().unwrap() = crate::nav::load_profile(model);
        }
        if let Some(color) = v["color"].as_str() {
            if !(color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|x| x.is_ascii_hexdigit())) { bail!("colour must be like #e60012"); }
            *c.color.lock().unwrap() = color.to_string();
        }
        if v.get("network_ip").is_some() {
            let ip = v["network_ip"].as_str().map(str::trim).filter(|s| !s.is_empty());
            if let Some(ip) = ip { ip.parse::<std::net::Ipv4Addr>().map_err(|_| anyhow!("{ip} is not an IPv4 address"))?; }
            *c.network_ip.lock().unwrap() = ip.map(str::to_string);
        }
        if let Some(u) = v["uses_controller"].as_bool() {
            c.uses_controller.store(u, std::sync::atomic::Ordering::Relaxed);
        }
        self.save_consoles()?;
        self.logs.add("bench", format!("console {} updated: {v}", c.id));
        Ok(())
    }

    pub fn remove_console(&self, id: &str) -> Result<()> {
        let _one = self.changes.lock().unwrap();
        let c = self.console(id).ok_or_else(|| anyhow!("no console {id:?}"))?;
        if self.list().len() < 2 { bail!("the last console can't be removed (rename it instead)"); }
        c.stop();
        self.consoles.write().unwrap().retain(|x| x.id != c.id);
        {
            let mut on = self.controller_on.lock().unwrap();
            if on.as_deref() == Some(c.id.as_str()) {
                self.hub.release_all();
                *on = None;
                self.hub.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        self.apply_pause();
        self.save_consoles()?;
        self.logs.add("bench", format!("console removed: {} ({})", c.name(), c.id));
        Ok(())
    }

    // ------------------------------------------------------------ video

    pub fn restart_video(self: &Arc<Self>, c: &Arc<Console>) {
        let b = Arc::downgrade(self);
        let first = self.list().first().is_some_and(|f| f.id == c.id);
        let video_cfg = c.video_cfg.clone();
        // a capture card picked automatically is kept (consoles.json; the first console also in
        // video.json, which older versions read)
        let save: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(b) = b.upgrade() { let _ = b.save_consoles(); }
            if first { save_video_json(&video_cfg.lock().unwrap()); }
        });
        // cards the other consoles use: an automatic pick never takes one
        let (b2, me) = (Arc::downgrade(self), c.id.clone());
        let taken: crate::video::Taken = Arc::new(move || {
            b2.upgrade().map(|b| b.list().iter().filter(|o| o.id != me)
                .filter_map(|o| o.video_cfg.lock().unwrap()["device"].as_str().map(str::to_string)).collect()).unwrap_or_default()
        });
        c.restart_video(&self.logs, save, taken);
    }

    pub async fn set_video(self: &Arc<Self>, c: &Arc<Console>, changes: Value) -> Result<()> {
        let Some(obj) = changes.as_object() else { bail!("JSON object required") };
        let allowed = crate::console::VIDEO_KEYS;
        if let Some(k) = obj.keys().find(|k| !allowed.contains(&k.as_str())) { bail!("unknown video setting: {k}"); }
        if obj.get("fps").and_then(Value::as_u64).is_some_and(|f| !(1..=120).contains(&f)) { bail!("fps must be 1-120"); }
        if let Some(d) = obj.get("device").and_then(Value::as_str) { self.check_device_free(d, Some(&c.id))?; }
        {
            let mut v = c.video_cfg.lock().unwrap();
            for (k, x) in obj { v[k] = x.clone(); }
            // new settings: ask the card for them again (it may accept these)
            if let Some(o) = v.as_object_mut() { o.remove("_device_default"); }
            if self.list().first().is_some_and(|f| f.id == c.id) { save_video_json(&v); }
        }
        self.save_consoles()?;
        self.logs.add("bench", format!("video settings changed ({}): {changes}", c.name()));
        self.restart_video(c);
        Ok(())
    }

    pub fn status(&self) -> Value {
        let target = self.target().ok();
        let on = self.controller_on.lock().unwrap().clone();
        let mut s = json!({
            "video": target.as_ref().map(|c| c.frames.status()).unwrap_or(json!({})),
            "input": self.hub.status(),
            "calibration": *self.calibration.lock().unwrap(),
            "uptime_s": self.started.elapsed().as_secs(),
            "active_operations": self.active_operations.load(std::sync::atomic::Ordering::Relaxed),
            "engine": "native",
            "version": env!("CARGO_PKG_VERSION"),
            "safe_mode": crate::config::safe_mode(),
            // what the page can offer: putting a console to sleep needs HDMI-CEC
            "power": {"cec": crate::power::settings()["cec"] == true},
            "last_run_unclean": crate::report::unclean_previous().is_some(),
            "console": target.as_ref().map(|c| c.id.clone()),
            "consoles": self.list().iter().map(|c| c.info(on.as_deref() == Some(c.id.as_str()))).collect::<Vec<_>>(),
            "controller_on": on,
            "controller_detecting": self.detecting.load(std::sync::atomic::Ordering::Relaxed),
            "models": crate::console::MODELS.iter().map(|m| json!({"id": m.0, "name": m.1, "mode": m.2})).collect::<Vec<_>>(),
        });
        crate::features::status(self, target.as_deref(), &mut s);
        s
    }
}

fn save_video_json(v: &Value) {
    let saved: serde_json::Map<String, Value> = crate::console::VIDEO_KEYS.iter()
        .filter_map(|k| v.get(*k).map(|x| (k.to_string(), x.clone()))).collect();
    let _ = write_json(&state_dir().join("video.json"), &Value::Object(saved));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench() -> Arc<Bench> {
        Bench::new(Config::from_raw(None, json!({"input": {"backend": "none"}, "video": {"backend": "none"}, "audio": {"enabled": false}})))
    }

    #[tokio::test]
    async fn requests_reach_the_named_console_or_the_controllers() {
        let b = bench();
        b.add_console(&json!({"name": "Switch 1", "model": "switch1", "video": {"device": "card-2"}})).unwrap();
        // no name: the console the controller is on
        assert_eq!(b.target().unwrap().id, "switch-2");
        // by id, or by name in any case
        let one = crate::console::SELECTED.scope(Some("switch 1".into()), async { b.target().unwrap().id.clone() }).await;
        assert_eq!(one, "switch-1");
        let none = crate::console::SELECTED.scope(Some("nope".into()), async { b.target().is_err() }).await;
        assert!(none);
    }

    #[tokio::test]
    async fn input_only_reaches_the_console_with_the_controller() {
        let b = bench();
        b.add_console(&json!({"name": "Switch 1", "model": "switch1", "video": {"device": "card-2"}})).unwrap();
        assert!(crate::console::SELECTED.scope(Some("switch-2".into()), async { b.check_input_target().is_ok() }).await);
        let elsewhere = crate::console::SELECTED.scope(Some("switch-1".into()), async { b.check_input_target() }).await;
        assert!(elsewhere.unwrap_err().to_string().contains("plugged into Switch 2"));
        // the cable moved: unknown, input paused (letting go still works)
        b.set_controller_on(None).unwrap();
        assert!(b.check_input_target().is_err());
        assert!(b.hub.paused.lock().unwrap().is_some());
        b.set_controller_on(Some("switch-1")).unwrap();
        assert!(b.hub.paused.lock().unwrap().is_none());
        assert_eq!(b.controller_console().unwrap().id, "switch-1");
    }

    #[tokio::test]
    async fn consoles_are_named_and_cards_are_not_shared() {
        let b = bench();
        b.add_console(&json!({"name": "Switch 1", "model": "switch1", "video": {"device": "card-2"}})).unwrap();
        assert!(b.add_console(&json!({"name": "switch 1", "video": {"device": "card-3"}})).is_err(), "names are unique, any case");
        // only a name is needed; without a card it doesn't take the first console's
        let later = b.add_console(&json!({"name": "No card"})).unwrap();
        assert!(later.video_cfg.lock().unwrap()["device"].is_null());
        assert_eq!(*later.color.lock().unwrap(), "#0ab9e6");
        b.remove_console("no-card").unwrap();
        assert!(b.add_console(&json!({"name": "Spare", "video": {"device": "card-2"}})).err().unwrap().to_string().contains("already used by Switch 1"));
        b.update_console("switch-1", &json!({"name": "Switch 1 OLED", "color": "#123abc", "network_ip": "192.168.86.252"})).unwrap();
        assert_eq!(b.console("switch 1 oled").unwrap().network_ip.lock().unwrap().as_deref(), Some("192.168.86.252"));
        assert!(b.update_console("switch-1", &json!({"network_ip": "nope"})).is_err());
        // the controller's console goes away: where it is becomes unknown
        b.set_controller_on(Some("switch-1")).unwrap();
        b.remove_console("switch-1").unwrap();
        assert!(b.controller_console().is_none());
        // (one console left: nothing to ask, input isn't paused)
        assert!(b.check_input_target().is_ok());
        assert!(b.remove_console("switch-2").is_err(), "the last console stays");
    }

    #[tokio::test]
    async fn a_run_started_before_the_controller_moved_presses_nothing_after() {
        let b = bench();
        b.add_console(&json!({"name": "Switch 1", "model": "switch1", "video": {"device": "card-2"}})).unwrap();
        let started = b.hub.generation.load(std::sync::atomic::Ordering::SeqCst);
        b.set_controller_on(Some("switch-1")).unwrap();
        let r = crate::inputs::RUN_GENERATION.scope(Some(started), async { b.hub.hold(&["A".into()], "api") }).await;
        assert!(r.unwrap_err().to_string().contains("moved to another console"));
    }

    #[tokio::test]
    async fn the_shared_controller_follows_the_console_on_screen_but_not_out_of_a_switch2() {
        let b = bench();
        b.add_console(&json!({"name": "Switch 1", "model": "switch1"})).unwrap();
        b.set_controller_on(Some("switch-2")).unwrap();
        assert!(b.follow_view("switch-1").unwrap(), "moved to the console on screen");
        assert_eq!(b.controller_console().unwrap().id, "switch-1");
        assert!(!b.follow_view("switch-1").unwrap(), "already there");
        // the board says a Switch 2 is driving it: it can't be in the Switch 1
        b.set_controller_on(Some("switch-2")).unwrap();
        b.hub.backend.status.lock().unwrap().host = Some("switch2".into());
        assert!(b.follow_view("switch-1").unwrap_err().to_string().contains("still plugged into Switch 2"));
        assert_eq!(b.controller_console().unwrap().id, "switch-2");
        // a console that doesn't use the shared controller, or following turned off: nothing moves
        b.hub.backend.status.lock().unwrap().host = None;
        b.update_console("switch-1", &json!({"uses_controller": false})).unwrap();
        assert!(!b.follow_view("switch-1").unwrap());
    }

    #[tokio::test]
    async fn its_own_mode_switch_is_not_a_cable_move_and_a_choice_is_remembered() {
        let b = bench();
        assert!(!b.hub.switching_mode());
        let _ = b.hub.switch_usb_mode("standard");     // (no board in tests: only the timing matters)
        assert!(b.hub.switching_mode(), "the board dropping off right after is the switch, not the cable");
        b.add_console(&json!({"name": "Switch 1", "model": "switch1"})).unwrap();
        assert!(b.hub.placed.lock().unwrap().is_none());
        b.set_controller_on(Some("switch-1")).unwrap();
        assert!(b.hub.placed.lock().unwrap().is_some(), "said where it is: a board back from restarting won't ask again");
    }

    #[tokio::test]
    async fn a_removed_console_stops_everything_it_ran() {
        let b = bench();
        let c = b.add_console(&json!({"name": "Switch 1", "model": "switch1", "video": {"device": "card-2"}})).unwrap();
        assert!(!c.tasks.lock().unwrap().is_empty(), "its recorder, sound and watchers run");
        b.remove_console("switch-1").unwrap();
        assert!(c.stopped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(c.tasks.lock().unwrap().is_empty());
        // a late settings change can't start its capture again
        b.restart_video(&c);
    }
}
