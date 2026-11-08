//! Standard notation on the tab page (P44.1, D181; new in quickRS, no Python source): a five-line staff above each
//! row of tab (Both) or in its place (Staff), for sight reading.
//!
//! Everything comes from the chart: a note's pitch is its string's open note (the tuning) plus its fret (charts
//! number frets on the neck, so a capo needs nothing), written an octave up as guitar and bass music is (a treble
//! clef for guitar, a bass clef for bass, each with the small 8 below). The key signature is the part's own key (the
//! one the chord numbers use) and spells the notes in sharps or flats; an accidental lasts to the end of its bar.
//! The rhythm is the tab's own (`Rhythm` pieces: values, dots, ties, triplets, rests), so the staff and the rhythm
//! under the tab always agree. Symbols are Bravura's (SMuFL): its em is four staff spaces, a glyph's baseline is the
//! staff position it sits on.

use std::collections::HashMap;

use eframe::egui;
use egui::{Color32, FontId, Painter, Shape, Stroke, pos2};
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
const F_CLEF_8VB: char = '\u{E064}';
const HEAD_WHOLE: char = '\u{E0A2}';
const HEAD_HALF: char = '\u{E0A3}';
const HEAD_BLACK: char = '\u{E0A4}';
const HEAD_X: char = '\u{E0A9}';
const FLAT: char = '\u{E260}';
const NATURAL: char = '\u{E261}';
const SHARP: char = '\u{E262}';
const DOT: char = '\u{E1E7}';
const TUPLET_3: char = '\u{E883}';
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

/// The order sharps and flats are added in a key signature, as diatonic steps (C = 0 .. B = 6).
const SHARP_ORDER: [i32; 7] = [3, 0, 4, 1, 5, 2, 6];
const FLAT_ORDER: [i32; 7] = [6, 2, 5, 1, 4, 0, 3];
/// Where each of them sits on a treble staff (diatonic numbers, octave * 7 + step); a bass staff's are 14 lower.
const SHARP_AT: [i32; 7] = [38, 35, 39, 36, 33, 37, 34];
const FLAT_AT: [i32; 7] = [34, 37, 33, 36, 32, 35, 31];

/// Spells pitches in a key: sharps or flats by the key signature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speller {
    /// Sharps (positive) or flats (negative) in the key signature.
    pub fifths: i32,
}

impl Speller {
    pub fn new(key: Option<Key>) -> Speller {
        let Some((tonic, mode)) = key else {
            return Speller { fifths: 0 };
        };
        // a minor key shares its relative major's signature
        let major = if mode == Mode::Minor {
            (tonic + 3) % 12
        } else {
            tonic % 12
        };
        // C G D A E B F# (sharps), F Bb Eb Ab Db (flats)
        const FIFTHS: [i32; 12] = [0, -5, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5];
        Speller {
            fifths: FIFTHS[major as usize],
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

    /// A MIDI pitch as (diatonic number = octave * 7 + step, alteration).
    pub fn spell(&self, midi: i32) -> (i32, i32) {
        const SHARPS: [(i32, i32); 12] = [
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
            (2, 0),
            (3, 0),
            (3, 1),
            (4, 0),
            (4, 1),
            (5, 0),
            (5, 1),
            (6, 0),
        ];
        const FLATS: [(i32, i32); 12] = [
            (0, 0),
            (1, -1),
            (1, 0),
            (2, -1),
            (2, 0),
            (3, 0),
            (4, -1),
            (4, 0),
            (5, -1),
            (5, 0),
            (6, -1),
            (6, 0),
        ];
        let pc = midi.rem_euclid(12) as usize;
        let (step, alter) = if self.fifths < 0 { FLATS[pc] } else { SHARPS[pc] };
        let octave = midi.div_euclid(12) - 1;
        (octave * 7 + step, alter)
    }
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

/// The written diatonic range of a chart's notes (lowest, highest), for the room the staff needs.
pub fn range(chart: &Chart, sp: &Speller) -> Option<(i32, i32)> {
    let mut lo_hi: Option<(i32, i32)> = None;
    for g in &chart.glyphs {
        let (d, _) = sp.spell(sounding(chart, g.string, g.fret, g.tech) + 12);
        lo_hi = Some(lo_hi.map_or((d, d), |(a, b)| (a.min(d), b.max(d))));
    }
    lo_hi
}

/// A notehead in a chord.
struct Head {
    d: i32,
    /// The accidental drawn before it (an alteration), if one is needed.
    acc: Option<i32>,
    x_head: bool,
    /// Drawn on the stem's other side (a second in a chord).
    flip: bool,
    col: Color32,
}

/// One rhythm piece on the staff.
struct Event<'a> {
    pc: &'a Piece,
    x: f64,
    heads: Vec<Head>,
    up: bool,
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
        let sp = (self.line_gap * 0.42).max(7.0 * s);
        self.sp = sp;
        let speller = Speller::new(chart.key);
        let b = bottom_line(chart.arr.is_bass);
        let (lo, hi) = range(chart, &speller).unwrap_or((b, b + 8));
        // ledger lines and stems need room: stems go away from the middle, so a note far out has its stem inwards
        let above = ((hi - (b + 8)).max(0) as f64 / 2.0 + 1.5) * sp;
        let below = ((b - lo).max(0) as f64 / 2.0 + 1.5) * sp;
        self.stave_above = above.max(2.0 * sp);
        let gap = if c.score.tab() { ipx(14.0) } else { 0.0 };
        self.stave_h = self.stave_above + 4.0 * sp + below.max(2.5 * sp) + gap;
        let n_acc = speller.fifths.unsigned_abs() as f64;
        // clef, key signature, time signature
        self.clef_w = (3.4 + n_acc * 1.0 + 3.4) * sp;
    }

    /// The staff of one row: lines, clef and key signature, bar lines, notes, rests, beams and ties. `top` is the
    /// stave band's top (pixels).
    pub(super) fn draw_staff(&self, p: &Painter, c: &TabCtx, top: f64, row: &[usize], first_row: bool) {
        let chart = c.chart;
        let th = t();
        let sp = self.sp;
        let bass = chart.arr.is_bass;
        let b = bottom_line(bass);
        let speller = Speller::new(chart.key);
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
        // clef, key signature (every row), time signature (the first row)
        let ink = th.text_2;
        let mut x = x_start + 0.6 * sp;
        if bass {
            sym(p, F_CLEF_8VB, x, y_of(b + 6), &font, ink);
        } else {
            sym(p, G_CLEF_8VB, x, y_of(b + 2), &font, ink);
        }
        x += 3.2 * sp;
        let shift = if bass { -14 } else { 0 };
        let n = speller.fifths.unsigned_abs() as usize;
        for i in 0..n {
            let (ch, d) = if speller.fifths > 0 {
                (SHARP, SHARP_AT[i] + shift)
            } else {
                (FLAT, FLAT_AT[i] + shift)
            };
            sym(p, ch, x, y_of(d), &font, ink);
            x += 1.0 * sp;
        }
        if first_row && let Some(m) = self.measures.get(row[0]) {
            let beats = m.beats.clamp(1, 99);
            let x_t = x + 0.6 * sp;
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

        let Some(rh) = &chart.rhythm else { return };
        let (r_start, r_end) = (self.measures[row[0]].start, last.end);
        let times = &rh.piece_times;
        let (i0, i1) = (
            lower_bound(times, r_start - 1e-4),
            lower_bound(times, r_end - 1e-4),
        );
        let pieces = &rh.pieces[i0..i1.max(i0)];
        let mid = b + 4;
        // the bar's accidentals so far: diatonic number -> alteration in force; reset at each bar line
        let mut in_force: HashMap<i32, i32> = HashMap::new();
        let mut bar_of_last: Option<usize> = None;
        let mut prev_heads: Option<(f64, Vec<i32>, bool)> = None;
        let mut events: Vec<Event> = Vec::new();
        let mut ties: Vec<(f64, f64, Vec<i32>, bool)> = Vec::new();
        for pc in pieces {
            let bar = self.measure_index(pc.t);
            if bar_of_last != Some(bar) {
                in_force.clear();
                bar_of_last = Some(bar);
            }
            let x = match pc.onset {
                Some(o) if !c.snap => self.x_of(o, None),
                _ => self.x_of(pc.t, None),
            };
            if pc.rest {
                events.push(Event {
                    pc,
                    x,
                    heads: Vec::new(),
                    up: true,
                });
                continue;
            }
            // the notes this piece starts (or, tied, the ones it holds on)
            let mut heads: Vec<Head> = Vec::new();
            if let Some(o) = pc.onset {
                let g0 = lower_bound(&chart.gtimes, o - 1e-3);
                let g1 = lower_bound(&chart.gtimes, o + 1e-3);
                for g in &chart.glyphs[g0..g1.max(g0)] {
                    let midi = sounding(chart, g.string, g.fret, g.tech) + 12;
                    let (d, alter) = speller.spell(midi);
                    if heads.iter().any(|h| h.d == d) {
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
                    let acc = (force != alter).then_some(alter);
                    in_force.insert(d, alter);
                    heads.push(Head {
                        d,
                        acc,
                        x_head: g.tech.has(Techs::FRET_HAND_MUTE),
                        flip: false,
                        col,
                    });
                }
            } else if pc.tied
                && let Some((px, ds, _)) = &prev_heads
            {
                let col = if pc.t < c.now { th.caption } else { th.text };
                for &d in ds {
                    heads.push(Head {
                        d,
                        acc: None,
                        x_head: false,
                        flip: false,
                        col,
                    });
                }
                ties.push((*px, x, ds.clone(), false));
            }
            if heads.is_empty() {
                continue;
            }
            heads.sort_by_key(|h| h.d);
            // stems away from the middle line: the note farthest from it decides
            let (lo, hi) = (heads[0].d, heads[heads.len() - 1].d);
            let up = (mid - lo) >= (hi - mid);
            prev_heads = Some((x, heads.iter().map(|h| h.d).collect(), up));
            events.push(Event { pc, x, heads, up });
        }
        // beams: runs of eighths or shorter in the same beat (a rest breaks a run), one stem direction each
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
        }
        let mut in_run: HashMap<usize, (usize, f64)> = HashMap::new(); // event -> (run, beam y)
        for (ri, run) in runs.iter().enumerate() {
            let sum: i32 = run
                .iter()
                .map(|&k| {
                    let h = &events[k].heads;
                    (h[0].d + h[h.len() - 1].d) - 2 * mid
                })
                .sum();
            let up = sum <= 0;
            for &k in run {
                events[k].up = up;
            }
            let y = if up {
                let top_head = run
                    .iter()
                    .map(|&k| y_of(events[k].heads.last().unwrap().d))
                    .fold(f64::MAX, f64::min);
                (top_head - STEM_LEN * sp).min(y_of(mid))
            } else {
                let low_head = run
                    .iter()
                    .map(|&k| y_of(events[k].heads[0].d))
                    .fold(f64::MIN, f64::max);
                (low_head + STEM_LEN * sp).max(y_of(mid))
            };
            for &k in run {
                in_run.insert(k, (ri, y));
            }
        }
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
        for (k, e) in events.iter().enumerate() {
            let pc = e.pc;
            if pc.rest {
                let fl = (pc.flags + 2).clamp(0, 5) as usize;
                let d = match pc.flags {
                    -2 => b + 6,
                    _ => mid,
                };
                let col = if pc.t < c.now { th.caption } else { th.text_2 };
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
                // ledger lines out to the note
                let lx = (x - 0.4 * sp, x + (hw + 0.4) * sp);
                let mut l = b - 2;
                while l >= h.d {
                    line(p, (lx.0, y_of(l)), (lx.1, y_of(l)), lw * 1.3, th.line);
                    l -= 2;
                }
                let mut l = b + 10;
                while l <= h.d {
                    line(p, (lx.0, y_of(l)), (lx.1, y_of(l)), lw * 1.3, th.line);
                    l += 2;
                }
                sym(p, if h.x_head { HEAD_X } else { head }, x, y, &font, h.col);
                if pc.dots > 0 {
                    let dy = if (h.d - b).rem_euclid(2) == 0 { 1 } else { 0 };
                    sym(p, DOT, hx + (hw + 0.4) * sp, y_of(h.d + dy), &font, h.col);
                }
            }
            if pc.flags == -2 {
                continue;
            }
            let col = e.heads[0].col;
            let (far, near) = if e.up {
                (y_of(e.heads.last().unwrap().d), y_of(e.heads[0].d))
            } else {
                (y_of(e.heads[0].d), y_of(e.heads.last().unwrap().d))
            };
            let end = match in_run.get(&k) {
                Some(&(_, y)) => y,
                None if e.up => (far - STEM_LEN * sp).min(y_of(mid)),
                None => (far + STEM_LEN * sp).max(y_of(mid)),
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
                    let (a, bx) = if x2 >= x { (x, x2) } else { (x2, x) };
                    let (y0, y1) = if up { (y, y + thick) } else { (y - thick, y) };
                    p.rect_filled(
                        egui::Rect::from_min_max(pos2(pt(a), pt(y0)), pos2(pt(bx), pt(y1))),
                        0.0,
                        col,
                    );
                }
            }
        }
        // triplets: a 3 beyond the beam (or the notes) of each beat written in triplets
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
                let up = events[k].up;
                let ends: Vec<f64> = (k..=j)
                    .map(|i| {
                        if events[i].pc.rest {
                            y_of(mid)
                        } else {
                            stem_ends[i].1
                        }
                    })
                    .collect();
                let y = if up {
                    ends.iter().copied().fold(f64::MAX, f64::min) - 1.2 * sp
                } else {
                    ends.iter().copied().fold(f64::MIN, f64::max) + 2.2 * sp
                };
                let xm = (events[k].x + events[j].x) / 2.0;
                sym(p, TUPLET_3, xm - 0.45 * sp, y, &font, th.text_2);
                k = j + 1;
            } else {
                k += 1;
            }
        }
        // ties: an arc from each held note to its continuation, away from the stems
        for (x1, x2, ds, _) in ties {
            for d in ds {
                let up_side = d >= mid;
                let y = y_of(d) + if up_side { -0.6 * sp } else { 0.6 * sp };
                let (a, bx) = (x1 + 0.7 * HEAD_W * sp, x2 - 0.2 * HEAD_W * sp);
                if bx <= a {
                    continue;
                }
                let bulge = if up_side { -0.7 * sp } else { 0.7 * sp };
                let pts: Vec<egui::Pos2> = (0..=12)
                    .map(|i| {
                        let u = i as f64 / 12.0;
                        pos2(pt(a + (bx - a) * u), pt(y + bulge * 4.0 * u * (1.0 - u)))
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new(pt(0.16 * sp).max(1.0), th.text_2)));
            }
        }
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
