//! NS2Bench desktop app: the bench UI in a native window, with the engine running in-process.
//!   ns2bench-app          open the window (starts the engine, or attaches to one already running)
//!   ns2bench-app mcp      MCP server on stdio, for agents (same as `ns2bench mcp`)
//! Config: $NS2BENCH_CONFIG, else ./bench.json, else ~/.ns2bench/config.json.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

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

/// Is an NS2Bench engine already answering at this config's address?
async fn running(cfg: &ns2bench_core::Config) -> bool {
    let c = Client::from_config(cfg, "ui");
    matches!(tokio::time::timeout(Duration::from_millis(800), c.get("/api/status")).await,
        Ok(Ok(r)) if r.status == 200 && r.text().contains("uptime_s"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("mcp") {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        if let Err(e) = rt.block_on(ns2bench_core::mcp::run(load_config())) {
            eprintln!("NS2Bench mcp: {e}");
            std::process::exit(1);
        }
        return;
    }

    let cfg = load_config();
    let url = format!("{}/", cfg.url());
    let attach = tauri::async_runtime::block_on(running(&cfg));
    if attach {
        eprintln!("NS2Bench: attaching to the engine already running at {url}");
    } else {
        let c = cfg.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = ns2bench_core::serve(c).await {
                eprintln!("NS2Bench: engine stopped: {e}");
            }
        });
        // wait for the engine to answer before showing the UI
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(15) && !tauri::async_runtime::block_on(running(&cfg)) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    let mut target: tauri::Url = url.parse().expect("engine url");
    if let Some(token) = cfg.token() { target.query_pairs_mut().append_pair("token", &token); }
    tauri::Builder::default()
        .manage(updates::Updates::default())
        .invoke_handler(tauri::generate_handler![updates::update_action])
        .setup(move |app| {
            app.add_capability(tauri::ipc::CapabilityBuilder::new("engine-updates").local(false).window("main")
                .remote(format!("{}/*", target.origin().ascii_serialization())).permission("allow-update-action"))?;
            let mut window = app.config().app.windows.iter().find(|w| w.label == "main")
                .expect("main window configuration").clone();
            window.url = WebviewUrl::External(target.clone());
            window.visible = false;
            let origin = target.origin();
            let nav_origin = origin.clone();
            let main = WebviewWindowBuilder::from_config(app, &window)?
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
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the NS2Bench app");
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
