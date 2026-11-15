//! NS2Bench engine. See the repository README.
pub mod audio;
pub mod bench;
pub mod calib;
pub mod ch9329;
pub mod client;
pub mod mcp;
pub mod doctor;
pub mod dvr;
pub mod nav;
pub mod ocr;
pub mod testrun;
pub mod config;
pub mod features;
pub mod h264;
pub mod inputs;
pub mod logs;
pub mod server;
pub mod transport;
pub mod video;

pub use bench::Bench;
pub use config::Config;

/// Build the bench, start its tasks and serve HTTP until the process exits.
pub async fn serve(cfg: Config) -> anyhow::Result<()> {
    let host = cfg.server["host"].as_str().unwrap_or("127.0.0.1").to_string();
    let port = cfg.server["port"].as_u64().unwrap_or(7630) as u16;
    if !config::is_loopback(&host) && cfg.token().is_none() {
        anyhow::bail!("binding to a non-loopback address requires server.token");
    }
    let bench = Bench::new(cfg.clone());
    bench.start();
    let app = server::router(bench.clone());
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    server::SERVING.store(true, std::sync::atomic::Ordering::Relaxed);
    println!("NS2Bench: open {}/  (native engine)", cfg.url());
    let b2 = bench.clone();
    axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        b2.hub.release_all();
    }).await?;
    Ok(())
}

pub mod settings;
