//! RS's keys for the app (P20.2): where they're looked for, what was found (for Settings > Library to say),
//! the keys typed or pasted there, written to keys.json, and a watch on the keys files so songs that needed keys
//! are read again as soon as keys arrive (no F5).
//!
//! Looked for in order: the data folder (keys.json, then keys.json.txt and keys.json.json: Notepad's names), then
//! `qrs_formats::keys::candidate_files` with the program's folder ($RSPLAY_KEYS_FILE, the working folder, the
//! program's folder, ~/.config/rsplay). The environment's RSPLAY_*_KEY variables win over any file.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use qrs_formats::Keys;
use qrs_formats::keys::{self, parse_hex_key};
use qrs_library::SongEntry;

/// What the key lookup found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyState {
    /// Both keys (psarc songs read). `misnamed`: the file is keys.json.txt or keys.json.json.
    Found { path: Option<PathBuf>, misnamed: bool },
    /// A file was there, but without both keys.
    Partial { path: PathBuf },
    /// No keys file anywhere it looked.
    Missing,
    /// A keys file that doesn't read (bad JSON, a key of the wrong length...).
    Invalid { path: PathBuf, why: String },
}

/// The files looked at, in order.
pub fn candidates(data_root: &Path, exe_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = keys::files_in(data_root);
    for p in keys::candidate_files(None, exe_dirs) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Read the keys from the first of `files` there is (the environment's keys over it).
pub fn load(files: &[PathBuf]) -> (Keys, KeyState) {
    let Some(path) = files.iter().find(|p| p.is_file()) else {
        // (the environment's keys, if any)
        let k = env_keys().unwrap_or_default();
        let state = if k.can_read_psarc() {
            KeyState::Found {
                path: None,
                misnamed: false,
            }
        } else {
            KeyState::Missing
        };
        return (k, state);
    };
    match qrs_formats::load_keys(Some(path), &[]) {
        Ok(k) if k.can_read_psarc() => {
            let misnamed = keys::is_misnamed(path);
            (
                k,
                KeyState::Found {
                    path: Some(path.clone()),
                    misnamed,
                },
            )
        }
        Ok(k) => (k, KeyState::Partial { path: path.clone() }),
        Err(e) => (
            Keys::default(),
            KeyState::Invalid {
                path: path.clone(),
                why: e.to_string(),
            },
        ),
    }
}

/// Keys from the RSPLAY_*_KEY variables alone.
fn env_keys() -> Option<Keys> {
    let get = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .and_then(|v| parse_hex_key(&v, name).ok())
    };
    let k = Keys {
        psarc: get(keys::ENV_PSARC),
        sng: get(keys::ENV_SNG),
        sng_mac: get(keys::ENV_SNG_MAC),
        source: None,
    };
    (k.psarc.is_some() || k.sng.is_some()).then_some(k)
}

/// After the keys changed from `old` to `new`: whether songs should be read again (new keys that read psarcs, and
/// songs that couldn't be read without them).
pub fn needs_reread(old: &Keys, new: &Keys, songs: &[SongEntry]) -> bool {
    !old.same_keys(new) && new.can_read_psarc() && songs.iter().any(|s| s.needs_keys)
}

/// What's typed in one of Settings' key boxes: nothing, a key, or why it isn't one.
pub fn check_key(text: &str) -> Result<Option<keys::Key>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let hex: String = t
        .to_lowercase()
        .replace("0x", "")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',' && *c != '"')
        .collect();
    if let Some(c) = hex.chars().find(|c| !c.is_ascii_hexdigit()) {
        return Err(format!("{c:?} isn't a hex digit (0-9, a-f)"));
    }
    if hex.len() != 64 {
        return Err(format!("{} characters: a key is 64", hex.len()));
    }
    parse_hex_key(&hex, "key").map(Some).map_err(|e| e.to_string())
}

/// A whole keys.json pasted into a key box: its psarc, sng and Mac sng keys as text (None when it isn't one; a key
/// it hasn't is "").
pub fn split_pasted(text: &str) -> Option<(String, String, String)> {
    if !text.contains("psarc_key") && !text.contains("sng_key") {
        return None;
    }
    let k = qrs_formats::keys_from_text(text, "pasted text").ok()?;
    let hex = |k: Option<keys::Key>| {
        k.map(|k| k.iter().map(|b| format!("{b:02x}")).collect::<String>())
            .unwrap_or_default()
    };
    Some((hex(k.psarc), hex(k.sng), hex(k.sng_mac)))
}

/// Write the keys to `path` as keys.json; without a Mac SNG key, one already in a keys file there is kept.
pub fn write(path: &Path, psarc: keys::Key, sng: keys::Key, mac: Option<keys::Key>) -> std::io::Result<()> {
    let old_mac = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| qrs_formats::keys_from_text(&t, "keys.json").ok())
        .and_then(|k| k.sng_mac);
    let k = Keys {
        psarc: Some(psarc),
        sng: Some(sng),
        sng_mac: mac.or(old_mac),
        source: None,
    };
    qrs_library::paths::write_atomic(path, &keys::keys_to_json(&k))
}

/// The size and time of each file (None: not there), to tell when one changes.
fn stamps(files: &[PathBuf]) -> Vec<Option<(u64, SystemTime)>> {
    files
        .iter()
        .map(|f| {
            std::fs::metadata(f)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect()
}

/// A thread that looks at the keys files every 2 s (a stat each, nothing read) and raises `changed` (and wakes
/// the window) when one appears, changes or goes.
pub fn watch(files: Vec<PathBuf>, wake: impl Fn() + Send + 'static) -> Arc<AtomicBool> {
    let changed = Arc::new(AtomicBool::new(false));
    let flag = changed.clone();
    // (stamped here, not on the thread: a file arriving before the thread first runs is still a change)
    let mut last = stamps(&files);
    std::thread::Builder::new()
        .name("qrs-keys-watch".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let now = stamps(&files);
                if now != last {
                    last = now;
                    flag.store(true, Ordering::Release);
                    wake();
                }
            }
        })
        .ok();
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs-keyfile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // made-up keys: never real ones in tests
    const A: &str = "0101010101010101010101010101010101010101010101010101010101010101";
    const B: &str = "0202020202020202020202020202020202020202020202020202020202020202";

    #[test]
    fn the_data_folders_notepad_file_is_found_and_said_so() {
        let d = scratch("found");
        let files = keys::files_in(&d);
        assert_eq!(load(&files).1, KeyState::Missing);
        std::fs::write(
            d.join("keys.json.txt"),
            format!(r#"{{"psarc_key": "{A}", "sng_key": "{B}"}}"#),
        )
        .unwrap();
        let (k, state) = load(&files);
        assert!(k.can_read_psarc());
        assert_eq!(
            state,
            KeyState::Found {
                path: Some(d.join("keys.json.txt")),
                misnamed: true
            }
        );
        // a keys.json that doesn't read is said to be invalid, with why
        std::fs::write(d.join("keys.json"), r#"{"psarc_key": "<64 hex characters>"}"#).unwrap();
        match load(&files).1 {
            KeyState::Invalid { path, why } => {
                assert!(path.ends_with("keys.json"));
                assert!(why.contains("example"), "{why}");
            }
            s => panic!("{s:?}"),
        }
        std::fs::write(d.join("keys.json"), format!(r#"{{"psarc_key": "{A}"}}"#)).unwrap();
        assert!(matches!(load(&files).1, KeyState::Partial { .. }));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn typed_keys_are_checked() {
        assert_eq!(check_key("  "), Ok(None));
        assert_eq!(check_key(A).unwrap().unwrap()[0], 1);
        assert_eq!(
            check_key(&format!("\"{}\",", A.to_uppercase())).unwrap().unwrap()[31],
            1
        );
        assert!(check_key("abcd").unwrap_err().contains("4 characters"));
        assert!(check_key(&A.replace('1', "g")).unwrap_err().contains("hex digit"));
        assert_eq!(
            split_pasted(&format!(r#"{{"psarc_key": "{A}", "sng_key": "{B}"}}"#)),
            Some((A.to_string(), B.to_string(), String::new()))
        );
        assert_eq!(split_pasted(A), None);
    }

    #[test]
    fn written_keys_read_back_and_keep_a_mac_key() {
        let d = scratch("write");
        let path = d.join("keys.json");
        std::fs::write(
            &path,
            format!(r#"{{"psarc_key": "{B}", "sng_key": "{B}", "sng_mac_key": "{A}"}}"#),
        )
        .unwrap();
        write(&path, [1; 32], [2; 32], None).unwrap();
        let k = qrs_formats::load_keys(Some(&path), &[]).unwrap();
        assert_eq!(
            (k.psarc, k.sng, k.sng_mac),
            (Some([1; 32]), Some([2; 32]), Some([1; 32]))
        );
        // a Mac key given replaces it (P51.2, Settings' third box)
        write(&path, [1; 32], [2; 32], Some([3; 32])).unwrap();
        let k = qrs_formats::load_keys(Some(&path), &[]).unwrap();
        assert_eq!(k.sng_mac, Some([3; 32]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn songs_are_read_again_only_when_new_keys_can_read_them() {
        let none = Keys::default();
        let keys = Keys {
            psarc: Some([1; 32]),
            sng: Some([2; 32]),
            ..Default::default()
        };
        let waiting = vec![SongEntry {
            needs_keys: true,
            ..Default::default()
        }];
        let fine = vec![SongEntry::default()];
        assert!(needs_reread(&none, &keys, &waiting));
        assert!(!needs_reread(&keys, &keys, &waiting)); // nothing changed
        assert!(!needs_reread(&none, &keys, &fine)); // nothing waits for keys
        let half = Keys {
            psarc: Some([1; 32]),
            ..Default::default()
        };
        assert!(!needs_reread(&none, &half, &waiting)); // can't read them yet
    }

    #[test]
    fn the_watch_sees_a_keys_file_arrive() {
        let d = scratch("watch");
        let files = keys::files_in(&d);
        let changed = watch(files, || {});
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(d.join("keys.json.txt"), "{}").unwrap();
        let t0 = std::time::Instant::now();
        while !changed.load(Ordering::Acquire) && t0.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(changed.load(Ordering::Acquire));
        let _ = std::fs::remove_dir_all(&d);
    }
}
