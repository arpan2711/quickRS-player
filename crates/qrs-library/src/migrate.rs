//! The first start: bring over the Python app's (../rocksmith-player) settings, scores, keys and cached covers,
//! so quickRS starts where it left off (P8.2). Its files keep their shapes (D6), so they're copied as they are,
//! into an empty data folder only (nothing of quickRS's is ever overwritten). The app turns the Python sound
//! device settings into its own `audio` choice (it knows the devices).

use std::path::{Path, PathBuf};

use crate::paths::DataDir;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub from: PathBuf,
    /// What came over: "settings", "scores", "keys".
    pub copied: Vec<&'static str>,
    pub covers: usize,
}

impl Report {
    /// "your settings, scores, keys and 601 covers"
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = self.copied.iter().map(|s| s.to_string()).collect();
        if self.covers > 0 {
            parts.push(format!(
                "{} cover{}",
                self.covers,
                if self.covers == 1 { "" } else { "s" }
            ));
        }
        match parts.len() {
            0 => String::new(),
            1 => format!("your {}", parts[0]),
            n => format!("your {} and {}", parts[..n - 1].join(", "), parts[n - 1]),
        }
    }
}

/// The Python app's folder: RSPLAY_DATA, else a `rocksmith-player` folder beside the program or beside one of the
/// folders above it (the repo layout, or the zip unpacked next to it).
pub fn find_python_app(exe_dir: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(d) = std::env::var_os("RSPLAY_DATA") {
        candidates.push(PathBuf::from(d));
    }
    if let Some(start) = exe_dir {
        let mut d = start.to_path_buf();
        for _ in 0..6 {
            candidates.push(d.join("rocksmith-player"));
            if !d.pop() {
                break;
            }
        }
    }
    candidates.into_iter().find(|d| looks_like_python_app(d))
}

/// A folder with the Python app's data (and the app or its cache, so any folder with a config.json won't do).
fn looks_like_python_app(d: &Path) -> bool {
    (d.join("config.json").is_file() || d.join("userdata.json").is_file())
        && (d.join("rsplay").is_dir() || d.join(".rsplay-cache").is_dir())
}

/// Whether `to` is a fresh data folder (no settings or scores of quickRS's yet).
pub fn is_fresh(to: &DataDir) -> bool {
    !to.config().exists() && !to.userdata().exists()
}

/// Copy the Python app's files from `from` into `to`; files already in `to` are left alone.
pub fn migrate(from: &Path, to: &DataDir) -> std::io::Result<Report> {
    let mut report = Report {
        from: from.to_path_buf(),
        ..Default::default()
    };
    for (what, src, dst) in [
        ("settings", from.join("config.json"), to.config()),
        ("scores", from.join("userdata.json"), to.userdata()),
        ("keys", from.join("keys.json"), to.keys()),
    ] {
        if src.is_file() && !dst.exists() {
            if let Some(dir) = dst.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::copy(&src, &dst)?;
            report.copied.push(what);
        }
    }
    // the covers the Python app extracted (same file names: the song's id)
    let art = from.join(".rsplay-cache").join("art");
    if let Ok(rd) = std::fs::read_dir(&art) {
        let dst = to.art();
        std::fs::create_dir_all(&dst)?;
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png"))
                && let Some(name) = p.file_name()
            {
                let target = dst.join(name);
                if !target.exists() && std::fs::copy(&p, &target).is_ok() {
                    report.covers += 1;
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs-migrate-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_and_copies_the_python_app() {
        let root = tmp("root");
        let py = root.join("rocksmith-player");
        std::fs::create_dir_all(py.join("rsplay")).unwrap();
        std::fs::create_dir_all(py.join(".rsplay-cache/art")).unwrap();
        std::fs::write(
            py.join("config.json"),
            r#"{"theme": "daylight", "library_dirs": ["D:/songs"]}"#,
        )
        .unwrap();
        std::fs::write(py.join("userdata.json"), r#"{"favorites": ["a"]}"#).unwrap();
        std::fs::write(py.join("keys.json"), "{}").unwrap();
        std::fs::write(py.join(".rsplay-cache/art/song_a.png"), b"png").unwrap();
        std::fs::write(py.join(".rsplay-cache/art/notes.txt"), b"x").unwrap();
        // the program three folders down, like target/release in the repo next to it
        let exe = root.join("quickRS-rust/target/release");
        std::fs::create_dir_all(&exe).unwrap();
        assert_eq!(find_python_app(Some(&exe)), Some(py.clone()));
        let to = DataDir::at(root.join("data"));
        assert!(is_fresh(&to));
        let r = migrate(&py, &to).unwrap();
        assert_eq!(r.copied, vec!["settings", "scores", "keys"]);
        assert_eq!(r.covers, 1);
        assert_eq!(r.summary(), "your settings, scores, keys and 1 cover");
        assert!(to.art().join("song_a.png").is_file());
        assert!(std::fs::read_to_string(to.config()).unwrap().contains("daylight"));
        assert!(!is_fresh(&to));
        // a second run changes nothing
        std::fs::write(to.config(), "{}").unwrap();
        let again = migrate(&py, &to).unwrap();
        assert!(again.copied.is_empty() && again.covers == 0);
        assert_eq!(std::fs::read_to_string(to.config()).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_plain_folder_with_a_config_is_not_the_python_app() {
        let root = tmp("plain");
        let other = root.join("rocksmith-player");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("config.json"), "{}").unwrap();
        assert_eq!(find_python_app(Some(&root.join("x"))), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
