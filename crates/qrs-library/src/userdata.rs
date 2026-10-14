//! userdata.json: favourites, playlists, play history and best scores, in the Python app's shape (port of
//! rsplay/userdata.py, D6). Songs are identified by file name plus song key ("1979_p.psarc|",
//! "songs.psarc|WalkThisWay"), so they survive the library moving drive. New here: `aliases`, canonical song ids
//! ("artist|title") pointing at every source id a song was played from, so a psarc and its converted feedpak share
//! one history (PLAN.md section 5). Unknown keys survive a save.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Map, Value, json};

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
        d.join("userdata.json")
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
