//! Write one song from a Rocksmith 2014 .psarc as a feedpak (spec 1.19.0, got-feedback.github.io/feedpak-spec):
//! the port of rsfeedpak/export.py (P9.3). What goes where:
//!
//!   manifest.yaml           title, artist, album, year, duration, the parts (tuning, capo, cent offset, type),
//!                           the stems and side-file pointers, and a `rocksmith` block (the source file and song
//!                           key, sort names, average tempo) that other readers ignore
//!   arrangements/<id>.json  every part at full difficulty, plus `phrases` with every level of Rocksmith's
//!                           difficulty ladder, the chord templates, anchors, hand shapes and tone changes
//!   song_timeline.json      beats and the main part's sections (a part keeps its own under `rocksmith` when they
//!                           differ)
//!   stems/full.ogg          the song, rewrapped from Wwise Vorbis without re-encoding (qrs_formats::wem)
//!   preview.ogg             Rocksmith's 30-second song-list preview
//!   lyrics.json             the vocals chart; Japanese songs also get a romaji track (lyric_tracks)
//!   vocal_pitch.json        only when the vocals carry a real melody
//!   cover.png               the 256 px album art
//!
//! Rocksmith maps onto feedpak's note fields as in Rocksmith2014.NET and feedBack's own import: a single note's
//! fret-hand mute is `mt`, a chord-wide one `fhm` on each chord note; chord-wide palm mute, accent and ignore go
//! on each chord note; bends (`bn`, `bnv`) are in semitones. The text is written as the Python exporter writes it,
//! so the two give the same pack (bar the converter's name). Audio that can't be rewrapped would need re-encoding,
//! which this exporter doesn't do (the Python one re-encoded with vgmstream; no song of the owner's needed it).

pub mod folder;
pub mod text;

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use qrs_formats::package::{ChartRef, stem};
use qrs_formats::sng::{
    self, MASK_ACCENT, MASK_FRET_HAND_MUTE, MASK_HAMMER_ON, MASK_HARMONIC, MASK_HIGH_DENSITY, MASK_IGNORE,
    MASK_MUTE, MASK_PALM_MUTE, MASK_PARENT, MASK_PINCH_HARMONIC, MASK_PLUCK, MASK_PULL_OFF, MASK_SLAP,
    MASK_TAP, MASK_TREMOLO, MASK_VIBRATO, Sng, SngChordTemplate, SngLevel, SngNote,
};
use qrs_formats::{ArrangementInfo, Keys, Package};
use serde_json::{Map, Value, json};

use text::{manifest_yaml, to_json};

pub const FEEDPAK_VERSION: &str = "1.19.0";
const JSON_WIDTH: usize = 180;

/// Note mask bits -> feedpak boolean fields (single notes and chord notes alike).
const NOTE_FLAGS: [(u32, &str); 14] = [
    (MASK_HAMMER_ON, "ho"),
    (MASK_PULL_OFF, "po"),
    (MASK_HARMONIC, "hm"),
    (MASK_PINCH_HARMONIC, "hp"),
    (MASK_PALM_MUTE, "pm"),
    (MASK_MUTE, "mt"),
    (MASK_VIBRATO, "vb"),
    (MASK_TREMOLO, "tr"),
    (MASK_ACCENT, "ac"),
    (MASK_TAP, "tp"),
    (MASK_PARENT, "ln"),
    (MASK_PLUCK, "plk"),
    (MASK_SLAP, "slp"),
    (MASK_IGNORE, "ig"),
];
/// A chord's own mask bits that apply to every note in it.
const CHORD_WIDE: [(u32, &str); 4] = [
    (MASK_PALM_MUTE, "pm"),
    (MASK_FRET_HAND_MUTE, "fhm"),
    (MASK_ACCENT, "ac"),
    (MASK_IGNORE, "ig"),
];
/// Field order inside a note object (the spec's listing).
const FIELD_ORDER: [&str; 26] = [
    "t", "s", "f", "sus", "sl", "slu", "bn", "bt", "bnv", "ho", "po", "hm", "hp", "pm", "mt", "vb", "tr",
    "ac", "tp", "ln", "fhm", "plk", "slp", "pkd", "fg", "ig",
];

#[derive(Debug)]
pub struct ExportError(pub String);

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExportError {}

impl From<qrs_formats::Error> for ExportError {
    fn from(e: qrs_formats::Error) -> Self {
        ExportError(e.to_string())
    }
}

type Res<T> = Result<T, ExportError>;

fn err<T>(m: impl Into<String>) -> Res<T> {
    Err(ExportError(m.into()))
}

/// Times and lengths to the millisecond (Python's `round(x, 3)`: correctly rounded, ties to even).
pub fn r3(x: f64) -> f64 {
    let v: f64 = format!("{x:.3}").parse().unwrap_or(0.0);
    if v == 0.0 { 0.0 } else { v }
}

fn num(x: f64) -> Value {
    json!(r3(x))
}

// -- notes ----------------------------------------------------------------------------------------------------
/// feedpak `bt`: 0 bend up, 1 release, 2 pre-bend, 3 pre-bend and release, 4 round trip.
fn bend_intent(curve: &[(f64, f64)], continues_bend: bool) -> i64 {
    let peak = curve.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let starts_bent = curve[0].1 > 0.0;
    let released = curve[curve.len() - 1].1 < peak - 1e-3;
    if starts_bent {
        if continues_bend {
            return if released { 1 } else { 0 };
        }
        return if released { 3 } else { 2 };
    }
    if released { 4 } else { 0 }
}

/// Fill bn / bnv / bt (Rocksmith's bend steps are half steps: semitones). Returns the bend at the note's end.
fn add_bend(
    d: &mut Vec<(&'static str, Value)>,
    onset: f64,
    max_bend: f64,
    points: &[(f32, f32)],
    continues: bool,
) -> f64 {
    let mut pts: Vec<(f64, f64)> = points
        .iter()
        .map(|&(t, s)| ((t as f64 - onset).max(0.0), s as f64))
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let peak = pts.iter().map(|p| p.1).fold(max_bend, f64::max);
    if peak <= 0.0 {
        return 0.0;
    }
    d.push(("bn", num(peak)));
    if pts.is_empty() {
        return peak;
    }
    // Rocksmith bends from the unbent pitch unless the first point sits on the note's start (a pre-bend)
    let curve: Vec<(f64, f64)> = if pts[0].0 <= 0.0005 {
        pts
    } else {
        std::iter::once((0.0, 0.0)).chain(pts).collect()
    };
    d.push((
        "bnv",
        Value::Array(
            curve
                .iter()
                .map(|&(t, v)| json!({"t": r3(t), "v": r3(v)}))
                .collect(),
        ),
    ));
    let bt = bend_intent(&curve, continues);
    if bt != 0 {
        d.push(("bt", json!(bt)));
    }
    curve[curve.len() - 1].1
}

fn ordered(mut d: Vec<(&'static str, Value)>) -> Value {
    let idx = |k: &str| FIELD_ORDER.iter().position(|x| *x == k).unwrap_or(99);
    d.sort_by_key(|(k, _)| idx(k)); // stable: duplicates keep their order
    let mut m = Map::new();
    for (k, v) in d {
        m.insert(k.into(), v);
    }
    Value::Object(m)
}

/// Per string: whether the last note links into the next one while bent (a linked note that starts bent is then
/// a release or a held bend, not a pre-bend).
#[derive(Default)]
struct BendContext {
    held: HashMap<i32, bool>,
}

impl BendContext {
    fn take(&mut self, string: i32) -> bool {
        self.held.remove(&string).unwrap_or(false)
    }
    fn set(&mut self, string: i32, linked: bool, end_bend: f64) {
        self.held.insert(string, linked && end_bend > 0.0);
    }
}

fn note(n: &SngNote, ctx: &mut BendContext) -> Value {
    let t = n.time as f64;
    let mut d: Vec<(&'static str, Value)> = vec![
        ("t", num(t)),
        ("s", json!(n.string as i64)),
        ("f", json!(n.fret as i64)),
    ];
    if n.sustain > 0.0 {
        d.push(("sus", num(n.sustain as f64)));
    }
    if n.slide_to >= 0 {
        d.push(("sl", json!(n.slide_to as i64)));
    }
    if n.slide_unpitch_to >= 0 {
        d.push(("slu", json!(n.slide_unpitch_to as i64)));
    }
    let cont = ctx.take(n.string as i32);
    let end_bend = add_bend(&mut d, t, n.max_bend as f64, &n.bends, cont);
    let mut flags: Vec<&str> = Vec::new();
    for (bit, key) in NOTE_FLAGS {
        if n.mask & bit != 0 {
            flags.push(key);
        }
    }
    if n.vibrato > 0 {
        flags.push("vb");
    }
    if n.tap > 0 {
        flags.push("tp");
    }
    set_flags(&mut d, &flags);
    if n.pick_direction > 0 {
        // Rocksmith only marks upstrokes; unmarked means unset, not "down"
        d.push(("pkd", json!(1)));
    }
    if (0..=4).contains(&n.left_hand) {
        d.push(("fg", json!(n.left_hand as i64)));
    }
    ctx.set(n.string as i32, n.mask & MASK_PARENT != 0, end_bend);
    ordered(d)
}

/// Boolean flags set to true, each once (a dict assignment in Python: setting it twice keeps one entry).
fn set_flags(d: &mut Vec<(&'static str, Value)>, flags: &[&str]) {
    for f in flags {
        let key: &'static str = FIELD_ORDER.iter().find(|k| *k == f).copied().unwrap_or("ig");
        if !d.iter().any(|(k, _)| *k == key) {
            d.push((key, Value::Bool(true)));
        }
    }
}

fn chord(n: &SngNote, s: &Sng, strings: usize) -> Res<Value> {
    if n.chord_id < 0 || n.chord_id as usize >= s.chord_templates.len() {
        return err(format!(
            "chord at {:.3}s uses a missing chord template ({})",
            n.time, n.chord_id
        ));
    }
    let tpl = &s.chord_templates[n.chord_id as usize];
    let cn = (n.chord_notes_id >= 0)
        .then(|| s.chord_notes.get(n.chord_notes_id as usize))
        .flatten();
    let wide: Vec<&str> = CHORD_WIDE
        .iter()
        .filter(|(bit, _)| n.mask & bit != 0)
        .map(|(_, k)| *k)
        .collect();
    let chord_links = n.mask & MASK_PARENT != 0;
    let any_string_links = cn.is_some_and(|cn| {
        cn.masks
            .iter()
            .enumerate()
            .any(|(st, m)| st < strings && tpl.frets[st] >= 0 && m & MASK_PARENT != 0)
    });
    let mut notes = Vec::new();
    for st in 0..strings.min(6) {
        let fret = tpl.frets[st];
        if fret < 0 {
            continue;
        }
        let mut d: Vec<(&'static str, Value)> = vec![("s", json!(st as i64)), ("f", json!(fret as i64))];
        if n.sustain > 0.0 {
            d.push(("sus", num(n.sustain as f64)));
        }
        if let Some(cn) = cn {
            if cn.slide_to[st] >= 0 {
                d.push(("sl", json!(cn.slide_to[st] as i64)));
            }
            if cn.slide_unpitch_to[st] >= 0 {
                d.push(("slu", json!(cn.slide_unpitch_to[st] as i64)));
            }
            add_bend(&mut d, n.time as f64, 0.0, &cn.bends[st], false);
            let mut flags: Vec<&str> = NOTE_FLAGS
                .iter()
                .filter(|(bit, _)| cn.masks[st] & bit != 0)
                .map(|(_, k)| *k)
                .collect();
            if cn.vibrato[st] > 0 {
                flags.push("vb");
            }
            set_flags(&mut d, &flags);
        }
        set_flags(&mut d, &wide);
        if chord_links && !any_string_links {
            // a linked chord without per-string detail: all its strings link
            set_flags(&mut d, &["ln"]);
        }
        notes.push(ordered(d));
    }
    let mut out = Map::new();
    out.insert("t".into(), num(n.time as f64));
    out.insert("id".into(), json!(n.chord_id as i64));
    if n.mask & MASK_HIGH_DENSITY != 0 {
        out.insert("hd".into(), Value::Bool(true));
    }
    out.insert("notes".into(), Value::Array(notes));
    Ok(Value::Object(out))
}

/// (notes, chords) of these chart events.
fn events(level_notes: &[&SngNote], s: &Sng, strings: usize) -> Res<(Vec<Value>, Vec<Value>)> {
    let mut sorted: Vec<&SngNote> = level_notes.to_vec();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.string.cmp(&b.string)));
    let mut ctx = BendContext::default();
    let (mut notes, mut chords) = (Vec::new(), Vec::new());
    for n in sorted {
        if n.is_chord() {
            chords.push(chord(n, s, strings)?);
            for st in 0..strings as i32 {
                ctx.take(st); // a chord breaks any single-note bend chain on its strings
            }
        } else {
            notes.push(note(n, &mut ctx));
        }
    }
    Ok((notes, chords))
}

fn anchor(a: &sng::SngAnchor) -> Value {
    json!({"time": r3(a.start as f64), "fret": a.fret as i64, "width": a.width as i64})
}

fn handshape(h: &sng::SngShape) -> Value {
    json!({"chord_id": h.chord_id as i64, "start_time": r3(h.start as f64), "end_time": r3(h.end as f64), "arp": h.arpeggio})
}

fn template(t: &SngChordTemplate, strings: usize) -> Value {
    let k = strings.min(6);
    json!({
        "name": t.name,
        "displayName": t.name,
        "arp": t.mask & 1 != 0,
        "fingers": t.fingers[..k].iter().map(|&x| x as i64).collect::<Vec<_>>(),
        "frets": t.frets[..k].iter().map(|&x| x as i64).collect::<Vec<_>>(),
    })
}

// -- the difficulty ladder ------------------------------------------------------------------------------------
/// A level's events with their start times, for cutting windows (bisect_left).
struct LevelIndex<'a> {
    level: &'a SngLevel,
    note_times: Vec<f64>,
    anchor_times: Vec<f64>,
    shape_times: Vec<f64>,
}

fn bisect_left(times: &[f64], x: f64) -> usize {
    times.partition_point(|&t| t < x)
}

impl<'a> LevelIndex<'a> {
    fn of(level: &'a SngLevel) -> LevelIndex<'a> {
        LevelIndex {
            level,
            note_times: level.notes.iter().map(|n| n.time as f64).collect(),
            anchor_times: level.anchors.iter().map(|a| a.start as f64).collect(),
            shape_times: level.handshapes.iter().map(|h| h.start as f64).collect(),
        }
    }

    #[allow(clippy::type_complexity)]
    fn window(&self, lo: f64, hi: f64) -> (&'a [SngNote], &'a [sng::SngAnchor], &'a [sng::SngShape]) {
        let cut = |times: &[f64]| (bisect_left(times, lo), bisect_left(times, hi));
        let (a, b) = cut(&self.note_times);
        let (c, d) = cut(&self.anchor_times);
        let (e, f) = cut(&self.shape_times);
        let lv = self.level;
        (
            &lv.notes[a..b.max(a)],
            &lv.anchors[c..d.max(c)],
            &lv.handshapes[e..f.max(e)],
        )
    }
}

/// (start, end, max difficulty, lo, hi) per phrase iteration; lo / hi open the first and last windows.
fn phrase_windows(s: &Sng, song_end: f64) -> Vec<(f64, f64, i32, f64, f64)> {
    let pis = &s.phrase_iterations;
    let mut out = Vec::new();
    for (i, pi) in pis.iter().enumerate() {
        let max_diff = if pi.phrase_id >= 0 && (pi.phrase_id as usize) < s.phrases.len() {
            s.phrases[pi.phrase_id as usize].max_difficulty
        } else {
            0
        };
        let start = pi.start as f64;
        let next = pis.get(i + 1).map(|p| p.start as f64);
        let end = next.unwrap_or(song_end.max(start));
        let lo = if i == 0 { f64::NEG_INFINITY } else { start };
        let hi = next.unwrap_or(f64::INFINITY);
        out.push((start, end, max_diff, lo, hi));
    }
    out
}

/// notes / chords / anchors / handshapes at full difficulty, the templates, and (with a ladder) phrases.
fn arrangement_body(s: &Sng, strings: usize, song_end: f64) -> Res<Map<String, Value>> {
    let templates: Vec<Value> = s.chord_templates.iter().map(|t| template(t, strings)).collect();
    let mut body = Map::new();
    if s.levels.is_empty() {
        for k in ["notes", "chords", "anchors", "handshapes"] {
            body.insert(k.into(), json!([]));
        }
        body.insert("templates".into(), Value::Array(templates));
        return Ok(body);
    }
    // later levels of the same difficulty win, as in a dict
    let mut by_diff: BTreeMap<i32, LevelIndex> = BTreeMap::new();
    for lv in &s.levels {
        by_diff.insert(lv.difficulty, LevelIndex::of(lv));
    }
    let level = |d: i32| -> &LevelIndex {
        for k in (0..=d).rev() {
            if let Some(l) = by_diff.get(&k) {
                return l;
            }
        }
        by_diff.values().next().unwrap()
    };
    let mut full_notes: Vec<&SngNote> = Vec::new();
    let mut full_anchors: Vec<&sng::SngAnchor> = Vec::new();
    let mut full_shapes: Vec<&sng::SngShape> = Vec::new();
    let mut phrases = Vec::new();
    let mut any_ladder = false;
    if s.phrase_iterations.is_empty() {
        let top = by_diff.values().next_back().unwrap().level;
        full_notes = top.notes.iter().collect();
        full_anchors = top.anchors.iter().collect();
        full_shapes = top.handshapes.iter().collect();
    } else {
        for (start, end, max_diff, lo, hi) in phrase_windows(s, song_end) {
            let (n, a, h) = level(max_diff).window(lo, hi);
            full_notes.extend(n.iter());
            full_anchors.extend(a.iter());
            full_shapes.extend(h.iter());
            let mut levels = Vec::new();
            for d in 0..=max_diff.max(0) {
                if d > max_diff {
                    break;
                }
                let (ln, la, lh) = level(d).window(lo, hi);
                let refs: Vec<&SngNote> = ln.iter().collect();
                let (notes, chords) = events(&refs, s, strings)?;
                levels.push(json!({
                    "difficulty": d as i64,
                    "notes": notes,
                    "chords": chords,
                    "anchors": la.iter().map(anchor).collect::<Vec<_>>(),
                    "handshapes": lh.iter().map(handshape).collect::<Vec<_>>(),
                }));
            }
            if max_diff > 0 {
                any_ladder = true;
            }
            phrases.push(json!({
                "start_time": r3(start),
                "end_time": r3(end),
                "max_difficulty": max_diff as i64,
                "levels": if max_diff < 0 { Vec::new() } else { levels },
            }));
        }
    }
    let (notes, chords) = events(&full_notes, s, strings)?;
    body.insert("notes".into(), Value::Array(notes));
    body.insert("chords".into(), Value::Array(chords));
    body.insert(
        "anchors".into(),
        Value::Array(full_anchors.iter().map(|a| anchor(a)).collect()),
    );
    body.insert(
        "handshapes".into(),
        Value::Array(full_shapes.iter().map(|h| handshape(h)).collect()),
    );
    body.insert("templates".into(), Value::Array(templates));
    // spec 6.7: only a chart with a real ladder carries phrases (consumers gate the difficulty control on it)
    if by_diff.len() > 1 && any_ladder {
        body.insert("phrases".into(), Value::Array(phrases));
    }
    Ok(body)
}

// -- song-level pieces ----------------------------------------------------------------------------------------
fn beats(s: &Sng) -> Vec<Value> {
    // SNG numbers measures from 0; feedpak's downbeats are 1-based
    s.beats
        .iter()
        .map(|b| {
            json!({"time": r3(b.time as f64), "measure": if b.mask & 1 != 0 { b.measure as i64 + 1 } else { -1 }})
        })
        .collect()
}

fn sections(s: &Sng) -> Vec<Value> {
    s.sections
        .iter()
        .map(|x| json!({"name": x.name, "number": x.number as i64, "time": r3(x.start as f64)}))
        .collect()
}

fn attr_str<'a>(attrs: &'a Map<String, Value>, k: &str) -> &'a str {
    attrs.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Python truthiness of a manifest value.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(m)) => !m.is_empty(),
    }
}

fn is_one(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Number(n)) => n.as_f64() == Some(1.0),
        Some(Value::Bool(b)) => *b,
        _ => false,
    }
}

fn tones(attrs: &Map<String, Value>, s: &Sng) -> Map<String, Value> {
    let base = [attr_str(attrs, "Tone_Base"), attr_str(attrs, "Tone_Default")]
        .into_iter()
        .find(|x| !x.is_empty())
        .unwrap_or("");
    let slots: Vec<&str> = ["Tone_A", "Tone_B", "Tone_C", "Tone_D"]
        .iter()
        .map(|k| attr_str(attrs, k))
        .collect();
    let changes: Vec<Value> = s
        .tones
        .iter()
        .filter(|(_, i)| (0..4).contains(i) && !slots[*i as usize].is_empty())
        .map(|(t, i)| json!({"t": r3(*t as f64), "name": slots[*i as usize]}))
        .collect();
    let mut out = Map::new();
    if !base.is_empty() {
        out.insert("base".into(), json!(base));
    }
    if !changes.is_empty() {
        out.insert("changes".into(), Value::Array(changes));
    }
    if let Some(Value::Array(defs)) = attrs.get("Tones")
        && !defs.is_empty()
    {
        out.insert("definitions".into(), Value::Array(defs.clone()));
    }
    out
}

fn lyrics(vocals: &[sng::SngVocal]) -> Value {
    Value::Array(
        vocals
            .iter()
            .map(|v| json!({"t": r3(v.time as f64), "d": r3((v.length as f64).max(0.0)), "w": v.text.replace('\u{feff}', "")}))
            .collect(),
    )
}

/// Per-syllable pitches, only when they look like a melody (many charts give every syllable one placeholder).
fn vocal_pitch(vocals: &[sng::SngVocal]) -> Option<Value> {
    let pitched: Vec<&sng::SngVocal> = vocals.iter().filter(|v| (0..=127).contains(&v.note)).collect();
    if pitched.len() < 8 {
        return None;
    }
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for v in &pitched {
        *counts.entry(v.note).or_default() += 1;
    }
    let most = counts.values().copied().max().unwrap_or(0);
    if counts.len() < 4 || most as f64 / pitched.len() as f64 >= 0.6 {
        return None;
    }
    Some(json!({
        "version": 1,
        "notes": pitched.iter().map(|v| json!({"t": r3(v.time as f64), "d": r3((v.length as f64).max(0.0)), "midi": v.note as i64})).collect::<Vec<_>>(),
    }))
}

/// Display names as Rocksmith shows them: the represented part is "Lead", others "Alt. Lead" or "Bonus Lead"
/// (numbered when there are several).
fn part_names(infos: &[ArrangementInfo], attrs: &[Map<String, Value>]) -> Vec<String> {
    let mut names = vec![String::new(); infos.len()];
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, info) in infos.iter().enumerate() {
        match groups.iter_mut().find(|g| g.0 == info.name) {
            Some(g) => g.1.push(i),
            None => groups.push((info.name.clone(), vec![i])),
        }
    }
    let empty = Map::new();
    let props = |i: usize| -> &Map<String, Value> {
        attrs[i]
            .get("ArrangementProperties")
            .and_then(Value::as_object)
            .unwrap_or(&empty)
    };
    for (base, idx) in groups {
        let main: Vec<usize> = idx.iter().copied().filter(|&i| !infos[i].bonus).collect();
        let bonus: Vec<usize> = idx.iter().copied().filter(|&i| infos[i].bonus).collect();
        let mut rep: Vec<usize> = main
            .iter()
            .copied()
            .filter(|&i| is_one(props(i).get("represent")))
            .take(1)
            .collect();
        if rep.is_empty() {
            rep = main.iter().copied().take(1).collect();
        }
        let alts: Vec<usize> = main.iter().copied().filter(|i| !rep.contains(i)).collect();
        for &i in &rep {
            names[i] = base.clone();
        }
        for (k, &i) in alts.iter().enumerate() {
            names[i] = if alts.len() > 1 {
                format!("Alt. {base} {}", k + 1)
            } else {
                format!("Alt. {base}")
            };
        }
        for (k, &i) in bonus.iter().enumerate() {
            names[i] = if bonus.len() > 1 {
                format!("Bonus {base} {}", k + 1)
            } else {
                format!("Bonus {base}")
            };
        }
    }
    names
}

/// rsplay-style keys ("lead", "lead2", "bass"), numbered in display order.
fn part_ids(infos: &[ArrangementInfo], order: &[usize]) -> HashMap<usize, String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut ids = HashMap::new();
    for &i in order {
        let mut base: String = infos[i]
            .name
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .collect();
        if base.is_empty() {
            base = "part".into();
        }
        let n = seen.entry(base.clone()).or_insert(0);
        *n += 1;
        ids.insert(i, if *n == 1 { base } else { format!("{base}{n}") });
    }
    ids
}

/// The arrangement manifests' attributes, keyed by the chart's SNG stem (lowercase), and by "@" + the
/// manifest's own stem.
fn manifest_attrs(pkg: &mut Package) -> HashMap<String, Map<String, Value>> {
    let mut out: HashMap<String, Map<String, Value>> = HashMap::new();
    let Some(archive) = pkg.psarc() else { return out };
    for entry in archive.find("manifests/*.json") {
        let Ok(bytes) = archive.read_entry(&entry) else {
            continue;
        };
        let Ok(t) = String::from_utf8(bytes) else { continue };
        let Ok(doc) = serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')) else {
            continue;
        };
        let Some(items) = doc.get("Entries").and_then(Value::as_object) else {
            continue;
        };
        for item in items.values() {
            let attrs = item
                .get("Attributes")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let asset = attr_str(&attrs, "SongAsset")
                .rsplit(':')
                .next()
                .unwrap_or("")
                .to_lowercase();
            if !asset.is_empty() {
                out.insert(asset, attrs.clone());
            }
            out.entry(format!("@{}", stem(&entry.name))).or_insert(attrs);
        }
    }
    out
}

fn chart_paths(info: &ArrangementInfo) -> (Option<String>, Option<String>) {
    match &info.chart {
        ChartRef::Psarc { sng, xml } => (sng.clone(), xml.clone()),
        _ => (None, None),
    }
}

fn attrs_for(info: &ArrangementInfo, table: &HashMap<String, Map<String, Value>>) -> Map<String, Value> {
    let (sng, xml) = chart_paths(info);
    let st = stem(&sng.or(xml).unwrap_or_default());
    table
        .get(&st)
        .or_else(|| table.get(&format!("@{st}")))
        .cloned()
        .unwrap_or_default()
}

// -- audio and art ----------------------------------------------------------------------------------------------
/// The song's (or preview's) .wem, found through its Wwise bank (in a multi-song pack no "largest file" fallback:
/// that would be some other song's).
fn read_wem(pkg: &mut Package, preview: bool, multi: bool) -> Option<Vec<u8>> {
    let loc = qrs_formats::song_audio::wem_location_with(pkg, preview, !multi).ok()?;
    qrs_formats::song_audio::read_wem(pkg, &loc).ok().map(|x| x.0)
}

fn ogg(wem: &[u8], what: &str) -> Res<(Vec<u8>, f64)> {
    let info = qrs_formats::wem::parse_wem(wem).map_err(|e| ExportError(format!("{what}: {e}")))?;
    let ogg = qrs_formats::wem::wem_to_ogg(wem).map_err(|e| {
        ExportError(format!(
            "{what} can't be rewrapped losslessly ({e}); re-encoding isn't supported"
        ))
    })?;
    Ok((ogg, info.duration()))
}

fn cover(pkg: &mut Package, info: &ArrangementInfo, multi: bool) -> Option<Vec<u8>> {
    let archive = pkg.psarc()?;
    let mut names = Vec::new();
    if !info.album_art.is_empty() {
        for size in [256, 128, 64] {
            names.extend(archive.find(&format!("gfxassets/album_art/{}_{size}.dds", info.album_art)));
        }
    }
    if names.is_empty() {
        let key = info.song_key.to_lowercase();
        let mut pool = if key.is_empty() {
            Vec::new()
        } else {
            archive.find(&format!("gfxassets/album_art/album_{key}_*.dds"))
        };
        if pool.is_empty() && !multi {
            pool = archive.find("gfxassets/album_art/*.dds");
        }
        let rank = |n: &str| match n.rsplit('_').next().unwrap_or("").trim_end_matches(".dds") {
            "256" => 0,
            "128" => 1,
            "64" => 2,
            _ => 3,
        };
        pool.sort_by_key(|e| rank(&e.name)); // stable, as Python's sorted
        names = pool;
    }
    for e in names {
        let Ok(data) = archive.read_entry(&e) else {
            continue;
        };
        let Some(img) = crate::art::decode_dds(&data) else {
            continue;
        };
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        // small, like Pillow's optimize=True
        let mut png = Vec::new();
        let enc = image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        );
        if image::ImageEncoder::write_image(
            enc,
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .is_ok()
        {
            return Some(png);
        }
    }
    None
}

/// (vocals, jvocals) SNG bytes for this song.
fn vocal_charts(pkg: &mut Package) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let want = pkg.song_key.to_lowercase();
    let Some(archive) = pkg.psarc() else {
        return (None, None);
    };
    let (mut vocals, mut jvocals) = (None, None);
    for e in archive.find("songs/bin/*vocals.sng") {
        let st = stem(&e.name);
        if !want.is_empty() && st != format!("{want}_vocals") && st != format!("{want}_jvocals") {
            continue;
        }
        if st.ends_with("jvocals") {
            if jvocals.is_none() {
                jvocals = archive.read_entry(&e).ok();
            }
        } else if vocals.is_none() {
            vocals = archive.read_entry(&e).ok();
        }
    }
    (vocals, jvocals)
}

// -- the song -----------------------------------------------------------------------------------------------
#[derive(Debug)]
pub struct ExportResult {
    pub title: String,
    pub artist: String,
    pub song_key: String,
    pub parts: Vec<String>,
    /// "lossless" or "missing".
    pub audio: String,
    pub files: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
    pub seconds: f64,
}

fn json_file(v: &Value) -> Vec<u8> {
    (to_json(v, 0, JSON_WIDTH) + "\n").into_bytes()
}

/// Every file of the feedpak for the song in `pkg` (filtered to one song). `converter`: who wrote it, for the
/// manifest ("quickrs-cli 0.1.0").
pub fn build(pkg: &mut Package, source_name: &str, multi: bool, converter: &str) -> Res<ExportResult> {
    let t0 = Instant::now();
    let mut warnings = Vec::new();
    let infos: Vec<ArrangementInfo> = pkg
        .arrangements
        .iter()
        .filter(|a| chart_paths(a).0.is_some())
        .cloned()
        .collect();
    for a in pkg.arrangements.iter().filter(|a| chart_paths(a).0.is_none()) {
        warnings.push(format!("{}: XML-only chart skipped", a.name));
    }
    if infos.is_empty() {
        return err("no SNG charts to convert");
    }
    let table = manifest_attrs(pkg);
    let attrs: Vec<Map<String, Value>> = infos.iter().map(|a| attrs_for(a, &table)).collect();
    let (a0, at0) = (&infos[0], &attrs[0]);
    let platform = pkg.platform();
    let keys = pkg.keys.clone();
    let mut sngs = Vec::with_capacity(infos.len());
    for a in &infos {
        let path = chart_paths(a).0.unwrap_or_default();
        let data = pkg
            .psarc()
            .ok_or_else(|| ExportError("not a psarc".into()))?
            .read(&path)?;
        sngs.push(sng::load_sng(&data, &keys, platform)?);
    }
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let song_length = infos
        .iter()
        .map(|a| a.song_length)
        .chain(sngs.iter().map(|s| s.metadata.song_length as f64))
        .fold(f64::MIN, f64::max);

    // audio first: its length bounds the duration
    let (mut audio_mode, mut audio_len) = ("missing", 0.0);
    let wem = read_wem(pkg, false, multi);
    match &wem {
        None => warnings.push("no song audio found".into()),
        Some(w) => {
            let (o, len) = ogg(w, "song audio")?;
            files.insert("stems/full.ogg".into(), o);
            audio_mode = "lossless";
            audio_len = len;
        }
    }
    if let Some(p) = read_wem(pkg, true, multi)
        && Some(&p) != wem.as_ref()
    {
        match ogg(&p, "preview") {
            Ok((o, _)) => {
                files.insert("preview.ogg".into(), o);
            }
            Err(e) => warnings.push(e.0),
        }
    }
    let duration = song_length.max(audio_len);

    // parts: display order is the package's (lead, rhythm, combo, bass; bonus last)
    let names = part_names(&infos, &attrs);
    let mut order: Vec<usize> = (0..infos.len()).collect();
    order.sort_by_key(|&i| (names[i].starts_with("Bonus"), names[i].starts_with("Alt."), i));
    let mut first_seen: Vec<&str> = Vec::new();
    for a in &infos {
        if !first_seen.contains(&a.name.as_str()) {
            first_seen.push(&a.name);
        }
    }
    order.sort_by_key(|&i| first_seen.iter().position(|n| *n == infos[i].name).unwrap_or(0));
    let ids = part_ids(&infos, &order);
    let primary = &sngs[order[0]];
    let timeline = json!({"version": 1, "beats": beats(primary), "sections": sections(primary)});

    let mut entries = Vec::new();
    for &i in &order {
        let (info, at, s) = (&infos[i], &attrs[i], &sngs[i]);
        let meta = &s.metadata;
        // strings the part plays: its notes, and the shapes its chords use (not every template: bass charts
        // carry the song's guitar chord shapes without playing them)
        let mut used: Vec<usize> = Vec::new();
        let mut played: Vec<i32> = Vec::new();
        for lv in &s.levels {
            for n in &lv.notes {
                if n.is_chord() {
                    if !played.contains(&n.chord_id) {
                        played.push(n.chord_id);
                    }
                } else if n.string >= 0 {
                    used.push(n.string as usize);
                }
            }
        }
        for cid in played {
            if cid >= 0 && (cid as usize) < s.chord_templates.len() {
                for (st, f) in s.chord_templates[cid as usize].frets.iter().enumerate() {
                    if *f >= 0 {
                        used.push(st);
                    }
                }
            }
        }
        let strings = used
            .iter()
            .map(|s| s + 1)
            .fold(if info.is_bass { 4 } else { 6 }, usize::max);
        let mut tuning: Vec<i64> = if !meta.tuning.is_empty() {
            meta.tuning.iter().map(|&x| x as i64).collect()
        } else {
            info.tuning.iter().map(|&x| x as i64).collect()
        };
        tuning.extend([0; 6]);
        tuning.truncate(strings.max(4));
        if !info.is_bass {
            tuning.truncate(strings.max(6));
        }
        let capo = meta.capo as i64;
        let cent = at
            .get("CentOffset")
            .filter(|v| truthy(Some(v)))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|x| x.parse().ok())))
            .unwrap_or(0.0);
        let body = arrangement_body(s, strings, duration)?;
        let mut arr = Map::new();
        arr.insert("name".into(), json!(names[i]));
        arr.insert("tuning".into(), json!(tuning));
        arr.insert("capo".into(), json!(capo));
        arr.insert("centOffset".into(), json!(cent));
        arr.extend(body);
        let tn = tones(at, s);
        if !tn.is_empty() {
            arr.insert("tones".into(), Value::Object(tn));
        }
        let mut own = Map::new();
        let (my_beats, my_sections) = (beats(s), sections(s));
        if Value::Array(my_beats.clone()) != timeline["beats"] {
            own.insert("beats".into(), Value::Array(my_beats));
        }
        if Value::Array(my_sections.clone()) != timeline["sections"] {
            own.insert("sections".into(), Value::Array(my_sections));
        }
        if !own.is_empty() {
            arr.insert("rocksmith".into(), Value::Object(own)); // this part's own beats / sections
        }
        let path = format!("arrangements/{}.json", ids[&i]);
        files.insert(path.clone(), json_file(&Value::Object(arr)));
        let empty = Map::new();
        let props = at
            .get("ArrangementProperties")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let mut entry = Map::new();
        entry.insert("id".into(), json!(ids[&i]));
        entry.insert("name".into(), json!(names[i]));
        entry.insert("file".into(), json!(path));
        entry.insert("type".into(), json!(if info.is_bass { "bass" } else { "guitar" }));
        entry.insert("tuning".into(), json!(tuning));
        entry.insert("capo".into(), json!(capo));
        if cent != 0.0 {
            entry.insert("centOffset".into(), json!(cent));
        }
        let route = if info.is_bass {
            "bass".to_string()
        } else if is_one(props.get("pathLead")) {
            "lead".into()
        } else if is_one(props.get("pathRhythm")) {
            "rhythm".into()
        } else {
            info.name.to_lowercase()
        };
        entry.insert(
            "rocksmith".into(),
            json!({
                "arrangement": info.name,
                "path": route,
                "bonus": info.bonus,
                "represent": is_one(props.get("represent")),
                "chart": chart_paths(info).0.unwrap_or_default(),
            }),
        );
        entries.push(Value::Object(entry));
    }
    files.insert("song_timeline.json".into(), json_file(&timeline));

    // lyrics
    let (vocals_raw, jvocals_raw) = vocal_charts(pkg);
    let load_vocals = |raw: &Option<Vec<u8>>| -> Res<Vec<sng::SngVocal>> {
        Ok(match raw {
            Some(r) => sng::load_sng(r, &keys, platform)?.vocals,
            None => Vec::new(),
        })
    };
    let vocals = load_vocals(&vocals_raw)?;
    let jvocals = load_vocals(&jvocals_raw)?;
    let mut lyric_keys = Map::new();
    if !jvocals.is_empty() {
        // a Japanese song: the original script is the primary track, the romaji a transliteration
        files.insert("lyrics.json".into(), json_file(&lyrics(&jvocals)));
        lyric_keys.insert("lyrics".into(), json!("lyrics.json"));
        lyric_keys.insert("lyrics_source".into(), json!("authored"));
        let mut tracks =
            vec![json!({"id": "ja", "file": "lyrics.json", "language": "ja", "kind": "original"})];
        if !vocals.is_empty() {
            files.insert("lyrics_romaji.json".into(), json_file(&lyrics(&vocals)));
            tracks.push(json!({"id": "romaji", "file": "lyrics_romaji.json", "language": "ja-Latn", "kind": "transliteration", "name": "Romaji"}));
        }
        lyric_keys.insert("lyric_tracks".into(), Value::Array(tracks));
    } else if !vocals.is_empty() {
        files.insert("lyrics.json".into(), json_file(&lyrics(&vocals)));
        lyric_keys.insert("lyrics".into(), json!("lyrics.json"));
        lyric_keys.insert("lyrics_source".into(), json!("authored"));
    }
    let pitch = if vocals.is_empty() {
        None
    } else {
        vocal_pitch(&vocals)
    };
    if let Some(p) = &pitch {
        files.insert("vocal_pitch.json".into(), json_file(p));
    }
    let a0c = a0.clone();
    let cov = cover(pkg, &a0c, multi);
    match &cov {
        Some(c) => {
            files.insert("cover.png".into(), c.clone());
        }
        None => warnings.push("no album art found".into()),
    }

    // the manifest, last, so its index matches what was written
    let mut m = Map::new();
    m.insert("feedpak_version".into(), json!(FEEDPAK_VERSION));
    let title = if a0.title.is_empty() {
        attr_str(at0, "SongName")
    } else {
        &a0.title
    };
    let artist = if a0.artist.is_empty() {
        attr_str(at0, "ArtistName")
    } else {
        &a0.artist
    };
    m.insert("title".into(), json!(title));
    m.insert("artist".into(), json!(artist));
    if !a0.album.is_empty() {
        m.insert("album".into(), json!(a0.album));
    }
    let year = at0
        .get("SongYear")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0);
    if year > 0 {
        m.insert("year".into(), json!(year));
    }
    if !jvocals.is_empty() {
        m.insert("language".into(), json!("ja"));
    }
    m.insert("duration".into(), json!(r3(duration)));
    m.insert("arrangements".into(), Value::Array(entries));
    if files.contains_key("stems/full.ogg") {
        m.insert(
            "stems".into(),
            json!([{"id": "full", "file": "stems/full.ogg", "default": true}]),
        );
    } else {
        // the spec: stems must not be empty, so a song with no audio can't be a conformant feedpak
        return err("no song audio found");
    }
    if files.contains_key("preview.ogg") {
        m.insert("preview".into(), json!("preview.ogg"));
    }
    if cov.is_some() {
        m.insert("cover".into(), json!("cover.png"));
    }
    m.insert("song_timeline".into(), json!("song_timeline.json"));
    m.extend(lyric_keys);
    if pitch.is_some() {
        m.insert("vocal_pitch".into(), json!("vocal_pitch.json"));
    }
    let song_key = if attr_str(at0, "SongKey").is_empty() {
        a0.song_key.clone()
    } else {
        attr_str(at0, "SongKey").to_string()
    };
    let mut rs = Map::new();
    rs.insert("source".into(), json!(source_name));
    rs.insert("song_key".into(), json!(song_key));
    rs.insert(
        "rsplay_id".into(),
        json!(format!(
            "{source_name}|{}",
            if multi { song_key.as_str() } else { "" }
        )),
    );
    for (src, dst) in [
        ("ArtistNameSort", "artist_sort"),
        ("SongNameSort", "title_sort"),
        ("AlbumNameSort", "album_sort"),
        ("JapaneseArtistName", "artist_ja"),
        ("JapaneseSongName", "title_ja"),
    ] {
        if truthy(at0.get(src)) {
            rs.insert(dst.into(), at0[src].clone());
        }
    }
    if truthy(at0.get("SongAverageTempo"))
        && let Some(t) = at0["SongAverageTempo"]
            .as_f64()
            .or_else(|| at0["SongAverageTempo"].as_str().and_then(|x| x.parse().ok()))
    {
        rs.insert("average_tempo".into(), json!(r3(t)));
    }
    rs.insert("converter".into(), json!(converter));
    m.insert("rocksmith".into(), Value::Object(rs));
    let header = format!(
        "{artist} - {title}\nConverted from Rocksmith 2014 ({source_name}) by {converter}.\n\
         The `rocksmith` blocks are an rsplay extension; other readers ignore them."
    );
    files.insert(
        "manifest.yaml".into(),
        manifest_yaml(&m, &header).map_err(ExportError)?.into_bytes(),
    );
    Ok(ExportResult {
        title: title.to_string(),
        artist: artist.to_string(),
        song_key,
        parts: order.iter().map(|i| ids[i].clone()).collect(),
        audio: audio_mode.into(),
        files,
        warnings,
        seconds: t0.elapsed().as_secs_f64(),
    })
}

type Stamp = (u16, u8, u8, u8, u8, u8);

fn local_stamp(t: chrono::DateTime<chrono::Local>) -> Stamp {
    use chrono::{Datelike, Timelike};
    (
        t.year().clamp(0, 2107) as u16,
        t.month() as u8,
        t.day() as u8,
        t.hour() as u8,
        t.minute() as u8,
        t.second() as u8,
    )
}

const STORED: [&str; 7] = [".ogg", ".png", ".jpg", ".wav", ".mp3", ".flac", ".opus"];

/// Write the pack as a .feedpak zip (manifest first; audio and images stored, text deflated), atomically.
pub fn write_zip(files: &BTreeMap<String, Vec<u8>>, out: &Path, stamp: Option<Stamp>) -> Res<()> {
    use zip::write::SimpleFileOptions;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| ExportError(e.to_string()))?;
    }
    let tmp = out.with_file_name(format!(
        "{}.part",
        out.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    let f = std::fs::File::create(&tmp).map_err(|e| ExportError(e.to_string()))?;
    let mut zw = zip::ZipWriter::new(f);
    // as Python: none is now, and before 1980 (zip's epoch) is 1980-01-01
    let (y, mo, d, h, mi, s) = stamp
        .unwrap_or_else(|| local_stamp(chrono::Local::now()))
        .max((1980, 1, 1, 0, 0, 0));
    let dt = zip::DateTime::from_date_and_time(y, mo, d, h, mi, s).unwrap_or_default();
    let mut order: Vec<&String> = vec![];
    if let Some((k, _)) = files.get_key_value("manifest.yaml") {
        order.push(k);
    }
    order.extend(files.keys().filter(|k| *k != "manifest.yaml"));
    for name in order {
        let stored = STORED.iter().any(|ext| name.ends_with(ext));
        let opts = SimpleFileOptions::default()
            .compression_method(if stored {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            })
            .compression_level(if stored { None } else { Some(6) })
            .last_modified_time(dt)
            .external_attributes(0o644 << 16); // as Python's (unix_permissions would add the file-type bits)
        zw.start_file(name.as_str(), opts)
            .map_err(|e| ExportError(e.to_string()))?;
        zw.write_all(&files[name])
            .map_err(|e| ExportError(e.to_string()))?;
    }
    zw.finish().map_err(|e| ExportError(e.to_string()))?;
    std::fs::rename(&tmp, out).map_err(|e| ExportError(e.to_string()))
}

/// Write the pack in its directory form (song.feedpak/), for hand editing.
pub fn write_dir(files: &BTreeMap<String, Vec<u8>>, out: &Path) -> Res<()> {
    for (name, data) in files {
        let target = out.join(name);
        if let Some(d) = target.parent() {
            std::fs::create_dir_all(d).map_err(|e| ExportError(e.to_string()))?;
        }
        std::fs::write(&target, data).map_err(|e| ExportError(e.to_string()))?;
    }
    Ok(())
}

/// 'Artist - Title' made safe for Windows, macOS and Linux file names (rsfeedpak/library.py `file_name`).
pub fn file_name(artist: &str, title: &str) -> String {
    let a = if artist.trim().is_empty() {
        "Unknown Artist"
    } else {
        artist.trim()
    };
    let t = if title.trim().is_empty() {
        "Untitled"
    } else {
        title.trim()
    };
    let mut name: String = format!("{a} - {t}").replace(['/', '\\', '|'], "-");
    // \s*:\s* -> " - "
    while let Some(i) = name.find(':') {
        let (l, r) = name.split_at(i);
        name = format!("{} - {}", l.trim_end(), r[1..].trim_start());
    }
    name = name.replace('"', "'");
    name.retain(|c| !matches!(c, '<' | '>' | '?' | '*') && !((c as u32) < 0x20 || c as u32 == 0x7f));
    name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.trim_matches(|c| c == ' ' || c == '.').to_string();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let first = name.split('.').next().unwrap_or("").to_uppercase();
    let name = if RESERVED.contains(&first.as_str()) {
        format!("_{name}")
    } else {
        name
    };
    let cut: String = name.chars().take(150).collect();
    let cut = cut.trim_end_matches([' ', '.']).to_string();
    if cut.is_empty() { "Untitled".into() } else { cut }
}

/// Convert one song (the only one, or `song_key`'s in a multi-song pack) and write it to `out`.
pub fn export(
    psarc: &Path,
    keys: &Keys,
    out: &Path,
    song_key: Option<&str>,
    as_dir: bool,
    converter: &str,
) -> Res<ExportResult> {
    let whole = Package::open(psarc, keys, None)?;
    let multi = whole.song_keys().len() > 1;
    drop(whole);
    if multi && song_key.is_none() {
        return err(format!(
            "{} holds several songs; pick one with --song-key",
            psarc.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
        ));
    }
    let mut pkg = Package::open(psarc, keys, if multi { song_key } else { None })?;
    let source = psarc
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let result = build(&mut pkg, &source, multi, converter)?;
    if as_dir {
        write_dir(&result.files, out)?;
    } else {
        // the psarc's modification time (local)
        let stamp = std::fs::metadata(psarc)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| local_stamp(t.into()));
        write_zip(&result.files, out, stamp)?;
    }
    Ok(result)
}

/// The default output: the current folder, "Artist - Title.feedpak".
pub fn default_out(psarc: &Path, keys: &Keys, song_key: Option<&str>) -> PathBuf {
    let stem = psarc
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match Package::open(psarc, keys, song_key) {
        Ok(p) if !p.arrangements.is_empty() => file_name(&p.arrangements[0].artist, &p.arrangements[0].title),
        _ => file_name("", &stem),
    };
    PathBuf::from(format!("{name}.feedpak"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_like_python() {
        assert_eq!(r3(2.6755), 2.675); // 2.6755 is 2.67549999... in binary
        assert_eq!(r3(0.0004), 0.0);
        assert_eq!(r3(-0.0001), 0.0); // never -0.0
        assert_eq!(r3(12.3456), 12.346);
    }

    #[test]
    fn bend_intents() {
        assert_eq!(bend_intent(&[(0.0, 0.0), (0.2, 1.0)], false), 0);
        assert_eq!(bend_intent(&[(0.0, 0.0), (0.2, 1.0), (0.4, 0.0)], false), 4);
        assert_eq!(bend_intent(&[(0.0, 1.0)], false), 2);
        assert_eq!(bend_intent(&[(0.0, 1.0), (0.3, 0.0)], false), 3);
        assert_eq!(bend_intent(&[(0.0, 1.0), (0.3, 0.0)], true), 1);
        let mut d = Vec::new();
        let end = add_bend(&mut d, 1.0, 0.0, &[(1.2, 1.0), (1.4, 0.5)], false);
        assert_eq!(end, 0.5);
        assert_eq!(d[0].0, "bn");
        assert_eq!(
            d[1].1,
            json!([{"t": 0.0, "v": 0.0}, {"t": 0.2, "v": 1.0}, {"t": 0.4, "v": 0.5}])
        );
    }

    #[test]
    fn names_and_ids() {
        let info = |name: &str, bonus: bool| ArrangementInfo {
            name: name.into(),
            bonus,
            ..Default::default()
        };
        let infos = vec![
            info("Lead", false),
            info("Lead", false),
            info("Rhythm", false),
            info("Lead", true),
        ];
        let rep = |r: i64| {
            json!({"ArrangementProperties": {"represent": r}})
                .as_object()
                .unwrap()
                .clone()
        };
        let attrs = vec![rep(0), rep(1), rep(1), rep(0)];
        let names = part_names(&infos, &attrs);
        assert_eq!(names, vec!["Alt. Lead", "Lead", "Rhythm", "Bonus Lead"]);
        let ids = part_ids(&infos, &[1, 0, 3, 2]);
        assert_eq!(
            (ids[&1].as_str(), ids[&0].as_str(), ids[&3].as_str()),
            ("lead", "lead2", "lead3")
        );
        assert_eq!(file_name("AC/DC", "Back: In Black?"), "AC-DC - Back - In Black");
        assert_eq!(file_name("", "con"), "Unknown Artist - con");
        assert_eq!(file_name("CON", "x"), "CON - x"); // the part before the first "." is "CON - x"
        assert_eq!(file_name("Con.", ""), "_Con. - Untitled"); // ... here it's "Con", reserved on Windows
    }
    #[test]
    fn zip_layout_like_export_py() {
        let dir = std::env::temp_dir().join(format!("qrs-feedpak-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut files = BTreeMap::new();
        files.insert("stems/full.ogg".to_string(), b"OggS....".to_vec());
        files.insert(
            "arrangements/lead.json".to_string(),
            b"{\"notes\": []}\n".to_vec(),
        );
        files.insert("manifest.yaml".to_string(), b"title: \"x\"\n".to_vec());
        files.insert("cover.png".to_string(), vec![0x89, b'P', b'N', b'G']);
        let out = dir.join("x.feedpak");
        write_zip(&files, &out, Some((1979, 6, 1, 12, 0, 0))).unwrap();
        assert!(!dir.join("x.feedpak.part").exists());
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let names: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "manifest.yaml",
                "arrangements/lead.json",
                "cover.png",
                "stems/full.ogg"
            ]
        );
        for (i, stored) in [(0, false), (1, false), (2, true), (3, true)] {
            let f = z.by_index(i).unwrap();
            let want = if stored {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            assert_eq!(f.compression(), want, "{:?}", f.name());
            let t = f.last_modified().unwrap();
            assert_eq!((t.year(), t.month(), t.day(), t.hour()), (1980, 1, 1, 0)); // before 1980: 1980-01-01
        }
        // the raw attributes, exactly as Python writes them (zip's reader shows DOS-made entries as 0o664)
        let raw = std::fs::read(&out).unwrap();
        let attrs: Vec<u32> = raw
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == b"PK\x01\x02")
            .map(|(i, _)| u32::from_le_bytes(raw[i + 38..i + 42].try_into().unwrap()))
            .collect();
        assert_eq!(attrs.len(), 4);
        assert!(attrs.iter().all(|a| *a == 0o644 << 16), "{attrs:?}");
        let mut text = String::new();
        std::io::Read::read_to_string(&mut z.by_name("arrangements/lead.json").unwrap(), &mut text).unwrap();
        assert_eq!(text, "{\"notes\": []}\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
