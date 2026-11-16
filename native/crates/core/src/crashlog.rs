//! When NS2Bench stops unexpectedly, leave a file behind that says why: the error, where in the
//! code, a backtrace, the version and the system. ~/.ns2bench/logs/crash-<time>.txt. A window
//! app on Windows has no console, so without this a crash at launch leaves nothing to report.

use std::io::Write;
use std::path::PathBuf;

fn dir() -> PathBuf {
    crate::config::state_dir().join("logs")
}

/// Write `text` to a new crash file and return its path.
pub fn write(program: &str, text: &str) -> Option<PathBuf> {
    let d = dir();
    std::fs::create_dir_all(&d).ok()?;
    // newest 10 kept
    let mut old: Vec<PathBuf> = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with("crash-"))).collect();
    old.sort();
    for p in old.iter().rev().skip(9) { let _ = std::fs::remove_file(p); }
    let path = d.join(format!("crash-{}-{}-{program}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"), std::process::id()));
    let mut f = std::fs::File::create(&path).ok()?;
    let _ = writeln!(f, "NS2Bench {} ({program}) on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    let _ = writeln!(f, "time: {}", chrono::Local::now().to_rfc3339());
    let _ = writeln!(f, "{text}");
    Some(path)
}

/// From now on a panic anywhere also writes a crash file; `then` runs after it (for example to
/// tell the person where the file is). The default message still goes to the terminal.
pub fn install(program: &'static str, then: fn(&std::path::Path)) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        // A panic on the main thread ends the program: that's a crash. Elsewhere (a request, a
        // background task) NS2Bench carries on, so it's only noted in the log, without a message.
        if thread == "main" {
            let text = format!("thread: {thread}\n{info}\n\nbacktrace:\n{}", std::backtrace::Backtrace::force_capture());
            if let Some(p) = write(program, &text) {
                then(&p);
            }
        } else {
            note(&format!("{} panic in thread {thread} (NS2Bench carried on): {info}", chrono::Local::now().to_rfc3339()));
        }
        default(info);
    }));
}

/// One line in logs/panics.log (kept under 256 KB).
fn note(line: &str) {
    let p = dir().join("panics.log");
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 256 << 10) {
        let _ = std::fs::rename(&p, dir().join("panics-old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
    }
}

/// The newest crash files (at most `n`), newest first, for the bug report.
pub fn recent(n: usize) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir()).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with("crash-") && f.ends_with(".txt")))
        .collect();
    files.sort();
    files.into_iter().rev().take(n)
        .filter_map(|p| Some((p.file_name()?.to_str()?.to_string(), std::fs::read(&p).ok()?.into_iter().take(256 * 1024).collect())))
        .collect()
}
