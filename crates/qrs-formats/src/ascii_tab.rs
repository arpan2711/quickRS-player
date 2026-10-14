//! An Arrangement as plain-text tablature. Port of rsplay/ascii_tab.py (output is identical).
//!
//! Layout: notes are placed on a beat grid (default: 4 slots per beat, i.e. sixteenths) so spacing roughly follows
//! rhythm. A beat that holds more events than slots grows extra columns, so nothing is ever dropped. Bar lines come
//! from the chart's downbeats; sections start a new line with a header.

use std::collections::{BTreeMap, HashMap};

use crate::model::{Arrangement, Event, Note, Section, Techs};
use crate::package::capitalize;
use crate::sng::py_round;

pub const LEGEND: &str = "h hammer-on  p pull-off  / \\ slide  b bend  ~ vibrato  x fret-hand mute  \
<n> harmonic  n* pinch harmonic  t tap  PM palm mute";
/// Events closer than this share a column.
const SAME_TIME: f64 = 0.002;

pub fn note_token(n: &Note) -> String {
    let t = n.techniques;
    let mut core = if t.has(Techs::FRET_HAND_MUTE) {
        "x".to_string()
    } else if t.has(Techs::HARMONIC) {
        format!("<{}>", n.fret)
    } else {
        n.fret.to_string()
    };
    if t.has(Techs::PINCH_HARMONIC) {
        core.push('*');
    }
    let prefix = if t.has(Techs::HAMMER_ON) {
        "h"
    } else if t.has(Techs::PULL_OFF) {
        "p"
    } else if t.has(Techs::TAP) {
        "t"
    } else {
        ""
    };
    let mut suffix = String::new();
    if t.has(Techs::BEND) {
        suffix.push('b');
    }
    let target = if n.slide_to >= 0 {
        n.slide_to
    } else {
        n.slide_unpitch_to
    };
    if target >= 0 && target != n.fret {
        suffix.push(if target > n.fret { '/' } else { '\\' });
    }
    if t.has(Techs::VIBRATO) {
        suffix.push('~');
    }
    format!("{prefix}{core}{suffix}")
}

pub fn section_name(name: &str) -> String {
    let fixed = match name.to_lowercase().as_str() {
        "noguitar" => Some("No guitar"),
        "prechorus" => Some("Pre-chorus"),
        "postchorus" => Some("Post-chorus"),
        "modverse" => Some("Verse (mod)"),
        "modchorus" => Some("Chorus (mod)"),
        "modbridge" => Some("Bridge (mod)"),
        "buildup" => Some("Build-up"),
        "fadein" => Some("Fade in"),
        "fadeout" => Some("Fade out"),
        _ => None,
    };
    match fixed {
        Some(f) => f.to_string(),
        None => {
            let c = capitalize(name.replace('_', " ").trim());
            if c.is_empty() { "Section".into() } else { c }
        }
    }
}

pub fn section_label(s: &Section) -> String {
    let name = section_name(&s.name);
    if s.number > 1 {
        format!("{name} {}", s.number)
    } else {
        name
    }
}

#[derive(Clone, Debug)]
struct Column {
    time: f64,
    cells: HashMap<i32, String>,
    label: String,
    palm_mute: bool,
}

impl Column {
    fn width(&self) -> usize {
        self.cells.values().map(|c| c.chars().count()).max().unwrap_or(1)
    }
}

fn columns(arr: &Arrangement) -> Vec<Column> {
    let mut cols: Vec<Column> = Vec::new();
    for ev in arr.events() {
        let (time, notes, label, chord_pm) = match ev {
            Event::Note(n) => (n.time, vec![n], String::new(), false),
            Event::Chord(c) => (
                c.time,
                c.notes.iter().collect(),
                c.name.clone(),
                c.techniques.has(Techs::PALM_MUTE),
            ),
        };
        let mut cells = HashMap::new();
        for n in &notes {
            cells.insert(n.string, note_token(n));
        }
        let col = Column {
            time,
            cells,
            label,
            palm_mute: notes.iter().any(|n| n.has(Techs::PALM_MUTE)) || chord_pm,
        };
        if let Some(prev) = cols.last_mut()
            && col.time - prev.time < SAME_TIME
            && !prev.cells.keys().any(|k| col.cells.contains_key(k))
            && !(!prev.label.is_empty() && !col.label.is_empty())
        {
            prev.cells.extend(col.cells);
            if prev.label.is_empty() {
                prev.label = col.label;
            }
            prev.palm_mute = prev.palm_mute || col.palm_mute;
            continue;
        }
        cols.push(col);
    }
    cols
}

/// Beat times with downbeat flags, extended to cover every column with a spare beat at the end.
fn beat_grid(arr: &Arrangement, cols: &[Column]) -> Vec<(f64, bool)> {
    let mut beats: Vec<(f64, bool)> = arr.beats.iter().map(|b| (b.time, b.is_downbeat())).collect();
    beats.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    if beats.len() < 2 {
        beats = (0..2).map(|i| (i as f64 * 0.5, i % 4 == 0)).collect();
    }
    let first_step = (beats[1].0 - beats[0].0).max(0.05);
    let n = beats.len();
    let last_step = (beats[n - 1].0 - beats[n - 2].0).max(0.05);
    let downs: Vec<usize> = beats
        .iter()
        .enumerate()
        .filter(|(_, b)| b.1)
        .map(|(i, _)| i)
        .collect();
    let per_bar = if downs.len() >= 2 {
        downs[downs.len() - 1] - downs[downs.len() - 2]
    } else {
        4
    };

    let first = cols.first().map(|c| c.time).unwrap_or(beats[0].0);
    let last = cols.last().map(|c| c.time).unwrap_or(beats[n - 1].0);
    let mut pickup = Vec::new();
    let mut t = beats[0].0;
    while t > first + SAME_TIME {
        t -= first_step;
        pickup.push((t, false));
    }
    pickup.reverse();
    pickup.extend(beats);
    let mut beats = pickup;
    let last_down = beats
        .iter()
        .enumerate()
        .filter(|(_, b)| b.1)
        .map(|(i, _)| i)
        .max()
        .unwrap_or(0);
    let mut since_down = beats.len() - 1 - last_down;
    while beats.last().unwrap().0 <= last + last_step {
        since_down += 1;
        let t = beats.last().unwrap().0 + last_step;
        beats.push((t, since_down % per_bar.max(1) == 0));
    }
    beats
}

/// Place columns into per-beat slot lists (indices into `cols`).
fn assign(cols: &[Column], times: &[f64], resolution: usize) -> HashMap<usize, Vec<Option<usize>>> {
    let mut wanted: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    let mut order: Vec<usize> = Vec::new();
    for (ci, col) in cols.iter().enumerate() {
        let right = times.partition_point(|&t| t <= col.time);
        let mut j = (right as i64 - 1).clamp(0, times.len() as i64 - 2) as usize;
        let denom = times[j + 1] - times[j];
        let frac = if denom > 0.0 {
            (col.time - times[j]) / denom
        } else {
            0.0
        };
        let mut slot = py_round(frac * resolution as f64).max(0) as usize;
        if slot >= resolution && j + 2 < times.len() {
            j += 1;
            slot = 0;
        }
        if !wanted.contains_key(&j) {
            order.push(j);
        }
        wanted.entry(j).or_default().push((slot, ci));
    }
    let mut grid = HashMap::new();
    for (j, mut items) in wanted {
        items.sort_by(|a, b| cols[a.1].time.total_cmp(&cols[b.1].time));
        let mut placed = Vec::new();
        let mut prev: i64 = -1;
        for (slot, ci) in items {
            prev = (slot as i64).max(prev + 1);
            placed.push((prev as usize, ci));
        }
        let mut slots = vec![None; resolution.max((prev + 1) as usize)];
        for (s, ci) in placed {
            slots[s] = Some(ci);
        }
        grid.insert(j, slots);
    }
    grid
}

#[derive(Clone, Debug)]
struct Measure {
    number: i64,
    start: f64,
    /// One per string, low string first.
    rows: Vec<String>,
    /// (offset, chord name)
    marks: Vec<(usize, String)>,
    /// (offset, width, palm-muted) for every note column
    pm: Vec<(usize, usize, bool)>,
    section: String,
}

impl Measure {
    fn width(&self) -> usize {
        self.rows[0].chars().count()
    }
}

fn ljust(s: &str, w: usize, fill: char) -> String {
    let n = s.chars().count();
    let mut out = s.to_string();
    for _ in n..w {
        out.push(fill);
    }
    out
}

fn render_measure(
    number: i64,
    start: f64,
    beats: &[Vec<Option<usize>>],
    cols: &[Column],
    n_strings: usize,
) -> Measure {
    let mut rows: Vec<String> = vec!["-".to_string(); n_strings];
    let (mut marks, mut pm, mut pos) = (Vec::new(), Vec::new(), 1usize);
    for slots in beats {
        for col in slots {
            let col = col.map(|i| &cols[i]);
            let w = col.map(|c| c.width()).unwrap_or(1);
            for (s, row) in rows.iter_mut().enumerate() {
                let token = col
                    .and_then(|c| c.cells.get(&(s as i32)))
                    .map(|t| t.as_str())
                    .unwrap_or("");
                row.push_str(&ljust(token, w, '-'));
                row.push('-');
            }
            if let Some(c) = col {
                if !c.label.is_empty() {
                    marks.push((pos, c.label.clone()));
                }
                pm.push((pos, w, c.palm_mute));
            }
            pos += w + 1;
        }
    }
    Measure {
        number,
        start,
        rows,
        marks,
        pm,
        section: String::new(),
    }
}

fn rest_measure(number: i64, start: f64, bars: usize, n_strings: usize) -> Measure {
    Measure {
        number,
        start,
        rows: vec!["-".repeat(9); n_strings],
        marks: vec![(1, format!("{bars} bars"))],
        pm: Vec::new(),
        section: String::new(),
    }
}

fn annotation_rows(measures: &[Measure], offset0: usize, length: usize) -> Vec<String> {
    let mut chords = vec![' '; length];
    let mut pm = vec![' '; length];
    fn put(row: &mut [char], pos: usize, text: &str) -> bool {
        let t: Vec<char> = text.chars().collect();
        if pos + t.len() > row.len() || row[pos..pos + t.len()].iter().any(|c| *c != ' ') {
            return false;
        }
        row[pos..pos + t.len()].copy_from_slice(&t);
        true
    }
    let mut off = offset0;
    let mut flat: Vec<(usize, usize, bool)> = Vec::new();
    for m in measures {
        for (pos, name) in &m.marks {
            put(&mut chords, off + pos, name);
        }
        flat.extend(m.pm.iter().map(|&(pos, w, is_pm)| (off + pos, w, is_pm)));
        off += m.width() + 1;
    }
    let (mut run_start, mut run_end): (Option<usize>, usize) = (None, 0);
    for item in flat.iter().map(|x| Some(*x)).chain(std::iter::once(None)) {
        let (pos, w, is_pm) = item.unwrap_or((0, 0, false));
        if item.is_some() && is_pm {
            run_start = Some(run_start.unwrap_or(pos));
            run_end = pos + w;
        } else if let Some(rs) = run_start {
            let span = (run_end - rs).max(2);
            let text: String = format!("PM{}", "-".repeat(span)).chars().take(span).collect();
            put(&mut pm, rs, &text);
            run_start = None;
        }
    }
    [chords, pm]
        .iter()
        .map(|r| r.iter().collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim_end().to_string())
        .collect()
}

/// The arrangement as ASCII tab text.
pub fn render(arr: &Arrangement, width: usize, resolution: usize, header: bool) -> String {
    let n_strings = arr.string_count();
    let mut names: Vec<String> = arr.string_names().iter().map(|s| s.to_string()).collect();
    if n_strings >= 2 && names[names.len() - 1] == names[0] {
        let last = names.len() - 1;
        names[last] = names[last].to_lowercase();
    }
    let label_w = names.iter().map(|n| n.chars().count()).max().unwrap_or(1);
    let labels: Vec<String> = names.iter().map(|n| format!("{n:>label_w$}")).collect();

    let mut out: Vec<String> = Vec::new();
    if header {
        let title: Vec<&str> = [arr.title.as_str(), arr.artist.as_str()]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect();
        let title = if title.is_empty() {
            "Untitled".to_string()
        } else {
            title.join(" - ")
        };
        let tuning = format!("{} ({})", arr.tuning_name(), arr.string_names().join(" "));
        let capo = if arr.capo != 0 {
            format!(" | Capo: {}", arr.capo)
        } else {
            String::new()
        };
        let name = if arr.name.is_empty() {
            "?"
        } else {
            arr.name.as_str()
        };
        out.push(title);
        out.push(format!("Arrangement: {name} | Tuning: {tuning}{capo}"));
        out.push(format!("Legend: {LEGEND}"));
        out.push(String::new());
    }

    let cols = columns(arr);
    if cols.is_empty() {
        out.push("(no notes)".into());
        return out.join("\n") + "\n";
    }

    let beats = beat_grid(arr, &cols);
    let times: Vec<f64> = beats.iter().map(|b| b.0).collect();
    let grid = assign(&cols, &times, resolution);
    let mut starts = vec![0usize];
    starts.extend((1..beats.len() - 1).filter(|&i| beats[i].1));
    let has_pickup = !beats[0].1;

    let mut measures: Vec<Measure> = Vec::new();
    let mut is_empty: Vec<bool> = Vec::new();
    for (k, &b0) in starts.iter().enumerate() {
        let b1 = starts.get(k + 1).copied().unwrap_or(beats.len() - 1);
        let number = if has_pickup { k as i64 } else { k as i64 + 1 };
        let slot_lists: Vec<Vec<Option<usize>>> = (b0..b1)
            .map(|j| grid.get(&j).cloned().unwrap_or_else(|| vec![None; resolution]))
            .collect();
        is_empty.push(slot_lists.iter().all(|s| s.iter().all(|c| c.is_none())));
        measures.push(render_measure(number, times[b0], &slot_lists, &cols, n_strings));
    }

    // Attach section headers to the measure in which each section starts.
    let mstarts: Vec<f64> = measures.iter().map(|m| m.start).collect();
    for s in &arr.sections {
        let idx = (mstarts.partition_point(|&t| t <= s.start + SAME_TIME) as i64 - 1).max(0) as usize;
        if measures[idx].section.is_empty() {
            measures[idx].section = section_label(s);
        }
    }

    // Trim empty bars at both ends; collapse runs of 2+ empty bars into one rest marker.
    let first = is_empty.iter().position(|e| !e).unwrap_or(0);
    let last = is_empty.iter().rposition(|e| !e).unwrap_or(0);
    let mut compact: Vec<Measure> = Vec::new();
    let mut i = first;
    while i <= last {
        if is_empty[i] {
            let mut j = i;
            while j <= last && is_empty[j] {
                j += 1;
            }
            if j - i >= 2 {
                let mut rest = rest_measure(measures[i].number, measures[i].start, j - i, n_strings);
                rest.section = measures[i..j]
                    .iter()
                    .find(|m| !m.section.is_empty())
                    .map(|m| m.section.clone())
                    .unwrap_or_default();
                compact.push(rest);
                i = j;
                continue;
            }
        }
        compact.push(measures[i].clone());
        i += 1;
    }

    // Lay measures out into lines.
    let prefix = label_w + 1;
    let mut lines: Vec<Vec<Measure>> = Vec::new();
    for m in compact {
        let start_new = match lines.last() {
            None => true,
            Some(cur) => {
                let used = prefix + cur.iter().map(|x| x.width() + 1).sum::<usize>();
                !m.section.is_empty() || used + m.width() + 1 > width
            }
        };
        if start_new {
            lines.push(vec![m]);
        } else {
            lines.last_mut().unwrap().push(m);
        }
    }

    for line in &lines {
        if !line[0].section.is_empty() {
            out.push(format!("[{}]  bar {}", line[0].section, line[0].number));
        }
        let total = prefix + line.iter().map(|m| m.width() + 1).sum::<usize>();
        out.extend(annotation_rows(line, prefix, total));
        for s in (0..n_strings).rev() {
            let bars: Vec<&str> = line.iter().map(|m| m.rows[s].as_str()).collect();
            out.push(format!("{}|{}|", labels[s], bars.join("|")));
        }
        out.push(String::new());
    }
    out.join("\n").trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Beat;
    use crate::sng::{ArrMeta, parse_sng, sng_to_arrangement};
    use crate::testutil::demo_sng_plain;

    fn demo() -> Arrangement {
        let sng = parse_sng(&demo_sng_plain()).unwrap();
        sng_to_arrangement(
            &sng,
            &ArrMeta {
                name: "Lead",
                is_bass: false,
                title: "Demo Song",
                artist: "Test Band",
                album: "",
            },
            1.0,
        )
        .unwrap()
    }

    fn n(fret: i32, techs: Techs) -> Note {
        Note {
            techniques: techs,
            ..Note::new(0.0, 0, fret)
        }
    }

    #[test]
    fn note_tokens() {
        let cases: Vec<(Note, &str)> = vec![
            (n(5, Techs::NONE), "5"),
            (n(7, Techs::HAMMER_ON), "h7"),
            (n(5, Techs::PULL_OFF), "p5"),
            (n(12, Techs::TAP), "t12"),
            (n(7, Techs::BEND), "7b"),
            (
                Note {
                    slide_to: 7,
                    ..n(5, Techs::SLIDE)
                },
                "5/",
            ),
            (
                Note {
                    slide_to: 5,
                    ..n(7, Techs::SLIDE)
                },
                "7\\",
            ),
            (
                Note {
                    slide_unpitch_to: 1,
                    ..n(5, Techs::UNPITCHED_SLIDE)
                },
                "5\\",
            ),
            (n(9, Techs::VIBRATO), "9~"),
            (n(3, Techs::FRET_HAND_MUTE), "x"),
            (n(12, Techs::HARMONIC), "<12>"),
            (n(5, Techs::PINCH_HARMONIC), "5*"),
            (n(7, Techs::BEND | Techs::VIBRATO), "7b~"),
        ];
        for (note, token) in cases {
            assert_eq!(note_token(&note), token);
        }
    }

    #[test]
    fn render_contents() {
        let text = render(&demo(), 100, 4, true);
        assert!(
            text.starts_with("Demo Song - Test Band\nArrangement: Lead | Tuning: Drop D (D A D G B E)"),
            "{text}"
        );
        assert!(text.contains("[Riff]") && text.contains("[No guitar]"));
        for token in ["h5", "p5", "7b", "5/", "<12>", "E5", "PM"] {
            assert!(text.contains(token), "{token}\n{text}");
        }
        assert!(!text.contains("99"));
    }

    #[test]
    fn staff_lines_are_aligned_and_labelled() {
        let text = render(&demo(), 100, 4, false);
        let staves: Vec<&str> = text
            .lines()
            .filter(|l| l.len() > 1 && l.as_bytes()[1] == b'|' && !l[..1].trim().is_empty())
            .collect();
        assert!(!staves.is_empty());
        for block in staves.chunks(6) {
            let firsts: Vec<char> = block.iter().map(|l| l.chars().next().unwrap()).collect();
            assert_eq!(firsts, vec!['E', 'B', 'G', 'D', 'A', 'D']);
            assert!(block.iter().all(|l| l.len() == block[0].len()));
        }
    }

    #[test]
    fn width_is_respected() {
        let d = demo();
        let widest = render(&d, 1000, 4, false).lines().map(|l| l.len()).max().unwrap();
        for width in [60, 100] {
            for line in render(&d, width, 4, false).lines() {
                assert!(line.len() <= width.max(widest));
            }
        }
    }

    fn beats(n: i32) -> Vec<Beat> {
        (0..n)
            .map(|i| Beat {
                time: i as f64 * 0.5,
                measure: if i % 4 == 0 { 1 } else { -1 },
            })
            .collect()
    }

    #[test]
    fn bass_uses_four_strings() {
        let mut a = Arrangement::new("Bass");
        a.is_bass = true;
        a.notes = vec![Note::new(0.25, 0, 3), Note::new(0.5, 3, 5)];
        a.beats = beats(8);
        let text = render(&a, 100, 4, false);
        let staff: Vec<char> = text
            .lines()
            .filter(|l| l.contains('|'))
            .map(|l| l.chars().next().unwrap())
            .collect();
        assert_eq!(staff, vec!['G', 'D', 'A', 'E']);
    }

    #[test]
    fn crowded_beat_grows() {
        let mut a = Arrangement::new("Lead");
        a.notes = (0..8).map(|i| Note::new(0.5 + i as f64 * 0.05, 0, i)).collect();
        a.beats = beats(8);
        let text = render(&a, 100, 4, false);
        let low_e = text.lines().find(|l| l.starts_with("E|")).unwrap();
        assert!(low_e.contains("0-1-2-3-4-5-6-7"), "{low_e}");
    }

    #[test]
    fn long_rests_are_collapsed() {
        let mut a = Arrangement::new("Lead");
        a.notes = vec![Note::new(0.0, 0, 1), Note::new(20.0, 0, 2)];
        a.beats = beats(48);
        assert!(render(&a, 100, 4, false).contains("9 bars"));
    }

    #[test]
    fn no_beats_and_no_notes() {
        assert!(render(&Arrangement::new("Lead"), 100, 4, true).contains("(no notes)"));
        let mut a = Arrangement::new("Lead");
        a.notes = vec![Note::new(3.0, 2, 4)];
        assert!(render(&a, 100, 4, false).contains('4'));
    }
}
