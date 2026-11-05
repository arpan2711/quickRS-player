//! The system's file dialogs (rfd: Windows' own, the Mac's, and on Linux the desktop's own through its xdg portal):
//! a folder of songs, a keys file. They block the window while open, as any native dialog does.
//!
//! Linux (P28.3): a desktop without a portal (a bare window manager, some minimal installs) shows no dialog at all, and
//! rfd answers "nothing picked" at once. A pick that ends that fast with nothing is taken as no dialog, and
//! `unavailable` says so, for the caller to say what to do instead.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// What to do when there's no file dialog (shown by the callers).
pub const NO_PICKER: &str = "No file picker on this desktop: drop the folder onto quickRS's window instead (or install \
     xdg-desktop-portal-gtk, or -kde on KDE)";

static UNAVAILABLE: AtomicBool = AtomicBool::new(false);

/// Whether the last pick found no dialog to show (Linux without an xdg portal).
pub fn unavailable() -> bool {
    UNAVAILABLE.load(Ordering::Relaxed)
}

/// Runs a pick and notes whether a dialog was there: nothing picked within a moment is no dialog (no one cancels
/// that fast). Only on Linux, where a missing portal is possible.
fn noted(pick: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    let t = Instant::now();
    let got = pick();
    let none_shown = cfg!(target_os = "linux") && got.is_none() && t.elapsed() < Duration::from_millis(300);
    UNAVAILABLE.store(none_shown, Ordering::Relaxed);
    got
}

/// A folder of songs, starting at `from` when it's a folder.
pub fn pick_folder(from: Option<&Path>) -> Option<PathBuf> {
    pick_folder_titled("Add a folder of songs", from)
}

/// A folder for something else (`title` says what), starting at `from` when it's a folder.
pub fn pick_folder_titled(title: &str, from: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(title);
    if let Some(f) = from.filter(|f| f.is_dir()) {
        d = d.set_directory(f);
    }
    noted(|| d.pick_folder())
}

/// A capture to add: a .nam (an amp or pedal) or a .wav (a cab's impulse response).
pub fn pick_capture_file() -> Option<PathBuf> {
    let d = rfd::FileDialog::new()
        .set_title("Add a .nam capture or an IR")
        .add_filter("NAM capture or IR", &["nam", "wav"])
        .add_filter("All files", &["*"]);
    noted(|| d.pick_file())
}

/// A keys file (keys.json, or Notepad's keys.json.txt).
pub fn pick_keys_file() -> Option<PathBuf> {
    let d = rfd::FileDialog::new()
        .set_title("Load a keys file")
        .add_filter("Keys file", &["json", "txt"])
        .add_filter("All files", &["*"]);
    noted(|| d.pick_file())
}
