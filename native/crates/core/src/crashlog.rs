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
    #[cfg(windows)]
    native::install(program);
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

/// A window app on Windows has no stderr: Rust's last words before an abort ("thread 'main' has
/// overflowed its stack", "memory allocation of N bytes failed") were lost. Send them to
/// logs/stderr-<program>.txt (the previous run's kept as stderr-<program>-previous.txt).
/// Only when there is no console: a terminal keeps its output.
pub fn capture_stderr(program: &str) {
    #[cfg(windows)]
    native::capture_stderr(program);
    #[cfg(not(windows))]
    let _ = program;
}

#[cfg(windows)]
mod native {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS};

    static PROGRAM: OnceLock<&'static str> = OnceLock::new();

    fn name(code: u32) -> &'static str {
        match code {
            0xC00000FD => "stack overflow",
            0xC0000005 => "access violation",
            0xC0000409 => "fast fail / stack buffer overrun (also a Rust abort)",
            0xC0000374 => "heap corruption",
            0xC000001D => "illegal instruction",
            0xC0000094 => "integer division by zero",
            0x80000003 => "breakpoint",
            _ => "unknown",
        }
    }

    /// A crash that is not a Rust panic (stack overflow, abort, a fault in a system library)
    /// ends the process without the panic hook: leave a crash file for these too.
    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes valid pointers (checked for null anyway).
        let (code, addr) = unsafe {
            match info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) {
                Some(r) => (r.ExceptionCode as u32, r.ExceptionAddress as usize),
                None => (0, 0),
            }
        };
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let module = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        let text = format!("native crash (not a Rust panic)\nexception: 0x{code:08X} ({})\naddress: 0x{addr:X}\nthread: {thread}\nexe: {module}\n\nSee logs/stderr-{}.txt for Rust's own message, if any.",
            name(code), PROGRAM.get().copied().unwrap_or("app"));
        super::write(PROGRAM.get().copied().unwrap_or("app"), &text);
        EXCEPTION_CONTINUE_SEARCH
    }

    pub fn install(program: &'static str) {
        let _ = PROGRAM.set(program);
        // SAFETY: registers a function with the right signature; it lives for the program.
        unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    }

    pub fn capture_stderr(program: &str) {
        use std::os::windows::io::IntoRawHandle;
        use windows_sys::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE};
        // SAFETY: plain handle queries; the file handle is leaked on purpose (stderr lives on).
        unsafe {
            let h = GetStdHandle(STD_ERROR_HANDLE);
            if !h.is_null() && h != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                return;
            }
            let d = super::dir();
            if std::fs::create_dir_all(&d).is_err() { return }
            let path = d.join(format!("stderr-{program}.txt"));
            let _ = std::fs::rename(&path, d.join(format!("stderr-{program}-previous.txt")));
            if let Ok(f) = std::fs::File::create(&path) {
                SetStdHandle(STD_ERROR_HANDLE, f.into_raw_handle() as _);
            }
        }
    }
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
