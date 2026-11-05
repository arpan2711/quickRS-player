//! The last runs of each song's part, for its review (new in quickRS, P29.3, owner request): `reviews.json` in the
//! data folder (not userdata.json, which keeps the Python app's shape), one entry per song and part ("<song id>|<part
//! name>") holding its newest `KEEP` runs, newest first: when, the accuracy, and the judged notes (time in ms and a
//! code: 0 hit, 1 late, 2 early, 3 miss). The review is made again from the notes and the chart when one is picked.
//! Since P34.6 a run also keeps its review's gist (the parts listed, the worst one, the accuracy cleaning them up
//! would make), for Home's Carry on card, which has no chart to make the review from.

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
    /// Its review in short (runs saved before P34.6 have none).
    pub gist: Option<Gist>,
}

/// A review in short, for Home.
#[derive(Clone, Debug, PartialEq)]
pub struct Gist {
    /// The parts listed to practise.
    pub spots: usize,
    /// The one that cost the most: "Breakdown riff, bars 41–44", "Main riff, plays 6×"; "" with none.
    pub worst: String,
    /// The run's accuracy with the listed parts played clean.
    pub potential: f64,
}

/// Home's lines on a part's last review (P34.6): an encouraging headline, the worst part, and what cleaning up
/// would make; or, for a run saved without its gist, the headline from its accuracy and where to look.
pub fn gist_lines(acc: f64, gist: Option<&Gist>) -> Vec<String> {
    let pc = |a: f64| format!("{}%", (a * 100.0).round() as i64);
    let Some(g) = gist else {
        let head = match acc {
            a if a >= 0.995 => "A perfect run last time".to_string(),
            a if a >= 0.95 => format!("Almost there: {} last time", pc(a)),
            a if a >= 0.85 => format!("A little here and there: {}", pc(a)),
            a if a >= 0.7 => format!("Getting there: {} last time", pc(a)),
            a => format!("A tough one: {} last time", pc(a)),
        };
        return vec![head, "Open it to see where to practise".to_string()];
    };
    let n = g.spots;
    let spots = if n == 1 {
        "1 spot".to_string()
    } else {
        format!("{n} spots")
    };
    let head = match acc {
        _ if n == 0 && acc >= 0.995 => "A perfect run: nothing to clean up".to_string(),
        _ if n == 0 => "Clean: no spot lost a whole note".to_string(),
        a if a >= 0.95 => format!("Almost there: {spots} left to clean up"),
        a if a >= 0.85 => format!("A little here and there: {spots}"),
        a if a >= 0.7 => format!("Getting there: {spots} to work on"),
        _ => format!("A tough one: {spots}, start with the worst"),
    };
    let mut out = vec![head];
    if !g.worst.is_empty() {
        out.push(g.worst.clone());
    }
    if n > 0 && g.potential > acc + 0.005 {
        out.push(format!("Clean them up and you'd be at {}", pc(g.potential)));
    }
    out
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
    let mut v = json!({"at": r.at, "acc": r.acc, "notes": notes});
    if let Some(g) = &r.gist {
        v["gist"] = json!({"spots": g.spots, "worst": g.worst, "potential": g.potential});
    }
    v
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
        gist: v.get("gist").and_then(|g| {
            Some(Gist {
                spots: g.get("spots")?.as_u64()? as usize,
                worst: g.get("worst").and_then(Value::as_str).unwrap_or("").to_string(),
                potential: g.get("potential").and_then(Value::as_f64).unwrap_or(0.0),
            })
        }),
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
            gist: None,
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

    #[test]
    fn a_gist_is_kept_with_its_run() {
        let dir = std::env::temp_dir().join(format!("qrs-gist-{}", std::process::id()));
        let path = dir.join("reviews.json");
        let _ = std::fs::remove_file(&path);
        let gist = Gist {
            spots: 2,
            worst: "Main riff, plays 6×".into(),
            potential: 0.97,
        };
        save(
            &path,
            "s|Lead",
            SavedRun {
                gist: Some(gist.clone()),
                ..run(1.0)
            },
        )
        .unwrap();
        save(&path, "s|Lead", run(2.0)).unwrap();
        let runs = load(&path, "s|Lead");
        assert_eq!(runs[0].gist, None);
        assert_eq!(runs[1].gist, Some(gist));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn home_lines_cheer_and_name_the_worst() {
        let g = |spots, worst: &str, potential| Gist {
            spots,
            worst: worst.into(),
            potential,
        };
        assert_eq!(
            gist_lines(0.96, Some(&g(2, "Breakdown riff, bars 41–44", 0.99))),
            vec![
                "Almost there: 2 spots left to clean up",
                "Breakdown riff, bars 41–44",
                "Clean them up and you'd be at 99%",
            ]
        );
        assert_eq!(
            gist_lines(0.88, Some(&g(1, "Chorus 2, bars 30–33", 0.9)))[0],
            "A little here and there: 1 spot"
        );
        assert_eq!(
            gist_lines(0.6, Some(&g(5, "Solo", 0.8)))[0],
            "A tough one: 5 spots, start with the worst"
        );
        assert_eq!(
            gist_lines(1.0, Some(&g(0, "", 1.0))),
            vec!["A perfect run: nothing to clean up"]
        );
        // a run saved before gists: its accuracy only
        assert_eq!(
            gist_lines(0.952, None),
            vec!["Almost there: 95% last time", "Open it to see where to practise"]
        );
    }
}
