//! Encryption key loading. Port of rsplay/keys.py, plus the Mac SNG key from rsfeedpak/platform.py.
//!
//! Keys are never stored in this repository. They are read, in order of precedence, from:
//! 1. environment variables `RSPLAY_PSARC_KEY` / `RSPLAY_SNG_KEY` / `RSPLAY_SNG_MAC_KEY` (64 hex characters);
//! 2. a JSON file: the explicit path, else `$RSPLAY_KEYS_FILE`, else `./keys.json`, else any extra folders the
//!    caller passes (the app's data folder), else `~/.config/rsplay/keys.json`.
//!
//! In each folder `keys.json.txt` and `keys.json.json` are read too when there's no `keys.json` (Notepad saves
//! "keys.json" as keys.json.txt unless the type is set to All files; P20.2).

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub const ENV_PSARC: &str = "RSPLAY_PSARC_KEY";
pub const ENV_SNG: &str = "RSPLAY_SNG_KEY";
pub const ENV_SNG_MAC: &str = "RSPLAY_SNG_MAC_KEY";
pub const ENV_FILE: &str = "RSPLAY_KEYS_FILE";

const HELP: &str = "Set it in keys.json (see keys.example.json) or via the {env} environment variable. \
The key is published in open-source RS modding tools; it is not shipped with quickRS.";

pub type Key = [u8; 32];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub psarc: Option<Key>,
    pub sng: Option<Key>,
    pub sng_mac: Option<Key>,
    /// The file the keys came from, if any.
    pub source: Option<PathBuf>,
}

impl Keys {
    pub fn require_psarc(&self) -> Result<&Key> {
        self.psarc.as_ref().ok_or_else(|| {
            Error::KeyMissing(format!(
                "This archive has an encrypted table of contents and no PSARC key is configured. {}",
                HELP.replace("{env}", ENV_PSARC)
            ))
        })
    }

    pub fn require_sng(&self) -> Result<&Key> {
        self.sng.as_ref().ok_or_else(|| {
            Error::KeyMissing(format!(
                "SNG note charts are encrypted and no SNG key is configured. {}",
                HELP.replace("{env}", ENV_SNG)
            ))
        })
    }

    /// Whether psarc songs can be read at all (TOC and PC charts).
    pub fn can_read_psarc(&self) -> bool {
        self.psarc.is_some() && self.sng.is_some()
    }
}

pub fn parse_hex_key(value: &str, label: &str) -> Result<Key> {
    if value.contains('<') || value.contains('>') {
        return Err(Error::Other(format!(
            "{label}: still has the example's <...> text; replace all of it, < and > too, with the key"
        )));
    }
    let cleaned: String = value
        .trim()
        .to_lowercase()
        .replace("0x", "")
        .chars()
        .filter(|c| *c != ' ' && *c != ',')
        .collect();
    if !cleaned.len().is_multiple_of(2) || !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Other(format!("{label}: not valid hexadecimal")));
    }
    let bytes: Vec<u8> = (0..cleaned.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).unwrap())
        .collect();
    bytes.as_slice().try_into().map_err(|_| {
        Error::Other(format!(
            "{label}: expected 32 bytes (64 hex characters), got {} bytes",
            bytes.len()
        ))
    })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The names a keys file goes by in a folder, in order: the right one, then what editors make of it.
pub const FILE_NAMES: [&str; 3] = ["keys.json", "keys.json.txt", "keys.json.json"];

/// The keys files a folder could hold, in order (`FILE_NAMES`).
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    FILE_NAMES.iter().map(|n| dir.join(n)).collect()
}

/// The first keys file in a folder, if there is one.
pub fn find_in(dir: &Path) -> Option<PathBuf> {
    files_in(dir).into_iter().find(|p| p.is_file())
}

/// A keys file under another of `FILE_NAMES` than keys.json (keys.json.txt: Notepad's doing).
pub fn is_misnamed(path: &Path) -> bool {
    let Some(n) = path.file_name().map(|n| n.to_string_lossy().to_lowercase()) else {
        return false;
    };
    n != FILE_NAMES[0] && FILE_NAMES.contains(&n.as_str())
}

/// Files that are searched for keys, in order.
pub fn candidate_files(explicit: Option<&Path>, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    if let Some(p) = explicit {
        return vec![p.to_path_buf()];
    }
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os(ENV_FILE) {
        out.push(PathBuf::from(p));
    }
    if let Ok(cwd) = std::env::current_dir() {
        out.extend(files_in(&cwd));
    }
    for d in extra_dirs {
        out.extend(files_in(d));
    }
    if let Some(h) = home_dir() {
        out.extend(files_in(&h.join(".config").join("rsplay")));
    }
    let mut seen: Vec<PathBuf> = Vec::new();
    out.retain(|p| {
        let fresh = !seen.contains(p);
        seen.push(p.clone());
        fresh
    });
    out
}

/// A keys.json's text as keys (no environment variables; `label` names it in errors). Also takes the text without
/// its braces (`"psarc_key": "...", "sng_key": "..."`), as people copy it out of a message.
pub fn keys_from_text(text: &str, label: &str) -> Result<Keys> {
    let t = text.trim().trim_start_matches('\u{feff}');
    let v: serde_json::Value = serde_json::from_str(t)
        .or_else(|e| {
            if t.starts_with('{') {
                Err(e)
            } else {
                serde_json::from_str(&format!("{{{}}}", t.trim_end_matches(',')))
            }
        })
        .map_err(|e| Error::Other(format!("{label}: invalid JSON ({e})")))?;
    let serde_json::Value::Object(m) = v else {
        return Err(Error::Other(format!("{label}: not a JSON object")));
    };
    let get = |field: &str| -> Result<Option<Key>> {
        match m.get(field).and_then(|v| v.as_str()) {
            Some(v) if !v.trim().is_empty() => parse_hex_key(v, &format!("{field} in {label}")).map(Some),
            _ => Ok(None),
        }
    };
    Ok(Keys {
        psarc: get("psarc_key")?,
        sng: get("sng_key")?,
        sng_mac: get("sng_mac_key")?,
        source: None,
    })
}

/// Keys as a keys.json's text (the ones there are; two-space indented).
pub fn keys_to_json(keys: &Keys) -> String {
    let hex = |k: &Key| k.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut m = serde_json::Map::new();
    for (name, k) in [
        ("psarc_key", &keys.psarc),
        ("sng_key", &keys.sng),
        ("sng_mac_key", &keys.sng_mac),
    ] {
        if let Some(k) = k {
            m.insert(name.into(), serde_json::Value::String(hex(k)));
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(m)).unwrap_or_default() + "\n"
}

impl Keys {
    /// The same keys (where they came from aside).
    pub fn same_keys(&self, other: &Keys) -> bool {
        self.psarc == other.psarc && self.sng == other.sng && self.sng_mac == other.sng_mac
    }
}

/// Load keys from the environment and/or a keys.json file. Missing keys are `None`.
pub fn load_keys(explicit: Option<&Path>, extra_dirs: &[PathBuf]) -> Result<Keys> {
    let mut data = serde_json::Map::new();
    let mut source = None;
    for candidate in candidate_files(explicit, extra_dirs) {
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate)?;
            let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .map_err(|e| Error::Other(format!("{}: invalid JSON ({e})", candidate.display())))?;
            if let serde_json::Value::Object(m) = v {
                data = m;
            }
            source = Some(candidate);
            break;
        }
    }
    if source.is_none()
        && let Some(p) = explicit
    {
        return Err(Error::Other(format!("keys file not found: {}", p.display())));
    }
    let wher = source
        .as_ref()
        .map(|s| format!(" in {}", s.display()))
        .unwrap_or_default();
    let get = |env: &str, field: &str| -> Result<Option<Key>> {
        let value = std::env::var(env)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| data.get(field).and_then(|v| v.as_str()).map(str::to_string));
        match value {
            Some(v) if !v.is_empty() => parse_hex_key(&v, &format!("{field}{wher}")).map(Some),
            _ => Ok(None),
        }
    };
    Ok(Keys {
        psarc: get(ENV_PSARC, "psarc_key")?,
        sng: get(ENV_SNG, "sng_key")?,
        sng_mac: get(ENV_SNG_MAC, "sng_mac_key")?,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_in_common_spellings() {
        let hex = "00".repeat(31) + "ff";
        assert_eq!(parse_hex_key(&hex, "k").unwrap()[31], 0xff);
        let spaced = "0x00, ".repeat(31) + "0x01";
        assert_eq!(parse_hex_key(&spaced, "k").unwrap()[31], 1);
    }

    #[test]
    fn rejects_placeholder_and_bad_length() {
        assert!(parse_hex_key("<64 hex characters>", "k").is_err());
        assert!(
            parse_hex_key("abcd", "k")
                .unwrap_err()
                .to_string()
                .contains("expected 32 bytes")
        );
        assert!(
            parse_hex_key("zz", "k")
                .unwrap_err()
                .to_string()
                .contains("hexadecimal")
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A keys.json of made-up keys (every byte `a`, then every byte `b`).
    fn fake_json(a: u8, b: u8) -> String {
        format!(
            r#"{{"psarc_key": "{}", "sng_key": "{}"}}"#,
            format!("{a:02x}").repeat(32),
            format!("{b:02x}").repeat(32)
        )
    }

    #[test]
    fn notepads_names_are_read_when_keys_json_isnt_there() {
        let d = scratch("names");
        assert_eq!(find_in(&d), None);
        std::fs::write(d.join("keys.json.txt"), fake_json(1, 2)).unwrap();
        let found = find_in(&d).unwrap();
        assert!(found.ends_with("keys.json.txt") && is_misnamed(&found));
        let k = load_keys(Some(&found), &[]).unwrap();
        assert_eq!(k.psarc, Some([1; 32]));
        // keys.json.json too, and keys.json before either
        std::fs::remove_file(d.join("keys.json.txt")).unwrap();
        std::fs::write(d.join("keys.json.json"), fake_json(3, 4)).unwrap();
        assert!(find_in(&d).unwrap().ends_with("keys.json.json"));
        std::fs::write(d.join("keys.json"), fake_json(5, 6)).unwrap();
        let first = find_in(&d).unwrap();
        assert!(first.ends_with("keys.json") && !is_misnamed(&first));
        assert!(!is_misnamed(Path::new("notes.txt")));
        // in the search order, a folder's names together
        let c = candidate_files(None, std::slice::from_ref(&d));
        let i = c.iter().position(|p| p == &d.join("keys.json")).unwrap();
        assert_eq!(c[i + 1], d.join("keys.json.txt"));
        assert_eq!(c[i + 2], d.join("keys.json.json"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pasted_text_reads_with_or_without_braces_and_round_trips() {
        let k = keys_from_text(&fake_json(0xab, 0xcd), "pasted").unwrap();
        assert_eq!((k.psarc, k.sng), (Some([0xab; 32]), Some([0xcd; 32])));
        let json = fake_json(0xab, 0xcd);
        let bare = json.trim_start_matches('{').trim_end_matches('}');
        assert!(keys_from_text(bare, "pasted").unwrap().same_keys(&k));
        let back = keys_from_text(&keys_to_json(&k), "written").unwrap();
        assert!(back.same_keys(&k));
        assert!(keys_from_text(r#"{"psarc_key": "abcd"}"#, "pasted").is_err());
        assert!(keys_from_text("not json at all", "pasted").is_err());
    }

    #[test]
    fn missing_explicit_file_is_an_error() {
        assert!(load_keys(Some(Path::new("no/such/keys.json")), &[]).is_err());
    }
}
