//! config.json: the Python app's settings file, kept in its shape (D6). Unknown keys survive a save; values are
//! read through typed getters with the Python app's defaults.

use std::path::{Path, PathBuf};

use qrs_formats::game_names::{GAME_EXE, GAME_FOLDER, GAME_INI};
use serde_json::{Map, Value};

use crate::paths::write_atomic;

/// Where the Python app looked for songs when none were configured (the real folder names on disk, D149: kept in
/// full, as in `qrs_formats::game_names`).
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

    /// The configured song folders, else the defaults that exist (the Python app's, then Steam's RS).
    /// A folder starting with `./` or `../` is read from config.json's folder, so a portable install can carry its songs along
    /// (`"../Songs"`).
    pub fn library_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self.added_dirs();
        if dirs.is_empty() {
            dirs = DEFAULT_LIBRARY_DIRS
                .iter()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .collect();
            for d in steam_rs_dlc() {
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
        }
        dirs
    }

    /// library_dirs as written in config.json (relative ones left relative).
    pub fn saved_library_dirs(&self) -> Vec<PathBuf> {
        self.paths("library_dirs")
    }

    /// A folder as config.json names it, made whole: one starting with `./` or `../` is read from config.json's
    /// folder (a portable install's `"../Songs"`).
    fn resolve(&self, d: &Path) -> PathBuf {
        use std::path::Component::{CurDir, ParentDir};
        if matches!(d.components().next(), Some(CurDir | ParentDir)) {
            join_lexically(self.path.parent().unwrap_or(Path::new(".")), d)
        } else {
            d.to_path_buf()
        }
    }

    /// A folder setting (config.json `recordings_dir`, `takes_dir`: a path, or missing/empty for the default),
    /// made whole like the song folders. Any relative one (`recs`, not only `./recs`) is read from config.json's
    /// folder: left relative it would follow the program's working folder (a shortcut's "Start in"), and files would
    /// go somewhere other than where Open shows (P23.13).
    pub fn folder(&self, key: &str) -> Option<PathBuf> {
        let s = self.doc.get(key).and_then(Value::as_str)?.trim();
        if s.is_empty() {
            return None;
        }
        let d = Path::new(s);
        Some(if d.is_relative() && !d.has_root() {
            join_lexically(self.path.parent().unwrap_or(Path::new(".")), d)
        } else {
            self.resolve(d)
        })
    }

    /// Set a folder setting, or (None) put it back to its default.
    pub fn set_folder(&mut self, key: &str, dir: Option<&Path>) {
        match dir {
            Some(d) => self.set(key, d.to_string_lossy().into_owned()),
            None => {
                self.remove(key);
            }
        }
    }

    /// Where recordings of what you hear go (P23.9): `recordings_dir`, else `default_recordings_dir`.
    pub fn recordings_dir(&self, data_root: &Path) -> PathBuf {
        self.folder(RECORDINGS_DIR)
            .unwrap_or_else(|| default_recordings_dir(data_root))
    }

    /// Where song takes go (D30): `takes_dir`, else `<data>/takes` (where they always were).
    pub fn takes_dir(&self, data_root: &Path) -> PathBuf {
        self.folder(TAKES_DIR)
            .unwrap_or_else(|| default_takes_dir(data_root))
    }

    pub fn set_library_dirs(&mut self, dirs: &[PathBuf]) {
        self.set_paths("library_dirs", dirs);
    }

    fn paths(&self, key: &str) -> Vec<PathBuf> {
        self.doc
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(PathBuf::from).collect())
            .unwrap_or_default()
    }

    fn set_paths(&mut self, key: &str, dirs: &[PathBuf]) {
        let v: Vec<Value> = dirs
            .iter()
            .map(|d| Value::String(d.to_string_lossy().into_owned()))
            .collect();
        self.set(key, Value::Array(v));
    }

    /// The folders the user added (config.json's library_dirs, relative ones made whole; without the defaults).
    pub fn added_dirs(&self) -> Vec<PathBuf> {
        self.paths("library_dirs")
            .iter()
            .map(|d| self.resolve(d))
            .collect()
    }

    /// Folders taken out of the library that would otherwise be found by themselves (`library_dirs_removed`).
    pub fn removed_dirs(&self) -> Vec<PathBuf> {
        self.paths("library_dirs_removed")
    }

    /// Whether RS2014's own dlc folders are searched for songs (`SEARCH_RS_DLC`, on unless turned
    /// off; off: only the folders added, feedpaks without keys).
    pub fn search_dlc(&self) -> bool {
        self.bool(SEARCH_RS_DLC, true)
    }

    /// The folders to scan (P20.2), with RS's dlc folders as found on this machine:
    /// - the folders added (library_dirs), less any RS dlc folder when searching those is off;
    /// - RS's dlc folders when it's on;
    /// - a feedpak folder next to any of those dlc folders (rs-open-format's output), searched or not;
    /// - less the folders removed.
    pub fn scan_dirs_with(&self, rs_dlc: &[PathBuf]) -> Vec<PathBuf> {
        let added = self.added_dirs();
        let removed = self.removed_dirs();
        let search = self.search_dlc();
        let is_dlc = |d: &PathBuf| rs_dlc.iter().any(|r| same_dir(r, d)) || looks_like_rs_dlc(d);
        let mut out: Vec<PathBuf> = Vec::new();
        let push = |d: PathBuf, out: &mut Vec<PathBuf>| {
            if !out.iter().any(|o| same_dir(o, &d)) && !removed.iter().any(|r| same_dir(r, &d)) {
                out.push(d);
            }
        };
        for d in &added {
            if search || !is_dlc(d) {
                push(d.clone(), &mut out);
            }
        }
        if search {
            for d in rs_dlc {
                push(d.clone(), &mut out);
            }
        }
        for d in added.iter().chain(rs_dlc) {
            if let Some(f) = sibling_feedpak_dir(d) {
                push(f, &mut out);
            }
        }
        out
    }

    /// The folders to scan on this machine (`scan_dirs_with` RS's dlc folders found here).
    pub fn scan_dirs(&self) -> Vec<PathBuf> {
        self.scan_dirs_with(&rs_dlc_dirs())
    }

    /// A folder added for good: in library_dirs (and no longer among the removed).
    pub fn add_dir(&mut self, dir: &Path) {
        // (as written, so a portable "../Songs" stays relative)
        let mut added = self.saved_library_dirs();
        if !added.iter().any(|d| same_dir(&self.resolve(d), dir)) {
            added.push(dir.to_path_buf());
        }
        self.set_library_dirs(&added);
        let mut removed = self.removed_dirs();
        removed.retain(|d| !same_dir(d, dir));
        self.set_paths("library_dirs_removed", &removed);
    }

    /// A folder taken out: out of library_dirs, and remembered as removed so it isn't found by itself again.
    pub fn remove_dir(&mut self, dir: &Path) {
        let mut added = self.saved_library_dirs();
        added.retain(|d| !same_dir(&self.resolve(d), dir));
        self.set_library_dirs(&added);
        let mut removed = self.removed_dirs();
        if !removed.iter().any(|d| same_dir(d, dir)) {
            removed.push(dir.to_path_buf());
        }
        self.set_paths("library_dirs_removed", &removed);
    }
}

/// Two spellings of one folder (case and a trailing separator aside on Windows).
pub fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR);
        let s = s.trim_end_matches(std::path::MAIN_SEPARATOR).to_string();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

/// A folder called dlc with RS2014 next to it.
fn looks_like_rs_dlc(d: &Path) -> bool {
    d.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("dlc"))
        && d.parent()
            .is_some_and(|p| p.join(GAME_EXE).is_file() || p.join(GAME_INI).is_file())
}

/// RS2014's dlc folders on this machine: the Python app's defaults that exist, then Steam's.
pub fn rs_dlc_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = DEFAULT_LIBRARY_DIRS
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    for d in steam_rs_dlc() {
        if !dirs.iter().any(|x| same_dir(x, &d)) {
            dirs.push(d);
        }
    }
    dirs
}

/// config.json key: search the game's own dlc folders (the on-disk key keeps the full name, D149).
pub const SEARCH_RS_DLC: &str = "search_rocksmith_dlc";

/// config.json keys of the two recording folders (P23.9).
pub const RECORDINGS_DIR: &str = "recordings_dir";
pub const TAKES_DIR: &str = "takes_dir";

/// The folder recordings go to unless you choose one: `quickRS Recordings` in your Music folder (Windows' Music,
/// the Mac's ~/Music), else in Documents, else `<data>/recordings`.
pub fn default_recordings_dir(data_root: &Path) -> PathBuf {
    let user = directories::UserDirs::new();
    let base = user
        .as_ref()
        .and_then(|u| u.audio_dir().or_else(|| u.document_dir()).map(Path::to_path_buf));
    recordings_in(base.as_deref(), data_root)
}

/// `quickRS Recordings` in `base` (Music or Documents), or `<data>/recordings` when there's neither.
pub fn recordings_in(base: Option<&Path>, data_root: &Path) -> PathBuf {
    match base {
        Some(b) => b.join("quickRS Recordings"),
        None => data_root.join("recordings"),
    }
}

/// The songs folder the first-time setup offers when none is found (P23.14): `quickRS Songs` in Music (the Mac's
/// ~/Music), else in Documents, else `<data>/songs`.
pub fn default_songs_dir(data_root: &Path) -> PathBuf {
    let user = directories::UserDirs::new();
    let base = user
        .as_ref()
        .and_then(|u| u.audio_dir().or_else(|| u.document_dir()).map(Path::to_path_buf));
    songs_in(base.as_deref(), data_root)
}

/// `quickRS Songs` in `base` (Music or Documents), or `<data>/songs` when there's neither.
pub fn songs_in(base: Option<&Path>, data_root: &Path) -> PathBuf {
    match base {
        Some(b) => b.join("quickRS Songs"),
        None => data_root.join("songs"),
    }
}

/// Song takes' folder unless you choose one (where they've always been).
pub fn default_takes_dir(data_root: &Path) -> PathBuf {
    data_root.join("takes")
}

/// Where Steam keeps its own library on this OS: `C:\Program Files (x86)\Steam`; macOS
/// `~/Library/Application Support/Steam`; on Linux each of the usual installs (P28.3): `~/.local/share/Steam`,
/// `~/.steam/steam` and `~/.steam/debian-installation` (Debian and Ubuntu's package), Flatpak's and Snap's.
pub fn steam_roots() -> Vec<PathBuf> {
    #[cfg(unix)]
    if let Some(home) = directories::BaseDirs::new() {
        let mut out = vec![home.data_dir().join("Steam")];
        if cfg!(target_os = "linux") {
            let h = home.home_dir();
            out.extend([
                h.join(".steam/steam"),
                h.join(".steam/debian-installation"),
                h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
                h.join("snap/steam/common/.local/share/Steam"),
            ]);
        }
        return out;
    }
    vec![PathBuf::from(r"C:\Program Files (x86)\Steam")]
}

/// RS2014's dlc folders in every Steam library (from steamapps/libraryfolders.vdf).
pub fn steam_rs_dlc() -> Vec<PathBuf> {
    let mut roots = steam_roots();
    for root in roots.clone() {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        let Ok(text) = std::fs::read_to_string(vdf) else {
            continue;
        };
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
        let d = r.join("steamapps").join("common").join(GAME_FOLDER).join("dlc");
        // (on Linux one install is often a link to another: the same folder once)
        #[cfg(unix)]
        let d = d.canonicalize().unwrap_or(d);
        if d.is_dir() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// A folder of converted feedpaks next to an RS `dlc` folder (rs-open-format's default output), if there is.
pub fn sibling_feedpak_dir(dlc: &Path) -> Option<PathBuf> {
    if !dlc.file_name()?.to_string_lossy().eq_ignore_ascii_case("dlc") {
        return None;
    }
    let d = dlc.parent()?.join("feedpak");
    d.is_dir().then_some(d)
}

/// `base` joined with `rel`, with `.` and `..` folded away (the folders needn't exist).
fn join_lexically(base: &Path, rel: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = base.to_path_buf();
    for c in rel.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_folders_default_change_and_round_trip() {
        let dir = std::env::temp_dir().join(format!("qrs-config-rec-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"viz_style": "glow", "takes_dir": "  "}"#).unwrap();
        let data = Path::new("/data/quickRS");
        let mut c = Config::load(&path);
        // the defaults: takes where they always were (a blank setting is the default), recordings in Music
        assert_eq!(c.takes_dir(data), data.join("takes"));
        assert_eq!(c.recordings_dir(data), default_recordings_dir(data));
        assert!(
            c.recordings_dir(data).ends_with("quickRS Recordings")
                || c.recordings_dir(data) == data.join("recordings")
        );
        assert_eq!(recordings_in(None, data), data.join("recordings"));
        assert_eq!(
            recordings_in(Some(Path::new("/Users/me/Music")), data),
            PathBuf::from("/Users/me/Music/quickRS Recordings")
        );
        // changed, saved and read back; unknown keys kept
        let mine = PathBuf::from("/mnt/rec/amp");
        c.set_folder(RECORDINGS_DIR, Some(&mine));
        c.set_folder(TAKES_DIR, Some(Path::new("./takes here")));
        c.save().unwrap();
        let mut c = Config::load(&path);
        assert_eq!(c.recordings_dir(data), mine);
        assert_eq!(
            c.takes_dir(data),
            dir.join("takes here"),
            "relative to config.json's folder"
        );
        assert_eq!(c.str("viz_style", ""), "glow");
        // a bare relative folder is config.json's folder's too, not the working folder's (P23.13)
        c.set(RECORDINGS_DIR, "my recs".to_string());
        assert_eq!(c.recordings_dir(data), dir.join("my recs"));
        c.set_folder(RECORDINGS_DIR, Some(&mine));
        // reset to default: the key goes
        c.set_folder(TAKES_DIR, None);
        assert!(c.get(TAKES_DIR).is_none());
        assert_eq!(c.takes_dir(data), data.join("takes"));
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        let portable = Config {
            path: PathBuf::from("/Volumes/USB/quickRS/quickRS Data/config.json"),
            doc: serde_json::from_str(r#"{"library_dirs": ["../Songs", "./more"]}"#).unwrap(),
        };
        assert_eq!(
            portable.library_dirs(),
            vec![
                PathBuf::from("/Volumes/USB/quickRS/Songs"),
                PathBuf::from("/Volumes/USB/quickRS/quickRS Data/more")
            ]
        );
        // scanned as whole paths; adding the same folder again keeps the relative spelling, removing it works
        let mut portable = portable;
        assert_eq!(portable.scan_dirs_with(&[]), portable.library_dirs());
        let songs = portable.library_dirs()[0].clone();
        portable.add_dir(&songs);
        assert_eq!(
            portable.saved_library_dirs(),
            vec![PathBuf::from("../Songs"), PathBuf::from("./more")]
        );
        portable.remove_dir(&songs);
        assert_eq!(portable.saved_library_dirs(), vec![PathBuf::from("./more")]);
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

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn folders_added_and_removed_persist() {
        let dir = scratch("folders");
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"library_dirs": ["X:\\songs"], "theme": "graphite"}"#).unwrap();
        let mut c = Config::load(&path);
        let feedpaks = dir.join("feedpaks");
        c.add_dir(&feedpaks);
        c.add_dir(&feedpaks); // (once)
        c.save().unwrap();
        let mut back = Config::load(&path);
        assert_eq!(
            back.added_dirs(),
            vec![PathBuf::from(r"X:\songs"), feedpaks.clone()]
        );
        assert_eq!(back.scan_dirs_with(&[]), back.added_dirs());
        back.remove_dir(Path::new(r"X:\songs"));
        back.save().unwrap();
        let back = Config::load(&path);
        assert_eq!(back.added_dirs(), vec![feedpaks.clone()]);
        assert_eq!(back.removed_dirs(), vec![PathBuf::from(r"X:\songs")]);
        assert_eq!(back.str("theme", ""), "graphite");
        // a folder found by itself, removed: stays out; added again: back in
        let dlc = dir.join(GAME_FOLDER).join("dlc");
        let mut c = back;
        assert!(c.scan_dirs_with(std::slice::from_ref(&dlc)).contains(&dlc));
        c.remove_dir(&dlc);
        assert!(!c.scan_dirs_with(std::slice::from_ref(&dlc)).contains(&dlc));
        c.add_dir(&dlc);
        assert!(c.scan_dirs_with(std::slice::from_ref(&dlc)).contains(&dlc));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn feedpak_only_leaves_rs_dlc_out_but_keeps_its_feedpaks() {
        let dir = scratch("feedpak-only");
        let game = dir.join(GAME_FOLDER);
        let dlc = game.join("dlc");
        let feedpak = game.join("feedpak");
        let mine = dir.join("mine");
        for d in [&dlc, &feedpak, &mine] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(game.join(GAME_EXE), b"").unwrap();
        let path = dir.join("config.json");
        let mut c = Config::load(&path);
        c.add_dir(&mine);
        c.add_dir(&dlc); // (saved by 0.1.0 with the defaults)
        let found = std::slice::from_ref(&dlc);
        assert_eq!(
            c.scan_dirs_with(found),
            vec![mine.clone(), dlc.clone(), feedpak.clone()]
        );
        c.set(SEARCH_RS_DLC, false);
        assert_eq!(c.scan_dirs_with(found), vec![mine.clone(), feedpak.clone()]);
        // a dlc folder with the game next to it is left out even when it's not one found by itself
        assert_eq!(c.scan_dirs_with(&[]), vec![mine.clone(), feedpak.clone()]);
        // and the scan of those folders finds no psarc from the dlc folder (nor the game's songs.psarc)
        std::fs::write(dlc.join("song_p.psarc"), b"x").unwrap();
        std::fs::write(game.join("songs.psarc"), b"x").unwrap();
        std::fs::write(feedpak.join("song.feedpak"), b"x").unwrap();
        let files = crate::scan::find_song_files(&c.scan_dirs_with(found));
        assert_eq!(files, vec![feedpak.join("song.feedpak")]);
        c.set(SEARCH_RS_DLC, true);
        let files = crate::scan::find_song_files(&c.scan_dirs_with(found));
        assert_eq!(files.len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// P37.16 (D149): the game is called RS in the app, but the folders searched for its songs keep their real
    /// names, so the owner's library is still found.
    #[test]
    fn the_dlc_search_keeps_the_games_real_folder_names() {
        use qrs_formats::game_names::REMASTERED_FOLDER;
        // (the config key too: the owner's config.json says it in full)
        assert_eq!(SEARCH_RS_DLC, ["search_rock", "smith_dlc"].concat());
        assert_eq!(DEFAULT_LIBRARY_DIRS[0], format!(r"Y:\{REMASTERED_FOLDER}\dlc"));
        assert_eq!(
            DEFAULT_LIBRARY_DIRS[1],
            format!(r"C:\Program Files (x86)\Steam\steamapps\common\{GAME_FOLDER}\dlc")
        );
        // a dlc folder with the game's program or settings file beside it is the game's own
        for marker in [GAME_EXE, GAME_INI] {
            let dir = scratch(&format!("real-names-{marker}"));
            let dlc = dir.join(GAME_FOLDER).join("dlc");
            std::fs::create_dir_all(&dlc).unwrap();
            assert!(!looks_like_rs_dlc(&dlc));
            std::fs::write(dir.join(GAME_FOLDER).join(marker), b"").unwrap();
            assert!(looks_like_rs_dlc(&dlc), "{marker}");
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
