//! config.json: the Python app's settings file, kept in its shape (D6). Unknown keys survive a save; values are
//! read through typed getters with the Python app's defaults.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::paths::write_atomic;

/// Where the Python app looked for songs when none were configured.
pub const DEFAULT_LIBRARY_DIRS: &[&str] = &[
    r"Y:\Rocksmith 2014 Edition - Remastered\dlc",
    r"C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc",
];

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub path: PathBuf,
    pub doc: Map<String, Value>,
}

impl Config {
    /// Read config.json (a missing or broken file reads as empty).
    pub fn load(path: impl Into<PathBuf>) -> Config {
        let path = path.into();
        let doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        Config { path, doc }
    }

    /// Write it back, two-space indented like the Python app.
    pub fn save(&self) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(&Value::Object(self.doc.clone())).unwrap_or_default();
        write_atomic(&self.path, &text)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.doc.get(key)
    }

    pub fn set(&mut self, key: &str, v: impl Into<Value>) {
        self.doc.insert(key.to_string(), v.into());
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.doc.shift_remove(key)
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.doc.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    pub fn f64(&self, key: &str, default: f64) -> f64 {
        self.doc.get(key).and_then(Value::as_f64).unwrap_or(default)
    }

    pub fn i64(&self, key: &str, default: i64) -> i64 {
        self.doc.get(key).and_then(Value::as_i64).unwrap_or(default)
    }

    pub fn str(&self, key: &str, default: &str) -> String {
        self.doc
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string()
    }

    /// The configured song folders, else the defaults that exist (the Python app's, then Steam's Rocksmith).
    pub fn library_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self
            .doc
            .get("library_dirs")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(PathBuf::from).collect())
            .unwrap_or_default();
        if dirs.is_empty() {
            dirs = DEFAULT_LIBRARY_DIRS
                .iter()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .collect();
            for d in steam_rocksmith_dlc() {
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
        }
        dirs
    }

    pub fn set_library_dirs(&mut self, dirs: &[PathBuf]) {
        let v: Vec<Value> = dirs
            .iter()
            .map(|d| Value::String(d.to_string_lossy().into_owned()))
            .collect();
        self.set("library_dirs", Value::Array(v));
    }
}

/// Rocksmith 2014's dlc folders in every Steam library (from steamapps/libraryfolders.vdf).
pub fn steam_rocksmith_dlc() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(r"C:\Program Files (x86)\Steam")];
    let vdf = roots[0].join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(vdf) {
        for line in text.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            // 	"path"		"D:\\SteamLibrary"
            if parts.len() >= 4 && parts[1] == "path" {
                roots.push(PathBuf::from(parts[3].replace("\\\\", "\\")));
            }
        }
    }
    let mut out = Vec::new();
    for r in roots {
        let d = r
            .join("steamapps")
            .join("common")
            .join("Rocksmith2014")
            .join("dlc");
        if d.is_dir() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// A folder of converted feedpaks next to a Rocksmith `dlc` folder (rs-open-format's default output), if there is.
pub fn sibling_feedpak_dir(dlc: &Path) -> Option<PathBuf> {
    if !dlc.file_name()?.to_string_lossy().eq_ignore_ascii_case("dlc") {
        return None;
    }
    let d = dlc.parent()?.join("feedpak");
    d.is_dir().then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_survive_a_save() {
        let dir = std::env::temp_dir().join(format!("qrs-config-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{"library_dirs": ["X:\\songs"], "viz_style": "glow", "volume": 0.2}"#,
        )
        .unwrap();
        let mut c = Config::load(&path);
        assert_eq!(c.library_dirs(), vec![PathBuf::from(r"X:\songs")]);
        assert_eq!(c.f64("volume", 0.8), 0.2);
        c.set("theme", "graphite");
        c.save().unwrap();
        let back = Config::load(&path);
        assert_eq!(back.str("viz_style", ""), "glow");
        assert_eq!(back.str("theme", ""), "graphite");
        let keys: Vec<&String> = back.doc.keys().collect();
        assert_eq!(keys, ["library_dirs", "viz_style", "volume", "theme"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
