//! Finding songs: every `.psarc`, `.feedpak` and `.sloppak` (file, or folder with a manifest) in the library
//! folders, read into a cached index keyed by path + size + modification time, so later launches only read what
//! changed (port of rsplay/library.py, for both formats). Multi-song psarcs (songs.psarc, RS1 packs) list each song.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use qrs_formats::package::Format;
use qrs_formats::{Keys, Package};
use serde::{Deserialize, Serialize};

use crate::paths::write_atomic;

/// Bumped whenever SongEntry changes, so old indexes are read again.
pub const INDEX_VERSION: u32 = 1;

/// Multi-song game archives that live next to the dlc folder; they aren't single songs.
pub const NOT_SONGS: &[&str] = &[
    "audio", "cache", "crowd", "etudes", "gears", "guitars", "session", "songs", "static", "video",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartEntry {
    pub key: String,
    pub name: String,
    pub tuning: String,
    #[serde(default)]
    pub bonus: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SongEntry {
    pub path: String,
    pub size: u64,
    pub mtime: f64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub year: i32,
    #[serde(default)]
    pub length: f64,
    #[serde(default)]
    pub arrangements: Vec<PartEntry>,
    #[serde(default)]
    pub error: String,
    /// Set for songs inside multi-song packs (songs.psarc, rs1compatibility*).
    #[serde(default)]
    pub song_key: String,
    /// "psarc" or "feedpak".
    #[serde(default)]
    pub format: String,
    /// A feedpak's provenance: the id of the psarc it was converted from ("file_p.psarc|SongKey").
    #[serde(default)]
    pub source_id: String,
    /// A psarc that can't be read without Rocksmith's keys.
    #[serde(default)]
    pub needs_keys: bool,
}

impl SongEntry {
    pub fn file_name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The Python app's id: "file name|song key".
    pub fn id(&self) -> String {
        format!("{}|{}", self.file_name(), self.song_key)
    }

    pub fn display_title(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        let stem = Path::new(&self.path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        stem.strip_suffix("_p").map(String::from).unwrap_or(stem)
    }

    pub fn is_feedpak(&self) -> bool {
        self.format == "feedpak"
    }

    pub fn playable(&self) -> bool {
        self.error.is_empty() && !self.needs_keys && !self.arrangements.is_empty()
    }

    pub fn search_text(&self) -> String {
        let stem = Path::new(&self.path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{} {} {} {}", self.title, self.artist, self.album, stem).to_lowercase()
    }
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Every song file in the folders (sorted, folders walked recursively). The base game's songs.psarc next to a dlc
/// folder is included, as the Python app did.
pub fn find_song_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut items: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        items.sort();
        for p in items {
            let ext = ext_of(&p);
            if ext == "feedpak" || ext == "sloppak" {
                out.push(p); // a file or a folder pack
            } else if p.is_dir() {
                walk(&p, out);
            } else if ext == "psarc" {
                let stem = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !NOT_SONGS.contains(&stem.as_str()) {
                    out.push(p);
                }
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        if !d.is_dir() {
            continue;
        }
        walk(d, &mut out);
        if d.file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("dlc"))
            && let Some(parent) = d.parent()
        {
            let songs = parent.join("songs.psarc");
            if songs.is_file() && !out.contains(&songs) {
                out.push(songs);
            }
        }
    }
    out
}

/// (size, mtime) of a song file; for a folder pack, its manifest's.
pub fn stamp(path: &Path) -> Option<(u64, f64)> {
    let target = if path.is_dir() {
        ["manifest.yaml", "manifest.yml"]
            .iter()
            .map(|m| path.join(m))
            .find(|m| m.is_file())?
    } else {
        path.to_path_buf()
    };
    let md = std::fs::metadata(&target).ok()?;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Some((md.len(), mtime))
}

fn entry_of(pkg: &Package, path: &Path, size: u64, mtime: f64, song_key: &str) -> SongEntry {
    let mut e = SongEntry {
        path: path.to_string_lossy().into_owned(),
        size,
        mtime,
        song_key: song_key.to_string(),
        format: match pkg.format() {
            Format::Psarc => "psarc".into(),
            Format::Feedpak => "feedpak".into(),
        },
        ..Default::default()
    };
    e.title = pkg.title();
    e.artist = pkg.artist();
    e.album = pkg.album();
    e.year = pkg.year();
    e.length = pkg.duration();
    e.source_id = pkg.feedpak_ref().map(|f| f.source_id()).unwrap_or_default();
    for a in &pkg.arrangements {
        e.arrangements.push(PartEntry {
            key: a.key.clone(),
            name: a.name.clone(),
            tuning: a.tuning_name(),
            bonus: a.bonus,
        });
    }
    if e.arrangements.is_empty() {
        e.error = "no playable arrangements".into();
    }
    e
}

/// One entry per song in the package (packs like songs.psarc hold dozens). A file that can't be read gives one
/// entry with its error (or `needs_keys`), so it isn't read again until it changes.
pub fn read_entries(path: &Path, keys: &Keys) -> Vec<SongEntry> {
    let Some((size, mtime)) = stamp(path) else {
        return Vec::new();
    };
    let failed = |err: qrs_formats::Error| -> Vec<SongEntry> {
        let fmt = if ext_of(path) == "psarc" {
            "psarc"
        } else {
            "feedpak"
        };
        vec![SongEntry {
            path: path.to_string_lossy().into_owned(),
            size,
            mtime,
            needs_keys: err.is_key_missing(),
            error: err.to_string(),
            format: fmt.into(),
            ..Default::default()
        }]
    };
    let pkg = match Package::open(path, keys, None) {
        Ok(p) => p,
        Err(e) => return failed(e),
    };
    let song_keys = pkg.song_keys();
    if song_keys.len() <= 1 {
        return vec![entry_of(&pkg, path, size, mtime, "")];
    }
    let mut out = Vec::new();
    for k in song_keys {
        match Package::open(path, keys, Some(&k)) {
            Ok(sub) => out.push(entry_of(&sub, path, size, mtime, &k)),
            Err(e) => out.extend(failed(e)),
        }
    }
    out
}

#[derive(Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    songs: Vec<SongEntry>,
}

/// The cached index: path -> its entries.
pub fn load_index(path: &Path) -> std::collections::HashMap<String, Vec<SongEntry>> {
    let mut out: std::collections::HashMap<String, Vec<SongEntry>> = std::collections::HashMap::new();
    let Some(doc) = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<IndexFile>(&t).ok())
    else {
        return out;
    };
    if doc.version != INDEX_VERSION {
        return out;
    }
    for e in doc.songs {
        out.entry(e.path.clone()).or_default().push(e);
    }
    out
}

pub fn save_index(path: &Path, songs: &[SongEntry]) {
    let doc = IndexFile {
        version: INDEX_VERSION,
        songs: songs.to_vec(),
    };
    if let Ok(text) = serde_json::to_string(&doc) {
        let _ = write_atomic(path, &text);
    }
}

/// The song list; `start_scan` fills it in the background and reports progress.
#[derive(Default)]
pub struct Library {
    pub songs: Mutex<Arc<Vec<SongEntry>>>,
    /// Files found / read so far in the current scan.
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub scanning: AtomicBool,
    /// Goes up whenever `songs` changes.
    pub version: AtomicU64,
}

impl Library {
    pub fn new() -> Arc<Library> {
        Arc::new(Library::default())
    }

    /// The current list (cheap: shared).
    pub fn snapshot(&self) -> Arc<Vec<SongEntry>> {
        self.songs.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn publish(&self, songs: Vec<SongEntry>) {
        if let Ok(mut g) = self.songs.lock() {
            *g = Arc::new(songs);
        }
        self.version.fetch_add(1, Ordering::Release);
    }

    /// Scan `dirs` on a background thread (read what isn't in the index at `index`, save the index after).
    pub fn start_scan(self: &Arc<Self>, keys: Keys, dirs: Vec<PathBuf>, index: PathBuf) {
        if self.scanning.swap(true, Ordering::AcqRel) {
            return;
        }
        let lib = self.clone();
        std::thread::Builder::new()
            .name("qrs-library-scan".into())
            .spawn(move || {
                lib.scan(&keys, &dirs, &index);
                // the new songs first: whoever sees the scan finished sees them too
                lib.version.fetch_add(1, Ordering::Release);
                lib.scanning.store(false, Ordering::Release);
            })
            .ok();
    }

    /// Forget the index and scan everything again.
    pub fn rescan(self: &Arc<Self>, keys: Keys, dirs: Vec<PathBuf>, index: PathBuf) {
        let _ = std::fs::remove_file(&index);
        self.start_scan(keys, dirs, index);
    }

    /// The scan itself (blocking).
    pub fn scan(&self, keys: &Keys, dirs: &[PathBuf], index: &Path) {
        let cached = load_index(index);
        let files = find_song_files(dirs);
        self.total.store(files.len(), Ordering::Release);
        let mut fresh: Vec<SongEntry> = Vec::new();
        let mut stale: Vec<PathBuf> = Vec::new();
        for f in &files {
            let key = f.to_string_lossy().into_owned();
            match (cached.get(&key), stamp(f)) {
                (Some(c), Some((size, mtime))) if c[0].size == size && (c[0].mtime - mtime).abs() < 1.0 => {
                    fresh.extend(c.iter().cloned())
                }
                (_, Some(_)) => stale.push(f.clone()),
                _ => {}
            }
        }
        self.done.store(files.len() - stale.len(), Ordering::Release);
        self.publish(fresh.clone());
        if stale.is_empty() {
            if cached.len() != files.len() {
                save_index(index, &fresh); // files went away
            }
            return;
        }
        let next = AtomicUsize::new(0);
        let results: Mutex<Vec<SongEntry>> = Mutex::new(fresh);
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 8);
        let last_publish = Mutex::new(std::time::Instant::now());
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::AcqRel);
                        let Some(f) = stale.get(i) else { break };
                        let entries = read_entries(f, keys);
                        self.done.fetch_add(1, Ordering::AcqRel);
                        let mut r = results.lock().unwrap_or_else(|e| e.into_inner());
                        r.extend(entries);
                        let mut lp = last_publish.lock().unwrap_or_else(|e| e.into_inner());
                        if lp.elapsed().as_millis() > 250 {
                            *lp = std::time::Instant::now();
                            self.publish(r.clone());
                        }
                    }
                });
            }
        });
        let mut all = results.into_inner().unwrap_or_else(|e| e.into_inner());
        all.sort_by(|a, b| a.path.cmp(&b.path).then(a.song_key.cmp(&b.song_key)));
        save_index(index, &all);
        self.publish(all);
    }
}
