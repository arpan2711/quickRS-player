//! feedpak songs read into the chart model. Port of rs-open-format/rsfeedpak/feedpak.py.
//!
//! A feedpak (spec: got-feedback.github.io/feedpak-spec, vendored in ../rs-open-format/third_party) is one song:
//! a zip named song.feedpak, or a folder named song.feedpak/, holding manifest.yaml, one JSON chart per part,
//! audio stems and optional side files (lyrics, beats and sections, cover). ".sloppak" is the old name.
//!
//! Reader rules (spec §2.2, §3, §6, §7.4, §8): everything is found through the manifest, never by file name;
//! paths must stay inside the pack; `.jsonc` files have their comments stripped; unknown keys are ignored;
//! manifest tuning/capo override the chart's own; song_timeline.json's beats and sections win over ones in a
//! chart, which are otherwise taken from the first chart that has them.
//!
//! Rocksmith conversions (rsfeedpak) add a `rocksmith` block used here too: each part's own sections, and the
//! source .psarc and song key (`rsplay_id`, to share score history with the psarc).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::lyrics::{Line, RawSyllable, build_lines};
use crate::model::{Anchor, Arrangement, Beat, Chord, HandShape, Note, Section, Techs, ToneChange};
use crate::package::ArrangementInfo;
use crate::sng::py_round;

pub const SUFFIXES: [&str; 2] = ["feedpak", "sloppak"];
pub const SUPPORTED_MAJOR: u32 = 1;

pub fn is_feedpak(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUFFIXES.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn ferr(msg: impl Into<String>) -> Error {
    Error::Feedpak(msg.into())
}

// --- value helpers (Python's float(), int(), bool() on JSON values) --------------------------------------
pub(crate) fn num(v: Option<&Value>, default: f64) -> f64 {
    let x = match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(Value::Bool(b)) => Some(*b as i32 as f64),
        _ => None,
    };
    match x {
        Some(x) if x.is_finite() => x,
        _ => default,
    }
}

pub(crate) fn int(v: Option<&Value>, default: i64) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|x| x.is_finite()).map(|x| x.trunc() as i64))
            .unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse::<i64>().unwrap_or(default),
        Some(Value::Bool(b)) => *b as i64,
        _ => default,
    }
}

fn int32(v: Option<&Value>, default: i32) -> i32 {
    int(v, default as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

pub(crate) fn flag(v: Option<&Value>) -> bool {
    match v {
        Some(Value::String(s)) => matches!(s.trim().to_lowercase().as_str(), "true" | "on" | "yes" | "1"),
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|x| x != 0.0).unwrap_or(false),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        _ => false,
    }
}

/// Python's str() of a scalar (None → "").
pub(crate) fn vstr(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => {
            if *b {
                "True".into()
            } else {
                "False".into()
            }
        }
        _ => String::new(),
    }
}

/// Python's bool() of a JSON value.
pub(crate) fn py_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `x or ...`-style: values Python treats as false (empty strings, 0, false, null, []) count as missing.
fn truthy(v: Option<&Value>) -> Option<&Value> {
    v.filter(|v| py_truthy(v))
}

fn dicts(v: Option<&Value>) -> Vec<&Map<String, Value>> {
    v.and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_object()).collect())
        .unwrap_or_default()
}

/// Strip `//` and `/* */` comments outside strings (JSONC), keeping line numbers.
pub fn strip_comments(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < b.len() {
                out.push(b[i]);
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i + 1]);
                    i += 2;
                    continue;
                }
                if b[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else if c == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && i + 1 < b.len() && b[i + 1] == '*' {
            let start = i;
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
            let newlines = b[start..i].iter().filter(|&&c| c == '\n').count();
            if newlines > 0 {
                out.extend(std::iter::repeat_n('\n', newlines));
            } else {
                out.push(' ');
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// A manifest path as a safe POSIX relative path (spec §2.2).
pub fn safe_relpath(p: &str) -> Result<&str> {
    if p.is_empty() {
        return Err(ferr(format!("bad path in manifest: {p:?}")));
    }
    let parts: Vec<&str> = p.split('/').collect();
    if p.starts_with('/')
        || p.contains('\\')
        || p.contains(':')
        || parts.contains(&"..")
        || parts.contains(&"")
    {
        return Err(ferr(format!("unsafe path in manifest: {p:?}")));
    }
    Ok(p)
}

// feedpak boolean note fields -> techniques. `mt` (string mute) and `fhm` (fret-hand mute) both draw as a
// fret-hand mute, as the XML reader does with Rocksmith's "mute".
const TECHS: [(&str, Techs); 15] = [
    ("ho", Techs::HAMMER_ON),
    ("po", Techs::PULL_OFF),
    ("pm", Techs::PALM_MUTE),
    ("mt", Techs::FRET_HAND_MUTE),
    ("fhm", Techs::FRET_HAND_MUTE),
    ("hm", Techs::HARMONIC),
    ("hp", Techs::PINCH_HARMONIC),
    ("tp", Techs::TAP),
    ("tr", Techs::TREMOLO),
    ("slp", Techs::SLAP),
    ("plk", Techs::POP),
    ("ac", Techs::ACCENT),
    ("ln", Techs::LINK_NEXT),
    ("ig", Techs::IGNORE),
    ("vb", Techs::VIBRATO),
];
/// Chord-wide techniques: the chord has one when all its notes do (link-next: when any note links on).
const CHORD_ALL: [(&str, Techs); 4] = [
    ("pm", Techs::PALM_MUTE),
    ("fhm", Techs::FRET_HAND_MUTE),
    ("ac", Techs::ACCENT),
    ("ig", Techs::IGNORE),
];

fn parse_note(d: &Map<String, Value>, time: Option<f64>) -> Note {
    let t = time.unwrap_or_else(|| num(d.get("t"), 0.0));
    let sus = num(d.get("sus"), 0.0).max(0.0);
    let sl = int32(d.get("sl"), -1);
    let slu = int32(d.get("slu"), -1);
    let bend = num(d.get("bn"), 0.0).max(0.0);
    let mut techs = Techs::NONE;
    for (k, tech) in TECHS {
        if flag(d.get(k)) {
            techs |= tech;
        }
    }
    techs = techs
        .with(Techs::BEND, bend > 0.0)
        .with(Techs::SLIDE, sl >= 0)
        .with(Techs::UNPITCHED_SLIDE, slu >= 0)
        .with(Techs::SUSTAIN, sus > 0.0);
    let curve = dicts(d.get("bnv"))
        .into_iter()
        .map(|p| (t + num(p.get("t"), 0.0), num(p.get("v"), 0.0)))
        .collect();
    Note {
        time: t,
        string: int32(d.get("s"), 0),
        fret: int32(d.get("f"), 0),
        sustain: sus,
        techniques: techs,
        slide_to: sl,
        slide_unpitch_to: slu,
        bend,
        bend_curve: curve,
    }
}

fn non_latin_script(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    !letters.is_empty()
        && letters.iter().filter(|&&c| c as u32 > 0x24F).count() as f64 > letters.len() as f64 * 0.02
}

/// feedpak numbers downbeats from 1 (-1 = other beats); the model, like Rocksmith's SNG, from 0.
fn measure(v: Option<&Value>) -> i32 {
    let m = int32(v, -1);
    if m > 0 {
        m - 1
    } else if m == 0 {
        0
    } else {
        -1
    }
}

fn pad6(v: Option<&Value>, fill: i32) -> Vec<i32> {
    let mut out: Vec<i32> = v
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|x| int32(Some(x), fill)).collect())
        .unwrap_or_default();
    if out.len() < 6 {
        out.resize(6, fill);
    }
    out
}

#[derive(Clone, Debug)]
struct Template {
    name: String,
    frets: Vec<i32>,
    fingers: Vec<i32>,
}

fn templates(raw: Option<&Value>) -> Vec<Template> {
    let empty = Map::new();
    raw.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|t| {
                    let t = t.as_object().unwrap_or(&empty);
                    let name = vstr(truthy(t.get("displayName")).or_else(|| truthy(t.get("name"))));
                    Template {
                        name,
                        frets: pad6(t.get("frets"), -1),
                        fingers: pad6(t.get("fingers"), -1),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_chord(d: &Map<String, Value>, templates: &[Template]) -> Chord {
    let t = num(d.get("t"), 0.0);
    let cid = int32(d.get("id"), -1);
    let tpl = usize::try_from(cid)
        .ok()
        .and_then(|k| templates.get(k))
        .cloned()
        .unwrap_or(Template {
            name: String::new(),
            frets: vec![-1; 6],
            fingers: vec![-1; 6],
        });
    let mut raw: Vec<Map<String, Value>> = dicts(d.get("notes")).into_iter().cloned().collect();
    if raw.is_empty() {
        // no notes given: the template's shape
        raw = tpl
            .frets
            .iter()
            .enumerate()
            .filter(|(_, f)| **f >= 0)
            .map(|(s, f)| {
                let mut m = Map::new();
                m.insert("s".into(), Value::from(s as i64));
                m.insert("f".into(), Value::from(*f));
                m
            })
            .collect();
    }
    let notes: Vec<Note> = raw.iter().map(|n| parse_note(n, Some(t))).collect();
    let mut techs = Techs::NONE;
    for (key, tech) in CHORD_ALL {
        if !raw.is_empty() && raw.iter().all(|n| flag(n.get(key))) {
            techs |= tech;
        }
    }
    if raw.iter().any(|n| flag(n.get("ln"))) {
        techs |= Techs::LINK_NEXT;
    }
    let sustain = notes.iter().map(|n| n.sustain).fold(0.0, f64::max);
    Chord {
        time: t,
        chord_id: cid,
        name: tpl.name,
        notes,
        techniques: techs,
        sustain,
        frets: tpl.frets,
        fingers: tpl.fingers,
    }
}

enum Store {
    Zip {
        zip: zip::ZipArchive<File>,
        names: HashSet<String>,
        top: String,
    },
    Dir {
        root: PathBuf,
    },
}

pub struct FeedpakPackage {
    pub path: PathBuf,
    store: Store,
    json_cache: HashMap<String, Value>,
    pub manifest: Map<String, Value>,
    pub version: String,
    pub newer_major: bool,
    pub arrangements: Vec<ArrangementInfo>,
    /// Parts skipped because their path was unsafe.
    pub problems: Vec<String>,
}

impl FeedpakPackage {
    pub fn open(path: &Path) -> Result<FeedpakPackage> {
        let store = if path.is_dir() {
            Store::Dir {
                root: path.canonicalize()?,
            }
        } else if path.is_file() {
            let f = File::open(path)?;
            let zip = zip::ZipArchive::new(f)
                .map_err(|_| ferr(format!("{}: not a zip archive", file_name(path))))?;
            let names: HashSet<String> = zip
                .file_names()
                .filter_map(|r| r.ok().map(|c| c.into_owned()))
                .collect();
            let top = top_folder(&names);
            Store::Zip { zip, names, top }
        } else {
            return Err(ferr(format!("{}: no such file or folder", path.display())));
        };
        let mut p = FeedpakPackage {
            path: path.to_path_buf(),
            store,
            json_cache: HashMap::new(),
            manifest: Map::new(),
            version: "1.0.0".into(),
            newer_major: false,
            arrangements: Vec::new(),
            problems: Vec::new(),
        };
        p.read_manifest()?;
        p.discover();
        Ok(p)
    }

    fn name(&self) -> String {
        file_name(&self.path)
    }

    fn member(&self, rel: &str) -> Result<String> {
        match &self.store {
            Store::Zip { names, top, .. } => {
                if names.contains(rel) {
                    Ok(rel.to_string())
                } else if !top.is_empty() && names.contains(&format!("{top}{rel}")) {
                    Ok(format!("{top}{rel}"))
                } else {
                    Err(ferr(format!("{}: missing {rel}", self.name())))
                }
            }
            Store::Dir { .. } => Ok(rel.to_string()),
        }
    }

    fn dir_path(root: &Path, rel: &str) -> Result<PathBuf> {
        let target = root.join(rel);
        if let Ok(real) = target.canonicalize()
            && real != root
            && !real.starts_with(root)
        {
            return Err(ferr(format!("{rel}: escapes the pack (symlink?)")));
        }
        Ok(target)
    }

    pub fn exists(&self, rel: &str) -> bool {
        let Ok(rel) = safe_relpath(rel) else { return false };
        match &self.store {
            Store::Zip { .. } => self.member(rel).is_ok(),
            Store::Dir { root } => Self::dir_path(root, rel).map(|p| p.is_file()).unwrap_or(false),
        }
    }

    /// The bytes of a file named by the manifest.
    pub fn read(&mut self, rel: &str) -> Result<Vec<u8>> {
        let rel = safe_relpath(rel)?.to_string();
        let member = self.member(&rel)?;
        let name = self.name();
        match &mut self.store {
            Store::Zip { zip, .. } => {
                let mut f = zip
                    .by_name(&member)
                    .map_err(|e| ferr(format!("{name}: can't read {rel} ({e})")))?;
                let mut out = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut out)
                    .map_err(|e| ferr(format!("{name}: can't read {rel} ({e})")))?;
                Ok(out)
            }
            Store::Dir { root } => {
                let target = Self::dir_path(root, &rel)?;
                if !target.is_file() {
                    return Err(ferr(format!("{name}: missing {rel}")));
                }
                Ok(std::fs::read(target)?)
            }
        }
    }

    /// The size of a file in the pack without reading it (0 when unknown).
    pub fn size_of(&mut self, rel: &str) -> u64 {
        let Ok(member) = self.member(rel) else { return 0 };
        match &mut self.store {
            Store::Zip { zip, .. } => zip.by_name(&member).map(|f| f.size()).unwrap_or(0),
            Store::Dir { root } => std::fs::metadata(root.join(rel)).map(|m| m.len()).unwrap_or(0),
        }
    }

    pub fn read_json(&mut self, rel: &str) -> Result<Value> {
        if let Some(v) = self.json_cache.get(rel) {
            return Ok(v.clone());
        }
        let bytes = self.read(rel)?;
        let text = String::from_utf8(bytes).map_err(|e| ferr(format!("{rel}: invalid JSON ({e})")))?;
        let mut text = text.trim_start_matches('\u{feff}').to_string();
        if rel.to_lowercase().ends_with(".jsonc") {
            text = strip_comments(&text);
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| ferr(format!("{rel}: invalid JSON ({e})")))?;
        self.json_cache.insert(rel.to_string(), v.clone());
        Ok(v)
    }

    fn read_manifest(&mut self) -> Result<()> {
        let name = self.name();
        let bytes = self.read("manifest.yaml")?;
        let text = String::from_utf8(bytes)
            .map_err(|e| ferr(format!("{name}: manifest.yaml isn't valid YAML ({e})")))?;
        let doc: Value = serde_norway::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|e| ferr(format!("{name}: manifest.yaml isn't valid YAML ({e})")))?;
        let Value::Object(map) = doc else {
            return Err(ferr(format!("{name}: manifest.yaml isn't a mapping")));
        };
        let version = vstr(truthy(map.get("feedpak_version")));
        let version = if version.is_empty() {
            "1.0.0".to_string()
        } else {
            version
        };
        let parts: Vec<&str> = version.splitn(3, '.').collect();
        let ok = parts.len() == 3
            && parts[0].chars().all(|c| c.is_ascii_digit())
            && !parts[0].is_empty()
            && parts[1].chars().all(|c| c.is_ascii_digit())
            && !parts[1].is_empty()
            && parts[2]
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false);
        self.newer_major = ok
            && parts[0]
                .parse::<u32>()
                .map(|m| m > SUPPORTED_MAJOR)
                .unwrap_or(false);
        self.version = if ok { version } else { "1.0.0".into() };
        self.manifest = map;
        Ok(())
    }

    // -- metadata --------------------------------------------------------------------------------------
    pub fn title(&self) -> String {
        vstr(truthy(self.manifest.get("title")))
    }
    pub fn artist(&self) -> String {
        vstr(truthy(self.manifest.get("artist")))
    }
    pub fn album(&self) -> String {
        vstr(truthy(self.manifest.get("album")))
    }
    pub fn year(&self) -> i32 {
        int32(self.manifest.get("year"), 0)
    }
    pub fn duration(&self) -> f64 {
        num(self.manifest.get("duration"), 0.0).max(0.0)
    }
    /// The conversion's provenance block (source .psarc, song key, rsplay_id), or empty.
    pub fn rocksmith(&self) -> Map<String, Value> {
        self.manifest
            .get("rocksmith")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default()
    }
    /// "file_p.psarc|SongKey" style id of the psarc this pack was converted from, if any.
    pub fn source_id(&self) -> String {
        vstr(self.rocksmith().get("rsplay_id"))
    }

    fn entries(&self) -> Vec<Map<String, Value>> {
        dicts(self.manifest.get("arrangements"))
            .into_iter()
            .filter(|e| {
                let kind = vstr(e.get("type")).trim().to_lowercase();
                !(kind == "drums" || kind == "drum") && matches!(e.get("file"), Some(Value::String(_)))
            })
            .cloned()
            .collect()
    }

    fn discover(&mut self) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let (title, artist, album, duration) = (self.title(), self.artist(), self.album(), self.duration());
        for e in self.entries() {
            let file = vstr(e.get("file"));
            if let Err(err) = safe_relpath(&file) {
                self.problems.push(err.to_string());
                continue;
            }
            let ident: String = vstr(truthy(e.get("id")))
                .to_lowercase()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect();
            let ident = if ident.is_empty() {
                "arr".to_string()
            } else {
                ident
            };
            let n = seen.entry(ident.clone()).or_insert(0);
            *n += 1;
            let key = if *n == 1 {
                ident.clone()
            } else {
                format!("{ident}{n}")
            };
            let name = {
                let s = vstr(truthy(e.get("name")).or_else(|| truthy(e.get("id"))));
                if s.is_empty() { key.clone() } else { s }
            };
            let kind = vstr(e.get("type")).trim().to_lowercase();
            let tuning: Vec<i32> = match e.get("tuning") {
                Some(Value::Array(a)) => a.iter().map(|x| int32(Some(x), 0)).collect(),
                _ => vec![0; 6],
            };
            let is_bass = kind == "bass"
                || (kind.is_empty()
                    && (matches!(tuning.len(), 4 | 5) || name.to_lowercase().contains("bass")));
            let rs = e
                .get("rocksmith")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let bonus = flag(rs.get("bonus")) || name.to_lowercase().starts_with("bonus");
            self.arrangements.push(ArrangementInfo {
                key,
                name,
                chart: crate::package::ChartRef::Feedpak(file),
                title: title.clone(),
                artist: artist.clone(),
                album: album.clone(),
                tuning,
                capo: int32(e.get("capo"), 0).max(0),
                is_bass,
                song_length: duration,
                bonus,
                source_chart: vstr(rs.get("chart")),
                ..Default::default()
            });
        }
    }

    /// (beats, sections) for the song: song_timeline.json when valid, else the first chart carrying them.
    fn timeline(&mut self) -> (Vec<Map<String, Value>>, Vec<Map<String, Value>>) {
        if let Some(Value::String(rel)) = self.manifest.get("song_timeline").cloned()
            && !rel.is_empty()
            && let Ok(Value::Object(st)) = self.read_json(&rel)
        {
            let ok = |k: &str| matches!(st.get(k), None | Some(Value::Array(_)));
            if ok("beats") && ok("sections") {
                return (
                    dicts(st.get("beats")).into_iter().cloned().collect(),
                    dicts(st.get("sections")).into_iter().cloned().collect(),
                );
            }
        }
        let (mut beats, mut sections) = (Vec::new(), Vec::new());
        let files: Vec<String> = self
            .arrangements
            .iter()
            .filter_map(|a| a.chart.feedpak_file().map(str::to_string))
            .collect();
        for f in files {
            let Ok(Value::Object(data)) = self.read_json(&f) else {
                continue;
            };
            if beats.is_empty() {
                beats = dicts(data.get("beats")).into_iter().cloned().collect();
            }
            if sections.is_empty() {
                sections = dicts(data.get("sections")).into_iter().cloned().collect();
            }
            if !beats.is_empty() && !sections.is_empty() {
                break;
            }
        }
        (beats, sections)
    }

    fn entry_for(&self, info: &ArrangementInfo) -> Map<String, Value> {
        let file = info.chart.feedpak_file().unwrap_or_default();
        self.entries()
            .into_iter()
            .find(|e| vstr(e.get("file")) == file)
            .unwrap_or_default()
    }

    /// One part at a mastery between 0 and 1 (each phrase's level picked as the SNG reader does) when the chart
    /// has a difficulty ladder (`phrases`); the full chart otherwise.
    pub fn load(&mut self, info: &ArrangementInfo, mastery: f64) -> Result<Arrangement> {
        let file = info
            .chart
            .feedpak_file()
            .ok_or_else(|| ferr("not a feedpak part"))?
            .to_string();
        let Value::Object(data) = self.read_json(&file)? else {
            return Err(ferr(format!("{file}: not a JSON object")));
        };
        let e = self.entry_for(info);
        let tuning = match e.get("tuning") {
            Some(Value::Array(a)) => Some(a.clone()),
            _ => data.get("tuning").and_then(|v| v.as_array()).cloned(),
        };
        let capo = if e.contains_key("capo") {
            e.get("capo")
        } else {
            data.get("capo")
        };
        let duration = self.duration();
        let mut arr = Arrangement::new(&info.name);
        arr.title = self.title();
        arr.artist = self.artist();
        arr.album = self.album();
        arr.tuning = match tuning {
            Some(t) if !t.is_empty() => t.iter().map(|x| int32(Some(x), 0)).collect(),
            _ => vec![0; 6],
        };
        arr.capo = int32(capo, 0).max(0);
        arr.cent_offset = num(data.get("centOffset"), 0.0);
        arr.song_length = duration;
        arr.is_bass = info.is_bass;
        arr.source = "feedpak".into();
        arr.mastery = mastery;
        arr.has_levels = data.get("phrases").map(py_truthy).unwrap_or(false);

        let (mut beats, mut sections) = self.timeline();
        let own = data
            .get("rocksmith")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let own_beats: Vec<Map<String, Value>> = dicts(own.get("beats")).into_iter().cloned().collect();
        if !own_beats.is_empty() {
            beats = own_beats;
        }
        let own_sections: Vec<Map<String, Value>> = dicts(own.get("sections")).into_iter().cloned().collect();
        if !own_sections.is_empty() {
            sections = own_sections;
        }
        arr.beats = beats
            .iter()
            .map(|b| Beat {
                time: num(b.get("time"), 0.0),
                measure: measure(b.get("measure")),
            })
            .collect();
        let sec_time = |s: &Map<String, Value>| num(s.get("time").or_else(|| s.get("start_time")), 0.0);
        sections.sort_by(|a, b| sec_time(a).total_cmp(&sec_time(b)));
        let starts: Vec<f64> = sections.iter().map(sec_time).collect();
        arr.sections = sections
            .iter()
            .enumerate()
            .map(|(k, s)| Section {
                name: vstr(s.get("name")),
                number: int32(s.get("number"), 1),
                start: starts[k],
                end: starts
                    .get(k + 1)
                    .copied()
                    .unwrap_or_else(|| duration.max(starts[k])),
            })
            .collect();

        let tpls = templates(data.get("templates"));
        let (notes, chords, mut anchors, shapes) = events_at(&data, mastery);
        arr.notes = notes.iter().map(|n| parse_note(n, None)).collect();
        arr.notes
            .sort_by(|a, b| a.time.total_cmp(&b.time).then(a.string.cmp(&b.string)));
        arr.chords = chords.iter().map(|c| parse_chord(c, &tpls)).collect();
        anchors.sort_by(|a, b| num(a.get("time"), 0.0).total_cmp(&num(b.get("time"), 0.0)));
        for (k, a) in anchors.iter().enumerate() {
            let start = num(a.get("time"), 0.0);
            let end = anchors
                .get(k + 1)
                .map(|n| num(n.get("time"), 0.0))
                .unwrap_or_else(|| duration.max(start));
            arr.anchors.push(Anchor {
                start,
                end,
                fret: int32(a.get("fret"), 1),
                width: int32(a.get("width"), 4),
            });
        }
        for h in shapes {
            let cid = int32(h.get("chord_id"), 0);
            if let Some(t) = usize::try_from(cid).ok().and_then(|k| tpls.get(k)) {
                arr.handshapes.push(HandShape {
                    start: num(h.get("start_time"), 0.0),
                    end: num(h.get("end_time"), 0.0),
                    name: t.name.clone(),
                    frets: t.frets.clone(),
                    fingers: t.fingers.clone(),
                    arpeggio: flag(h.get("arp")),
                });
            }
        }
        if let Some(tones) = data.get("tones").and_then(|v| v.as_object()) {
            for c in dicts(tones.get("changes")) {
                arr.tones.push(ToneChange {
                    time: num(c.get("t"), 0.0),
                    name: vstr(c.get("name")),
                });
            }
        }
        Ok(arr)
    }

    // -- lyrics ----------------------------------------------------------------------------------------
    fn syllables(&mut self, rel: Option<&Value>) -> Vec<Map<String, Value>> {
        let Some(Value::String(rel)) = rel else {
            return Vec::new();
        };
        if rel.is_empty() {
            return Vec::new();
        }
        match self.read_json(&rel.clone()) {
            Ok(Value::Array(a)) => a.into_iter().filter_map(|x| x.as_object().cloned()).collect(),
            _ => Vec::new(),
        }
    }

    /// The lyrics as display lines: the main track or, with `prefer_latin`, when that track is in a non-Latin
    /// script (e.g. Japanese) and the pack has a Latin transliteration (romaji), that one.
    pub fn load_lyrics(&mut self, prefer_latin: bool) -> Vec<Line> {
        let main = self.manifest.get("lyrics").cloned();
        let mut syl = self.syllables(main.as_ref());
        let all: String = syl.iter().map(|s| vstr(s.get("w"))).collect();
        if prefer_latin && non_latin_script(&all) {
            let tracks = dicts(self.manifest.get("lyric_tracks"))
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            for t in tracks {
                if vstr(t.get("kind")) == "transliteration"
                    && vstr(t.get("language")).to_lowercase().contains("latn")
                {
                    let alt = self.syllables(t.get("file"));
                    if !alt.is_empty() {
                        syl = alt;
                    }
                    break;
                }
            }
        }
        let raw: Vec<RawSyllable> = syl
            .iter()
            .map(|s| RawSyllable {
                time: num(s.get("t"), 0.0),
                length: num(s.get("d"), 0.0),
                text: vstr(s.get("w")),
                pitch: None,
            })
            .collect();
        build_lines(&raw)
    }

    // -- audio and art ---------------------------------------------------------------------------------
    pub fn stems(&self) -> Vec<Map<String, Value>> {
        dicts(self.manifest.get("stems"))
            .into_iter()
            .filter(|s| matches!(s.get("file"), Some(Value::String(_))))
            .cloned()
            .collect()
    }

    /// The stem to play as the song: `full` (the mixdown), else the first default one, else the first.
    pub fn song_stem(&self) -> Option<String> {
        let stems = self.stems();
        let picks: [&dyn Fn(&Map<String, Value>) -> bool; 3] = [
            &|s| vstr(s.get("id")) == "full",
            &|s| flag(s.get("default")),
            &|_| true,
        ];
        for pick in picks {
            for s in &stems {
                let f = vstr(s.get("file"));
                if pick(s) && self.exists(&f) {
                    return Some(f);
                }
            }
        }
        None
    }

    pub fn preview(&self) -> Option<String> {
        match self.manifest.get("preview") {
            Some(Value::String(p)) if self.exists(p) => Some(p.clone()),
            _ => None,
        }
    }

    pub fn cover_bytes(&mut self) -> Option<Vec<u8>> {
        match self.manifest.get("cover").cloned() {
            Some(Value::String(p)) if self.exists(&p) => self.read(&p).ok(),
            _ => None,
        }
    }
}

impl std::fmt::Debug for FeedpakPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeedpakPackage")
            .field("path", &self.path)
            .field("version", &self.version)
            .finish()
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// 'song.feedpak/' when the zip holds the pack inside one top folder rather than at its root.
fn top_folder(names: &HashSet<String>) -> String {
    if names.contains("manifest.yaml") {
        return String::new();
    }
    let tops: Vec<&String> = names
        .iter()
        .filter(|n| n.ends_with("/manifest.yaml") && n.matches('/').count() == 1)
        .collect();
    if tops.len() == 1 {
        tops[0][..tops[0].len() - "manifest.yaml".len()].to_string()
    } else {
        String::new()
    }
}

type Events = (
    Vec<Map<String, Value>>,
    Vec<Map<String, Value>>,
    Vec<Map<String, Value>>,
    Vec<Map<String, Value>>,
);

/// (notes, chords, anchors, handshapes) at a mastery: the top-level chart at full difficulty or when there's no
/// ladder; otherwise each phrase's level round(mastery * max difficulty), else the nearest easier one.
fn events_at(data: &Map<String, Value>, mastery: f64) -> Events {
    let own = |v: Option<&Value>| -> Vec<Map<String, Value>> { dicts(v).into_iter().cloned().collect() };
    let phrases = dicts(data.get("phrases"));
    if mastery >= 1.0 || phrases.is_empty() {
        return (
            own(data.get("notes")),
            own(data.get("chords")),
            own(data.get("anchors")),
            own(data.get("handshapes")),
        );
    }
    let m = mastery.clamp(0.0, 1.0);
    let (mut notes, mut chords, mut anchors, mut shapes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for p in phrases {
        let mut levels = dicts(p.get("levels"));
        if levels.is_empty() {
            continue;
        }
        levels.sort_by_key(|l| int(l.get("difficulty"), 0));
        let want = py_round(m * int(p.get("max_difficulty"), 0) as f64) as i64;
        let mut pick = levels[0];
        for l in &levels {
            if int(l.get("difficulty"), 0) <= want {
                pick = l;
            }
        }
        notes.extend(own(pick.get("notes")));
        chords.extend(own(pick.get("chords")));
        anchors.extend(own(pick.get("anchors")));
        shapes.extend(own(pick.get("handshapes")));
    }
    (notes, chords, anchors, shapes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_comments_are_stripped_outside_strings() {
        let t = "{\"a\": \"// not a comment\", // yes\n \"b\": /* x\n y */ 1}";
        let v: Value = serde_json::from_str(&strip_comments(t)).unwrap();
        assert_eq!(v["a"], "// not a comment");
        assert_eq!(v["b"], 1);
    }

    #[test]
    fn unsafe_paths() {
        for p in ["../x", "/abs", "a\\b", "c:x", "a//b", ""] {
            assert!(safe_relpath(p).is_err(), "{p}");
        }
        assert!(safe_relpath("stems/full.ogg").is_ok());
    }

    #[test]
    fn python_like_value_helpers() {
        assert_eq!(int(Some(&Value::from(3.7)), 0), 3);
        assert_eq!(int(Some(&Value::from("4")), 0), 4);
        assert_eq!(int(Some(&Value::from("4.5")), -1), -1);
        assert!(flag(Some(&Value::from("Yes"))));
        assert!(!flag(Some(&Value::from(0))));
        assert_eq!(num(Some(&Value::from("1.5")), 0.0), 1.5);
    }
}
