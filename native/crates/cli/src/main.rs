use clap::{Parser, Subcommand};
use ns2bench_core::client::Client;
use serde_json::{json, Value};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "ns2bench", version, about = "See, hear and drive a game console by hand or by AI agents")]
struct Cli {
    /// config JSON (default: ./bench.json or ~/.ns2bench/config.json)
    #[arg(long, global = true)]
    config: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// start the UI/API (open http://127.0.0.1:7630)
    Serve {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// MCP server on stdio for Claude Code, Codex, opencode (talks to the running server)
    Mcp,
    /// status of the running server
    Status,
    /// save a screenshot from the running server
    Shot {
        #[arg(short, long)]
        output: Option<String>,
        #[arg(long)]
        png: bool,
    },
    /// press buttons through the running server, e.g. `ns2bench press A`
    Press {
        #[arg(required = true)]
        buttons: Vec<String>,
        #[arg(long, default_value_t = 100)]
        hold_ms: u64,
    },
    /// check capture, board, USB, console and network end to end
    Doctor {
        /// also press RIGHT/LEFT (HOME menu only) to prove input works
        #[arg(long)]
        probe: bool,
    },
    /// run test files against the console; reports go to ~/.ns2bench/reports
    Test {
        #[arg(required = true)]
        files: Vec<String>,
        /// run every step even after a failure
        #[arg(long)]
        keep_going: bool,
    },
    /// list capture devices this machine can see
    Devices,
    /// build + install the NS2Bench Bridge daemon, scripts and token over SSH (needs the source tree)
    BoardInstall {
        /// SSH host of the Linux USB-gadget board
        host: String,
        #[arg(long, default_value = "armv7-unknown-linux-musleabihf")]
        target: String,
    },
}

async fn api(c: &Client, method: &str, path: &str, body: Option<Value>) -> Result<ns2bench_core::client::Resp, String> {
    c.call(method, path, body.as_ref()).await.map_err(|e| e.to_string())
}

async fn run(cli: Cli) -> Result<ExitCode, String> {
    let mut cfg = ns2bench_core::Config::load(cli.config.as_deref()).map_err(|e| e.to_string())?;
    let c = Client::from_config(&cfg, "api");
    match cli.cmd {
        Cmd::Serve { host, port } => {
            if let Some(h) = host { cfg.server["host"] = h.into(); }
            if let Some(p) = port { cfg.server["port"] = p.into(); }
            ns2bench_core::serve(cfg).await.map_err(|e| e.to_string())?;
        }
        Cmd::Mcp => ns2bench_core::mcp::run(cfg).await.map_err(|e| e.to_string())?,
        Cmd::Status => {
            let v = api(&c, "GET", "/api/status", None).await?.json().map_err(|e| e.to_string())?;
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
        }
        Cmd::Shot { output, png } => {
            let r = api(&c, "GET", &format!("/api/screenshot?format={}", if png { "png" } else { "jpeg" }), None).await?;
            let out = output.unwrap_or_else(|| format!("ns2bench-{}.{}", chrono_stamp(), if png { "png" } else { "jpg" }));
            std::fs::write(&out, r.body).map_err(|e| format!("{out}: {e}"))?;
            println!("{out}");
        }
        Cmd::Press { buttons, hold_ms } => {
            api(&c, "POST", "/api/press", Some(json!({"buttons": buttons, "hold_ms": hold_ms}))).await?;
            println!("ok");
        }
        Cmd::Doctor { probe } => {
            let r = api(&c, "POST", "/api/doctor", Some(json!({"probe": probe}))).await?.json().map_err(|e| e.to_string())?;
            for ch in r["checks"].as_array().into_iter().flatten() {
                let s = ch["status"].as_str().unwrap_or("");
                let mark = match s { "ok" => "ok  ", "warn" => "WARN", "fail" => "FAIL", _ => "skip" };
                println!("{mark}  {:<26} {}", ch["name"].as_str().unwrap_or(""), ch["detail"].as_str().unwrap_or(""));
                if matches!(s, "warn" | "fail") && !ch["hint"].as_str().unwrap_or("").is_empty() {
                    println!("      -> {}", ch["hint"].as_str().unwrap_or(""));
                }
            }
            let overall = r["overall"].as_str().unwrap_or("?");
            println!("overall: {}", overall.to_uppercase());
            return Ok(if overall == "fail" { ExitCode::FAILURE } else { ExitCode::SUCCESS });
        }
        Cmd::Test { files, keep_going } => {
            let mut failed = 0;
            for f in files {
                let test: Value = serde_json::from_str(&std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?)
                    .map_err(|e| format!("{f}: {e}"))?;
                let r = api(&c, "POST", "/api/test", Some(json!({"test": test, "stop_on_fail": !keep_going}))).await?.json().map_err(|e| e.to_string())?;
                let ok = r["passed"] == true;
                failed += (!ok) as u32;
                println!("{}  {}  ({} s)  {}", if ok { "PASS" } else { "FAIL" }, r["name"].as_str().unwrap_or(""), r["duration_s"], r["dir"].as_str().unwrap_or(""));
                for s in r["steps"].as_array().into_iter().flatten().filter(|s| s["ok"] != true) {
                    println!("      step {} {}: {}", s["n"], s["step"], s["detail"].as_str().unwrap_or(""));
                }
            }
            return Ok(if failed > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS });
        }
        Cmd::Devices => {
            let d = ns2bench_core::video::list_devices().await;
            for dev in d.as_array().into_iter().flatten() {
                let formats: Vec<String> = dev["formats"].as_array().into_iter().flatten().take(6)
                    .map(|f| format!("{}x{}@{}", f["width"], f["height"], f["max_fps"].as_f64().map_or("?".into(), |x| format!("{x:.0}")))).collect();
                println!("{}  {}{}\n    {}", dev["id"].as_str().unwrap_or("-"), dev["name"].as_str().unwrap_or("?"),
                    if dev["in_use"] == true { "  (in use)" } else { "" }, if formats.is_empty() { "-".into() } else { formats.join("  ") });
            }
        }
        Cmd::BoardInstall { host, target } => {
            let root = std::env::current_dir().map_err(|e| e.to_string())?;
            let script = root.join("board/install.sh");
            if !script.exists() {
                return Err("run this from the NS2Bench source tree (board/install.sh not found)".into());
            }
            let ok = std::process::Command::new("make").args(["board", &format!("BOARD_TARGET={target}")]).status().map_err(|e| e.to_string())?;
            if !ok.success() { return Err("board build failed".into()); }
            let token = cfg.input["token_file"].as_str().map(|p| ns2bench_core::config::expand(p).to_string_lossy().to_string())
                .unwrap_or_else(|| ns2bench_core::config::state_dir().join("hidd.token").to_string_lossy().to_string());
            let bin = format!("native/target/{target}/release/ns2bench-board");
            let ok = std::process::Command::new("sh").args([script.to_str().unwrap(), &host, &bin, &token]).status().map_err(|e| e.to_string())?;
            if !ok.success() { return Err("install failed".into()); }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn chrono_stamp() -> String {
    ns2bench_core::logs::stamp()
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("NS2Bench: {e}");
            ExitCode::FAILURE
        }
    }
}
