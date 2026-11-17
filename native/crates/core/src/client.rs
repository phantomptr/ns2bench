//! Minimal HTTP/1.1 client for talking to a running NS2Bench server (MCP server and CLI).
//! Loopback/LAN only, one request per connection; no extra dependencies.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
pub struct Client {
    pub base: String, // http://host:port
    pub token: Option<String>,
    pub source: &'static str,
    /// The console every request is about (several consoles on one bench), or the default.
    pub console: Option<String>,
}

pub struct Resp {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn json(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&self.body)?)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

pub fn query(params: &[(&str, String)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    "?".to_string() + &params.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect::<Vec<_>>().join("&")
}

impl Client {
    pub fn from_config(cfg: &crate::Config, source: &'static str) -> Client {
        Client { base: cfg.url(), token: cfg.token(), source, console: None }
    }

    pub async fn get(&self, path: &str) -> Result<Resp> {
        self.request("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Resp> {
        self.request("POST", path, Some(body)).await
    }

    /// Like get/post but errors on HTTP >= 400 with the server's message.
    pub async fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Resp> {
        let r = self.request(method, path, body).await?;
        if r.status >= 400 {
            bail!("{}: {}", r.status, r.text().trim());
        }
        Ok(r)
    }

    pub async fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Resp> {
        let hostport = self.base.trim_start_matches("http://").trim_end_matches('/').to_string();
        let mut sock = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(&hostport)).await
            .map_err(|_| anyhow!("timed out connecting to {}", self.base))?
            .map_err(|_| anyhow!("NS2Bench server is not running at {}; start it with `ns2bench serve`", self.base))?;
        let data = body.map(|b| b.to_string()).unwrap_or_default();
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nX-NS2Bench-Source: {}\r\n", self.source);
        if let Some(t) = &self.token {
            req += &format!("Authorization: Bearer {t}\r\n");
        }
        // (a header can't carry a line break: names with one are dropped)
        if let Some(c) = self.console.as_deref().filter(|c| !c.contains(['\r', '\n'])) {
            req += &format!("X-NS2Bench-Console: {c}\r\n");
        }
        if body.is_some() {
            req += &format!("Content-Type: application/json\r\nContent-Length: {}\r\n", data.len());
        }
        req += "\r\n";
        req += &data;
        sock.write_all(req.as_bytes()).await?;
        let mut raw = vec![];
        tokio::time::timeout(Duration::from_secs(3600), sock.read_to_end(&mut raw)).await
            .map_err(|_| anyhow!("request timed out"))??;
        parse(&raw)
    }
}

fn parse(raw: &[u8]) -> Result<Resp> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| anyhow!("bad HTTP response"))?;
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("bad HTTP status line"))?;
    let (mut ct, mut chunked) = (String::new(), false);
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            if k == "content-type" {
                ct = v.trim().to_string();
            } else if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
        }
    }
    let mut body = raw[end + 4..].to_vec();
    if chunked {
        let mut out = vec![];
        let mut rest = &body[..];
        while let Some(p) = rest.windows(2).position(|w| w == b"\r\n") {
            let n = usize::from_str_radix(String::from_utf8_lossy(&rest[..p]).split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
            if n == 0 || n > rest.len() || rest.len() < p + 2 + n {
                break;
            }
            out.extend_from_slice(&rest[p + 2..p + 2 + n]);
            rest = &rest[(p + 4 + n).min(rest.len())..];
        }
        body = out;
    }
    Ok(Resp { status, content_type: ct, body })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_plain_and_chunked() {
        let r = super::parse(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!((r.status, r.body.as_slice(), r.content_type.as_str()), (200, b"{}".as_slice(), "application/json"));
        let r = super::parse(b"HTTP/1.1 409 Conflict\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (409, b"abcde".as_slice()));
        assert_eq!(super::query(&[("region", "0,0.5".into())]), "?region=0%2C0.5");
    }
}
