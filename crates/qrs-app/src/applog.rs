//! quickrs.log in the cache folder: errors worth keeping (a song that won't open, a crash), as the Python app
//! kept .rsplay-cache/rsplay.log. The release exe has no console, so a panic's message and where it happened
//! land here too.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static PATH: OnceLock<PathBuf> = OnceLock::new();
static START: OnceLock<std::time::Instant> = OnceLock::new();

/// When the program started (call first thing in main).
pub fn mark_start() {
    let _ = START.set(std::time::Instant::now());
}

/// Milliseconds since the program started.
pub fn since_start_ms() -> u128 {
    START.get().map(|s| s.elapsed().as_millis()).unwrap_or(0)
}

/// Log to `<cache>/quickrs.log` from now on, panics included.
pub fn init(cache: PathBuf) {
    // (once: main starts it, and AppState again)
    if PATH.get().is_some() {
        return;
    }
    let path = cache.join("quickrs.log");
    let _ = std::fs::create_dir_all(&cache);
    // keep it small: start afresh when it has grown past 1 MB
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::remove_file(&path);
    }
    let _ = PATH.set(path);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        write(&format!("panic: {info}\n{bt}"));
        default(info);
    }));
    write(&format!(
        "quickRS {} started on {}",
        env!("CARGO_PKG_VERSION"),
        system()
    ));
}

/// The system, for a tester's report (P28.3): the OS and, on Linux, the distro, the desktop and Wayland or X11.
fn system() -> String {
    let mut s = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    if cfg!(target_os = "linux") {
        let distro = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|v| v.trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "unknown distro".into());
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| "?".into());
        s += &format!(
            " ({distro}; desktop {}, session {})",
            var("XDG_CURRENT_DESKTOP"),
            var("XDG_SESSION_TYPE")
        );
    }
    s
}

/// Where the log is (none before `init`).
pub fn path() -> Option<PathBuf> {
    PATH.get().cloned()
}

pub fn write(msg: &str) {
    let Some(path) = PATH.get() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "{now}  {msg}");
    }
}
