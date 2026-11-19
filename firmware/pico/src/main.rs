//! NS2Bench Bridge firmware for a Raspberry Pi Pico (RP2040).
//!
//! The Pico's USB port goes to the console and shows exactly what the Linux boards show in
//! "standard" mode: a HORI-style controller (0f0d:0092) + boot keyboard + boot mouse, with the
//! same report descriptors. The computer talks to the Pico over UART0 (GP0 = TX, GP1 = RX,
//! 921600 8N1 by default) through any USB-serial adapter, with the same JSON-lines protocol as
//! ns2bench-board hidd, so the host's "serial" backend drives it unchanged:
//!   {"hello": token} {"pad": [...]} {"kbd": [...]} {"mouse": [...]} {"ping": n} {"logs": n}
//! Safety: silent for 1 s after hello -> everything goes neutral.
//! Build-time options: NS2BENCH_TOKEN (empty = accept any hello), NS2BENCH_BAUD.

#![no_std]
#![no_main]

use core::cell::RefCell;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, Ordering};
use portable_atomic::AtomicU32; // RP2040 (Cortex-M0+) has no atomic read-modify-write
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::{UART0, USB};
use embassy_rp::uart::{BufferedInterruptHandler, BufferedUart, BufferedUartRx, BufferedUartTx, Config as UartConfig};
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use embassy_usb::class::hid::{self, HidProtocolMode, HidReaderWriter, HidWriter, ReportId, RequestHandler};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Handler};
use embedded_io_async::{Read, Write};
use heapless::String;
use panic_halt as _;
use serde::Deserialize;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
    UART0_IRQ => BufferedInterruptHandler<UART0>;
});

const TOKEN: &str = match option_env!("NS2BENCH_TOKEN") { Some(t) => t, None => "" };
const BAUD: u32 = parse_u32(match option_env!("NS2BENCH_BAUD") { Some(b) => b, None => "921600" });
const WATCHDOG: Duration = Duration::from_millis(1000);
const KEEPALIVE: Duration = Duration::from_millis(500);

const fn parse_u32(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut v = 0u32;
    while i < b.len() {
        v = v * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    v
}

// ------------------------------------------------------------------ descriptors (same as board/gadget.sh)

const PAD_DESC: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, 0x15, 0x00, 0x25, 0x01, 0x35, 0x00, 0x45, 0x01, 0x75, 0x01, 0x95, 0x10, 0x05, 0x09,
    0x19, 0x01, 0x29, 0x10, 0x81, 0x02, 0x05, 0x01, 0x25, 0x07, 0x46, 0x3b, 0x01, 0x75, 0x04, 0x95, 0x01, 0x65, 0x14, 0x09,
    0x39, 0x81, 0x42, 0x65, 0x00, 0x95, 0x01, 0x81, 0x01, 0x26, 0xff, 0x00, 0x46, 0xff, 0x00, 0x09, 0x30, 0x09, 0x31, 0x09,
    0x32, 0x09, 0x35, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02, 0x06, 0x00, 0xff, 0x09, 0x20, 0x95, 0x01, 0x81, 0x02, 0x0a, 0x21,
    0x26, 0x95, 0x08, 0x91, 0x02, 0xc0,
];
const KBD_DESC: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, 0x15, 0x00, 0x25, 0x01,
    0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02,
    0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x65, 0x05, 0x07, 0x19, 0x00, 0x29, 0x65,
    0x81, 0x00, 0xc0,
];
const MOUSE_DESC: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x05, 0x15, 0x00, 0x25, 0x01,
    0x95, 0x05, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38,
    0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x03, 0x81, 0x06, 0xc0, 0xc0,
];

// ------------------------------------------------------------------ shared state

#[derive(Clone, Copy, PartialEq)]
struct Pad([u32; 6]);
const PAD_NEUTRAL: Pad = Pad([0, 8, 128, 128, 128, 128]);

struct Shared {
    pad: Pad,
    kbd: [u8; 7],
    mouse_buttons: u8,
    mouse: (i32, i32, i32), // pending motion
}

static STATE: Mutex<CriticalSectionRawMutex, RefCell<Shared>> =
    Mutex::new(RefCell::new(Shared { pad: PAD_NEUTRAL, kbd: [0; 7], mouse_buttons: 0, mouse: (0, 0, 0) }));
static PAD_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static KBD_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static MOUSE_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static CONFIGURED: AtomicBool = AtomicBool::new(false);
static WRITES: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

fn neutral_all() {
    STATE.lock(|s| {
        let mut s = s.borrow_mut();
        s.pad = PAD_NEUTRAL;
        s.kbd = [0; 7];
        s.mouse_buttons = 0;
        s.mouse = (0, 0, 0);
    });
    PAD_CHANGED.signal(());
    KBD_CHANGED.signal(());
    MOUSE_CHANGED.signal(());
}

// ------------------------------------------------------------------ small log ring for {"logs": n}

/// (last sequence number, the last 16 lines as (seq, text))
type LogRing = (u32, heapless::Deque<(u32, String<64>), 16>);
static LOGS: Mutex<CriticalSectionRawMutex, RefCell<LogRing>> =
    Mutex::new(RefCell::new((0, heapless::Deque::new())));

fn log(text: &str) {
    LOGS.lock(|l| {
        let mut l = l.borrow_mut();
        l.0 += 1;
        let seq = l.0;
        if l.1.is_full() {
            l.1.pop_front();
        }
        let mut s = String::new();
        for c in text.chars() {
            if s.push(c).is_err() {
                break;
            }
        }
        let _ = l.1.push_back((seq, s));
    });
}

// ------------------------------------------------------------------ USB

struct UsbHandler;
impl Handler for UsbHandler {
    fn configured(&mut self, configured: bool) {
        CONFIGURED.store(configured, Ordering::Relaxed);
        log(if configured { "usb configured" } else { "usb unconfigured" });
    }
    fn reset(&mut self) {
        CONFIGURED.store(false, Ordering::Relaxed);
        log("usb reset");
    }
}

/// Accept LED/vendor SET_REPORTs and boot protocol (our reports already use the boot layout).
struct Accept;
impl RequestHandler for Accept {
    fn set_report(&mut self, _id: ReportId, _data: &[u8]) -> OutResponse {
        OutResponse::Accepted
    }
    fn set_protocol(&mut self, _p: HidProtocolMode) -> OutResponse {
        OutResponse::Accepted
    }
    fn set_idle_ms(&mut self, _id: Option<ReportId>, _ms: u32) {}
}

type UsbDriver = Driver<'static, USB>;

#[embassy_executor::task]
async fn usb_task(mut dev: embassy_usb::UsbDevice<'static, UsbDriver>) -> ! {
    dev.run().await
}

#[embassy_executor::task]
async fn pad_task(rw: HidReaderWriter<'static, UsbDriver, 8, 8>) {
    let (mut reader, mut writer) = rw.split();
    let drain = async {
        let mut b = [0u8; 8];
        loop {
            let _ = reader.read(&mut b).await; // vendor OUT reports: accepted, ignored
        }
    };
    let send = async {
        loop {
            writer.ready().await;
            let p = STATE.lock(|s| s.borrow().pad).0;
            let b = (p[0] & 0x3FFF) as u16; // HORI report has no C/GL/GR
            let r = [b as u8, (b >> 8) as u8, p[1] as u8, p[2] as u8, p[3] as u8, p[4] as u8, p[5] as u8, 0];
            if writer.write(&r).await.is_ok() {
                WRITES[0].fetch_add(1, Ordering::Relaxed);
            }
            let _ = select(PAD_CHANGED.wait(), Timer::after(KEEPALIVE)).await;
        }
    };
    embassy_futures::join::join(drain, send).await;
}

#[embassy_executor::task]
async fn kbd_task(mut writer: HidWriter<'static, UsbDriver, 8>) {
    loop {
        writer.ready().await;
        let k = STATE.lock(|s| s.borrow().kbd);
        let r = [k[0], 0, k[1], k[2], k[3], k[4], k[5], k[6]];
        if writer.write(&r).await.is_ok() {
            WRITES[1].fetch_add(1, Ordering::Relaxed);
        }
        let _ = select(KBD_CHANGED.wait(), Timer::after(KEEPALIVE)).await;
    }
}

#[embassy_executor::task]
async fn mouse_task(mut writer: HidWriter<'static, UsbDriver, 4>) {
    loop {
        MOUSE_CHANGED.wait().await;
        loop {
            writer.ready().await;
            // motion is queued and sent in <=127 steps; buttons are state
            let (btn, sx, sy, sw, more) = STATE.lock(|s| {
                let mut s = s.borrow_mut();
                let (dx, dy, dw) = s.mouse;
                let (sx, sy, sw) = (dx.clamp(-127, 127), dy.clamp(-127, 127), dw.clamp(-127, 127));
                s.mouse = (dx - sx, dy - sy, dw - sw);
                (s.mouse_buttons, sx, sy, sw, s.mouse != (0, 0, 0))
            });
            if writer.write(&[btn, sx as i8 as u8, sy as i8 as u8, sw as i8 as u8]).await.is_ok() {
                WRITES[2].fetch_add(1, Ordering::Relaxed);
            }
            if !more {
                break;
            }
        }
    }
}

// ------------------------------------------------------------------ host link (UART, JSON lines)

#[derive(Deserialize)]
struct Msg<'a> {
    #[serde(borrow, default)]
    hello: Option<&'a str>,
    #[serde(default)]
    pad: Option<[i64; 6]>,
    #[serde(default)]
    kbd: Option<[i64; 7]>,
    #[serde(default)]
    mouse: Option<[i64; 4]>,
    #[serde(default)]
    ping: Option<u64>,
    #[serde(default)]
    logs: Option<u64>,
    #[serde(borrow, default)]
    usb_mode: Option<&'a str>,
}

fn udc() -> &'static str {
    if CONFIGURED.load(Ordering::Relaxed) { "configured" } else { "not attached" }
}

async fn send(tx: &mut BufferedUartTx, s: &str) {
    let _ = tx.write_all(s.as_bytes()).await;
    let _ = tx.write_all(b"\n").await;
    let _ = tx.flush().await;
}

/// Returns false when the session must end.
async fn handle(line: &[u8], authed: &mut bool, tx: &mut BufferedUartTx) -> bool {
    let Ok((m, _)) = serde_json_core::from_slice::<Msg>(line) else {
        send(tx, "{\"error\": \"bad message\"}").await;
        return true;
    };
    let mut out: String<1024> = String::new();
    if !*authed {
        let Some(h) = m.hello else {
            send(tx, "{\"error\": \"auth\"}").await;
            return false;
        };
        if !TOKEN.is_empty() && h != TOKEN {
            send(tx, "{\"error\": \"auth\"}").await;
            return false;
        }
        *authed = true;
        neutral_all();
        log("client ready");
        let seq = LOGS.lock(|l| l.borrow().0);
        let _ = write!(out, "{{\"ok\": \"ready\", \"udc\": \"{}\", \"watchdog_s\": 1.0, \"devices\": [\"kbd\", \"mouse\", \"pad\"], \"log_seq\": {}, \"mode\": \"standard\", \"board\": \"pico\"}}", udc(), seq);
        send(tx, &out).await;
        return true;
    }
    if let Some(p) = m.pad {
        let ok = (0..=0x1FFFF).contains(&p[0]) && (0..=8).contains(&p[1]) && p[2..].iter().all(|a| (0..=255).contains(a));
        if !ok {
            send(tx, "{\"error\": \"bad pad\"}").await;
            return true;
        }
        let new = Pad([p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32, p[4] as u32, p[5] as u32]);
        if STATE.lock(|s| core::mem::replace(&mut s.borrow_mut().pad, new)) != new {
            PAD_CHANGED.signal(());
        }
    }
    if let Some(k) = m.kbd {
        if !((0..=255).contains(&k[0]) && k[1..].iter().all(|x| (0..=0xE7).contains(x))) {
            send(tx, "{\"error\": \"bad kbd\"}").await;
            return true;
        }
        let new: [u8; 7] = core::array::from_fn(|i| k[i] as u8);
        if STATE.lock(|s| core::mem::replace(&mut s.borrow_mut().kbd, new)) != new {
            KBD_CHANGED.signal(());
        }
    }
    if let Some(v) = m.mouse {
        if !((0..=0x1F).contains(&v[0]) && v[1..3].iter().all(|x| (-4096..=4096).contains(x)) && (-64..=64).contains(&v[3])) {
            send(tx, "{\"error\": \"bad mouse\"}").await;
            return true;
        }
        STATE.lock(|s| {
            let mut s = s.borrow_mut();
            s.mouse_buttons = v[0] as u8;
            s.mouse.0 += v[1] as i32;
            s.mouse.1 += v[2] as i32;
            s.mouse.2 += v[3] as i32;
        });
        MOUSE_CHANGED.signal(());
    }
    if m.usb_mode.is_some() {
        send(tx, "{\"error\": \"the Pico has one USB mode (standard)\"}").await;
    }
    if let Some(after) = m.logs {
        let _ = write!(out, "{{\"logs\": [");
        let seq = LOGS.lock(|l| {
            let l = l.borrow();
            let mut first = true;
            for (s, t) in l.1.iter().filter(|(s, _)| *s as u64 > after) {
                let _ = write!(out, "{}[{}, {}, \"pico\", \"{}\"]", if first { "" } else { ", " }, s, Instant::now().as_millis() as f64 / 1000.0, t);
                first = false;
            }
            l.0
        });
        let _ = write!(out, "], \"seq\": {}, \"now\": {}}}", seq, Instant::now().as_millis() as f64 / 1000.0);
        send(tx, &out).await;
        out.clear();
    }
    if let Some(n) = m.ping {
        let _ = write!(out, "{{\"pong\": {}, \"udc\": \"{}\", \"writes\": {{\"pad\": {}, \"kbd\": {}, \"mouse\": {}}}}}", n, udc(),
            WRITES[0].load(Ordering::Relaxed), WRITES[1].load(Ordering::Relaxed), WRITES[2].load(Ordering::Relaxed));
        send(tx, &out).await;
    }
    true
}

#[embassy_executor::task]
async fn link_task(mut rx: BufferedUartRx, mut tx: BufferedUartTx) {
    let mut line: heapless::Vec<u8, 4096> = heapless::Vec::new();
    let mut authed = false;
    let mut last = Instant::now();
    let mut chunk = [0u8; 64];
    loop {
        let limit = if authed { WATCHDOG } else { Duration::from_secs(5) };
        match select(rx.read(&mut chunk), Timer::at(last + limit)).await {
            Either::First(Ok(n)) => {
                // (only a whole line counts as the host being there: noise on an unplugged
                // serial line must not keep held buttons alive)
                let mut whole = false;
                for &b in &chunk[..n] {
                    if b == b'\n' {
                        whole |= !line.is_empty();
                        if !line.is_empty() && !handle(&line, &mut authed, &mut tx).await {
                            authed = false;
                            neutral_all();
                        }
                        line.clear();
                    } else if line.push(b).is_err() {
                        line.clear(); // over-long line: drop it
                    }
                }
                if whole { last = Instant::now(); }
                // (bytes that keep coming without ever making a line must not hold the timer off)
                else if authed && Instant::now() >= last + limit {
                    log("watchdog: no whole line, releasing");
                    authed = false;
                    neutral_all();
                    last = Instant::now();
                }
            }
            Either::First(Err(_)) => {}
            Either::Second(()) => {
                if authed {
                    log("watchdog: silent, releasing");
                    authed = false;
                    neutral_all();
                }
                last = Instant::now();
            }
        }
    }
}

// ------------------------------------------------------------------ main

macro_rules! mk_static {
    ($t:ty, $v:expr) => {{
        static S: StaticCell<$t> = StaticCell::new();
        S.init($v)
    }};
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // host link
    let mut ucfg = UartConfig::default();
    ucfg.baudrate = BAUD;
    let uart = BufferedUart::new(p.UART0, p.PIN_0, p.PIN_1, Irqs, mk_static!([u8; 1024], [0; 1024]), mk_static!([u8; 4096], [0; 4096]), ucfg);
    let (tx, rx) = uart.split();

    // console-facing USB device, same identity as the Linux boards' standard mode
    let driver = Driver::new(p.USB, Irqs);
    let mut config = embassy_usb::Config::new(0x0F0D, 0x0092);
    config.manufacturer = Some("PICO SWITCH PRO.");
    config.product = Some("J24 CONTROLLER");
    config.serial_number = None;
    config.device_release = 0x0100;
    config.max_power = 500;
    config.max_packet_size_0 = 64;
    config.device_class = 0x00;
    config.device_sub_class = 0x00;
    config.device_protocol = 0x00;
    config.composite_with_iads = false;

    let mut builder = Builder::new(driver, config, mk_static!([u8; 256], [0; 256]), mk_static!([u8; 64], [0; 64]),
        &mut [], mk_static!([u8; 64], [0; 64]));
    builder.handler(mk_static!(UsbHandler, UsbHandler));

    let pad = HidReaderWriter::<_, 8, 8>::new(&mut builder, mk_static!(hid::State, hid::State::new()), hid::Config {
        report_descriptor: PAD_DESC, request_handler: Some(mk_static!(Accept, Accept)), poll_ms: 1, max_packet_size: 64,
        hid_subclass: hid::HidSubclass::No, hid_boot_protocol: hid::HidBootProtocol::None,
    });
    let kbd = HidWriter::<_, 8>::new(&mut builder, mk_static!(hid::State, hid::State::new()), hid::Config {
        report_descriptor: KBD_DESC, request_handler: Some(mk_static!(Accept, Accept)), poll_ms: 1, max_packet_size: 8,
        hid_subclass: hid::HidSubclass::Boot, hid_boot_protocol: hid::HidBootProtocol::Keyboard,
    });
    let mouse = HidWriter::<_, 4>::new(&mut builder, mk_static!(hid::State, hid::State::new()), hid::Config {
        report_descriptor: MOUSE_DESC, request_handler: Some(mk_static!(Accept, Accept)), poll_ms: 1, max_packet_size: 4,
        hid_subclass: hid::HidSubclass::Boot, hid_boot_protocol: hid::HidBootProtocol::Mouse,
    });
    let usb = builder.build();
    log("ns2bench-pico started");

    spawner.spawn(usb_task(usb).unwrap());
    spawner.spawn(pad_task(pad).unwrap());
    spawner.spawn(kbd_task(kbd).unwrap());
    spawner.spawn(mouse_task(mouse).unwrap());
    spawner.spawn(link_task(rx, tx).unwrap());
}
