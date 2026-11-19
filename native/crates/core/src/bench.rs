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
        // ...unless the controller follows the console picked on screen: then it's on that one.
        let follows = crate::console::FOLLOW_VIEW.load(std::sync::atomic::Ordering::Relaxed);
        let active = crate::console::active()
            .filter(|id| consoles.iter().any(|c| &c.id == id && c.uses_controller.load(std::sync::atomic::Ordering::Relaxed)));
        let controller_on = if follows && consoles.len() > 1 && !cfg!(test) { active } else {
            controller_on.filter(|id| consoles.iter().any(|c| &c.id == id)).filter(|_| consoles.len() < 2 || cfg!(test))
        };
        let bench = Arc::new(Bench {
            input_cfg: Mutex::new(cfg.input.clone()), settings_lock: tokio::sync::Mutex::new(()),
            ext: crate::features::Ext::new(&cfg),
            consoles: RwLock::new(consoles), controller_on: Mutex::new(controller_on), changes: Mutex::new(()), detecting: false.into(),
            cfg, logs, hub,
            active_operations: 0.into(), screen_log: screen_log.into(), calibration: Mutex::new(calibration), started: Instant::now(),
        });
        let h = bench.hub.clone();
        // After every connect and every finished mode switch: what is held is sent again, and
        // the watcher is told to check the board's mode (see `mode_check`).
        tokio::spawn(async move {
            while conn_rx.recv().await.is_some() {
                h.resend();
                h.mode_check.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });
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
        let on = self.controller_on.lock().unwrap().clone();
        match on {
            Some(on) => self.console(&on),
            // one console: the controller can only be on it (a saved "unknown" from when there
            // were two must not leave the last console without its mode or its settings)
            None => { let l = self.list(); if l.len() == 1 { l.into_iter().next() } else { None } }
        }
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
        // a console that isn't there (removed since the agent picked it) is never "this one"
        if let Some(sel) = crate::console::selected() {
            if self.console(&sel).is_none() { bail!("no console named {sel:?}"); }
        }
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
        // one save at a time, each of the state as it is when it runs: two savers must not
        // write their snapshots in the opposite order they took them
        static SAVING: Mutex<()> = Mutex::new(());
        let _one = SAVING.lock().unwrap_or_else(|e| e.into_inner());
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
        // where the controller was really placed (by the person, by following, or found) is where
        // it's put again after the board restarts and at the next start
        if let Some(id) = &new { crate::console::set_active(Some(id.clone())); }
        let moved = *self.controller_on.lock().unwrap() != new;
        *self.controller_on.lock().unwrap() = new;
        // anything still running for the old place (a sequence, a test) stops pressing
        if moved { self.hub.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
        self.apply_pause();
        self.save_consoles()?;
        match &target {
            Some(c) => {
                *self.hub.placed.lock().unwrap() = Some(std::time::Instant::now());
                self.hub.placed_sure.store(true, std::sync::atomic::Ordering::SeqCst);
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
        if on.as_ref().is_some_and(|o| o.id == c.id) {
            // back to the console it is in: an earlier pick of another one is off, and the board
            // starts as what this console needs again
            if self.hub.pending_follow.lock().unwrap().take().is_some() { self.apply_mode(); }
            return Ok(false);
        }
        let st = self.hub.status();
        if st["host"] == "switch2" && *c.model.lock().unwrap() != "switch2" {
            let there = on.map(|o| o.name()).unwrap_or_else(|| "a Switch 2".into());
            bail!(crate::inputs::Conflict(format!("the controller is still plugged into {there} (it's driving it as its Pro Controller): move its cable to {} first, then pick it again", c.name())));
        }
        // The cable can't have moved: the board has been there, plugged in and running, ever since
        // the controller was placed on the other console. Putting it "on" this one would switch
        // its mode under the console it is really in, and send this console's presses there.
        // Remembered instead: when the cable is seen to move, the controller goes here by itself
        // (and the board is told to start as what this console needs, so that is quick).
        if let (Some(o), true) = (&on, self.cable_still_in()) {
            *self.hub.pending_follow.lock().unwrap() = Some((c.id.clone(), std::time::Instant::now()));
            let want = crate::console::mode_for(&c.model.lock().unwrap(), &c.controller.lock().unwrap());
            if !(st["usb_mode"] == "hori" && want == "standard") { let _ = self.hub.backend.send(crate::inputs::Msg::BootMode(want.to_string())); }
            self.logs.add("bench", format!("controller: {} picked on screen, but the cable is still in {}; it switches over when the cable is moved", c.name(), o.name()));
            bail!(crate::inputs::Conflict(format!("the controller is still plugged into {}: move its cable to {} and NS2Bench switches to it by itself", o.name(), c.name())));
        }
        *self.hub.pending_follow.lock().unwrap() = None;
        self.set_controller_on(Some(&c.id))?;
        // (taken on the person's word that the screen is where the cable is: not proof of it)
        self.hub.placed_sure.store(false, Relaxed);
        self.logs.add("bench", format!("controller: follows the console on screen: {}", c.name()));
        Ok(true)
    }

    /// Whether the controller is certainly still in the console it was placed on: the board is
    /// connected with its USB side running, and nothing since the placement could have been the
    /// cable moving (the board restarting, or its USB cable out and in).
    pub fn cable_still_in(&self) -> bool {
        let st = self.hub.status();
        if st["connected"] != true || !st["usb"].as_str().is_some_and(|u| u.starts_with("configured")) { return false; }
        let placed = *self.hub.placed.lock().unwrap();
        let event = *self.hub.cable_event.lock().unwrap();
        // (never placed in this run, e.g. taken from the last one at start: nothing is certain)
        self.hub.placed_sure.load(std::sync::atomic::Ordering::SeqCst) && placed.is_some_and(|p| event.is_none_or(|e| e <= p))
    }

    /// The cable was seen to move: the console picked on screen while it was still in another one
    /// (within the last few minutes), if any. Taken, so it is used once.
    pub fn take_pending_follow(&self) -> Option<Arc<Console>> {
        let (id, at) = self.hub.pending_follow.lock().unwrap().take()?;
        if at.elapsed() > std::time::Duration::from_secs(300) { return None; }
        self.console(&id).filter(|c| c.uses_controller.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// The console the controller follows to (the one picked on screen), when following is on.
    pub fn active_console(&self) -> Option<Arc<Console>> {
        use std::sync::atomic::Ordering::Relaxed;
        if !crate::console::FOLLOW_VIEW.load(Relaxed) { return None; }
        let id = crate::console::active()?;
        self.console(&id).filter(|c| c.uses_controller.load(Relaxed))
    }

    /// Reorder the consoles (the strip and Settings show them in this order).
    pub fn reorder(&self, ids: &[String]) -> Result<()> {
        let _one = self.changes.lock().unwrap();
        {
            let mut list = self.consoles.write().unwrap();
            if ids.len() != list.len() || !list.iter().all(|c| ids.contains(&c.id)) {
                bail!("the new order must list every console once");
            }
            list.sort_by_key(|c| ids.iter().position(|i| i == &c.id).unwrap_or(usize::MAX));
        }
        self.save_consoles()?;
        Ok(())
    }

    /// Put the board in the USB mode the controller's console needs, if it's connected and in
    /// another one.
    pub fn apply_mode(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        let Some(c) = self.controller_console() else { return };
        if self.hub.manual_mode.load(SeqCst) { return; }
        // (one switch at a time: asked again when this one has finished, see `mode_check`)
        if self.hub.backend.status.lock().unwrap().switching_until.is_some_and(|t| std::time::Instant::now() < t) { return; }
        // the console's own choice for its type (Pro Controller or controller + keyboard)
        let want = crate::console::mode_for(&c.model.lock().unwrap(), &c.controller.lock().unwrap());
        let st = self.hub.status();
        // Only a Bridge board has modes to switch between (a Pico or CH9329 is what it is: asking
        // would only refuse presses for the seconds a switch is waited for).
        if st["backend"] != "hidd" || !st["usb_mode"].as_str().is_some_and(|m| matches!(m, "standard" | "hori" | "pro" | "pro1")) { return; }
        // The board's own word wins: a Switch 2 is driving it as its Pro Controller. A console
        // set up as another type doesn't get to take that away.
        // (a Switch 2's own choice of controller + keyboard is its to make)
        if st["host"] == "switch2" && want != "pro" && st["usb_mode"] == "pro" && *c.model.lock().unwrap() != "switch2" { return; }
        // next time the board powers up (the console woke), it starts as what this console needs,
        // instead of starting as the other and switching over (seconds, and a reconnect)
        // (not over a hand-picked "controller + keyboard without mouse": it serves the same choice)
        if st["connected"] == true && !(st["usb_mode"] == "hori" && want == "standard") { let _ = self.hub.backend.send(crate::inputs::Msg::BootMode(want.to_string())); }
        let fits = |m: &str| m == want || (m == "hori" && want == "standard");
        if st["connected"] == true && st["usb_mode"].as_str().is_some_and(fits) {
            self.hub.mode_tries.store(0, SeqCst);       // there: later corrections start counting afresh
        }
        if st["connected"] == true && st["usb_mode"].as_str().is_some_and(|m| !fits(m)) {
            // (counted to 3 and no further: a counter that wrapped would start the tries again)
            let tries = self.hub.mode_tries.load(SeqCst);
            self.hub.mode_tries.store(tries.saturating_add(1).min(4), SeqCst);
            if tries >= 2 {
                if tries == 2 {
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
        // (the console there was before had the controller, said or not: adding a second one
        // must not make its place unknown and pause input)
        {
            let list = self.list();
            let mut on = self.controller_on.lock().unwrap();
            if list.len() == 1 && on.is_none() { *on = Some(list[0].id.clone()); }
            if crate::console::active().is_none() { crate::console::set_active(on.clone()); }
        }
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
        let mut new_choice = false;
        if let Some(ch) = v["controller"].as_str() {
            if ch != "pro" && ch != "keyboard" { bail!("controller must be pro or keyboard"); }
            new_choice = *c.controller.lock().unwrap() != ch;
            *c.controller.lock().unwrap() = ch.to_string();
        }
        let new_type = v.get("model").is_some();
        self.save_consoles()?;
        self.logs.add("bench", format!("console {} updated: {v}", c.id));
        // the controller is on this console: its new choice or type takes effect now
        if (new_choice || new_type) && self.controller_console().is_some_and(|o| o.id == c.id) {
            self.hub.manual_mode.store(false, std::sync::atomic::Ordering::SeqCst);
            self.hub.mode_tries.store(0, std::sync::atomic::Ordering::SeqCst);
            self.apply_mode();
        }
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
            // (one console left: the controller is its)
            let left = self.list();
            if left.len() == 1 { *on = Some(left[0].id.clone()); crate::console::set_active(on.clone()); }
        }
        self.apply_pause();
        self.save_consoles()?;
        self.apply_mode();          // (one console left: the board becomes what that one needs)
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
            if let Some(o) = v.as_object_mut() { o.remove("_device_default"); o.remove("_raw"); o.remove("_raw_tried"); }
            if self.list().first().is_some_and(|f| f.id == c.id) { save_video_json(&v); }
        }
        self.save_consoles()?;
        self.logs.add("bench", format!("video settings changed ({}): {changes}", c.name()));
        self.restart_video(c);
        Ok(())
    }

    pub fn status(&self) -> Value {
        let target = self.target().ok();
        // (as the engine sees it: with one console the controller is that console's)
        let on = self.controller_console().map(|c| c.id.clone());
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
            "power": {"cec": crate::power::cec_on()},
            "last_run_unclean": crate::report::unclean_previous_exists(),
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
        // (one console left: the controller can only be on it; nothing to ask, input isn't paused)
        assert_eq!(b.controller_console().unwrap().id, "switch-2");
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

    /// A board that answers (hello, pings) as a Bridge in pro mode with its USB side running.
    async fn board() -> u16 {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            let (r, mut w) = s.into_split();
            let mut lines = tokio::io::BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = serde_json::from_str(&line).unwrap_or_default();
                let reply = if v.get("hello").is_some() { Some(r#"{"ok":"ready","udc":"configured","devices":["pad"],"mode":"pro"}"#.to_string()) }
                    else { v["ping"].as_u64().map(|n| format!(r#"{{"pong":{n},"udc":"configured"}}"#)) };
                if let Some(reply) = reply { if w.write_all(format!("{reply}\n").as_bytes()).await.is_err() { break; } }
            }
        });
        port
    }

    #[tokio::test]
    async fn a_console_picked_while_the_cable_is_provably_elsewhere_waits_for_the_cable() {
        let port = board().await;
        let b = Bench::new(Config::from_raw(None, json!({"input": {"backend": "hidd", "host": "127.0.0.1", "port": port}, "video": {"backend": "none"}, "audio": {"enabled": false}})));
        for _ in 0..100 {
            if b.hub.status()["connected"] == true { break; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(b.hub.status()["connected"], true);
        b.add_console(&json!({"name": "Switch 1", "model": "switch1"})).unwrap();
        // nothing certain yet (the place was taken over from the last run): following is allowed
        assert!(!b.cable_still_in());
        // the person says where the cable is: from now on it is known to be there
        b.set_controller_on(Some("switch-2")).unwrap();
        assert!(b.cable_still_in());
        let refused = b.follow_view("switch-1").unwrap_err().to_string();
        assert!(refused.contains("still plugged into Switch 2"), "{refused}");
        assert_eq!(b.controller_console().unwrap().id, "switch-2", "presses keep going where the cable is");
        // picking the console it is in again takes the other pick back
        assert!(!b.follow_view("switch-2").unwrap());
        assert!(b.take_pending_follow().is_none());
        // picked again, then the cable is seen to move: that is where it went
        assert!(b.follow_view("switch-1").is_err());
        *b.hub.cable_event.lock().unwrap() = Some(std::time::Instant::now());
        assert!(!b.cable_still_in(), "the cable may have moved");
        assert_eq!(b.take_pending_follow().unwrap().id, "switch-1");
        assert!(b.take_pending_follow().is_none(), "used once");
        // a pick followed on the person's word alone is not proof for the next one
        b.set_controller_on(Some("switch-2")).unwrap();
        *b.hub.cable_event.lock().unwrap() = Some(std::time::Instant::now());
        assert!(b.follow_view("switch-1").unwrap());
        assert!(!b.cable_still_in());
        assert!(b.follow_view("switch-2").unwrap(), "a guess doesn't hold the controller in place");
    }

    #[tokio::test]
    async fn the_console_picked_on_screen_is_where_the_controller_is_and_consoles_reorder() {
        let b = bench();
        b.add_console(&json!({"name": "Switch 1", "model": "switch1"})).unwrap();
        b.follow_view("switch-1").unwrap();
        assert_eq!(b.active_console().unwrap().id, "switch-1", "remembered as the console picked");
        // the board restarting doesn't make it unknown: it's still on the console picked
        assert_eq!(b.controller_console().unwrap().id, "switch-1");
        b.reorder(&["switch-1".into(), "switch-2".into()]).unwrap();
        assert_eq!(b.list().iter().map(|c| c.id.clone()).collect::<Vec<_>>(), ["switch-1", "switch-2"]);
        assert!(b.reorder(&["switch-1".into()]).is_err(), "every console, once");
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
        // no board: a switch that can't be sent marks nothing (a board that's away isn't switching)
        let sent = b.hub.switch_usb_mode("standard").is_ok();
        assert_eq!(b.hub.switching_mode(), sent);
        // a switch under way: the board dropping off is the switch, not the cable, and presses wait
        *b.hub.mode_switched.lock().unwrap() = Some(std::time::Instant::now());
        b.hub.backend.status.lock().unwrap().switching_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(15));
        assert!(b.hub.switching_mode());
        assert!(b.hub.hold(&["A".into()], "api").unwrap_err().to_string().contains("changing mode"));
        // the Bridge says "switched": over at once
        b.hub.backend.status.lock().unwrap().switching_until = None;
        assert!(!b.hub.switching_mode());
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
