//! How the host reaches the input board. Same line protocol over either link:
//!   tcp     Linux USB-gadget boards running ns2bench-board hidd ("input.backend": "hidd")
//!   serial  microcontrollers over USB CDC / UART, e.g. firmware/pico ("input.backend": "serial")

use anyhow::{bail, Result};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

pub type Reader = Box<dyn AsyncRead + Unpin + Send>;
pub type Writer = Box<dyn AsyncWrite + Unpin + Send>;

#[derive(Clone, Debug)]
pub enum Link {
    Tcp { host: String, port: u16 },
    Serial { path: String, baud: u32 },
    None,
}

impl Link {
    pub fn from_config(cfg: &Value) -> Link {
        match cfg["backend"].as_str().unwrap_or("none") {
            "hidd" => match cfg["host"].as_str() {
                Some(h) => Link::Tcp { host: h.into(), port: cfg["port"].as_u64().unwrap_or(7631) as u16 },
                None => Link::None,
            },
            "serial" => match cfg["serial_port"].as_str() {
                Some(p) => Link::Serial { path: p.into(), baud: cfg["baud"].as_u64().unwrap_or(921600) as u32 },
                None => Link::None,
            },
            _ => Link::None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Link::Tcp { host, port } => format!("{host}:{port}"),
            Link::Serial { path, baud } => format!("{path} @{baud}"),
            Link::None => "none".into(),
        }
    }

    pub async fn connect(&self) -> Result<(Reader, Writer)> {
        match self {
            Link::Tcp { host, port } => {
                let s = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect((host.as_str(), *port))).await??;
                s.set_nodelay(true)?;
                let (r, w) = s.into_split();
                Ok((Box::new(r), Box::new(w)))
            }
            Link::Serial { path, baud } => {
                configure_serial(path, *baud).await;
                // Unix: one non-blocking handle watched by the runtime, so a read that never gets
                // an answer costs nothing and ends when the connection is dropped (a blocking read
                // would keep a thread forever, one more for every reconnect to a silent device).
                #[cfg(unix)]
                if let Ok(port) = unix_serial::open(path, *baud) {
                    return Ok((Box::new(unix_serial::Half(port.clone())), Box::new(unix_serial::Half(port))));
                }
                // Windows: reads that time out every 200 ms on one reader thread, which ends with the
                // connection (a plain blocking read would keep a thread forever on a silent device).
                #[cfg(windows)]
                if let Ok((r, w)) = win_serial::open(path) {
                    return Ok((Box::new(r), Box::new(w)));
                }
                // Separate handles for reading and writing: a tokio File runs one operation at a time,
                // so a split File would block every write behind the pending read.
                let f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
                let w = f.try_clone()?;
                Ok((Box::new(tokio::fs::File::from_std(f)), Box::new(tokio::fs::File::from_std(w))))
            }
            Link::None => bail!("input.host / input.serial_port must be set"),
        }
    }
}

/// Raw mode + baud. USB CDC ignores the baud rate; real UARTs need it.
async fn configure_serial(path: &str, baud: u32) {
    let b = baud.to_string();
    #[cfg(target_os = "macos")]
    let args = vec!["-f", path, &b, "raw", "-echo", "clocal"];
    #[cfg(target_os = "linux")]
    let args = vec!["-F", path, &b, "raw", "-echo", "clocal"];
    #[cfg(unix)]
    {
        let _ = tokio::process::Command::new("stty").args(&args).status().await;
    }
    #[cfg(windows)]
    {
        let port = path.trim_start_matches(r"\\.\");
        // `mode` is mode.com: Command::new("mode") would look for mode.exe and fail.
        let mode = std::path::Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join(r"System32\mode.com");
        let _ = tokio::process::Command::new(mode).args([port, &format!("BAUD={b}"), "PARITY=N", "DATA=8", "STOP=1"]).status().await;
    }
}

#[cfg(windows)]
pub(crate) mod win_serial {
    use std::io::{self, Read};
    use std::os::windows::io::AsRawHandle;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, ReadBuf};
    use tokio::sync::mpsc;
    use windows_sys::Win32::Devices::Communication::{SetCommTimeouts, COMMTIMEOUTS};

    /// Reads that return within 200 ms even when nothing arrives (0 bytes), and writes that give
    /// up after 2 s: so the reader thread can notice the connection is gone and end.
    fn timeouts(f: &std::fs::File) -> io::Result<()> {
        let t = COMMTIMEOUTS {
            ReadIntervalTimeout: u32::MAX,
            ReadTotalTimeoutMultiplier: u32::MAX,
            ReadTotalTimeoutConstant: 200,
            WriteTotalTimeoutMultiplier: 0,
            WriteTotalTimeoutConstant: 2000,
        };
        // SAFETY: the handle belongs to `f`, which is open for the duration of the call.
        if unsafe { SetCommTimeouts(f.as_raw_handle() as _, &t) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn open(path: &str) -> io::Result<(ChanReader, tokio::fs::File)> {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
        timeouts(&f)?;
        let w = f.try_clone()?;
        let (tx, rx) = mpsc::channel::<io::Result<Vec<u8>>>(64);
        std::thread::Builder::new().name("ns2bench-serial".into()).spawn(move || {
            let mut f = f;
            let mut buf = [0u8; 512];
            // Ends when the connection is dropped (nobody receives) or the port goes away.
            while !tx.is_closed() {
                match f.read(&mut buf) {
                    Ok(0) => continue, // nothing within 200 ms
                    Ok(n) => {
                        if tx.blocking_send(Ok(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.blocking_send(Err(e));
                        break;
                    }
                }
            }
        })?;
        Ok((ChanReader { rx, pending: Vec::new(), at: 0 }, tokio::fs::File::from_std(w)))
    }

    /// The reading side: bytes the reader thread received.
    pub struct ChanReader {
        rx: mpsc::Receiver<io::Result<Vec<u8>>>,
        pending: Vec<u8>,
        at: usize,
    }

    impl AsyncRead for ChanReader {
        fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            if self.at >= self.pending.len() {
                match self.rx.poll_recv(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(None) => return Poll::Ready(Ok(())), // port closed: end of input
                    Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(e)),
                    Poll::Ready(Some(Ok(data))) => {
                        self.pending = data;
                        self.at = 0;
                    }
                }
            }
            let n = buf.remaining().min(self.pending.len() - self.at);
            let at = self.at;
            buf.put_slice(&self.pending[at..at + n]);
            self.at += n;
            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(unix)]
pub(crate) mod unix_serial {
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{ready, Context, Poll};
    use tokio::io::unix::AsyncFd;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    /// Open the port non-blocking and set it up on this handle (raw mode, baud): settings made
    /// by a separate `stty` do not always outlive that program on macOS.
    pub fn open(path: &str, baud: u32) -> io::Result<Arc<AsyncFd<File>>> {
        let f = std::fs::OpenOptions::new().read(true).write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY).open(path)?;
        configure(&f, baud);
        // SAFETY: the AsyncFd owns the File, so the descriptor stays open for as long as it is registered.
        Ok(Arc::new(unsafe { AsyncFd::register(f) }?))
    }

    fn speed(baud: u32) -> Option<libc::speed_t> {
        Some(match baud {
            9600 => libc::B9600, 19200 => libc::B19200, 38400 => libc::B38400, 57600 => libc::B57600,
            115200 => libc::B115200, 230400 => libc::B230400,
            #[cfg(target_os = "linux")]
            460800 => libc::B460800,
            #[cfg(target_os = "linux")]
            921600 => libc::B921600,
            // macOS takes the number itself for speeds it has no name for
            #[cfg(target_os = "macos")]
            460800 | 921600 => baud as libc::speed_t,
            _ => return None,
        })
    }

    /// Best effort: a pseudo-terminal or a USB CDC port that refuses a setting still works.
    fn configure(f: &File, baud: u32) {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(f.as_raw_fd(), &mut t) != 0 {
                return;
            }
            libc::cfmakeraw(&mut t);
            t.c_cflag |= libc::CLOCAL | libc::CREAD;
            if let Some(s) = speed(baud) {
                libc::cfsetspeed(&mut t, s);
            }
            libc::tcsetattr(f.as_raw_fd(), libc::TCSANOW, &t);
        }
    }

    /// Reading or writing side of the shared port.
    pub struct Half(pub Arc<AsyncFd<File>>);

    impl AsyncRead for Half {
        fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            loop {
                let mut guard = ready!(self.0.poll_read_ready(cx))?;
                match guard.try_io(|fd| fd.get_ref().read(buf.initialize_unfilled())) {
                    Ok(Ok(n)) => {
                        buf.advance(n);
                        return Poll::Ready(Ok(()));
                    }
                    Ok(Err(e)) => return Poll::Ready(Err(e)),
                    Err(_not_ready) => continue,
                }
            }
        }
    }

    impl AsyncWrite for Half {
        fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
            loop {
                let mut guard = ready!(self.0.poll_write_ready(cx))?;
                match guard.try_io(|fd| fd.get_ref().write(data)) {
                    Ok(r) => return Poll::Ready(r),
                    Err(_not_ready) => continue,
                }
            }
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::FromRawFd;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A pseudo-terminal stands in for the serial port: data flows both ways, and a read that
    /// gets no answer times out without leaving anything behind.
    #[tokio::test]
    async fn serial_link_reads_writes_and_gives_up_cleanly() {
        let (mut master, mut slave) = (0, 0);
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
        let path = unsafe { std::ffi::CStr::from_ptr(libc::ttyname(slave)) }.to_str().unwrap().to_string();
        let mut far = unsafe { std::fs::File::from_raw_fd(master) };
        let port = unix_serial::open(&path, 115200).expect("open the pty like a serial port");
        let (mut r, mut w) = (unix_serial::Half(port.clone()), unix_serial::Half(port));
        // nothing to read yet: the wait ends with the timeout, not with a stuck thread
        let mut buf = [0u8; 16];
        assert!(tokio::time::timeout(Duration::from_millis(100), r.read(&mut buf)).await.is_err());
        use std::io::{Read, Write};
        far.write_all(b"hello\n").unwrap();
        let n = tokio::time::timeout(Duration::from_secs(2), r.read(&mut buf)).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"hello\n");
        w.write_all(b"ping\n").await.unwrap();
        let mut got = [0u8; 5];
        far.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"ping\n");
        unsafe { libc::close(slave) };
    }
}
