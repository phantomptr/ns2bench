//! Which console is the controller plugged into? (docs/design/multi-console.md)
//!
//! Only a Switch 2 starts a Pro Controller: it sends the "input reports on" command, and the
//! board reports that as `host: "switch2"`. So, while the controller's place is unknown and the
//! bench has exactly one Switch 2 and exactly one other console:
//!
//! - Pro mode and the Switch 2 started it: it's on the Switch 2.
//! - Pro mode, a console is there (the board's USB is configured, addressed or default) and none
//!   started it for [`NO_HANDSHAKE`]: it's not a Switch 2, so it's on the other console.
//! - Standard mode with a console there: become a Pro Controller for a moment to find out (no
//!   input is ever sent; input is paused while the place is unknown).
//! - A console asleep ("suspended"), the board's USB not attached, or the board away: wait. It
//!   never guesses, and after [`GIVE_UP`] it stops and leaves the question to the person.
//!
//! The person can answer at any time; their answer ends detection.

use std::time::{Duration, Instant};

/// A console that was there this long in Pro mode without starting the controller is not a Switch 2.
pub const NO_HANDSHAKE: Duration = Duration::from_secs(15);
/// After switching to Pro to find out: this long with no Switch 2 handshake (while not asleep)
/// means it's the other console, even if the console never showed up in Pro mode.
pub const AFTER_PROBE: Duration = Duration::from_secs(30);
pub const GIVE_UP: Duration = Duration::from_secs(120);

/// What the bridge says now.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub connected: bool,
    pub mode: Option<String>,
    pub usb: String,
    pub switch2: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    /// It's on the Switch 2 (true) or on the other console (false), and why.
    Found { switch2: bool, why: &'static str },
    /// Become a Pro Controller to find out.
    TryPro,
    GiveUp,
}

pub struct Detector {
    started: Instant,
    /// When a console was first seen there in Pro mode (reset while it sleeps or isn't there).
    there_in_pro: Option<Instant>,
    probed: Option<Instant>,
}

fn console_there(usb: &str) -> bool {
    matches!(usb, "configured" | "addressed" | "default")
}

impl Detector {
    pub fn new(now: Instant) -> Self {
        Detector { started: now, there_in_pro: None, probed: None }
    }

    pub fn probing(&self) -> bool {
        self.probed.is_some()
    }

    pub fn step(&mut self, now: Instant, s: &Seen) -> Step {
        if now.duration_since(self.started) >= GIVE_UP {
            return Step::GiveUp;
        }
        if !s.connected {
            return Step::Wait;
        }
        if s.switch2 {
            return Step::Found { switch2: true, why: "the Switch 2 started it as its Pro Controller" };
        }
        let asleep = s.usb == "suspended";
        match s.mode.as_deref() {
            Some("pro") => {
                if asleep {
                    self.there_in_pro = None;
                    return Step::Wait;
                }
                if console_there(&s.usb) {
                    let since = *self.there_in_pro.get_or_insert(now);
                    if now.duration_since(since) >= NO_HANDSHAKE {
                        return Step::Found { switch2: false, why: "the console there didn't start a Pro Controller, which a Switch 2 does" };
                    }
                } else {
                    self.there_in_pro = None;
                    // We saw a console there in standard mode and switched: a Switch 2 would have
                    // started the Pro Controller by now.
                    if self.probed.is_some_and(|p| now.duration_since(p) >= AFTER_PROBE) {
                        return Step::Found { switch2: false, why: "the console there accepted the standard controller but not the Pro Controller" };
                    }
                }
                Step::Wait
            }
            Some(_) if console_there(&s.usb) && self.probed.is_none() => {
                self.probed = Some(now);
                Step::TryPro
            }
            _ => Step::Wait,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(mode: &str, usb: &str, switch2: bool) -> Seen {
        Seen { connected: true, mode: Some(mode.into()), usb: usb.into(), switch2 }
    }

    #[test]
    fn a_switch2_handshake_decides_at_once() {
        let t = Instant::now();
        let mut d = Detector::new(t);
        assert_eq!(d.step(t, &seen("pro", "configured", true)), Step::Found { switch2: true, why: "the Switch 2 started it as its Pro Controller" });
    }

    #[test]
    fn pro_without_a_handshake_is_the_other_console_after_15_s_only() {
        let t = Instant::now();
        let mut d = Detector::new(t);
        assert_eq!(d.step(t, &seen("pro", "configured", false)), Step::Wait);
        assert_eq!(d.step(t + Duration::from_secs(14), &seen("pro", "configured", false)), Step::Wait);
        assert!(matches!(d.step(t + Duration::from_secs(15), &seen("pro", "configured", false)), Step::Found { switch2: false, .. }));
    }

    #[test]
    fn a_sleeping_console_is_never_judged() {
        let t = Instant::now();
        let mut d = Detector::new(t);
        assert_eq!(d.step(t, &seen("pro", "configured", false)), Step::Wait);
        // it fell asleep: the clock starts again when it's back
        assert_eq!(d.step(t + Duration::from_secs(10), &seen("pro", "suspended", false)), Step::Wait);
        assert_eq!(d.step(t + Duration::from_secs(20), &seen("pro", "configured", false)), Step::Wait);
        assert_eq!(d.step(t + Duration::from_secs(34), &seen("pro", "configured", false)), Step::Wait);
        assert!(matches!(d.step(t + Duration::from_secs(35), &seen("pro", "configured", false)), Step::Found { switch2: false, .. }));
    }

    #[test]
    fn standard_with_a_console_tries_pro_once_then_decides() {
        let t = Instant::now();
        let mut d = Detector::new(t);
        assert_eq!(d.step(t, &seen("standard", "configured", false)), Step::TryPro);
        // the switch is under way: still standard, then the board is away, then pro
        assert_eq!(d.step(t + Duration::from_secs(1), &seen("standard", "configured", false)), Step::Wait);
        assert_eq!(d.step(t + Duration::from_secs(5), &Seen::default()), Step::Wait);
        // a Switch 2 would start it; this console never even shows up in pro mode
        assert_eq!(d.step(t + Duration::from_secs(12), &seen("pro", "not attached", false)), Step::Wait);
        assert!(matches!(d.step(t + Duration::from_secs(30), &seen("pro", "not attached", false)), Step::Found { switch2: false, .. }));
    }

    #[test]
    fn standard_then_a_switch2_handshake() {
        let t = Instant::now();
        let mut d = Detector::new(t);
        assert_eq!(d.step(t, &seen("standard", "configured", false)), Step::TryPro);
        assert!(matches!(d.step(t + Duration::from_secs(12), &seen("pro", "configured", true)), Step::Found { switch2: true, .. }));
    }

    #[test]
    fn nothing_there_waits_then_gives_up() {
        let t = Instant::now();
        let mut d = Detector::new(t);
        for s in [0, 30, 60, 119] {
            assert_eq!(d.step(t + Duration::from_secs(s), &seen("standard", "not attached", false)), Step::Wait);
            assert_eq!(d.step(t + Duration::from_secs(s), &seen("pro", "not attached", false)), Step::Wait);
        }
        assert_eq!(d.step(t + GIVE_UP, &seen("pro", "not attached", false)), Step::GiveUp);
        // the board away is never a decision either
        let mut d = Detector::new(t);
        assert_eq!(d.step(t, &Seen::default()), Step::Wait);
    }
}
