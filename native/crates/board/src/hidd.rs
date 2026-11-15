//! Network -> USB gamepad/keyboard/mouse. One authenticated TCP client at a time sends JSON lines;
//! each line carries the *whole* state, so a lost or duplicated message can never leave a button
//! stuck:
//!   {"hello": "<token>"}                     first line, required
//!   {"pad": [buttons, hat, lx, ly, rx, ry]}  17-bit mask, hat 0-8, axes 0-255
//!   {"kbd": [modifiers, k1, ..., k6]}        boot-keyboard usage codes
//!   {"mouse": [buttons, dx, dy, wheel]}      relative move (only if the mouse function exists)
//!   {"ping": n}                              -> {"pong": n, "udc", "writes"}
//!   {"logs": after_seq}                      -> {"logs": [[seq, time, source, text]], "seq": n}
//!   {"usb_mode": "standard"|"hori"|"pro"}    rebuild the gadget (detached) and restart
//! Safety: if the client is silent for 1 s, disconnects or errors, every device goes neutral and
//! is closed.

use crate::logbuf::{self, log};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixDatagram;
use std::time::{Duration, Instant};

const WATCHDOG: Duration = Duration::from_secs(1);
const KEEPALIVE: Duration = Duration::from_millis(500);
const MAX_LINE: usize = 4096;
const MODE_FILE: &str = "/etc/ns2bench-usb-mode";
const PRO_SOCK: &str = "/run/ns2bench-pro2.sock";
const GADGET_CFG: &str = "/sys/kernel/config/usb_gadget/rockchip/configs/b.1";
const PAD_NEUTRAL: [i64; 6] = [0, 8, 128, 128, 128, 128];
const KBD_NEUTRAL: [i64; 7] = [0; 7];

struct Args {
    bind: String,
    port: u16,
    token: String,
    pad: String,
    kbd: String,
    mouse: String,
    kmsg: String,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        bind: "192.168.79.2".into(), port: 7631, token: String::new(), pad: "/dev/hidg0".into(),
        kbd: "/dev/hidg1".into(), mouse: "/dev/hidg2".into(), kmsg: "/dev/kmsg".into(),
    };
    let mut token_file = "/opt/ns2bench/token".to_string();
    let mut it = argv.iter();
    while let Some(k) = it.next() {
        let mut v = || it.next().cloned().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--bind" => a.bind = v()?,
            "--port" => a.port = v()?.parse().map_err(|_| "bad --port")?,
            "--token-file" => token_file = v()?,
            "--pad" => a.pad = v()?,
            "--kbd" => a.kbd = v()?,
            "--mouse" => a.mouse = v()?,
            "--kmsg" => a.kmsg = v()?,
            _ => return Err(format!("unknown option {k}")),
        }
    }
    a.token = std::fs::read_to_string(&token_file).map_err(|e| format!("{token_file}: {e}"))?.trim().to_string();
    if a.token.len() < 16 {
        return Err("token too short".into());
    }
    Ok(a)
}

// ------------------------------------------------------------------ low-level helpers

fn open_nb(path: &str, flags: i32) -> std::io::Result<RawFd> {
    let c = CString::new(path).unwrap();
    let fd = unsafe { libc::open(c.as_ptr(), flags | libc::O_NONBLOCK | libc::O_CLOEXEC) };
    if fd < 0 { Err(std::io::Error::last_os_error()) } else { Ok(fd) }
}

/// Ok(true) written, Ok(false) would block.
fn write_nb(fd: RawFd, data: &[u8]) -> std::io::Result<bool> {
    let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
    if n >= 0 {
        return Ok(true);
    }
    let e = std::io::Error::last_os_error();
    if e.kind() == ErrorKind::WouldBlock { Ok(false) } else { Err(e) }
}

fn read_nb(fd: RawFd, buf: &mut [u8]) -> std::io::Result<Option<usize>> {
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
    if n >= 0 {
        return Ok(Some(n as usize));
    }
    let e = std::io::Error::last_os_error();
    if e.kind() == ErrorKind::WouldBlock { Ok(None) } else { Err(e) }
}

fn close(fd: RawFd) {
    unsafe { libc::close(fd) };
}

fn udc_state() -> String {
    std::fs::read_dir("/sys/class/udc").ok().and_then(|mut d| d.next()).and_then(|e| e.ok())
        .and_then(|e| std::fs::read_to_string(e.path().join("state")).ok())
        .map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".into())
}

fn usb_mode() -> String {
    std::fs::read_to_string(MODE_FILE).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        .unwrap_or_else(|| "standard".into())
}

fn exists(p: &str) -> bool {
    !p.is_empty() && std::path::Path::new(p).exists()
}

// ------------------------------------------------------------------ devices

enum Kind {
    Pad,
    Kbd,
}

enum Dev {
    /// One hidg device: newest state wins, keepalive every 0.5 s, OUT reports drained.
    Hid { fd: RawFd, kind: Kind, state: Vec<i64>, pending: bool, last: Option<Instant>, writes: u64 },
    /// Relative mouse: buttons are state, motion is queued and sent in <=127 steps.
    Mouse { fd: RawFd, buttons: u8, dx: i64, dy: i64, wheel: i64, pending: bool, writes: u64 },
    /// Pro Controller mode: the pad state goes to `pro2` over a datagram socket.
    Pro { sock: UnixDatagram, state: Vec<i64>, pending: bool, last: Option<Instant>, writes: u64 },
}

fn pack(kind: &Kind, s: &[i64]) -> Vec<u8> {
    match kind {
        // HORI report: 14 buttons (no C/GL/GR), hat, 4 axes, vendor byte
        Kind::Pad => {
            let b = (s[0] & 0x3FFF) as u16;
            vec![b as u8, (b >> 8) as u8, s[1] as u8, s[2] as u8, s[3] as u8, s[4] as u8, s[5] as u8, 0]
        }
        Kind::Kbd => {
            let mut v = vec![s[0] as u8, 0];
            v.extend(s[1..7].iter().map(|x| *x as u8));
            v
        }
    }
}

impl Dev {
    fn hid(path: &str, kind: Kind) -> std::io::Result<Dev> {
        let state = match kind { Kind::Pad => PAD_NEUTRAL.to_vec(), Kind::Kbd => KBD_NEUTRAL.to_vec() };
        Ok(Dev::Hid { fd: open_nb(path, libc::O_RDWR)?, kind, state, pending: true, last: None, writes: 0 })
    }

    fn mouse(path: &str) -> std::io::Result<Dev> {
        Ok(Dev::Mouse { fd: open_nb(path, libc::O_RDWR)?, buttons: 0, dx: 0, dy: 0, wheel: 0, pending: false, writes: 0 })
    }

    fn pro() -> std::io::Result<Dev> {
        let sock = UnixDatagram::unbound()?;
        sock.set_nonblocking(true)?;
        Ok(Dev::Pro { sock, state: PAD_NEUTRAL.to_vec(), pending: true, last: None, writes: 0 })
    }

    fn fd(&self) -> RawFd {
        match self {
            Dev::Hid { fd, .. } | Dev::Mouse { fd, .. } => *fd,
            Dev::Pro { sock, .. } => sock.as_raw_fd(),
        }
    }

    fn writes(&self) -> u64 {
        match self {
            Dev::Hid { writes, .. } | Dev::Mouse { writes, .. } | Dev::Pro { writes, .. } => *writes,
        }
    }

    fn set(&mut self, v: Vec<i64>) {
        match self {
            Dev::Hid { state, pending, .. } | Dev::Pro { state, pending, .. } => {
                if *state != v {
                    *state = v;
                    *pending = true;
                }
            }
            Dev::Mouse { buttons, dx, dy, wheel, pending, .. } => {
                *buttons = v[0] as u8;
                *dx += v[1];
                *dy += v[2];
                *wheel += v[3];
                *pending = true;
            }
        }
    }

    fn due(&self, now: Instant) -> bool {
        match self {
            Dev::Hid { pending, last, .. } | Dev::Pro { pending, last, .. } => {
                *pending || last.is_none_or(|l| now - l >= KEEPALIVE)
            }
            Dev::Mouse { pending, .. } => *pending,
        }
    }

    fn flush(&mut self, now: Instant) {
        match self {
            Dev::Hid { fd, kind, state, pending, last, writes } => {
                if let Ok(true) = write_nb(*fd, &pack(kind, state)) {
                    *pending = false;
                    *last = Some(now);
                    *writes += 1;
                }
            }
            Dev::Mouse { fd, buttons, dx, dy, wheel, pending, writes } => {
                let step = |v: i64| v.clamp(-127, 127);
                let (sx, sy, sw) = (step(*dx), step(*dy), step(*wheel));
                if let Ok(true) = write_nb(*fd, &[*buttons, sx as i8 as u8, sy as i8 as u8, sw as i8 as u8]) {
                    *dx -= sx;
                    *dy -= sy;
                    *wheel -= sw;
                    *pending = *dx != 0 || *dy != 0 || *wheel != 0;
                    *writes += 1;
                }
            }
            Dev::Pro { sock, state, pending, last, writes } => {
                let b = state[0] as u32;
                let mut m = b.to_le_bytes().to_vec();
                m.extend(state[1..6].iter().map(|x| *x as u8));
                if sock.send_to(&m, PRO_SOCK).is_ok() {
                    *writes += 1;
                }
                // Sent or not: when pro2 is down (crashed, still starting) retry on the
                // keepalive instead of spinning; the state is kept and resent then.
                *pending = false;
                *last = Some(now);
            }
        }
    }

    fn drain(&self) {
        if let Dev::Hid { fd, .. } = self {
            let mut b = [0u8; 64];
            let _ = read_nb(*fd, &mut b); // keyboard LED / vendor OUT reports
        }
    }

    fn close(self) {
        match self {
            Dev::Hid { fd, kind, .. } => {
                let neutral = match kind { Kind::Pad => PAD_NEUTRAL.to_vec(), Kind::Kbd => KBD_NEUTRAL.to_vec() };
                let deadline = Instant::now() + Duration::from_millis(200);
                while Instant::now() < deadline {
                    match write_nb(fd, &pack(&kind, &neutral)) {
                        Ok(false) => std::thread::sleep(Duration::from_millis(10)),
                        _ => break,
                    }
                }
                close(fd);
            }
            Dev::Mouse { fd, .. } => {
                let _ = write_nb(fd, &[0; 4]);
                close(fd);
            }
            Dev::Pro { sock, .. } => {
                let mut m = 0u32.to_le_bytes().to_vec();
                m.extend([8u8, 128, 128, 128, 128]);
                let _ = sock.send_to(&m, PRO_SOCK);
            }
        }
    }
}

// ------------------------------------------------------------------ validation

fn ints(v: &Value, n: usize) -> Option<Vec<i64>> {
    let a = v.as_array().filter(|a| a.len() == n)?;
    a.iter().map(|x| x.as_i64()).collect()
}

fn valid_pad(v: &Value) -> Option<Vec<i64>> {
    let s = ints(v, 6)?;
    ((0..=0x1FFFF).contains(&s[0]) && (0..=8).contains(&s[1]) && s[2..].iter().all(|a| (0..=255).contains(a))).then_some(s)
}

fn valid_kbd(v: &Value) -> Option<Vec<i64>> {
    let s = ints(v, 7)?;
    ((0..=255).contains(&s[0]) && s[1..].iter().all(|k| (0..=0xE7).contains(k))).then_some(s)
}

fn valid_mouse(v: &Value) -> Option<Vec<i64>> {
    let s = ints(v, 4)?;
    ((0..=0x1F).contains(&s[0]) && s[1..3].iter().all(|x| (-4096..=4096).contains(x)) && (-64..=64).contains(&s[3])).then_some(s)
}

// ------------------------------------------------------------------ session

struct Session<'a> {
    conn: TcpStream,
    peer: String,
    args: &'a Args,
    buf: Vec<u8>,
    authed: bool,
    devices: BTreeMap<&'static str, Dev>,
    mode: String,
    last_rx: Instant,
    accepted: Instant,
}

impl<'a> Session<'a> {
    fn send(&mut self, v: Value) {
        let mut s = v.to_string();
        s.push('\n');
        let _ = self.conn.set_nonblocking(false);
        let _ = self.conn.set_write_timeout(Some(Duration::from_millis(500)));
        let _ = self.conn.write_all(s.as_bytes());
        let _ = self.conn.set_nonblocking(true);
    }

    fn open_devices(&mut self) -> std::io::Result<()> {
        self.mode = usb_mode();
        let a = self.args;
        if self.mode == "pro" {
            self.devices.insert("pad", Dev::pro()?);
            if std::fs::symlink_metadata(format!("{GADGET_CFG}/f2")).is_ok() && exists(&a.kbd) {
                self.devices.insert("kbd", Dev::hid(&a.kbd, Kind::Kbd)?);
            }
            if std::fs::symlink_metadata(format!("{GADGET_CFG}/f3")).is_ok() && exists(&a.mouse) {
                self.devices.insert("mouse", Dev::mouse(&a.mouse)?);
            }
            return Ok(());
        }
        self.devices.insert("pad", Dev::hid(&a.pad, Kind::Pad)?);
        self.devices.insert("kbd", Dev::hid(&a.kbd, Kind::Kbd)?);
        if exists(&a.mouse) {
            self.devices.insert("mouse", Dev::mouse(&a.mouse)?);
        }
        Ok(())
    }

    fn close(mut self) {
        for (_, d) in std::mem::take(&mut self.devices) {
            d.close();
        }
    }

    /// false = end the session
    fn handle(&mut self, msg: &Value) -> bool {
        if !self.authed {
            let given = msg["hello"].as_str().unwrap_or("").as_bytes();
            let want = self.args.token.as_bytes();
            let same = given.len() == want.len() && given.iter().zip(want).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0;
            if !same {
                self.send(json!({"error": "auth"}));
                return false;
            }
            if let Err(e) = self.open_devices() {
                self.send(json!({"error": format!("opening devices: {e}")}));
                return false;
            }
            self.authed = true;
            let devices: Vec<&str> = self.devices.keys().copied().collect();
            self.send(json!({"ok": "ready", "udc": udc_state(), "watchdog_s": WATCHDOG.as_secs_f64(),
                             "devices": devices, "log_seq": logbuf::seq(), "mode": self.mode}));
            log(format!("client {} ready", self.peer));
            return true;
        }
        if let Some(v) = msg.get("pad") {
            match valid_pad(v) {
                Some(s) => self.devices.get_mut("pad").unwrap().set(s),
                None => {
                    self.send(json!({"error": "bad pad"}));
                    return true;
                }
            }
        }
        if let (Some(v), true) = (msg.get("kbd"), self.devices.contains_key("kbd")) {
            match valid_kbd(v) {
                Some(s) => self.devices.get_mut("kbd").unwrap().set(s),
                None => {
                    self.send(json!({"error": "bad kbd"}));
                    return true;
                }
            }
        }
        if let Some(v) = msg.get("mouse") {
            match (valid_mouse(v), self.devices.get_mut("mouse")) {
                (Some(s), Some(d)) => d.set(s),
                (s, _) => {
                    self.send(json!({"error": if s.is_none() { "bad mouse" } else { "no mouse on this board" }}));
                    return true;
                }
            }
        }
        if let Some(v) = msg.get("usb_mode") {
            let mode = v.as_str().unwrap_or("");
            if !["standard", "hori", "pro"].contains(&mode) {
                self.send(json!({"error": "bad usb_mode"}));
                return true;
            }
            log(format!("switching USB mode to {mode}"));
            let here = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| "/opt/ns2bench".into());
            let here = here.display();
            // detached: stop this daemon (it holds the HID devices the rebuild removes), rebuild, start again
            let script = format!("(sleep 0.5; {here}/ns2bench-hidd.sh stop >/dev/null 2>&1; \
                {here}/gadget.sh {mode} >> /tmp/ns2bench-gadget.log 2>&1; \
                {here}/ns2bench-hidd.sh start >/dev/null 2>&1) </dev/null >/dev/null 2>&1 &");
            let _ = std::process::Command::new("/bin/sh").arg("-c").arg(script).spawn();
            self.send(json!({"ok": format!("switching to {mode}")}));
            return false;
        }
        if let Some(v) = msg.get("logs") {
            let after = v.as_u64().unwrap_or(0);
            self.send(json!({"logs": logbuf::after(after), "seq": logbuf::seq(), "now": logbuf::now()}));
        }
        if let Some(n) = msg.get("ping") {
            let writes: serde_json::Map<String, Value> = self.devices.iter().map(|(k, d)| (k.to_string(), json!(d.writes()))).collect();
            self.send(json!({"pong": n, "udc": udc_state(), "writes": writes}));
        }
        true
    }

    fn feed(&mut self, data: &[u8]) -> bool {
        self.buf.extend_from_slice(data);
        if self.buf.len() > MAX_LINE && !self.buf.contains(&b'\n') {
            return false;
        }
        while let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let Ok(msg) = serde_json::from_slice::<Value>(&line) else { return false };
            if !msg.is_object() || !self.handle(&msg) {
                return false;
            }
        }
        self.last_rx = Instant::now();
        true
    }
}

// ------------------------------------------------------------------ kernel log

struct KernelLog(Option<RawFd>);

impl KernelLog {
    fn open(path: &str) -> KernelLog {
        if path.is_empty() {
            return KernelLog(None);
        }
        match open_nb(path, libc::O_RDONLY) {
            Ok(fd) => {
                unsafe { libc::lseek(fd, 0, libc::SEEK_END) };
                KernelLog(Some(fd))
            }
            Err(_) => KernelLog(None),
        }
    }

    fn drain(&self) {
        let Some(fd) = self.0 else { return };
        let mut buf = [0u8; 8192];
        loop {
            match read_nb(fd, &mut buf) {
                Ok(Some(n)) if n > 0 => {
                    let rec = String::from_utf8_lossy(&buf[..n]);
                    let msg = rec.split_once(';').map_or("", |x| x.1);
                    logbuf::remember("kernel", msg.lines().next().unwrap_or("").trim().to_string());
                }
                Err(e) if e.raw_os_error() == Some(libc::EPIPE) => continue, // overrun: records lost
                _ => return,
            }
        }
    }
}

// ------------------------------------------------------------------ main loop

static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

pub fn main(argv: &[String]) -> Result<(), String> {
    let args = parse(argv)?;
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        // `ns2bench-hidd.sh stop`, upgrades and mode switches send SIGTERM: release first.
        libc::signal(libc::SIGTERM, on_stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_stop as *const () as libc::sighandler_t);
    }
    let srv = TcpListener::bind((args.bind.as_str(), args.port)).map_err(|e| format!("bind {}:{}: {e}", args.bind, args.port))?;
    srv.set_nonblocking(true).map_err(|e| e.to_string())?;
    log(format!("listening {}:{} (NS2Bench Bridge {})", args.bind, args.port, env!("CARGO_PKG_VERSION")));
    let kmsg = KernelLog::open(&args.kmsg);
    let mut session: Option<Session> = None;
    loop {
        if STOP.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(s) = session.take() {
                log(format!("stopping: releasing client {}", s.peer));
                s.close();
            }
            log("stopped");
            return Ok(());
        }
        let now = Instant::now();
        let mut fds = vec![libc::pollfd { fd: srv.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        if let Some(fd) = kmsg.0 {
            fds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
        }
        if let Some(s) = &session {
            fds.push(libc::pollfd { fd: s.conn.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            for d in s.devices.values() {
                let ev = libc::POLLIN | if d.due(now) { libc::POLLOUT } else { 0 };
                fds.push(libc::pollfd { fd: d.fd(), events: ev, revents: 0 });
            }
        }
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 50) };
        let ready = |fd: RawFd, ev: i16| fds.iter().any(|p| p.fd == fd && p.revents & (ev | libc::POLLERR | libc::POLLHUP) != 0);
        let now = Instant::now();
        if let Some(fd) = kmsg.0 {
            if ready(fd, libc::POLLIN) {
                kmsg.drain();
            }
        }
        if ready(srv.as_raw_fd(), libc::POLLIN) {
            if let Ok((mut conn, peer)) = srv.accept() {
                if session.is_some() {
                    let _ = conn.write_all(b"{\"error\": \"busy\"}\n");
                } else {
                    let _ = conn.set_nodelay(true);
                    let _ = conn.set_nonblocking(true);
                    session = Some(Session {
                        conn, peer: peer.ip().to_string(), args: &args, buf: vec![], authed: false,
                        devices: BTreeMap::new(), mode: String::new(), last_rx: now, accepted: now,
                    });
                }
            }
        }
        let Some(s) = session.as_mut() else { continue };
        let mut ok = true;
        if ready(s.conn.as_raw_fd(), libc::POLLIN) {
            let mut buf = [0u8; 4096];
            ok = match s.conn.read(&mut buf) {
                Ok(0) => false,
                Ok(n) => s.feed(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => true,
                Err(_) => false,
            };
        }
        if ok {
            let fds_ready: Vec<(RawFd, bool, bool)> = s.devices.values()
                .map(|d| (d.fd(), ready(d.fd(), libc::POLLIN), ready(d.fd(), libc::POLLOUT))).collect();
            for (d, (_, r, w)) in s.devices.values_mut().zip(fds_ready) {
                if r {
                    d.drain();
                }
                if w && d.due(now) {
                    d.flush(now);
                }
            }
        }
        // Unauthenticated clients get a fixed deadline from accept, so sending blank
        // lines cannot hold the only session slot.
        let silent = if s.authed { now - s.last_rx > WATCHDOG } else { now - s.accepted > Duration::from_secs(5) };
        if ok && silent {
            log(format!("client {} watchdog: silent, releasing", s.peer));
            ok = false;
        }
        if !ok {
            log(format!("client {} closed", s.peer));
            session.take().unwrap().close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_and_packing() {
        assert!(valid_pad(&json!([0x1FFFF, 8, 0, 255, 128, 128])).is_some());
        assert!(valid_pad(&json!([0x20000, 8, 0, 0, 0, 0])).is_none());
        assert!(valid_pad(&json!([1, 9, 0, 0, 0, 0])).is_none());
        assert!(valid_pad(&json!([1, 8, 0.5, 0, 0, 0])).is_none());
        assert!(valid_kbd(&json!([2, 4, 0, 0, 0, 0, 0])).is_some());
        assert!(valid_kbd(&json!([2, 0xE8, 0, 0, 0, 0, 0])).is_none());
        assert!(valid_mouse(&json!([1, -4096, 4096, 64])).is_some());
        assert!(valid_mouse(&json!([1, 0, 0, 65])).is_none());
        // GL bit (15) dropped in the HORI report; little-endian buttons
        assert_eq!(pack(&Kind::Pad, &[0x8004, 2, 1, 2, 3, 4]), vec![0x04, 0x00, 2, 1, 2, 3, 4, 0]);
        assert_eq!(pack(&Kind::Kbd, &[2, 4, 5, 0, 0, 0, 0]), vec![2, 0, 4, 5, 0, 0, 0, 0]);
    }
}
