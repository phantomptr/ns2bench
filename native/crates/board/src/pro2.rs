//! Switch 2 Pro Controller over wired USB, as a Linux FunctionFS gadget function.
//! Protocol and data follow GP2040-CE's Switch 2 Pro driver (MIT, see NOTICE and LICENSES/) and
//! ndeadly/switch2_controller_research.
//! Interface 0 = HID (interrupt IN/OUT, report 0x09 input, 0x02 rumble out); interface 1 = vendor
//! (bulk OUT/IN carrying the command protocol). Runs at full speed.
//! State arrives from `hidd` on /run/ns2bench-pro2.sock: <u32 buttons LE><hat><lx><ly><rx><ry>.

use crate::aes;
use std::collections::HashSet;
use std::ffi::CString;
use std::os::fd::RawFd;
use std::os::unix::net::UnixDatagram;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const STATE_SOCK: &str = "/run/ns2bench-pro2.sock";
const IDENTITY: &str = "/opt/ns2bench/pro2-identity.json";

pub const REPORT_DESC: [u8; 97] = [
    0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, 0x85, 0x05, 0x05, 0xff, 0x09, 0x01, 0x15, 0x00, 0x26, 0xff, 0x00, 0x95, 0x3f,
    0x75, 0x08, 0x81, 0x02, 0x85, 0x09, 0x09, 0x01, 0x95, 0x02, 0x81, 0x02, 0x05, 0x09, 0x19, 0x01, 0x29, 0x15, 0x25,
    0x01, 0x95, 0x15, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x81, 0x03, 0x05, 0x01, 0x09, 0x01, 0xa1, 0x00,
    0x09, 0x30, 0x09, 0x31, 0x09, 0x33, 0x09, 0x35, 0x26, 0xff, 0x0f, 0x95, 0x04, 0x75, 0x0c, 0x81, 0x02, 0xc0, 0x05,
    0xff, 0x09, 0x02, 0x26, 0xff, 0x00, 0x95, 0x34, 0x75, 0x08, 0x81, 0x02, 0x85, 0x02, 0x09, 0x01, 0x95, 0x3f, 0x91,
    0x02, 0xc0,
];

fn log(msg: impl AsRef<str>) {
    // stdout goes to /tmp/ns2bench-pro2.log; gadget.sh waits there for "console initialised USB"
    let s = crate::logbuf::now() as i64;
    println!("{:02}:{:02}:{:02} {}", (s / 3600) % 24, (s / 60) % 60, s % 60, msg.as_ref());
}

// ------------------------------------------------------------------ descriptors

fn ep(addr: u8, attr: u8, size: u16, interval: u8) -> Vec<u8> {
    vec![7, 5, addr, attr, size as u8, (size >> 8) as u8, interval]
}

fn descriptor_set(int_interval: u8, bulk: u16) -> (Vec<u8>, u32) {
    let parts: Vec<Vec<u8>> = vec![
        vec![8, 0x0B, 0, 1, 0x03, 0x00, 0x00, 0],                      // IAD -> HID interface
        vec![9, 4, 0, 0, 2, 0x03, 0x00, 0x00, 1],                      // if0 HID, iInterface 1
        vec![9, 0x21, 0x11, 0x01, 0x00, 0x01, 0x22, REPORT_DESC.len() as u8, 0],
        ep(0x81, 3, 64, int_interval),                                 // interrupt IN
        ep(0x01, 3, 64, int_interval),                                 // interrupt OUT
        vec![8, 0x0B, 1, 1, 0xFF, 0x00, 0x00, 0],                      // IAD -> vendor interface
        vec![9, 4, 1, 0, 2, 0xFF, 0x00, 0x00, 2],                      // if1 vendor, iInterface 2
        ep(0x02, 2, bulk, 0),                                          // bulk OUT (commands)
        ep(0x82, 2, bulk, 0),                                          // bulk IN (replies)
    ];
    (parts.concat(), parts.len() as u32)
}

fn le(v: &mut Vec<u8>, x: u32) {
    v.extend(x.to_le_bytes());
}

pub fn ffs_descriptors() -> Vec<u8> {
    let (fs, nfs) = descriptor_set(4, 64);
    let (hs, nhs) = descriptor_set(6, 512);
    let mut v = vec![];
    le(&mut v, 3); // FUNCTIONFS_DESCRIPTORS_MAGIC_V2
    le(&mut v, (20 + fs.len() + hs.len()) as u32);
    le(&mut v, 1 | 2 | 64); // HAS_FS | HAS_HS | ALL_CTRL_RECIP
    le(&mut v, nfs);
    le(&mut v, nhs);
    v.extend(fs);
    v.extend(hs);
    v
}

pub fn ffs_strings() -> Vec<u8> {
    let mut lang = 0x0409u16.to_le_bytes().to_vec();
    lang.extend(b"If_Hid\0Switch 2 Pro Controller\0");
    let mut v = vec![];
    le(&mut v, 2); // FUNCTIONFS_STRINGS_MAGIC
    le(&mut v, (16 + lang.len()) as u32);
    le(&mut v, 2);
    le(&mut v, 1);
    v.extend(lang);
    v
}

// ------------------------------------------------------------------ identity (stable across reboots)

struct Identity {
    bt_rev: [u8; 6],
    serial: String,
    key: [u8; 16],
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let v: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut b);
    }
    b
}

fn load_identity() -> Identity {
    if let Some(v) = std::fs::read_to_string(IDENTITY).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
        if let (Some(bt), Some(serial), Some(key)) =
            (v["bt"].as_str().and_then(unhex::<6>), v["serial"].as_str(), v["key"].as_str().and_then(unhex::<16>)) {
            return Identity { bt_rev: bt, serial: serial.into(), key };
        }
    }
    let r: [u8; 3] = random();
    let bt = [r[0], r[1], r[2], 0x05, 0x48, 0xC8]; // reversed order
    let n = u32::from_le_bytes(random()) % 1_000_000_000;
    let id = Identity { bt_rev: bt, serial: format!("HEJ1{n:09}2"), key: random() };
    let j = serde_json::json!({"bt": hex(&id.bt_rev), "serial": id.serial, "key": hex(&id.key)});
    if std::fs::write(IDENTITY, j.to_string()).is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(IDENTITY, std::fs::Permissions::from_mode(0o600));
    }
    id
}

// ------------------------------------------------------------------ command protocol (bulk)

struct Proto {
    id: Identity,
    device_info: Vec<u8>,
    factory: Vec<u8>,
    flash: Vec<(u32, Vec<u8>)>,
    grip: Vec<u8>,
    ltk: [u8; 16],
    seen: HashSet<(u8, u8)>,
}

fn unhexv(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

impl Proto {
    fn new(id: Identity) -> Proto {
        let mut device_info = vec![0x02, 0x01, 0x04, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x02, 0x03];
        device_info.extend(id.bt_rev);
        let mut sn = id.serial.as_bytes()[..id.serial.len().min(15)].to_vec();
        sn.resize(16, 0);
        let mut factory = vec![0x01, 0x00];
        factory.extend(sn);
        factory.extend([0x7E, 0x05, 0x69, 0x20, 0x01, 0x06, 0x01]);
        factory.extend([0x23, 0x23, 0x23, 0xA0, 0xA0, 0xA0, 0xE6, 0xE6, 0xE6, 0x32, 0x32, 0x32]);
        factory.extend([0xFF; 15]);
        let stick_cal = |tail: &str| {
            let mut v = unhexv(&format!("01add99a555665a0000aa0000ae2200ee2200e9aadd99aadd90aa5500aa5502ff6622ff6620affff{tail}"));
            v.extend([0xFF; 15]);
            v
        };
        let mut f5 = vec![0u8; 12];
        f5.extend(unhexv("fed27ebd338abebeaa822041"));
        let flash = vec![
            (0x13000, factory.clone()),
            (0x13040, unhexv("852eec4149ec20bcc55d3fbcad11423b")),
            (0x13080, stick_cal("bc97854f165f2e9661")),
            (0x130C0, stick_cal("733882f555602a5664")),
            (0x13100, f5),
        ];
        let mut grip = vec![0, 0, 0, 0, 0x01, 0x00];
        grip.extend(b"HEJ71001121247\0\0");
        grip.extend([0x7E, 0x05, 0x69, 0x20, 0x01, 0x06, 0x01]);
        grip.extend([0xFF; 39]);
        Proto { id, device_info, factory, flash, grip, ltk: [0; 16], seen: HashSet::new() }
    }

    fn reply(cmd: u8, sub: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![cmd, 0x01, 0x00, sub, 0x00, 0xF8, 0x00, 0x00];
        v.extend(&payload[..payload.len().min(152)]);
        v
    }

    fn flash_read(&self, req: &[u8]) -> Vec<u8> {
        let len = (req[8] as usize).min(0x40);
        let addr = u32::from_le_bytes([req[12], req[13], req[14], req[15]]);
        let mut data = vec![0xFFu8; len];
        for (base, block) in &self.flash {
            for (i, d) in data.iter_mut().enumerate() {
                let a = addr + i as u32;
                if *base <= a && a < base + block.len() as u32 {
                    *d = block[(a - base) as usize];
                }
            }
        }
        let mut v = vec![len as u8, 0, 0, 0];
        v.extend(&req[12..16]);
        v.extend(data);
        v
    }

    /// req is at least 64 bytes.
    fn handle(&mut self, req: &[u8]) -> Vec<u8> {
        let (cmd, sub) = (req[0], req[3]);
        if self.seen.insert((cmd, sub)) {
            log(format!("command {cmd:02x}/{sub:02x} first seen: {}", hex(&req[..16])));
        }
        let r = |p: &[u8]| Self::reply(cmd, sub, p);
        match (cmd, sub) {
            (0x01, 0x0C) => r(&[0x61, 0x12, 0x50, 0x10]),
            (0x01, 0x05) => {
                let mut p = vec![9, 0, 0, 0, 1, 1, 2, 0, 7];
                p.extend([0; 52]);
                r(&p)
            }
            (0x02, 0x01 | 0x04) => r(&self.flash_read(req)),
            (0x02, 0x02) => r(&req[8..16]),
            (0x02, 0x03) => r(&[0; 4]),
            (0x02, 0x05) => {
                let mut p = vec![0; 4];
                p.extend(&req[12..16]);
                r(&p)
            }
            (0x03, 0x07) => {
                self.ltk = std::array::from_fn(|i| req[29 - i]);
                r(&[])
            }
            (0x03, 0x0D) => {
                log("console initialised USB: input reports on");
                set_host(true);
                r(&[0x01, 0, 0, 0])
            }
            (0x03, 0x0F) => r(&[0x05, 0, 0, 0]),
            (0x07, 0x01) => r(&[0x00]),
            (0x08, 0x01) => r(&self.grip[..36]),
            (0x08, 0x03) => r(&self.grip[..68]),
            (0x09, 0x07) => {
                log(format!("player LEDs: {:04b}", req[8]));
                r(&[])
            }
            (0x0A, 0x02) => r(&[req[8], 0, 0, 0]),
            (0x0B, 0x03) => r(&[0xA5, 0x0E, 0, 0]),
            (0x0B, 0x04) => r(&[0x34, 0x00, 0x83, 0x00]),
            (0x0B, 0x06) => r(&[0x11, 0, 0, 0]),
            (0x0C, 0x01) => {
                let (f, mut p) = (req[8], [0u8; 12]);
                if f & 0x01 != 0 { p[4] = 0x07 }
                if f & 0x02 != 0 { p[5] = 0x07 }
                if f & 0x04 != 0 { p[6] = 0x01 }
                if f & 0x80 != 0 { p[7] = 0x01 }
                if f & 0x10 != 0 { p[8] = 0x01 }
                if f & 0x20 != 0 { p[9] = 0x03 }
                r(&p)
            }
            (0x0C, 0x02..=0x05) => r(&[0; 4]),
            (0x10, 0x01) => r(&[0x02, 0x01, 0x04, 0x02, 0x0C, 0, 0, 0, 0, 0x02, 0x03, 0]),
            (0x11, 0x01) => r(&[0x01, 0, 0, 0]),
            (0x11, 0x03) => r(&unhexv("01200300000ae81c3b797d8b3a0ae89c4258a00b420ae89c4158a00b41")),
            (0x13, 0x01) => r(&[0x01, 0, 0, 0]),
            (0x15, 0x01) => {
                let mut p = vec![0x01, 0x04, 0x01];
                p.extend(self.id.bt_rev);
                r(&p)
            }
            (0x15, 0x04) => {
                self.ltk = std::array::from_fn(|i| req[24 - i] ^ self.id.key[i]);
                let mut p = vec![0x01];
                p.extend((0..16).map(|i| self.id.key[15 - i]));
                r(&p)
            }
            (0x15, 0x02) => {
                let chal: [u8; 16] = std::array::from_fn(|i| req[24 - i]);
                let mut p = vec![0x01];
                p.extend(aes::encrypt(&self.ltk, &chal));
                r(&p)
            }
            (0x15, 0x03) => r(&[0x01]),
            (0x16, 0x01) => {
                let mut p = [0u8; 0x18];
                p[12] = 0x7C;
                p[13] = 0x06;
                r(&p)
            }
            (0x18, 0x01) => r(&[0x00, 0x00, 0x40, 0xF0, 0x00, 0x00, 0x60, 0x00]),
            (0x18, 0x03) => r(&[req[8]]),
            _ => r(&[]),
        }
    }
}

// ------------------------------------------------------------------ input report

/// NS2Bench wire bit -> (report byte offset after the ID, mask)
const BIT_MAP: [(usize, u8); 17] = [
    (2, 0x04), (2, 0x01), (2, 0x02), (2, 0x08), // Y B A X
    (3, 0x10), (2, 0x10), (3, 0x20), (2, 0x20), // L R ZL ZR
    (3, 0x40), (2, 0x40), (3, 0x80), (2, 0x80), // MINUS PLUS LSTICK RSTICK
    (4, 0x01), (4, 0x02), (4, 0x10), (4, 0x08), // HOME CAPTURE C GL
    (4, 0x04),                                   // GR
];
const HAT_DPAD: [u8; 9] = [0x08, 0x0A, 0x02, 0x03, 0x01, 0x05, 0x04, 0x0C, 0x00]; // up right down left bits

fn stick(x8: u8, y8: u8) -> [u8; 3] {
    let x = (2048 + (x8 as i32 - 128) * 16).clamp(0, 4095) as u32; // 128 -> 0x800 exactly
    let y = (2048 - (y8 as i32 - 128) * 16).clamp(0, 4095) as u32; // report: larger y = up
    [(x & 0xFF) as u8, (((x >> 8) & 0x0F) | ((y & 0x0F) << 4)) as u8, ((y >> 4) & 0xFF) as u8]
}

#[derive(Clone, Copy)]
struct State {
    buttons: u32,
    hat: u8,
    axes: [u8; 4],
}

const NEUTRAL: State = State { buttons: 0, hat: 8, axes: [128; 4] };
fn is_neutral(s: &State) -> bool {
    s.buttons == 0 && s.hat == 8 && s.axes == [128; 4]
}

fn build_report(s: State, counter: u8) -> [u8; 64] {
    let mut r = [0u8; 64];
    r[0] = 0x09;
    let body = &mut r[1..];
    body[0] = counter;
    body[1] = 0x23; // wired, charging, level 8
    for (bit, (off, mask)) in BIT_MAP.iter().enumerate() {
        if s.buttons >> bit & 1 != 0 {
            body[*off] |= mask;
        }
    }
    body[3] |= HAT_DPAD[(s.hat as usize).min(8)];
    body[5..8].copy_from_slice(&stick(s.axes[0], s.axes[1]));
    body[8..11].copy_from_slice(&stick(s.axes[2], s.axes[3]));
    body[11] = 0x38;
    r
}

// ------------------------------------------------------------------ threads

/// "enabled by the console" flag (FunctionFS ENABLE/DISABLE), and the latest pad state.
struct Shared {
    enabled: Mutex<bool>,
    enabled_cv: Condvar,
    state: Mutex<(State, u64)>,
    state_cv: Condvar,
}

impl Shared {
    fn wait_enabled(&self) {
        let mut e = self.enabled.lock().unwrap();
        while !*e {
            e = self.enabled_cv.wait(e).unwrap();
        }
    }
    fn is_enabled(&self) -> bool {
        *self.enabled.lock().unwrap()
    }
    fn set_enabled(&self, v: bool) {
        *self.enabled.lock().unwrap() = v;
        self.enabled_cv.notify_all();
    }
}

fn open(path: &str, flags: i32) -> std::io::Result<RawFd> {
    let c = CString::new(path).unwrap();
    let fd = unsafe { libc::open(c.as_ptr(), flags | libc::O_CLOEXEC) };
    if fd < 0 { Err(std::io::Error::last_os_error()) } else { Ok(fd) }
}

fn write(fd: RawFd, b: &[u8]) -> std::io::Result<usize> {
    let n = unsafe { libc::write(fd, b.as_ptr() as *const _, b.len()) };
    if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
}

fn read(fd: RawFd, b: &mut [u8]) -> std::io::Result<usize> {
    let n = unsafe { libc::read(fd, b.as_mut_ptr() as *mut _, b.len()) };
    if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
}

fn input_loop(sh: Arc<Shared>, ffs: String) {
    let mut counter = 0u8;
    loop {
        sh.wait_enabled();
        let Ok(fd) = open(&format!("{ffs}/ep1"), libc::O_WRONLY) else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let mut seen = 0u64;
        while sh.is_enabled() {
            let s = sh.state.lock().unwrap().0;
            if let Err(e) = write(fd, &build_report(s, counter)) {
                log(format!("input endpoint: {e}"));
                std::thread::sleep(Duration::from_millis(200));
                break;
            }
            counter = counter.wrapping_add(1);
            // ~125 Hz, immediately on change
            let g = sh.state.lock().unwrap();
            let (g, _) = sh.state_cv.wait_timeout_while(g, Duration::from_millis(8), |st| st.1 == seen).unwrap();
            seen = g.1;
        }
        unsafe { libc::close(fd) };
    }
}

fn drain_loop(sh: Arc<Shared>, ffs: String) {
    loop {
        sh.wait_enabled();
        let Ok(fd) = open(&format!("{ffs}/ep2"), libc::O_RDONLY) else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let mut b = [0u8; 64];
        while sh.is_enabled() {
            if read(fd, &mut b).is_err() {
                std::thread::sleep(Duration::from_millis(200));
                break;
            } // rumble output reports: accepted, ignored
        }
        unsafe { libc::close(fd) };
    }
}

fn command_loop(sh: Arc<Shared>, ffs: String, mut proto: Proto) {
    loop {
        sh.wait_enabled();
        let (Ok(rfd), Ok(wfd)) = (open(&format!("{ffs}/ep3"), libc::O_RDONLY), open(&format!("{ffs}/ep4"), libc::O_WRONLY)) else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let mut buf = [0u8; 512];
        while sh.is_enabled() {
            let n = match read(rfd, &mut buf) {
                Ok(n) => n,
                Err(e) => {
                    log(format!("command endpoint: {e}"));
                    std::thread::sleep(Duration::from_millis(200));
                    break;
                }
            };
            if n < 4 || buf[1] != 0x91 || buf[2] != 0x00 || buf[0] == 0x0D {
                continue; // 0x0D = firmware update: never answer
            }
            let mut req = buf[..n].to_vec();
            if req.len() < 64 {
                req.resize(64, 0);
            }
            if let Err(e) = write(wfd, &proto.handle(&req)) {
                log(format!("command endpoint: {e}"));
                break;
            }
        }
        unsafe {
            libc::close(rfd);
            libc::close(wfd);
        }
    }
}

fn state_loop(sh: Arc<Shared>) {
    let _ = std::fs::remove_file(STATE_SOCK);
    let s = match UnixDatagram::bind(STATE_SOCK) {
        Ok(s) => s,
        Err(e) => {
            log(format!("state socket {STATE_SOCK}: {e}"));
            return;
        }
    };
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(STATE_SOCK, std::fs::Permissions::from_mode(0o600));
    }
    // hidd refreshes the state every 0.5 s while a host is connected. If it stops
    // (killed, crashed, upgraded), go neutral instead of holding the last buttons.
    let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(1500)));
    let mut b = [0u8; 64];
    loop {
        let n = match s.recv(&mut b) {
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                let mut g = sh.state.lock().unwrap();
                if !is_neutral(&g.0) {
                    log("no state from hidd for 1.5 s: releasing");
                    g.0 = NEUTRAL;
                    g.1 += 1;
                    sh.state_cv.notify_all();
                }
                continue;
            }
            Err(_) => continue,
        };
        if n >= 9 {
            let st = State { buttons: u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & 0x1FFFF, hat: b[4], axes: [b[5], b[6], b[7], b[8]] };
            let mut g = sh.state.lock().unwrap();
            g.0 = st;
            g.1 += 1;
            sh.state_cv.notify_all();
        }
    }
}

// ------------------------------------------------------------------ ep0 (control)

#[allow(clippy::too_many_arguments)] // mirrors the usb_ctrlrequest fields
fn setup(fd: RawFd, rt: u8, req: u8, val: u16, idx: u16, len: u16, info: &[u8], factory: &[u8]) {
    let dir_in = rt & 0x80 != 0;
    let typ = (rt >> 5) & 3;
    let l = len as usize;
    let mut sink = vec![0u8; l];
    let r = if typ == 0 && req == 0x06 && (val >> 8) == 0x22 {
        write(fd, &REPORT_DESC[..l.min(REPORT_DESC.len())]) // GET_DESCRIPTOR(HID report)
    } else if typ == 1 {
        if dir_in { write(fd, &vec![0u8; l]) } else { read(fd, &mut sink) } // HID class
    } else if typ == 2 && dir_in && req == 0x02 {
        write(fd, &info[..l.min(info.len())])
    } else if typ == 2 && dir_in && req == 0x03 {
        let mut f = factory.to_vec();
        f.resize(64.max(f.len()), 0xFF);
        write(fd, &f[..l.min(f.len())])
    } else if typ == 2 && !dir_in && req == 0x04 {
        read(fd, &mut sink)
    } else {
        log(format!("stall setup {rt:02x} {req:02x} {val:04x} {idx:04x} {len}"));
        // a transfer in the wrong direction stalls ep0
        if dir_in { read(fd, &mut []) } else { write(fd, &[]) }
    };
    if let Err(e) = r {
        if e.raw_os_error() != Some(51) {
            // 51 = Linux EL2HLT: ep0 stalled as intended
            log(format!("setup {rt:02x}/{req:02x}: {e}"));
        }
    }
}

/// Written once a Switch 2 has started this controller (the "input reports on" command, which
/// only a Switch 2 sends); removed when the console lets go. The bridge reports it, so the bench
/// can tell which console the controller is plugged into.
pub const HOST_FILE: &str = "/run/ns2bench-host";

fn set_host(on: bool) {
    let _ = if on { std::fs::write(HOST_FILE, "switch2\n") } else { std::fs::remove_file(HOST_FILE) };
}

pub fn main(ffs: &str) -> Result<(), String> {
    set_host(false);
    let id = load_identity();
    let fd = open(&format!("{ffs}/ep0"), libc::O_RDWR).map_err(|e| format!("{ffs}/ep0: {e}"))?;
    write(fd, &ffs_descriptors()).map_err(|e| format!("descriptors: {e}"))?;
    write(fd, &ffs_strings()).map_err(|e| format!("strings: {e}"))?;
    let mut mac = id.bt_rev;
    mac.reverse();
    log(format!("descriptors written; identity bt={} serial={} (NS2Bench Bridge {})",
        mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"), id.serial, env!("CARGO_PKG_VERSION")));
    let proto = Proto::new(id);
    let (info, factory) = (proto.device_info.clone(), proto.factory.clone());
    let sh = Arc::new(Shared { enabled: Mutex::new(false), enabled_cv: Condvar::new(), state: Mutex::new((NEUTRAL, 0)), state_cv: Condvar::new() });
    let f = ffs.to_string();
    { let (s, f) = (sh.clone(), f.clone()); std::thread::spawn(move || input_loop(s, f)); }
    { let (s, f) = (sh.clone(), f.clone()); std::thread::spawn(move || command_loop(s, f, proto)); }
    { let (s, f) = (sh.clone(), f.clone()); std::thread::spawn(move || drain_loop(s, f)); }
    { let s = sh.clone(); std::thread::spawn(move || state_loop(s)); }

    // struct usb_functionfs_event: 8-byte ctrlrequest + u8 type + 3 pad = 12 bytes
    let mut buf = [0u8; 48];
    loop {
        let n = read(fd, &mut buf).map_err(|e| format!("ep0: {e}"))?;
        for ev in buf[..n].as_chunks::<12>().0 {
            let (rt, req) = (ev[0], ev[1]);
            let val = u16::from_le_bytes([ev[2], ev[3]]);
            let idx = u16::from_le_bytes([ev[4], ev[5]]);
            let len = u16::from_le_bytes([ev[6], ev[7]]);
            match ev[8] {
                2 => {
                    log("enabled by the console");
                    sh.set_enabled(true);
                }
                1 | 3 => {
                    log(if ev[8] == 1 { "unbind" } else { "disable" });
                    sh.set_enabled(false);
                    set_host(false);
                }
                4 => setup(fd, rt, req, val, idx, len, &info, &factory),
                5 => log("suspend"),
                6 => log("resume"),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_match_reference() {
        // lengths: header 20 + 2 * (8+9+9+7+7+8+9+7+7 = 71)
        let d = ffs_descriptors();
        assert_eq!(d.len(), 20 + 2 * 71);
        assert_eq!(&d[..4], &3u32.to_le_bytes());
        assert_eq!(ffs_strings().len(), 16 + 2 + 7 + 24);
    }

    #[test]
    fn report_layout() {
        // A (bit 2), HOME (12), GR (16); hat right; left stick full left/up
        let s = State { buttons: (1 << 2) | (1 << 12) | (1 << 16), hat: 2, axes: [0, 0, 128, 128] };
        let r = build_report(s, 7);
        assert_eq!(r[0], 0x09);
        assert_eq!(r[1], 7);
        assert_eq!(r[3], 0x02); // A
        assert_eq!(r[4], 0x02); // dpad right
        assert_eq!(r[5], 0x01 | 0x04); // HOME | GR
        assert_eq!(stick(128, 128), [0x00, 0x08, 0x80]);
        assert_eq!(&r[6..9], &stick(0, 0));
        assert_eq!(r[12], 0x38);
    }

    #[test]
    fn pairing_reply_uses_aes() {
        let id = Identity { bt_rev: [1, 2, 3, 4, 5, 6], serial: "HEJ10000000012".into(), key: [7; 16] };
        let mut p = Proto::new(id);
        let mut req = vec![0u8; 64];
        req[0] = 0x15;
        req[3] = 0x02;
        let out = p.handle(&req);
        assert_eq!(&out[..8], &[0x15, 0x01, 0x00, 0x02, 0x00, 0xF8, 0x00, 0x00]);
        assert_eq!(out.len(), 8 + 17);
        assert_eq!(&out[9..], &aes::encrypt(&[0; 16], &[0; 16]));
    }
}

#[cfg(test)]
mod parity {
    use super::*;
    // Golden responses captured from the original daemon using a synthetic identity.
    #[test]
    fn reference_protocol() {
        let id = Identity { bt_rev: [0x11, 0x22, 0x33, 0x05, 0x48, 0xC8], serial: "HEJ1123456789".into(), key: std::array::from_fn(|i| i as u8 * 3) };
        let mut p = Proto::new(id);
        let mut out = serde_json::Map::new();
        out.insert("desc".into(), hex(&ffs_descriptors()).into());
        out.insert("strings".into(), hex(&ffs_strings()).into());
        for cmd in 0u8..0x1A {
            for sub in 0u8..0x12 {
                let req: Vec<u8> = (0..64u32).map(|i| match i { 0 => cmd, 1 => 0x91, 2 => 0, 3 => sub, 8 => 0x10, 12 => 0x40, 13 => 0x30, 14 => 0x01, _ => (i * 7 % 251) as u8 }).collect();
                out.insert(format!("c{cmd:02x}{sub:02x}"), hex(&p.handle(&req)).into());
            }
        }
        for (i, b) in [0u32, 1, 0x1FFFF, 0x8004, 0x1000].iter().enumerate() {
            let s = State { buttons: *b, hat: (i * 2) as u8, axes: [0, 255, 77, 200] };
            out.insert(format!("r{i}"), hex(&build_report(s, i as u8)).into());
        }
        let expected: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/pro2.json")).unwrap();
        assert_eq!(serde_json::Value::Object(out), expected);
    }
}
