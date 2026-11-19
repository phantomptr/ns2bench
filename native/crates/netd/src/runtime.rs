//! Socket ownership, persistence and fail-closed isolation checks. Never changes network settings.
use crate::{handle_dhcp, http_reply, Config, Dhcp, Leases};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn stamp() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm = std::mem::zeroed();
        let mut buf = [0u8; 64];
        if libc::localtime_r(&t, &mut tm).is_null() {
            return t.to_string();
        }
        let n = libc::strftime(
            buf.as_mut_ptr().cast(),
            buf.len(),
            c"%Y-%m-%dT%H:%M:%S".as_ptr(),
            &tm,
        );
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}
struct Log(PathBuf);
impl Log {
    fn event(&self, event: &str, mut fields: Value) {
        fields["t"] = stamp().into();
        fields["event"] = event.into();
        let line = format!("{fields}\n");
        print!("{line}");
        if fs::metadata(&self.0).is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
            let _ = fs::rename(&self.0, self.0.with_extension("1"));
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.0)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Invalid controls disable every local service. Isolation is never a control.
/// Shared network: DHCP is off whatever the file says.
pub fn service_controls(value: &Value) -> [bool; 3] {
    let on = |key: &str| value[key].as_bool().unwrap_or(false);
    match value["topology"].as_str() {
        Some("direct") => [on("dhcp"), on("dns"), on("connectivity")],
        Some("shared") => [false, on("dns"), on("connectivity")],
        _ => [false; 3],
    }
}
pub fn is_shared(value: &Value) -> bool {
    value["topology"] == "shared"
}
/// (dhcp, dns, connectivity) and whether this is a shared network.
fn read_controls(state: &Path) -> ([bool; 3], bool) {
    match fs::read(state.join("network-controls.json")) {
        Ok(bytes) => {
            let v = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
            (service_controls(&v), is_shared(&v))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => ([true; 3], false),
        Err(_) => ([false; 3], false),
    }
}

/// What each listed console did last (shared network): the engine's leak watch reads it.
#[derive(Default, Clone)]
pub struct Seen {
    last_dns: Option<f64>,
    last_ctest: Option<f64>,
    lookups: u64,
    refused: u64,
}
pub type SeenMap = std::sync::Arc<std::sync::Mutex<BTreeMap<Ipv4Addr, Seen>>>;

fn status_value(cfg: &Config, controls: [bool; 3], shared: bool, seen: &SeenMap) -> Value {
    let consoles: serde_json::Map<String, Value> = cfg.consoles.iter().map(|ip| {
        let s = seen.lock().map(|m| m.get(ip).cloned().unwrap_or_default()).unwrap_or_default();
        (ip.to_string(), json!({"last_dns": s.last_dns, "last_ctest": s.last_ctest, "lookups": s.lookups, "refused": s.refused}))
    }).collect();
    json!({"time": now(), "interface": cfg.interface, "address":cfg.address.to_string(),
        "forwarding_off":forwarding_off(), "interface_ready":interface_ready(cfg),
        "topology": if shared { "shared" } else { "direct" }, "consoles": consoles,
        "dhcp":controls[0], "dns":controls[1], "connectivity":controls[2], "blocking":true})
}
fn publish_status(state: &Path, cfg: &Config, controls: [bool; 3], shared: bool, seen: &SeenMap) {
    let value = status_value(cfg, controls, shared, seen);
    let tmp = state.join("network-status.tmp");
    if let Ok(mut file) = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp) {
        if file.write_all(value.to_string().as_bytes()).is_ok() { let _ = fs::rename(tmp, state.join("network-status.json")); }
    }
}

pub fn load_leases(path: &Path) -> Result<Leases, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let leases: Leases =
                serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            if leases.values().any(|v| {
                !v.is_object()
                    || v["ip"]
                        .as_str()
                        .and_then(|s| s.parse::<Ipv4Addr>().ok())
                        .is_none()
                    || v["expires"].as_f64().is_none()
            }) {
                return Err("invalid lease file; refusing to discard existing assignments".into());
            }
            Ok(leases)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Leases::new()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn save_leases(path: &Path, leases: &Leases) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(leases)?)?;
    f.sync_all()?;
    fs::rename(tmp, path)
}

pub fn forwarding_values(v4: Option<&str>, v6: Option<&str>) -> bool {
    matches!((v4, v6), (Some(v4), Some(v6)) if v4.trim() == "0" && v6.trim() == "0")
}
pub fn forwarding_off() -> bool {
    #[cfg(target_os = "linux")]
    {
        let v4 = fs::read_to_string("/proc/sys/net/ipv4/ip_forward").ok();
        // IPv6 turned off in the kernel (ipv6.disable=1): no /proc/sys/net/ipv6 at all, and
        // nothing can be forwarded over it. Any other missing value still counts as "not off".
        let v6 = if std::path::Path::new("/proc/sys/net/ipv6").exists() {
            fs::read_to_string("/proc/sys/net/ipv6/conf/all/forwarding").ok()
        } else {
            Some("0".to_string())
        };
        // The kernel decides per incoming interface, so one interface with forwarding on is
        // enough to route for the console even while the global switches read 0.
        let per_interface_off = |family: &str| {
            fs::read_dir(format!("/proc/sys/net/{family}/conf")).is_ok_and(|dir| {
                dir.flatten().all(|e| {
                    let f = e.path().join("forwarding");
                    // (an entry with no such file isn't an interface; one that can't be read is not "off")
                    !f.exists() || fs::read_to_string(f).is_ok_and(|v| v.trim() == "0")
                })
            })
        };
        let no_ipv6 = !std::path::Path::new("/proc/sys/net/ipv6").exists();
        forwarding_values(v4.as_deref(), v6.as_deref()) && per_interface_off("ipv4") && (no_ipv6 || per_interface_off("ipv6"))
    }
    #[cfg(target_os = "macos")]
    {
        [c"net.inet.ip.forwarding", c"net.inet6.ip6.forwarding"]
            .iter()
            .all(|name| unsafe {
                let mut value: libc::c_int = -1;
                let mut size = std::mem::size_of_val(&value);
                libc::sysctlbyname(
                    name.as_ptr(),
                    (&mut value as *mut libc::c_int).cast(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                ) == 0
                    && size == std::mem::size_of_val(&value)
                    && value == 0
            })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    false
}

/// One row of getifaddrs: adapter name, flags, IPv4 address and netmask (if it has them).
struct IfRow {
    name: Vec<u8>,
    flags: u32,
    addr: Option<Ipv4Addr>,
    mask: Option<Ipv4Addr>,
}

/// An IPv4 address out of a sockaddr, reading no more than the record holds. On macOS a record
/// says its own length (a netmask's can be shorter than a full sockaddr_in): missing bytes are 0.
/// `need_inet`: only an AF_INET record counts (a netmask may carry no family).
///
/// SAFETY: `sa` is null or points to a sockaddr from getifaddrs, alive until freeifaddrs.
unsafe fn sockaddr_v4(sa: *const libc::sockaddr, need_inet: bool) -> Option<Ipv4Addr> {
    if sa.is_null() {
        return None;
    }
    let family = unsafe { std::ptr::addr_of!((*sa).sa_family).read_unaligned() } as i32;
    if need_inet && family != libc::AF_INET {
        return None;
    }
    let mut buf = [0u8; std::mem::size_of::<libc::sockaddr_in>()];
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    let len = (unsafe { std::ptr::addr_of!((*sa).sa_len).read_unaligned() } as usize).min(buf.len());
    // Linux: an AF_INET address or netmask is a whole sockaddr_in, and other families are longer
    #[cfg(not(any(target_os = "macos", target_os = "freebsd")))]
    let len = buf.len();
    unsafe { std::ptr::copy_nonoverlapping(sa.cast::<u8>(), buf.as_mut_ptr(), len) };
    // sin_addr sits after the 2-byte family (or len + family) and the 2-byte port, in network order
    Some(Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]))
}

/// Every interface row, copied out before the list is freed.
fn if_rows() -> Vec<IfRow> {
    let mut out = vec![];
    let mut list = std::mem::MaybeUninit::<*mut libc::ifaddrs>::uninit();
    // SAFETY: getifaddrs initialises `list` when it returns 0 (and only then is it read). The list
    // is walked through NonNull (a null node is never read), each node field by field through raw
    // pointers, and everything is copied out before freeifaddrs.
    unsafe {
        if libc::getifaddrs(list.as_mut_ptr()) != 0 {
            return out;
        }
        let head = list.assume_init();
        let mut node = std::ptr::NonNull::new(head);
        while let Some(n) = node {
            let p = n.as_ptr();
            let name_ptr = std::ptr::addr_of!((*p).ifa_name).read();
            let addr = std::ptr::addr_of!((*p).ifa_addr).read();
            let mask = std::ptr::addr_of!((*p).ifa_netmask).read();
            let flags: u32 = std::ptr::addr_of!((*p).ifa_flags).read();
            if let Some(name) = std::ptr::NonNull::new(name_ptr) {
                out.push(IfRow {
                    name: CStr::from_ptr(name.as_ptr()).to_bytes().to_vec(),
                    flags,
                    addr: sockaddr_v4(addr, true),
                    mask: sockaddr_v4(mask, false),
                });
            }
            node = std::ptr::NonNull::new(std::ptr::addr_of!((*p).ifa_next).read());
        }
        if !head.is_null() {
            libc::freeifaddrs(head);
        }
    }
    out
}

/// Every up IPv4 address on this computer except loopback: (adapter, address, prefix length).
/// The shared-network setup offers these so the person picks the adapter.
pub fn ipv4_interfaces() -> Vec<(String, Ipv4Addr, u32)> {
    if_rows().into_iter()
        .filter(|r| r.flags & (libc::IFF_UP as u32) != 0 && r.flags & (libc::IFF_LOOPBACK as u32) == 0)
        .filter_map(|r| {
            let (ip, mask) = (r.addr?, r.mask?);
            Some((String::from_utf8_lossy(&r.name).into_owned(), ip, u32::from(mask).count_ones()))
        })
        .collect()
}

pub fn interface_ready(cfg: &Config) -> bool {
    if_rows().iter().any(|r| r.flags & (libc::IFF_UP as u32) != 0 && r.name == cfg.interface.as_bytes() && r.addr == Some(cfg.address))
}

pub(crate) fn interface_index(interface: &str) -> io::Result<u32> {
    let name = CString::new(interface)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid interface"))?;
    let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
    if index == 0 {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "interface not found",
        ))
    } else {
        Ok(index)
    }
}
fn sockopt(fd: RawFd, level: i32, opt: i32, bytes: &[u8]) -> io::Result<()> {
    if unsafe {
        libc::setsockopt(
            fd,
            level,
            opt,
            bytes.as_ptr().cast(),
            bytes.len() as libc::socklen_t,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn bound_socket(
    kind: i32,
    interface: &str,
    address: Ipv4Addr,
    port: u16,
) -> io::Result<OwnedFd> {
    let index = interface_index(interface)?;
    let raw = unsafe { libc::socket(libc::AF_INET, kind, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    sockopt(
        raw,
        libc::SOL_SOCKET,
        libc::SO_REUSEADDR,
        &1i32.to_ne_bytes(),
    )?;
    #[cfg(target_os = "macos")]
    sockopt(
        raw,
        libc::IPPROTO_IP,
        25, /* IP_BOUND_IF */
        &index.to_ne_bytes(),
    )?;
    // macOS still hands a socket bound to INADDR_ANY the broadcasts of every interface (seen:
    // the home router's DHCP requests reached the console service). Ask which interface each
    // datagram came in on, so recv_on() can drop the others.
    #[cfg(target_os = "macos")]
    if kind == libc::SOCK_DGRAM {
        sockopt(raw, libc::IPPROTO_IP, 20 /* IP_RECVIF */, &1i32.to_ne_bytes())?;
    }
    #[cfg(target_os = "linux")]
    {
        let _ = index;
        sockopt(
            raw,
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            CString::new(interface)?.as_bytes_with_nul(),
        )?;
    }
    if !socket_is_bound(raw, interface) {
        return Err(io::Error::other(
            "socket interface binding could not be verified",
        ));
    }
    let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    #[cfg(target_os = "macos")]
    {
        addr.sin_len = std::mem::size_of_val(&addr) as u8;
    }
    addr.sin_family = libc::AF_INET as _;
    addr.sin_port = port.to_be();
    addr.sin_addr.s_addr = u32::from_ne_bytes(address.octets());
    if unsafe {
        libc::bind(
            raw,
            (&addr as *const libc::sockaddr_in).cast(),
            std::mem::size_of_val(&addr) as _,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}
pub fn socket_is_bound(fd: RawFd, interface: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let mut index = 0u32;
        let mut len = std::mem::size_of_val(&index) as libc::socklen_t;
        (unsafe {
            libc::getsockopt(
                fd,
                libc::IPPROTO_IP,
                25,
                (&mut index as *mut u32).cast(),
                &mut len,
            ) == 0
        }) && interface_index(interface).is_ok_and(|expected| index == expected)
    }
    #[cfg(target_os = "linux")]
    {
        let mut name = [0u8; 16];
        let mut len = name.len() as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_BINDTODEVICE,
                name.as_mut_ptr().cast(),
                &mut len,
            )
        };
        result == 0 && name.split(|b| *b == 0).next() == Some(interface.as_bytes())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (fd, interface);
        false
    }
}

/// Serves one connectivity-test request. Runs on its own thread so a client that
/// connects and stays silent cannot stall DHCP/DNS; the isolation guard has been
/// checked by the poll loop just before the connection was accepted.
fn http(mut stream: TcpStream, log: &Log, peer: Ipv4Addr, seen: &SeenMap) -> io::Result<()> {
    // accept() inherits O_NONBLOCK on BSD/macOS. Use bounded blocking reads here.
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut request = Vec::new();
    while request.len() < 8192 && !request.windows(4).any(|s| s == b"\r\n\r\n") {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        stream.set_read_timeout(Some(remaining))?;
        let mut data = [0u8; 2048];
        let n = stream.read(&mut data)?;
        if n == 0 {
            break;
        }
        request.extend_from_slice(&data[..n]);
    }
    if !forwarding_off() {
        return Ok(());
    }
    let response = http_reply(&request);
    let ok = response.starts_with(b"HTTP/1.1 200");
    log.event("ctest", json!({"ok": ok, "peer": peer.to_string()}));
    if ok {
        if let Ok(mut m) = seen.lock() { m.entry(peer).or_default().last_ctest = Some(now()); }
    }
    stream.write_all(response)
}

/// Receive one datagram that came in on `index` (the console interface). `Ok(None)`: it came in
/// on another interface (macOS only; Linux's SO_BINDTODEVICE never delivers those).
#[cfg(target_os = "macos")]
pub fn recv_on(sock: &UdpSocket, buf: &mut [u8], index: u32) -> io::Result<Option<(usize, SocketAddr)>> {
    let mut name: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    let mut iov = libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() };
    let mut control = [0u64; 32];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_name = (&mut name as *mut libc::sockaddr_in).cast();
    msg.msg_namelen = std::mem::size_of_val(&name) as _;
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control) as _;
    let n = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut msg, 0) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut from = None;
    // SAFETY: walks the control messages the kernel just wrote into `control`.
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(&msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::IPPROTO_IP && (*c).cmsg_type == 20 {
                let dl = libc::CMSG_DATA(c) as *const libc::sockaddr_dl;
                from = Some(std::ptr::read_unaligned(dl).sdl_index as u32);
            }
            c = libc::CMSG_NXTHDR(&msg, c);
        }
    }
    // Unknown interface: dropped (fail closed).
    if from != Some(index) {
        return Ok(None);
    }
    let peer = SocketAddr::V4(std::net::SocketAddrV4::new(Ipv4Addr::from(u32::from_be(name.sin_addr.s_addr)), u16::from_be(name.sin_port)));
    Ok(Some((n as usize, peer)))
}
#[cfg(not(target_os = "macos"))]
pub fn recv_on(sock: &UdpSocket, buf: &mut [u8], _index: u32) -> io::Result<Option<(usize, SocketAddr)>> {
    sock.recv_from(buf).map(Some)
}

// macOS permits unprivileged low-port binds only on INADDR_ANY. IP_BOUND_IF
// still restricts ingress/egress to the one verified console interface.
pub fn service_address(address: Ipv4Addr) -> Ipv4Addr {
    if cfg!(target_os = "macos") {
        Ipv4Addr::UNSPECIFIED
    } else {
        address
    }
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}
pub fn main(args: Vec<String>) -> Result<(), String> {
    if args.iter().any(|s| s == "--version") {
        println!("ns2bench-netd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let check = args.iter().any(|s| s == "--check");
    let paths: Vec<_> = args.iter().filter(|s| s.as_str() != "--check").collect();
    if paths.len() > 1 || paths.first().is_some_and(|p| p.starts_with('-')) {
        return Err("usage: ns2bench-netd [--check | --version] [config.json]".into());
    }
    let path = paths
        .first()
        .map(|p| expand(p))
        .unwrap_or_else(|| expand("~/.ns2bench/config.json"));
    let data: Value =
        serde_json::from_slice(&fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| e.to_string())?;
    let cfg = Config::from_value(&data)?;
    let control = crate::control::from_config(&data, &cfg, expand)?;
    if !forwarding_off() {
        return Err(
            "refusing service: IPv4/IPv6 forwarding is enabled or cannot be verified".into(),
        );
    }
    if check {
        if !interface_ready(&cfg) {
            return Err("configured interface/address is not ready".into());
        }
        println!("NS2Bench netd: config valid, interface/address ready, IPv4/IPv6 forwarding off");
        return Ok(());
    }
    let state = expand(data["state_dir"].as_str().unwrap_or("~/.ns2bench"));
    fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let log = Log(state.join("netd.log"));
    let lease_path = state.join("leases.json");
    // An unreadable lease file must not keep the helper restarting forever with nothing to tell
    // the user why: set it aside (nothing is discarded) and start with an empty table.
    let mut leases = match load_leases(&lease_path) {
        Ok(l) => l,
        // (a file that can't be read at all right now, e.g. a disk error, is not thrown over)
        Err(e) if fs::read(&lease_path).is_err() => return Err(e),
        Err(e) => {
            let aside = lease_path.with_extension("json.unreadable");
            let kept = fs::rename(&lease_path, &aside).is_ok();
            log.event("leases_unreadable", json!({"error": e, "kept_as": if kept { aside.display().to_string() } else { String::new() }}));
            if !kept { return Err(e); }
            Leases::new()
        }
    };
    let mut wait_log = Instant::now() - Duration::from_secs(10);
    while !interface_ready(&cfg) {
        publish_status(&state, &cfg, [false; 3], false, &SeenMap::default());
        if !forwarding_off() {
            return Err("forwarding enabled while waiting for interface".into());
        }
        if wait_log.elapsed() >= Duration::from_secs(10) {
            log.event(
                "waiting",
                json!({"interface": cfg.interface, "address": cfg.address.to_string()}),
            );
            wait_log = Instant::now();
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let bind = |kind, address, port| {
        bound_socket(kind, &cfg.interface, address, port)
            .map_err(|e| format!("{} port {port}: {e}", cfg.interface))
    };
    let ifindex = interface_index(&cfg.interface).map_err(|e| e.to_string())?;
    let dhcp = UdpSocket::from(bind(libc::SOCK_DGRAM, Ipv4Addr::UNSPECIFIED, 67)?);
    dhcp.set_broadcast(true).map_err(|e| e.to_string())?;
    let dns = UdpSocket::from(bind(libc::SOCK_DGRAM, service_address(cfg.address), 53)?);
    let fd = bind(libc::SOCK_STREAM, service_address(cfg.address), 80)?;
    if unsafe { libc::listen(fd.as_raw_fd(), 8) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let listener = TcpListener::from(fd);
    let control_listener = control.as_ref().map(|c| c.bind()).transpose()?;
    // Nonblocking descriptors prevent stale readiness from delaying the isolation guard.
    dhcp.set_nonblocking(true).map_err(|e| e.to_string())?;
    dns.set_nonblocking(true).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    log.event("ready", json!({"engine":"rust", "control": control.as_ref().map(|c| c.listen.to_string()), "interface": cfg.interface, "address": cfg.address.to_string(), "pool":cfg.pool_text(), "consoles": cfg.consoles.iter().map(|c| c.to_string()).collect::<Vec<_>>()}));
    let console_seen: SeenMap = Default::default();
    let mut status_at = Instant::now() - Duration::from_secs(2);
    let mut seen: BTreeMap<String, Instant> = BTreeMap::new();
    loop {
        let (mut controls, file_shared) = read_controls(&state);
        // Shared comes from the config too: a missing, stale or edited controls file must never
        // turn DHCP on (or open DNS to everyone) on a home network.
        let shared = file_shared || cfg.is_shared();
        if shared {
            controls[0] = false;
        }
        if status_at.elapsed() >= Duration::from_secs(1) { publish_status(&state, &cfg, controls, shared, &console_seen); status_at = Instant::now(); }
        if !forwarding_off() {
            log.event(
                "refuse",
                json!({"reason":"forwarding enabled or unverifiable"}),
            );
            return Err("forwarding enabled or unverifiable".into());
        }
        if !interface_ready(&cfg) {
            log.event("interface-lost", json!({"interface":cfg.interface}));
            return Err("interface address lost".into());
        }
        // The management listener (if any) is polled last; -1 makes poll() skip it.
        let control_fd = control_listener.as_ref().map_or(-1, |l| l.as_raw_fd());
        let mut polls = [dhcp.as_raw_fd(), dns.as_raw_fd(), listener.as_raw_fd(), control_fd]
            .map(|fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
        let rc = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as _, 1000) };
        if rc < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e.to_string());
        }
        for (i, poll) in polls.iter().enumerate().filter(|(_, p)| p.revents != 0) {
            if !forwarding_off() || !interface_ready(&cfg) {
                return Err("isolation/interface guard failed".into());
            }
            if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err("network socket failed".into());
            }
            let result = (|| -> io::Result<()> {
                let mut packet = [0u8; 4096];
                match i {
                    0 => {
                        let Some((n, _)) = recv_on(&dhcp, &mut packet, ifindex)? else { return Ok(()) };
                        if !controls[0] { return Ok(()); }
                        if let Some(req) = Dhcp::parse(&packet[..n]) {
                            log.event(
                                "dhcp-rx",
                                json!({"mac": req.mac, "type": req.kind, "hostname":req.hostname}),
                            );
                            let response = handle_dhcp(&cfg, &mut leases, &req, now(), &stamp());
                            if req.kind == 3 || req.kind == 7 {
                                save_leases(&lease_path, &leases)?;
                            }
                            if let Some((kind, ip)) = response {
                                dhcp.send_to(
                                    &cfg.dhcp_reply(&req, kind, ip),
                                    (Ipv4Addr::BROADCAST, 68),
                                )?;
                                log.event(
                                    match kind {
                                        5 => "dhcp-ack",
                                        6 => "dhcp-nak",
                                        _ => "dhcp-offer",
                                    },
                                    json!({"mac":req.mac,"ip":ip.map(|p|p.to_string())}),
                                );
                            }
                        }
                    }
                    1 => {
                        let Some((n, peer)) = recv_on(&dns, &mut packet, ifindex)? else { return Ok(()) };
                        if !controls[1] { return Ok(()); }
                        if let SocketAddr::V4(peer) = peer {
                            if cfg.serves(*peer.ip(), shared) {
                                if let Some((response, name)) = cfg.dns_reply(&packet[..n]) {
                                    let local = response[3] & 15 == 0;
                                    if shared {
                                        if let Ok(mut m) = console_seen.lock() {
                                            let s = m.entry(*peer.ip()).or_default();
                                            s.last_dns = Some(now());
                                            s.lookups += 1;
                                            if !local { s.refused += 1; }
                                        }
                                    }
                                    if seen
                                        .get(&name)
                                        .is_none_or(|at| at.elapsed() > Duration::from_secs(60))
                                    {
                                        if seen.len() >= 4096 {
                                            seen.clear();
                                        }
                                        log.event("dns", json!({"name":name,"peer":peer.ip().to_string(),"local":response[3]&15==0}));
                                        seen.insert(name, Instant::now());
                                    }
                                    dns.send_to(&response, peer)?;
                                }
                            }
                        }
                    }
                    3 => {
                        let (Some(l), Some(c)) = (&control_listener, &control) else { return Ok(()) };
                        let (stream, peer) = l.accept()?;
                        // Hosts accept packets for any local address on any interface:
                        // never serve the management port to the console network.
                        if matches!(peer, SocketAddr::V4(p) if cfg.contains(*p.ip())) {
                            return Ok(());
                        }
                        // Own thread: a slow management client must never stall DHCP/DNS.
                        static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                        use std::sync::atomic::Ordering::SeqCst;
                        if ACTIVE.fetch_add(1, SeqCst) >= 4 {
                            ACTIVE.fetch_sub(1, SeqCst);
                            return Ok(()); // dropped: too many management connections
                        }
                        let (c, state, status, log) = (c.clone(), state.clone(),
                            status_value(&cfg, controls, shared, &console_seen), Log(log.0.clone()));
                        std::thread::spawn(move || {
                            match crate::control::serve(stream, peer, &c, &state, status, now()) {
                                Ok((peer, code)) => log.event("control", json!({"peer": peer, "status": code})),
                                Err(e) => log.event("control-error", json!({"detail": e.to_string()})),
                            }
                            ACTIVE.fetch_sub(1, SeqCst);
                        });
                    }
                    _ => {
                        let (stream, peer) = listener.accept()?;
                        if !controls[2] { return Ok(()); }
                        if let SocketAddr::V4(peer) = peer {
                            if !cfg.serves(*peer.ip(), shared) { return Ok(()); }
                            let (ip, seen) = (*peer.ip(), console_seen.clone());
                            static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                            use std::sync::atomic::Ordering::SeqCst;
                            if ACTIVE.fetch_add(1, SeqCst) >= 8 {
                                ACTIVE.fetch_sub(1, SeqCst);
                                return Ok(());
                            }
                            let log = Log(log.0.clone());
                            std::thread::spawn(move || {
                                if let Err(e) = http(stream, &log, ip, &seen) {
                                    log.event("error", json!({"detail": e.to_string()}));
                                }
                                ACTIVE.fetch_sub(1, SeqCst);
                            });
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                if e.kind() != io::ErrorKind::WouldBlock {
                    log.event("error", json!({"detail":e.to_string()}));
                }
            }
        }
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;
    #[test]
    fn malformed_controls_fail_closed_and_router_disables_dhcp() {
        assert_eq!(service_controls(&Value::Null), [false;3]);
        assert_eq!(service_controls(&json!({"topology":"router","dhcp":true,"dns":true,"connectivity":true})), [false;3]);
        assert_eq!(service_controls(&json!({"topology":"direct","dhcp":true,"dns":false,"connectivity":true})), [true,false,true]);
    }
}
