//! Real daemon lifecycle with pseudo-terminals in place of USB devices. No hardware required.
#![cfg(unix)]
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::fd::FromRawFd,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Daemon {
    child: Child,
    dir: std::path::PathBuf,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn pty() -> (File, File, String) {
    unsafe {
        let (mut master, mut slave) = (-1, -1);
        let mut name = [0 as libc::c_char; 128];
        assert_eq!(
            libc::openpty(
                &mut master,
                &mut slave,
                name.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            0
        );
        let mut term = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave, &mut term), 0);
        libc::cfmakeraw(&mut term);
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &term), 0);
        libc::fcntl(master, libc::F_SETFL, libc::O_NONBLOCK);
        libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(slave, libc::F_SETFD, libc::FD_CLOEXEC);
        (
            File::from_raw_fd(master),
            File::from_raw_fd(slave),
            std::ffi::CStr::from_ptr(name.as_ptr())
                .to_str()
                .unwrap()
                .into(),
        )
    }
}
fn send(s: &mut BufReader<TcpStream>, v: Value) {
    writeln!(s.get_mut(), "{v}").unwrap();
}
fn reply(s: &mut BufReader<TcpStream>) -> Value {
    let mut line = String::new();
    s.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn report(f: &mut File, expected: &[u8], size: usize) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    loop {
        let mut buf = [0u8; 1024];
        match f.read(&mut buf) {
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("{e}"),
        }
        if bytes.chunks_exact(size).any(|r| r == expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "missing report {expected:?}, got {bytes:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn auth_exclusivity_validation_reports_and_watchdog() {
    let (mut pad, _ps, pp) = pty();
    let (mut kbd, _ks, kp) = pty();
    let (_mouse, _ms, mp) = pty();
    let reserve = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserve.local_addr().unwrap();
    drop(reserve);
    let dir = std::env::temp_dir().join(format!("ns2bench-hidd-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("token"), "synthetic-test-token-1234").unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_ns2bench-board"))
        .args([
            "hidd",
            "--bind",
            "127.0.0.1",
            "--port",
            &addr.port().to_string(),
            "--token-file",
            dir.join("token").to_str().unwrap(),
            "--pad",
            &pp,
            "--kbd",
            &kp,
            "--mouse",
            &mp,
            "--kmsg",
            "/nonexistent",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _daemon = Daemon { child, dir };
    let connect = || {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match TcpStream::connect(addr) {
                Ok(s) => {
                    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                    return BufReader::new(s);
                }
                Err(_) => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    };
    let mut bad = connect();
    send(&mut bad, json!({"hello":"wrong"}));
    assert_eq!(reply(&mut bad)["error"], "auth");
    drop(bad);
    let mut s = connect();
    send(&mut s, json!({"hello":"synthetic-test-token-1234"}));
    assert_eq!(reply(&mut s)["ok"], "ready");
    let mut busy = connect();
    assert_eq!(reply(&mut busy)["error"], "busy");
    drop(busy);
    send(&mut s, json!({"pad":[1,9,0,0,0,0]}));
    assert_eq!(reply(&mut s)["error"], "bad pad");
    send(
        &mut s,
        json!({"pad":[4,8,128,128,128,128],"kbd":[2,4,0,0,0,0,0]}),
    );
    report(&mut pad, &[4, 0, 8, 128, 128, 128, 128, 0], 8);
    report(&mut kbd, &[2, 0, 4, 0, 0, 0, 0, 0], 8);
    send(&mut s, json!({"ping":7}));
    assert_eq!(reply(&mut s)["pong"], 7);
    send(&mut s, json!({"logs":0}));
    assert!(reply(&mut s)["logs"].is_array());
    // Silence must release a held button and close the client, rather than leaving input stuck.
    report(&mut pad, &[0, 0, 8, 128, 128, 128, 128, 0], 8);
    report(&mut kbd, &[0; 8], 8);
    let mut line = String::new();
    assert_eq!(s.read_line(&mut line).unwrap(), 0);
    drop(s);
    let mut again = connect();
    send(&mut again, json!({"hello":"synthetic-test-token-1234"}));
    assert_eq!(reply(&mut again)["ok"], "ready");
}
