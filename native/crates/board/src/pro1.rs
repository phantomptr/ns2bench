//! Switch 1 Pro Controller (057e:2009) over USB, on a plain HID gadget (/dev/hidg0).
//!
//! The console starts it with USB commands (output report 0x80: 01 status, 02 handshake, 03 fast
//! link, 04 "HID only" = start reporting), then asks subcommands (output report 0x01: device
//! info, SPI flash reads for stick calibration and colours, lights, IMU and vibration on/off),
//! each answered with a 0x21 report. Then a 0x30 input report every 8 ms carries the buttons and
//! sticks. Protocol: the community reverse-engineering notes (dekuNukem,
//! Nintendo_Switch_Reverse_Engineering), which list these reports and the SPI layout.

use std::collections::VecDeque;
use std::time::Duration;

pub const REPORT: Duration = Duration::from_millis(8);

/// The bench's pad state ([buttons, hat, lx, ly, rx, ry], see core::inputs) as the Pro
/// Controller's three button bytes and two 12-bit sticks.
pub fn buttons(s: &[i64]) -> [u8; 3] {
    let b = s[0] as u32;
    let bit = |n: u32| (b >> n) & 1 == 1;
    let mut right = 0u8;
    if bit(0) { right |= 0x01 } // Y
    if bit(3) { right |= 0x02 } // X
    if bit(1) { right |= 0x04 } // B
    if bit(2) { right |= 0x08 } // A
    if bit(5) { right |= 0x40 } // R
    if bit(7) { right |= 0x80 } // ZR
    let mut shared = 0u8;
    if bit(8) { shared |= 0x01 } // Minus
    if bit(9) { shared |= 0x02 } // Plus
    if bit(11) { shared |= 0x04 } // right stick press
    if bit(10) { shared |= 0x08 } // left stick press
    if bit(12) { shared |= 0x10 } // HOME
    if bit(13) { shared |= 0x20 } // Capture
    let mut left = 0u8;
    // hat: 0 up, 1 up-right, 2 right, 3 down-right, 4 down, 5 down-left, 6 left, 7 up-left, 8 none
    let (up, right_d, down, left_d) = match s[1] {
        0 => (true, false, false, false), 1 => (true, true, false, false), 2 => (false, true, false, false),
        3 => (false, true, true, false), 4 => (false, false, true, false), 5 => (false, false, true, true),
        6 => (false, false, false, true), 7 => (true, false, false, true), _ => (false, false, false, false),
    };
    if down { left |= 0x01 }
    if up { left |= 0x02 }
    if right_d { left |= 0x04 }
    if left_d { left |= 0x08 }
    if bit(4) { left |= 0x40 } // L
    if bit(6) { left |= 0x80 } // ZL
    [right, shared, left]
}

/// A stick (0..255 each, 128 centre, y down) as the controller's packed 12-bit x/y (y up).
pub fn stick(x: i64, y: i64) -> [u8; 3] {
    let x12 = (x.clamp(0, 255) as u32 * 4095 / 255) as u16;
    let y12 = 4095 - (y.clamp(0, 255) as u32 * 4095 / 255) as u16;
    [(x12 & 0xFF) as u8, ((x12 >> 8) as u8) | (((y12 & 0x0F) as u8) << 4), (y12 >> 4) as u8]
}

/// The controller's SPI flash, as far as the console reads it: factory calibration and colours;
/// everything else reads as erased (0xFF).
fn spi(addr: u32, len: usize) -> Vec<u8> {
    const BLOCKS: &[(u32, &[u8])] = &[
        // factory IMU calibration
        (0x6020, &[0xD3, 0xFF, 0xD5, 0xFF, 0x55, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x40,
                   0x19, 0x00, 0xDD, 0xFF, 0xDC, 0xFF, 0x3B, 0x34, 0x3B, 0x34, 0x3B, 0x34]),
        // factory stick calibration: left (max above, centre, min below), right (centre, min, max)
        (0x603D, &[0x00, 0x07, 0x70, 0x00, 0x08, 0x80, 0x00, 0x07, 0x70,
                   0x00, 0x08, 0x80, 0x00, 0x07, 0x70, 0x00, 0x07, 0x70]),
        // colours: body, buttons, left grip, right grip
        (0x6050, &[0x32, 0x32, 0x32, 0xFF, 0xFF, 0xFF, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32]),
        // stick parameters (dead zone, range) for each stick
        (0x6080, &[0x50, 0xFD, 0x00, 0x00, 0xC6, 0x0F, 0x0F, 0x30, 0x61, 0x96, 0x30, 0xF3,
                   0xD4, 0x14, 0x54, 0x41, 0x15, 0x54, 0xC7, 0x79, 0x9C, 0x33, 0x36, 0x63]),
        (0x6098, &[0x0F, 0x30, 0x61, 0x96, 0x30, 0xF3, 0xD4, 0x14, 0x54, 0x41, 0x15, 0x54,
                   0xC7, 0x79, 0x9C, 0x33, 0x36, 0x63]),
    ];
    (0..len as u32).map(|i| {
        let a = addr + i;
        BLOCKS.iter().find_map(|(base, data)| (a >= *base && ((a - base) as usize) < data.len()).then(|| data[(a - base) as usize]))
            .unwrap_or(0xFF)
    }).collect()
}

pub struct Pro1 {
    /// the console sent 80 04 (or asked anything): report the buttons every 8 ms
    pub started: bool,
    timer: u8,
    mac: [u8; 6],
    pub replies: VecDeque<Vec<u8>>,
}

impl Pro1 {
    pub fn new(mac: [u8; 6]) -> Pro1 {
        Pro1 { started: false, timer: 0, mac, replies: VecDeque::new() }
    }

    fn pad64(mut v: Vec<u8>) -> Vec<u8> {
        v.resize(64, 0);
        v
    }

    /// The 0x30 report: timer, battery/connection, buttons, sticks, vibration byte, IMU (zero).
    pub fn input(&mut self, s: &[i64]) -> Vec<u8> {
        self.timer = self.timer.wrapping_add(1);
        let mut v = vec![0x30, self.timer, 0x91];
        v.extend(buttons(s));
        v.extend(stick(s[2], s[3]));
        v.extend(stick(s[4], s[5]));
        v.push(0x0B);
        Self::pad64(v)
    }

    fn reply21(&mut self, s: &[i64], ack: u8, sub: u8, data: &[u8]) {
        self.timer = self.timer.wrapping_add(1);
        let mut v = vec![0x21, self.timer, 0x91];
        v.extend(buttons(s));
        v.extend(stick(s[2], s[3]));
        v.extend(stick(s[4], s[5]));
        v.push(0x0B);
        v.push(ack);
        v.push(sub);
        v.extend(data);
        self.replies.push_back(Self::pad64(v));
    }

    /// One output report from the console. Returns what to log, if anything worth it.
    pub fn command(&mut self, r: &[u8], s: &[i64]) -> Option<String> {
        match r.first() {
            Some(0x80) => {
                let cmd = *r.get(1)?;
                match cmd {
                    0x01 => {
                        let mut v = vec![0x81, 0x01, 0x00, 0x03];
                        v.extend(self.mac.iter().rev());
                        self.replies.push_back(Self::pad64(v));
                    }
                    0x02 | 0x03 => self.replies.push_back(Self::pad64(vec![0x81, cmd])),
                    0x04 => { self.started = true; return Some("console started the Pro Controller (USB, Switch 1 protocol)".into()) }
                    0x05 => self.started = false,
                    _ => {}
                }
                None
            }
            Some(0x01) if r.len() >= 11 => {
                self.started = true;
                let sub = r[10];
                match sub {
                    0x02 => {
                        let mut d = vec![0x03, 0x8B, 0x03, 0x02];
                        d.extend(self.mac);
                        d.extend([0x01, 0x01]);
                        self.reply21(s, 0x82, sub, &d);
                    }
                    0x10 => {
                        let addr = u32::from_le_bytes([r[11], r[12], r[13], r[14]]);
                        let len = (r[15] as usize).min(0x1D);
                        let mut d = r[11..16].to_vec();
                        d.extend(spi(addr, len));
                        self.reply21(s, 0x90, sub, &d);
                    }
                    0x04 => self.reply21(s, 0x83, sub, &[0; 14]),
                    0x01 => self.reply21(s, 0x81, sub, &[0x03]),
                    0x21 => self.reply21(s, 0xA0, sub, &[0x01, 0x00, 0xFF, 0x00, 0x03, 0x00, 0x05, 0x01]),
                    // input mode, lights, HOME light, IMU, vibration, low power...: acknowledged
                    _ => self.reply21(s, 0x80, sub, &[]),
                }
                Some(format!("subcommand {sub:02x}"))
            }
            _ => None, // 0x10: vibration only
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_and_sticks() {
        // A + HOME + up-right on the d-pad + L
        let s = [(1 << 2) | (1 << 12) | (1 << 4), 1, 128, 128, 255, 0];
        assert_eq!(buttons(&s), [0x08, 0x10, 0x02 | 0x04 | 0x40]);
        // centre is close to 2048 both ways; full right/up is 4095
        let c = stick(128, 128);
        let x = c[0] as u16 | ((c[1] as u16 & 0x0F) << 8);
        let y = (c[1] as u16 >> 4) | ((c[2] as u16) << 4);
        assert!((2040..=2060).contains(&x) && (2030..=2060).contains(&y), "{x} {y}");
        assert_eq!(stick(255, 0), [0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn start_up_and_subcommands() {
        let n = [0, 8, 128, 128, 128, 128];
        let mut p = Pro1::new([1, 2, 3, 4, 5, 6]);
        p.command(&[0x80, 0x01], &n);
        assert_eq!(&p.replies.pop_front().unwrap()[..10], &[0x81, 0x01, 0x00, 0x03, 6, 5, 4, 3, 2, 1]);
        p.command(&[0x80, 0x02], &n);
        assert_eq!(&p.replies.pop_front().unwrap()[..2], &[0x81, 0x02]);
        assert!(!p.started);
        p.command(&[0x80, 0x04], &n);
        assert!(p.started);
        // SPI read of the left stick calibration
        let mut r = vec![0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10];
        r.extend([0x3D, 0x60, 0, 0, 9]);
        p.command(&r, &n);
        let rep = p.replies.pop_front().unwrap();
        assert_eq!(rep[0], 0x21);
        assert_eq!(&rep[13..15], &[0x90, 0x10]);
        assert_eq!(&rep[15..20], &[0x3D, 0x60, 0, 0, 9]);
        assert_eq!(&rep[20..29], &[0x00, 0x07, 0x70, 0x00, 0x08, 0x80, 0x00, 0x07, 0x70]);
        assert_eq!(rep.len(), 64);
        // unknown memory reads as erased
        assert_eq!(spi(0x8010, 4), vec![0xFF; 4]);
        let i = p.input(&n);
        assert_eq!((i[0], i.len()), (0x30, 64));
    }
}
