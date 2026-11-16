//! NS2Bench desktop app: the bench UI in a native window, with the engine running in-process.
//!   ns2bench-app          open the window (starts the engine, or attaches to one already running)
//!   ns2bench-app mcp      MCP server on stdio, for agents (same as `ns2bench mcp`)
//! Config: $NS2BENCH_CONFIG, else ./bench.json, else ~/.ns2bench/config.json.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod agents;
mod cli_install;
mod components;
mod updates;
use tauri::Manager;
use ns2bench_core::client::Client;
use std::time::{Duration, Instant};
use tauri::{WebviewUrl, WebviewWindowBuilder};

fn load_config() -> ns2bench_core::Config {
    let explicit = std::env::var("NS2BENCH_CONFIG").ok();
    ns2bench_core::Config::load(explicit.as_deref()).unwrap_or_else(|e| {
        eprintln!("NS2Bench: config: {e}; using defaults");
        ns2bench_core::Config::from_raw(None, serde_json::json!({}))
    })
}

/// Open a web link in the system browser. Only https: links; everything else stays
/// inside the app (the engine's own pages) or is refused.
fn open_external(url: &tauri::Url) {
    if url.scheme() != "https" {
        return;
    }
    let u = url.as_str();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("/usr/bin/open").arg(u).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", u]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(u).spawn();
    match r {
        // Reap the launcher so it doesn't linger as a zombie.
        Ok(mut child) => { std::thread::spawn(move || { let _ = child.wait(); }); }
        Err(e) => eprintln!("NS2Bench: could not open {u}: {e}"),
    }
}

/// Save downloads (bug-report bundle, screenshots, logs) to the Downloads folder,
/// never over an existing file.
fn download_path(dir: &std::path::Path, suggested: &std::path::Path) -> std::path::PathBuf {
    let name = suggested.file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or("ns2bench-download");
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    let mut p = dir.join(name);
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    p
}

/// Release builds on Windows have no console of their own: borrow the terminal's,
/// so `ns2bench-app update` and `ns2bench-app mcp` errors are visible.
fn attach_console() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
}

/// Is an NS2Bench engine already answering at this config's address?
async fn running(cfg: &ns2bench_core::Config) -> bool {
    let c = Client::from_config(cfg, "ui");
    matches!(tokio::time::timeout(Duration::from_millis(800), c.get("/api/status")).await,
        Ok(Ok(r)) if r.status == 200 && r.text().contains("uptime_s"))
}

/// After a crash: say so, and where the details are (a Windows window app has no console).
fn crash_notice(path: &std::path::Path) {
    eprintln!("NS2Bench stopped unexpectedly. Details: {}", path.display());
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let text = wide(&format!("NS2Bench stopped unexpectedly.\n\nThe details were saved to:\n{}\n\nPlease attach this file when you report the problem (GitHub issue or Discord).", path.display()));
        let title = wide("NS2Bench");
        // SAFETY: both strings are NUL-terminated and live for the call.
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    }
}

/// Help → Open logs folder.
#[tauri::command]
fn open_logs() {
    let dir = ns2bench_core::config::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let _ = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&dir).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("explorer").arg(&dir).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&dir).spawn()
    };
}

fn main() {
    ns2bench_core::crashlog::install("app", crash_notice);
    let args: Vec<String> = std::env::args().collect();
    // `ns2bench-app report`: a diagnostic zip in Downloads, without opening the window (for when
    // the window can't open). `ns2bench-app --safe`: start without capture, input or sound.
    if args.get(1).map(String::as_str) == Some("report") {
        attach_console();
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        match rt.block_on(ns2bench_core::report::save_offline(&ns2bench_core::report::downloads_dir())) {
            Ok(p) => { println!("NS2Bench report saved: {}", p.display()); ns2bench_core::report::reveal(&p); }
            Err(e) => { eprintln!("NS2Bench report: {e}"); std::process::exit(1); }
        }
        return;
    }
    if args.iter().any(|a| a == "--safe") {
        // SAFETY: set before any other thread starts.
        unsafe { std::env::set_var("NS2BENCH_SAFE", "1") };
    }
    if args.get(1).map(String::as_str) == Some("update") {
        attach_console();
        let action = args.get(2).map(String::as_str).unwrap_or("check");
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        // Installing swaps the app on disk and restarts it: never under a running copy.
        if action == "install" && rt.block_on(running(&load_config())) {
            eprintln!("NS2Bench update: NS2Bench is running. Quit it first, or install from Settings → Updates.");
            std::process::exit(1);
        }
        match rt.block_on(updates::cli(action)) {
            Ok(v) => println!("{v}"),
            Err(e) => {
                eprintln!("NS2Bench update: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("mcp") {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        if let Err(e) = rt.block_on(ns2bench_core::mcp::run(load_config())) {
            eprintln!("NS2Bench mcp: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Some(prev) = ns2bench_core::report::start_trace("app") {
        eprintln!("NS2Bench: the last run ended unexpectedly; its startup trace:\n{prev}");
    }
    ns2bench_core::report::trace(if ns2bench_core::config::safe_mode() { "safe mode: no capture, input or sound" } else { "normal start" });
    let cfg = load_config();
    ns2bench_core::report::trace(format!("config loaded ({})", cfg.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "defaults".into())));
    let url = format!("{}/", cfg.url());
    let _ = agents::ENGINE_URL.set(cfg.url());
    agents::TOKEN_REQUIRED.store(cfg.token().is_some(), std::sync::atomic::Ordering::Relaxed);
    let attach = tauri::async_runtime::block_on(running(&cfg));
    // The window's proof that it is the app (kept in memory, never written anywhere).
    let ui_key = updates::hex(&rand::random::<[u8; 24]>());
    if attach {
        eprintln!("NS2Bench: attaching to the engine already running at {url}");
    } else {
        ns2bench_core::server::set_ui_key(ui_key.clone());
        let c = cfg.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = ns2bench_core::serve(c).await {
                eprintln!("NS2Bench: engine stopped: {e}");
                // The window then has nothing to show: keep the reason for the bug report.
                ns2bench_core::crashlog::write("engine", &format!("the engine stopped: {e:#}"));
            }
        });
        // wait for the engine to answer before showing the UI
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(15) && !tauri::async_runtime::block_on(running(&cfg)) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // Our own engine holds the port (not something else that happens to answer on it): only
    // then does the page get the UI key and the install/update commands.
    let ours = !attach && ns2bench_core::server::SERVING.load(std::sync::atomic::Ordering::Relaxed);
    ns2bench_core::report::trace(if attach { "attached to an engine already running" } else if ours { "engine started" } else { "engine did NOT start (port in use?)" });
    if !attach && !ours {
        eprintln!("NS2Bench: the engine could not start on {url} (is another program using that port?)");
    }

    let mut target: tauri::Url = url.parse().expect("engine url");
    if let Some(token) = cfg.token() { target.query_pairs_mut().append_pair("token", &token); }
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(updates::Updates::default())
        .invoke_handler(tauri::generate_handler![updates::update_action, cli_install::cli_action, components::component_action, agents::agent_action, open_logs])
        .setup(move |app| {
            // Installing, updating and the command line tool are only offered to the engine this
            // app started itself, never to whatever else may be answering on the port.
            let mut cap = tauri::ipc::CapabilityBuilder::new("engine-ui").local(false).window("main")
                .remote(format!("{}/*", target.origin().ascii_serialization())).permission("notification:default");
            if ours {
                cap = cap.permission("allow-update-action").permission("allow-cli-action").permission("allow-component-action").permission("allow-agent-action").permission("allow-open-logs");
            }
            app.add_capability(cap)?;
            let mut window = app.config().app.windows.iter().find(|w| w.label == "main")
                .expect("main window configuration").clone();
            window.url = WebviewUrl::External(target.clone());
            window.visible = false;
            let origin = target.origin();
            let nav_origin = origin.clone();
            let key_script = if !ours { String::new() } else { format!("window.__NS2BENCH_UI__ = {ui_key:?};") };
            let main = WebviewWindowBuilder::from_config(app, &window)?
                .initialization_script(&key_script)
                // The engine's pages stay in the app; web links open in the browser.
                .on_navigation(move |url| {
                    if url.origin() == nav_origin || url.scheme() == "blob" || url.scheme() == "about" {
                        return true;
                    }
                    open_external(url);
                    false
                })
                .on_new_window(move |url, _| {
                    open_external(&url);
                    tauri::webview::NewWindowResponse::Deny
                })
                .on_download(|webview, event| {
                    if let tauri::webview::DownloadEvent::Requested { destination, .. } = event {
                        let dir = webview.path().download_dir().unwrap_or_else(|_| std::env::temp_dir());
                        *destination = download_path(&dir, destination);
                    }
                    true
                })
                .build()?;
            // Leave room for the OS title bar and dock/taskbar, including on
            // scaled displays whose logical work area is smaller than 1480x920.
            if let Some(monitor) = main.current_monitor()? {
                let area = monitor.work_area();
                let scale = monitor.scale_factor();
                let width = (1480.0_f64).min((area.size.width as f64 / scale - 40.0).max(320.0));
                let height = (920.0_f64).min((area.size.height as f64 / scale - 80.0).max(240.0));
                main.set_min_size(Some(tauri::LogicalSize::new(width.min(960.0), height.min(600.0))))?;
                main.set_size(tauri::LogicalSize::new(width, height))?;
                main.set_position(tauri::PhysicalPosition::new(
                    area.position.x + ((area.size.width as f64 - width * scale) / 2.0) as i32,
                    area.position.y + (20.0 * scale) as i32,
                ))?;
            }
            main.show()?;
            ns2bench_core::report::trace("window shown");
            Ok(())
        })
        .build(tauri::generate_context!())
        .inspect_err(|e| { ns2bench_core::report::trace(format!("window could not be created: {e}")); })
        .expect("error while running the NS2Bench app")
        .run(|_, event| {
            match event {
                tauri::RunEvent::Ready => ns2bench_core::report::trace("ready"),
                // closing normally: no "ended unexpectedly" next time
                tauri::RunEvent::Exit => { ns2bench_core::report::trace("closed normally"); ns2bench_core::report::clean_exit(); }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn downloads_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("ns2bench-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = super::download_path(&dir, std::path::Path::new("/x/bundle.zip"));
        assert_eq!(p, dir.join("bundle.zip"));
        std::fs::write(&p, "a").unwrap();
        assert_eq!(super::download_path(&dir, std::path::Path::new("bundle.zip")), dir.join("bundle (1).zip"));
        assert_eq!(super::download_path(&dir, std::path::Path::new("")), dir.join("ns2bench-download"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
