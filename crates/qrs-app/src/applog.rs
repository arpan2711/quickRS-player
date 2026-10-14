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
}

pub fn write(msg: &str) {
    let Some(path) = PATH.get() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "{now}  {msg}");
    }
}
