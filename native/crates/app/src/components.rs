//! Settings → Install: the parts of NS2Bench that live outside the app window.
//!   pico   flash the Bridge firmware onto a Raspberry Pi Pico in BOOTSEL mode
//!   board  install or update the Bridge daemon on a Linux USB-gadget board over SSH
//!   netd   install the offline console network helper as a service (macOS, Linux)
//! (The command line tool is in cli_install.rs.)
//!
//! Firmware and binaries come from this app version's GitHub release and are checked
//! against the release's SHA256SUMS before use. Board scripts are built into the app,
//! so the board always gets the scripts that match the app.

use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE: &str = concat!("https://github.com/phantomptr/ns2bench/releases/download/v", env!("CARGO_PKG_VERSION"), "/");
const MAX_ASSET: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------- release downloads

fn sums_entry(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

/// Download one asset of this version's release into ~/.ns2bench/downloads/<version>/,
/// verified against the release's SHA256SUMS. A verified copy is reused.
async fn fetch(name: &str) -> Result<PathBuf> {
    ensure!(!name.contains(['/', '\\']) && !name.starts_with('.'), "bad asset name");
    let client = crate::updates::client()?;
    let sums = client.get(format!("{RELEASE}SHA256SUMS")).send().await?;
    if sums.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("NS2Bench {VERSION} has no published release to download from. Use a released version of the app.");
    }
    let sums = sums.error_for_status()?.text().await?;
    // The list itself must carry the release key's signature: a checksum next to a file only
    // proves it wasn't damaged, not who put it there. These files end up running as root.
    let sig = client.get(format!("{RELEASE}SHA256SUMS.sig")).send().await?.error_for_status()
        .map_err(|_| anyhow::anyhow!("The {VERSION} release's file list isn't signed; nothing was installed"))?.text().await?;
    ensure!(crate::updates::signature_ok(crate::updates::UPDATE_KEY, &sums, sig.trim()),
        "The {VERSION} release's file list doesn't match NS2Bench's signature; nothing was installed");
    let expected = sums_entry(&sums, name).with_context(|| format!("The {VERSION} release has no {name}"))?;
    let dir = ns2bench_core::config::state_dir().join("downloads").join(VERSION);
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(name);
    if let Ok(bytes) = tokio::fs::read(&path).await {
        if crate::updates::hex(&Sha256::digest(&bytes)) == expected {
            return Ok(path);
        }
    }
    let r = client.get(format!("{RELEASE}{name}")).send().await?.error_for_status()?;
    ensure!(r.content_length().is_none_or(|n| n as usize <= MAX_ASSET), "{name} is too large");
    let bytes = r.bytes().await?;
    ensure!(bytes.len() <= MAX_ASSET, "{name} is too large");
    ensure!(crate::updates::hex(&Sha256::digest(&bytes)) == expected, "{name} failed its SHA-256 check; nothing was installed");
    let tmp = path.with_extension("partial");
    tokio::fs::write(&tmp, &bytes).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(path)
}

fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" }
}

// ---------------------------------------------------------------- Pico

/// A Pico in BOOTSEL mode shows up as a small drive named RPI-RP2 with INFO_UF2.TXT.
fn pico_drive() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut scan = |dir: PathBuf| {
        if let Ok(rd) = std::fs::read_dir(dir) {
            roots.extend(rd.flatten().map(|e| e.path()));
        }
    };
    if cfg!(target_os = "macos") {
        scan("/Volumes".into());
    } else if cfg!(windows) {
        roots.extend((b'D'..=b'Z').map(|c| PathBuf::from(format!("{}:\\", c as char))));
    } else {
        let user = std::env::var("USER").unwrap_or_default();
        for d in [format!("/media/{user}"), format!("/run/media/{user}"), "/media".into(), "/mnt".into()] {
            scan(d.into());
        }
    }
    roots.into_iter().find(|r| {
        std::fs::read_to_string(r.join("INFO_UF2.TXT")).is_ok_and(|t| t.contains("Board-ID: RPI-RP2"))
    })
}

async fn pico_flash() -> Result<Value> {
    let uf2 = fetch(&format!("ns2bench-pico-{VERSION}.uf2")).await?;
    // Give the person time to hold BOOTSEL and plug the Pico in.
    let t0 = Instant::now();
    let drive = loop {
        if let Some(d) = tokio::task::spawn_blocking(pico_drive).await? {
            break d;
        }
        ensure!(t0.elapsed() < Duration::from_secs(120), "No Pico found. Hold BOOTSEL while you plug it in, then try again.");
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    let dst = drive.join("NS2BENCH.UF2");
    let size = std::fs::metadata(&uf2)?.len();
    match tokio::fs::copy(&uf2, &dst).await {
        Ok(n) if n == size => {}
        // The Pico restarts the moment the last block lands; the drive vanishing then is normal.
        Err(_) if !drive.exists() => {}
        Ok(_) => bail!("The copy to {} was incomplete. Try again.", drive.display()),
        Err(e) => bail!("Could not copy to {}: {e}", drive.display()),
    }
    Ok(json!({ "message": format!("Flashed NS2Bench Bridge {VERSION} via {}. The Pico restarts as the Bridge; plug it into the console.", drive.display()) }))
}

// ---------------------------------------------------------------- Linux board over SSH

const REMOTE_INSTALL: &str = include_str!("../../../../board/remote-install.sh");
/// Board scripts as uploaded (name on the board, contents). Keep in step with board/.
const BOARD_SCRIPTS: &[(&str, &str)] = &[
    ("gadget.sh", include_str!("../../../../board/gadget.sh")),
    ("ns2bench-hidd.sh", include_str!("../../../../board/ns2bench-hidd.sh")),
    ("swap.sh", include_str!("../../../../board/swap.sh")),
    ("restore-default-gadget.sh", include_str!("../../../../board/restore-default-gadget.sh")),
    ("S98ns2bench-swap", include_str!("../../../../board/S98ns2bench-swap")),
    ("quirks/luckfox.sh", include_str!("../../../../board/quirks/luckfox.sh")),
];

fn valid_host(h: &str) -> bool {
    !h.is_empty() && h.len() <= 253 && !h.starts_with('-')
        && h.chars().all(|c| c.is_ascii_alphanumeric() || "@._:-".contains(c))
}

/// Run `cmd` on the board with `ssh -o BatchMode=yes` (keys only: the app never handles passwords).
fn ssh(host: &str, cmd: &str, stdin: Option<&[u8]>) -> Result<String> {
    use std::io::Write;
    let mut ssh = Command::new("ssh");
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut ssh, 0x0800_0000); // CREATE_NO_WINDOW
    let mut child = ssh
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3", "--", host, cmd])
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not run ssh. Install an OpenSSH client.")?;
    if let Some(data) = stdin {
        let mut pipe = child.stdin.take().context("ssh stdin")?;
        // A closed pipe means the far side gave up: its own message (below) says why.
        if let Err(e) = pipe.write_all(data) {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.into());
            }
        }
    }
    let out = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if out.status.code() == Some(255) {
            bail!("Could not sign in to {host} over SSH ({}). Set up key sign-in first (ssh-copy-id {host}), then try again.", err.trim());
        }
        bail!("{}\n{}", text.trim(), err.trim());
    }
    Ok(text)
}

fn board_arch(uname: &str) -> Result<&'static str> {
    match uname.trim() {
        a if a.starts_with("armv7") || a == "armhf" || a == "armv8l" => Ok("linux-armv7"),
        "aarch64" | "arm64" => Ok("linux-arm64"),
        a => bail!("NS2Bench Bridge runs on 32- or 64-bit ARM Linux boards; this board reports {a}"),
    }
}

fn board_probe(host: &str) -> Result<Value> {
    ensure!(valid_host(host), "Enter an SSH host, like root@192.168.79.2 or a name from ~/.ssh/config");
    let out = ssh(host, "uname -m; /opt/ns2bench/ns2bench-board version 2>/dev/null || true", None)?;
    let mut lines = out.lines();
    let uname = lines.next().unwrap_or("").to_string();
    let installed = lines.next().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    Ok(json!({ "arch": uname, "supported": board_arch(&uname).is_ok(), "installed": installed, "version": VERSION }))
}

async fn board_install(host: String) -> Result<Value> {
    ensure!(valid_host(&host), "Enter an SSH host, like root@192.168.79.2 or a name from ~/.ssh/config");
    let h = host.clone();
    let uname = tokio::task::spawn_blocking(move || ssh(&h, "uname -m", None)).await??;
    let target = board_arch(&uname)?;
    let bin = tokio::fs::read(fetch(&format!("ns2bench-board-{VERSION}-{target}")).await?).await?;
    let cfg = crate::load_config();
    let token_file = cfg.input["token_file"].as_str().map(ns2bench_core::config::expand)
        .unwrap_or_else(|| ns2bench_core::config::state_dir().join("hidd.token"));
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let mut log = vec![format!("{host}: {} board", uname.trim())];
        ssh(&host, "mkdir -p /opt/ns2bench/quirks && chmod 700 /opt/ns2bench", None)?;
        ssh(&host, "cat > /opt/ns2bench/ns2bench-board.new", Some(&bin))?;
        for (name, body) in BOARD_SCRIPTS {
            ssh(&host, &format!("cat > /opt/ns2bench/{name}.new"), Some(body.as_bytes()))?;
        }
        log.push(format!("uploaded ns2bench-board {VERSION} and {} scripts", BOARD_SCRIPTS.len()));
        // An existing board token is never replaced (this computer would lose access).
        // Asked so that a dropped connection is an error, never mistaken for "no token".
        let has_token = match ssh(&host, "if [ -s /opt/ns2bench/token ]; then echo yes; else echo no; fi", None)?.trim() {
            "yes" => true,
            "no" => false,
            other => bail!("Could not tell whether the board already has a token ({other})"),
        };
        if !has_token {
            let token = match std::fs::read_to_string(&token_file).ok().filter(|t| !t.trim().is_empty()) {
                Some(t) => t,
                None => {
                    let t = crate::updates::hex(&rand::random::<[u8; 24]>()) + "\n";
                    if let Some(d) = token_file.parent() { std::fs::create_dir_all(d)?; }
                    write_private(&token_file, t.as_bytes())?;
                    log.push(format!("created {}", token_file.display()));
                    t
                }
            };
            ssh(&host, "umask 077; cat > /opt/ns2bench/token", Some(token.as_bytes()))?;
            log.push("uploaded the Bridge token".into());
        }
        let out = ssh(&host, "sh -s", Some(REMOTE_INSTALL.as_bytes()))?;
        log.extend(out.lines().map(String::from));
        log.push("done; previous files are kept on the board as *.bak".into());
        Ok(json!({ "message": format!("Installed NS2Bench Bridge {VERSION} on {host}."), "log": log.join("\n") }))
    }).await?
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    std::io::Write::write_all(&mut o.open(path)?, data)
}

// ---------------------------------------------------------------- network helper (netd)

#[cfg(target_os = "macos")]
const LABEL: &str = "com.phantomptr.ns2bench.netd";

fn netd_os() -> Option<&'static str> {
    if cfg!(target_os = "macos") { Some("macos") } else if cfg!(target_os = "linux") { Some("linux") } else { None }
}

fn netd_path() -> PathBuf {
    if cfg!(target_os = "linux") { PathBuf::from("/opt/ns2bench/ns2bench-netd") } else { ns2bench_core::config::state_dir().join("bin/ns2bench-netd") }
}

fn netd_version(bin: &Path) -> Option<String> {
    let o = Command::new(bin).arg("--version").stdin(Stdio::null()).output().ok()?;
    let v = String::from_utf8_lossy(&o.stdout);
    // 0.0.1/0.0.2 helpers predate --version: they print usage and fail.
    if o.status.success() { v.split_whitespace().nth(1).map(String::from) } else { bin.exists().then(|| "older".into()) }
}

fn output(cmd: &mut Command) -> (bool, String) {
    match cmd.stdin(Stdio::null()).output() {
        Ok(o) => (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string()),
        Err(e) => (false, e.to_string()),
    }
}

#[cfg(target_os = "macos")]
fn uid() -> String {
    output(Command::new("/usr/bin/id").arg("-u")).1
}
#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    ns2bench_core::config::home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

fn netd_status() -> Value {
    let Some(_) = netd_os() else {
        return json!({ "supported": false });
    };
    let bin = netd_path();
    #[cfg(target_os = "macos")]
    let (installed, running, config) = {
        let plist = std::fs::read_to_string(plist_path()).unwrap_or_default();
        let (ok, text) = output(Command::new("/bin/launchctl").args(["print", &format!("gui/{}/{LABEL}", uid())]));
        // the config is the second ProgramArguments string
        let config = plist.split("<string>").filter_map(|s| s.split_once("</string>").map(|(a, _)| a.to_string())).nth(2);
        (!plist.is_empty(), ok && text.contains("state = running"), config)
    };
    #[cfg(not(target_os = "macos"))]
    let (installed, running, config) = {
        let unit = std::fs::read_to_string("/etc/systemd/system/ns2bench-netd.service").unwrap_or_default();
        let config = unit.lines().find_map(|l| l.strip_prefix("ExecStart=")).and_then(|l| l.split_whitespace().nth(1)).map(String::from);
        (!unit.is_empty(), output(Command::new("systemctl").args(["is-active", "--quiet", "ns2bench-netd"])).0, config)
    };
    json!({
        "supported": true, "installed": installed, "running": running, "config": config,
        "binary": bin.display().to_string(), "installed_version": netd_version(&bin), "version": VERSION,
        "app_config": crate::load_config().path.map(|p| p.display().to_string()),
    })
}

#[cfg(target_os = "macos")]
fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

async fn netd_install() -> Result<Value> {
    let os = netd_os().context("The network helper runs on macOS and Linux. On Windows, run it on a Linux board or PC (see the setup guide).")?;
    let cfg = crate::load_config();
    let config = cfg.path.clone().filter(|p| p.is_file())
        .context("Save your network settings first (Settings → Network), then install the helper.")?;
    let config = std::fs::canonicalize(&config)?;
    let dl = fetch(&format!("ns2bench-netd-{VERSION}-{os}-{}", arch())).await?;
    tokio::task::spawn_blocking(move || -> Result<Value> {
        #[cfg(unix)]
        std::fs::set_permissions(&dl, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        // Check the configuration without opening any ports.
        let (ok, check) = output(Command::new(&dl).arg("--check").arg(&config));
        if !ok && !check.contains("not ready") {
            bail!("The network settings aren't valid yet: {check}");
        }
        let note = if ok { String::new() } else { " The console's network adapter isn't ready yet; the helper starts serving once it is.".into() };
        #[cfg(target_os = "macos")]
        {
            let home = ns2bench_core::config::home();
            let state = ns2bench_core::config::state_dir();
            let bin = netd_path();
            std::fs::create_dir_all(bin.parent().context("bin")?)?;
            let tmp = bin.with_extension("new");
            std::fs::copy(&dl, &tmp)?;
            std::fs::rename(&tmp, &bin)?;
            let plist = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardOutPath</key><string>/dev/null</string>
  <key>StandardErrorPath</key><string>{}</string>
</dict>
</plist>
"#, xml(&bin.display().to_string()), xml(&config.display().to_string()),
                xml(&state.join("netd.stderr").display().to_string()));
            let path = plist_path();
            std::fs::create_dir_all(home.join("Library/LaunchAgents"))?;
            std::fs::write(&path, plist)?;
            let domain = format!("gui/{}", uid());
            let _ = output(Command::new("/bin/launchctl").args(["bootout", &format!("{domain}/{LABEL}")]));
            std::thread::sleep(Duration::from_millis(500));
            let (ok, err) = output(Command::new("/bin/launchctl").arg("bootstrap").arg(&domain).arg(&path));
            ensure!(ok, "Could not start the helper: {err}");
        }
        #[cfg(target_os = "linux")]
        {
            let user = output(Command::new("id").arg("-un")).1;
            let home = ns2bench_core::config::home();
            ensure!(!user.is_empty() && !user.contains(char::is_whitespace), "Could not find your user name");
            // Runs as you, with only the two network capabilities it needs.
            let unit = format!("[Unit]\nDescription=NS2Bench offline console network\nAfter=network-online.target\n\n[Service]\nUser={user}\nExecStart=/opt/ns2bench/ns2bench-netd {}\nRestart=always\nRestartSec=10\nCapabilityBoundingSet=CAP_NET_RAW CAP_NET_BIND_SERVICE\nAmbientCapabilities=CAP_NET_RAW CAP_NET_BIND_SERVICE\nNoNewPrivileges=yes\nProtectSystem=strict\nEnvironment=HOME={}\nReadWritePaths={}\n\n[Install]\nWantedBy=multi-user.target\n",
                config.display(), home.display(), ns2bench_core::config::state_dir().display());
            ensure!(!config.display().to_string().contains(char::is_whitespace), "The config path must not contain spaces");
            let unit_file = ns2bench_core::config::state_dir().join("downloads/ns2bench-netd.service");
            std::fs::write(&unit_file, unit)?;
            // The root step copies both files somewhere only root can write and checks them
            // there, so nothing can swap them while the password prompt is open.
            let sha = |p: &Path| -> Result<String> { Ok(crate::updates::hex(&Sha256::digest(std::fs::read(p)?))) };
            let (bin_sha, unit_sha) = (sha(&dl)?, sha(&unit_file)?);
            let script = "set -e; d=$(mktemp -d); trap 'rm -rf \"$d\"' EXIT; cp \"$1\" \"$d/netd\"; cp \"$2\" \"$d/unit\"; \
                echo \"$3  $d/netd\" | sha256sum -c --quiet; echo \"$4  $d/unit\" | sha256sum -c --quiet; \
                install -D -m 755 \"$d/netd\" /opt/ns2bench/ns2bench-netd; install -m 644 \"$d/unit\" /etc/systemd/system/ns2bench-netd.service; \
                systemctl daemon-reload; systemctl enable ns2bench-netd; systemctl restart ns2bench-netd";
            let (ok, err) = output(Command::new("pkexec").args(["/bin/sh", "-c", script, "sh"]).arg(&dl).arg(&unit_file).arg(&bin_sha).arg(&unit_sha));
            ensure!(ok, "Could not install the service (an administrator password is needed): {err}");
        }
        Ok(json!({ "message": format!("Network helper {VERSION} installed and started.{note}"), "status": netd_status() }))
    }).await?
}

/// Start the installed helper again, so it reads new network settings (another adapter, the
/// shared network's consoles). The controls (DNS on/off ...) apply without this.
fn netd_restart() -> Result<Value> {
    ensure!(netd_os().is_some(), "The network helper runs on macOS and Linux.");
    ensure!(netd_status()["installed"] == true, "Install the network helper first (Settings → Network).");
    #[cfg(target_os = "macos")]
    {
        let (ok, err) = output(Command::new("/bin/launchctl").args(["kickstart", "-k", &format!("gui/{}/{LABEL}", uid())]));
        ensure!(ok, "Could not restart the helper: {err}");
    }
    #[cfg(target_os = "linux")]
    {
        let (ok, err) = output(Command::new("pkexec").args(["systemctl", "restart", "ns2bench-netd"]));
        ensure!(ok, "Could not restart the helper (an administrator password is needed): {err}");
    }
    std::thread::sleep(Duration::from_secs(2));
    Ok(json!({ "message": "Network helper restarted with the new settings.", "status": netd_status() }))
}

fn netd_uninstall() -> Result<Value> {
    #[cfg(target_os = "macos")]
    {
        let _ = output(Command::new("/bin/launchctl").args(["bootout", &format!("gui/{}/{LABEL}", uid())]));
        let _ = std::fs::remove_file(plist_path());
        let _ = std::fs::remove_file(netd_path());
    }
    #[cfg(target_os = "linux")]
    {
        let script = "systemctl disable --now ns2bench-netd || true; rm -f /etc/systemd/system/ns2bench-netd.service /opt/ns2bench/ns2bench-netd; systemctl daemon-reload";
        let (ok, err) = output(Command::new("pkexec").args(["/bin/sh", "-c", script]));
        ensure!(ok, "Could not remove the service: {err}");
    }
    ensure!(netd_os().is_some(), "Not supported on this computer");
    Ok(json!({ "message": "Network helper removed.", "status": netd_status() }))
}

// ---------------------------------------------------------------- IPC

#[tauri::command]
pub async fn component_action(component: String, action: String, host: Option<String>) -> Result<Value, String> {
    let r = match (component.as_str(), action.as_str()) {
        ("pico", "status") => Ok(json!({ "drive": pico_drive().map(|d| d.display().to_string()), "version": VERSION })),
        ("pico", "flash") => pico_flash().await,
        ("board", "probe") => tokio::task::spawn_blocking(move || board_probe(host.as_deref().unwrap_or("").trim()))
            .await.map_err(anyhow::Error::from).and_then(|r| r),
        ("board", "install") => board_install(host.unwrap_or_default().trim().to_string()).await,
        ("netd", "status") => tokio::task::spawn_blocking(netd_status).await.map_err(anyhow::Error::from),
        ("netd", "install") => netd_install().await,
        ("netd", "restart") => tokio::task::spawn_blocking(netd_restart).await.map_err(anyhow::Error::from).and_then(|r| r),
        ("netd", "uninstall") => tokio::task::spawn_blocking(netd_uninstall).await.map_err(anyhow::Error::from).and_then(|r| r),
        _ => Err(anyhow::anyhow!("Unknown action")),
    };
    r.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_board_script_is_built_in() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../board");
        let mut want: Vec<String> = ["gadget.sh", "ns2bench-hidd.sh", "swap.sh", "restore-default-gadget.sh", "S98ns2bench-swap"].map(String::from).to_vec();
        for e in std::fs::read_dir(root.join("quirks")).unwrap().flatten() {
            want.push(format!("quirks/{}", e.file_name().to_string_lossy()));
        }
        let have: Vec<String> = BOARD_SCRIPTS.iter().map(|(n, _)| n.to_string()).collect();
        want.sort();
        let mut have_sorted = have.clone();
        have_sorted.sort();
        assert_eq!(want, have_sorted);
    }
    #[test]
    fn hosts_and_sums() {
        assert!(valid_host("root@192.168.79.2") && valid_host("my-board") && valid_host("u@fe80::1"));
        assert!(!valid_host("-oProxyCommand=x") && !valid_host("a b") && !valid_host("a;b") && !valid_host(""));
        let h = "a".repeat(64);
        assert_eq!(sums_entry(&format!("{h}  x.uf2\n{h}  y"), "x.uf2"), Some(h.clone()));
        assert_eq!(sums_entry(&format!("{h}  x.uf2.bak"), "x.uf2"), None);
        assert_eq!(board_arch("armv7l\n").unwrap(), "linux-armv7");
        assert_eq!(board_arch("aarch64").unwrap(), "linux-arm64");
        assert!(board_arch("x86_64").is_err());
    }
}
