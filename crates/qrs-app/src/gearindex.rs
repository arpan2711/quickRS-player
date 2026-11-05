//! Which songs in the library use which pedalboard models (PLAN.md P21.4), and each song's own tones as boards
//! (P22.3, the amp room's Sounds tab). The editor's About card lists the songs whose RS tones are built from a
//! model; the room finds a song's tones by song, artist or part. Read in the background, once a session, from each
//! song's manifest (no audio, no chart), and kept in `.quickrs-cache/gear.json` by file and mtime, so the next
//! session only reads new or changed songs. New code (no Python source).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use qrs_audio::board::Board;
use qrs_formats::{Keys, Package};
use qrs_library::SongEntry;
use serde_json::{Map, Value, json};

/// Bumped when what's kept changes (the mapping from gear to models, say), so old caches are read again.
/// 2: each part's tones (P22.3). 3: each song's RS gear keys (P36.7, the setup's suggested captures). 4: the
/// guitar parts' and the bass parts' gear apart (P37.6a: a bass player's suggestions rank only bass gear).
const VERSION: u32 = 4;

/// One of a song part's tones.
#[derive(Clone, Debug, Default)]
struct Tone {
    /// The part's name ("Lead", "Rhythm", "Bass").
    part: String,
    /// The tone's key and short name ("AudBe_cleanlead", "clean lead").
    key: String,
    label: String,
    /// The board it maps to (as `Board::to_json`).
    board: Value,
}

#[derive(Clone, Debug, Default)]
struct Song {
    mtime: f64,
    title: String,
    artist: String,
    /// The model ids the song's tones use (every part), sorted.
    models: Vec<String>,
    /// RS's gear keys its guitar parts' tones use ("Amp_AT20", "Cab_TW110C_57_Cone"), sorted.
    gear: Vec<String>,
    /// Its bass parts' (P37.6a), sorted.
    bass_gear: Vec<String>,
    tones: Vec<Tone>,
}

/// The cache file: {"version": n, "songs": {id: {"mtime", "title", "artist", "models", "tones"}}}.
fn read_cache(path: &Path) -> HashMap<String, Song> {
    let Some(v) = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return HashMap::new();
    };
    if v.get("version").and_then(Value::as_u64) != Some(VERSION as u64) {
        return HashMap::new();
    }
    let Some(m) = v.get("songs").and_then(Value::as_object) else {
        return HashMap::new();
    };
    m.iter()
        .map(|(id, s)| {
            let text = |k: &str| s.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let song = Song {
                mtime: s.get("mtime").and_then(Value::as_f64).unwrap_or(-1.0),
                title: text("title"),
                artist: text("artist"),
                models: strings(s.get("models")),
                gear: strings(s.get("gear")),
                bass_gear: strings(s.get("bass_gear")),
                tones: s
                    .get("tones")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|t| {
                                let f = |k: &str| t.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                                Tone {
                                    part: f("part"),
                                    key: f("key"),
                                    label: f("label"),
                                    board: t.get("board").cloned().unwrap_or(Value::Null),
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            (id.clone(), song)
        })
        .collect()
}

/// A JSON array of strings (nothing for anything else).
fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn write_cache(path: &Path, songs: &HashMap<String, Song>) {
    let mut m = Map::new();
    for (id, s) in songs {
        let tones: Vec<Value> = s
            .tones
            .iter()
            .map(|t| json!({"part": t.part, "key": t.key, "label": t.label, "board": t.board}))
            .collect();
        m.insert(
            id.clone(),
            json!({"mtime": s.mtime, "title": s.title, "artist": s.artist, "models": s.models, "gear": s.gear,
                "bass_gear": s.bass_gear,
                "tones": tones}),
        );
    }
    let v = json!({"version": VERSION, "songs": Value::Object(m)});
    let _ = qrs_library::paths::write_atomic(path, &v.to_string());
}

#[derive(Default)]
struct Shared {
    songs: HashMap<String, Song>,
    /// (read so far, to read in all)
    progress: (usize, usize),
    running: bool,
    /// Goes up as songs come in (the room's search reads again).
    version: u64,
}

fn shared() -> &'static Arc<Mutex<Shared>> {
    static S: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();
    S.get_or_init(|| Arc::new(Mutex::new(Shared::default())))
}

/// Start reading the library (once a session; later calls do nothing).
pub fn start(songs: Arc<Vec<SongEntry>>, keys: Keys, cache: PathBuf) {
    {
        let Ok(mut s) = shared().lock() else { return };
        if s.running || s.progress.1 > 0 {
            return;
        }
        s.running = true;
    }
    let spawned = std::thread::Builder::new()
        .name("gear-index".into())
        .spawn(move || run(&songs, &keys, &cache));
    if spawned.is_err()
        && let Ok(mut s) = shared().lock()
    {
        s.running = false;
    }
}

/// What `read_song` finds: the models, the guitar parts' and the bass parts' gear keys, the tones.
type Read = (Vec<String>, Vec<String>, Vec<String>, Vec<Tone>);

/// A song's models, gear keys and tones, from its manifest.
fn read_song(e: &SongEntry, keys: &Keys) -> Option<Read> {
    let key = (!e.song_key.is_empty()).then_some(e.song_key.as_str());
    let p = Package::open(&e.path, keys, key).ok()?;
    let mut ids: Vec<String> = Vec::new();
    let mut gear: Vec<String> = Vec::new();
    let mut bass_gear: Vec<String> = Vec::new();
    let mut tones: Vec<Tone> = Vec::new();
    for a in &p.arrangements {
        for t in &a.rig {
            let keys = t.gear.iter().map(|g| g.key.clone());
            if a.is_bass {
                bass_gear.extend(keys);
            } else {
                gear.extend(keys);
            }
            let b = qrs_audio::board::rs_gear::board_for(t);
            ids.extend(b.blocks.iter().map(|x| x.def().id.to_string()));
            if !b.blocks.is_empty() {
                tones.push(Tone {
                    part: a.name.clone(),
                    key: t.key.clone(),
                    label: t.label(),
                    board: b.to_json(),
                });
            }
        }
    }
    ids.sort();
    ids.dedup();
    for g in [&mut gear, &mut bass_gear] {
        g.sort();
        g.dedup();
    }
    Some((ids, gear, bass_gear, tones))
}

fn run(songs: &[SongEntry], keys: &Keys, cache: &Path) {
    let old = read_cache(cache);
    let todo: Vec<&SongEntry> = songs
        .iter()
        .filter(|e| e.error.is_empty() && !e.needs_keys)
        .collect();
    if let Ok(mut s) = shared().lock() {
        s.progress = (0, todo.len());
    }
    let mut out: HashMap<String, Song> = HashMap::new();
    let mut fresh = 0usize;
    for (i, e) in todo.iter().enumerate() {
        let id = e.id();
        let song = match old.get(&id) {
            Some(s) if s.mtime == e.mtime => s.clone(),
            _ => {
                fresh += 1;
                let (models, gear, bass_gear, tones) = read_song(e, keys).unwrap_or_default();
                Song {
                    mtime: e.mtime,
                    title: e.display_title(),
                    artist: e.artist.clone(),
                    models,
                    gear,
                    bass_gear,
                    tones,
                }
            }
        };
        out.insert(id.clone(), song.clone());
        if let Ok(mut s) = shared().lock() {
            s.songs.insert(id, song);
            s.progress.0 = i + 1;
            if i % 64 == 0 {
                s.version += 1;
            }
        }
    }
    if fresh > 0 || out.len() != old.len() {
        write_cache(cache, &out);
    }
    if let Ok(mut s) = shared().lock() {
        s.running = false;
        s.version += 1;
    }
    crate::applog::write(&format!("gear index: {} songs ({fresh} read)", todo.len()));
}

/// A song that uses a model: (song id, title, artist).
pub type SongRef = (String, String, String);

/// The songs read so far that use `model`, by title.
pub fn songs_with(model: &str) -> Vec<SongRef> {
    let Ok(s) = shared().lock() else {
        return Vec::new();
    };
    let mut v: Vec<SongRef> = s
        .songs
        .iter()
        .filter(|(_, x)| x.models.iter().any(|m| m == model))
        .map(|(id, x)| (id.clone(), x.title.clone(), x.artist.clone()))
        .collect();
    v.sort_by_key(|a| a.1.to_lowercase());
    v
}

/// Each song's RS gear keys (the songs read so far): the suggested captures' ranking (P36.7). `bass`: its
/// bass parts' only, else its guitar parts' only (P37.6a); a song without such a part isn't listed.
pub fn song_gear(bass: bool) -> Vec<Vec<String>> {
    let Ok(s) = shared().lock() else {
        return Vec::new();
    };
    s.songs
        .values()
        .map(|x| if bass { x.bass_gear.clone() } else { x.gear.clone() })
        .filter(|g| !g.is_empty())
        .collect()
}

/// (songs read, songs to read), while the library is being read.
pub fn reading() -> Option<(usize, usize)> {
    let s = shared().lock().ok()?;
    s.running.then_some(s.progress)
}

/// Changes as songs are read (a search made at one version is still good at it).
pub fn version() -> u64 {
    shared().lock().map(|s| s.version).unwrap_or(0)
}

/// A song's tone, found.
#[derive(Clone, Debug)]
pub struct SongTone {
    pub title: String,
    pub artist: String,
    pub part: String,
    pub label: String,
    pub board: Board,
}

/// Every word of `q` in the song's title, artist, part, tone or gear (any case). Returns the first `limit`
/// matches (by artist, title, part) and how many there are.
pub fn find_tones(q: &str, limit: usize) -> (Vec<SongTone>, usize) {
    let Ok(s) = shared().lock() else {
        return (Vec::new(), 0);
    };
    let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<(&Song, &Tone)> = Vec::new();
    for song in s.songs.values() {
        let head = format!("{} {}", song.title, song.artist).to_lowercase();
        for t in &song.tones {
            let ok = words.iter().all(|w| {
                head.contains(w.as_str())
                    || t.part.to_lowercase().contains(w.as_str())
                    || t.label.to_lowercase().contains(w.as_str())
            });
            if ok {
                hits.push((song, t));
            }
        }
    }
    hits.sort_by_cached_key(|(song, t)| {
        (
            song.artist.to_lowercase(),
            song.title.to_lowercase(),
            t.part.clone(),
            t.key.clone(),
        )
    });
    let total = hits.len();
    let out = hits
        .into_iter()
        .take(limit)
        .map(|(song, t)| SongTone {
            title: song.title.clone(),
            artist: song.artist.clone(),
            part: t.part.clone(),
            label: t.label.clone(),
            board: crate::captures::nam_of_board(Board::from_json(&t.board), &t.part),
        })
        .collect();
    (out, total)
}

/// The tones of these songs, in their order (at most `limit`).
pub fn tones_of(sids: &[String], limit: usize) -> Vec<SongTone> {
    let Ok(s) = shared().lock() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for sid in sids {
        let Some(song) = s.songs.get(sid) else { continue };
        for t in &song.tones {
            if out.len() >= limit {
                return out;
            }
            out.push(SongTone {
                title: song.title.clone(),
                artist: song.artist.clone(),
                part: t.part.clone(),
                label: t.label.clone(),
                board: crate::captures::nam_of_board(Board::from_json(&t.board), &t.part),
            });
        }
    }
    out
}

/// Screenshots: songs to show without reading the library (title, artist, part, tone, board).
pub fn stage(tones: Vec<(String, String, String, String, Board)>) {
    let Ok(mut s) = shared().lock() else { return };
    for (i, (title, artist, part, label, b)) in tones.into_iter().enumerate() {
        let id = format!("shot-{title}|{artist}");
        let e = s.songs.entry(id).or_insert_with(|| Song {
            mtime: 0.0,
            title: title.clone(),
            artist: artist.clone(),
            ..Default::default()
        });
        e.tones.push(Tone {
            part,
            key: format!("shot_{i}"),
            label,
            board: b.to_json(),
        });
    }
    s.version += 1;
}
