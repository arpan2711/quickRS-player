//! Compare two charts of the same part the way the player uses them. Port of rs-open-format/rsfeedpak/verify.py
//! `compare_arrangements`: times to the millisecond (charts are authored to it; feedpak rounds to it), the
//! "sustain" tag ignored (it only restates sustain > 0), chord-wide techniques compared on the footing that a
//! technique on every note of a chord counts as the chord's (feedpak has no chord-level flags).
//!
//! Used to check a converted feedpak against its psarc source, and by the library to confirm duplicates.

use crate::model::{Arrangement, Chord, Note, Techs};

pub const TIME_TOL: f64 = 0.0015;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn pad(v: &[i32], n: usize, fill: i32) -> Vec<i32> {
    let mut out: Vec<i32> = v.to_vec();
    out.resize(n.max(out.len()), fill);
    out.truncate(n);
    out
}

const CHORD_WIDE: Techs =
    Techs(Techs::PALM_MUTE.0 | Techs::FRET_HAND_MUTE.0 | Techs::ACCENT.0 | Techs::IGNORE.0);

fn chord_wide(c: &Chord) -> Techs {
    let common = if c.notes.is_empty() {
        Techs::NONE
    } else {
        c.notes.iter().fold(Techs(u32::MAX), |acc, n| acc & n.techniques) & CHORD_WIDE
    };
    (c.techniques | common) - Techs::LINK_NEXT - Techs::SUSTAIN
}

fn note_diff(a: &Note, b: &Note, extra_a: Techs, extra_b: Techs, at: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !(close(a.time, b.time, TIME_TOL) && a.string == b.string && a.fret == b.fret) {
        return vec![format!(
            "{at}: note {:.3}s s{} f{} vs {:.3}s s{} f{}",
            a.time, a.string, a.fret, b.time, b.string, b.fret
        )];
    }
    if !close(a.sustain, b.sustain, TIME_TOL) {
        out.push(format!("{at}: sustain {:.3} vs {:.3}", a.sustain, b.sustain));
    }
    let (ta, tb) = (
        (a.techniques | extra_a) - Techs::SUSTAIN,
        (b.techniques | extra_b) - Techs::SUSTAIN,
    );
    if ta != tb {
        out.push(format!("{at}: techniques {:?} vs {:?}", ta, tb));
    }
    if (a.slide_to, a.slide_unpitch_to) != (b.slide_to, b.slide_unpitch_to) {
        out.push(format!(
            "{at}: slides {:?} vs {:?}",
            (a.slide_to, a.slide_unpitch_to),
            (b.slide_to, b.slide_unpitch_to)
        ));
    }
    if !close(a.bend, b.bend, 0.001) {
        out.push(format!("{at}: bend {} vs {}", a.bend, b.bend));
    }
    out
}

fn r3(x: f64) -> i64 {
    (x * 1000.0).round() as i64
}

/// Differences between `a` and `b` (empty = the same for the player).
pub fn compare_arrangements(a: &Arrangement, b: &Arrangement, label: &str) -> Vec<String> {
    let mut out = Vec::new();
    let n = (if a.is_bass { 4 } else { 6 }).max(b.tuning.len());
    if pad(&a.tuning, n, 0) != pad(&b.tuning, n, 0) || a.capo != b.capo || a.is_bass != b.is_bass {
        out.push(format!(
            "{label}: tuning/capo/bass {:?} {} {} vs {:?} {} {}",
            pad(&a.tuning, n, 0),
            a.capo,
            a.is_bass,
            b.tuning,
            b.capo,
            b.is_bass
        ));
    }
    if a.notes.len() != b.notes.len() {
        out.push(format!("{label}: {} notes vs {}", a.notes.len(), b.notes.len()));
    }
    for (x, y) in a.notes.iter().zip(&b.notes) {
        out.extend(note_diff(
            x,
            y,
            Techs::NONE,
            Techs::NONE,
            &format!("{label} note@{:.3}", x.time),
        ));
    }
    if a.chords.len() != b.chords.len() {
        out.push(format!(
            "{label}: {} chords vs {}",
            a.chords.len(),
            b.chords.len()
        ));
    }
    for (x, y) in a.chords.iter().zip(&b.chords) {
        let at = format!("{label} chord@{:.3}", x.time);
        if !close(x.time, y.time, TIME_TOL) || x.chord_id != y.chord_id || x.name != y.name {
            out.push(format!(
                "{at}: {} {:?} vs {:.3} {} {:?}",
                x.chord_id, x.name, y.time, y.chord_id, y.name
            ));
            continue;
        }
        if pad(&x.frets, 6, -1) != pad(&y.frets, 6, -1) || pad(&x.fingers, 6, -1) != pad(&y.fingers, 6, -1) {
            out.push(format!(
                "{at}: shape {:?}/{:?} vs {:?}/{:?}",
                x.frets, x.fingers, y.frets, y.fingers
            ));
        }
        if chord_wide(x) != chord_wide(y) {
            out.push(format!(
                "{at}: chord techniques {:?} vs {:?}",
                chord_wide(x),
                chord_wide(y)
            ));
        }
        if !close(x.sustain, y.sustain, TIME_TOL) {
            out.push(format!("{at}: sustain {:.3} vs {:.3}", x.sustain, y.sustain));
        }
        if x.notes.len() != y.notes.len() {
            out.push(format!("{at}: {} notes vs {}", x.notes.len(), y.notes.len()));
        }
        let mut xs: Vec<&Note> = x.notes.iter().collect();
        let mut ys: Vec<&Note> = y.notes.iter().collect();
        xs.sort_by_key(|n| n.string);
        ys.sort_by_key(|n| n.string);
        // a chord note is drawn and graded with its own techniques plus the chord's
        for (p, q) in xs.iter().zip(&ys) {
            out.extend(note_diff(p, q, x.techniques, y.techniques, &at));
        }
    }
    if a.anchors.len() != b.anchors.len() {
        out.push(format!(
            "{label}: {} anchors vs {}",
            a.anchors.len(),
            b.anchors.len()
        ));
    }
    for (x, y) in a.anchors.iter().zip(&b.anchors) {
        if !close(x.start, y.start, TIME_TOL) || x.fret != y.fret || x.width != y.width {
            out.push(format!(
                "{label} anchor@{:.3}: {}/{} vs {:.3} {}/{}",
                x.start, x.fret, x.width, y.start, y.fret, y.width
            ));
        }
    }
    if a.handshapes.len() != b.handshapes.len() {
        out.push(format!(
            "{label}: {} hand shapes vs {}",
            a.handshapes.len(),
            b.handshapes.len()
        ));
    }
    for (x, y) in a.handshapes.iter().zip(&b.handshapes) {
        if !(close(x.start, y.start, TIME_TOL)
            && close(x.end, y.end, TIME_TOL)
            && x.name == y.name
            && x.arpeggio == y.arpeggio
            && pad(&x.frets, 6, -1) == pad(&y.frets, 6, -1))
        {
            out.push(format!(
                "{label} hand shape@{:.3}: {:?}..{:.3} vs {:.3} {:?}..{:.3}",
                x.start, x.name, x.end, y.start, y.name, y.end
            ));
        }
    }
    let ba: Vec<(i64, i32)> = a.beats.iter().map(|x| (r3(x.time), x.measure)).collect();
    let bb: Vec<(i64, i32)> = b.beats.iter().map(|x| (r3(x.time), x.measure)).collect();
    if ba != bb {
        out.push(format!(
            "{label}: beats differ ({} vs {})",
            a.beats.len(),
            b.beats.len()
        ));
    }
    let sa: Vec<(&str, i32, i64)> = a
        .sections
        .iter()
        .map(|s| (s.name.as_str(), s.number, r3(s.start)))
        .collect();
    let sb: Vec<(&str, i32, i64)> = b
        .sections
        .iter()
        .map(|s| (s.name.as_str(), s.number, r3(s.start)))
        .collect();
    if sa != sb {
        out.push(format!("{label}: sections differ ({} vs {})", sa.len(), sb.len()));
    }
    out
}
