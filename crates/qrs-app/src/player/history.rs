//! The last runs of each song's part, for its review (new in quickRS, P29.3, owner request): `reviews.json` in the
//! data folder (not userdata.json, which keeps the Python app's shape), one entry per song and part ("<song id>|<part
//! name>") holding its newest `KEEP` runs, newest first: when, the accuracy, and the judged notes (time in ms and a
//! code: 0 hit, 1 late, 2 early, 3 miss). The review is made again from the notes and the chart when one is picked.

use std::path::Path;

use qrs_analysis::Judgement;
use serde_json::{Map, Value, json};

/// Runs kept per song and part.
pub const KEEP: usize = 4;

/// A run as saved.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedRun {
    /// Seconds since 1970.
    pub at: f64,
    pub acc: f64,
    pub notes: Vec<(f64, Judgement)>,
}

fn code(j: Judgement) -> u8 {
    match j {
        Judgement::Hit => 0,
        Judgement::Late => 1,
        Judgement::Early => 2,
        Judgement::Miss => 3,
    }
}

fn judgement(c: u64) -> Option<Judgement> {
    Some(match c {
        0 => Judgement::Hit,
        1 => Judgement::Late,
        2 => Judgement::Early,
        3 => Judgement::Miss,
        _ => return None,
    })
}

fn to_json(r: &SavedRun) -> Value {
    let notes: Vec<Value> = r
        .notes
        .iter()
        .map(|&(t, j)| json!([(t * 1000.0).round() as i64, code(j)]))
        .collect();
    json!({"at": r.at, "acc": r.acc, "notes": notes})
}

fn from_json(v: &Value) -> Option<SavedRun> {
    let notes = v
        .get("notes")?
        .as_array()?
        .iter()
        .filter_map(|n| {
            let a = n.as_array()?;
            Some((
                a.first()?.as_i64()? as f64 / 1000.0,
                judgement(a.get(1)?.as_u64()?)?,
            ))
        })
        .collect();
    Some(SavedRun {
        at: v.get("at")?.as_f64()?,
        acc: v.get("acc").and_then(Value::as_f64).unwrap_or(0.0),
        notes,
    })
}

fn read(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// The runs saved for `key`, newest first.
pub fn load(path: &Path, key: &str) -> Vec<SavedRun> {
    read(path)
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(from_json).collect())
        .unwrap_or_default()
}

/// Save a run for `key` (the oldest past KEEP goes); the runs now, newest first.
pub fn save(path: &Path, key: &str, run: SavedRun) -> std::io::Result<Vec<SavedRun>> {
    let mut all = read(path);
    let mut runs = load(path, key);
    runs.insert(0, run);
    runs.truncate(KEEP);
    all.insert(key.to_string(), Value::Array(runs.iter().map(to_json).collect()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // (written beside it, then moved over it: a crash mid-write leaves the old file)
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string(&Value::Object(all)).unwrap_or_default(),
    )?;
    std::fs::rename(&tmp, path)?;
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(at: f64) -> SavedRun {
        SavedRun {
            at,
            acc: 0.5,
            notes: vec![
                (1.25, Judgement::Hit),
                (2.5, Judgement::Miss),
                (3.0, Judgement::Early),
            ],
        }
    }

    #[test]
    fn the_newest_four_are_kept_newest_first() {
        let dir = std::env::temp_dir().join(format!("qrs-reviews-{}", std::process::id()));
        let path = dir.join("reviews.json");
        let _ = std::fs::remove_file(&path);
        for k in 0..6 {
            save(&path, "song|Lead", run(k as f64)).unwrap();
        }
        save(&path, "other|Bass", run(9.0)).unwrap();
        let runs = load(&path, "song|Lead");
        assert_eq!(
            runs.iter().map(|r| r.at).collect::<Vec<_>>(),
            vec![5.0, 4.0, 3.0, 2.0]
        );
        assert_eq!(runs[0].notes, run(5.0).notes);
        assert_eq!(load(&path, "other|Bass").len(), 1);
        assert!(load(&path, "nothing").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
