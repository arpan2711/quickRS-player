//! A song package, whatever its format: a Rocksmith `.psarc` (PC or Mac, single song or a multi-song pack like
//! songs.psarc) or a `.feedpak` / `.sloppak` (zip or folder). Port of rsplay/song_loader.py with
//! rsfeedpak/platform.py and the `open_package` helper of rsfeedpak/feedpak.py.
//!
//! Everything above this module (library, player, CLI) works on `Package` and `ArrangementInfo` and never asks
//! which format a song came from, except to find its audio and art.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::feedpak::{self, FeedpakPackage};
use crate::keys::Keys;
use crate::lyrics::{Line, RawSyllable, build_lines};
use crate::model::Arrangement;
use crate::psarc::{Platform, Psarc};
use crate::sng::{self, ArrMeta};
use crate::xml;

pub const NON_INSTRUMENT: [&str; 3] = ["vocals", "jvocals", "showlights"];

fn arrangement_order(name: &str) -> u32 {
    match name.to_lowercase().as_str() {
        "lead" => 0,
        "rhythm" => 1,
        "combo" => 2,
        "bass" => 3,
        _ => 9,
    }
}

/// Where a part's chart lives.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ChartRef {
    #[default]
    None,
    Psarc {
        sng: Option<String>,
        xml: Option<String>,
    },
    Feedpak(String),
}

impl ChartRef {
    pub fn feedpak_file(&self) -> Option<&str> {
        match self {
            ChartRef::Feedpak(f) => Some(f),
            _ => None,
        }
    }
    pub fn describe(&self) -> String {
        match self {
            ChartRef::Psarc { sng: Some(s), .. } => s.clone(),
            ChartRef::Psarc { xml: Some(x), .. } => x.clone(),
            ChartRef::Feedpak(f) => f.clone(),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrangementInfo {
    /// Unique lowercase selector, e.g. "lead", "lead2", "bass".
    pub key: String,
    /// Display name, e.g. "Lead".
    pub name: String,
    pub chart: ChartRef,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub tuning: Vec<i32>,
    pub capo: i32,
    pub is_bass: bool,
    pub song_length: f64,
    pub bonus: bool,
    /// Manifest SongKey; packs like songs.psarc hold many songs.
    pub song_key: String,
    /// Wwise sound bank name, e.g. "song_reptilia.bnk".
    pub bank: String,
    /// The short preview clip's bank, e.g. "song_reptilia_preview.bnk".
    pub preview_bank: String,
    /// Texture base name, e.g. "album_reptilia" (gfxassets/album_art/<name>_128.dds).
    pub album_art: String,
    pub year: i32,
    pub artist_sort: String,
    pub title_sort: String,
    /// Rocksmith tone names by slot ("Tone_A"..), from the manifest.
    pub tone_names: Vec<(String, String)>,
    /// feedpak converted from a psarc: the source chart's path in that psarc (`rocksmith.chart`).
    pub source_chart: String,
}

impl ArrangementInfo {
    pub fn tuning_name(&self) -> String {
        crate::model::tuning_name(&self.tuning, self.is_bass)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Psarc,
    Feedpak,
}

enum Kind {
    Psarc(Box<Psarc>),
    Feedpak(Box<FeedpakPackage>),
}

pub struct Package {
    pub path: PathBuf,
    pub keys: Keys,
    /// Set when this is one song of a multi-song pack.
    pub song_key: String,
    pub arrangements: Vec<ArrangementInfo>,
    kind: Kind,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package")
            .field("path", &self.path)
            .field("format", &self.format())
            .field("song_key", &self.song_key)
            .field("arrangements", &self.arrangements.len())
            .finish()
    }
}

fn urn_tail(urn: Option<&Value>) -> String {
    urn.and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.rsplit(':').next().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// PurePosixPath(path).stem.lower()
pub fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let s = match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    };
    s.to_lowercase()
}

fn s(attrs: &serde_json::Map<String, Value>, k: &str) -> String {
    match attrs.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Python's `capitalize()`.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

impl Package {
    /// Open a package by its path: `.feedpak` / `.sloppak` (file or folder) or a `.psarc`.
    pub fn open(path: impl AsRef<Path>, keys: &Keys, song_key: Option<&str>) -> Result<Package> {
        let path = path.as_ref();
        if feedpak::is_feedpak(path) {
            let fp = FeedpakPackage::open(path)?;
            let arrangements = fp.arrangements.clone();
            return Ok(Package {
                path: path.to_path_buf(),
                keys: keys.clone(),
                song_key: String::new(),
                arrangements,
                kind: Kind::Feedpak(Box::new(fp)),
            });
        }
        let archive = Psarc::open(path, keys.psarc.as_ref())?;
        let mut p = Package {
            path: path.to_path_buf(),
            keys: keys.clone(),
            song_key: song_key.unwrap_or("").to_lowercase(),
            arrangements: Vec::new(),
            kind: Kind::Psarc(Box::new(archive)),
        };
        p.arrangements = p.discover_psarc()?;
        Ok(p)
    }

    /// A psarc held in memory (tests, or a package read from another archive).
    pub fn from_psarc_bytes(
        data: Vec<u8>,
        name: &str,
        keys: &Keys,
        song_key: Option<&str>,
    ) -> Result<Package> {
        let archive = Psarc::from_bytes(data, keys.psarc.as_ref())?;
        let mut p = Package {
            path: PathBuf::from(name),
            keys: keys.clone(),
            song_key: song_key.unwrap_or("").to_lowercase(),
            arrangements: Vec::new(),
            kind: Kind::Psarc(Box::new(archive)),
        };
        p.arrangements = p.discover_psarc()?;
        Ok(p)
    }

    pub fn format(&self) -> Format {
        match self.kind {
            Kind::Psarc(_) => Format::Psarc,
            Kind::Feedpak(_) => Format::Feedpak,
        }
    }

    pub fn psarc(&mut self) -> Option<&mut Psarc> {
        match &mut self.kind {
            Kind::Psarc(p) => Some(p),
            _ => None,
        }
    }

    pub fn psarc_ref(&self) -> Option<&Psarc> {
        match &self.kind {
            Kind::Psarc(p) => Some(p),
            _ => None,
        }
    }

    pub fn feedpak(&mut self) -> Option<&mut FeedpakPackage> {
        match &mut self.kind {
            Kind::Feedpak(f) => Some(f),
            _ => None,
        }
    }

    pub fn feedpak_ref(&self) -> Option<&FeedpakPackage> {
        match &self.kind {
            Kind::Feedpak(f) => Some(f),
            _ => None,
        }
    }

    pub fn platform(&self) -> Platform {
        self.psarc_ref().map(|p| p.platform).unwrap_or(Platform::Pc)
    }

    fn discover_psarc(&mut self) -> Result<Vec<ArrangementInfo>> {
        let want_key = self.song_key.clone();
        let Kind::Psarc(archive) = &mut self.kind else {
            unreachable!()
        };
        // Note: glob '*' also matches '/', so "songs/bin/*.sng" covers songs/bin/<platform>/x.sng.
        let mut sngs: HashMap<String, String> = HashMap::new();
        let mut sng_order: Vec<String> = Vec::new();
        for e in archive.find("songs/bin/*.sng") {
            let st = stem(&e.name);
            if !sngs.contains_key(&st) {
                sng_order.push(st.clone());
            }
            sngs.insert(st, e.name.clone());
        }
        let mut xmls: HashMap<String, String> = HashMap::new();
        for e in archive.find("songs/arr/*.xml") {
            xmls.insert(stem(&e.name), e.name.clone());
        }
        let mut found: Vec<ArrangementInfo> = Vec::new();
        let mut years: Vec<(String, i32)> = Vec::new();
        for entry in archive.find("manifests/*.json") {
            let Ok(bytes) = archive.read_entry(&entry) else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let Ok(Value::Object(doc)) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
            else {
                continue;
            };
            let Some(Value::Object(items)) = doc.get("Entries") else {
                continue;
            };
            for item in items.values() {
                let Some(attrs) = item.get("Attributes").and_then(|a| a.as_object()) else {
                    continue;
                };
                let song_key = s(attrs, "SongKey");
                let year = attrs
                    .get("SongYear")
                    .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                    .unwrap_or(0) as i32;
                if year != 0 {
                    years.push((song_key.to_lowercase(), year));
                }
                let name = s(attrs, "ArrangementName");
                if name.is_empty() || NON_INSTRUMENT.contains(&name.to_lowercase().as_str()) {
                    continue;
                }
                let asset = urn_tail(attrs.get("SongAsset")).to_lowercase();
                let xml_asset = urn_tail(attrs.get("SongXml")).to_lowercase();
                let empty = serde_json::Map::new();
                let props = attrs
                    .get("ArrangementProperties")
                    .and_then(|p| p.as_object())
                    .unwrap_or(&empty);
                let tuning_d = attrs.get("Tuning").and_then(|p| p.as_object()).unwrap_or(&empty);
                let manifest_stem = stem(&entry.name);
                let sng_path = sngs.get(&asset).or_else(|| sngs.get(&manifest_stem)).cloned();
                let xml_path = xmls.get(&xml_asset).or_else(|| xmls.get(&manifest_stem)).cloned();
                let num = |v: Option<&Value>| {
                    v.and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                        .unwrap_or(0.0)
                };
                let mut tone_names = Vec::new();
                for slot in ["Tone_Base", "Tone_A", "Tone_B", "Tone_C", "Tone_D"] {
                    let t = s(attrs, slot);
                    if !t.is_empty() {
                        tone_names.push((slot.to_string(), t));
                    }
                }
                found.push(ArrangementInfo {
                    key: String::new(),
                    is_bass: name.to_lowercase() == "bass"
                        || props.get("pathBass").and_then(|v| v.as_i64()) == Some(1),
                    bonus: props.get("bonusArr").and_then(|v| v.as_i64()) == Some(1),
                    name,
                    chart: ChartRef::Psarc {
                        sng: sng_path,
                        xml: xml_path,
                    },
                    title: s(attrs, "SongName"),
                    artist: s(attrs, "ArtistName"),
                    album: s(attrs, "AlbumName"),
                    tuning: (0..6)
                        .map(|i| num(tuning_d.get(&format!("string{i}"))) as i32)
                        .collect(),
                    capo: (num(attrs.get("CapoFret")) as i32).max(0),
                    song_length: num(attrs.get("SongLength")),
                    song_key,
                    bank: s(attrs, "SongBank"),
                    preview_bank: s(attrs, "PreviewBankPath"),
                    album_art: urn_tail(attrs.get("AlbumArt")),
                    year,
                    artist_sort: s(attrs, "ArtistNameSort"),
                    title_sort: s(attrs, "SongNameSort"),
                    tone_names,
                    source_chart: String::new(),
                });
            }
        }
        if found.is_empty() {
            // no usable manifests: fall back to every non-vocal chart
            let mut stems: Vec<&String> = sngs.keys().collect();
            stems.sort();
            for st in stems {
                let suffix = st.rsplit('_').next().unwrap_or(st);
                if NON_INSTRUMENT.contains(&suffix) {
                    continue;
                }
                found.push(ArrangementInfo {
                    name: capitalize(suffix),
                    chart: ChartRef::Psarc {
                        sng: Some(sngs[st].clone()),
                        xml: xmls.get(st).cloned(),
                    },
                    is_bass: suffix.starts_with("bass"),
                    tuning: vec![0; 6],
                    ..Default::default()
                });
            }
        }
        found.retain(|a| matches!(&a.chart, ChartRef::Psarc { sng, xml } if sng.is_some() || xml.is_some()));
        if !want_key.is_empty() {
            found.retain(|a| a.song_key.to_lowercase() == want_key);
        }
        found.sort_by(|a, b| {
            let sa = match &a.chart {
                ChartRef::Psarc { sng, .. } => sng.clone().unwrap_or_default(),
                _ => String::new(),
            };
            let sb = match &b.chart {
                ChartRef::Psarc { sng, .. } => sng.clone().unwrap_or_default(),
                _ => String::new(),
            };
            (arrangement_order(&a.name), a.bonus, sa).cmp(&(arrangement_order(&b.name), b.bonus, sb))
        });
        assign_keys(&mut found);
        // the song's year: from this song's manifests (any arrangement, vocals included)
        let year = years
            .iter()
            .find(|(k, _)| want_key.is_empty() || *k == want_key)
            .map(|(_, y)| *y)
            .unwrap_or(0);
        for a in &mut found {
            if a.year == 0 {
                a.year = year;
            }
        }
        Ok(found)
    }

    // -- selection ---------------------------------------------------------------------------------------
    pub fn find(&self, query: Option<&str>) -> Result<&ArrangementInfo> {
        find_arrangement(&self.arrangements, query, &self.display_name())
    }

    fn display_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn title(&self) -> String {
        match &self.kind {
            Kind::Feedpak(f) => f.title(),
            _ => self
                .arrangements
                .first()
                .map(|a| a.title.clone())
                .unwrap_or_default(),
        }
    }
    pub fn artist(&self) -> String {
        match &self.kind {
            Kind::Feedpak(f) => f.artist(),
            _ => self
                .arrangements
                .first()
                .map(|a| a.artist.clone())
                .unwrap_or_default(),
        }
    }
    pub fn album(&self) -> String {
        match &self.kind {
            Kind::Feedpak(f) => f.album(),
            _ => self
                .arrangements
                .first()
                .map(|a| a.album.clone())
                .unwrap_or_default(),
        }
    }
    pub fn year(&self) -> i32 {
        match &self.kind {
            Kind::Feedpak(f) => f.year(),
            _ => self
                .arrangements
                .iter()
                .map(|a| a.year)
                .find(|y| *y != 0)
                .unwrap_or(0),
        }
    }
    /// Song length: the longest part's.
    pub fn duration(&self) -> f64 {
        match &self.kind {
            Kind::Feedpak(f) => f.duration(),
            _ => self
                .arrangements
                .iter()
                .map(|a| a.song_length)
                .fold(0.0, f64::max),
        }
    }

    /// Distinct song keys in this package (more than one for packs like songs.psarc).
    pub fn song_keys(&self) -> Vec<String> {
        let set: BTreeSet<String> = self
            .arrangements
            .iter()
            .filter(|a| !a.song_key.is_empty())
            .map(|a| a.song_key.clone())
            .collect();
        set.into_iter().collect()
    }

    // -- charts ------------------------------------------------------------------------------------------
    pub fn load(&mut self, query: Option<&str>, mastery: f64) -> Result<Arrangement> {
        let info = self.find(query)?.clone();
        self.load_info(&info, mastery)
    }

    pub fn load_info(&mut self, info: &ArrangementInfo, mastery: f64) -> Result<Arrangement> {
        let keys = self.keys.clone();
        match &mut self.kind {
            Kind::Feedpak(f) => f.load(info, mastery),
            Kind::Psarc(archive) => {
                let ChartRef::Psarc { sng, xml: xml_path } = &info.chart else {
                    return Err(Error::Arrangement("not a psarc part".into()));
                };
                let platform = archive.platform;
                let mut arr = if let Some(sng_path) = sng {
                    let data = archive.read(sng_path)?;
                    let chart = sng::load_sng(&data, &keys, platform)?;
                    sng::sng_to_arrangement(
                        &chart,
                        &ArrMeta {
                            name: &info.name,
                            is_bass: info.is_bass,
                            title: &info.title,
                            artist: &info.artist,
                            album: &info.album,
                        },
                        mastery,
                    )?
                } else if let Some(x) = xml_path {
                    let mut a = xml::parse_xml(&archive.read(x)?, mastery)?;
                    a.name = info.name.clone();
                    a.is_bass = info.is_bass;
                    if a.title.is_empty() {
                        a.title = info.title.clone();
                    }
                    if a.artist.is_empty() {
                        a.artist = info.artist.clone();
                    }
                    a
                } else {
                    return Err(Error::Arrangement(format!("{}: part has no chart", info.name)));
                };
                if arr.song_length == 0.0 {
                    arr.song_length = info.song_length;
                }
                for t in &mut arr.tones {
                    if let Some((_, n)) = info.tone_names.iter().find(|(slot, _)| *slot == t.name) {
                        t.name = n.clone();
                    }
                }
                Ok(arr)
            }
        }
    }

    /// The song's lyrics as display lines (empty if it has none).
    pub fn load_lyrics(&mut self) -> Result<Vec<Line>> {
        let keys = self.keys.clone();
        let want = if self.song_key.is_empty() {
            None
        } else {
            Some(format!("{}_vocals", self.song_key))
        };
        match &mut self.kind {
            Kind::Feedpak(f) => Ok(f.load_lyrics(true)),
            Kind::Psarc(archive) => {
                let platform = archive.platform;
                for e in archive.find("songs/bin/*vocals.sng") {
                    let st = stem(&e.name);
                    if st.ends_with("jvocals") || want.as_ref().is_some_and(|w| *w != st) {
                        continue; // jvocals are the Japanese lyrics
                    }
                    let data = archive.read_entry(&e)?;
                    let chart = sng::load_sng(&data, &keys, platform)?;
                    let raw: Vec<RawSyllable> = chart
                        .vocals
                        .iter()
                        .map(|v| RawSyllable {
                            time: v.time as f64,
                            length: v.length as f64,
                            text: v.text.clone(),
                            pitch: (v.note < 254).then_some(v.note),
                        })
                        .collect();
                    return Ok(build_lines(&raw));
                }
                Ok(Vec::new())
            }
        }
    }

    /// (name, size) of the package's audio files, largest first (psarc: .wem; feedpak: stems).
    pub fn audio_files(&mut self) -> Vec<(String, u64)> {
        match &mut self.kind {
            Kind::Psarc(a) => {
                let mut v: Vec<(String, u64)> =
                    a.find("*.wem").into_iter().map(|e| (e.name, e.length)).collect();
                v.sort_by_key(|x| std::cmp::Reverse(x.1));
                v
            }
            Kind::Feedpak(f) => {
                let files: Vec<String> = f.stems().iter().map(|s| feedpak::vstr(s.get("file"))).collect();
                let mut out = Vec::new();
                for p in files {
                    if f.exists(&p) {
                        let n = f.size_of(&p);
                        out.push((p, n));
                    }
                }
                out
            }
        }
    }
}

pub fn assign_keys(found: &mut [ArrangementInfo]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for a in found.iter_mut() {
        let base: String = a
            .name
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let base = if base.is_empty() { "arr".to_string() } else { base };
        let n = seen.entry(base.clone()).or_insert(0);
        *n += 1;
        a.key = if *n == 1 { base } else { format!("{base}{n}") };
    }
}

/// Pick a part: exact key, 1-based number, or a key/name prefix; None picks the first.
pub fn find_arrangement<'a>(
    arrs: &'a [ArrangementInfo],
    query: Option<&str>,
    what: &str,
) -> Result<&'a ArrangementInfo> {
    if arrs.is_empty() {
        return Err(Error::Arrangement(format!(
            "{what}: no instrument arrangements found"
        )));
    }
    let Some(q) = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty()) else {
        return Ok(&arrs[0]);
    };
    if let Some(a) = arrs.iter().find(|a| a.key == q) {
        return Ok(a);
    }
    if let Ok(n) = q.parse::<usize>()
        && (1..=arrs.len()).contains(&n)
    {
        return Ok(&arrs[n - 1]);
    }
    if let Some(a) = arrs
        .iter()
        .find(|a| a.key.starts_with(&q) || a.name.to_lowercase().starts_with(&q))
    {
        return Ok(a);
    }
    let options: Vec<&str> = arrs.iter().map(|a| a.key.as_str()).collect();
    Err(Error::Arrangement(format!(
        "no arrangement matching {:?}; available: {}",
        query.unwrap_or(""),
        options.join(", ")
    )))
}

/// Load one part at full difficulty from a .psarc, .feedpak/.sloppak, .sng or .xml file.
pub fn load_arrangement(path: &Path, query: Option<&str>, keys: &Keys) -> Result<Arrangement> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "psarc" | "feedpak" | "sloppak" => Package::open(path, keys, None)?.load(query, 1.0),
        "xml" => xml::parse_xml(&std::fs::read(path)?, 1.0),
        "sng" => {
            let file_stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let st = file_stem.rsplit('_').next().unwrap_or("").to_lowercase();
            let name = if ["lead", "rhythm", "combo", "bass"].contains(&st.as_str()) {
                capitalize(&st)
            } else {
                "Arrangement".into()
            };
            let chart = sng::load_sng(&std::fs::read(path)?, keys, Platform::Pc)?;
            sng::sng_to_arrangement(
                &chart,
                &ArrMeta {
                    name: &name,
                    is_bass: st.starts_with("bass"),
                    title: "",
                    artist: "",
                    album: "",
                },
                1.0,
            )
        }
        _ => Err(Error::Arrangement(format!(
            "unsupported file type {:?} (expected .psarc, .feedpak, .sng or .xml)",
            path.extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default()
        ))),
    }
}
