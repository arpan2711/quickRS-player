//! userdata.json: favourites, playlists, play history and best scores, in the Python app's shape (port of
//! rsplay/userdata.py, D6). Songs are identified by file name plus song key ("1979_p.psarc|",
//! "songs.psarc|WalkThisWay"), so they survive the library moving drive. New here: `aliases`, canonical song ids
//! ("artist|title") pointing at every source id a song was played from, so a psarc and its converted feedpak share
//! one history (PLAN.md section 5). Also new: `licks`, stretches of songs saved from the player (P11.1), and
//! `lessons`, how far you've got with each lesson's riffs (P13.3), and `song_audio`, each song's pitch shift and
//! stem mix (P20.3). Unknown keys survive a save.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::lessons::RiffProgress;

use crate::paths::write_atomic;

/// Best runs kept per arrangement, with when they were played.
pub const TOP_N: usize = 3;

/// One of a part's best runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub score: i64,
    pub acc: Option<f64>,
    pub speed: f64,
    /// Epoch seconds (None for records from before the top list).
    pub at: Option<f64>,
}

/// A saved lick: a stretch of one part of a song (the loop it was saved from) and what it was in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lick {
    /// The song's id it was played from ("file name|song key"), and its canonical id ("artist|title").
    pub song: String,
    pub canonical: String,
    /// The part's name ("Lead") and key (its arrangement's).
    pub part: String,
    pub part_key: String,
    /// Chart seconds.
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub artist: String,
    pub tuning: String,
    /// The sections it falls in, in order ("Verse", "Chorus 2").
    pub sections: Vec<String>,
    /// First and last bar (bar numbers as the tab shows them).
    pub bars: Option<(i32, i32)>,
    /// A short name, e.g. "Chorus · bars 29-31".
    pub name: String,
    /// Epoch seconds.
    pub saved: f64,
    /// Fields this version doesn't use, kept as they were.
    pub other: Map<String, Value>,
}

impl Lick {
    /// "Chorus · bars 29-31" (the first section, the bars), or the time when there's neither.
    pub fn auto_name(sections: &[String], bars: Option<(i32, i32)>, start: f64) -> String {
        let mut bits: Vec<String> = Vec::new();
        if let Some(s) = sections.first() {
            bits.push(s.clone());
        }
        match bars {
            Some((a, b)) if a == b => bits.push(format!("bar {a}")),
            Some((a, b)) => bits.push(format!("bars {a}-{b}")),
            None => {}
        }
        if bits.is_empty() {
            let t = start.max(0.0) as i64;
            bits.push(format!("from {}:{:02}", t / 60, t % 60));
        }
        bits.join(" · ")
    }

    fn from_json(v: &Value) -> Option<Lick> {
        let mut m = v.as_object()?.clone();
        let mut take_str = |k: &str| match m.remove(k) {
            Some(Value::String(s)) => s,
            _ => String::new(),
        };
        let song = take_str("song");
        let canonical = take_str("canonical");
        let part = take_str("part");
        let part_key = take_str("part_key");
        let title = take_str("title");
        let artist = take_str("artist");
        let tuning = take_str("tuning");
        let name = take_str("name");
        if song.is_empty() && canonical.is_empty() {
            return None;
        }
        let start = m.remove("start").and_then(|v| v.as_f64())?;
        let end = m.remove("end").and_then(|v| v.as_f64())?;
        let sections = match m.remove("sections") {
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        };
        let bars = match m.remove("bars") {
            Some(Value::Array(a)) if a.len() == 2 => a[0]
                .as_i64()
                .zip(a[1].as_i64())
                .map(|(x, y)| (x as i32, y as i32)),
            _ => None,
        };
        let saved = m.remove("saved").and_then(|v| v.as_f64()).unwrap_or(0.0);
        Some(Lick {
            song,
            canonical,
            part,
            part_key,
            start,
            end,
            title,
            artist,
            tuning,
            sections,
            bars,
            name,
            saved,
            other: m,
        })
    }

    fn to_json(&self) -> Value {
        let mut m = self.other.clone();
        m.insert("song".into(), json!(self.song));
        m.insert("canonical".into(), json!(self.canonical));
        m.insert("part".into(), json!(self.part));
        m.insert("part_key".into(), json!(self.part_key));
        m.insert("start".into(), json!(round_to(self.start, 3)));
        m.insert("end".into(), json!(round_to(self.end, 3)));
        m.insert("title".into(), json!(self.title));
        m.insert("artist".into(), json!(self.artist));
        m.insert("tuning".into(), json!(self.tuning));
        m.insert("sections".into(), json!(self.sections));
        if let Some((a, b)) = self.bars {
            m.insert("bars".into(), json!([a, b]));
        }
        m.insert("name".into(), json!(self.name));
        m.insert("saved".into(), json!(self.saved));
        Value::Object(m)
    }
}

/// How one stem sits in a song's mix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StemSetting {
    pub gain: f64,
    pub on: bool,
    pub solo: bool,
}

/// A song's own audio settings (P20.3): its pitch shift (semitones) and, for a stemmed feedpak, its stem mix by
/// stem id (empty: the pack's own mix).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SongAudio {
    pub pitch: i32,
    pub stems: BTreeMap<String, StemSetting>,
}

impl SongAudio {
    pub fn is_default(&self) -> bool {
        self.pitch == 0 && self.stems.is_empty()
    }

    fn from_json(v: &Value) -> Option<SongAudio> {
        let m = v.as_object()?;
        let mut a = SongAudio {
            pitch: m.get("pitch").and_then(Value::as_i64).unwrap_or(0).clamp(-24, 24) as i32,
            ..Default::default()
        };
        if let Some(Value::Object(st)) = m.get("stems") {
            for (id, v) in st {
                let Some(o) = v.as_object() else { continue };
                a.stems.insert(
                    id.clone(),
                    StemSetting {
                        gain: o
                            .get("gain")
                            .and_then(Value::as_f64)
                            .unwrap_or(1.0)
                            .clamp(0.0, 4.0),
                        on: o.get("on").and_then(Value::as_bool).unwrap_or(true),
                        solo: o.get("solo").and_then(Value::as_bool).unwrap_or(false),
                    },
                );
            }
        }
        Some(a)
    }

    fn to_json(&self) -> Value {
        let mut m = Map::new();
        if self.pitch != 0 {
            m.insert("pitch".into(), json!(self.pitch));
        }
        if !self.stems.is_empty() {
            let mut st = Map::new();
            for (id, s) in &self.stems {
                st.insert(
                    id.clone(),
                    json!({"gain": round_to(s.gain, 3), "on": s.on, "solo": s.solo}),
                );
            }
            m.insert("stems".into(), Value::Object(st));
        }
        Value::Object(m)
    }
}

#[derive(Clone, Debug, Default)]
pub struct UserData {
    pub path: PathBuf,
    pub favorites: BTreeSet<String>,
    /// Name -> song ids, in the order the playlists were made.
    pub playlists: Vec<(String, Vec<String>)>,
    /// Song id -> part (lower case) -> record ({plays, completions, best, speed, best_acc, last, top}).
    pub scores: BTreeMap<String, Map<String, Value>>,
    /// Song id -> last time played (epoch seconds).
    pub recent: BTreeMap<String, f64>,
    /// Canonical id -> source ids.
    pub aliases: BTreeMap<String, BTreeSet<String>>,
    /// Saved licks, oldest first (as they were saved).
    pub licks: Vec<Lick>,
    /// Lesson id -> its progress ({"riffs": {riff id -> `RiffProgress`}, and anything else kept as it was}).
    pub lessons: Map<String, Value>,
    /// Song id -> its pitch shift and stem mix (only songs that were changed).
    pub song_audio: BTreeMap<String, SongAudio>,
    /// Keys this version doesn't use, kept as they were.
    pub other: Map<String, Value>,
    /// Goes up with every change, so views can cache what they count.
    pub version: u64,
}

/// "file name|song key", the Python app's id.
pub fn song_id(path: &std::path::Path, song_key: &str) -> String {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{name}|{song_key}")
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn round_to(v: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    // Python's round() is half-to-even on the decimal value
    (v * k).round_ties_even() / k
}

impl UserData {
    pub fn load(path: impl Into<PathBuf>) -> UserData {
        let path = path.into();
        let mut u = UserData {
            path: path.clone(),
            ..Default::default()
        };
        let Some(Value::Object(mut d)) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
        else {
            return u;
        };
        if let Some(Value::Array(f)) = d.remove("favorites") {
            u.favorites = f.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
        if let Some(Value::Object(p)) = d.remove("playlists") {
            u.playlists = p
                .into_iter()
                .map(|(k, v)| {
                    let ids = v
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    (k, ids)
                })
                .collect();
        }
        if let Some(Value::Object(s)) = d.remove("scores") {
            for (k, v) in s {
                if let Value::Object(m) = v {
                    u.scores.insert(k, m);
                }
            }
        }
        if let Some(Value::Object(r)) = d.remove("recent") {
            for (k, v) in r {
                if let Some(t) = v.as_f64() {
                    u.recent.insert(k, t);
                }
            }
        }
        if let Some(Value::Object(a)) = d.remove("aliases") {
            for (k, v) in a {
                let ids = v
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                u.aliases.insert(k, ids);
            }
        }
        if let Some(Value::Array(l)) = d.remove("licks") {
            u.licks = l.iter().filter_map(Lick::from_json).collect();
        }
        if let Some(Value::Object(l)) = d.remove("lessons") {
            u.lessons = l;
        }
        if let Some(Value::Object(a)) = d.remove("song_audio") {
            for (k, v) in a {
                if let Some(sa) = SongAudio::from_json(&v).filter(|s| !s.is_default()) {
                    u.song_audio.insert(k, sa);
                }
            }
        }
        u.other = d;
        u
    }

    pub fn to_json(&self) -> Value {
        let mut doc = Map::new();
        doc.insert(
            "favorites".into(),
            json!(self.favorites.iter().collect::<Vec<_>>()),
        );
        let mut pl = Map::new();
        for (k, v) in &self.playlists {
            pl.insert(k.clone(), json!(v));
        }
        doc.insert("playlists".into(), Value::Object(pl));
        let mut sc = Map::new();
        for (k, v) in &self.scores {
            sc.insert(k.clone(), Value::Object(v.clone()));
        }
        doc.insert("scores".into(), Value::Object(sc));
        let mut rc = Map::new();
        for (k, v) in &self.recent {
            rc.insert(k.clone(), json!(v));
        }
        doc.insert("recent".into(), Value::Object(rc));
        if !self.aliases.is_empty() {
            let mut al = Map::new();
            for (k, v) in &self.aliases {
                al.insert(k.clone(), json!(v.iter().collect::<Vec<_>>()));
            }
            doc.insert("aliases".into(), Value::Object(al));
        }
        if !self.licks.is_empty() {
            doc.insert(
                "licks".into(),
                Value::Array(self.licks.iter().map(Lick::to_json).collect()),
            );
        }
        if !self.lessons.is_empty() {
            doc.insert("lessons".into(), Value::Object(self.lessons.clone()));
        }
        if !self.song_audio.is_empty() {
            let mut sa = Map::new();
            for (k, v) in &self.song_audio {
                sa.insert(k.clone(), v.to_json());
            }
            doc.insert("song_audio".into(), Value::Object(sa));
        }
        for (k, v) in &self.other {
            doc.insert(k.clone(), v.clone());
        }
        Value::Object(doc)
    }

    pub fn save(&mut self) {
        self.version += 1;
        // the Python app writes indent=1
        let mut buf = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
        if serde::Serialize::serialize(&self.to_json(), &mut ser).is_ok() {
            let _ = write_atomic(&self.path, &String::from_utf8_lossy(&buf));
        }
    }

    // -- canonical ids ----------------------------------------------------------------------------------------

    /// Remember that `sid` is a source of the song `canonical` (when it changed, saves).
    pub fn alias(&mut self, canonical: &str, sid: &str) {
        if canonical.is_empty() {
            return;
        }
        if self
            .aliases
            .entry(canonical.to_string())
            .or_default()
            .insert(sid.to_string())
        {
            self.save();
        }
    }

    /// Every id a song's history may be under: its sources' ids and any others recorded for its canonical id.
    pub fn ids_for(&self, canonical: &str, sources: &[String]) -> Vec<String> {
        let mut out: Vec<String> = sources.to_vec();
        if let Some(a) = self.aliases.get(canonical) {
            for id in a {
                if !out.contains(id) {
                    out.push(id.clone());
                }
            }
        }
        out
    }

    // -- favourites and playlists ---------------------------------------------------------------------------

    pub fn is_favorite(&self, ids: &[String]) -> bool {
        ids.iter().any(|i| self.favorites.contains(i))
    }

    /// Toggle a song (all its ids) as a favourite; returns whether it is one now.
    pub fn toggle_favorite(&mut self, ids: &[String]) -> bool {
        let on = !self.is_favorite(ids);
        for i in ids {
            self.favorites.remove(i);
        }
        if on && let Some(first) = ids.first() {
            self.favorites.insert(first.clone());
        }
        self.save();
        on
    }

    pub fn create_playlist(&mut self, name: &str) -> String {
        let base = if name.trim().is_empty() {
            "Playlist".to_string()
        } else {
            name.trim().to_string()
        };
        let mut name = base.clone();
        let mut n = 2;
        while self.playlists.iter().any(|(k, _)| *k == name) {
            name = format!("{base} {n}");
            n += 1;
        }
        self.playlists.push((name.clone(), Vec::new()));
        self.save();
        name
    }

    pub fn rename_playlist(&mut self, old: &str, new: &str) -> String {
        let new = new.trim();
        if new.is_empty()
            || !self.playlists.iter().any(|(k, _)| k == old)
            || (new != old && self.playlists.iter().any(|(k, _)| k == new))
        {
            return old.to_string();
        }
        for (k, _) in self.playlists.iter_mut() {
            if k == old {
                *k = new.to_string();
            }
        }
        self.save();
        new.to_string()
    }

    pub fn delete_playlist(&mut self, name: &str) {
        self.playlists.retain(|(k, _)| k != name);
        self.save();
    }

    pub fn in_playlist(&self, name: &str, ids: &[String]) -> bool {
        self.playlists
            .iter()
            .any(|(k, v)| k == name && ids.iter().any(|i| v.contains(i)))
    }

    pub fn toggle_in_playlist(&mut self, name: &str, ids: &[String]) -> bool {
        let on = !self.in_playlist(name, ids);
        if !self.playlists.iter().any(|(k, _)| k == name) {
            self.playlists.push((name.to_string(), Vec::new()));
        }
        if let Some((_, v)) = self.playlists.iter_mut().find(|(k, _)| k == name) {
            v.retain(|i| !ids.contains(i));
            if on && let Some(first) = ids.first() {
                v.push(first.clone());
            }
        }
        self.save();
        on
    }

    // -- saved licks ------------------------------------------------------------------------------------------

    /// Save a lick (stamped now when it has no time); returns its index. The same stretch of the same part saved
    /// again replaces the older one (it moves to the top).
    pub fn add_lick(&mut self, mut lick: Lick) -> usize {
        if lick.saved <= 0.0 {
            lick.saved = now();
        }
        self.licks.retain(|l| {
            !(l.song == lick.song
                && l.part_key == lick.part_key
                && (l.start - lick.start).abs() < 0.01
                && (l.end - lick.end).abs() < 0.01)
        });
        self.licks.push(lick);
        self.save();
        self.licks.len() - 1
    }

    pub fn delete_lick(&mut self, i: usize) -> Option<Lick> {
        if i >= self.licks.len() {
            return None;
        }
        let l = self.licks.remove(i);
        self.save();
        Some(l)
    }

    /// The licks' indices, newest first.
    pub fn licks_newest_first(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.licks.len()).collect();
        v.sort_by(|&a, &b| {
            self.licks[b]
                .saved
                .total_cmp(&self.licks[a].saved)
                .then(b.cmp(&a))
        });
        v
    }

    // -- lessons (P13.3) --------------------------------------------------------------------------------------

    /// How far you've got with a lesson's riff (the default when it hasn't been opened).
    pub fn riff_progress(&self, lesson: &str, riff: &str) -> RiffProgress {
        self.lessons
            .get(lesson)
            .and_then(|l| l.get("riffs"))
            .and_then(|r| r.get(riff))
            .map(RiffProgress::from_json)
            .unwrap_or_default()
    }

    /// Keep a riff's progress (saves).
    pub fn set_riff_progress(&mut self, lesson: &str, riff: &str, p: &RiffProgress) {
        let l = self
            .lessons
            .entry(lesson.to_string())
            .or_insert_with(|| json!({}));
        if !l.is_object() {
            *l = json!({});
        }
        if let Some(lm) = l.as_object_mut() {
            let riffs = lm.entry("riffs").or_insert_with(|| json!({}));
            if !riffs.is_object() {
                *riffs = json!({});
            }
            if let Some(rm) = riffs.as_object_mut() {
                rm.insert(riff.to_string(), p.to_json());
            }
        }
        self.save();
    }

    /// Start a riff again from its first step (saves).
    pub fn forget_riff(&mut self, lesson: &str, riff: &str) {
        if let Some(rm) = self
            .lessons
            .get_mut(lesson)
            .and_then(|l| l.get_mut("riffs"))
            .and_then(|r| r.as_object_mut())
            && rm.remove(riff).is_some()
        {
            self.save();
        }
    }

    // -- a song's pitch and stem mix ---------------------------------------------------------------------------

    /// The song's pitch shift and stem mix (the default when it was never changed).
    pub fn song_audio(&self, sid: &str) -> SongAudio {
        self.song_audio.get(sid).cloned().unwrap_or_default()
    }

    /// Remember the song's pitch shift and stem mix (forgotten when back to the default); saves on a change.
    pub fn set_song_audio(&mut self, sid: &str, a: SongAudio) {
        let changed = if a.is_default() {
            self.song_audio.remove(sid).is_some()
        } else {
            self.song_audio.insert(sid.to_string(), a.clone()) != Some(a)
        };
        if changed {
            self.save();
        }
    }

    // -- history and scores -----------------------------------------------------------------------------------

    fn record(&mut self, sid: &str, part: &str) -> &mut Map<String, Value> {
        self.scores
            .entry(sid.to_string())
            .or_default()
            .entry(part.to_lowercase())
            .or_insert_with(|| Value::Object(Map::new()));
        match self
            .scores
            .get_mut(sid)
            .and_then(|m| m.get_mut(&part.to_lowercase()))
        {
            Some(Value::Object(m)) => m,
            _ => unreachable!("just inserted an object"),
        }
    }

    /// A part was started (counts a play, marks the song recently played).
    pub fn started(&mut self, sid: &str, part: &str) {
        self.recent.insert(sid.to_string(), now());
        let rec = self.record(sid, part);
        let plays = rec.get("plays").and_then(Value::as_i64).unwrap_or(0) + 1;
        rec.insert("plays".into(), json!(plays));
        self.save();
    }

    /// Record a finished run. Returns where it landed in the top 3 (0 = a new best), or None if it didn't.
    pub fn completed(
        &mut self,
        sid: &str,
        part: &str,
        score: i64,
        accuracy: f64,
        speed: f64,
    ) -> Option<usize> {
        let t = now();
        let rec = self.record(sid, part);
        let comps = rec.get("completions").and_then(Value::as_i64).unwrap_or(0) + 1;
        rec.insert("completions".into(), json!(comps));
        let mut top = top_of(rec);
        // ties: the older one stays up
        let rank = top.iter().position(|r| score > r.score).unwrap_or(top.len());
        top.insert(
            rank,
            Run {
                score,
                acc: Some(round_to(accuracy, 4)),
                speed: round_to(speed, 2),
                at: Some(t),
            },
        );
        top.truncate(TOP_N);
        let top_json: Vec<Value> = top
            .iter()
            .map(|r| json!({"score": r.score, "acc": r.acc, "speed": r.speed, "at": r.at}))
            .collect();
        rec.insert("top".into(), Value::Array(top_json));
        if rank == 0 {
            rec.insert("best".into(), json!(score));
            rec.insert("speed".into(), json!(round_to(speed, 2)));
        }
        let best_acc = rec
            .get("best_acc")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .max(round_to(accuracy, 4));
        rec.insert("best_acc".into(), json!(best_acc));
        rec.insert("last".into(), json!(t));
        self.save();
        (rank < TOP_N).then_some(rank)
    }

    /// Up to TOP_N best runs of one part across all of a song's ids, best first.
    pub fn top(&self, ids: &[String], part: &str) -> Vec<Run> {
        let mut all: Vec<Run> = ids
            .iter()
            .filter_map(|i| {
                self.scores
                    .get(i)
                    .and_then(|m| m.get(&part.to_lowercase()))
                    .and_then(Value::as_object)
            })
            .flat_map(top_of)
            .collect();
        all.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.at.unwrap_or(0.0).total_cmp(&b.at.unwrap_or(0.0)))
        });
        all.truncate(TOP_N);
        all
    }

    /// The best accuracy (0 to 1) of any part's runs at full speed: what the 70% / 80% / 90% playlists go by (a slowed
    /// run doesn't count).
    pub fn best_full_speed_acc(&self, ids: &[String]) -> Option<f64> {
        self.best_full_speed(ids).map(|b| b.0)
    }

    /// `best_full_speed_acc` with the part it was played on (its scores key, lower case: "lead", "bass"...); of
    /// equal bests, the first part in the scores' order.
    pub fn best_full_speed(&self, ids: &[String]) -> Option<(f64, String)> {
        let mut best: Option<(f64, String)> = None;
        for recs in ids.iter().filter_map(|i| self.scores.get(i)) {
            for (part, r) in recs {
                let Some(r) = r.as_object() else { continue };
                let rec = (r.get("speed").and_then(Value::as_f64).unwrap_or(1.0) >= 0.999)
                    .then(|| r.get("best_acc").and_then(Value::as_f64))
                    .flatten();
                let acc = top_of(r)
                    .into_iter()
                    .filter(|run| run.speed >= 0.999)
                    .filter_map(|run| run.acc)
                    .chain(rec)
                    .reduce(f64::max);
                if let Some(a) = acc
                    && best.as_ref().is_none_or(|b| a > b.0)
                {
                    best = Some((a, part.clone()));
                }
            }
        }
        best
    }

    /// The best record of one part (or, with None, the best across the song's parts by accuracy): (score, accuracy,
    /// speed).
    pub fn best(&self, ids: &[String], part: Option<&str>) -> Option<(i64, f64, f64)> {
        let mut best: Option<(i64, f64, f64)> = None;
        for i in ids {
            let Some(recs) = self.scores.get(i) else { continue };
            for (name, r) in recs {
                if part.is_some_and(|p| p.to_lowercase() != *name) {
                    continue;
                }
                let Some(r) = r.as_object() else { continue };
                let Some(score) = r.get("best").and_then(Value::as_i64) else {
                    continue;
                };
                let acc = r.get("best_acc").and_then(Value::as_f64).unwrap_or(0.0);
                let speed = r.get("speed").and_then(Value::as_f64).unwrap_or(1.0);
                let better = match (part, best) {
                    (_, None) => true,
                    (Some(_), Some((s, _, _))) => score > s,
                    (None, Some((_, a, _))) => acc > a,
                };
                if better {
                    best = Some((score, acc, speed));
                }
            }
        }
        best
    }

    /// Plays of a song (every part, every id).
    pub fn plays(&self, ids: &[String]) -> i64 {
        ids.iter()
            .filter_map(|i| self.scores.get(i))
            .flat_map(|m| m.values())
            .filter_map(|r| r.get("plays").and_then(Value::as_i64))
            .sum()
    }

    /// Plays of one part.
    pub fn part_plays(&self, ids: &[String], part: &str) -> i64 {
        ids.iter()
            .filter_map(|i| self.scores.get(i).and_then(|m| m.get(&part.to_lowercase())))
            .filter_map(|r| r.get("plays").and_then(Value::as_i64))
            .sum()
    }

    /// When the song was last played (any id).
    pub fn last_played(&self, ids: &[String]) -> Option<f64> {
        ids.iter()
            .filter_map(|i| self.recent.get(i).copied())
            .fold(None, |a, t| Some(a.map_or(t, |a: f64| a.max(t))))
    }
}

/// A record's top runs, best first. Records from before the top list have just their best, undated.
fn top_of(rec: &Map<String, Value>) -> Vec<Run> {
    if let Some(Value::Array(top)) = rec.get("top") {
        return top
            .iter()
            .filter_map(|r| {
                Some(Run {
                    score: r.get("score")?.as_i64()?,
                    acc: r.get("acc").and_then(Value::as_f64),
                    speed: r.get("speed").and_then(Value::as_f64).unwrap_or(1.0),
                    at: r.get("at").and_then(Value::as_f64),
                })
            })
            .collect();
    }
    if let Some(best) = rec.get("best").and_then(Value::as_i64) {
        return vec![Run {
            score: best,
            acc: rec.get("best_acc").and_then(Value::as_f64),
            speed: rec.get("speed").and_then(Value::as_f64).unwrap_or(1.0),
            at: None,
        }];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs-ud-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        // (afresh: the folder is named by the process id, which Windows reuses, so an old run's file could be there:
        // playlists_and_aliases found its "Gig" now and then)
        let f = d.join("userdata.json");
        let _ = std::fs::remove_file(&f);
        f
    }

    #[test]
    fn the_best_at_full_speed_names_its_part() {
        let p = tmp("fullspeed");
        std::fs::write(
            &p,
            r#"{"scores": {
                "a|": {"lead": {"best": 900, "best_acc": 0.95, "speed": 0.8,
                                "top": [{"score": 900, "acc": 0.95, "speed": 0.8}, {"score": 800, "acc": 0.71, "speed": 1.0}]},
                       "rhythm": {"best": 850, "best_acc": 0.893, "speed": 1.0}},
                "b|": {"bass": {"best": 10, "best_acc": 0.2, "speed": 0.5}}}}"#,
        )
        .unwrap();
        let u = UserData::load(&p);
        let a = vec!["a|".to_string()];
        // lead's 95% was slowed: rhythm's 89.3% at full speed is the best
        assert_eq!(u.best_full_speed(&a), Some((0.893, "rhythm".to_string())));
        assert_eq!(u.best_full_speed_acc(&a), Some(0.893));
        // only slowed runs: nothing
        assert_eq!(u.best_full_speed(&["b|".to_string()]), None);
        assert_eq!(u.best_full_speed(&["nope|".to_string()]), None);
    }

    #[test]
    fn python_records_read_and_runs_rank() {
        let p = tmp("rank");
        std::fs::write(
            &p,
            r#"{"favorites": ["a_p.psarc|"], "playlists": {"Gig": ["a_p.psarc|"]},
                "scores": {"a_p.psarc|": {"lead": {"plays": 3, "best": 1000, "best_acc": 0.5, "speed": 1.0}}},
                "recent": {"a_p.psarc|": 1700000000.0}, "future_key": 7}"#,
        )
        .unwrap();
        let mut u = UserData::load(&p);
        let ids = vec!["a_p.psarc|".to_string(), "A - Song.feedpak|".to_string()];
        assert!(u.is_favorite(&ids));
        assert_eq!(u.best(&ids, Some("Lead")), Some((1000, 0.5, 1.0)));
        assert_eq!(u.top(&ids, "lead").len(), 1);
        // a better run from the feedpak lands first; the old undated best stays second
        assert_eq!(u.completed(&ids[1], "Lead", 2000, 0.61234, 1.0), Some(0));
        let top = u.top(&ids, "lead");
        assert_eq!(top[0].score, 2000);
        assert_eq!(top[0].acc, Some(0.6123));
        assert_eq!(top[1].score, 1000);
        assert_eq!(u.completed(&ids[1], "lead", 10, 0.1, 1.0), Some(1));
        assert_eq!(u.plays(&ids), 3);
        u.started(&ids[1], "rhythm");
        assert_eq!(u.plays(&ids), 4);
        let back = UserData::load(&p);
        assert_eq!(back.other.get("future_key"), Some(&json!(7)));
        assert_eq!(back.playlists[0].0, "Gig");
        assert_eq!(back.scores["A - Song.feedpak|"]["lead"]["completions"], json!(2));
    }

    #[test]
    fn song_audio_round_trips_and_is_forgotten_at_default() {
        let p = tmp("song_audio");
        std::fs::write(
            &p,
            r#"{"favorites": [], "playlists": {}, "scores": {}, "recent": {}}"#,
        )
        .unwrap();
        let mut u = UserData::load(&p);
        assert!(u.song_audio("a.feedpak|").is_default());
        let mut a = SongAudio {
            pitch: -2,
            ..Default::default()
        };
        a.stems.insert(
            "guitar".into(),
            StemSetting {
                gain: 0.5,
                on: false,
                solo: false,
            },
        );
        u.set_song_audio("a.feedpak|", a.clone());
        let back = UserData::load(&p);
        assert_eq!(back.song_audio("a.feedpak|"), a);
        assert!(std::fs::read_to_string(&p).unwrap().contains("song_audio"));
        u.set_song_audio("a.feedpak|", SongAudio::default());
        let back = UserData::load(&p);
        assert!(back.song_audio.is_empty());
        assert!(!std::fs::read_to_string(&p).unwrap().contains("song_audio"));
    }

    #[test]
    fn licks_round_trip_and_keep_other_keys() {
        let p = tmp("licks");
        std::fs::write(
            &p,
            r#"{"favorites": ["a_p.psarc|"], "playlists": {}, "scores": {}, "recent": {}, "future_key": [1, 2],
                "licks": [{"song": "a_p.psarc|", "start": 1.0, "end": 2.0, "saved": 5.0, "colour": "red"},
                          {"start": 1.0, "end": 2.0}]}"#,
        )
        .unwrap();
        let mut u = UserData::load(&p);
        assert_eq!(u.licks.len(), 1, "a lick with no song is dropped");
        assert_eq!(u.licks[0].other.get("colour"), Some(&json!("red")));
        let sections = vec!["Chorus".to_string(), "Verse 2".to_string()];
        let i = u.add_lick(Lick {
            song: "b_p.psarc|".into(),
            canonical: "artist|title".into(),
            part: "Lead".into(),
            part_key: "lead0".into(),
            start: 61.23456,
            end: 72.5,
            title: "Title".into(),
            artist: "Artist".into(),
            tuning: "E Standard".into(),
            name: Lick::auto_name(&sections, Some((29, 31)), 61.2),
            sections: sections.clone(),
            bars: Some((29, 31)),
            saved: 0.0,
            other: Map::new(),
        });
        assert_eq!(i, 1);
        assert!(u.licks[1].saved > 5.0);
        assert_eq!(u.licks[1].name, "Chorus · bars 29-31");
        assert_eq!(u.licks_newest_first(), vec![1, 0]);
        let back = UserData::load(&p);
        assert_eq!(back.licks.len(), 2);
        assert_eq!(back.licks[1].start, 61.235);
        assert_eq!(back.licks[1].sections, sections);
        assert_eq!(back.licks[1].bars, Some((29, 31)));
        assert_eq!(back.licks[0].other.get("colour"), Some(&json!("red")));
        assert_eq!(back.other.get("future_key"), Some(&json!([1, 2])));
        assert!(back.favorites.contains("a_p.psarc|"));
        // the same stretch saved again replaces the old one; delete
        let mut u = back;
        let again = u.licks[1].clone();
        u.add_lick(Lick { saved: 0.0, ..again });
        assert_eq!(u.licks.len(), 2);
        assert_eq!(u.delete_lick(0).map(|l| l.song), Some("a_p.psarc|".to_string()));
        assert_eq!(UserData::load(&p).licks.len(), 1);
        // no licks: no key written
        u.delete_lick(0);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("\"licks\""));
        assert_eq!(Lick::auto_name(&[], None, 75.0), "from 1:15");
        assert_eq!(Lick::auto_name(&[], Some((4, 4)), 75.0), "bar 4");
    }

    #[test]
    fn lesson_progress_round_trips_and_keeps_other_keys() {
        let p = tmp("lessons");
        std::fs::write(
            &p,
            r#"{"favorites": [], "playlists": {}, "scores": {}, "recent": {},
                "lessons": {"by-ear-1": {"note": "mine", "riffs": {"smoke": {"step": "listen", "listens": 2,
                "mood": "good"}}}}}"#,
        )
        .unwrap();
        let mut u = UserData::load(&p);
        let mut smoke = u.riff_progress("by-ear-1", "smoke");
        assert_eq!((smoke.step.as_str(), smoke.listens), ("listen", 2));
        assert_eq!(u.riff_progress("by-ear-1", "other"), RiffProgress::default());
        assert_eq!(u.riff_progress("nope", "smoke"), RiffProgress::default());
        smoke.step = "notes".into();
        smoke.found = 3;
        u.set_riff_progress("by-ear-1", "smoke", &smoke);
        u.set_riff_progress("by-ear-2", "a", &RiffProgress::default());
        let back = UserData::load(&p);
        let s = back.riff_progress("by-ear-1", "smoke");
        assert_eq!((s.step.as_str(), s.found), ("notes", 3));
        assert_eq!(s.other.get("mood"), Some(&json!("good")));
        assert_eq!(back.lessons["by-ear-1"]["note"], json!("mine"));
        assert!(back.lessons.contains_key("by-ear-2"));
        let mut u = back;
        u.forget_riff("by-ear-1", "smoke");
        assert_eq!(
            UserData::load(&p).riff_progress("by-ear-1", "smoke"),
            RiffProgress::default()
        );
    }

    #[test]
    fn playlists_and_aliases() {
        let p = tmp("pl");
        let mut u = UserData::load(&p);
        let ids = vec!["x_p.psarc|".to_string()];
        assert_eq!(u.create_playlist("Gig"), "Gig");
        assert_eq!(u.create_playlist("Gig"), "Gig 2");
        assert!(u.toggle_in_playlist("Gig", &ids));
        assert!(u.in_playlist("Gig", &ids));
        assert_eq!(u.rename_playlist("Gig 2", "Gig"), "Gig 2");
        assert_eq!(u.rename_playlist("Gig 2", "Practice"), "Practice");
        u.alias("artist|title", "x_p.psarc|");
        u.alias("artist|title", "Artist - Title.feedpak|");
        let all = u.ids_for("artist|title", &["Artist - Title.feedpak|".to_string()]);
        assert_eq!(all.len(), 2);
    }
}
