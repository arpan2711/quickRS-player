//! Rocksmith 2014 XML arrangement parser (the unencrypted format used in custom-song projects). Port of
//! rsplay/xml_arr.py.
//!
//! `<song>` holds `<title>`, `<arrangement>`, `<tuning string0..5>`, `<capo>`, `<ebeats>`, `<phrases>`,
//! `<phraseIterations>`, `<chordTemplates>`, `<sections>` and `<levels><level difficulty=..>` with
//! `<notes>`/`<chords>`. String 0 is the low E.

use std::collections::BTreeMap;

use roxmltree::{Document, Node};

use crate::error::{Error, Result};
use crate::model::{Arrangement, Beat, Chord, Note, Section, Techs};
use crate::sng::py_round;

/// Python's float(): surrounding whitespace allowed.
fn parse_f(v: &str) -> Option<f64> {
    let t = v.trim();
    if t.is_empty() {
        None
    } else {
        t.replace('_', "").parse::<f64>().ok().filter(|x| !x.is_nan())
    }
}

fn f(el: Node, name: &str, default: f64) -> f64 {
    el.attribute(name).and_then(parse_f).unwrap_or(default)
}

fn i(el: Node, name: &str, default: i32) -> i32 {
    el.attribute(name)
        .and_then(parse_f)
        .filter(|x| x.is_finite())
        .map(|x| x.trunc() as i32)
        .unwrap_or(default)
}

fn child<'a, 'i>(el: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
    el.children().find(|c| c.is_element() && c.has_tag_name(tag))
}

fn children<'a, 'i>(el: Node<'a, 'i>, tag: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    el.children()
        .filter(move |c| c.is_element() && c.has_tag_name(tag))
}

/// `root.iterfind("a/b")`: every `b` child of every `a` child.
fn path<'a, 'i>(el: Node<'a, 'i>, a: &'a str, b: &'a str) -> Vec<Node<'a, 'i>> {
    children(el, a)
        .flat_map(|x| children(x, b).collect::<Vec<_>>())
        .collect()
}

fn text(root: Node, tag: &str, default: &str) -> String {
    match child(root, tag).and_then(|e| e.text()) {
        Some(t) if !t.is_empty() => t.trim().to_string(),
        _ => default.to_string(),
    }
}

const NOTE_FLAGS: [(&str, Techs); 14] = [
    ("hammerOn", Techs::HAMMER_ON),
    ("pullOff", Techs::PULL_OFF),
    ("palmMute", Techs::PALM_MUTE),
    ("mute", Techs::FRET_HAND_MUTE),
    ("harmonic", Techs::HARMONIC),
    ("harmonicPinch", Techs::PINCH_HARMONIC),
    ("tremolo", Techs::TREMOLO),
    ("accent", Techs::ACCENT),
    ("linkNext", Techs::LINK_NEXT),
    ("ignore", Techs::IGNORE),
    ("slap", Techs::SLAP),
    ("pluck", Techs::POP),
    ("tap", Techs::TAP),
    ("vibrato", Techs::VIBRATO),
];

fn note_techniques(el: Node) -> Techs {
    let mut t = NOTE_FLAGS
        .iter()
        .filter(|(a, _)| i(el, a, 0) > 0)
        .fold(Techs::NONE, |acc, (_, x)| acc | *x);
    t = t
        .with(Techs::BEND, f(el, "bend", 0.0) > 0.0)
        .with(Techs::SLIDE, i(el, "slideTo", -1) >= 0)
        .with(Techs::UNPITCHED_SLIDE, i(el, "slideUnpitchTo", -1) >= 0)
        .with(Techs::SUSTAIN, f(el, "sustain", 0.0) > 0.0);
    t
}

fn parse_note(el: Node, extra: Techs) -> Note {
    let mut max_bend = f(el, "bend", 0.0);
    let mut curve = Vec::new();
    if let Some(bv) = child(el, "bendValues") {
        for b in children(bv, "bendValue") {
            let step = f(b, "step", 0.0);
            max_bend = max_bend.max(step);
            curve.push((f(b, "time", 0.0), step));
        }
    }
    Note {
        time: f(el, "time", 0.0),
        string: i(el, "string", 0),
        fret: i(el, "fret", 0),
        sustain: f(el, "sustain", 0.0),
        techniques: note_techniques(el) | extra,
        slide_to: i(el, "slideTo", -1),
        slide_unpitch_to: i(el, "slideUnpitchTo", -1),
        bend: max_bend,
        bend_curve: curve,
    }
}

/// Parse an XML arrangement at a mastery (1.0 = full difficulty).
pub fn parse_xml(source: &[u8], mastery: f64) -> Result<Arrangement> {
    let src = std::str::from_utf8(source)
        .map_err(|e| Error::Arrangement(format!("invalid XML: {e}")))?
        .trim_start_matches('\u{feff}');
    let doc = Document::parse(src).map_err(|e| Error::Arrangement(format!("invalid XML: {e}")))?;
    let root = doc.root_element();
    let tag = root.tag_name().name();
    if tag == "vocals" {
        return Err(Error::Arrangement(
            "this is a vocals (lyrics) arrangement, not a note chart".into(),
        ));
    }
    if tag != "song" {
        return Err(Error::Arrangement(format!(
            "not a Rocksmith arrangement (root element <{tag}>)"
        )));
    }

    let tuning_el = child(root, "tuning");
    let tuning: Vec<i32> = (0..6)
        .map(|k| tuning_el.map(|t| i(t, &format!("string{k}"), 0)).unwrap_or(0))
        .collect();
    let props = child(root, "arrangementProperties");
    let name = text(root, "arrangement", "Lead");
    let is_bass = name.to_lowercase() == "bass" || props.map(|p| i(p, "pathBass", 0) == 1).unwrap_or(false);
    let capo = parse_f(&text(root, "capo", "0"))
        .map(|x| x.trunc() as i32)
        .unwrap_or(0);
    let song_length = parse_f(&text(root, "songLength", "0")).unwrap_or(0.0);

    let mut arr = Arrangement::new(&name);
    arr.title = text(root, "title", "");
    arr.artist = text(root, "artistName", "");
    arr.album = text(root, "albumName", "");
    arr.tuning = tuning;
    arr.capo = capo.max(0);
    arr.song_length = song_length;
    arr.is_bass = is_bass;
    arr.source = "xml".into();
    arr.mastery = mastery;
    if let Some(c) = child(root, "centOffset").and_then(|e| e.text()).and_then(parse_f) {
        arr.cent_offset = c;
    }

    arr.beats = path(root, "ebeats", "ebeat")
        .into_iter()
        .map(|b| Beat {
            time: f(b, "time", 0.0),
            measure: i(b, "measure", -1),
        })
        .collect();
    let section_els = path(root, "sections", "section");
    for (k, s) in section_els.iter().enumerate() {
        let start = f(*s, "startTime", 0.0);
        let end = section_els
            .get(k + 1)
            .map(|n| f(*n, "startTime", 0.0))
            .unwrap_or(song_length);
        arr.sections.push(Section {
            name: s.attribute("name").unwrap_or("").to_string(),
            number: i(*s, "number", 1),
            start,
            end,
        });
    }

    struct Tpl {
        name: String,
        frets: Vec<i32>,
        fingers: Vec<i32>,
    }
    let templates: Vec<Tpl> = path(root, "chordTemplates", "chordTemplate")
        .into_iter()
        .map(|ct| Tpl {
            name: ct
                .attribute("chordName")
                .filter(|s| !s.is_empty())
                .or_else(|| ct.attribute("displayName"))
                .unwrap_or("")
                .to_string(),
            frets: (0..6).map(|s| i(ct, &format!("fret{s}"), -1)).collect(),
            fingers: (0..6).map(|s| i(ct, &format!("finger{s}"), -1)).collect(),
        })
        .collect();

    let phrases: Vec<i32> = path(root, "phrases", "phrase")
        .into_iter()
        .map(|p| i(p, "maxDifficulty", 0))
        .collect();
    let iterations: Vec<(f64, i32)> = path(root, "phraseIterations", "phraseIteration")
        .into_iter()
        .map(|pi| (f(pi, "time", 0.0), i(pi, "phraseId", 0)))
        .collect();

    let mut levels: BTreeMap<i32, Vec<Node>> = BTreeMap::new();
    for level in path(root, "levels", "level") {
        let mut events: Vec<Node> = path(level, "notes", "note");
        events.extend(path(level, "chords", "chord"));
        events.sort_by(|a, b| f(*a, "time", 0.0).total_cmp(&f(*b, "time", 0.0)));
        levels.insert(i(level, "difficulty", 0), events);
    }
    arr.has_levels = levels.len() > 1;
    if levels.is_empty() {
        return Ok(arr);
    }
    let level_events = |max_diff: i32| -> &Vec<Node> {
        for d in (0..=max_diff).rev() {
            if let Some(ev) = levels.get(&d) {
                return ev;
            }
        }
        levels.values().next().unwrap()
    };

    let m = mastery.clamp(0.0, 1.0);
    let mut selected: Vec<Node> = Vec::new();
    if iterations.is_empty() {
        selected = levels.values().next_back().unwrap().clone();
    } else {
        for (k, &(start, phrase_id)) in iterations.iter().enumerate() {
            let lo = if k == 0 { f64::NEG_INFINITY } else { start };
            let hi = iterations.get(k + 1).map(|x| x.0).unwrap_or(f64::INFINITY);
            let max_diff = usize::try_from(phrase_id)
                .ok()
                .and_then(|p| phrases.get(p))
                .copied()
                .unwrap_or(0);
            let events = level_events(py_round(m * max_diff as f64));
            let a = events.partition_point(|e| f(*e, "time", 0.0) < lo);
            let b = events.partition_point(|e| f(*e, "time", 0.0) < hi);
            selected.extend(events[a..b.max(a)].iter().copied());
        }
    }

    for el in selected {
        if el.has_tag_name("note") {
            arr.notes.push(parse_note(el, Techs::NONE));
            continue;
        }
        let chord_id = i(el, "chordId", 0);
        let tpl = usize::try_from(chord_id).ok().and_then(|k| templates.get(k));
        let mut chord_techs = Techs::NONE;
        for (attr, t) in [
            ("palmMute", Techs::PALM_MUTE),
            ("fretHandMute", Techs::FRET_HAND_MUTE),
            ("accent", Techs::ACCENT),
            ("linkNext", Techs::LINK_NEXT),
            ("ignore", Techs::IGNORE),
        ] {
            if i(el, attr, 0) > 0 {
                chord_techs |= t;
            }
        }
        let shared = chord_techs & (Techs::PALM_MUTE | Techs::FRET_HAND_MUTE);
        let time = f(el, "time", 0.0);
        let mut notes: Vec<Note> = children(el, "chordNote")
            .map(|cn| parse_note(cn, shared))
            .collect();
        if notes.is_empty() {
            let frets = tpl.map(|t| t.frets.clone()).unwrap_or_else(|| vec![-1; 6]);
            notes = frets
                .iter()
                .enumerate()
                .filter(|(_, fr)| **fr >= 0)
                .map(|(s, fr)| Note {
                    techniques: shared,
                    ..Note::new(time, s as i32, *fr)
                })
                .collect();
        }
        let sustain = notes.iter().map(|n| n.sustain).fold(0.0, f64::max);
        arr.chords.push(Chord {
            time,
            chord_id,
            name: tpl.map(|t| t.name.clone()).unwrap_or_default(),
            notes,
            techniques: chord_techs,
            sustain,
            frets: tpl.map(|t| t.frets.clone()).unwrap_or_default(),
            fingers: tpl.map(|t| t.fingers.clone()).unwrap_or_default(),
        });
    }
    arr.notes
        .sort_by(|a, b| a.time.total_cmp(&b.time).then(a.string.cmp(&b.string)));
    Ok(arr)
}
