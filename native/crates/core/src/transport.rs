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
