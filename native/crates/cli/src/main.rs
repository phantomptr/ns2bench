//! `ns2bench`: the engine, the MCP server, and a command line for everything the HTTP
//! API and MCP tools can do (see, drive, navigate, test, record, configure, debug).
//! Commands other than `serve`, `mcp`, `devices` and `board-install` talk to a running
//! engine (the app or `ns2bench serve`), so whoever watches the UI sees every action.
use clap::{Parser, Subcommand, ValueEnum};
use ns2bench_core::client::{query, Client};
use serde_json::{json, Value};
use std::io::Read;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "ns2bench", version, about = "See, hear and drive a Nintendo Switch 2 by hand or by AI agents",
    after_help = "Examples:\n  ns2bench status\n  ns2bench press A --hold-ms 120\n  ns2bench navigate system_settings\n  ns2bench read --region 0,0,0.5,0.2\n  ns2bench wait-text \"Airplane Mode\" --timeout 10\n  ns2bench test examples/tests/settings-smoke.json\n  ns2bench logs --follow --source bench")]
struct Cli {
    /// config JSON (default: ./bench.json or ~/.ns2bench/config.json)
    #[arg(long, global = true)]
    config: Option<String>,
    /// engine address, e.g. http://127.0.0.1:7630 (default: from the config)
    #[arg(long, global = true, env = "NS2BENCH_URL")]
    url: Option<String>,
    /// API token (default: from the config)
    #[arg(long, global = true, env = "NS2BENCH_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// print raw JSON instead of a summary
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Stick { Left, Right }
#[derive(Clone, Copy, ValueEnum)]
enum Mode { Standard, Pro }
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum OnOff { On, Off }
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum PowerAction { Wake, Sleep, Button }

#[derive(Subcommand)]
enum Cmd {
    // ------------------------------------------------------------ run
    /// Start the engine and UI (open http://127.0.0.1:7630)
    Serve {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// start without capture, input or sound (when a device makes NS2Bench crash)
        #[arg(long)]
        safe: bool,
    },
    /// Save a diagnostic report (zip) to send with a bug report. Uses the running engine if
    /// there is one, else what's on disk (works when the app can't open)
    Report {
        /// folder or file to save to (default: Downloads)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// MCP server on stdio for Claude Code, Codex, opencode (talks to the running engine)
    Mcp,

    // ------------------------------------------------------------ see
    /// Engine, video, Bridge and USB status
    Status,
    /// Save a screenshot
    #[command(alias = "shot")]
    Screenshot {
        #[arg(short, long)]
        output: Option<String>,
        #[arg(long)]
        png: bool,
        #[arg(long)]
        max_width: Option<u32>,
        /// crop to x,y,w,h as fractions of the screen, e.g. 0,0.8,1,0.2
        #[arg(long)]
        region: Option<String>,
    },
    /// Read the text on screen (OCR); --region x,y,w,h as fractions of the screen
    Read {
        #[arg(long)]
        region: Option<String>,
        /// faster, less accurate
        #[arg(long)]
        fast: bool,
    },
    /// Find text on screen and print where each match is (box x,y,w,h as fractions)
    Find {
        text: String,
        #[arg(long)]
        regex: bool,
        #[arg(long)]
        region: Option<String>,
        #[arg(long)]
        fast: bool,
    },
    /// Wait until the screen changes (default) or stops moving (--stable)
    WaitScreen {
        /// wait for the picture to settle instead of to change
        #[arg(long)]
        stable: bool,
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        /// with --stable: how long it must stay still
        #[arg(long, default_value_t = 500)]
        stable_ms: u64,
        #[arg(long)]
        region: Option<String>,
    },
    /// Wait, then save a screenshot (e.g. after a loading screen)
    Wait {
        /// milliseconds
        ms: u64,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Ready to drive? Picture, input device and agent permission, with what to fix
    Health,
    /// Wait until text appears (or with --gone, disappears)
    WaitText {
        text: String,
        /// treat TEXT as a regular expression
        #[arg(long)]
        regex: bool,
        #[arg(long)]
        gone: bool,
        /// seconds
        #[arg(long, default_value_t = 10.0)]
        timeout: f64,
        #[arg(long)]
        region: Option<String>,
        /// faster, less accurate reading
        #[arg(long)]
        fast: bool,
    },
    /// Which known screen is showing
    Where,

    // ------------------------------------------------------------ drive
    /// Press buttons: A B X Y L R ZL ZR PLUS MINUS HOME CAPTURE LSTICK RSTICK UP DOWN LEFT RIGHT (+ C GL GR in Pro mode)
    Press {
        #[arg(required = true)]
        buttons: Vec<String>,
        #[arg(long, default_value_t = 100)]
        hold_ms: u64,
    },
    /// Hold buttons down until `release`
    Hold {
        #[arg(required = true)]
        buttons: Vec<String>,
    },
    /// Release held buttons (all if none given)
    Release { buttons: Vec<String> },
    /// Tilt a stick: x and y from -1 to 1 (up is y = -1)
    Stick {
        #[arg(value_enum)]
        stick: Stick,
        #[arg(allow_hyphen_values = true)]
        x: f64,
        #[arg(allow_hyphen_values = true)]
        y: f64,
        /// hold then return to center (omit to leave it tilted)
        #[arg(long)]
        hold_ms: Option<u64>,
    },
    /// Press a keyboard key on the console, e.g. `key Enter --mod ctrl`
    Key {
        key: String,
        #[arg(long = "mod")]
        modifiers: Vec<String>,
        #[arg(long, default_value_t = 60)]
        hold_ms: u64,
    },
    /// Type text with the console keyboard
    Type {
        text: String,
        #[arg(long, default_value_t = 40)]
        delay_ms: u64,
    },
    /// Move the mouse by dx, dy (and scroll)
    Mouse {
        #[arg(allow_hyphen_values = true)]
        dx: i64,
        #[arg(allow_hyphen_values = true)]
        dy: i64,
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        wheel: i64,
        /// keep buttons down while moving (drag): 1 left, 2 right, 4 middle
        #[arg(long)]
        buttons: Option<u8>,
    },
    /// Click a mouse button
    Click {
        #[arg(default_value = "left")]
        button: String,
        #[arg(long, default_value_t = 60)]
        hold_ms: u64,
    },
    /// Run several steps in one call from a JSON file (or - for stdin): [{"press":["A"]},{"wait_ms":300}]
    Sequence { file: String },
    /// Go to a known screen and verify it, e.g. `navigate home`
    Navigate { route: String },
    /// List known routes and screens
    Routes,

    // ------------------------------------------------------------ test and record
    /// Run test files; reports go to ~/.ns2bench/reports
    Test {
        #[arg(required = true)]
        files: Vec<String>,
        /// run every step even after a failure
        #[arg(long)]
        keep_going: bool,
    },
    /// Press and watch the screen: did it register, how fast
    Probe {
        #[arg(required = true)]
        buttons: Vec<String>,
        #[arg(long, default_value_t = 100)]
        hold_ms: u64,
        #[arg(long, default_value_t = 1500)]
        timeout_ms: u64,
    },
    /// Measure the shortest reliable press (HOME menu)
    Calibrate,
    /// Save the last N seconds of video as an MP4
    Clip {
        #[arg(long)]
        seconds: Option<f64>,
        #[arg(long, default_value = "cli")]
        label: String,
    },
    /// List saved clips, or download one: `clips NAME -o file.mp4`
    Clips {
        name: Option<String>,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// List saved test reports
    Reports,

    // ------------------------------------------------------------ configure
    /// Console-facing USB device of a Linux board: standard (controller + keyboard + mouse) or pro (Pro Controller)
    UsbMode {
        #[arg(value_enum)]
        mode: Mode,
    },
    /// Block agent input (only the NS2Bench window can turn it off again)
    ManualOnly {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Show or change capture settings
    Video {
        #[arg(long)]
        device: Option<String>,
        /// WIDTHxHEIGHT, e.g. 1920x1080
        #[arg(long)]
        size: Option<String>,
        #[arg(long)]
        fps: Option<u32>,
        #[arg(long)]
        quality: Option<u32>,
    },
    /// Show or change the input device connection
    Input {
        /// none | hidd | serial | ch9329
        #[arg(long)]
        backend: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        serial_port: Option<String>,
        #[arg(long)]
        baud: Option<u32>,
        #[arg(long)]
        token_file: Option<String>,
    },
    /// Show or change the offline console network services (internet blocking always stays on)
    Network {
        /// direct | router | none
        #[arg(long)]
        topology: Option<String>,
        #[arg(long, value_enum)]
        dhcp: Option<OnOff>,
        #[arg(long, value_enum)]
        dns: Option<OnOff>,
        #[arg(long, value_enum)]
        connectivity: Option<OnOff>,
    },
    /// List the capture devices the engine sees (--local: this computer, without an engine)
    Devices {
        #[arg(long)]
        local: bool,
    },
    /// Wake the console, put it to sleep (HDMI-CEC) or press its power button (a presser you set up)
    Power {
        #[arg(value_enum, default_value = "wake")]
        action: PowerAction,
    },
    /// Turn the screen log on or off (new on-screen text becomes log lines)
    ScreenLog {
        #[arg(value_enum)]
        state: OnOff,
    },

    // ------------------------------------------------------------ debug
    /// Check capture, Bridge, USB, console and network end to end
    Doctor {
        /// also press RIGHT/LEFT (HOME menu only) to prove input works
        #[arg(long)]
        probe: bool,
    },
    /// Show log lines (bench, board, kernel, network)
    Logs {
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        contains: Option<String>,
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: u32,
        /// only this level and above: debug, info, warn, error
        #[arg(short, long, value_parser = ["debug", "info", "warn", "error"])]
        level: Option<String>,
        /// keep printing new lines
        #[arg(short, long)]
        follow: bool,
    },
    /// Recent inputs from you and agents
    Events {
        /// only ui, api or mcp
        #[arg(long)]
        source: Option<String>,
        #[arg(short = 'n', long, default_value_t = 30)]
        lines: u32,
    },
    /// Markdown summary for bug reports
    Diagnostics {
        /// log lines to include
        #[arg(long, default_value_t = 40)]
        lines: u32,
    },
    /// Save a bug-report zip (logs, status, screenshot; secrets removed)
    Bundle {
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Build + install the NS2Bench Bridge daemon, scripts and token over SSH (needs the source tree)
    BoardInstall {
        /// SSH host of the Linux USB-gadget board
        host: String,
        #[arg(long, default_value = "armv7-unknown-linux-musleabihf")]
        target: String,
    },
}

type Res<T> = Result<T, String>;

async fn api(c: &Client, method: &str, path: &str, body: Option<Value>) -> Res<ns2bench_core::client::Resp> {
    c.call(method, path, body.as_ref()).await.map_err(|e| e.to_string())
}
async fn get(c: &Client, path: &str) -> Res<Value> {
    api(c, "GET", path, None).await?.json().map_err(|e| e.to_string())
}
async fn post(c: &Client, path: &str, body: Value) -> Res<Value> {
    api(c, "POST", path, Some(body)).await?.json().map_err(|e| e.to_string())
}
fn pretty(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}
fn on(v: Option<OnOff>) -> Option<bool> {
    v.map(|x| x == OnOff::On)
}
fn save(path: Option<String>, default: String, bytes: &[u8]) -> Res<()> {
    let out = path.unwrap_or(default);
    std::fs::write(&out, bytes).map_err(|e| format!("{out}: {e}"))?;
    println!("{out}");
    Ok(())
}

async fn run(cli: Cli) -> Res<ExitCode> {
    let mut cfg = ns2bench_core::Config::load(cli.config.as_deref()).map_err(|e| e.to_string())?;
    let mut c = Client::from_config(&cfg, "api");
    if let Some(u) = cli.url {
        if !u.starts_with("http://") {
            return Err("--url must be an http:// address of an NS2Bench engine, e.g. http://127.0.0.1:7630".into());
        }
        c.base = u.trim_end_matches('/').to_string();
    }
    if let Some(t) = cli.token { c.token = Some(t); }
    let raw = cli.json;
    let ok = |v: &Value| if raw { pretty(v) } else { println!("ok") };
    match cli.cmd {
        Cmd::Serve { host, port, safe } => {
            if safe {
                // SAFETY: set before the engine starts its threads.
                unsafe { std::env::set_var("NS2BENCH_SAFE", "1") };
                cfg = ns2bench_core::Config::from_raw(cfg.path.clone(), cfg.raw.clone());
            }
            if let Some(prev) = ns2bench_core::report::start_trace("serve") {
                eprintln!("note: the last `ns2bench serve` ended unexpectedly (its startup trace is in the report)");
                let _ = prev;
            }
            if let Some(h) = host { cfg.server["host"] = h.into(); }
            if let Some(p) = port { cfg.server["port"] = p.into(); }
            ns2bench_core::serve(cfg).await.map_err(|e| e.to_string())?;
        }
        Cmd::Mcp => ns2bench_core::mcp::run(cfg).await.map_err(|e| e.to_string())?,
        Cmd::Report { output } => {
            let target = output.map(std::path::PathBuf::from).unwrap_or_else(ns2bench_core::report::downloads_dir);
            // The running engine's report has live status, the check and a screenshot too.
            let path = match api(&c, "GET", "/api/bundle", None).await {
                Ok(r) if r.body.starts_with(b"PK") => {
                    let p = if target.is_dir() { target.join(ns2bench_core::report::file_name()) } else { target.clone() };
                    std::fs::write(&p, &r.body).map_err(|e| format!("{}: {e}", p.display()))?;
                    p
                }
                _ => {
                    eprintln!("(no running engine: the report has what's on disk)");
                    let dir = if target.extension().is_some() { target.parent().map(std::path::Path::to_path_buf).unwrap_or_default() } else { target.clone() };
                    let p = ns2bench_core::report::save_offline(&dir).await.map_err(|e| e.to_string())?;
                    if target.extension().is_some() { std::fs::rename(&p, &target).map_err(|e| e.to_string())?; target.clone() } else { p }
                }
            };
            println!("{}", path.display());
            eprintln!("Report saved. Attach it to a GitHub issue or send it on Discord.");
        }

        Cmd::Status => {
            let v = get(&c, "/api/status").await?;
            if raw { pretty(&v) } else {
                let (vi, i) = (&v["video"], &v["input"]);
                println!("NS2Bench {}", v["version"].as_str().unwrap_or("?"));
                println!("video   {}", if vi["error"].is_null() { format!("{} fps ({})", vi["fps"], vi["source"].as_str().unwrap_or("?")) } else { vi["error"].as_str().unwrap_or("?").to_string() });
                println!("bridge  {} {}{}", i["backend"].as_str().unwrap_or("?"),
                    if i["connected"] == true { format!("connected, {} ms", i["rtt_ms"]) } else { "offline".into() },
                    i["usb"].as_str().map(|u| format!(", console usb {u}")).unwrap_or_default());
                println!("agents  {}", if i["manual_only"] == true { "blocked (manual only)" } else { "allowed" });
            }
        }
        Cmd::Screenshot { output, png, max_width, region } => {
            let mut q = vec![("format", if png { "png" } else { "jpeg" }.to_string())];
            if let Some(w) = max_width { q.push(("max_width", w.to_string())); }
            if let Some(r) = region { q.push(("region", r)); }
            let r = api(&c, "GET", &format!("/api/screenshot{}", query(&q)), None).await?;
            save(output, format!("ns2bench-{}.{}", stamp(), if png { "png" } else { "jpg" }), &r.body)?;
        }
        Cmd::Read { region, fast } => {
            let mut q: Vec<_> = region.map(|r| ("region", r)).into_iter().collect();
            q.push(("fast", if fast { "1" } else { "0" }.to_string()));
            let v = get(&c, &format!("/api/ocr{}", query(&q))).await?;
            if raw { pretty(&v) } else { println!("{}", v["text"].as_str().unwrap_or("")); }
        }
        Cmd::Find { text, regex, region, fast } => {
            let mut q = vec![(if regex { "regex" } else { "text" }, text), ("fast", fast.to_string())];
            if let Some(r) = region { q.push(("region", r)); }
            let v = get(&c, &format!("/api/find_text{}", query(&q))).await?;
            if raw { pretty(&v) } else {
                for m in v["matches"].as_array().into_iter().flatten() {
                    let b: Vec<String> = m["box"].as_array().into_iter().flatten().map(|x| format!("{:.3}", x.as_f64().unwrap_or(0.0))).collect();
                    println!("{}  @ {}", m["text"].as_str().unwrap_or(""), b.join(","));
                }
            }
            if v["found"] != true { if !raw { eprintln!("not on screen"); } return Ok(ExitCode::FAILURE); }
        }
        Cmd::WaitScreen { stable, timeout_ms, stable_ms, region } => {
            let mut b = json!({"until": if stable { "stable" } else { "change" }, "timeout_ms": timeout_ms, "stable_ms": stable_ms});
            if let Some(r) = region { b["region"] = r.into(); }
            let v = post(&c, "/api/wait_screen", b).await?;
            if raw { pretty(&v) } else if v["ok"] == true {
                println!("{} after {} ms", if stable { "still" } else { "changed" }, v["waited_ms"]);
            } else { println!("{} ({} ms)", v["message"].as_str().unwrap_or("timed out"), v["waited_ms"]); }
            if v["ok"] != true { return Ok(ExitCode::FAILURE); }
        }
        Cmd::Wait { ms, output } => {
            tokio::time::sleep(std::time::Duration::from_millis(ms.min(600_000))).await;
            let r = api(&c, "GET", "/api/screenshot", None).await?;
            save(output, format!("ns2bench-{}.jpg", stamp()), &r.body)?;
        }
        Cmd::Health => {
            let v = get(&c, "/api/health").await?;
            if raw { pretty(&v) } else {
                println!("{} · picture {} · input {} · agents {}", if v["ok"] == true { "ready" } else { "NOT READY" },
                    if v["video"] == true { "ok" } else { "missing" }, if v["input"] == true { "ok" } else { "not connected" },
                    if v["agents_allowed"] == true { "allowed" } else { "blocked" });
                for p in v["problems"].as_array().into_iter().flatten() {
                    println!("  {}: {}\n    → {}", p["area"].as_str().unwrap_or(""), p["problem"].as_str().unwrap_or(""), p["fix"].as_str().unwrap_or(""));
                }
            }
            if v["ok"] != true { return Ok(ExitCode::FAILURE); }
        }
        Cmd::WaitText { text, regex, gone, timeout, region, fast } => {
            let mut b = json!({"timeout_ms": timeout * 1000.0, "gone": gone, "fast": fast});
            b[if regex { "regex" } else { "text" }] = text.into();
            if let Some(r) = region { b["region"] = r.into(); }
            let v = post(&c, "/api/wait_text", b).await?;
            let hit = v["found"] == true || v["ok"] == true;
            let (yes, no) = if gone { ("gone", "still there") } else { ("found", "not found") };
            if raw { pretty(&v) } else { println!("{}", if hit { yes } else { no }); }
            if !hit { return Ok(ExitCode::FAILURE); }
        }
        Cmd::Where => {
            let v = get(&c, "/api/where").await?;
            if raw { pretty(&v) } else { println!("{}", v["screens"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).filter(|s| !s.is_empty()).unwrap_or_else(|| "unknown screen".into())); }
        }

        Cmd::Press { buttons, hold_ms } => ok(&post(&c, "/api/press", json!({"buttons": buttons, "hold_ms": hold_ms})).await?),
        Cmd::Hold { buttons } => ok(&post(&c, "/api/hold", json!({"buttons": buttons})).await?),
        Cmd::Release { buttons } => ok(&post(&c, "/api/release", json!({"buttons": buttons})).await?),
        Cmd::Stick { stick, x, y, hold_ms } => {
            let s = match stick { Stick::Left => "left", Stick::Right => "right" };
            ok(&post(&c, "/api/stick", json!({"stick": s, "x": x, "y": y, "hold_ms": hold_ms})).await?)
        }
        Cmd::Key { key, modifiers, hold_ms } => ok(&post(&c, "/api/key", json!({"key": key, "modifiers": modifiers, "hold_ms": hold_ms})).await?),
        Cmd::Type { text, delay_ms } => ok(&post(&c, "/api/type", json!({"text": text, "delay_ms": delay_ms})).await?),
        Cmd::Mouse { dx, dy, wheel, buttons } => ok(&post(&c, "/api/mouse", json!({"dx": dx, "dy": dy, "wheel": wheel, "buttons": buttons})).await?),
        Cmd::Click { button, hold_ms } => ok(&post(&c, "/api/click", json!({"button": button, "hold_ms": hold_ms})).await?),
        Cmd::Sequence { file } => {
            let text = if file == "-" {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
                s
            } else {
                std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?
            };
            let steps: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
            let steps = if steps.is_array() { steps } else { steps["steps"].clone() };
            ok(&post(&c, "/api/sequence", json!({"steps": steps})).await?)
        }
        Cmd::Navigate { route } => {
            let v = post(&c, "/api/navigate", json!({"route": route})).await?;
            let arrived = v["arrived"] == true;
            if raw { pretty(&v) } else { println!("{}", if arrived { "arrived" } else { "did not arrive" }); }
            if !arrived { return Ok(ExitCode::FAILURE); }
        }
        Cmd::Routes => {
            let v = get(&c, "/api/routes").await?;
            if raw { pretty(&v) } else if let Some(m) = v["routes"].as_object() {
                for (name, r) in m {
                    let about = r["note"].as_str().or(r["expect"].as_str()).unwrap_or("");
                    println!("{name:<24} {about}");
                }
            } else { pretty(&v) }
        }

        Cmd::Test { files, keep_going } => {
            let mut failed = 0;
            for f in files {
                let test: Value = serde_json::from_str(&std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?)
                    .map_err(|e| format!("{f}: {e}"))?;
                let r = post(&c, "/api/test", json!({"test": test, "stop_on_fail": !keep_going})).await?;
                let passed = r["passed"] == true;
                failed += (!passed) as u32;
                if raw { pretty(&r); continue; }
                println!("{}  {}  ({} s)  {}", if passed { "PASS" } else { "FAIL" }, r["name"].as_str().unwrap_or(""), r["duration_s"], r["dir"].as_str().unwrap_or(""));
                for s in r["steps"].as_array().into_iter().flatten().filter(|s| s["ok"] != true) {
                    println!("      step {} {}: {}", s["n"], s["step"], s["detail"].as_str().unwrap_or(""));
                }
            }
            return Ok(if failed > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS });
        }
        Cmd::Probe { buttons, hold_ms, timeout_ms } => {
            let v = post(&c, "/api/probe", json!({"buttons": buttons, "hold_ms": hold_ms, "timeout_ms": timeout_ms})).await?;
            if raw { pretty(&v) } else if v["changed"] == true { println!("the screen reacted after {} ms", v["latency_ms"]); }
            else { println!("no visible reaction (change {} vs noise {})", v["diff"], v["noise"]); }
            if v["changed"] != true { return Ok(ExitCode::FAILURE); }
        }
        Cmd::Calibrate => pretty(&post(&c, "/api/calibrate", json!({})).await?),
        Cmd::Clip { seconds, label } => {
            let v = post(&c, "/api/clip", json!({"seconds": seconds, "label": label})).await?;
            if raw { pretty(&v) } else { println!("{}", v["path"].as_str().unwrap_or("")); }
        }
        Cmd::Clips { name: Some(name), output } => {
            if name.contains(['/', '\\']) { return Err("give the clip's file name, as `clips` lists it".into()); }
            // Without -o the clip lands in this folder under its own name: never over a file already there.
            if output.is_none() && std::path::Path::new(&name).exists() {
                return Err(format!("{name} already exists here; choose another name with -o"));
            }
            let r = api(&c, "GET", &format!("/clips/{name}"), None).await?;
            save(output, name.clone(), &r.body)?;
        }
        Cmd::Reports => {
            let v = get(&c, "/api/reports").await?;
            if raw { pretty(&v) } else {
                for r in v["reports"].as_array().into_iter().flatten() {
                    println!("{} {}  {}", match r["passed"].as_bool() { Some(true) => "PASS", Some(false) => "FAIL", None => "?   " },
                        r["test"].as_str().unwrap_or(""), r["path"].as_str().unwrap_or(""));
                }
            }
        }
        Cmd::Clips { name: None, .. } => {
            let v = get(&c, "/api/clips").await?;
            if raw { pretty(&v) } else {
                for clip in v.as_array().or_else(|| v["clips"].as_array()).into_iter().flatten() {
                    println!("{}", clip["path"].as_str().or(clip["name"].as_str()).unwrap_or(""));
                }
            }
        }

        Cmd::UsbMode { mode } => {
            let m = match mode { Mode::Standard => "standard", Mode::Pro => "pro" };
            let v = post(&c, "/api/control", json!({"usb_mode": m})).await?;
            if raw { pretty(&v) } else { println!("switching to {m}: the console sees the controller reconnect (about 10 s)"); }
        }
        Cmd::ManualOnly { state } => {
            let v = post(&c, "/api/control", json!({"manual_only": state == OnOff::On})).await?;
            if raw { pretty(&v) } else { println!("agents {}", if state == OnOff::On { "blocked" } else { "allowed" }); }
        }
        Cmd::Video { device, size, fps, quality } => {
            if device.is_none() && size.is_none() && fps.is_none() && quality.is_none() {
                pretty(&get(&c, "/api/video").await?);
            } else {
                let mut b = json!({});
                if let Some(d) = device { b["device"] = d.into(); }
                if let Some(s) = size {
                    let (w, h) = s.split_once('x').ok_or("size must be WIDTHxHEIGHT")?;
                    b["width"] = w.parse::<u32>().map_err(|e| e.to_string())?.into();
                    b["height"] = h.parse::<u32>().map_err(|e| e.to_string())?.into();
                }
                if let Some(f) = fps { b["fps"] = f.into(); }
                if let Some(q) = quality { b["quality"] = q.into(); }
                ok(&post(&c, "/api/video", b).await?);
            }
        }
        Cmd::Input { backend, host, port, serial_port, baud, token_file } => {
            let cur = get(&c, "/api/input").await?;
            if backend.is_none() && host.is_none() && port.is_none() && serial_port.is_none() && baud.is_none() && token_file.is_none() {
                pretty(&cur);
                return Ok(ExitCode::SUCCESS);
            }
            // Change only what was given: keep the rest of the current settings.
            let s = &cur["settings"];
            let backend = backend.unwrap_or_else(|| s["backend"].as_str().unwrap_or("none").into());
            let mut b = json!({"backend": backend});
            for k in ["host", "port", "serial_port", "baud", "token_file"] {
                if !s[k].is_null() { b[k] = s[k].clone(); }
            }
            if let Some(h) = host { b["host"] = h.into(); }
            if let Some(p) = port { b["port"] = p.into(); }
            if let Some(s) = serial_port { b["serial_port"] = s.into(); }
            if let Some(r) = baud { b["baud"] = r.into(); }
            if let Some(t) = token_file { b["token_file"] = t.into(); }
            ok(&post(&c, "/api/input", b).await?);
        }
        Cmd::Network { topology, dhcp, dns, connectivity } => {
            let cur = get(&c, "/api/network").await?;
            if topology.is_none() && dhcp.is_none() && dns.is_none() && connectivity.is_none() {
                pretty(&cur);
                return Ok(ExitCode::SUCCESS);
            }
            let ctl = &cur["controls"];
            let topology = topology.unwrap_or_else(|| ctl["topology"].as_str().unwrap_or("direct").into());
            // With a router or no managed network, local services must be off unless asked.
            let keep = |k: &str| topology == "direct" && ctl[k] == true;
            let b = json!({
                "topology": topology,
                "dhcp": on(dhcp).unwrap_or_else(|| keep("dhcp")),
                "dns": on(dns).unwrap_or_else(|| keep("dns")),
                "connectivity": on(connectivity).unwrap_or_else(|| keep("connectivity")),
            });
            ok(&post(&c, "/api/network", b).await?);
        }
        Cmd::Power { action } => {
            let a = match action { PowerAction::Wake => "wake", PowerAction::Sleep => "sleep", PowerAction::Button => "button" };
            let v = post(&c, "/api/power", json!({"action": a})).await?;
            if raw { pretty(&v); } else {
                println!("{}", v["message"].as_str().or(v["detail"].as_str()).unwrap_or("done"));
                for s in v["steps"].as_array().into_iter().flatten() {
                    println!("  {} {}{}", if s["ok"] == true { "✓" } else { "✕" }, s["step"].as_str().unwrap_or(""),
                        s["detail"].as_str().filter(|x| !x.is_empty()).map(|x| format!(": {x}")).unwrap_or_default());
                }
            }
            if v["ok"] != true { return Ok(ExitCode::from(1)); }
        }
        Cmd::ScreenLog { state } => {
            let on = matches!(state, OnOff::On);
            let v = post(&c, "/api/control", json!({"screen_log": on})).await?;
            if raw { pretty(&v) } else { println!("screen log {}", if on { "on" } else { "off" }); }
        }
        Cmd::Devices { local } => {
            let d = if local { ns2bench_core::video::list_devices().await } else { get(&c, "/api/video").await?["devices"].clone() };
            if raw { pretty(&d); return Ok(ExitCode::SUCCESS); }
            for dev in d.as_array().into_iter().flatten() {
                let formats: Vec<String> = dev["formats"].as_array().into_iter().flatten().take(6)
                    .map(|f| format!("{}x{}@{}", f["width"], f["height"], f["max_fps"].as_f64().map_or("?".into(), |x| format!("{x:.0}")))).collect();
                println!("{}  {}{}\n    {}", dev["id"].as_str().unwrap_or("-"), dev["name"].as_str().unwrap_or("?"),
                    if dev["in_use"] == true { "  (in use)" } else { "" }, if formats.is_empty() { "-".into() } else { formats.join("  ") });
            }
        }

        Cmd::Doctor { probe } => {
            let r = post(&c, "/api/doctor", json!({"probe": probe})).await?;
            if raw { pretty(&r); } else {
                for ch in r["checks"].as_array().into_iter().flatten() {
                    let s = ch["status"].as_str().unwrap_or("");
                    let mark = match s { "ok" => "ok  ", "warn" => "WARN", "fail" => "FAIL", _ => "skip" };
                    println!("{mark}  {:<26} {}", ch["name"].as_str().unwrap_or(""), ch["detail"].as_str().unwrap_or(""));
                    if matches!(s, "warn" | "fail") && !ch["hint"].as_str().unwrap_or("").is_empty() {
                        println!("      -> {}", ch["hint"].as_str().unwrap_or(""));
                    }
                }
                println!("overall: {}", r["overall"].as_str().unwrap_or("?").to_uppercase());
            }
            return Ok(if r["overall"] == "fail" { ExitCode::FAILURE } else { ExitCode::SUCCESS });
        }
        Cmd::Logs { source, contains, lines, level, follow } => {
            let mut after = 0u64;
            let mut first = true;
            loop {
                let mut q = vec![("limit", if first { lines } else { 500 }.to_string()), ("after", after.to_string())];
                if let Some(s) = &source { q.push(("source", s.clone())); }
                if let Some(t) = &contains { q.push(("contains", t.clone())); }
                if let Some(l) = &level { q.push(("level", l.clone())); }
                let v = get(&c, &format!("/api/logs{}", query(&q))).await?;
                for e in v["entries"].as_array().into_iter().flatten() {
                    after = after.max(e["seq"].as_u64().unwrap_or(after));
                    if raw { println!("{e}"); } else {
                        println!("{} {:<5} {:<8} {}", ns2bench_core::logs::local_time(e["t"].as_f64().unwrap_or(0.0)),
                            e["level"].as_str().unwrap_or("info").to_uppercase(), e["source"].as_str().unwrap_or(""), e["text"].as_str().unwrap_or(""));
                        if let Some(h) = e["hint"].as_str() { println!("{:>24}→ {h}", ""); }
                    }
                }
                if !follow { break; }
                first = false;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        Cmd::Events { source, lines } => {
            let mut q = vec![("limit", lines.to_string())];
            if let Some(s) = source { q.push(("source", s)); }
            let v = get(&c, &format!("/api/events{}", query(&q))).await?;
            if raw { pretty(&v) } else {
                for e in v.as_array().into_iter().flatten() {
                    let mut d = e.clone();
                    if let Some(o) = d.as_object_mut() { o.remove("t"); o.remove("source"); o.remove("action"); }
                    println!("{} {:<4} {} {}", ns2bench_core::logs::local_time(e["t"].as_f64().unwrap_or(0.0)), e["source"].as_str().unwrap_or(""), e["action"].as_str().unwrap_or(""), d);
                }
            }
        }
        Cmd::Diagnostics { lines } => println!("{}", api(&c, "GET", &format!("/api/diagnostics?lines={}", lines.min(2000)), None).await?.text()),
        Cmd::Bundle { output } => {
            let r = api(&c, "GET", "/api/bundle", None).await?;
            save(output, format!("ns2bench-report-{}.zip", stamp()), &r.body)?;
        }
        Cmd::BoardInstall { host, target } => {
            let root = std::env::current_dir().map_err(|e| e.to_string())?;
            let script = root.join("board/install.sh");
            if !script.exists() {
                return Err("run this from the NS2Bench source tree (board/install.sh not found)".into());
            }
            let built = std::process::Command::new("make").args(["board", &format!("BOARD_TARGET={target}")]).status().map_err(|e| e.to_string())?;
            if !built.success() { return Err("board build failed".into()); }
            let token = cfg.input["token_file"].as_str().map(|p| ns2bench_core::config::expand(p).to_string_lossy().to_string())
                .unwrap_or_else(|| ns2bench_core::config::state_dir().join("hidd.token").to_string_lossy().to_string());
            let bin = format!("native/target/{target}/release/ns2bench-board");
            let installed = std::process::Command::new("sh").args([script.to_str().unwrap(), &host, &bin, &token]).status().map_err(|e| e.to_string())?;
            if !installed.success() { return Err("install failed".into()); }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn stamp() -> String {
    ns2bench_core::logs::stamp()
}

#[tokio::main]
async fn main() -> ExitCode {
    ns2bench_core::crashlog::install("cli", |p| eprintln!("ns2bench stopped unexpectedly. Details: {}", p.display()));
    match run(Cli::parse()).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("NS2Bench: {e}");
            ExitCode::FAILURE
        }
    }
}
