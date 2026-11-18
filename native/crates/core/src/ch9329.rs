//! CH9329 serial-to-USB-HID module: a USB keyboard + mouse for the console from a $3 chip on a
//! USB-UART adapter. No controller (the chip has no gamepad mode), so only "kbd" and "mouse".
//! Config: "input": {"backend": "ch9329", "serial_port": "/dev/cu.usbserial-XXXX", "baud": 9600}
//! Frame: 57 AB <addr 00> <cmd> <len> <data...> <sum of all previous bytes>. Replies set cmd|0x80.

use crate::inputs::{BackendStatus, Kbd, Msg, NEUTRAL_KBD};
use crate::logs::LogHub;
use anyhow::{bail, Result};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// One write to the chip, at most 2 s: a wedged adapter must not freeze the loop (no heartbeat,
/// reconnect or Stop would get through).
async fn put<W: tokio::io::AsyncWrite + Unpin>(w: &mut W, data: &[u8]) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), w.write_all(data)).await
        .map_err(|_| anyhow::anyhow!("the CH9329 took no data for 2 s"))??;
    Ok(())
}

pub const CMD_GET_INFO: u8 = 0x01;
pub const CMD_KEYBOARD: u8 = 0x02;
pub const CMD_MOUSE_REL: u8 = 0x05;

pub fn frame(cmd: u8, data: &[u8]) -> Vec<u8> {
    let mut f = vec![0x57, 0xAB, 0x00, cmd, data.len() as u8];
    f.extend_from_slice(data);
    let sum = f.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    f.push(sum);
    f
}

pub fn keyboard(k: &Kbd) -> Vec<u8> {
    frame(CMD_KEYBOARD, &[k[0], 0, k[1], k[2], k[3], k[4], k[5], k[6]])
}

/// Relative mouse frames for a move of any size (the chip takes -127..127 per report).
pub fn mouse(buttons: u8, dx: i32, dy: i32, wheel: i32) -> Vec<Vec<u8>> {
    let (mut dx, mut dy, mut wh) = (dx, dy, wheel);
    let mut out = vec![];
    loop {
        let (sx, sy, sw) = (dx.clamp(-127, 127), dy.clamp(-127, 127), wh.clamp(-127, 127));
        out.push(frame(CMD_MOUSE_REL, &[0x01, buttons, sx as i8 as u8, sy as i8 as u8, sw as i8 as u8]));
        dx -= sx;
        dy -= sy;
        wh -= sw;
        if dx == 0 && dy == 0 && wh == 0 {
            return out;
        }
    }
}

/// Parse one reply frame from the front of `buf`; returns (cmd, data) and drops it from buf.
pub fn take_reply(buf: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    loop {
        let Some(start) = buf.windows(2).position(|w| w == [0x57, 0xAB]) else {
            // No frame start in there: keep only a possible first half of one, not the noise.
            let keep = usize::from(buf.last() == Some(&0x57));
            buf.drain(..buf.len() - keep);
            return None;
        };
        buf.drain(..start);
        if buf.len() < 6 {
            return None;
        }
        let len = buf[4] as usize;
        if buf.len() < 6 + len {
            return None;
        }
        let sum = buf[..5 + len].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        if sum != buf[5 + len] {
            buf.drain(..2); // bad frame: resync after the header
            continue;
        }
        let f: Vec<u8> = buf.drain(..6 + len).collect();
        return Some((f[3], f[5..5 + len].to_vec()));
    }
}

pub async fn run(
    link: crate::transport::Link,
    mut rx: mpsc::UnboundedReceiver<Msg>,
    status: Arc<Mutex<BackendStatus>>,
    logs: LogHub,
    on_connect: mpsc::UnboundedSender<()>,
) {
    let mut delay = Duration::from_secs(1);
    loop {
        let res: Result<()> = async {
            let (mut r, mut w) = link.connect().await?;
            let mut buf = vec![];
            let mut chunk = [0u8; 256];
            // the chip answers GET_INFO with: version, USB enumerated (1), lock LEDs
            put(&mut w, &frame(CMD_GET_INFO, &[])).await?;
            let t0 = Instant::now();
            let info = loop {
                // One deadline for the whole answer: another device chattering on this port
                // must not keep the handshake waiting forever.
                let left = Duration::from_secs(3).saturating_sub(t0.elapsed());
                let n = tokio::time::timeout(left, r.read(&mut chunk)).await
                    .map_err(|_| anyhow::anyhow!("no answer from the CH9329 (check port, wiring and baud)"))??;
                if n == 0 { bail!("serial port closed"); }
                buf.extend_from_slice(&chunk[..n]);
                if let Some((cmd, d)) = take_reply(&mut buf) {
                    if cmd == CMD_GET_INFO | 0x80 { break d; }
                }
            };
            {
                let mut s = status.lock().unwrap();
                s.connected = true;
                s.detail = None;
                s.rtt_ms = Some((t0.elapsed().as_secs_f64() * 10000.0).round() / 10.0);
                s.usb = Some(if info.get(1) == Some(&1) { "configured" } else { "not attached" }.into());
                s.usb_mode = Some("ch9329".into());
                s.devices = vec!["kbd".into(), "mouse".into()];
                logs.add("bench", format!("CH9329 connected: {} chip v{:#04x} usb={}", link.describe(), info.first().copied().unwrap_or(0),
                    s.usb.clone().unwrap_or_default()));
            }
            delay = Duration::from_secs(1);
            // The chip keeps its last report across a host crash: always start neutral.
            put(&mut w, &keyboard(&NEUTRAL_KBD)).await?;
            for f in mouse(0, 0, 0, 0) {
                put(&mut w, &f).await?;
            }
            let _ = on_connect.send(());
            let mut last_kbd = NEUTRAL_KBD;
            let mut pad_warned = false;
            let mut beat = tokio::time::interval(Duration::from_secs(1));
            let mut ping_at: Option<Instant> = None;
            let mut writes = 0u64;
            // Mouse moves that queue up while one is being sent (9600 baud is ~85 frames a second)
            // are merged into one: the pointer keeps up instead of lagging further behind.
            let mut pending: Option<Msg> = None;
            loop {
                let m = if let Some(p) = pending.take() { p } else {
                    tokio::select! {
                        m = rx.recv() => {
                            let Some(m) = m else {
                                // replaced without a Stop: still leave the chip neutral
                                let _ = put(&mut w, &keyboard(&NEUTRAL_KBD)).await;
                                for f in mouse(0, 0, 0, 0) { let _ = put(&mut w, &f).await; }
                                return Ok(());
                            };
                            m
                        }
                        _ = beat.tick() => {
                            if ping_at.is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
                                bail!("CH9329 stopped answering");
                            }
                            if ping_at.is_none() {
                                ping_at = Some(Instant::now());
                                put(&mut w, &frame(CMD_GET_INFO, &[])).await?;
                            }
                            status.lock().unwrap().writes = [("kbd".to_string(), writes)].into_iter().collect();
                            continue;
                        }
                        n = r.read(&mut chunk) => {
                            let n = n?;
                            if n == 0 { bail!("serial port closed"); }
                            buf.extend_from_slice(&chunk[..n]);
                            while let Some((cmd, d)) = take_reply(&mut buf) {
                                if cmd == CMD_GET_INFO | 0x80 {
                                    let mut s = status.lock().unwrap();
                                    if let Some(t) = ping_at.take() { s.rtt_ms = Some((t.elapsed().as_secs_f64() * 10000.0).round() / 10.0); }
                                    s.usb = Some(if d.get(1) == Some(&1) { "configured" } else { "not attached" }.into());
                                } else if cmd & 0xC0 == 0xC0 {
                                    logs.error("board", format!("CH9329 error reply to {:#04x}: {:?}", cmd & 0x3F, d));
                                }
                            }
                            continue;
                        }
                    }
                };
                match m {
                    Msg::State(p, k) => {
                        if p != crate::inputs::NEUTRAL_PAD && !pad_warned {
                            logs.error("bench", "CH9329 has no controller: buttons/sticks are ignored (keyboard and mouse work)");
                            pad_warned = true;
                        }
                        if k != last_kbd {
                            put(&mut w, &keyboard(&k)).await?;
                            last_kbd = k;
                            writes += 1;
                        }
                    }
                    Msg::Mouse(v) => {
                        let (mut dx, mut dy, mut wh) = (v[1], v[2], v[3]);
                        loop {
                            match rx.try_recv() {
                                Ok(Msg::Mouse(n)) if n[0] == v[0] => { dx += n[1]; dy += n[2]; wh += n[3]; }
                                Ok(other) => { pending = Some(other); break; }
                                Err(_) => break,
                            }
                        }
                        for f in mouse(v[0] as u8, dx, dy, wh) {
                            put(&mut w, &f).await?;
                            writes += 1;
                        }
                    }
                    Msg::Stop(done) => {
                        put(&mut w, &keyboard(&NEUTRAL_KBD)).await?;
                        for f in mouse(0, 0, 0, 0) { put(&mut w, &f).await?; }
                        let _ = done.send(());
                        return Ok(());
                    }
                    Msg::UsbMode(..) => logs.error("bench", "CH9329 has a single USB mode (keyboard + mouse)"),
                }
            }
        }.await;
        if res.is_ok() { return; }
        let was = { let mut s = status.lock().unwrap(); let w = s.connected; s.connected = false; w };
        let detail = match res { Ok(()) => "stopped".to_string(), Err(e) => e.to_string() };
        if was || status.lock().unwrap().detail.as_deref() != Some(detail.as_str()) {
            logs.error("bench", format!("CH9329: {detail}"));
        }
        status.lock().unwrap().detail = Some(detail);
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames() {
        // documented example: GET_INFO = 57 AB 00 01 00 03
        assert_eq!(frame(CMD_GET_INFO, &[]), vec![0x57, 0xAB, 0x00, 0x01, 0x00, 0x03]);
        // keyboard "a" pressed: 57 AB 00 02 08 00 00 04 00 00 00 00 00 10
        assert_eq!(keyboard(&[0, 4, 0, 0, 0, 0, 0]), vec![0x57, 0xAB, 0x00, 0x02, 0x08, 0, 0, 4, 0, 0, 0, 0, 0, 0x10]);
        let m = mouse(1, 300, -5, 0);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0][5..10], [0x01, 0x01, 127, (-5i8) as u8, 0]);
        assert_eq!(m[2][7], 46); // 300 - 127 - 127
        let mut buf = vec![0xFF, 0x57, 0xAB, 0x00, 0x81, 0x03, 0x30, 0x01, 0x00];
        let s = buf[1..].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        buf.push(s);
        assert_eq!(take_reply(&mut buf), Some((0x81, vec![0x30, 0x01, 0x00])));
        assert!(buf.is_empty());
    }
}
