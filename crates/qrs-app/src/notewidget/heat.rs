//! The note widget's heat strip (P25, new in quickRS; D65, D68), in the drawer under the chord strip: two necks, all
//! 24 frets, where the notes just played are. Each note lights up where it's played and fades over `FADE` seconds,
//! from a hot red through ultraviolet to a deep blue, so a fast lick lights a trail across the neck that fades in the
//! order it was played. TAB: the open song's chart, every note and chord note as it passes the now-line (paused, the
//! trail holds still). YOU: always on, song or not, a best guess from the sound: a single note goes where the hand
//! most likely is (the string whose fret is inside or nearest a four-fret hand, which moves to follow), a chord as the
//! chord strip's shape. The right-click menu's "Forget" clears both.

use std::collections::VecDeque;

use eframe::egui;
use egui::{Align2, Color32, Pos2, Rect, Stroke, pos2};
use qrs_formats::model::Arrangement;
use qrs_ui::raster::mix;
use qrs_ui::text::Style;

use super::chords::DRAWER_H;
use super::pil_text;
use super::{DIM, FG, FRET, FRET_NUMBERS, H, INLAY, NUT, NUT_W, PAD, STRING, SUB};

/// The strip's height (points, under the chord strip).
pub const HEAT_H: f32 = 200.0;
/// How long a note glows after it's played (seconds of playing), from full to nothing.
pub const FADE: f64 = 2.5;
/// A rise in the input's level this many times over is a pluck (a note played again on the same fret).
const PLUCK: f32 = 1.8;

/// The heat strip's box in the drawer, in the widget's points, for a window `w` wide.
pub fn strip_rect(w: f32) -> Rect {
    Rect::from_min_max(
        pos2(PAD, H + DRAWER_H - 4.0),
        pos2(w - PAD, H + DRAWER_H + HEAT_H - PAD),
    )
}

/// A neck's board in the strip: 0 the tab's, 1 yours; right of its name, over its fret numbers.
pub fn board_rect(w: f32, row: usize) -> Rect {
    let b = strip_rect(w);
    let y = b.min.y + 30.0 + row as f32 * 82.0;
    Rect::from_min_max(pos2(b.min.x + 140.0, y), pos2(b.max.x - 14.0, y + 60.0))
}

/// Where notes landed in the last `FADE` seconds: (when, string, fret), oldest first.
#[derive(Default)]
pub struct Heat {
    hits: VecDeque<(f64, usize, i32)>,
}

impl Heat {
    pub fn add(&mut self, t: f64, string: usize, fret: i32) {
        if (0..=24).contains(&fret) {
            self.hits.push_back((t, string, fret));
        }
    }

    /// Forget what has faded out by `t`.
    pub fn keep(&mut self, t: f64) {
        while self.hits.front().is_some_and(|h| h.0 <= t - FADE) {
            self.hits.pop_front();
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.hits.len()
    }

    /// How brightly each spot glows at `now` (1 just played, fading to 0 over `FADE`; a spot played again glows
    /// from its latest), per string (`n` of them, low first) and fret (0 the open string to 24).
    pub fn grid(&self, n: usize, now: f64) -> Vec<[f32; 25]> {
        let mut g = vec![[0.0f32; 25]; n];
        for &(t, s, f) in &self.hits {
            let glow = (1.0 - (now - t).max(0.0) / FADE) as f32;
            if s < n && glow > 0.0 {
                let c = &mut g[s][f as usize];
                *c = c.max(glow);
            }
        }
        g
    }

    /// The notes still glowing at `now`.
    pub fn lit(&self, now: f64) -> usize {
        self.hits.iter().filter(|h| now - h.0 < FADE).count()
    }

    pub fn clear(&mut self) {
        self.hits.clear();
    }
}

/// Where a note `midi` is most likely played, on `strings` (open MIDI, low first), with the hand's index finger at
/// fret `hand` (it covers `hand` to `hand + 3`): the fret inside the hand, else the nearest one outside it (an open
/// string free near the nut, dearer up the neck), the lower fret on a tie. The hand moves to cover a fretted note.
pub fn place(midi: i32, strings: &[i32], hand: &mut i32) -> Option<(usize, i32)> {
    let cost = |f: i32| -> f32 {
        if f == 0 {
            if *hand <= 4 { 0.0 } else { 2.5 }
        } else {
            (*hand - f).max(0) as f32 + (f - (*hand + 3)).max(0) as f32
        }
    };
    let (s, f) = strings
        .iter()
        .enumerate()
        .map(|(s, &open)| (s, midi - open))
        .filter(|&(_, f)| (0..=24).contains(&f))
        .min_by(|a, b| (cost(a.1) + 0.01 * a.1 as f32).total_cmp(&(cost(b.1) + 0.01 * b.1 as f32)))?;
    if f > 0 {
        if f < *hand {
            *hand = f;
        } else if f > *hand + 3 {
            *hand = f - 3;
        }
    }
    Some((s, f))
}

/// The heat strip's state: the two maps and what they're fed from.
pub struct HeatStrip {
    pub tab: Heat,
    pub you: Heat,
    /// Seconds of the song played (the tab's clock: it stops when the song does), and the chart time last seen.
    played: f64,
    /// The time of the last listen (your notes' clock).
    heard: f64,
    last_pos: Option<f64>,
    /// The song: none open, paused or playing.
    song: Option<bool>,
    /// The hand's index finger (fret) for single notes.
    hand: i32,
    /// The note held at the last listen, the input's level then, and the chord shape last counted.
    last_note: Option<i32>,
    last_level: f32,
    last_shape: Option<Vec<i32>>,
}

impl Default for HeatStrip {
    fn default() -> Self {
        HeatStrip {
            tab: Heat::default(),
            you: Heat::default(),
            played: 0.0,
            heard: 0.0,
            last_pos: None,
            song: None,
            hand: 1,
            last_note: None,
            last_level: 0.0,
            last_shape: None,
        }
    }
}

impl HeatStrip {
    /// Every listen: the chart's notes that passed the now-line since the last one (`song` is the open part, the
    /// chart time at the now-line and whether it's playing; `dt` the seconds since the last listen). A jump (a seek,
    /// a loop back) adds nothing.
    pub fn song(&mut self, song: Option<(&Arrangement, f64, bool)>, dt: f64, n: usize) {
        let Some((arr, now, playing)) = song else {
            self.song = None;
            self.last_pos = None;
            return;
        };
        self.song = Some(playing);
        if playing {
            self.played += dt;
            if let Some(from) = self.last_pos
                && now > from
                && now - from < 1.0
            {
                let t = self.played;
                let passed = |time: f64| time > from && time <= now;
                for note in arr.notes.iter().filter(|x| passed(x.time)) {
                    self.add_tab(t, note.string, note.fret, n);
                }
                for c in arr.chords.iter().filter(|x| passed(x.time)) {
                    if c.notes.is_empty() {
                        for (s, &f) in c.frets.iter().enumerate() {
                            self.add_tab(t, s as i32, f, n);
                        }
                    } else {
                        for note in &c.notes {
                            self.add_tab(t, note.string, note.fret, n);
                        }
                    }
                }
            }
        }
        self.last_pos = Some(now);
        self.tab.keep(self.played);
    }

    fn add_tab(&mut self, t: f64, s: i32, f: i32, n: usize) {
        if s >= 0 && (s as usize) < n && f >= 0 {
            self.tab.add(t, s as usize, f);
        }
    }

    /// Every listen, at `t` seconds: what you played, from the sound. `note` is the note sounding now (None while
    /// it's unclear or quiet), `held` the note still shown, `chord` the chord strip's shape while a chord of three
    /// or more notes rings, `level` the input's level.
    pub fn listen(
        &mut self,
        t: f64,
        note: Option<i32>,
        held: Option<i32>,
        chord: Option<&[i32]>,
        level: f32,
        strings: &[i32],
    ) {
        self.heard = t;
        let pluck = level > 0.01 && level > self.last_level * PLUCK;
        self.last_level = level;
        if let Some(shape) = chord {
            if pluck || self.last_shape.as_deref() != Some(shape) {
                for (s, &f) in shape.iter().enumerate().take(strings.len()) {
                    self.you.add(t, s, f);
                }
                if let Some(&lo) = shape.iter().filter(|&&f| f > 0).min() {
                    self.hand = lo;
                }
                self.last_shape = Some(shape.to_vec());
            }
        } else if let Some(m) = note
            && (pluck || self.last_note != Some(m))
            && let Some((s, f)) = place(m, strings, &mut self.hand)
        {
            self.you.add(t, s, f);
        }
        if chord.is_none() {
            self.last_shape = None;
        }
        self.last_note = held;
        self.you.keep(t);
    }

    pub fn reset(&mut self) {
        self.tab.clear();
        self.you.clear();
        self.hand = 1;
        self.last_shape = None;
    }

    /// Screenshot mode: a run up an A minor pentatonic box at the fifth fret, the last notes just played (the first
    /// ones fading), and you a little behind it.
    pub fn stage(&mut self, strings: &[i32]) {
        self.reset();
        self.song = Some(true);
        (self.played, self.heard) = (FADE, FADE);
        let boxed = [
            (0, 5),
            (0, 8),
            (1, 5),
            (1, 7),
            (2, 5),
            (2, 7),
            (3, 5),
            (3, 7),
            (4, 5),
            (4, 8),
            (5, 5),
            (5, 8),
        ];
        for (i, &(s, f)) in boxed.iter().enumerate() {
            let t = FADE * (i as f64 + 1.0) / boxed.len() as f64;
            self.tab.add(t, s, f);
            if i + 2 < boxed.len() {
                self.you.add(t + 0.2, s.min(strings.len() - 1), f);
            }
        }
    }

    /// The strip over its sunken box (the background has it): each neck's name, how many notes, and its map.
    pub fn draw(&self, p: &egui::Painter, u: f32, at: impl Fn(f32, f32) -> Pos2, w: f32, strings: &[i32]) {
        let bx = strip_rect(w);
        let st = |size: f32, weight: u16, color: Color32| Style::new(size * u, weight, color);
        pil_text(
            p,
            "fading over a few seconds",
            at(bx.max.x - 14.0, bx.min.y + 8.0),
            Align2::RIGHT_TOP,
            st(11.0, 500, SUB),
        );
        let tab_says = match self.song {
            None => "no song open",
            Some(false) => "paused",
            Some(true) => "from the tab",
        };
        let rows = [
            ("TAB", &self.tab, self.played, tab_says),
            ("YOU", &self.you, self.heard, "best guess, by ear"),
        ];
        for (row, (name, heat, now, says)) in rows.into_iter().enumerate() {
            let board = board_rect(w, row);
            let x = bx.min.x + 14.0;
            pil_text(
                p,
                name,
                at(x, board.min.y + 2.0),
                Align2::LEFT_TOP,
                st(18.0, 800, FG),
            );
            let n = heat.lit(now);
            pil_text(
                p,
                &format!("{n} note{} lit", if n == 1 { "" } else { "s" }),
                at(x, board.min.y + 28.0),
                Align2::LEFT_TOP,
                st(10.0, 600, SUB),
            );
            pil_text(
                p,
                says,
                at(x, board.min.y + 43.0),
                Align2::LEFT_TOP,
                st(9.0, 500, DIM),
            );
            draw_map(p, u, &at, board, strings, &heat.grid(strings.len(), now));
        }
    }
}

/// A note's glow from faded to just played (`t` 0 to 1): a deep blue, ultraviolet, a hot red.
pub fn heat_color(t: f32) -> Color32 {
    const STOPS: [(f32, Color32); 4] = [
        (0.0, Color32::from_rgb(0x1e, 0x3a, 0x9a)),
        (0.4, Color32::from_rgb(0x5a, 0x3c, 0xe6)),
        (0.7, Color32::from_rgb(0xa8, 0x48, 0xf0)),
        (1.0, Color32::from_rgb(0xff, 0x4a, 0x6a)),
    ];
    let t = t.clamp(0.0, 1.0);
    let i = STOPS
        .iter()
        .rposition(|s| s.0 <= t)
        .unwrap_or(0)
        .min(STOPS.len() - 2);
    let (a, b) = (STOPS[i], STOPS[i + 1]);
    mix(a.1, b.1, (t - a.0) / (b.0 - a.0))
}

/// A neck, nut to fret 24 (as the scale box lays it out, low string at the bottom), each spot played a cell in its
/// glow's colour, fading out (and shrinking a little) as the glow does; the open strings in the column left of the
/// nut.
fn draw_map(
    p: &egui::Painter,
    u: f32,
    at: &impl Fn(f32, f32) -> Pos2,
    board: Rect,
    strings: &[i32],
    grid: &[[f32; 25]],
) {
    let n = strings.len().max(1);
    let x0 = board.min.x + NUT_W;
    let fw = (board.width() - NUT_W - 2.0) / 24.0;
    let m = 6.0;
    let gap = (board.height() - 2.0 * m) / (n - 1).max(1) as f32;
    let sy = |s: usize| board.min.y + m + (n - 1 - s) as f32 * gap;
    let fx = |f: i32| {
        if f == 0 {
            board.min.x + NUT_W / 2.0
        } else {
            x0 + (f as f32 - 0.5) * fw
        }
    };
    let mid = (board.min.y + board.max.y) / 2.0;
    for f in FRET_NUMBERS {
        let dots: &[f32] = if f % 12 == 0 { &[-1.0, 1.0] } else { &[0.0] };
        for d in dots {
            p.circle_filled(at(fx(f), mid + d * gap), 2.0 * u, INLAY);
        }
        pil_text(
            p,
            &f.to_string(),
            at(fx(f), board.max.y + 2.0),
            Align2::CENTER_TOP,
            Style::new(8.0 * u, 700, DIM),
        );
    }
    for f in 1..=24 {
        let x = x0 + f as f32 * fw;
        p.line_segment(
            [at(x, board.min.y + 3.0), at(x, board.max.y - 3.0)],
            Stroke::new(u, FRET),
        );
    }
    p.rect_filled(
        Rect::from_min_max(at(x0 - 3.0, board.min.y + 3.0), at(x0, board.max.y - 3.0)),
        u,
        NUT,
    );
    for s in 0..n {
        let thick = [1.6, 1.4, 1.2, 1.0, 0.9, 0.8][s.min(5)];
        p.line_segment(
            [at(x0, sy(s)), at(board.max.x - 3.0, sy(s))],
            Stroke::new(thick * u, STRING),
        );
    }
    let (cw, ch) = (fw * 0.84, (gap * 0.8).min(11.0));
    for (s, row) in grid.iter().enumerate().take(n) {
        for (f, &t) in row.iter().enumerate() {
            if t <= 0.0 {
                continue;
            }
            let c = pos2(fx(f as i32), sy(s));
            let r = Rect::from_center_size(at(c.x, c.y), egui::vec2(cw, ch) * u);
            let r = r.shrink((1.0 - t) * 1.5 * u);
            p.rect_filled(r, 3.0 * u, heat_color(t).gamma_multiply(0.2 + 0.8 * t));
            // just played: a bright edge
            if t > 0.85 {
                let edge = Color32::from_rgb(0xff, 0xd0, 0xd8).gamma_multiply((t - 0.85) / 0.15);
                p.rect_stroke(r, 3.0 * u, Stroke::new(u, edge), egui::StrokeKind::Inside);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_formats::model::{Chord, GUITAR_OPEN_MIDI, Note};

    const E: [i32; 6] = GUITAR_OPEN_MIDI;

    #[test]
    fn a_note_goes_where_the_hand_is() {
        let mut hand = 1;
        // open position: E4 is the open top string, A3 the G string's second fret
        assert_eq!(place(64, &E, &mut hand), Some((5, 0)));
        assert_eq!(place(57, &E, &mut hand), Some((3, 2)));
        // the hand at the fifth fret: A3 on the D string's seventh, and E4 fretted, not open
        hand = 5;
        assert_eq!(place(57, &E, &mut hand), Some((2, 7)));
        assert_eq!(place(64, &E, &mut hand), Some((4, 5)));
        // a note out of reach moves the hand to it
        hand = 1;
        assert_eq!(place(69, &E, &mut hand), Some((5, 5)));
        assert_eq!(hand, 2);
        assert_eq!(place(20, &E, &mut hand), None, "below the neck");
    }

    #[test]
    fn a_note_glows_then_fades() {
        let mut h = Heat::default();
        h.add(0.0, 0, 3);
        h.add(1.0, 5, 24);
        h.add(1.0, 5, 25); // off the neck
        let g = h.grid(6, 1.0);
        assert_eq!(g[5][24], 1.0, "just played: full");
        assert!(
            (g[0][3] - (1.0 - 1.0 / FADE as f32)).abs() < 1e-5,
            "a second on: fading"
        );
        assert_eq!(h.lit(1.0), 2);
        // played again: glows from the new one
        h.add(2.0, 0, 3);
        assert_eq!(h.grid(6, 2.0)[0][3], 1.0);
        // faded out
        let later = 1.0 + FADE;
        assert_eq!(h.grid(6, later)[5][24], 0.0);
        h.keep(later);
        assert_eq!(h.len(), 1);
        assert_eq!(h.lit(2.0 + FADE), 0);
    }

    #[test]
    fn the_tab_counts_what_passes_while_playing() {
        let mut arr = Arrangement {
            notes: vec![Note::new(1.0, 0, 3), Note::new(2.0, 1, 5), Note::new(9.0, 2, 7)],
            ..Default::default()
        };
        let mut ch = Chord {
            time: 1.5,
            chord_id: 0,
            name: "C".into(),
            notes: Vec::new(),
            techniques: Default::default(),
            sustain: 0.0,
            frets: vec![-1, 3, 2, 0, 1, 0],
            fingers: Vec::new(),
            high_density: false,
        };
        arr.chords = vec![ch.clone()];
        let mut hs = HeatStrip::default();
        hs.song(Some((&arr, 0.5, true)), 0.05, 6);
        hs.song(Some((&arr, 1.2, true)), 0.05, 6);
        hs.song(Some((&arr, 2.0, true)), 0.05, 6);
        assert_eq!(hs.tab.len(), 2 + 5, "two notes and the chord's five strings");
        // paused: nothing; a seek ahead: nothing; then on from there
        hs.song(Some((&arr, 2.0, false)), 0.05, 6);
        hs.song(Some((&arr, 8.9, true)), 0.05, 6);
        hs.song(Some((&arr, 9.1, true)), 0.05, 6);
        assert_eq!(hs.tab.len(), 8);
        assert!(hs.tab.grid(6, hs.played)[2][7] > 0.9, "just passed: lit");
        // a chord with its notes listed counts those
        ch.time = 9.2;
        ch.notes = vec![Note::new(9.2, 4, 1)];
        arr.chords = vec![ch];
        hs.song(Some((&arr, 9.3, true)), 0.05, 6);
        assert_eq!(hs.tab.len(), 9);
        // paused, the trail holds; a few seconds of playing on, it's all gone
        let before = hs.tab.len();
        for _ in 0..100 {
            hs.song(Some((&arr, 9.3, false)), 0.05, 6);
        }
        assert_eq!(hs.tab.len(), before);
        for _ in 0..60 {
            hs.song(Some((&arr, 9.3, true)), 0.05, 6);
        }
        assert_eq!(hs.tab.len(), 0);
    }

    #[test]
    fn you_count_each_note_and_chord_once() {
        let mut hs = HeatStrip::default();
        // A3 held for three listens: once
        for _ in 0..3 {
            hs.listen(0.0, Some(57), Some(57), None, 0.05, &E);
        }
        assert_eq!(hs.you.len(), 1);
        // played again (the level jumps): twice
        hs.listen(0.1, None, Some(57), None, 0.005, &E);
        hs.listen(0.2, Some(57), Some(57), None, 0.05, &E);
        assert_eq!(hs.you.len(), 2);
        // a C chord ringing: its five strings once, again when strummed again
        let c = [-1, 3, 2, 0, 1, 0];
        hs.listen(0.3, None, None, Some(&c), 0.05, &E);
        hs.listen(0.4, None, None, Some(&c), 0.05, &E);
        assert_eq!(hs.you.len(), 7);
        hs.listen(0.5, None, None, Some(&c), 0.01, &E);
        hs.listen(0.6, None, None, Some(&c), 0.08, &E);
        assert_eq!(hs.you.len(), 12);
    }

    #[test]
    fn the_glow_runs_blue_to_red() {
        assert_eq!(heat_color(0.0), Color32::from_rgb(0x1e, 0x3a, 0x9a));
        assert_eq!(heat_color(1.0), Color32::from_rgb(0xff, 0x4a, 0x6a));
        // redder as it's fresher, bluest when faded
        let red = |c: Color32| c.r() as i32 - c.b() as i32;
        assert!(red(heat_color(0.1)) < red(heat_color(0.6)) && red(heat_color(0.6)) < red(heat_color(1.0)));
    }

    #[test]
    fn the_strip_fits_under_the_chords() {
        let w = super::super::width(false, true);
        let s = strip_rect(w);
        assert!(s.min.y > super::super::chords::strip_rect(w).max.y);
        assert!(s.max.y < H + DRAWER_H + HEAT_H);
        let (a, b) = (board_rect(w, 0), board_rect(w, 1));
        assert!(a.max.y + 14.0 < b.min.y, "room for the first neck's numbers");
        assert!(b.max.y + 12.0 < s.max.y, "and the second's");
    }
}
