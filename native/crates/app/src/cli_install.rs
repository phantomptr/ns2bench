//! "Install command line tool": put the `ns2bench` CLI that ships inside the app on the PATH.
//!   macOS    /usr/local/bin/ns2bench -> NS2Bench.app/Contents/MacOS/ns2bench (asks for an
//!            administrator password only if /usr/local/bin isn't writable)
//!   Linux    ~/.local/bin/ns2bench (a copy for an AppImage, whose files vanish when it exits;
//!            a link otherwise). The .deb already installs /usr/bin/ns2bench.
//!   Windows  adds the install folder (which holds ns2bench.exe) to the user's PATH.
//! A link follows app updates; nothing is installed until the person clicks the button.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const EXE: &str = if cfg!(windows) { "ns2bench.exe" } else { "ns2bench" };

/// The CLI bundled next to the app executable.
fn bundled() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join(EXE);
    p.is_file().then_some(p)
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Where the command goes on this platform.
fn target() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(PathBuf::from("/usr/local/bin/ns2bench"))
    } else if cfg!(windows) {
        bundled()
    } else {
        Some(home()?.join(".local/bin/ns2bench"))
    }
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

/// The `ns2bench` a terminal would run, if any.
#[allow(clippy::needless_return)] // one platform's block returns early
fn on_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let dir = bundled()?.parent()?.to_path_buf();
        return windows_user_path().iter().any(|d| same_dir(d, &dir)).then(|| dir.join(EXE));
    }
    #[cfg(not(windows))]
    {
        // The app may have been started from Finder with a minimal PATH: also look in
        // the places a login shell has.
        let mut dirs = path_dirs();
        for d in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"] {
            dirs.push(PathBuf::from(d));
        }
        if let Some(h) = home() {
            dirs.push(h.join(".local/bin"));
        }
        dirs.into_iter().map(|d| d.join(EXE)).find(|p| p.is_file())
    }
}

#[cfg(windows)]
fn same_dir(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    n(a) == n(b)
}

#[cfg(windows)]
fn windows_user_path() -> Vec<PathBuf> {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "[Environment]::GetEnvironmentVariable('Path','User')"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .split(';')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// Is `p` (a link or copy) the bundled CLI, or another ns2bench?
fn points_to_bundled(p: &Path) -> bool {
    match (std::fs::canonicalize(p), bundled().and_then(|b| std::fs::canonicalize(b).ok())) {
        (Ok(a), Some(b)) => a == b,
        _ => false,
    }
}

fn appimage() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some()
}

pub fn status() -> Value {
    let found = on_path();
    let ours = found.as_deref().is_some_and(|p| cfg!(windows) || points_to_bundled(p) || appimage());
    let dir_on_path = target()
        .and_then(|t| t.parent().map(Path::to_path_buf))
        .is_some_and(|d| cfg!(windows) || path_dirs().contains(&d));
    json!({
        "available": bundled().is_some(),
        "installed": found.is_some(),
        "ours": ours,
        "path": found.map(|p| p.display().to_string()),
        "target": target().map(|p| p.display().to_string()),
        // Linux: ~/.local/bin may not be on PATH yet (it is after the next login on most distros)
        "dir_on_path": dir_on_path,
        "system_package": cfg!(target_os = "linux") && bundled().is_some_and(|b| b.starts_with("/usr/bin")),
    })
}

#[allow(clippy::needless_return)] // one platform's block returns early
pub fn install() -> anyhow::Result<Value> {
    use anyhow::{bail, Context};
    let src = bundled().context("This copy of NS2Bench doesn't include the command line tool. Download it from the GitHub release.")?;
    if cfg!(target_os = "linux") && src.starts_with("/usr/bin") {
        return Ok(status()); // the .deb put it on the PATH already
    }
    let dst = target().context("Could not find your home folder")?;
    #[cfg(windows)]
    {
        let _ = &dst;
        let dir = src.parent().context("install folder")?;
        // Prepend to the user PATH (no administrator rights); SetEnvironmentVariable
        // tells new terminals about the change.
        let ps = "$d=$env:NS2BENCH_DIR; $p=[Environment]::GetEnvironmentVariable('Path','User'); \
                  $parts=@($p -split ';' | Where-Object { $_ -and ($_.TrimEnd('\\') -ne $d.TrimEnd('\\')) }); \
                  [Environment]::SetEnvironmentVariable('Path', (@($d) + $parts) -join ';', 'User')";
        let ok = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps])
            .env("NS2BENCH_DIR", dir)
            .status()
            .context("Could not run PowerShell")?
            .success();
        if !ok {
            bail!("Could not add {} to your PATH", dir.display());
        }
        return Ok(status());
    }
    #[cfg(not(windows))]
    {
        if let Ok(meta) = std::fs::symlink_metadata(&dst) {
            if !meta.file_type().is_symlink() && !points_to_bundled(&dst) && !appimage() {
                bail!("{} already exists and isn't NS2Bench's. Remove it first, then try again.", dst.display());
            }
        }
        let dir = dst.parent().context("target folder")?;
        let direct = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(dir)?;
            let tmp = dir.join(format!(".ns2bench.{}", std::process::id()));
            let _ = std::fs::remove_file(&tmp);
            if appimage() {
                std::fs::copy(&src, &tmp)?;
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
            } else {
                std::os::unix::fs::symlink(&src, &tmp)?;
            }
            std::fs::rename(&tmp, &dst).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })
        })();
        match direct {
            Ok(()) => {}
            #[cfg(target_os = "macos")]
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => admin_link(&src, &dst)?,
            Err(e) => bail!("Could not create {}: {e}", dst.display()),
        }
        Ok(status())
    }
}

/// macOS: /usr/local/bin is root-owned on a fresh Mac. Ask for an administrator password
/// with the system dialog. Paths are passed as arguments, never pasted into the script.
#[cfg(target_os = "macos")]
fn admin_link(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let script = r#"on run argv
        do shell script "/bin/mkdir -p " & quoted form of (item 3 of argv) & " && /bin/ln -sfh " & quoted form of (item 1 of argv) & " " & quoted form of (item 2 of argv) with prompt "NS2Bench wants to install the ns2bench command." with administrator privileges
    end run"#;
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .arg(src)
        .arg(dst)
        .arg(dst.parent().unwrap_or(Path::new("/usr/local/bin")))
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("-128") {
        anyhow::bail!("Cancelled");
    }
    anyhow::bail!("Could not create {}: {}", dst.display(), err.trim())
}

#[allow(clippy::needless_return)] // one platform's block returns early
pub fn uninstall() -> anyhow::Result<Value> {
    use anyhow::{bail, Context};
    #[cfg(windows)]
    {
        let dir = bundled().and_then(|b| b.parent().map(Path::to_path_buf)).context("install folder")?;
        let ps = "$d=$env:NS2BENCH_DIR; $p=[Environment]::GetEnvironmentVariable('Path','User'); \
                  [Environment]::SetEnvironmentVariable('Path', (@($p -split ';' | Where-Object { $_ -and ($_.TrimEnd('\\') -ne $d.TrimEnd('\\')) }) -join ';'), 'User')";
        let ok = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps])
            .env("NS2BENCH_DIR", &dir)
            .status()?
            .success();
        if !ok {
            bail!("Could not update your PATH");
        }
        return Ok(status());
    }
    #[cfg(not(windows))]
    {
        let dst = target().context("Could not find your home folder")?;
        // Only remove what the app put there.
        let is_link = std::fs::symlink_metadata(&dst).is_ok_and(|m| m.file_type().is_symlink());
        if !(points_to_bundled(&dst) || (is_link && std::fs::read_link(&dst).is_ok_and(|t| t.ends_with(format!("MacOS/{EXE}")))) || (appimage() && dst.is_file())) {
            bail!("{} wasn't installed by NS2Bench", dst.display());
        }
        match std::fs::remove_file(&dst) {
            Ok(()) => {}
            #[cfg(target_os = "macos")]
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                let script = r#"on run argv
                    do shell script "/bin/rm -f " & quoted form of (item 1 of argv) with prompt "NS2Bench wants to remove the ns2bench command." with administrator privileges
                end run"#;
                let ok = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).arg(&dst).status()?.success();
                if !ok {
                    bail!("Could not remove {}", dst.display());
                }
            }
            Err(e) => bail!("Could not remove {}: {e}", dst.display()),
        }
        Ok(status())
    }
}

#[tauri::command]
pub async fn cli_action(action: String) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || match action.as_str() {
        "status" => Ok(status()),
        "install" => install(),
        "uninstall" => uninstall(),
        _ => Err(anyhow::anyhow!("Unknown action")),
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
