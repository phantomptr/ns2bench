//! Release manifest, verified downloads and platform installers.
//! Only the fixed project release endpoint is accepted; never accepts a renderer URL/path.
use anyhow::{bail, ensure, Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Manager, State};
use tokio::io::AsyncWriteExt;
const MANIFEST: &str =
    "https://github.com/phantomptr/ns2bench/releases/latest/download/latest.json";
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
/// Designated requirement every released macOS build meets: the app identifier signed by
/// the NS2Bench certificate (SHA-1 of the certificate in ~/.ns2bench/signing, CI secret).
#[cfg(target_os = "macos")]
const RELEASE_REQUIREMENT: &str = "-R=identifier \"io.github.phantomptr.ns2bench\" and certificate leaf = H\"6b2b62e6d97e5a307b41534ba2c5d1baf5e35455\"";
/// Ed25519 key that signs every release (private half: CI secret NS2BENCH_UPDATE_KEY, backup in
/// ~/.ns2bench/signing/update-ed25519.pem). A manifest is only trusted with its signature: the
/// checksum next to an installer can't prove who put it there.
pub(crate) const UPDATE_KEY: &str = "5lU1976+dXXch6e5zkyqqM+R8fTCQpfcWAsP+Pxst8g=";
#[derive(Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: String,
    assets: HashMap<String, String>,
    sha256: HashMap<String, String>,
    #[serde(default)]
    signatures: HashMap<String, String>,
}
/// What is signed for one platform (scripts/release-manifest.mjs builds the same text).
fn signed_text(version: &str, platform: &str, url: &str, hash: &str) -> String {
    format!("ns2bench update\n{version}\n{platform}\n{url}\n{hash}\n")
}
pub(crate) fn signature_ok(public_b64: &str, text: &str, sig_b64: &str) -> bool {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let (Ok(pk), Ok(sig)) = (b64.decode(public_b64), b64.decode(sig_b64)) else { return false };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, pk).verify(text.as_bytes(), &sig).is_ok()
}
#[derive(Default, Clone, Serialize)]
pub struct Update {
    current_version: String,
    latest_version: Option<String>,
    available: bool,
    notes: String,
    message: String,
    downloaded: bool,
    /// The app can't install this one itself (a Linux system package): the person downloads it.
    manual: bool,
    #[serde(skip)]
    url: Option<String>,
    #[serde(skip)]
    hash: Option<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}
#[derive(Default)]
pub struct Updates(pub tokio::sync::Mutex<Update>);
fn platform() -> String {
    format!(
        "{}-{}",
        if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "linux"
        },
        std::env::consts::ARCH
    )
}
fn asset_url(s: &str) -> bool {
    reqwest::Url::parse(s).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("github.com")
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && u.path()
                .starts_with("/phantomptr/ns2bench/releases/download/")
            && u.query().is_none()
            && u.fragment().is_none()
    })
}
fn select(m: Manifest, key: &str, current: &str) -> Result<Update> {
    select_with(m, key, current, UPDATE_KEY)
}
fn select_with(m: Manifest, key: &str, current: &str, public_key: &str) -> Result<Update> {
    let latest = semver::Version::parse(m.version.trim_start_matches('v'))?;
    let available = latest > semver::Version::parse(current)? && latest.pre.is_empty();
    let mut u = Update {
        current_version: current.into(),
        latest_version: Some(latest.to_string()),
        available,
        notes: m.notes,
        ..Default::default()
    };
    if available {
        let url = m
            .assets
            .get(key)
            .context("This release has no installer for your platform")?;
        ensure!(asset_url(url), "release asset URL is not trusted");
        // the installer must come from the release it claims to be (no older one under a new number)
        ensure!(url.contains(&format!("/releases/download/v{latest}/")), "release asset is from another version");
        let hash = m.sha256.get(key).context("Release checksum is missing")?;
        ensure!(
            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid SHA-256"
        );
        let sig = m.signatures.get(key).context("This release is not signed")?;
        ensure!(
            signature_ok(public_key, &signed_text(&latest.to_string(), key, url, hash), sig),
            "The release signature does not match: not installing it"
        );
        u.url = Some(url.clone());
        u.hash = Some(hash.to_lowercase());
    }
    u.message = if available {
        "Update available"
    } else {
        "You are up to date"
    }
    .into();
    Ok(u)
}
pub(crate) fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("NS2Bench/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        // Idle timeout, not a total one: a large installer on a slow link must still finish.
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|a| {
            let u = a.url();
            if a.previous().len() < 5
                && u.scheme() == "https"
                && matches!(
                    u.host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()?)
}
async fn check() -> Result<Update> {
    let response = client()?.get(MANIFEST).send().await?;
    if response.status() == 404 {
        return Ok(Update {
            current_version: env!("CARGO_PKG_VERSION").into(),
            message: "No published update manifest yet".into(),
            ..Default::default()
        });
    }
    let mut body = Vec::new();
    let mut chunks = response.error_for_status()?.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        body.extend_from_slice(&chunk?);
        ensure!(body.len() <= 1024 * 1024, "manifest too large");
    }
    select(
        serde_json::from_slice(&body)?,
        &platform(),
        env!("CARGO_PKG_VERSION"),
    )
}
/// Bytes of the installer received so far, and its size (0 when the server doesn't say), for
/// the progress bar in Settings → Updates.
static RECEIVED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TOTAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn download(u: &mut Update) -> Result<()> {
    ensure!(u.available, "Check for an update first");
    let url = u.url.as_deref().context("No update asset")?;
    let suffix = if cfg!(target_os = "macos") {
        ".dmg"
    } else if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ".AppImage"
    };
    ensure!(
        reqwest::Url::parse(url)?.path().ends_with(suffix),
        "Unsupported installer format"
    );
    let dir = ns2bench_core::config::state_dir().join("updates");
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!(
        "NS2Bench-{}{}",
        u.latest_version.as_deref().context("version missing")?,
        suffix
    ));
    let tmp = path.with_extension("partial");
    let response = client()?.get(url).send().await?.error_for_status()?;
    ensure!(
        response.content_length().is_none_or(|n| n <= MAX_DOWNLOAD),
        "installer too large"
    );
    use std::sync::atomic::Ordering::Relaxed;
    TOTAL.store(response.content_length().unwrap_or(0), Relaxed);
    RECEIVED.store(0, Relaxed);
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut chunks = response.bytes_stream();
    let mut hash = Sha256::new();
    let mut size = 0;
    let result: Result<()> = async {
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk?;
            size += chunk.len() as u64;
            RECEIVED.store(size, Relaxed);
            ensure!(size <= MAX_DOWNLOAD, "installer too large");
            hash.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.sync_all().await?;
        ensure!(
            hex(&hash.finalize()) == u.hash.as_deref().unwrap_or(""),
            "SHA-256 mismatch; installer discarded"
        );
        Ok(())
    }
    .await;
    drop(file);
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    tokio::fs::rename(tmp, &path).await?;
    u.path = Some(path);
    u.downloaded = true;
    u.message = "Verified update ready to install".into();
    Ok(())
}
async fn verify(path: &Path, expected: &str) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    ensure!(
        hex(&hash.finalize()) == expected,
        "Downloaded installer changed; download it again"
    );
    Ok(())
}
#[cfg(target_os = "macos")]
async fn install(path: &Path, relaunch: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    let bundle = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .context("Run the packaged app to update")?;
    ensure!(
        bundle.file_name().is_some_and(|s| s == "NS2Bench.app"),
        "Unexpected app bundle"
    );
    let parent = bundle.parent().context("App folder missing")?;
    // Stage in the destination filesystem. A permission error occurs before quitting.
    let stage = parent.join(format!(".ns2bench-update-{}", std::process::id()));
    std::fs::create_dir(&stage)
        .context("App folder is not writable; move NS2Bench to Applications in your home folder")?;
    let mount = ns2bench_core::config::state_dir().join("updates/mount");
    std::fs::create_dir_all(&mount)?;
    // an image left mounted by an earlier attempt (a crash, a forced quit) would block this one
    let _ = tokio::process::Command::new("/usr/bin/hdiutil").args(["detach", "-force"]).arg(&mount)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().await;
    let attach = tokio::process::Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount)
        .arg(path)
        .output()
        .await?;
    if !attach.status.success() {
        let _ = std::fs::remove_dir(&stage);
        bail!(
            "Could not mount verified update: {}",
            String::from_utf8_lossy(&attach.stderr)
        );
    }
    let result = tokio::process::Command::new("/usr/bin/ditto")
        .arg(mount.join("NS2Bench.app"))
        .arg(stage.join("NS2Bench.app"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    let _ = tokio::process::Command::new("/usr/bin/hdiutil")
        .arg("detach")
        .arg(&mount)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    if !result.as_ref().is_ok_and(|s| s.success()) {
        let _ = std::fs::remove_dir_all(&stage);
        anyhow::bail!("Could not stage update");
    }
    let plist = stage.join("NS2Bench.app/Contents/Info.plist");
    if !plist.is_file() {
        let _ = std::fs::remove_dir_all(&stage);
        anyhow::bail!("Update is missing its app bundle");
    }
    let signature = tokio::process::Command::new("/usr/bin/codesign")
        // Any valid signature isn't enough: it must be NS2Bench's own certificate
        // (scripts/with-signing.sh), so a swapped download can't install itself.
        .args(["--verify", "--deep", "--strict", RELEASE_REQUIREMENT])
        .arg(stage.join("NS2Bench.app"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;
    if !signature.success() {
        let _ = std::fs::remove_dir_all(&stage);
        anyhow::bail!("App signature verification failed");
    }
    let script = ns2bench_core::config::state_dir().join("updates/install.sh");
    // the staged copy is ~100 MB in the Applications folder: never leave it behind on an error
    let cleanup = |e: std::io::Error| { let _ = std::fs::remove_dir_all(&stage); e };
    // All paths are positional parameters; no path or release text is evaluated as shell code.
    std::fs::write(
        &script,
        r#"#!/bin/sh
pid=$1; app=$2; stage=$3; relaunch=$4
while kill -0 "$pid" 2>/dev/null; do sleep 1; done
backup="$stage/previous.app"
if mv "$app" "$backup"; then
  if mv "$stage/NS2Bench.app" "$app"; then
    rm -rf "$stage"
    [ "$relaunch" = 1 ] && /usr/bin/open "$app"
    exit 0
  fi
  mv "$backup" "$app"
fi
rm -rf "$stage"
[ "$relaunch" = 1 ] && /usr/bin/open "$app"
exit 1
"#,
    ).map_err(cleanup)?;
    std::process::Command::new("/bin/sh")
        .arg(script)
        .arg(std::process::id().to_string())
        .arg(bundle)
        .arg(&stage)
        .arg(if relaunch { "1" } else { "0" })
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn().map_err(cleanup)?;
    Ok(())
}
#[cfg(target_os = "windows")]
async fn install(path: &Path, relaunch: bool) -> Result<()> {
    // Tauri NSIS update mode: a progress bar, no questions; /R starts the
    // app again afterwards (not when installing as NS2Bench quits).
    let args: &[&str] = if relaunch { &["/P", "/R", "/UPDATE"] } else { &["/P", "/UPDATE"] };
    std::process::Command::new(path)
        .args(args)
        .spawn()?;
    Ok(())
}
#[cfg(target_os = "linux")]
async fn install(path: &Path, relaunch: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let current=std::env::var_os("APPIMAGE").context("Automatic installation needs the AppImage build; use your package manager for a system package")?;
    let current = PathBuf::from(current);
    let stage = current.with_extension("ns2bench-new");
    let backup = current.with_extension("ns2bench-backup");
    // The running AppImage is the proof the previous update worked: its backup is spent.
    let _ = std::fs::remove_file(&backup);
    tokio::fs::copy(path, &stage).await?;
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&current, &backup)?;
    if let Err(e) = std::fs::rename(&stage, &current) {
        let _ = std::fs::rename(&backup, &current);
        return Err(e.into());
    }
    if let Err(e) = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 1; done; [ "$3" = 1 ] && exec "$2""#)
        .arg("ns2bench-update")
        .arg(std::process::id().to_string())
        .arg(&current)
        .arg(if relaunch { "1" } else { "0" })
        .spawn()
    {
        let _ = std::fs::rename(&backup, &current);
        return Err(e.into());
    }
    Ok(())
}
/// Headless updates: `ns2bench-app update check|install`. Same checks as the UI:
/// manifest, platform asset, SHA-256 before and after download, then the
/// platform installer (which restarts the app once this process exits).
pub async fn cli(action: &str) -> Result<Value> {
    ensure!(matches!(action, "check" | "install"), "usage: ns2bench-app update check|install");
    let mut u = check().await?;
    if action == "check" || !u.available {
        return Ok(json!(u));
    }
    ensure!(action == "install", "usage: ns2bench-app update check|install");
    download(&mut u).await?;
    let path = u.path.clone().context("download produced no file")?;
    verify(&path, u.hash.as_deref().context("checksum missing")?).await?;
    install(&path, true).await?;
    Ok(json!(u))
}

#[tauri::command]
pub async fn update_action(
    action: String,
    app: AppHandle,
    state: State<'_, Updates>,
) -> Result<Value, String> {
    // "Install when NS2Bench closes" (Settings → Updates), told by the page.
    if action == "quit_install_on" || action == "quit_install_off" {
        INSTALL_ON_QUIT.store(action == "quit_install_on", std::sync::atomic::Ordering::Relaxed);
        return Ok(json!({"install_on_quit": action == "quit_install_on"}));
    }
    // Asked while a download runs (which holds the lock): answered without waiting for it.
    if action == "progress" {
        use std::sync::atomic::Ordering::Relaxed;
        return Ok(json!({"received": RECEIVED.load(Relaxed), "total": TOTAL.load(Relaxed)}));
    }
    let mut u = state
        .0
        .try_lock()
        .map_err(|_| "An update operation is already running".to_string())?;
    let result: Result<()> = async {
        match action.as_str() {
            "status" => {
                if u.current_version.is_empty() {
                    u.current_version = env!("CARGO_PKG_VERSION").into();
                }
            }
            "check" => {
                let fresh = check().await?;
                // A finished download of this same version stays (no second download).
                let keep = u.downloaded && fresh.latest_version == u.latest_version && fresh.hash == u.hash;
                let (path, downloaded) = (u.path.clone(), u.downloaded);
                *u = fresh;
                if keep {
                    u.path = path;
                    u.downloaded = downloaded;
                    u.message = "Verified update ready to install".into();
                }
                // Only the AppImage can replace itself. A .deb install is told where the new one is,
                // instead of downloading an AppImage it could never install.
                if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_none() && u.available {
                    u.manual = true;
                    u.message = "This copy was installed as a system package: download the new .deb from https://github.com/phantomptr/ns2bench/releases/latest".into();
                }
            }
            "download" if u.manual => bail!("{}", u.message.clone()),
            "download" => download(&mut u).await?,
            "install" => {
                let path = u.path.as_deref().context("Download the update first")?;
                verify(path, u.hash.as_deref().context("checksum missing")?).await?;
                install(path, true).await?;
                // Installing now: nothing left to install as NS2Bench quits (no second installer).
                INSTALLING.store(true, std::sync::atomic::Ordering::SeqCst);
                u.downloaded = false;
                app.exit(0);
            }
            _ => bail!("Unknown update action"),
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        u.message = e.to_string();
        return Err(e.to_string());
    }
    Ok(json!(*u))
}
/// Lower-case hex of a digest (sha2 0.11 digests don't implement LowerHex).
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Install a downloaded update when NS2Bench quits (on by default).
static INSTALL_ON_QUIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// An installer already started (Restart now, or the idle install).
static INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called as the app exits: if an update was downloaded and verified, and installing on quit is
/// on, start the installer without opening NS2Bench again (quitting still means quitting).
pub fn install_on_quit(app: &AppHandle) {
    if !INSTALL_ON_QUIT.load(std::sync::atomic::Ordering::Relaxed) || INSTALLING.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let state = app.state::<Updates>();
    let Ok(u) = state.0.try_lock() else { return };
    let (Some(path), Some(hash)) = (u.path.clone(), u.hash.clone()) else { return };
    if !u.downloaded || u.manual {
        return;
    }
    drop(u);
    let r = tauri::async_runtime::block_on(async move {
        verify(&path, &hash).await?;
        install(&path, false).await
    });
    match r {
        Ok(()) => ns2bench_core::report::trace("update installs as NS2Bench quits"),
        Err(e) => ns2bench_core::report::trace(format!("update on quit failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn altered_installer_is_rejected_before_install() {
        let path = std::env::temp_dir().join(format!("ns2bench-hash-{}", std::process::id()));
        tokio::fs::write(&path, b"abc").await.unwrap();
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&path, hash).await.is_ok());
        tokio::fs::write(&path, b"changed").await.unwrap();
        assert!(verify(&path, hash).await.is_err());
        tokio::fs::remove_file(path).await.unwrap();
    }
    // A throwaway key pair; the signature was made by Node's crypto.sign, as the release
    // workflow does, so this also checks both sides build the same signed text.
    const TEST_KEY: &str = "7PXDr6RIAyZig6k4QKsoEu4vN9qyxRHOILjZqT9j+jU=";
    const TEST_SIG: &str = "+ga34N7aIJyA926gphX7nzK8ei4NGly8eRJHnM182BfIaZHSjHSqnZzHdBno09Ky+umGWiEkmIziHdslk8HXCA==";
    fn select(m: Manifest, key: &str, current: &str) -> Result<Update> {
        super::select_with(m, key, current, TEST_KEY)
    }
    #[test]
    fn needs_a_valid_signature() {
        let url = "https://github.com/phantomptr/ns2bench/releases/download/v0.3.0/NS2Bench.dmg";
        let m = |sig: Option<&str>, hash: String| serde_json::from_value::<Manifest>(json!({"version":"0.3.0","assets":{"test":url},
            "sha256":{"test":hash}, "signatures": sig.map(|s| json!({"test": s})).unwrap_or(json!({}))})).unwrap();
        assert!(select(m(Some(TEST_SIG), "a".repeat(64)), "test", "0.2.0").unwrap().available);
        assert!(select(m(None, "a".repeat(64)), "test", "0.2.0").is_err());
        // a different installer (hash) under the same signature
        assert!(select(m(Some(TEST_SIG), "b".repeat(64)), "test", "0.2.0").is_err());
        // the real key doesn't accept the test signature
        assert!(super::select(m(Some(TEST_SIG), "a".repeat(64)), "test", "0.2.0").is_err());
    }
    #[test]
    fn rejects_missing_hash_external_asset_and_prerelease() {
        let manifest = |url: &str, hash: &str, version: &str| {
            serde_json::from_value::<Manifest>(
                json!({"version":version,"assets":{"test":url},"sha256":{"test":hash},"signatures":{"test":TEST_SIG}}),
            )
            .unwrap()
        };
        let url = "https://github.com/phantomptr/ns2bench/releases/download/v0.3.0/NS2Bench.dmg";
        assert!(
            select(manifest(url, &"a".repeat(64), "0.3.0"), "test", "0.2.0")
                .unwrap()
                .available
        );
        assert!(select(manifest(url, "", "0.3.0"), "test", "0.2.0").is_err());
        assert!(select(
            manifest("https://evil.test/x", &"a".repeat(64), "0.3.0"),
            "test",
            "0.2.0"
        )
        .is_err());
        assert!(
            !select(
                manifest(url, &"a".repeat(64), "0.3.0-beta.1"),
                "test",
                "0.2.0"
            )
            .unwrap()
            .available
        );
        assert!(!asset_url(
            "https://github.com/phantomptr/other/releases/download/v1/x"
        ));
        assert!(!asset_url(
            "http://github.com/phantomptr/ns2bench/releases/download/v1/x"
        ));
    }
}
