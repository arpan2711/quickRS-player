//! Encryption key loading. Port of rsplay/keys.py, plus the Mac SNG key from rsfeedpak/platform.py.
//!
//! Keys are never stored in this repository. They are read, in order of precedence, from:
//! 1. environment variables `RSPLAY_PSARC_KEY` / `RSPLAY_SNG_KEY` / `RSPLAY_SNG_MAC_KEY` (64 hex characters);
//! 2. a JSON file: the explicit path, else `$RSPLAY_KEYS_FILE`, else `./keys.json`, else any extra folders the
//!    caller passes (the app's data folder), else `~/.config/rsplay/keys.json`.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub const ENV_PSARC: &str = "RSPLAY_PSARC_KEY";
pub const ENV_SNG: &str = "RSPLAY_SNG_KEY";
pub const ENV_SNG_MAC: &str = "RSPLAY_SNG_MAC_KEY";
pub const ENV_FILE: &str = "RSPLAY_KEYS_FILE";

const HELP: &str = "Set it in keys.json (see keys.example.json) or via the {env} environment variable. \
The key is published in open-source Rocksmith modding tools; it is not shipped with quickRS.";

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
            "{label}: still has the placeholder text; replace it with your 64-character hex key"
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
        out.push(cwd.join("keys.json"));
    }
    for d in extra_dirs {
        out.push(d.join("keys.json"));
    }
    if let Some(h) = home_dir() {
        out.push(h.join(".config").join("rsplay").join("keys.json"));
    }
    out
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

    #[test]
    fn missing_explicit_file_is_an_error() {
        assert!(load_keys(Some(Path::new("no/such/keys.json")), &[]).is_err());
    }
}
