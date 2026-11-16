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
    let path = d.join(format!("crash-{}-{program}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S")));
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
        let text = format!("thread: {thread}\n{info}\n\nbacktrace:\n{}", std::backtrace::Backtrace::force_capture());
        if let Some(p) = write(program, &text) {
            then(&p);
        }
        default(info);
    }));
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
