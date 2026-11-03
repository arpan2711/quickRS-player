//! Dev builds swap themselves in (D67, owner request; new in quickRS): the owner keeps playing while the code changes.
//!
//! `tools/dev-build.ps1` builds the release exe while this one runs from it: it renames the running exe (and cargo's
//! hard link to it in `deps\`) to `.old`, which Windows allows, and builds a new one in its place. This app watches
//! the path it was started from; once a different file has been there, unchanged, for `STEADY`, it writes where you
//! were (the song open, its part, the time in it, whether it was playing, the window) to `resume.json` in the cache,
//! starts the new exe with `--after-update <pid> --resume <file>` and closes. The new one waits for this one to have
//! gone (the sound devices free), opens in the same place, opens the song and seeks to where it was. A song playing
//! or a recording running holds the swap until it stops (pause: Space), with a notice. The next start deletes the
//! `.old` files (`update::cleanup`, from the list the script adds them to).
//!
//! On for an exe in a cargo `target` folder; `QUICKRS_DEV_SWAP=0` turns it off, `=1` on anywhere.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

/// A new build counts once it has been the same for this long (the linker done writing it).
const STEADY: Duration = Duration::from_secs(2);
/// How often the exe's path is looked at.
const EVERY: Duration = Duration::from_secs(1);

/// What a file at the exe's path is: when it was written and how big it is.
type Stamp = (SystemTime, u64);

fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// Whether an exe at `exe` swaps in new builds: one in a cargo `target` folder, unless `env` (QUICKRS_DEV_SWAP) says.
pub fn enabled(exe: &Path, env: Option<&str>) -> bool {
    match env {
        Some("0") => false,
        Some("1") => true,
        _ => exe
            .ancestors()
            .any(|a| a.file_name().is_some_and(|n| n.eq_ignore_ascii_case("target"))),
    }
}

pub struct DevSwap {
    exe: PathBuf,
    /// The file at the exe's path when the app started.
    started: Option<Stamp>,
    /// A different file seen there, and since when it has looked the same.
    seen: Option<(Stamp, Instant)>,
    next: Instant,
}

impl DevSwap {
    /// For this app, if it swaps in new builds.
    pub fn this() -> Option<DevSwap> {
        let exe = std::env::current_exe().ok()?;
        let env = std::env::var("QUICKRS_DEV_SWAP").ok();
        enabled(&exe, env.as_deref()).then(|| DevSwap::watching(exe))
    }

    pub fn watching(exe: PathBuf) -> DevSwap {
        DevSwap {
            started: stamp(&exe),
            exe,
            seen: None,
            next: Instant::now(),
        }
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Every frame: true once a new build has been at the exe's path, unchanged, for `STEADY`.
    pub fn poll(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next {
            return self.seen.is_some_and(|(_, t)| now.duration_since(t) >= STEADY);
        }
        self.next = now + EVERY;
        self.check(stamp(&self.exe), now)
    }

    fn check(&mut self, cur: Option<Stamp>, now: Instant) -> bool {
        let Some(cur) = cur.filter(|&c| Some(c) != self.started) else {
            self.seen = None; // (none there, mid-build, or the same file)
            return false;
        };
        match self.seen {
            Some((s, t)) if s == cur => now.duration_since(t) >= STEADY,
            _ => {
                self.seen = Some((cur, now));
                false
            }
        }
    }
}

/// Where you were, for the new build to open at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Resume {
    pub song: Option<ResumeSong>,
    /// The window (the system's points): left, top, inner width, inner height; and maximized / full screen.
    pub window: Option<[f32; 4]>,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResumeSong {
    /// The song file, and the part's key.
    pub path: String,
    pub part: String,
    /// Song seconds, and whether it was playing.
    pub at: f64,
    pub playing: bool,
}

pub fn resume_file(cache: &Path) -> PathBuf {
    cache.join("resume.json")
}

pub fn write_resume(file: &Path, r: &Resume) -> std::io::Result<()> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(file, serde_json::to_string_pretty(r).unwrap_or_default())
}

/// The resume file's contents (None if it isn't there or isn't one); it's deleted once read.
pub fn take_resume(file: &Path) -> Option<Resume> {
    let r = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let _ = std::fs::remove_file(file);
    r
}

/// Start the new build at `exe`; it waits for this app to close, then opens where `resume` says.
pub fn relaunch(exe: &Path, resume: &Path) -> std::io::Result<()> {
    std::process::Command::new(exe)
        .arg("--after-update")
        .arg(std::process::id().to_string())
        .arg("--resume")
        .arg(resume)
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_for_a_cargo_build_only() {
        // (forward slashes: separators on every OS, backslashes only on Windows)
        let dev = Path::new("C:/code/quickRS-rust/target/release/quickrs.exe");
        let installed = Path::new("C:/Users/me/quickRS-0.3.0/quickrs.exe");
        assert!(enabled(dev, None));
        assert!(!enabled(installed, None));
        assert!(!enabled(dev, Some("0")));
        assert!(enabled(installed, Some("1")));
    }

    #[test]
    fn a_new_build_counts_once_it_stops_changing() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut d = DevSwap {
            exe: PathBuf::new(),
            started: Some((t0, 100)),
            seen: None,
            next: Instant::now(),
        };
        let now = Instant::now();
        let s = |secs: u64, len: u64| Some((t0 + Duration::from_secs(secs), len));
        assert!(!d.check(Some((t0, 100)), now), "the same file");
        assert!(!d.check(None, now), "renamed away, nothing built yet");
        assert!(!d.check(s(5, 10), now), "being written");
        assert!(!d.check(s(6, 50), now + Duration::from_secs(1)), "still growing");
        assert!(!d.check(s(6, 50), now + Duration::from_secs(2)));
        assert!(d.check(s(6, 50), now + Duration::from_secs(3)), "steady for 2 s");
    }

    #[test]
    fn resume_goes_through_its_file_once() {
        let dir = std::env::temp_dir().join(format!("qrs-devswap-{}", std::process::id()));
        let f = resume_file(&dir);
        let r = Resume {
            song: Some(ResumeSong {
                path: "x.psarc".into(),
                part: "lead".into(),
                at: 61.5,
                playing: false,
            }),
            window: Some([10.0, 20.0, 1400.0, 860.0]),
            maximized: false,
            fullscreen: true,
        };
        write_resume(&f, &r).unwrap();
        assert_eq!(take_resume(&f), Some(r));
        assert_eq!(take_resume(&f), None, "read once");
        let _ = std::fs::remove_dir_all(dir);
    }
}
