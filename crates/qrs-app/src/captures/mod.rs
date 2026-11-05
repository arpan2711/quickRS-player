//! Your NAM captures and IR cabs (new in quickRS, P31, D80): the files the board's `amp_nam` and `cab_ir` blocks run,
//! kept in `captures/` in the data folder with an index, `captures.json` (not config.json: that keeps the Python
//! app's shape). Added from a file on disk or picked on TONE3000 (`tone3000`). At start every file is registered with
//! the audio side (`qrs_audio::board::nam`) by its id, which a block keeps (`Block::asset`), so a board saved with a
//! capture finds it again. Only in the owner's build for now (the `nam` feature, `ON`).
//!
//! captures.json: {"next": id, "items": [{id, kind ("nam" | "ir"), name, file (in captures/), added (s since
//! 1970), source ("file" | "tone3000"), creator, licence, tone_id, tone_url, gear, cab (true/false/absent),
//! size, songs (titles it's set up for, lower case), ir (the IR it goes with, for a capture without its cab)}]};
//! kind "pedal" is a NAM capture of a pedal (the board's NAM pedal, before the amp).
//! The file is read again when it changes on disk (`sync`), so a set put there from outside shows up.

pub mod auto;
pub mod suggest;
pub use suggest::suggested_captures;
pub mod tone3000;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use qrs_audio::board::nam;
use serde_json::{Map, Value, json};

/// NAM captures and TONE3000 in this build (the owner's; off in the shared builds for now, D80).
pub const ON: bool = cfg!(feature = "nam");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A NAM capture of an amp (or a whole rig).
    Nam,
    /// A NAM capture of a pedal.
    Pedal,
    Ir,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Nam => "nam",
            Kind::Pedal => "pedal",
            Kind::Ir => "ir",
        }
    }

    /// The board model that plays it.
    pub fn model(self) -> &'static str {
        match self {
            Kind::Nam => "amp_nam",
            Kind::Pedal => "pedal_nam",
            Kind::Ir => "cab_ir",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u32,
    pub kind: Kind,
    pub name: String,
    pub file: String,
    pub added: f64,
    pub source: String,
    pub creator: Option<String>,
    pub licence: Option<String>,
    pub tone_id: Option<u64>,
    pub tone_url: Option<String>,
    pub gear: Option<String>,
    /// A capture with its cab in it (no cab after it), when it says.
    pub cab: Option<bool>,
    pub size: Option<String>,
    /// The songs it's set up for (titles, lower case): NAM on a song's board picks one of these first.
    pub songs: Vec<String>,
    /// The IR it goes with (a capture without its cab): put on the board's IR cab with it.
    pub ir: Option<u32>,
    /// Our model it stands in for (P33): drawn as it, and picked for songs whose tone has it.
    pub skin: Option<String>,
    /// The RS gear it is the real unit of (amp and pedal keys, cab bodies): picked first for them.
    pub rs: Vec<String>,
    /// Its gain, 0-10 (a capture is one setting of the real knobs): the picker takes the nearest.
    pub gain: Option<f32>,
    /// Bass gear.
    pub bass: bool,
    /// Its level trim, dB (measured against the model it stands in for: `calibrate`).
    pub trim: Option<f32>,
    /// How its trim was measured (`TRIM_V`: 2 also levels a gain ladder's steps against each other, P37.6c).
    pub trim_v: u32,
}

/// The trim measurement's version: a capture trimmed by an older one is measured again at start.
pub const TRIM_V: u32 = 2;

impl Item {
    fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("kind".into(), json!(self.kind.key()));
        m.insert("name".into(), json!(self.name));
        m.insert("file".into(), json!(self.file));
        m.insert("added".into(), json!(self.added));
        m.insert("source".into(), json!(self.source));
        let opt = |m: &mut Map<String, Value>, k: &str, v: &Option<String>| {
            if let Some(v) = v {
                m.insert(k.into(), json!(v));
            }
        };
        opt(&mut m, "creator", &self.creator);
        opt(&mut m, "licence", &self.licence);
        opt(&mut m, "tone_url", &self.tone_url);
        opt(&mut m, "gear", &self.gear);
        opt(&mut m, "size", &self.size);
        if let Some(t) = self.tone_id {
            m.insert("tone_id".into(), json!(t));
        }
        if let Some(c) = self.cab {
            m.insert("cab".into(), json!(c));
        }
        if !self.songs.is_empty() {
            m.insert("songs".into(), json!(self.songs));
        }
        if let Some(i) = self.ir {
            m.insert("ir".into(), json!(i));
        }
        if let Some(k) = &self.skin {
            m.insert("skin".into(), json!(k));
        }
        if !self.rs.is_empty() {
            m.insert("rs".into(), json!(self.rs));
        }
        if let Some(g) = self.gain {
            m.insert("gain".into(), json!(g));
        }
        if self.bass {
            m.insert("bass".into(), json!(true));
        }
        if let Some(t) = self.trim {
            m.insert("trim".into(), json!((t * 10.0).round() / 10.0));
            if self.trim_v > 0 {
                m.insert("trim_v".into(), json!(self.trim_v));
            }
        }
        Value::Object(m)
    }

    /// The model of ours a capture "sounds like", made consistent across a unit's captures (P37.6c, D141): a
    /// channel-switching Fender (the Hot Rods) is our clean Twin on its clean captures and our 800 on its drive
    /// channel, whichever list harvested it (older captures said Twin for all, newer ones 800 for all).
    pub fn fix_skin(&mut self) {
        if self.kind != Kind::Nam {
            return;
        }
        let hay = format!("{} {}", self.name, self.gear.as_deref().unwrap_or("")).to_lowercase();
        if hay.contains("hot rod") {
            let clean = hay.contains("clean") || self.gain.is_some_and(|g| g <= 4.5);
            self.skin = Some(if clean { "amp_clean" } else { "amp_lead" }.into());
            return;
        }
        // (P41.1, D165, the tech's #8) an Ampeg B-18 or B-15 is a Portaflex: our B-15, not the SVT
        if self.bass
            && ["b-18", "b18", "b-15", "b15", "portaflex"]
                .iter()
                .any(|w| hay.contains(w))
        {
            self.skin = Some("amp_b15".into());
            return;
        }
        // the clean channel of a channel-switching high-gain amp (a Bogner, a Mark IV or V, a DSL, a JCM900 or 2000) is
        // a clean amp: our Twin, not our 800 at gain 2. A single-channel amp turned down (a JCM800, a Plexi) stays
        if self
            .skin
            .as_deref()
            .is_some_and(|s| s == "amp_lead" || s == "amp_modern")
            && hay.contains("clean")
            && self.gain.is_some_and(|g| g <= 3.0)
            && ![
                "jcm800",
                "jcm 800",
                "plexi",
                "super lead",
                "jmp",
                "jubilee",
                "crunch",
                " od",
                "overdrive",
            ]
            .iter()
            .any(|w| hay.contains(w))
        {
            self.skin = Some("amp_clean".into());
            return;
        }
        // (P41.6) a Mark, an SLO or 5150, an Orange or a Hiwatt has its own voice of ours since P41.2: a capture of one
        // sounds like it (and flips to it), not like the 800 or the Rectifier
        if self
            .skin
            .as_deref()
            .is_some_and(|s| s == "amp_lead" || s == "amp_modern" || s == "amp_crunch")
        {
            let has = |ws: &[&str]| ws.iter().any(|w| hay.contains(w));
            let voice = if has(&[
                "mark ii",
                "mark iv",
                "mark v",
                "mark i ",
                "iic+",
                "boogie mark",
                "mesa mark",
            ]) {
                Some("amp_cali")
            } else if has(&["soldano", "slo-100", "slo 100", "slo100", "5150", "6505", "evh"]) {
                Some("amp_snake")
            } else if has(&["orange", "rockerverb", "tiny terror", "or50", "or100", "ad30"]) {
                Some("amp_tangerine")
            } else if has(&["hiwatt", "dr103", "dr 103"]) {
                Some("amp_surrey")
            } else {
                None
            };
            if let Some(v) = voice {
                self.skin = Some(v.into());
            }
        }
    }

    fn from_json(v: &Value) -> Option<Item> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Item {
            id: v.get("id")?.as_u64()? as u32,
            kind: match v.get("kind")?.as_str()? {
                "nam" => Kind::Nam,
                "pedal" => Kind::Pedal,
                "ir" => Kind::Ir,
                _ => return None,
            },
            name: s("name").unwrap_or_default(),
            file: s("file")?,
            added: v.get("added").and_then(Value::as_f64).unwrap_or(0.0),
            source: s("source").unwrap_or_else(|| "file".into()),
            creator: s("creator"),
            licence: s("licence"),
            tone_id: v.get("tone_id").and_then(Value::as_u64),
            tone_url: s("tone_url"),
            gear: s("gear"),
            cab: v.get("cab").and_then(Value::as_bool),
            size: s("size"),
            songs: v
                .get("songs")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_lowercase)
                        .collect()
                })
                .unwrap_or_default(),
            ir: v.get("ir").and_then(Value::as_u64).map(|i| i as u32),
            skin: s("skin"),
            rs: v
                .get("rs")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
            gain: v.get("gain").and_then(Value::as_f64).map(|g| g as f32),
            bass: v.get("bass").and_then(Value::as_bool).unwrap_or(false),
            trim: v.get("trim").and_then(Value::as_f64).map(|t| t as f32),
            trim_v: v.get("trim_v").and_then(Value::as_u64).unwrap_or(0) as u32,
        })
    }

    /// The line under its name: who made it, the gear, where it came from.
    pub fn detail(&self) -> String {
        let mut bits: Vec<String> = Vec::new();
        if let Some(g) = &self.gear
            && !self.name.to_lowercase().contains(&g.to_lowercase())
        {
            bits.push(g.clone());
        }
        if let Some(c) = &self.creator {
            bits.push(format!("by {c}"));
        }
        if let Some(s) = &self.size {
            bits.push(s.clone());
        }
        if self.kind == Kind::Nam {
            match self.cab {
                Some(true) => bits.push("cab included".into()),
                Some(false) => bits.push("no cab: add an IR".into()),
                None => {}
            }
        }
        if self.source == "tone3000" {
            bits.push(match &self.licence {
                Some(l) => format!("TONE3000 \u{b7} {}", licence_name(l)),
                None => "TONE3000".into(),
            });
        }
        bits.join(" \u{b7} ")
    }
}

/// A TONE3000 licence key as people say it.
pub fn licence_name(l: &str) -> String {
    match l {
        "t3k" => "T3K licence".into(),
        "cco" | "cc0" => "CC0".into(),
        other => other.to_uppercase().replace("CC-", "CC "),
    }
}

#[derive(Default)]
pub struct Library {
    dir: PathBuf,
    next: u32,
    pub items: Vec<Item>,
    /// captures.json's time when last read or written (`sync` reads it again when it differs).
    stamp: Option<std::time::SystemTime>,
}

fn lib() -> &'static Mutex<Library> {
    static L: OnceLock<Mutex<Library>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(Library::default()))
}

/// The library, to read or change.
pub fn with<R>(f: impl FnOnce(&mut Library) -> R) -> R {
    f(&mut lib().lock().unwrap_or_else(|e| e.into_inner()))
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A file name from a name (for the checker's WAVs).
pub fn safe_name(name: &str) -> String {
    safe(name)
}

/// A file name from a name: letters, digits, spaces and dashes, short.
fn safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.chars().take(60).collect();
    if s.is_empty() { "capture".into() } else { s }
}

impl Library {
    /// The library in `data_root/captures`, every file registered for the board (off the UI thread is better: a
    /// big library parses for a moment).
    pub fn open(data_root: &Path) -> Library {
        let dir = data_root.join("captures");
        let v = std::fs::read_to_string(dir.join("captures.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
        let items: Vec<Item> = v
            .get("items")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Item::from_json)
                    .map(|mut i| {
                        i.fix_skin();
                        i
                    })
                    .collect()
            })
            .unwrap_or_default();
        let next = v
            .get("next")
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .unwrap_or(1)
            .max(items.iter().map(|i| i.id + 1).max().unwrap_or(1));
        let stamp = std::fs::metadata(dir.join("captures.json"))
            .and_then(|m| m.modified())
            .ok();
        let lib = Library {
            dir,
            next,
            items,
            stamp,
        };
        for it in &lib.items {
            if let Err(e) = lib.register(it) {
                crate::applog::write(&format!("captures: {} not loaded: {e}", it.file));
            }
        }
        lib
    }

    fn register(&self, it: &Item) -> Result<(), String> {
        let path = self.dir.join(&it.file);
        nam::set_trim(it.id, it.trim.unwrap_or(0.0));
        // the gain ladder (P37.2): the captures of one TONE3000 tone and kind are settings of one unit
        if let (Some(tone), Some(gain)) = (it.tone_id, it.gain) {
            let kind = match it.kind {
                Kind::Nam => 0,
                Kind::Pedal => 1,
                Kind::Ir => 2,
            };
            nam::set_ladder(it.id, tone * 4 + kind, gain);
        }
        match it.kind {
            Kind::Nam | Kind::Pedal => {
                let json = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                nam::register_nam(it.id, &json).map(|_| ())
            }
            Kind::Ir => {
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                nam::register_ir(it.id, &bytes).map(|_| ())
            }
        }
    }

    fn save(&mut self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let v = json!({"next": self.next, "items": self.items.iter().map(Item::to_json).collect::<Vec<_>>()});
        let tmp = self.dir.join("captures.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default())?;
        std::fs::rename(&tmp, self.dir.join("captures.json"))?;
        self.stamp = std::fs::metadata(self.dir.join("captures.json"))
            .and_then(|m| m.modified())
            .ok();
        Ok(())
    }

    /// Read captures.json again if it changed on disk since (a set added from outside the app).
    pub fn sync(&mut self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        let now = std::fs::metadata(self.dir.join("captures.json"))
            .and_then(|m| m.modified())
            .ok();
        if now.is_some() && now != self.stamp {
            let root = self.dir.parent().map(Path::to_path_buf).unwrap_or_default();
            *self = Library::open(&root);
        }
    }

    /// What to put on a song's board: the newest of a kind set up for the song, else (`any`) the newest.
    pub fn for_song(&self, kind: Kind, title: &str, any: bool) -> Option<&Item> {
        let t = title.trim().to_lowercase();
        let newest = |a: &&Item, b: &&Item| a.added.total_cmp(&b.added);
        let tagged = self
            .of(kind)
            .filter(|i| !t.is_empty() && i.songs.contains(&t))
            .max_by(newest);
        if any {
            tagged.or_else(|| self.of(kind).max_by(newest))
        } else {
            tagged
        }
    }

    /// Keep measured trims (and save the list).
    pub fn set_trims(&mut self, trims: &[(u32, f32)]) {
        for &(id, t) in trims {
            if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
                i.trim = Some(t);
                i.trim_v = TRIM_V;
            }
        }
        if let Err(e) = self.save() {
            crate::applog::write(&format!("captures: trims not saved: {e}"));
        }
    }

    /// The TONE3000 key saved beside the library (captures/tone3000.json), if any.
    pub fn key(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join("tone3000.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("client_id").and_then(Value::as_str).map(str::to_string))
    }

    pub fn get(&self, id: u32) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn of(&self, kind: Kind) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(move |i| i.kind == kind)
    }

    /// Add a file's bytes (checked first: a capture must parse and run, an IR decode), registered at once. `meta`
    /// fills in what the file doesn't say (TONE3000's tone).
    pub fn add(&mut self, kind: Kind, name: &str, bytes: &[u8], mut meta: Item) -> Result<Item, String> {
        let id = self.next;
        match kind {
            Kind::Nam | Kind::Pedal => {
                let json =
                    std::str::from_utf8(bytes).map_err(|_| "not a NAM capture (not text)".to_string())?;
                let info = nam::register_nam(id, json)?;
                if meta.gear.is_none() {
                    meta.gear = info.gear.clone();
                }
                if meta.creator.is_none() {
                    meta.creator = info.modeled_by.clone();
                }
                if meta.cab.is_none() && kind == Kind::Nam {
                    meta.cab = info.includes_cab;
                }
            }
            Kind::Ir => {
                nam::register_ir(id, bytes)?;
            }
        }
        let ext = if kind == Kind::Ir { "wav" } else { "nam" };
        let file = format!("{id} {}.{ext}", safe(name));
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.dir.join(&file);
        if let Err(e) = std::fs::write(&path, bytes) {
            nam::unregister(id);
            return Err(format!("couldn't save it: {e}"));
        }
        self.next = id + 1;
        let it = Item {
            id,
            kind,
            name: name.trim().to_string(),
            file,
            added: now(),
            ..meta
        };
        self.items.push(it.clone());
        self.save().map_err(|e| format!("couldn't save the list: {e}"))?;
        Ok(it)
    }

    /// Add a .nam or .wav from disk; a .nam as `as_` when given (the dialog's tab), else from its metadata.
    pub fn add_path_as(&mut self, path: &Path, as_: Option<Kind>) -> Result<Item, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let info = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|j| nam::inspect(j).ok());
        let kind = if ext == "wav" {
            Kind::Ir
        } else if let Some(k) = as_.filter(|k| *k != Kind::Ir) {
            k
        } else if gear_type(&bytes).as_deref() == Some("pedal") {
            Kind::Pedal
        } else {
            Kind::Nam
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("capture");
        let name = if kind != Kind::Ir {
            info.and_then(|i| i.name)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| stem.to_string())
        } else {
            stem.to_string()
        };
        self.add(kind, &name, &bytes, blank(kind))
    }

    pub fn remove(&mut self, id: u32) {
        if let Some(i) = self.items.iter().position(|x| x.id == id) {
            let it = self.items.remove(i);
            let _ = std::fs::remove_file(self.dir.join(&it.file));
            nam::unregister(id);
            let _ = self.save();
        }
    }
}

/// A .nam file's own word on what it is ("amp", "pedal", ...), from its metadata.
fn gear_type(bytes: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    v.get("metadata")?
        .get("gear_type")?
        .as_str()
        .map(str::to_lowercase)
}

/// An item with nothing filled in (`add` fills it).
pub fn blank(kind: Kind) -> Item {
    Item {
        id: 0,
        kind,
        name: String::new(),
        file: String::new(),
        added: 0.0,
        source: "file".into(),
        creator: None,
        licence: None,
        tone_id: None,
        tone_url: None,
        gear: None,
        cab: None,
        size: None,
        songs: Vec::new(),
        ir: None,
        skin: None,
        rs: Vec::new(),
        gain: None,
        bass: false,
        trim: None,
        trim_v: 0,
    }
}

static LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Songs' tones are made of captures (config.json `nam_songs`, on unless set false; P33): else of our models.
static SONGS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn songs_use_nam() -> bool {
    ON && SONGS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_songs_use_nam(on: bool) {
    SONGS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A song tone's board: of captures (`auto`) when songs use NAM, else of our models.
/// (P41.1: for a song, whose own captures and the amp on its record come first.)
pub fn song_board_in(tone: &qrs_formats::rig::RigTone, song: &auto::SongHint) -> qrs_audio::board::Board {
    if songs_use_nam() {
        ready();
        with(|l| auto::nam_board_for(tone, song, l))
    } else {
        qrs_audio::board::rs_gear::board_for(tone)
    }
}

/// A board's captures whose files aren't loaded (deleted, moved, another computer's, or a build without NAM) fall
/// back to models of ours instead of passing the guitar through dry (P38.2, D154): a missing amp capture turns off
/// and the amp of ours beside it (a song's NAM board keeps it, `auto::swap`) plays, or the model it stands in for
/// (its skin; our clean amp when nothing says) is put in after it, with a cab; a missing IR gives way to our cab, a
/// missing NAM pedal to its drive of ours. Returns how many fell back (the room says so). Nothing happens while the
/// library is still opening.
pub fn heal(board: &mut qrs_audio::board::Board) -> usize {
    if !board.blocks.iter().any(|b| b.def().file && b.on) {
        return 0;
    }
    ready();
    if ON && !is_ready() {
        return 0;
    }
    heal_with(
        board,
        |b| !nam::is_registered(b.asset),
        crate::player::board::captures::skin_of,
    )
}

/// The file blocks on a board whose files aren't loaded (on or off): the room's "capture missing" mark.
pub fn missing_on(board: &qrs_audio::board::Board) -> usize {
    if ON && !is_ready() {
        return 0;
    }
    board
        .blocks
        .iter()
        .filter(|b| b.def().file && !nam::is_registered(b.asset))
        .count()
}

/// `heal`, with what counts as missing and where skins come from (tests).
pub fn heal_with(
    board: &mut qrs_audio::board::Board,
    missing: impl Fn(&qrs_audio::board::Block) -> bool,
    skin: impl Fn(u32) -> Option<String>,
) -> usize {
    use crate::player::board::captures::{engage_with, has_cab};
    use qrs_audio::board::{Block, Kind as BKind, MAX_BLOCKS, catalog};
    let mut n = 0;
    let mut i = 0;
    while i < board.blocks.len() {
        let b = board.blocks[i];
        if !(b.def().file && b.on && missing(&b)) {
            i += 1;
            continue;
        }
        n += 1;
        board.blocks[i].on = false;
        let kind = b.def().kind;
        // the unit of ours to fall back to: one beside it (after, then anywhere) that's off, of the same kind
        let same = |x: &Block| {
            !x.def().file
                && !x.on
                && match kind {
                    BKind::Drive | BKind::Fuzz => matches!(x.def().kind, BKind::Drive | BKind::Fuzz),
                    k => x.def().kind == k,
                }
        };
        let near = board
            .blocks
            .get(i + 1)
            .filter(|x| same(x))
            .map(|_| i + 1)
            .or_else(|| {
                (kind != BKind::Drive && kind != BKind::Fuzz)
                    .then(|| board.blocks.iter().position(same))
                    .flatten()
            });
        let uid = match near {
            Some(j) => {
                board.blocks[j].on = true;
                board.blocks[j].uid
            }
            None => {
                let fallback = match kind {
                    BKind::Amp => "amp_clean",
                    BKind::Cab => "cab_1x12",
                    _ => "green",
                };
                let id = skin(b.asset)
                    .filter(|s| {
                        catalog::find(s).is_some_and(|m| {
                            qrs_audio::board::MODELS[m].kind == kind
                                || (matches!(kind, BKind::Drive | BKind::Fuzz)
                                    && matches!(qrs_audio::board::MODELS[m].kind, BKind::Drive | BKind::Fuzz))
                        })
                    })
                    .unwrap_or_else(|| fallback.to_string());
                let Some(mut nb) = Block::of(&id) else {
                    i += 1;
                    continue;
                };
                nb.on = true;
                let uid = nb.uid;
                if board.blocks.len() < MAX_BLOCKS {
                    board.blocks.insert(i + 1, nb);
                } else {
                    board.blocks[i] = nb;
                }
                // (an amp that comes in with no cab of ours on the board gets one)
                if kind == BKind::Amp
                    && !board
                        .blocks
                        .iter()
                        .any(|x| x.def().kind == BKind::Cab && !x.def().file)
                    && board.blocks.len() < MAX_BLOCKS
                    && let Some(c) = Block::of("cab_1x12")
                {
                    let at = board.index_of(uid).map(|k| k + 1).unwrap_or(board.blocks.len());
                    board.blocks.insert(at, c.with("mic", 2.5));
                }
                uid
            }
        };
        if kind == BKind::Amp {
            engage_with(board, uid, has_cab, &skin);
        }
        i += 1;
    }
    n
}

/// One of our boards for a song's part (the gear index's tones) as songs play it: NAM when songs use NAM.
pub fn nam_of_board(board: qrs_audio::board::Board, part: &str) -> qrs_audio::board::Board {
    if songs_use_nam() {
        ready();
        let bass = part.to_lowercase().contains("bass");
        with(|l| auto::nam_of(board, bass, l))
    } else {
        board
    }
}

/// The library's place and settings, with nothing started (the checker opens it itself).
pub fn init_quiet(data_root: &Path, config: &qrs_library::Config) {
    set_songs_use_nam(config.get("nam_songs").and_then(Value::as_bool).unwrap_or(true));
    set_reference(config);
    let _ = ROOT.set(data_root.to_path_buf());
}

/// Open the library at start (on a thread: the board finds captures as soon as they're registered).
pub fn init(data_root: &Path, config: &qrs_library::Config) {
    if !ON {
        return;
    }
    set_songs_use_nam(config.get("nam_songs").and_then(Value::as_bool).unwrap_or(true));
    set_reference(config);
    let _ = ROOT.set(data_root.to_path_buf());
    std::thread::spawn(|| {
        ready();
        // (captures new to the library get their level trim, measured here: a few tenths of a second each)
        let n = crate::namcheck::calibrate(false);
        if n > 0 {
            crate::applog::write(&format!("captures: {n} level trims measured"));
        }
    });
}

/// The interface reference level captures are driven at (config.json `nam_input_dbu`, Settings › Neural Amp
/// Modeler; +12 dBu unless set).
fn set_reference(config: &qrs_library::Config) {
    if let Some(v) = config.get("nam_input_dbu").and_then(Value::as_f64) {
        nam::set_input_dbu(v as f32);
    }
}

/// Read the library again from its folder (Settings › Neural Amp Modeler's Read again: captures.json changed by
/// hand, or files put back).
pub fn reload() {
    if !ON {
        return;
    }
    let Some(root) = ROOT.get() else { return };
    let l = Library::open(root);
    with(|x| *x = l);
    LOADED.store(true, std::sync::atomic::Ordering::Release);
}

/// Whether the library has been opened (it opens on a thread at start: a few seconds for hundreds of captures).
pub fn is_ready() -> bool {
    LOADED.load(std::sync::atomic::Ordering::Acquire)
}

/// The library is open (opened here, on this thread, if the start-up one hasn't yet: a song's tone needs it).
pub fn ready() {
    use std::sync::atomic::Ordering;
    if !ON || LOADED.load(Ordering::Acquire) {
        return;
    }
    let Some(root) = ROOT.get() else { return };
    // (opened outside the library's lock: the UI reads names from it every frame meanwhile)
    static OPENING: Mutex<()> = Mutex::new(());
    let _g = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    if LOADED.load(Ordering::Acquire) {
        return;
    }
    let l = Library::open(root);
    with(|x| *x = l);
    LOADED.store(true, Ordering::Release);
}

/// Read the library again if its file changed on disk.
pub fn sync() {
    with(Library::sync);
}

/// A capture's or IR's name, for a block (None: not in the library).
pub fn name_of(id: u32) -> Option<String> {
    with(|l| l.get(id).map(|i| i.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_audio::board::{Block, Board, Kind as BKind};

    fn ids(b: &Board) -> Vec<(&'static str, bool)> {
        b.blocks.iter().map(|x| (x.def().id, x.on)).collect()
    }

    #[test]
    fn a_missing_capture_falls_back_to_the_model_beside_it() {
        // a song's NAM board: the capture (with its cab), our amp off beside it, our cab off
        let mut nam = Block::of("amp_nam").unwrap();
        nam.asset = 900_001;
        let mut ours = Block::of("amp_lead").unwrap().with("gain", 7.0);
        ours.on = false;
        let mut cab = Block::of("cab_4x12").unwrap();
        cab.on = false;
        let mut b = Board::new(vec![nam, ours, cab]);
        assert_eq!(heal_with(&mut b, |_| true, |_| None), 1);
        assert_eq!(
            ids(&b),
            vec![("amp_nam", false), ("amp_lead", true), ("cab_4x12", true)]
        );
        assert_eq!(b.blocks[1].get("gain"), Some(7.0), "the song's knobs");
        // a file that's there: nothing changes
        let mut b2 = Board::new(vec![nam, ours, cab]);
        assert_eq!(heal_with(&mut b2, |_| false, |_| None), 0);
        assert!(b2.blocks[0].on);
    }

    #[test]
    fn a_missing_capture_with_nothing_beside_it_gets_its_skin_and_a_cab() {
        let mut nam = Block::of("amp_nam").unwrap();
        nam.asset = 900_002;
        let mut ir = Block::of("cab_ir").unwrap();
        ir.asset = 900_003;
        let mut b = Board::new(vec![nam, ir]);
        let n = heal_with(
            &mut b,
            |_| true,
            |a| (a == 900_002).then(|| "amp_crunch".to_string()),
        );
        assert!(n >= 1);
        let on: Vec<&str> = b.blocks.iter().filter(|x| x.on).map(|x| x.def().id).collect();
        assert!(on.contains(&"amp_crunch"), "{:?}", ids(&b));
        assert!(
            b.blocks
                .iter()
                .any(|x| x.on && x.def().kind == BKind::Cab && !x.def().file),
            "{:?}",
            ids(&b)
        );
        assert!(!b.blocks.iter().any(|x| x.on && x.def().file), "{:?}", ids(&b));
        // a missing NAM pedal gives way to the drive of ours after it
        let mut p = Block::of("pedal_nam").unwrap();
        p.asset = 900_004;
        let mut d = Block::of("green").unwrap();
        d.on = false;
        let mut b = Board::new(vec![
            p,
            d,
            Block::of("amp_lead").unwrap(),
            Block::of("cab_4x12").unwrap(),
        ]);
        assert_eq!(heal_with(&mut b, |_| true, |_| None), 1);
        assert_eq!(ids(&b)[..2], [("pedal_nam", false), ("green", true)]);
    }

    fn tiny() -> String {
        r#"{"version": "0.5.4", "architecture": "WaveNet", "sample_rate": 48000,
            "metadata": {"name": "Tiny Amp", "gear_make": "Test", "gear_model": "One", "gear_type": "amp"},
            "config": {"layers": [{"input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                "kernel_size": 1, "dilations": [1], "activation": "ReLU", "gated": false, "head_bias": false}],
                "head": null, "head_scale": 1.0},
            "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]}"#
            .to_string()
    }

    #[test]
    fn a_hot_rod_sounds_like_one_model_per_channel() {
        let mut a = blank(Kind::Nam);
        a.name = "Hot Rod Deluxe III \u{b7} Drive 5".into();
        a.gain = Some(5.0);
        a.skin = Some("amp_clean".into());
        a.fix_skin();
        assert_eq!(a.skin.as_deref(), Some("amp_lead"));
        let mut b = blank(Kind::Nam);
        b.name = "2015 Fender Hot Rod Deluxe III \u{b7} edge Funky Clean".into();
        b.gain = Some(4.0);
        b.skin = Some("amp_lead".into());
        b.fix_skin();
        assert_eq!(b.skin.as_deref(), Some("amp_clean"));
    }

    /// P41.1 (D165): clean channels flip to the Twin, a B-18 to the B-15; a JCM800 turned down stays the 800.
    #[test]
    fn clean_channels_sound_like_the_twin_and_a_b18_like_the_b15() {
        let cap = |name: &str, gear: &str, skin: &str, gain: f32, bass: bool| {
            let mut a = blank(Kind::Nam);
            a.name = name.into();
            a.gear = Some(gear.into());
            a.skin = Some(skin.into());
            a.gain = Some(gain);
            a.bass = bass;
            a.fix_skin();
            a.skin.unwrap()
        };
        for (n, g) in [
            (
                "Bogner Ecstasy 101B \u{b7} clean Fender Clean",
                "Bogner Ecstasy 101B",
            ),
            (
                "Mesa Boogie Mark V \u{b7} clean Ch 1 Fat CLN SC",
                "Mesa Boogie Mark V",
            ),
            (
                "Marshall DSL100H \u{b7} clean CH Classic Clean",
                "Marshall DSL100H",
            ),
            ("Marshall JCM900 \u{b7} clean CLEAN 01", "Marshall JCM900"),
        ] {
            assert_eq!(cap(n, g, "amp_lead", 2.0, false), "amp_clean", "{n}");
        }
        assert_eq!(
            cap(
                "Marshall JCM800 \u{b7} clean P5 B5 M5 T5 MV5 G3",
                "Marshall JCM800",
                "amp_lead",
                3.0,
                false
            ),
            "amp_lead"
        );
        // (P41.6) a driven Mark, SLO, Orange or Hiwatt capture sounds like its own voice of ours
        assert_eq!(
            cap(
                "Mesa Boogie Mark V \u{b7} crunch",
                "Mesa Boogie Mark V",
                "amp_lead",
                6.5,
                false
            ),
            "amp_cali"
        );
        assert_eq!(
            cap(
                "Soldano SLO-100 \u{b7} lead",
                "Soldano SLO-100",
                "amp_lead",
                7.0,
                false
            ),
            "amp_snake"
        );
        assert_eq!(
            cap(
                "EVH 5150 III \u{b7} red",
                "EVH 5150 III",
                "amp_modern",
                8.0,
                false
            ),
            "amp_snake"
        );
        assert_eq!(
            cap(
                "Orange Rockerverb 50 \u{b7} dirty",
                "Orange Rockerverb",
                "amp_crunch",
                6.0,
                false
            ),
            "amp_tangerine"
        );
        assert_eq!(
            cap(
                "Hiwatt DR103 \u{b7} loud",
                "Hiwatt DR103",
                "amp_crunch",
                6.0,
                false
            ),
            "amp_surrey"
        );
        assert_eq!(
            cap(
                "Ampeg B-18 \u{b7} clean B18 Bass Chan",
                "Ampeg B-18",
                "amp_bass",
                2.5,
                true
            ),
            "amp_b15"
        );
    }

    #[test]
    fn captures_are_kept_and_found_again() {
        let root = std::env::temp_dir().join(format!("qrs-captures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("in");
        std::fs::create_dir_all(&src).unwrap();
        let f = src.join("my amp.nam");
        std::fs::write(&f, tiny()).unwrap();
        let mut l = Library::open(&root);
        let it = l.add_path_as(&f, None).unwrap();
        assert_eq!(it.name, "Tiny Amp");
        assert_eq!(it.gear.as_deref(), Some("Test One"));
        assert!(nam::is_registered(it.id));
        // junk is refused and leaves nothing behind
        let bad = src.join("bad.nam");
        std::fs::write(&bad, "{}").unwrap();
        assert!(l.add_path_as(&bad, None).is_err());
        assert_eq!(l.items.len(), 1);
        // opened again: the same item, registered again
        nam::unregister(it.id);
        let l2 = Library::open(&root);
        // (the same items: `added` may lose its last digit through JSON)
        let sig = |v: &[Item]| {
            v.iter()
                .map(|i| (i.id, i.name.clone(), i.file.clone(), i.gear.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(sig(&l2.items), sig(&l.items));
        assert!((l2.items[0].added - l.items[0].added).abs() < 1e-3);
        assert!(nam::is_registered(it.id));
        let mut l2 = l2;
        l2.remove(it.id);
        assert!(l2.items.is_empty());
        assert!(!nam::is_registered(it.id));
        let _ = std::fs::remove_dir_all(&root);
    }
}
