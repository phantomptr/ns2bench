//! Offline protocol handlers. No resolver, upstream client, proxy, NAT or route management.
#[cfg(unix)]
pub mod control;
#[cfg(unix)]
pub mod runtime;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

pub const CTEST_HOSTS: [&str; 2] = ["ctest.p01.ctest.srv.nintendo.net", "ctest.cdn.nintendo.net"];
const MAGIC: [u8; 4] = [99, 130, 83, 99];
pub type Leases = BTreeMap<String, Value>;

/// The only network settings a host may change. Isolation (no forwarding, NAT,
/// proxy or upstream DNS) is not a setting. Shared by the engine and the helper.
pub fn validate_controls(v: &Value) -> Result<(), String> {
    let o = v.as_object().ok_or("object required")?;
    if !o.keys().all(|k| ["dhcp", "dns", "connectivity", "topology"].contains(&k.as_str())) {
        return Err("unknown network control; isolation cannot be disabled".into());
    }
    for key in ["dhcp", "dns", "connectivity"] {
        if !v[key].is_boolean() {
            return Err(format!("{key} must be boolean"));
        }
    }
    if !matches!(v["topology"].as_str(), Some("direct" | "shared" | "router" | "none")) {
        return Err("unknown network topology".into());
    }
    // shared: a home network that already has a router and its own DHCP. Only DNS and the
    // test page, only for the listed consoles; handing out addresses there is never allowed.
    if v["topology"] == "shared" && v["dhcp"] == true {
        return Err("DHCP stays off on a shared network: the router hands out addresses there".into());
    }
    if matches!(v["topology"].as_str(), Some("router" | "none")) && ["dhcp", "dns", "connectivity"].iter().any(|k| v[*k] == true) {
        return Err("local services must be off for an external router or unmanaged network".into());
    }
    Ok(())
}

pub struct Config {
    pub interface: String,
    pub address: Ipv4Addr,
    mask: Ipv4Addr,
    network: u32,
    broadcast: Ipv4Addr,
    pool: Vec<Ipv4Addr>,
    /// Shared network: the only addresses DNS and the test page answer.
    pub consoles: Vec<Ipv4Addr>,
    lease_s: u32,
    domain: String,
    reservations: BTreeMap<String, Ipv4Addr>,
}

impl Config {
    pub fn from_value(data: &Value) -> Result<Self, String> {
        let n = &data["network"];
        let text = |key: &str| {
            n[key]
                .as_str()
                .ok_or_else(|| format!("network.{key} required"))
        };
        let ip = |key: &str| {
            text(key)?
                .parse::<Ipv4Addr>()
                .map_err(|_| format!("invalid network.{key}"))
        };
        let interface = text("interface")?.to_string();
        if interface.is_empty() || interface.len() >= 16 || interface.as_bytes().contains(&0) {
            return Err("invalid interface name".into());
        }
        let address = ip("address")?;
        let (network, prefix) = text("subnet")?
            .split_once('/')
            .ok_or("subnet must be IPv4 CIDR")?;
        let network = u32::from(network.parse::<Ipv4Addr>().map_err(|_| "invalid subnet")?);
        let prefix = prefix.parse::<u32>().map_err(|_| "invalid subnet prefix")?;
        if !(8..=30).contains(&prefix) {
            return Err("subnet must have usable private host addresses".into());
        }
        let mask = u32::MAX << (32 - prefix);
        let broadcast = network | !mask;
        if network & mask != network
            || !Ipv4Addr::from(network).is_private()
            || !Ipv4Addr::from(broadcast).is_private()
        {
            return Err("subnet must be an aligned RFC1918 network".into());
        }
        let valid = |ip: Ipv4Addr| {
            u32::from(ip) > network && u32::from(ip) < broadcast && u32::from(ip) & mask == network
        };
        if !valid(address) {
            return Err("interface address must be a private subnet host".into());
        }
        // The DHCP pool is optional: a shared network has none (the router hands out addresses).
        let pool: Vec<Ipv4Addr> = if n.get("pool_start").is_none() && n.get("pool_end").is_none() {
            vec![]
        } else {
            let (start, end) = (u32::from(ip("pool_start")?), u32::from(ip("pool_end")?));
            if start > end || end - start >= 65536 {
                return Err("invalid or oversized DHCP pool".into());
            }
            (start..=end).map(Ipv4Addr::from).collect()
        };
        if pool.iter().any(|&ip| !valid(ip) || ip == address) {
            return Err("invalid DHCP pool address".into());
        }
        let mut reservations = BTreeMap::new();
        let mut reserved = BTreeSet::new();
        if let Some(v) = n.get("reservations") {
            let v = v.as_object().ok_or("reservations must be an object")?;
            for (mac, addr) in v {
                let mac = mac.to_lowercase();
                let parts: Vec<_> = mac.split(':').collect();
                if parts.len() != 6
                    || parts
                        .iter()
                        .any(|p| p.len() != 2 || u8::from_str_radix(p, 16).is_err())
                {
                    return Err("invalid reservation MAC".into());
                }
                let ip = addr
                    .as_str()
                    .ok_or("invalid reservation address")?
                    .parse::<Ipv4Addr>()
                    .map_err(|_| "invalid reservation address")?;
                if !valid(ip)
                    || ip == address
                    || !reserved.insert(ip)
                    || reservations.insert(mac, ip).is_some()
                {
                    return Err("invalid or duplicate reservation".into());
                }
            }
        }
        let mut consoles = vec![];
        if let Some(v) = n.get("consoles") {
            for c in v.as_array().ok_or("consoles must be a list of addresses")? {
                let ip = c.as_str().and_then(|s| s.parse::<Ipv4Addr>().ok()).ok_or("invalid console address")?;
                if !valid(ip) || ip == address || consoles.contains(&ip) {
                    return Err(format!("console {ip} must be another address on {}/{prefix}", Ipv4Addr::from(network)));
                }
                consoles.push(ip);
            }
            if consoles.len() > 16 {
                return Err("at most 16 consoles".into());
            }
        }
        let domain = n
            .get("domain")
            .and_then(Value::as_str)
            .unwrap_or("ns2.test")
            .to_string();
        if domain.len() > 253
            || domain.split('.').any(|l| {
                l.is_empty()
                    || l.len() > 63
                    || !l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err("invalid local domain".into());
        }
        let lease_s = n
            .get("lease_seconds")
            .map_or(Some(43200), Value::as_u64)
            .ok_or("invalid lease_seconds")?;
        if !(60..=u32::MAX as u64).contains(&lease_s) {
            return Err("invalid lease_seconds".into());
        }
        Ok(Self {
            interface,
            address,
            network,
            mask: mask.into(),
            broadcast: broadcast.into(),
            pool,
            consoles,
            lease_s: lease_s as u32,
            domain,
            reservations,
        })
    }

    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        u32::from(ip) & u32::from(self.mask) == self.network
    }

    /// May this peer use DNS and the test page? Direct link: anyone on it. Shared network: only
    /// the listed consoles (none listed: no one).
    pub fn serves(&self, ip: Ipv4Addr, shared: bool) -> bool {
        self.contains(ip) && (!shared || self.consoles.contains(&ip))
    }

    /// The pool as text for the log ("" without one).
    pub fn pool_text(&self) -> String {
        match (self.pool.first(), self.pool.last()) {
            (Some(a), Some(b)) => format!("{a}-{b}"),
            _ => String::new(),
        }
    }

    pub fn dhcp_reply(&self, req: &Dhcp, kind: u8, yiaddr: Option<Ipv4Addr>) -> Vec<u8> {
        let mut p = vec![0u8; 240];
        p[..3].copy_from_slice(&[2, 1, 6]);
        p[4..8].copy_from_slice(&req.xid);
        p[10..12].copy_from_slice(&req.flags);
        if kind == 5 && yiaddr.is_none() {
            p[12..16].copy_from_slice(&req.ciaddr.octets());
        }
        if let Some(ip) = yiaddr {
            p[16..20].copy_from_slice(&ip.octets());
        }
        p[20..24].copy_from_slice(&self.address.octets());
        p[28..44].copy_from_slice(&req.chaddr);
        p[236..240].copy_from_slice(&MAGIC);
        let mut opt = |code: u8, v: &[u8]| {
            p.extend_from_slice(&[code, v.len() as u8]);
            p.extend_from_slice(v);
        };
        opt(53, &[kind]);
        opt(54, &self.address.octets());
        if kind != 6 {
            opt(1, &self.mask.octets());
            opt(3, &self.address.octets());
            opt(6, &self.address.octets());
            opt(15, self.domain.as_bytes());
            opt(28, &self.broadcast.octets());
            if yiaddr.is_some() {
                opt(51, &self.lease_s.to_be_bytes());
                opt(58, &(self.lease_s / 2).to_be_bytes());
                opt(59, &((self.lease_s as u64 * 7 / 8) as u32).to_be_bytes());
            }
        }
        p.push(255);
        p
    }

    /// Answers only fixed local names. Unknown names always receive NXDOMAIN.
    pub fn dns_reply(&self, packet: &[u8]) -> Option<(Vec<u8>, String)> {
        if packet.len() < 12 || packet[2] & 0xf8 != 0 || packet[4..6] != [0, 1] {
            return None;
        }
        let mut labels = Vec::new();
        let mut pos = 12;
        loop {
            let size = *packet.get(pos)? as usize;
            pos += 1;
            if size == 0 {
                break;
            }
            if size > 63 {
                return None;
            }
            let label = packet.get(pos..pos + size)?;
            if !label.is_ascii() {
                return None;
            }
            labels.push(std::str::from_utf8(label).ok()?);
            pos += size;
            if pos - 12 > 253 {
                return None;
            }
        }
        let question_tail = packet.get(pos..pos + 4)?;
        let name = labels.join(".").to_ascii_lowercase();
        let local = CTEST_HOSTS.contains(&name.as_str()) || name == format!("mac.{}", self.domain);
        let answer = local && question_tail == [0, 1, 0, 1];
        let flags = 0x8480u16 | ((packet[2] as u16 & 1) << 8) | if local { 0 } else { 3 };
        let mut p = packet[..2].to_vec();
        p.extend_from_slice(&flags.to_be_bytes());
        p.extend_from_slice(&[0, 1, 0, answer as u8, 0, 0, 0, 0]);
        p.extend_from_slice(&packet[12..pos + 4]);
        if answer {
            p.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
            p.extend_from_slice(&self.address.octets());
        }
        Some((p, name))
    }
}

pub struct Dhcp {
    pub kind: u8,
    pub mac: String,
    pub hostname: String,
    xid: [u8; 4],
    flags: [u8; 2],
    ciaddr: Ipv4Addr,
    chaddr: [u8; 16],
    requested: Option<Ipv4Addr>,
    server: Option<Ipv4Addr>,
}
impl Dhcp {
    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < 240 || p[..3] != [1, 1, 6] || p[24..28] != [0; 4] || p[236..240] != MAGIC {
            return None;
        }
        let mut opts = BTreeMap::new();
        let mut pos = 240;
        loop {
            let code = *p.get(pos)?;
            pos += 1;
            if code == 255 {
                break;
            }
            if code == 0 {
                continue;
            }
            let size = *p.get(pos)? as usize;
            pos += 1;
            opts.insert(code, p.get(pos..pos + size)?);
            pos += size;
        }
        let kind = opts.get(&53)?;
        if kind.len() != 1 {
            return None;
        }
        let addr = |code| -> Option<Option<Ipv4Addr>> {
            Some(match opts.get(&code) {
                Some(v) => Some(Ipv4Addr::from(<[u8; 4]>::try_from(*v).ok()?)),
                None => None,
            })
        };
        Some(Self {
            kind: kind[0],
            mac: p[28..34]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(":"),
            hostname: String::from_utf8_lossy(opts.get(&12).copied().unwrap_or_default())
                .into_owned(),
            xid: p[4..8].try_into().ok()?,
            flags: p[10..12].try_into().ok()?,
            ciaddr: Ipv4Addr::from(<[u8; 4]>::try_from(&p[12..16]).ok()?),
            chaddr: p[28..44].try_into().ok()?,
            requested: addr(50)?,
            server: addr(54)?,
        })
    }
}

pub fn choose(cfg: &Config, leases: &Leases, mac: &str, now: f64) -> Option<Ipv4Addr> {
    if let Some(ip) = cfg.reservations.get(mac) {
        return Some(*ip);
    }
    let taken: BTreeSet<_> = leases
        .iter()
        .filter(|(m, v)| *m != mac && v["expires"].as_f64().unwrap_or(0.0) > now)
        .filter_map(|(_, v)| v["ip"].as_str()?.parse::<Ipv4Addr>().ok())
        .chain(
            cfg.reservations
                .iter()
                .filter(|(m, _)| *m != mac)
                .map(|(_, ip)| *ip),
        )
        .collect();
    if let Some(current) = leases
        .get(mac)
        .and_then(|v| v["ip"].as_str())
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
    {
        if cfg.pool.contains(&current) && !taken.contains(&current) {
            return Some(current);
        }
    }
    cfg.pool.iter().find(|ip| !taken.contains(ip)).copied()
}

pub fn handle_dhcp(
    cfg: &Config,
    leases: &mut Leases,
    req: &Dhcp,
    now: f64,
    stamp: &str,
) -> Option<(u8, Option<Ipv4Addr>)> {
    match req.kind {
        1 => choose(cfg, leases, &req.mac, now).map(|ip| (2, Some(ip))),
        3 => {
            if req.server.is_some_and(|s| s != cfg.address) {
                return None;
            }
            let wanted = req.requested.unwrap_or(req.ciaddr);
            match choose(cfg, leases, &req.mac, now) {
                Some(ip) if ip == wanted => {
                    leases.insert(req.mac.clone(), json!({"ip": ip.to_string(), "hostname": req.hostname, "expires": now + cfg.lease_s as f64, "acked": stamp}));
                    Some((5, Some(ip)))
                }
                _ => Some((6, None)),
            }
        }
        7 => {
            if let Some(v) = leases.get_mut(&req.mac) {
                v["expires"] = 0.into();
            }
            None
        }
        8 => Some((5, None)),
        _ => None,
    }
}

pub fn http_reply(data: &[u8]) -> &'static [u8] {
    let parsed = || -> Option<()> {
        let text = std::str::from_utf8(data).ok()?;
        if !text.ends_with("\r\n\r\n") {
            return None;
        }
        let mut lines = text.split("\r\n");
        let mut first = lines.next()?.split_whitespace();
        if first.next()? != "GET" || first.next()? != "/" {
            return None;
        }
        let mut hosts = lines
            .filter_map(|l| l.split_once(':'))
            .filter(|(k, _)| k.eq_ignore_ascii_case("host"));
        let host = hosts
            .next()?
            .1
            .trim()
            .split(':')
            .next()?
            .to_ascii_lowercase();
        if hosts.next().is_some() || !CTEST_HOSTS.contains(&host.as_str()) {
            return None;
        }
        Some(())
    };
    if parsed().is_some() {
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nX-Organization: Nintendo\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nok"
    } else {
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    }
}

#[cfg(test)]
mod tests;
