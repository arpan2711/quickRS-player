//! Standard notation on the tab page (P44.1, D181; P49 brought it to a published book's standard after three expert
//! reviews; new in quickRS, no Python source): a five-line staff above each row of tab (Both) or in its place
//! (Staff), for sight reading.
//!
//! Everything comes from the chart. A note's pitch is its string's open note (the tuning) plus its fret, written an
//! octave up as guitar and bass music is (a treble clef with the small 8 for guitar, a plain bass clef for bass). A
//! tuning that moves every string alike (Eb or D standard) is written as fingered, as books do, with "Tune down ½
//! step" over the first row; strings tuned apart from the rest are named ("6 = D"). The key signature is the song's
//! key (from all its parts) as written; notes outside it are spelled by their degree (b3, b7 as flats). The rhythm is
//! `Rhythm::staff`: the tab's snapped notes written by the book (a shuffle as eighths under a Shuffle mark, beat 3
//! shown, whole-bar rests). Technique marks ride on the staff too: slurs with H / P, slide lines, bend arrows, P.M.,
//! harmonics' diamonds, accents, vibrato, let ring, 8va. Symbols are Bravura's (SMuFL): its em is four staff spaces, a
//! glyph's baseline is the staff position it sits on.

use std::collections::HashMap;

use eframe::egui;
use egui::{Align2, Color32, FontId, Painter, Shape, Stroke, pos2};
use qrs_analysis::keyguess::{Key, Mode};
use qrs_formats::Techs;
use qrs_formats::rhythm::Piece;
use qrs_ui::text;
use qrs_ui::theme::{judge_color, t};

use super::{Chart, TabCtx, TabView, ipx, lower_bound, pt};

/// What the tab page shows (config `tab_score`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Score {
    #[default]
    Tab,
    Staff,
    Both,
}

impl Score {
    pub fn key(self) -> &'static str {
        match self {
            Score::Tab => "tab",
            Score::Staff => "staff",
            Score::Both => "both",
        }
    }

    pub fn parse(s: &str) -> Score {
        match s {
            "staff" => Score::Staff,
            "both" => Score::Both,
            _ => Score::Tab,
        }
    }

    /// The toolbar key's order: Tab, then Both (the tab with its notes above), then Staff alone.
    pub fn next(self) -> Score {
        match self {
            Score::Tab => Score::Both,
            Score::Both => Score::Staff,
            Score::Staff => Score::Tab,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Score::Tab => "Tab",
            Score::Staff => "Standard notation",
            Score::Both => "Notation and tab",
        }
    }

    pub fn staff(self) -> bool {
        self != Score::Tab
    }

    pub fn tab(self) -> bool {
        self != Score::Staff
    }
}

// SMuFL code points
const G_CLEF_8VB: char = '\u{E052}';
const F_CLEF: char = '\u{E062}';
const HEAD_WHOLE: char = '\u{E0A2}';
const HEAD_HALF: char = '\u{E0A3}';
const HEAD_BLACK: char = '\u{E0A4}';
const HEAD_X: char = '\u{E0A9}';
const HEAD_DIAMOND_WHOLE: char = '\u{E0D8}';
const HEAD_DIAMOND_HALF: char = '\u{E0D9}';
const HEAD_DIAMOND_BLACK: char = '\u{E0DB}';
const FLAT: char = '\u{E260}';
const NATURAL: char = '\u{E261}';
const SHARP: char = '\u{E262}';
const DOT: char = '\u{E1E7}';
const TUPLET_3: char = '\u{E883}';
const ACCENT_ABOVE: char = '\u{E4A0}';
const ACCENT_BELOW: char = '\u{E4A1}';
/// Rests, whole to 32nd (flags -2..=3).
const RESTS: [char; 6] = [
    '\u{E4E3}', '\u{E4E4}', '\u{E4E5}', '\u{E4E6}', '\u{E4E7}', '\u{E4E8}',
];
/// Flags up and down, eighth to 32nd.
const FLAGS_UP: [char; 3] = ['\u{E240}', '\u{E242}', '\u{E244}'];
const FLAGS_DOWN: [char; 3] = ['\u{E241}', '\u{E243}', '\u{E245}'];
const TIME_DIGITS: char = '\u{E080}';

/// Glyph widths in staff spaces (Bravura's metadata).
const HEAD_W: f64 = 1.18;
const WHOLE_W: f64 = 1.69;
const STEM_LEN: f64 = 3.5;
/// Written notes this many steps above the top line (past the fourth ledger line) go down an octave under 8va.
const OTTAVA_FROM: i32 = 9;

/// The order sharps and flats are added in a key signature, as diatonic steps (C = 0 .. B = 6).
const SHARP_ORDER: [i32; 7] = [3, 0, 4, 1, 5, 2, 6];
const FLAT_ORDER: [i32; 7] = [6, 2, 5, 1, 4, 0, 3];
/// Where each of them sits on a treble staff (diatonic numbers, octave * 7 + step); a bass staff's are 14 lower.
const SHARP_AT: [i32; 7] = [38, 35, 39, 36, 33, 37, 34];
const FLAT_AT: [i32; 7] = [34, 37, 33, 36, 32, 35, 31];
/// The natural notes' pitch classes, C to B.
const NAT: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

/// Spells pitches in a key: the key signature's notes as it has them, the others by their degree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speller {
    /// Sharps (positive) or flats (negative) in the key signature.
    pub fifths: i32,
    /// The key's tonic (pitch class) and mode, when the key is known.
    key: Option<(i32, bool)>,
}

impl Speller {
    pub fn new(key: Option<Key>) -> Speller {
        let Some((tonic, mode)) = key else {
            return Speller { fifths: 0, key: None };
        };
        let minor = mode == Mode::Minor;
        // a minor key shares its relative major's signature
        let major = if minor { (tonic + 3) % 12 } else { tonic % 12 };
        // C G D A E B F# (sharps), F Bb Eb Ab Db (flats)
        const FIFTHS: [i32; 12] = [0, -5, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5];
        let mut fifths = FIFTHS[major as usize];
        // (P49.1: Eb minor is six flats, not the six sharps of its enharmonic relative F#)
        if minor && tonic % 12 == 3 {
            fifths = -6;
        }
        Speller {
            fifths,
            key: Some(((tonic % 12) as i32, minor)),
        }
    }

    /// The key signature's alteration of a diatonic step.
    pub fn key_alter(&self, step: i32) -> i32 {
        let n = self.fifths.unsigned_abs() as usize;
        if self.fifths > 0 && SHARP_ORDER[..n].contains(&step) {
            1
        } else if self.fifths < 0 && FLAT_ORDER[..n].contains(&step) {
            -1
        } else {
            0
        }
    }

    /// A MIDI pitch as (diatonic number = octave * 7 + step, alteration): in the key as the key has it, else a
    /// natural, else by degree (P49.2, the reviews: a blues b3 in E is G, not F double-sharp, and b7 D, not C#; in a
    /// major key the chromatic notes are flats; in a minor key its raised 3rd, 6th and 7th are sharps; with no key,
    /// sharps).
    pub fn spell(&self, midi: i32) -> (i32, i32) {
        self.spell_toward(midi, None)
    }

    /// `spell`, with the note that follows (P49.5, the reviews): a chromatic note a semitone under its next note is a
    /// sharp (it leads up), one a semitone above it a flat (it leads down); B-Bb-A, not B-A#-A.
    pub fn spell_toward(&self, midi: i32, next: Option<i32>) -> (i32, i32) {
        let pc = midi.rem_euclid(12);
        let at = |step: usize, alter: i32| {
            let octave = (midi - NAT[step] - alter).div_euclid(12) - 1;
            (octave * 7 + step as i32, alter)
        };
        for step in 0..7 {
            let a = self.key_alter(step as i32);
            if (NAT[step] + a).rem_euclid(12) == pc {
                return at(step, a);
            }
        }
        if let Some(step) = NAT.iter().position(|&n| n == pc) {
            return at(step, 0);
        }
        let sharp = match (next.map(|n| n - midi), self.key) {
            (Some(1), _) => true,
            (Some(-1), _) => false,
            (_, None) => true,
            (_, Some((tonic, minor))) => {
                let deg = (pc - tonic).rem_euclid(12);
                minor && matches!(deg, 4 | 9 | 11)
            }
        };
        if sharp {
            let step = NAT.iter().position(|&n| n == (pc + 11) % 12).unwrap_or(0);
            at(step, 1)
        } else {
            let step = NAT.iter().position(|&n| n == (pc + 1) % 12).unwrap_or(0);
            at(step, -1)
        }
    }
}

/// A chord name as fingered, with its sounding name after it, for parts written as fingered (P49.5: a B7 grip in
/// E-shaped notation was labelled Bb7): "B7 (Bb7)". Sharps or flats as the written key has them.
pub fn fingered_name(chart: &Chart, name: &str) -> String {
    if chart.written_shift == 0 {
        return name.to_string();
    }
    let sharps = speller(chart).fifths >= 0;
    let shift_root = |root: &str| -> Option<String> {
        let mut ch = root.chars();
        let letter = ch.next()?;
        let step = "CDEFGAB".find(letter)?;
        let acc = match ch.next() {
            Some('#') | Some('♯') => 1,
            Some('b') | Some('♭') => -1,
            _ => 0,
        };
        let pc = (NAT[step] + acc + chart.written_shift).rem_euclid(12);
        const SHARPS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
        const FLATS: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];
        Some(
            if sharps {
                SHARPS[pc as usize]
            } else {
                FLATS[pc as usize]
            }
            .to_string(),
        )
    };
    let root_len = |s: &str| {
        let mut n = s.chars().next().map_or(0, |c| c.len_utf8());
        if let Some(c) = s[n..].chars().next()
            && matches!(c, '#' | 'b' | '♯' | '♭')
        {
            n += c.len_utf8();
        }
        n
    };
    let (main, bass) = match name.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (name, None),
    };
    let rl = root_len(main);
    let Some(root) = shift_root(&main[..rl]) else {
        return name.to_string();
    };
    let mut out = format!("{root}{}", &main[rl..]);
    if let Some(b) = bass {
        let bl = root_len(b);
        if let Some(br) = shift_root(&b[..bl]) {
            out = format!("{out}/{br}{}", &b[bl..]);
        }
    }
    format!("{out} ({name})")
}

/// The staff's bottom line as a diatonic number: E4 on a treble staff, G2 on a bass staff.
pub fn bottom_line(bass: bool) -> i32 {
    if bass { 18 } else { 30 }
}

/// The pitch a tab note sounds (MIDI), from the chart's open strings.
pub fn sounding(chart: &Chart, string: i32, fret: i32, tech: Techs) -> i32 {
    let open = chart
        .opens
        .get(string.max(0) as usize)
        .or(chart.opens.last())
        .copied()
        .unwrap_or(40);
    if tech.has(Techs::HARMONIC)
        && let Some(&(_, st)) = qrs_analysis::detect::HARMONIC_SEMITONES
            .iter()
            .find(|(f, _)| *f == fret)
    {
        return open + st;
    }
    open + fret.max(0)
}

/// The pitch a tab note is written at (MIDI): an octave above sounding, and as fingered (P49.1).
pub fn written(chart: &Chart, string: i32, fret: i32, tech: Techs) -> i32 {
    sounding(chart, string, fret, tech) + 12 + chart.written_shift
}

/// The speller for a chart: the song's key as written (the sounding key moved by the written shift).
pub fn speller(chart: &Chart) -> Speller {
    Speller::new(
        chart
            .key
            .map(|(t, m)| (((t as i32 + chart.written_shift).rem_euclid(12)) as u8, m)),
    )
}

/// The written diatonic range of a chart's notes (lowest, highest), for the room the staff needs (notes under 8va
/// counted an octave down).
pub fn range(chart: &Chart, sp: &Speller) -> Option<(i32, i32)> {
    let top = bottom_line(chart.arr.is_bass) + 8;
    let mut lo_hi: Option<(i32, i32)> = None;
    for g in &chart.glyphs {
        let (mut d, _) = sp.spell(written(chart, g.string, g.fret, g.tech));
        if d - top >= OTTAVA_FROM {
            d -= 7;
        }
        lo_hi = Some(lo_hi.map_or((d, d), |(a, b)| (a.min(d), b.max(d))));
    }
    lo_hi
}

/// A notehead in a chord.
struct Head {
    d: i32,
    string: i32,
    /// The accidental drawn before it (an alteration), if one is needed.
    acc: Option<i32>,
    x_head: bool,
    harm: bool,
    /// Drawn on the stem's other side (a second in a chord).
    flip: bool,
    col: Color32,
}

/// One rhythm piece on the staff, with what its notes ask for.
struct Event<'a> {
    pc: &'a Piece,
    x: f64,
    heads: Vec<Head>,
    up: bool,
    /// Its notes' techniques together.
    tech: Techs,
    bend: f64,
    /// A slide's target as written (diatonic), when one of its notes slides; the sliding note's own position; an
    /// unpitched slide (out of or into nowhere: a short line).
    slide_to: Option<i32>,
    slide_from: i32,
    unpitched: bool,
    /// The note struck by a hammer-on or pull-off (diatonic, string), for its slur's ends.
    legato: Option<(i32, i32)>,
    /// A unison on two strings (written once, given a second stem).
    unison: Option<i32>,
    /// Written an octave lower under an 8va line.
    ottava: bool,
    /// The bar it's in (index into `measures`).
    bar: usize,
}

/// Draws a SMuFL glyph with its left edge at `x` and its baseline at `y` (pixels).
fn sym(p: &Painter, ch: char, x: f64, y: f64, font: &FontId, col: Color32) {
    let g = p.layout_no_wrap(ch.to_string(), font.clone(), col);
    let base = g
        .rows
        .first()
        .and_then(|r| r.row.glyphs.first().map(|gl| r.pos.y + gl.pos.y))
        .unwrap_or(0.0);
    p.galley(pos2(pt(x), pt(y) - base), g, col);
}

fn line(p: &Painter, a: (f64, f64), b: (f64, f64), w: f64, col: Color32) {
    p.line_segment(
        [pos2(pt(a.0), pt(a.1)), pos2(pt(b.0), pt(b.1))],
        Stroke::new(pt(w).max(1.0), col),
    );
}

/// A dashed line from `a` to `b` (horizontal).
fn dashed(p: &Painter, a: (f64, f64), bx: f64, w: f64, dash: f64, col: Color32) {
    let mut x = a.0;
    while x < bx {
        line(p, (x, a.1), ((x + dash).min(bx), a.1), w, col);
        x += dash * 2.0;
    }
}

/// An arc (a slur or a tie) from `a` to `b`, bulging by `bulge` (negative: up).
fn arc(p: &Painter, a: (f64, f64), b: (f64, f64), bulge: f64, w: f64, col: Color32) {
    let pts: Vec<egui::Pos2> = (0..=16)
        .map(|i| {
            let u = i as f64 / 16.0;
            pos2(
                pt(a.0 + (b.0 - a.0) * u),
                pt(a.1 + (b.1 - a.1) * u + bulge * 4.0 * u * (1.0 - u)),
            )
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(pt(w).max(1.0), col)));
}

/// Words over the staff in the UI's face, `size` pixels.
fn words(p: &Painter, s: &str, x: f64, y: f64, size: f64, anchor: Align2, col: Color32) {
    p.text(
        pos2(pt(x), pt(y)),
        anchor,
        s,
        FontId::proportional(pt(size).max(6.0)),
        col,
    );
}

impl TabView {
    /// The staff's room in the layout: staff space, room above the top line and below the bottom line, and the clef
    /// and key signature's width (pixels).
    pub(super) fn staff_layout(&mut self, c: &TabCtx) {
        let chart = c.chart;
        if !c.score.staff() {
            self.stave_h = 0.0;
            self.clef_w = 0.0;
            return;
        }
        let s = super::sc();
        // (P49.4: alone, the staff is drawn bigger; over the tab it stays out of the tab's way)
        let k = if c.score.tab() { 0.42 } else { 0.55 };
        let sp = (self.line_gap * k).max(7.0 * s);
        self.sp = sp;
        let speller = speller(chart);
        let b = bottom_line(chart.arr.is_bass);
        let (lo, hi) = range(chart, &speller).unwrap_or((b, b + 8));
        // ledger lines and stems need room, and over the staff the marks (P.M., 8va, let ring, bends, the tuning note)
        // (P49.5: the first row's header, then the 8va line, then P.M. / let ring, each on a level of its own)
        let above = ((hi - (b + 8)).max(0) as f64 / 2.0 + 1.5) * sp + 6.0 * sp;
        let below = ((b - lo).max(0) as f64 / 2.0 + 1.5) * sp;
        self.stave_above = above.max(4.5 * sp);
        let gap = if c.score.tab() { ipx(14.0) } else { 0.0 };
        self.stave_h = self.stave_above + 4.0 * sp + below.max(2.5 * sp) + gap;
        let n_acc = speller.fifths.unsigned_abs() as f64;
        // clef, key signature, time signature
        self.clef_w = (3.4 + n_acc * 1.0 + 3.4) * sp;
    }

    /// The staff of one row: lines, clef and key signature, bar lines, notes, rests, beams, ties and the marks over
    /// the notes. `top` is the stave band's top (pixels).
    pub(super) fn draw_staff(&self, p: &Painter, c: &TabCtx, top: f64, row: &[usize], first_row: bool) {
        let chart = c.chart;
        let th = t();
        let sp = self.sp;
        let bass = chart.arr.is_bass;
        let b = bottom_line(bass);
        let speller = speller(chart);
        let line_y = top + self.stave_above; // the top line
        let y_of = |d: i32| line_y + 4.0 * sp - (d - b) as f64 * sp / 2.0;
        let font = text::music(pt(4.0 * sp));
        let lw = (0.13 * sp).max(1.0);
        let x_start = self.left - self.clef_w;
        let last = &self.measures[*row.last().unwrap()];
        let x_end = last.x + last.w;
        for k in 0..5 {
            let y = line_y + k as f64 * sp;
            line(p, (x_start, y), (x_end, y), lw, th.line);
        }
        // clef (a plain bass clef: the octave is understood in bass music, P49.4), key signature (every row)
        let ink = th.text_2;
        let mut x = x_start + 0.6 * sp;
        if bass {
            sym(p, F_CLEF, x, y_of(b + 6), &font, ink);
        } else {
            sym(p, G_CLEF_8VB, x, y_of(b + 2), &font, ink);
        }
        x += 3.2 * sp;
        let shift = if bass { -14 } else { 0 };
        let n = speller.fifths.unsigned_abs() as usize;
        for i in 0..n.min(7) {
            let (ch, d) = if speller.fifths > 0 {
                (SHARP, SHARP_AT[i] + shift)
            } else {
                (FLAT, FLAT_AT[i] + shift)
            };
            sym(p, ch, x, y_of(d), &font, ink);
            x += 1.0 * sp;
        }
        let time_sig = |m: &super::Measure, x_t: f64| {
            let beats = m.beats.clamp(1, 99);
            for (num, d) in [(beats, b + 6), (4, b + 2)] {
                let digits: Vec<char> = num
                    .to_string()
                    .chars()
                    .filter_map(|ch| ch.to_digit(10))
                    .map(|v| char::from_u32(TIME_DIGITS as u32 + v).unwrap_or(TIME_DIGITS))
                    .collect();
                let mut xx = x_t;
                for ch in digits {
                    sym(p, ch, xx, y_of(d), &font, ink);
                    xx += 1.8 * sp;
                }
            }
        };
        // the time signature: on the first row, and wherever the meter changes (P49.4)
        let meter_changes = |mi: usize| mi > 0 && self.measures[mi - 1].beats != self.measures[mi].beats;
        if let Some(m) = self.measures.get(row[0])
            && (first_row || meter_changes(row[0]))
        {
            time_sig(m, x + 0.6 * sp);
        }
        for &mi in &row[1..] {
            if meter_changes(mi) {
                time_sig(&self.measures[mi], self.measures[mi].x + 0.5 * sp);
            }
        }
        // over the first row: the tuning ("Tune down ½ step", "6 = D") and a shuffle's mark
        let rh = chart.rhythm.as_ref();
        if first_row {
            let ty = line_y - self.stave_above + 0.9 * sp;
            let mut tx = x_start;
            if !chart.tune_note.is_empty() {
                words(p, &chart.tune_note, tx, ty, 1.5 * sp, Align2::LEFT_TOP, ink);
                tx += (chart.tune_note.chars().count() as f64 * 0.75 + 3.0) * sp;
            }
            if rh.is_some_and(|r| r.shuffle) {
                self.shuffle_mark(p, tx, ty, sp, ink);
                tx += 16.0 * sp;
            }
            // the tempo: a quarter note = beats a minute, from the first bar
            if let Some(m) = self.measures.get(row[0])
                && m.beats > 0
                && m.end > m.start
            {
                let bpm = (60.0 * m.beats as f64 / (m.end - m.start)).round();
                if (20.0..=400.0).contains(&bpm) {
                    let small = text::music(pt(4.0 * sp * 0.55));
                    let base = ty + 1.6 * sp;
                    let s2 = sp * 0.55;
                    sym(p, HEAD_BLACK, tx, base, &small, ink);
                    let sx = tx + HEAD_W * s2 - 0.06 * s2;
                    line(
                        p,
                        (sx, base),
                        (sx, base - STEM_LEN * s2),
                        (0.12 * s2).max(1.0),
                        ink,
                    );
                    words(
                        p,
                        &format!("= {bpm}"),
                        tx + 1.6 * s2,
                        base - 0.3 * sp,
                        1.5 * sp,
                        Align2::LEFT_CENTER,
                        ink,
                    );
                }
            }
        }
        // bar lines
        for &mi in row {
            let m = &self.measures[mi];
            line(p, (m.x, line_y), (m.x, line_y + 4.0 * sp), lw * 1.2, th.bar_line);
        }
        line(
            p,
            (x_end, line_y),
            (x_end, line_y + 4.0 * sp),
            lw * 1.2,
            th.bar_line,
        );

        let Some(rh) = rh else { return };
        let (r_start, r_end) = (self.measures[row[0]].start, last.end);
        let times = &rh.staff_times;
        let (i0, i1) = (
            lower_bound(times, r_start - 1e-4),
            lower_bound(times, r_end - 1e-4),
        );
        let pieces = &rh.staff[i0..i1.max(i0)];
        let mid = b + 4;
        let top_line = b + 8;
        // the notes a piece starts, as (written diatonic, alteration, glyph)
        // (every note the tab snaps to this slot: two chart times can share one, and the second's notes went missing,
        // P49.5; up to the next slot's onset or 70 ms on)
        let notes_range = |o: f64| {
            let g0 = lower_bound(&chart.gtimes, o - 1e-3);
            let k = lower_bound(&rh.staff_times, rh.snapped_time(o) + 1e-4);
            let next = rh.staff[k.min(rh.staff.len())..]
                .iter()
                .find_map(|pc| pc.onset.filter(|&t| t > o + 1e-3))
                .unwrap_or(f64::INFINITY);
            let g1 = lower_bound(&chart.gtimes, (o + 0.07).min(next - 1e-3).max(o + 1e-3));
            (g0, g1.max(g0))
        };
        let notes_at = |o: f64| {
            let (g0, g1) = notes_range(o);
            &chart.glyphs[g0..g1]
        };
        // a note held into this row from the last: its heads, so the tie and the note aren't lost (P49.1)
        let mut prev_heads: Option<(f64, Vec<i32>, bool)> = None;
        if pieces
            .iter()
            .find(|pc| !pc.rest)
            .is_some_and(|pc| pc.tied && pc.onset.is_none())
            && let Some(o) = rh.staff[..i0].iter().rev().find_map(|pc| pc.onset)
        {
            let mut ds: Vec<i32> = notes_at(o)
                .iter()
                .map(|g| speller.spell(written(chart, g.string, g.fret, g.tech)).0)
                .collect();
            ds.sort_unstable();
            ds.dedup();
            if !ds.is_empty() {
                prev_heads = Some((self.left - 2.2 * sp, ds, true));
            }
        }
        // the bar's accidentals so far: diatonic number -> alteration in force; reset at each bar line
        let mut in_force: HashMap<i32, i32> = HashMap::new();
        let mut bar_of_last: Option<usize> = None;
        let mut events: Vec<Event> = Vec::new();
        let mut ties: Vec<(usize, Option<usize>, Vec<i32>, f64)> = Vec::new(); // (to event, from event, heads, from x)
        let mut last_note_event: Option<usize> = None;
        for pc in pieces {
            let bar = self.measure_index(pc.t);
            if bar_of_last != Some(bar) {
                in_force.clear();
                bar_of_last = Some(bar);
            }
            let x = if pc.bar_rest {
                let m = &self.measures[bar.min(self.measures.len() - 1)];
                m.x + m.w / 2.0
            } else {
                match pc.onset {
                    Some(o) if !c.snap => self.x_of(o, None),
                    Some(o) => self.x_of(rh.snapped_time(o), None),
                    None => self.x_of(pc.t, None),
                }
            };
            let mut ev = Event {
                pc,
                x,
                heads: Vec::new(),
                up: true,
                tech: Techs::default(),
                bend: 0.0,
                slide_to: None,
                slide_from: 0,
                unpitched: false,
                legato: None,
                unison: None,
                ottava: false,
                bar,
            };
            if pc.rest {
                events.push(ev);
                continue;
            }
            if let Some(o) = pc.onset {
                let (g0, g1) = notes_range(o);
                let group = &chart.glyphs[g0..g1];
                // a single note's next note, for spelling a chromatic step toward it
                let next = (group.len() == 1)
                    .then(|| chart.glyphs.get(g1))
                    .flatten()
                    .map(|n| written(chart, n.string, n.fret, n.tech));
                for g in group {
                    let midi = written(chart, g.string, g.fret, g.tech);
                    let (d, alter) = speller.spell_toward(midi, next);
                    ev.tech |= g.tech;
                    if g.bend > ev.bend {
                        ev.bend = g.bend;
                    }
                    if g.slide_to > 0 && ev.slide_to.is_none() {
                        ev.slide_to = Some(
                            speller
                                .spell(written(chart, g.string, g.slide_to, Techs::default()))
                                .0,
                        );
                        ev.slide_from = d;
                        ev.unpitched = g.tech.has(Techs::UNPITCHED_SLIDE) && !g.tech.has(Techs::SLIDE);
                    }
                    if g.tech.has(Techs::HAMMER_ON) || g.tech.has(Techs::PULL_OFF) {
                        ev.legato = Some((d, g.string));
                    }
                    if let Some(h) = ev.heads.iter().find(|h| h.d == d) {
                        if h.string != g.string {
                            ev.unison = Some(d);
                        }
                        continue;
                    }
                    let state = g.group.and_then(|i| c.states.get(i).copied().flatten());
                    let col = if (g.t - c.now).abs() <= 0.06 {
                        th.accent
                    } else if let Some(st) = state {
                        judge_color(match st {
                            qrs_analysis::Judgement::Hit => "hit",
                            qrs_analysis::Judgement::Late => "late",
                            qrs_analysis::Judgement::Early => "early",
                            qrs_analysis::Judgement::Miss => "miss",
                        })
                    } else if g.t < c.now - 0.06 {
                        th.caption
                    } else {
                        th.text
                    };
                    let force = in_force
                        .get(&d)
                        .copied()
                        .unwrap_or_else(|| speller.key_alter(d.rem_euclid(7)));
                    // (a dead note's X carries no accidental, P49.5)
                    let dead = g.tech.has(Techs::FRET_HAND_MUTE);
                    let acc = (force != alter && !dead).then_some(alter);
                    in_force.insert(d, alter);
                    ev.heads.push(Head {
                        d,
                        string: g.string,
                        acc,
                        x_head: g.tech.has(Techs::FRET_HAND_MUTE),
                        harm: g.tech.has(Techs::HARMONIC) || g.tech.has(Techs::PINCH_HARMONIC),
                        flip: false,
                        col,
                    });
                }
            } else if pc.tied
                && let Some((px, ds, _)) = &prev_heads
            {
                let col = if pc.t < c.now { th.caption } else { th.text };
                for &d in ds {
                    ev.heads.push(Head {
                        d,
                        string: -1,
                        acc: None,
                        x_head: false,
                        harm: false,
                        flip: false,
                        col,
                    });
                }
                ties.push((events.len(), last_note_event, ds.clone(), *px));
            }
            if ev.heads.is_empty() {
                continue;
            }
            ev.heads.sort_by_key(|h| h.d);
            // 8va: a note well above the staff is written an octave down under an 8va line (P49.4)
            if ev.heads.last().is_some_and(|h| h.d - top_line >= OTTAVA_FROM) {
                ev.ottava = true;
                for h in &mut ev.heads {
                    h.d -= 7;
                }
                ev.slide_from -= 7;
                ev.legato = ev.legato.map(|(d, st)| (d - 7, st));
                ev.unison = ev.unison.map(|d| d - 7);
            }
            // stems away from the middle line: the note farthest from it decides; on the middle line, down (P49.2)
            let (lo, hi) = (ev.heads[0].d, ev.heads[ev.heads.len() - 1].d);
            ev.up = (mid - lo) > (hi - mid);
            prev_heads = Some((x, ev.heads.iter().map(|h| h.d).collect(), ev.up));
            last_note_event = Some(events.len());
            events.push(ev);
        }
        self.space_events(&mut events, sp);
        // beams: runs of eighths or shorter in the same beat (a rest breaks a run); in a bar of four, straight eighths
        // are beamed in fours, half a bar at a time (P49.4)
        let mut runs: Vec<Vec<usize>> = Vec::new();
        if c.beams {
            let mut run: Vec<usize> = Vec::new();
            for (k, e) in events.iter().enumerate() {
                let beamable = !e.pc.rest && e.pc.flags >= 1;
                if beamable
                    && run
                        .last()
                        .is_some_and(|&l| l + 1 == k && events[l].pc.beat() == e.pc.beat())
                {
                    run.push(k);
                } else {
                    if run.len() >= 2 {
                        runs.push(std::mem::take(&mut run));
                    }
                    run = if beamable { vec![k] } else { Vec::new() };
                }
            }
            if run.len() >= 2 {
                runs.push(run);
            }
            let plain = |r: &Vec<usize>| {
                r.iter().all(|&k| {
                    let pc = events[k].pc;
                    pc.flags == 1 && pc.dots == 0 && !pc.triplet
                }) && r.len() == 2
            };
            // (by beat position, P49.5: by time, a tempo change put beat 3 in the first half and hid it)
            let half_of = |k: usize| {
                let beat = events[k].pc.pos.floor();
                let i = rh.bars.partition_point(|&b| b <= beat);
                let start = if i > 0 { rh.bars[i - 1] } else { 0 };
                let end = rh.bars.get(i).copied().unwrap_or(start + 4);
                (end - start == 4).then(|| (start, (beat - start) / 2))
            };
            let mut merged: Vec<Vec<usize>> = Vec::new();
            for r in runs {
                if let Some(prev) = merged.last_mut()
                    && plain(prev)
                    && plain(&r)
                    && prev.last().is_some_and(|&l| l + 1 == r[0])
                    && half_of(prev[0]).is_some()
                    && half_of(prev[0]) == half_of(r[0])
                {
                    prev.extend(r);
                    continue;
                }
                merged.push(r);
            }
            runs = merged;
        }
        // each beam: one stem direction, and a line sloped with the notes (at most a space, P49.4)
        let mut beam_line: HashMap<usize, (f64, f64, f64)> = HashMap::new(); // event -> (x0, y0, slope)
        for run in &runs {
            let sum: i32 = run
                .iter()
                .map(|&k| {
                    let h = &events[k].heads;
                    (h[0].d + h[h.len() - 1].d) - 2 * mid
                })
                .sum();
            let up = sum < 0;
            for &k in run {
                events[k].up = up;
            }
            let (k0, k1) = (run[0], run[run.len() - 1]);
            let (x0, x1) = (events[k0].x, events[k1].x);
            let outer = |k: usize| {
                let h = &events[k].heads;
                if up { y_of(h[h.len() - 1].d) } else { y_of(h[0].d) }
            };
            let rise = (outer(k1) - outer(k0)).clamp(-sp, sp) * 0.6;
            let slope = if x1 > x0 { rise / (x1 - x0) } else { 0.0 };
            // the line's height: every stem at least its length
            let y0 = if up {
                run.iter()
                    .map(|&k| outer(k) - STEM_LEN * sp - slope * (events[k].x - x0))
                    .fold(f64::MAX, f64::min)
                    .min(y_of(mid) - slope * ((x0 + x1) / 2.0 - x0))
            } else {
                run.iter()
                    .map(|&k| outer(k) + STEM_LEN * sp - slope * (events[k].x - x0))
                    .fold(f64::MIN, f64::max)
                    .max(y_of(mid) - slope * ((x0 + x1) / 2.0 - x0))
            };
            for &k in run {
                beam_line.insert(k, (x0, y0, slope));
            }
        }
        let in_run: HashMap<usize, usize> = runs
            .iter()
            .enumerate()
            .flat_map(|(ri, r)| r.iter().map(move |&k| (k, ri)))
            .collect();
        // seconds in a chord: the upper (stem up) or lower (stem down) of two neighbours on the other side
        for e in &mut events {
            if e.up {
                for i in 1..e.heads.len() {
                    if e.heads[i].d - e.heads[i - 1].d == 1 && !e.heads[i - 1].flip {
                        e.heads[i].flip = true;
                    }
                }
            } else {
                for i in (0..e.heads.len().saturating_sub(1)).rev() {
                    if e.heads[i + 1].d - e.heads[i].d == 1 && !e.heads[i + 1].flip {
                        e.heads[i].flip = true;
                    }
                }
            }
        }
        let stem_w = (0.12 * sp).max(1.0);
        let mut stem_ends: Vec<(f64, f64)> = vec![(0.0, 0.0); events.len()];
        let ledger = th.bar_line;
        for (k, e) in events.iter().enumerate() {
            let pc = e.pc;
            if pc.rest {
                let col = if pc.t < c.now { th.caption } else { th.text_2 };
                if pc.bar_rest {
                    // a whole bar's rest: the whole rest hanging from the fourth line, in the bar's middle
                    sym(p, RESTS[0], e.x - 0.8 * sp, y_of(b + 6), &font, col);
                    continue;
                }
                let fl = (pc.flags + 2).clamp(0, 5) as usize;
                let d = match pc.flags {
                    -2 => b + 6,
                    _ => mid,
                };
                sym(p, RESTS[fl], e.x - 0.6 * sp, y_of(d), &font, col);
                if pc.dots > 0 {
                    sym(p, DOT, e.x + 0.9 * sp, y_of(mid + 1), &font, col);
                }
                continue;
            }
            let (head, hw) = match pc.flags {
                -2 => (HEAD_WHOLE, WHOLE_W),
                -1 => (HEAD_HALF, HEAD_W),
                _ => (HEAD_BLACK, HEAD_W),
            };
            let diamond = match pc.flags {
                -2 => HEAD_DIAMOND_WHOLE,
                -1 => HEAD_DIAMOND_HALF,
                _ => HEAD_DIAMOND_BLACK,
            };
            let hx = e.x - hw * sp / 2.0;
            let stem_x = if e.up {
                hx + hw * sp - stem_w / 2.0
            } else {
                hx + stem_w / 2.0
            };
            // accidentals: columns to the left, a new column when two would be within a sixth
            let mut cols: Vec<Vec<i32>> = Vec::new();
            for h in e.heads.iter().rev() {
                let Some(a) = h.acc else { continue };
                let ci = cols
                    .iter()
                    .position(|cl| cl.iter().all(|&d| (d - h.d).abs() >= 6))
                    .unwrap_or_else(|| {
                        cols.push(Vec::new());
                        cols.len() - 1
                    });
                cols[ci].push(h.d);
                let ch = match a {
                    1 => SHARP,
                    -1 => FLAT,
                    _ => NATURAL,
                };
                let flip_left = e.heads.iter().any(|h| h.flip) && !e.up;
                let ax = hx - (1.25 + ci as f64 * 1.1) * sp - if flip_left { hw * sp } else { 0.0 };
                sym(p, ch, ax, y_of(h.d), &font, h.col);
            }
            for h in &e.heads {
                let x = if h.flip {
                    if e.up {
                        hx + hw * sp - stem_w
                    } else {
                        hx - hw * sp + stem_w
                    }
                } else {
                    hx
                };
                let y = y_of(h.d);
                // ledger lines out to the note, a little brighter than the staff (P49.4)
                let lx = (x - 0.5 * sp, x + (hw + 0.5) * sp);
                let mut l = b - 2;
                while l >= h.d {
                    line(p, (lx.0, y_of(l)), (lx.1, y_of(l)), lw * 1.3, ledger);
                    l -= 2;
                }
                let mut l = b + 10;
                while l <= h.d {
                    line(p, (lx.0, y_of(l)), (lx.1, y_of(l)), lw * 1.3, ledger);
                    l += 2;
                }
                let glyph = if h.x_head {
                    HEAD_X
                } else if h.harm {
                    diamond
                } else {
                    head
                };
                sym(p, glyph, x, y, &font, h.col);
                if pc.dots > 0 {
                    let dy = if (h.d - b).rem_euclid(2) == 0 { 1 } else { 0 };
                    sym(p, DOT, hx + (hw + 0.4) * sp, y_of(h.d + dy), &font, h.col);
                }
            }
            if pc.flags == -2 {
                stem_ends[k] = (e.x, y_of(e.heads.last().unwrap().d));
                continue;
            }
            let col = e.heads[0].col;
            let (far, near) = if e.up {
                (y_of(e.heads.last().unwrap().d), y_of(e.heads[0].d))
            } else {
                (y_of(e.heads[0].d), y_of(e.heads.last().unwrap().d))
            };
            // (a flagged stem down is a little longer, so the flag clears the head, P49.2)
            let flag_room = if pc.flags >= 1 && !in_run.contains_key(&k) && !e.up {
                0.8 * sp
            } else {
                0.0
            };
            let end = match beam_line.get(&k) {
                Some(&(x0, y0, slope)) => y0 + slope * (e.x - x0),
                None if e.up => (far - STEM_LEN * sp).min(y_of(mid)),
                None => (far + STEM_LEN * sp + flag_room).max(y_of(mid)),
            };
            line(p, (stem_x, near), (stem_x, end), stem_w, col);
            stem_ends[k] = (stem_x, end);
            if pc.flags >= 1 && !in_run.contains_key(&k) {
                let fl = (pc.flags - 1).clamp(0, 2) as usize;
                let ch = if e.up { FLAGS_UP[fl] } else { FLAGS_DOWN[fl] };
                sym(p, ch, stem_x - stem_w / 2.0, end, &font, col);
            }
        }
        // the beams themselves: one per level (eighths 1, sixteenths 2, 32nds 3), short stubs for a lone note
        let thick = 0.5 * sp;
        for run in &runs {
            let up = events[run[0]].up;
            let col = events[run[0]].heads[0].col;
            let (_, _, slope) = beam_line[&run[0]];
            for lvl in 1..=3 {
                let off = (lvl - 1) as f64 * 0.75 * sp;
                for (idx, &k) in run.iter().enumerate() {
                    if events[k].pc.flags < lvl {
                        continue;
                    }
                    let (x, y) = stem_ends[k];
                    let y = if up { y + off } else { y - off };
                    let nxt = run.get(idx + 1).copied();
                    let prv = if idx > 0 { Some(run[idx - 1]) } else { None };
                    let x2 = if let Some(nk) = nxt.filter(|&nk| events[nk].pc.flags >= lvl) {
                        stem_ends[nk].0
                    } else if prv.is_none_or(|pk| events[pk].pc.flags < lvl) {
                        x + if nxt.is_some() { 1.1 * sp } else { -1.1 * sp }
                    } else {
                        continue;
                    };
                    let (xa, xb) = if x2 >= x { (x, x2) } else { (x2, x) };
                    let ya = y + slope * (xa - x);
                    let yb = y + slope * (xb - x);
                    let th_y = if up { thick } else { -thick };
                    p.add(Shape::convex_polygon(
                        vec![
                            pos2(pt(xa), pt(ya)),
                            pos2(pt(xb), pt(yb)),
                            pos2(pt(xb), pt(yb + th_y)),
                            pos2(pt(xa), pt(ya + th_y)),
                        ],
                        col,
                        Stroke::NONE,
                    ));
                }
            }
        }
        // the highest thing over each event (stem end, head, beam), for the marks above
        let top_of = |k: usize| -> f64 {
            let e = &events[k];
            if e.pc.rest {
                return y_of(mid + 3);
            }
            let mut head_top = y_of(e.heads.last().unwrap().d) - 0.6 * sp;
            if e.tech.has(Techs::ACCENT) && !e.up {
                head_top -= 1.4 * sp; // (the accent sits over the head)
            }
            if e.up {
                head_top.min(stem_ends[k].1)
            } else {
                head_top
            }
        };
        // triplets: a bracketed 3 above the group, the same side every time (P49.2); no bracket under one beam
        let mut k = 0;
        while k < events.len() {
            if events[k].pc.triplet {
                let mut j = k;
                while j + 1 < events.len()
                    && events[j + 1].pc.triplet
                    && events[j + 1].pc.beat() == events[k].pc.beat()
                {
                    j += 1;
                }
                let (xa, xb) = (events[k].x - 0.6 * sp, events[j].x + 0.6 * sp);
                let xm = (xa + xb) / 2.0;
                let beamed = (k..=j).all(|i| in_run.get(&i).is_some_and(|&r| Some(&r) == in_run.get(&k)));
                // (under one beam the 3 goes on the beam's side, P49.5; otherwise above, bracketed)
                let below = beamed && !events[k].up;
                let y = if below {
                    (k..=j)
                        .map(|i| stem_ends[i].1)
                        .fold(f64::MIN, f64::max)
                        .max(line_y + 4.0 * sp)
                        + 1.0 * sp
                } else {
                    (k..=j).map(top_of).fold(f64::MAX, f64::min).min(line_y) - 1.6 * sp
                };
                if !beamed {
                    let yb = y + 0.2 * sp;
                    let gap = 0.9 * sp;
                    line(p, (xa, yb), (xm - gap, yb), lw, th.text_2);
                    line(p, (xm + gap, yb), (xb, yb), lw, th.text_2);
                    line(p, (xa, yb), (xa, yb + 0.6 * sp), lw, th.text_2);
                    line(p, (xb, yb), (xb, yb + 0.6 * sp), lw, th.text_2);
                }
                let ty = if below { y + 1.0 * sp } else { y + 0.7 * sp };
                sym(p, TUPLET_3, xm - 0.45 * sp, ty, &font, th.text_2);
                k = j + 1;
            } else {
                k += 1;
            }
        }
        // ties: an arc from each held note to its continuation, away from the stems (from the clef side when the
        // note was struck in the row before)
        let tie_w = 0.16 * sp;
        for (to, from, ds, px) in ties {
            let x2 = events[to].x;
            let x1 = from.map_or(px, |f| events[f].x);
            let single = ds.len() == 1;
            let stem_up = events[to].up;
            for d in ds {
                let up_side = if single { !stem_up } else { d >= mid };
                let y = y_of(d) + if up_side { -0.6 * sp } else { 0.6 * sp };
                let (a, bx) = (x1 + 0.7 * HEAD_W * sp, x2 - 0.2 * HEAD_W * sp);
                if bx <= a {
                    continue;
                }
                let bulge = if up_side { -0.7 * sp } else { 0.7 * sp };
                arc(p, (a, y), (bx, y), bulge, tie_w, th.text_2);
            }
        }
        // the techniques (P49.3): slurs with H / P, slides, bends, accents, harmonics, vibrato, then the lines over
        // the staff (P.M., let ring, 8va)
        let mark = th.text_2;
        let note_events: Vec<usize> = (0..events.len()).filter(|&k| !events[k].pc.rest).collect();
        for (n, &k) in note_events.iter().enumerate() {
            let e = &events[k];
            if e.heads.is_empty() || e.pc.onset.is_none() {
                continue;
            }
            let hd = e.heads.last().unwrap().d;
            let lo_d = e.heads[0].d;
            // hammer-on / pull-off: a slur from the note before, H or P over it
            let ho = e.tech.has(Techs::HAMMER_ON);
            if (ho || e.tech.has(Techs::PULL_OFF))
                && let Some(&pk) = n.checked_sub(1).and_then(|i| note_events.get(i))
            {
                let pe = &events[pk];
                // (from the string that moves, P49.5: the same string in the note before, else its nearest note)
                let (ld, ls) = e.legato.unwrap_or((hd, -1));
                let pd = pe
                    .heads
                    .iter()
                    .find(|h| h.string == ls)
                    .or_else(|| pe.heads.iter().min_by_key(|h| (h.d - ld).abs()))
                    .map_or(ld, |h| h.d);
                let above = !e.up;
                let off = if above { -0.9 * sp } else { 0.9 * sp };
                let (ya, yb) = (y_of(pd) + off, y_of(ld) + off);
                let _ = lo_d;
                let (xa, xb) = (pe.x + 0.2 * sp, e.x - 0.2 * sp);
                if xb > xa {
                    let bulge = if above { -0.9 * sp } else { 0.9 * sp };
                    arc(p, (xa, ya), (xb, yb), bulge, tie_w, mark);
                    // (clear of the arc's peak, P49.5)
                    let (ty, anchor) = if above {
                        (ya.min(yb) + bulge - 0.3 * sp, Align2::CENTER_BOTTOM)
                    } else {
                        (ya.max(yb) + bulge + 0.3 * sp, Align2::CENTER_TOP)
                    };
                    words(
                        p,
                        if ho { "H" } else { "P" },
                        (xa + xb) / 2.0,
                        ty,
                        1.2 * sp,
                        anchor,
                        mark,
                    );
                }
            }
            // a slide: a straight line toward its target (the next note, or a short way on)
            if let Some(td) = e.slide_to {
                // (from the sliding note's own head, clear of its dot, P49.5)
                let y0 = y_of(e.slide_from);
                let td = if e.ottava { td - 7 } else { td };
                let x0 = e.x + if e.pc.dots > 0 { 1.5 * sp } else { 0.8 * sp };
                if e.unpitched {
                    // out of the note into nowhere: a short line down (or up) and away
                    let dy = if td < e.slide_from { 1.2 * sp } else { -1.2 * sp };
                    line(p, (x0, y0), (x0 + 2.0 * sp, y0 + dy), lw * 1.4, mark);
                } else {
                    let x1 = note_events
                        .get(n + 1)
                        .map_or(x0 + 2.5 * sp, |&nk| events[nk].x - 0.9 * sp)
                        .max(x0 + 1.0 * sp);
                    line(p, (x0, y0), (x1, y_of(td)), lw * 1.4, mark);
                }
            }
            // a unison on two strings: a second stem, the other way
            if let Some(ud) = e.unison
                && e.pc.flags > -2
                && !in_run.contains_key(&k)
            {
                let y = y_of(ud);
                let hw = HEAD_W * sp;
                let hx = e.x - hw / 2.0;
                let sw = (0.12 * sp).max(1.0);
                let (sx, end) = if e.up {
                    (hx + sw / 2.0, y + STEM_LEN * sp + 0.8 * sp)
                } else {
                    (hx + hw - sw / 2.0, y - STEM_LEN * sp)
                };
                line(p, (sx, y), (sx, end), sw, e.heads[0].col);
                if e.pc.flags >= 1 {
                    let fl = (e.pc.flags - 1).clamp(0, 2) as usize;
                    let ch = if e.up { FLAGS_DOWN[fl] } else { FLAGS_UP[fl] };
                    sym(p, ch, sx - sw / 2.0, end, &font, e.heads[0].col);
                }
            }
            // a bend: a curved arrow up from the head, the amount at its tip
            if e.bend > 0.0 {
                let (x0, y0) = (e.x + 0.8 * sp, y_of(hd));
                let (x1, y1) = (x0 + 1.6 * sp, y0 - 3.0 * sp);
                arc(p, (x0, y0), (x1, y1 + 0.4 * sp), 0.3 * sp, lw * 1.4, mark);
                p.add(Shape::convex_polygon(
                    vec![
                        pos2(pt(x1 - 0.4 * sp), pt(y1 + 0.5 * sp)),
                        pos2(pt(x1 + 0.4 * sp), pt(y1 + 0.5 * sp)),
                        pos2(pt(x1), pt(y1 - 0.2 * sp)),
                    ],
                    mark,
                    Stroke::NONE,
                ));
                let amount = match (e.bend * 2.0).round() as i32 {
                    ..=1 => "¼".to_string(),
                    2 => "½".to_string(),
                    4 => "full".to_string(),
                    q if q % 4 == 0 => format!("{}", q / 4),
                    q if q % 4 == 2 => format!("{}½", q / 4),
                    _ => format!("{}", e.bend / 2.0),
                };
                words(
                    p,
                    &amount,
                    x1,
                    y1 - 0.3 * sp,
                    1.2 * sp,
                    Align2::CENTER_BOTTOM,
                    mark,
                );
            }
            // an accent: on the head's side away from the stem
            if e.tech.has(Techs::ACCENT) {
                if e.up {
                    sym(
                        p,
                        ACCENT_BELOW,
                        e.x - 0.55 * sp,
                        y_of(lo_d) + 1.6 * sp,
                        &font,
                        mark,
                    );
                } else {
                    sym(p, ACCENT_ABOVE, e.x - 0.55 * sp, y_of(hd) - 1.2 * sp, &font, mark);
                }
            }
            // a harmonic: its diamond head is drawn; "Harm." over it
            if e.heads.iter().any(|h| h.harm) {
                words(
                    p,
                    "Harm.",
                    e.x,
                    top_of(k) - 0.4 * sp,
                    1.1 * sp,
                    Align2::CENTER_BOTTOM,
                    mark,
                );
            }
            // a tap
            if e.tech.has(Techs::TAP) {
                words(
                    p,
                    "T",
                    e.x,
                    top_of(k) - 0.4 * sp,
                    1.2 * sp,
                    Align2::CENTER_BOTTOM,
                    mark,
                );
            }
            // vibrato: a wavy line over the note for as long as it lasts
            if e.tech.has(Techs::VIBRATO) {
                let x0 = e.x - 0.2 * sp;
                let x1 = note_events
                    .get(n + 1)
                    .map_or(x0 + 3.0 * sp, |&nk| events[nk].x - 0.6 * sp)
                    .max(x0 + 1.6 * sp);
                let yv = top_of(k).min(line_y) - 0.9 * sp;
                let pts: Vec<egui::Pos2> = (0..=((x1 - x0) / (0.12 * sp)) as usize)
                    .map(|i| {
                        let xx = x0 + i as f64 * 0.12 * sp;
                        pos2(
                            pt(xx),
                            pt(yv + 0.3 * sp * (xx / (0.5 * sp) * std::f64::consts::PI).sin()),
                        )
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new(pt(lw * 1.3).max(1.0), mark)));
            }
        }
        // lines over the staff: runs of palm-muted notes (P.M.) and the let-ring of picked chords on one level, 8va
        // on the level above it
        let lvl0 = line_y - self.stave_above + 5.6 * sp;
        let lvl1 = lvl0 - 1.5 * sp;
        let mut run_start: Option<usize> = None;
        let pm = |k: usize| events[k].tech.has(Techs::PALM_MUTE);
        for (n, &k) in note_events.iter().enumerate() {
            let on = pm(k);
            if on && run_start.is_none() {
                run_start = Some(n);
            }
            let ends = !note_events.get(n + 1).is_some_and(|&nk| pm(nk));
            if on
                && ends
                && let Some(s) = run_start.take()
            {
                let (xa, xb) = (events[note_events[s]].x, events[k].x);
                words(
                    p,
                    "P.M.",
                    xa - 0.6 * sp,
                    lvl0,
                    1.2 * sp,
                    Align2::LEFT_CENTER,
                    mark,
                );
                let dx = xa + 2.6 * sp;
                if xb > dx {
                    dashed(p, (dx, lvl0), xb + 0.4 * sp, lw, 0.5 * sp, mark);
                    line(
                        p,
                        (xb + 0.4 * sp, lvl0 - 0.4 * sp),
                        (xb + 0.4 * sp, lvl0 + 0.4 * sp),
                        lw,
                        mark,
                    );
                }
            }
        }
        // (P49.5: the shapes the chart marks as arpeggios, and those it picks through: "hold it, pick through")
        for h in chart
            .shapes
            .iter()
            .enumerate()
            .filter(|(i, h)| {
                (h.arpeggio || chart.shape_picked.get(*i).copied().unwrap_or(false))
                    && h.end > r_start
                    && h.start < r_end
            })
            .map(|(_, h)| h)
        {
            let (xa, xb) = (
                self.x_of(h.start.max(r_start), None),
                self.x_of(h.end.min(r_end), None),
            );
            if xb - xa < 4.0 * sp {
                continue;
            }
            let busy = note_events
                .iter()
                .any(|&k| pm(k) && events[k].x >= xa && events[k].x <= xb);
            let y = if busy { lvl1 } else { lvl0 };
            words(p, "let ring", xa, y, 1.2 * sp, Align2::LEFT_CENTER, mark);
            dashed(p, (xa + 4.0 * sp, y), xb, lw, 0.5 * sp, mark);
        }
        let mut k = 0;
        while k < note_events.len() {
            if events[note_events[k]].ottava {
                let mut j = k;
                while j + 1 < note_events.len() && events[note_events[j + 1]].ottava {
                    j += 1;
                }
                let (xa, xb) = (
                    events[note_events[k]].x - 0.6 * sp,
                    events[note_events[j]].x + 0.8 * sp,
                );
                let y = lvl1 - 1.3 * sp;
                words(p, "8va", xa, y, 1.3 * sp, Align2::LEFT_CENTER, mark);
                dashed(p, (xa + 2.4 * sp, y), xb, lw, 0.5 * sp, mark);
                line(p, (xb, y), (xb, y + 0.8 * sp), lw, mark);
                k = j + 1;
            } else {
                k += 1;
            }
        }
    }

    /// Room for every note (P49.1, the reviews: notes ran into each other in busy bars, an accidental sat over the bar
    /// line): within each bar, each note at least a head and a half after the last, more for its accidentals and the
    /// last one's dot, and clear of the bar line; a bar too narrow for that is shared out evenly.
    fn space_events(&self, events: &mut [Event], sp: f64) {
        let mut k = 0;
        while k < events.len() {
            let bar = events[k].bar;
            let mut j = k;
            while j + 1 < events.len() && events[j + 1].bar == bar {
                j += 1;
            }
            if let Some(m) = self.measures.get(bar) {
                let acc_w = |e: &Event| {
                    let n = e.heads.iter().filter(|h| h.acc.is_some()).count().min(3) as f64;
                    if n > 0.0 { (0.4 + n * 1.1) * sp } else { 0.0 }
                };
                // (room after the bar line for a time signature where the meter changes)
                let meter = bar > 0 && self.measures.get(bar - 1).is_some_and(|pm| pm.beats != m.beats);
                let x0 = m.x + if meter { 4.0 * sp } else { 1.2 * sp };
                let x1 = m.x + m.w - 1.0 * sp;
                let mut want: Vec<f64> = Vec::with_capacity(j - k + 1);
                for i in k..=j {
                    let e = &events[i];
                    if e.pc.bar_rest {
                        want.push(e.x);
                        continue;
                    }
                    let lo = if i == k {
                        x0 + acc_w(e)
                    } else {
                        let pe = &events[i - 1];
                        let dot = if pe.pc.dots > 0 { 0.7 * sp } else { 0.0 };
                        // (an unbeamed flag reaches right of its stem; a rest is wider than a head, P49.5)
                        let flag = if pe.pc.flags >= 1 && !pe.pc.rest {
                            0.6 * sp
                        } else {
                            0.0
                        };
                        let rest = if e.pc.rest { 0.5 * sp } else { 0.0 };
                        want[i - k - 1] + 2.1 * sp + acc_w(e) + dot * 1.4 + flag + rest
                    };
                    want.push(e.x.max(lo));
                }
                let over = want.last().copied().unwrap_or(x1) > x1;
                if over && j > k {
                    // not room for all: share the bar out evenly, accidentals first
                    let n = (j - k + 1) as f64;
                    let step = (x1 - x0) / n;
                    for (q, w) in want.iter_mut().enumerate() {
                        *w = x0 + step * (q as f64 + 0.5);
                    }
                }
                for (q, i) in (k..=j).enumerate() {
                    if !events[i].pc.bar_rest {
                        events[i].x = want[q];
                    }
                }
            }
            k = j + 1;
        }
    }

    /// "Shuffle" and its rule (two eighths = a quarter and an eighth under a 3), drawn small at (x, y) (P49.2).
    fn shuffle_mark(&self, p: &Painter, x: f64, y: f64, sp: f64, col: Color32) {
        words(p, "Shuffle", x, y, 1.5 * sp, Align2::LEFT_TOP, col);
        let k = 0.55;
        let font = text::music(pt(4.0 * sp * k));
        let s = sp * k;
        let base = y + 3.4 * sp;
        let x0 = x + 6.6 * sp;
        let stem_w = (0.12 * s).max(1.0);
        let note = |xh: f64, flag: bool| {
            sym(p, HEAD_BLACK, xh, base, &font, col);
            let sx = xh + HEAD_W * s - stem_w / 2.0;
            line(p, (sx, base), (sx, base - STEM_LEN * s), stem_w, col);
            if flag {
                sym(p, FLAGS_UP[0], sx - stem_w / 2.0, base - STEM_LEN * s, &font, col);
            }
            sx
        };
        // two beamed eighths
        let a = note(x0, false);
        let b = note(x0 + 2.4 * s, false);
        p.rect_filled(
            egui::Rect::from_min_max(
                pos2(pt(a), pt(base - STEM_LEN * s)),
                pos2(pt(b), pt(base - STEM_LEN * s + 0.5 * s)),
            ),
            0.0,
            col,
        );
        words(
            p,
            "=",
            x0 + 4.6 * s,
            base - 0.4 * s,
            1.6 * sp,
            Align2::LEFT_CENTER,
            col,
        );
        // a quarter and an eighth under a 3
        let q0 = x0 + 6.8 * s;
        note(q0, false);
        note(q0 + 2.6 * s, true);
        sym(
            p,
            TUPLET_3,
            q0 + 1.2 * s,
            base - STEM_LEN * s - 0.6 * s,
            &font,
            col,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_spell_in_their_own_accidentals() {
        // E minor: one sharp (F#); F# is spelled F sharp, natural in the key
        let em = Speller::new(Some((4, Mode::Minor)));
        assert_eq!(em.fifths, 1);
        let (d, a) = em.spell(66); // F#4
        assert_eq!((d, a), (4 * 7 + 3, 1));
        assert_eq!(em.key_alter(3), 1);
        // F major: one flat; A#/Bb is spelled B flat
        let f = Speller::new(Some((5, Mode::Major)));
        assert_eq!(f.fifths, -1);
        assert_eq!(f.spell(70), (4 * 7 + 6, -1));
        assert_eq!(f.key_alter(6), -1);
        // no key: C, sharps
        assert_eq!(Speller::new(None).spell(61), (4 * 7, 1));
    }

    #[test]
    fn chromatic_notes_go_by_their_degree() {
        // E major (4 sharps): the blues b3 is G natural and b7 D natural, not F double-sharp or C#
        let e = Speller::new(Some((4, Mode::Major)));
        assert_eq!(e.fifths, 4);
        assert_eq!(e.spell(67), (4 * 7 + 4, 0)); // G4
        assert_eq!(e.spell(62), (4 * 7 + 1, 0)); // D4
        // C major: Bb and Eb, not A# and D#
        let c = Speller::new(Some((0, Mode::Major)));
        assert_eq!(c.spell(70), (4 * 7 + 6, -1));
        assert_eq!(c.spell(63), (4 * 7 + 2, -1));
        // A minor: the raised 7th is G#, the raised 6th F#
        let am = Speller::new(Some((9, Mode::Minor)));
        assert_eq!(am.spell(68), (4 * 7 + 4, 1));
        assert_eq!(am.spell(66), (4 * 7 + 3, 1));
        // F# major (6 sharps): its 7th is E#, in the octave below F#
        let fs = Speller::new(Some((6, Mode::Major)));
        assert_eq!(fs.spell(65), (4 * 7 + 2, 1));
        // Eb minor is six flats, not six sharps
        assert_eq!(Speller::new(Some((3, Mode::Minor))).fifths, -6);
        // toward the next note: B-Bb-A going down, G-G#-A going up (E major)
        assert_eq!(e.spell_toward(70, Some(69)), (4 * 7 + 6, -1));
        assert_eq!(e.spell_toward(70, Some(71)), (4 * 7 + 5, 1));
    }

    #[test]
    fn guitar_low_e_sits_three_ledger_lines_under_the_treble_staff() {
        // E2 sounds, E3 is written: diatonic 23, the bottom line is 30, ledger lines at 28, 26, 24
        let (d, a) = Speller::new(None).spell(40 + 12);
        assert_eq!((d, a), (23, 0));
        assert_eq!(bottom_line(false) - d, 7);
        // bass low E1 written E2: one ledger line under the bass staff (G2)
        let (d, _) = Speller::new(None).spell(28 + 12);
        assert_eq!(bottom_line(true) - d, 2);
    }

    #[test]
    fn the_key_cycles_tab_both_staff() {
        assert_eq!(Score::Tab.next(), Score::Both);
        assert_eq!(Score::Both.next(), Score::Staff);
        assert_eq!(Score::Staff.next(), Score::Tab);
        for s in [Score::Tab, Score::Staff, Score::Both] {
            assert_eq!(Score::parse(s.key()), s);
        }
    }
}
