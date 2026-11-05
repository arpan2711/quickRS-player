//! The Scales page: a scale and one of its boxes (positions) on the neck, and the guitar heard against it. Pick a
//! key, a scale, a box and a tuning; each note of the box lights up when you play it (Free), or the box is played
//! in order, up and back down, with the next note marked (Up & down). New code (not in the Python app).
//!
//! Boxes are worked out from the scale, not tabled: box n starts on the low string at the scale's n-th degree and
//! takes the scale upwards a fixed number of notes per string (two for a pentatonic, which gives the five usual
//! pentatonic boxes, three for a seven-note scale, the three-notes-per-string positions). Blues is the minor
//! pentatonic's box with the ♭5s that fall inside it.

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, Event, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use qrs_ui::icons::Icon;
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{ButtonStyle, button, interact};

use crate::library::{LibView, View, page_rects, txt};
use crate::listen::Listen;
use crate::state::AppState;

const NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
/// Semitones above the root, named as degrees.
const DEGREES: [&str; 12] = ["1", "♭2", "2", "♭3", "3", "4", "♭5", "5", "♭6", "6", "♭7", "7"];
/// The frets drawn (the nut's open strings, then 1 to 22).
const FRETS: i32 = 22;
const INLAYS: [i32; 9] = [3, 5, 7, 9, 12, 15, 17, 19, 21];
/// A wrong note shows this long.
const WRONG_FOR: Duration = Duration::from_millis(1200);
/// The pitch must sit on one note this long before it counts as played.
const STEADY: Duration = Duration::from_millis(90);
/// Quieter than this (dBFS) and the next note counts again even if it's the same one (a re-pluck).
const QUIET_DB: f32 = -48.0;

/// A key's cap on a button, for the keyboard; none on Android (a touchscreen, and the room's needed for the label).
fn cap(k: &'static str) -> &'static str {
    if cfg!(target_os = "android") { "" } else { k }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScaleKind {
    Major,
    Minor,
    MajorPent,
    MinorPent,
    Blues,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    HarmonicMinor,
}

const KINDS: [ScaleKind; 11] = [
    ScaleKind::MinorPent,
    ScaleKind::MajorPent,
    ScaleKind::Blues,
    ScaleKind::Major,
    ScaleKind::Minor,
    ScaleKind::Dorian,
    ScaleKind::Mixolydian,
    ScaleKind::Phrygian,
    ScaleKind::Lydian,
    ScaleKind::Locrian,
    ScaleKind::HarmonicMinor,
];

impl ScaleKind {
    /// The scale's notes, semitones above the root.
    pub fn steps(self) -> &'static [i32] {
        match self {
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::Minor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::MajorPent => &[0, 2, 4, 7, 9],
            ScaleKind::MinorPent => &[0, 3, 5, 7, 10],
            ScaleKind::Blues => &[0, 3, 5, 6, 7, 10],
            ScaleKind::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleKind::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            ScaleKind::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            ScaleKind::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleKind::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            ScaleKind::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
        }
    }

    /// The scale its boxes come from (Blues: the minor pentatonic's, with the ♭5 added).
    fn box_steps(self) -> &'static [i32] {
        match self {
            ScaleKind::Blues => ScaleKind::MinorPent.steps(),
            k => k.steps(),
        }
    }

    pub fn boxes(self) -> usize {
        self.box_steps().len()
    }

    pub fn label(self) -> &'static str {
        match self {
            ScaleKind::Major => "Major",
            ScaleKind::Minor => "Minor",
            ScaleKind::MajorPent => "Maj pent",
            ScaleKind::MinorPent => "Min pent",
            ScaleKind::Blues => "Blues",
            ScaleKind::Dorian => "Dorian",
            ScaleKind::Phrygian => "Phrygian",
            ScaleKind::Lydian => "Lydian",
            ScaleKind::Mixolydian => "Mixolyd.",
            ScaleKind::Locrian => "Locrian",
            ScaleKind::HarmonicMinor => "Harm min",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ScaleKind::Major => "major",
            ScaleKind::Minor => "natural minor",
            ScaleKind::MajorPent => "major pentatonic",
            ScaleKind::MinorPent => "minor pentatonic",
            ScaleKind::Blues => "blues",
            ScaleKind::Dorian => "Dorian",
            ScaleKind::Phrygian => "Phrygian",
            ScaleKind::Lydian => "Lydian",
            ScaleKind::Mixolydian => "Mixolydian",
            ScaleKind::Locrian => "Locrian",
            ScaleKind::HarmonicMinor => "harmonic minor",
        }
    }

    fn id(self) -> &'static str {
        match self {
            ScaleKind::Major => "major",
            ScaleKind::Minor => "minor",
            ScaleKind::MajorPent => "major_pent",
            ScaleKind::MinorPent => "minor_pent",
            ScaleKind::Blues => "blues",
            ScaleKind::Dorian => "dorian",
            ScaleKind::Phrygian => "phrygian",
            ScaleKind::Lydian => "lydian",
            ScaleKind::Mixolydian => "mixolydian",
            ScaleKind::Locrian => "locrian",
            ScaleKind::HarmonicMinor => "harmonic_minor",
        }
    }

    fn from_id(s: &str) -> Option<ScaleKind> {
        KINDS.iter().copied().find(|k| k.id() == s)
    }
}

/// The tunings offered: (name, open strings low to high as MIDI).
const TUNINGS: [(&str, [i32; 6]); 4] = [
    ("E Standard", [40, 45, 50, 55, 59, 64]),
    ("Eb Standard", [39, 44, 49, 54, 58, 63]),
    ("D Standard", [38, 43, 48, 53, 57, 62]),
    ("Drop D", [38, 45, 50, 55, 59, 64]),
];

/// A place on the neck: string (0 = the low one) and fret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spot {
    pub string: usize,
    pub fret: i32,
}

/// The notes of `kind` on `root` (0 = C) on these strings, every fret 0..=FRETS.
pub fn neck_notes(root: i32, kind: ScaleKind, strings: &[i32; 6]) -> Vec<Spot> {
    let steps = kind.steps();
    let mut out = Vec::new();
    for (s, open) in strings.iter().enumerate() {
        for fret in 0..=FRETS {
            if steps.contains(&(open + fret - root).rem_euclid(12)) {
                out.push(Spot { string: s, fret });
            }
        }
    }
    out
}

/// Box `n` (0-based) of the scale, in playing order (low string first, each string low to high fret).
pub fn box_notes(root: i32, kind: ScaleKind, n: usize, strings: &[i32; 6]) -> Vec<Spot> {
    let steps = kind.box_steps();
    let per = if steps.len() <= 5 { 2 } else { 3 };
    // the scale as MIDI notes, low to high, from below the low string's open note
    let mut scale: Vec<i32> = Vec::new();
    for oct in 1..10 {
        for s in steps {
            scale.push(root + 12 * oct + s);
        }
    }
    scale.sort();
    // box n starts on the low string at the scale's n-th degree, at the lowest fret it falls on
    let start_pc = (root + steps[n % steps.len()]).rem_euclid(12);
    let fret0 = (start_pc - strings[0]).rem_euclid(12);
    let first = strings[0] + fret0;
    let Some(mut i) = scale.iter().position(|&m| m == first) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (s, open) in strings.iter().enumerate() {
        for _ in 0..per {
            let Some(&m) = scale.get(i) else { break };
            // a note lower than this string's open one can't be played on it: it was the last string's
            let fret = m - open;
            if fret < 0 {
                break;
            }
            out.push(Spot { string: s, fret });
            i += 1;
        }
    }
    if kind == ScaleKind::Blues {
        let lo = out.iter().map(|p| p.fret).min().unwrap_or(0);
        let hi = out.iter().map(|p| p.fret).max().unwrap_or(0);
        let blue = (root + 6).rem_euclid(12);
        let mut all: Vec<Spot> = out.clone();
        for (s, open) in strings.iter().enumerate() {
            for fret in lo..=hi {
                if (open + fret).rem_euclid(12) == blue {
                    all.push(Spot { string: s, fret });
                }
            }
        }
        // back in playing order: by pitch
        all.sort_by_key(|p| (strings[p.string] + p.fret, p.string));
        all.dedup_by_key(|p| strings[p.string] + p.fret);
        out = all;
    }
    out
}

/// A degree's colour (the note widget's: the root gold, the 3rd green, the 5th orange...).
fn degree_color(semis: i32) -> Color32 {
    match semis.rem_euclid(12) {
        0 => Color32::from_rgb(0xf5, 0xc5, 0x42),
        1 | 2 => Color32::from_rgb(0x7f, 0xc8, 0xf8),
        3 | 4 => Color32::from_rgb(0x4c, 0xaf, 0x7d),
        5 => Color32::from_rgb(0xb4, 0x8e, 0xf0),
        6 => Color32::from_rgb(0x5b, 0x7c, 0xf0), // the ♭5 / ♯4: the blue note
        7 => Color32::from_rgb(0xf0, 0x8c, 0x4a),
        8 | 9 => Color32::from_rgb(0x4f, 0xd1, 0xc5),
        _ => Color32::from_rgb(0xef, 0x6f, 0x8a),
    }
}

fn faded(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Every note of the box, in any order.
    Free,
    /// The box in order, up then back down.
    UpDown,
}

pub struct ScalesView {
    root: i32,
    kind: ScaleKind,
    /// None: the whole neck.
    boxn: Option<usize>,
    tuning: usize,
    mode: Mode,
    degrees: bool,
    loaded: bool,
    listen: Listen,
    /// The notes to play (the box, or the whole neck), and which have been played.
    targets: Vec<Spot>,
    hit: Vec<bool>,
    /// Up & down: the next note's index in `order`.
    order: Vec<usize>,
    step: usize,
    laps: u32,
    lap_start: Option<Instant>,
    last_lap: Option<Duration>,
    misses: u32,
    /// The note heard: (MIDI, since when it's been that note).
    heard: Option<(i32, Instant)>,
    /// The last note counted, and whether the input went quiet since (so the same note counts again).
    counted: Option<i32>,
    rearmed: bool,
    wrong: Option<(i32, Instant)>,
    flash: Option<(usize, Instant)>,
}

impl Default for ScalesView {
    fn default() -> Self {
        ScalesView {
            root: 9, // A
            kind: ScaleKind::MinorPent,
            boxn: Some(0),
            tuning: 0,
            mode: Mode::Free,
            degrees: false,
            loaded: false,
            listen: Listen::default(),
            targets: Vec::new(),
            hit: Vec::new(),
            order: Vec::new(),
            step: 0,
            laps: 0,
            lap_start: None,
            last_lap: None,
            misses: 0,
            heard: None,
            counted: None,
            rearmed: true,
            wrong: None,
            flash: None,
        }
    }
}

impl ScalesView {
    fn strings(&self) -> &'static [i32; 6] {
        &TUNINGS[self.tuning].1
    }

    fn midi(&self, p: Spot) -> i32 {
        self.strings()[p.string] + p.fret
    }

    /// What it was left on last time (config "scales").
    fn load(&mut self, st: &AppState) {
        self.loaded = true;
        let c = &st.config;
        self.root = c.i64("scales_root", 9).rem_euclid(12) as i32;
        self.kind = ScaleKind::from_id(&c.str("scales_kind", "minor_pent")).unwrap_or(ScaleKind::MinorPent);
        let b = c.i64("scales_box", 1);
        self.boxn = (b > 0).then(|| (b as usize - 1).min(self.kind.boxes() - 1));
        self.tuning = (c.i64("scales_tuning", 0).max(0) as usize).min(TUNINGS.len() - 1);
        self.mode = if c.str("scales_mode", "free") == "updown" { Mode::UpDown } else { Mode::Free };
        self.degrees = c.bool("scales_degrees", false);
        self.rebuild();
    }

    fn save(&self, st: &mut AppState) {
        st.config.set("scales_root", self.root as i64);
        st.config.set("scales_kind", self.kind.id());
        st.config.set("scales_box", self.boxn.map(|b| b as i64 + 1).unwrap_or(0));
        st.config.set("scales_tuning", self.tuning as i64);
        st.config.set("scales_mode", if self.mode == Mode::UpDown { "updown" } else { "free" });
        st.config.set("scales_degrees", self.degrees);
        st.save_config();
    }

    /// The notes to play for the current choice, from the start.
    fn rebuild(&mut self) {
        let strings = *self.strings();
        self.targets = match self.boxn {
            Some(n) => box_notes(self.root, self.kind, n, &strings),
            None => neck_notes(self.root, self.kind, &strings),
        };
        // playing order: by pitch (a box's order already; the whole neck's low to high, each pitch once)
        let mut idx: Vec<usize> = (0..self.targets.len()).collect();
        idx.sort_by_key(|&i| (self.midi(self.targets[i]), self.targets[i].string));
        if self.boxn.is_none() {
            idx.dedup_by_key(|i| strings[self.targets[*i].string] + self.targets[*i].fret);
        }
        let down: Vec<usize> = idx.iter().rev().skip(1).copied().collect();
        self.order = idx;
        self.order.extend(down);
        self.restart();
    }

    fn restart(&mut self) {
        self.hit = vec![false; self.targets.len()];
        self.step = 0;
        self.laps = 0;
        self.lap_start = None;
        self.last_lap = None;
        self.misses = 0;
        self.wrong = None;
        self.flash = None;
    }

    fn changed(&mut self, st: &mut AppState) {
        if let Some(b) = self.boxn {
            self.boxn = Some(b.min(self.kind.boxes() - 1));
        }
        self.rebuild();
        self.save(st);
    }

    /// A note was played: count it against the box.
    fn played(&mut self, m: i32, now: Instant) {
        let same = |p: &Spot, s: &Self| s.midi(*p) == m;
        match self.mode {
            Mode::Free => {
                let found: Vec<usize> = (0..self.targets.len()).filter(|&i| same(&self.targets[i], self)).collect();
                if found.is_empty() {
                    self.wrong = Some((m, now));
                    self.misses += 1;
                } else {
                    if self.hit.iter().all(|h| !h) {
                        self.lap_start = Some(now);
                    }
                    for i in found {
                        self.hit[i] = true;
                        self.flash = Some((i, now));
                    }
                    if self.hit.iter().all(|&h| h) {
                        self.laps += 1;
                        self.last_lap = self.lap_start.map(|s| now - s);
                    }
                }
            }
            Mode::UpDown => {
                let Some(&want) = self.order.get(self.step) else { return };
                if self.midi(self.targets[want]) == m {
                    if self.step == 0 {
                        self.lap_start = Some(now);
                        self.hit.iter_mut().for_each(|h| *h = false);
                    }
                    self.hit[want] = true;
                    self.flash = Some((want, now));
                    self.step += 1;
                    if self.step >= self.order.len() {
                        self.laps += 1;
                        self.last_lap = self.lap_start.map(|s| now - s);
                        self.step = 0;
                    }
                } else {
                    self.wrong = Some((m, now));
                    self.misses += 1;
                }
            }
        }
    }

    /// Listen: a note counts once it's held steady, and again only after another note or a quiet moment.
    fn hear(&mut self, st: &AppState) -> bool {
        let live = self.listen.update(st.audio.as_ref().map(|a| a.engine.as_ref()));
        let now = Instant::now();
        if self.listen.level_db < QUIET_DB {
            self.rearmed = true;
        }
        match self.listen.midi.map(|m| m.round() as i32) {
            None => self.heard = None,
            Some(m) => {
                let since = match self.heard {
                    Some((h, at)) if h == m => at,
                    _ => {
                        self.heard = Some((m, now));
                        now
                    }
                };
                let fresh = self.counted != Some(m) || self.rearmed;
                if fresh && now - since >= STEADY && self.listen.level_db >= QUIET_DB {
                    self.counted = Some(m);
                    self.rearmed = false;
                    self.played(m, now);
                }
            }
        }
        live
    }

    pub fn handle(&mut self, lv: &mut LibView, st: &mut AppState, ev: &Event) -> bool {
        let Event::Key { key, pressed: true, modifiers, .. } = ev else {
            return false;
        };
        if modifiers.command {
            return false;
        }
        match key {
            Key::ArrowRight => self.boxn = Some(self.boxn.map_or(0, |b| (b + 1) % self.kind.boxes())),
            Key::ArrowLeft => {
                let n = self.kind.boxes();
                self.boxn = Some(self.boxn.map_or(n - 1, |b| (b + n - 1) % n));
            }
            Key::ArrowUp => self.root = (self.root + 1).rem_euclid(12),
            Key::ArrowDown => self.root = (self.root - 1).rem_euclid(12),
            Key::N => self.boxn = None,
            Key::M => {
                self.mode = if self.mode == Mode::Free { Mode::UpDown } else { Mode::Free };
            }
            Key::D => self.degrees = !self.degrees,
            Key::R => {
                self.restart();
                return true;
            }
            Key::Escape => {
                lv.go(st, View::Home);
                return true;
            }
            _ => return false,
        }
        self.changed(st);
        true
    }

    pub fn ui(&mut self, ui: &mut Ui, lv: &mut LibView, st: &mut AppState, area: Rect) {
        if !self.loaded {
            self.load(st);
        }
        let p = ui.painter().clone();
        let th = t();
        let (side, main) = page_rects(area);
        lv.draw_sidebar(ui, st, side);
        if lv.view != View::Scales {
            return;
        }
        let live = self.hear(st);
        ui.ctx()
            .request_repaint_after(if live { Duration::from_millis(16) } else { Duration::from_millis(50) });

        // header
        let hy = main.min.y + px(26.0);
        txt(&p, "Scales", pos2(main.min.x, hy), 28.0, th.text, 800, Align2::LEFT_CENTER, None);
        let title = format!(
            "{} {}{}",
            NAMES[self.root as usize],
            self.kind.name(),
            match self.boxn {
                Some(b) => format!(" · box {}", b + 1),
                None => " · whole neck".into(),
            }
        );
        txt(&p, &title, pos2(main.min.x + px(130.0), hy + px(3.0)), 16.0, th.caption, 600, Align2::LEFT_CENTER, None);

        let mut changed = false;
        let gap = px(6.0);
        let row_h = px(40.0);
        let label_w = px(64.0);
        let mut y = hy + px(40.0);
        let row_label = |y: f32, s: &str| {
            txt(&p, s, pos2(main.min.x, y + row_h / 2.0), 11.0, th.caption, 800, Align2::LEFT_CENTER, None);
        };
        let x0 = main.min.x + label_w;
        let row_w = main.max.x - x0;

        // key
        row_label(y, "KEY");
        let w = (row_w - gap * 11.0) / 12.0;
        for k in 0..12 {
            let r = Rect::from_min_size(pos2(x0 + k as f32 * (w + gap), y), vec2(w, row_h));
            let on = self.root == k;
            if button(ui, r, ("scale-key", k), NAMES[k as usize], "", ButtonStyle::active(on)).clicked() && !on {
                self.root = k;
                changed = true;
            }
        }
        y += row_h + gap * 2.0;

        // scale
        row_label(y, "SCALE");
        let w = (row_w - gap * (KINDS.len() as f32 - 1.0)) / KINDS.len() as f32;
        for (i, k) in KINDS.iter().enumerate() {
            let r = Rect::from_min_size(pos2(x0 + i as f32 * (w + gap), y), vec2(w, row_h));
            let on = self.kind == *k;
            if button(ui, r, ("scale-kind", i), k.label(), "", ButtonStyle::active(on).size(12.0)).clicked() && !on {
                self.kind = *k;
                changed = true;
            }
        }
        y += row_h + gap * 2.0;

        // box, then the mode, names/degrees and the tuning on the right
        row_label(y, "BOX");
        let n = self.kind.boxes();
        let bw = px(72.0);
        let mut x = x0;
        let nw = px(if cfg!(target_os = "android") { 112.0 } else { 140.0 });
        let r = Rect::from_min_size(pos2(x, y), vec2(nw, row_h));
        if button(ui, r, "scale-neck", "Whole neck", cap("N"), ButtonStyle::active(self.boxn.is_none())).clicked()
            && self.boxn.is_some()
        {
            self.boxn = None;
            changed = true;
        }
        x += nw + gap;
        for b in 0..n {
            let r = Rect::from_min_size(pos2(x, y), vec2(bw, row_h));
            let on = self.boxn == Some(b);
            if button(ui, r, ("scale-box", b), &format!("Box {}", b + 1), "", ButtonStyle::active(on)).clicked()
                && !on
            {
                self.boxn = Some(b);
                changed = true;
            }
            x += bw + gap;
        }
        let mut rx = main.max.x;
        let tw = px(130.0);
        rx -= tw;
        let r = Rect::from_min_size(pos2(rx, y), vec2(tw, row_h));
        if button(ui, r, "scale-tuning", TUNINGS[self.tuning].0, "", ButtonStyle::default()).clicked() {
            self.tuning = (self.tuning + 1) % TUNINGS.len();
            changed = true;
        }
        let dw = px(104.0);
        rx -= dw + gap;
        let r = Rect::from_min_size(pos2(rx, y), vec2(dw, row_h));
        if button(ui, r, "scale-degrees", if self.degrees { "Degrees" } else { "Notes" }, cap("D"), ButtonStyle::default())
            .clicked()
        {
            self.degrees = !self.degrees;
            changed = true;
        }
        let mw = px(108.0);
        for (i, (m, label)) in [(Mode::UpDown, "Up & down"), (Mode::Free, "Free")].iter().enumerate() {
            rx -= mw + gap;
            let r = Rect::from_min_size(pos2(rx, y), vec2(mw, row_h));
            let on = self.mode == *m;
            if button(ui, r, ("scale-mode", i), label, "", ButtonStyle::active(on)).clicked() && !on {
                self.mode = *m;
                changed = true;
            }
        }
        y += row_h + px(22.0);

        if changed {
            self.changed(st);
        }

        // the neck
        let status_h = px(120.0);
        let neck = Rect::from_min_max(pos2(main.min.x, y), pos2(main.max.x, main.max.y - status_h - px(16.0)));
        self.draw_neck(ui, &p, neck);

        // under it: the note heard, progress, restart
        let sr = Rect::from_min_max(pos2(main.min.x, main.max.y - status_h), main.max);
        self.draw_status(ui, st, &p, sr);
    }

    fn draw_neck(&mut self, ui: &Ui, p: &egui::Painter, area: Rect) {
        let th = t();
        let strings = *self.strings();
        let label_h = px(22.0);
        let nut_w = px(34.0);
        let board = Rect::from_min_max(
            pos2(area.min.x + nut_w, area.min.y + px(8.0)),
            pos2(area.max.x - px(8.0), area.max.y - label_h),
        );
        let fw = board.width() / FRETS as f32;
        let sh = board.height() / 6.0;
        // string 0 (the low one) at the bottom, as you look down at the guitar
        let sy = |s: usize| board.max.y - sh * (s as f32 + 0.5);
        let fx = |f: i32| if f == 0 { area.min.x + nut_w / 2.0 } else { board.min.x + fw * (f as f32 - 0.5) };
        p.rect_filled(board, 6.0, if th.dark { Color32::from_rgb(0x1b, 0x14, 0x10) } else { Color32::from_rgb(0x6b, 0x4a, 0x2f) });
        // inlays
        for f in INLAYS {
            let x = board.min.x + fw * (f as f32 - 0.5);
            let c = faded(Color32::WHITE, 0.10);
            if f == 12 {
                p.circle_filled(pos2(x, board.min.y + sh * 1.5), px(6.0), c);
                p.circle_filled(pos2(x, board.max.y - sh * 1.5), px(6.0), c);
            } else {
                p.circle_filled(pos2(x, board.center().y), px(6.0), c);
            }
        }
        // frets and the nut
        for f in 0..=FRETS {
            let x = board.min.x + fw * f as f32;
            let (w, c) = if f == 0 { (px(5.0), Color32::from_rgb(0xe8, 0xe2, 0xd0)) } else { (px(2.0), Color32::from_rgb(0x9a, 0x9a, 0xa2)) };
            p.line_segment([pos2(x, board.min.y), pos2(x, board.max.y)], Stroke::new(w, c));
        }
        for f in INLAYS.iter().copied().chain([1]) {
            txt(p, &f.to_string(), pos2(fx(f), board.max.y + label_h / 2.0 + px(2.0)), 12.0, th.caption, 700, Align2::CENTER_CENTER, None);
        }
        // strings, thicker low, with their open note's name by the nut
        for s in 0..6 {
            let w = px(1.0 + (5 - s) as f32 * 0.35);
            p.line_segment(
                [pos2(area.min.x + nut_w * 0.75, sy(s)), pos2(board.max.x, sy(s))],
                Stroke::new(w, Color32::from_rgb(0xc8, 0xc4, 0xbc)),
            );
        }
        let r = (sh * 0.36).min(fw * 0.42).min(px(17.0));
        let now = Instant::now();
        // the rest of the scale, dimmed (with a box chosen)
        if self.boxn.is_some() {
            for spot in neck_notes(self.root, self.kind, &strings) {
                if self.targets.contains(&spot) {
                    continue;
                }
                let semis = (strings[spot.string] + spot.fret - self.root).rem_euclid(12);
                let c = pos2(fx(spot.fret), sy(spot.string));
                p.circle_filled(c, r * 0.55, faded(degree_color(semis), 0.28));
            }
        }
        let playing = self.listen.midi.map(|m| m.round() as i32);
        let next = (self.mode == Mode::UpDown).then(|| self.order.get(self.step).copied()).flatten();
        for (i, spot) in self.targets.iter().enumerate() {
            let m = strings[spot.string] + spot.fret;
            let semis = (m - self.root).rem_euclid(12);
            let col = degree_color(semis);
            let c = pos2(fx(spot.fret), sy(spot.string));
            let done = self.hit.get(i).copied().unwrap_or(false);
            let fill = if done || next.is_none() { col } else { faded(col, 0.55) };
            p.circle_filled(c, r, if done { col } else { faded(fill, if self.mode == Mode::Free { 0.55 } else { 0.85 }) });
            if semis == 0 {
                p.circle_stroke(c, r + px(2.0), Stroke::new(px(2.0), Color32::from_rgb(0xff, 0xe7, 0x9a)));
            }
            if done {
                p.circle_stroke(c, r, Stroke::new(px(2.5), Color32::WHITE));
            }
            if next == Some(i) {
                let pulse = 0.5 + 0.5 * (ui.input(|x| x.time) as f32 * 5.0).sin();
                p.circle_stroke(c, r + px(5.0) + pulse * px(3.0), Stroke::new(px(3.0), th.accent));
            }
            if playing == Some(m) {
                p.circle_filled(c, r + px(8.0), faded(Color32::WHITE, 0.18));
                p.circle_stroke(c, r + px(8.0), Stroke::new(px(2.0), Color32::WHITE));
            }
            if let Some((fi, at)) = self.flash {
                let age = (now - at).as_secs_f32();
                if fi == i && age < 0.4 {
                    p.circle_stroke(c, r + px(4.0) + age * px(30.0), Stroke::new(px(2.0), faded(Color32::WHITE, 1.0 - age / 0.4)));
                }
            }
            let label = if self.degrees { DEGREES[semis as usize] } else { NAMES[m.rem_euclid(12) as usize] };
            let ink = Color32::from_rgb(0x15, 0x15, 0x1a);
            txt(p, label, c, (r * 0.85 / px(1.0)).clamp(9.0, 14.0), ink, 800, Align2::CENTER_CENTER, None);
        }
        // a played note that's not in the box: marked where it is on the neck, in red
        if let Some((m, at)) = self.wrong.filter(|(_, at)| now - *at < WRONG_FOR) {
            let a = 1.0 - (now - at).as_secs_f32() / WRONG_FOR.as_secs_f32();
            let lo = self.targets.iter().map(|s| s.fret).min().unwrap_or(0).saturating_sub(1);
            for s in 0..6 {
                let f = m - strings[s];
                if (0..=FRETS).contains(&f) && (self.boxn.is_none() || (f >= lo && f <= lo + 6)) {
                    let c = pos2(fx(f), sy(s));
                    p.circle_stroke(c, r, Stroke::new(px(3.0), faded(Color32::from_rgb(0xff, 0x4d, 0x4d), a)));
                }
            }
        }
        let _ = interact(ui, area, "scale-neck-area", Sense::hover());
    }

    fn draw_status(&mut self, ui: &Ui, st: &AppState, p: &egui::Painter, area: Rect) {
        let th = t();
        qrs_ui::paint::card(p, area, px(18.0), false);
        let pad = px(20.0);
        let cy = area.center().y;
        // the note heard
        let input = st.audio.as_ref().is_some_and(|a| a.engine.status().input.is_some());
        let (big, small, col) = match (input, self.listen.note()) {
            (false, _) => ("—".to_string(), "no guitar input (Settings › Audio)".to_string(), th.caption),
            (true, None) => ("—".to_string(), "play a note".to_string(), th.caption),
            (true, Some((name, oct, cents))) => {
                let m = self.listen.midi.map(|m| m.round() as i32).unwrap_or(0);
                let in_scale = self.kind.steps().contains(&(m - self.root).rem_euclid(12));
                let deg = DEGREES[(m - self.root).rem_euclid(12) as usize];
                (
                    format!("{name}{oct}"),
                    format!("{cents:+}¢ · {}", if in_scale { format!("degree {deg}") } else { "not in the scale".into() }),
                    if in_scale { degree_color(m - self.root) } else { Color32::from_rgb(0xff, 0x6b, 0x6b) },
                )
            }
        };
        txt(p, "HEARD", pos2(area.min.x + pad, area.min.y + px(22.0)), 11.0, th.caption, 800, Align2::LEFT_CENTER, None);
        txt(p, &big, pos2(area.min.x + pad, cy + px(4.0)), 40.0, col, 800, Align2::LEFT_CENTER, None);
        txt(p, &small, pos2(area.min.x + pad, area.max.y - px(20.0)), 13.0, th.caption, 600, Align2::LEFT_CENTER, None);

        // progress
        let px0 = area.min.x + px(300.0);
        let total = self.targets.len();
        let done = self.hit.iter().filter(|&&h| h).count();
        let (head, line) = match self.mode {
            Mode::Free => (
                "FIND EVERY NOTE".to_string(),
                format!("{done} of {total}"),
            ),
            Mode::UpDown => {
                let up = self.order.len() / 2 + 1;
                let dir = if self.step < up { "up" } else { "down" };
                (
                    format!("PLAY IT IN ORDER · {dir}"),
                    match self.order.get(self.step) {
                        Some(&i) => {
                            let m = self.midi(self.targets[i]);
                            format!("next: {}  ({} of {})", NAMES[m.rem_euclid(12) as usize], self.step + 1, self.order.len())
                        }
                        None => String::new(),
                    },
                )
            }
        };
        txt(p, &head, pos2(px0, area.min.y + px(22.0)), 11.0, th.caption, 800, Align2::LEFT_CENTER, None);
        txt(p, &line, pos2(px0, cy + px(2.0)), 26.0, th.text, 800, Align2::LEFT_CENTER, None);
        let mut extra = format!("{} wrong", self.misses);
        if self.laps > 0 {
            extra += &format!(
                " · {} {} done{}",
                self.laps,
                if self.laps == 1 { "time" } else { "times" },
                self.last_lap.map(|d| format!(", last in {:.1} s", d.as_secs_f32())).unwrap_or_default()
            );
        }
        txt(p, &extra, pos2(px0, area.max.y - px(20.0)), 13.0, th.caption, 600, Align2::LEFT_CENTER, None);
        // the bar
        let bar = Rect::from_min_size(pos2(px0 + px(330.0), cy - px(5.0)), vec2((area.max.x - px(200.0) - px0 - px(330.0)).max(px(40.0)), px(10.0)));
        p.rect_filled(bar, 5.0, th.track);
        let k = match self.mode {
            Mode::Free if total > 0 => done as f32 / total as f32,
            Mode::UpDown if !self.order.is_empty() => self.step as f32 / self.order.len() as f32,
            _ => 0.0,
        };
        if k > 0.0 {
            p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * k, bar.height())), 5.0, th.accent);
        }
        // restart
        let r = Rect::from_min_size(pos2(area.max.x - pad - px(150.0), cy - px(22.0)), vec2(px(150.0), px(44.0)));
        if button(ui, r, "scale-restart", "Start again", cap("R"), ButtonStyle::default().icon(Icon::Restart)).clicked() {
            self.restart();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STD: [i32; 6] = [40, 45, 50, 55, 59, 64];

    fn frets(v: &[Spot]) -> Vec<(usize, i32)> {
        v.iter().map(|s| (s.string, s.fret)).collect()
    }

    #[test]
    fn a_minor_pentatonic_box_1_is_the_usual_one() {
        let b = box_notes(9, ScaleKind::MinorPent, 0, &STD);
        assert_eq!(
            frets(&b),
            vec![(0, 5), (0, 8), (1, 5), (1, 7), (2, 5), (2, 7), (3, 5), (3, 7), (4, 5), (4, 8), (5, 5), (5, 8)]
        );
    }

    #[test]
    fn a_minor_pentatonic_box_2_starts_on_c() {
        let b = box_notes(9, ScaleKind::MinorPent, 1, &STD);
        assert_eq!(b[0], Spot { string: 0, fret: 8 });
        assert_eq!(b.len(), 12);
        // the B string shifts up a fret: C (fret 1 on B... here 10) and D
        assert_eq!(frets(&b)[8..10], [(4, 8), (4, 10)]);
    }

    #[test]
    fn three_notes_per_string_for_seven_note_scales() {
        let b = box_notes(0, ScaleKind::Major, 0, &STD); // C major from C on the low E (fret 8)
        assert_eq!(b.len(), 18);
        assert_eq!(b[0], Spot { string: 0, fret: 8 });
    }

    #[test]
    fn blues_adds_the_flat_five_inside_the_box() {
        let b = box_notes(9, ScaleKind::Blues, 0, &STD);
        // Eb: low E fret 11 is outside 5..8; A string fret 6, G string fret 8
        assert!(b.contains(&Spot { string: 1, fret: 6 }));
        assert!(b.contains(&Spot { string: 3, fret: 8 }));
        assert!(!b.iter().any(|s| s.fret > 8 || s.fret < 5));
    }

    #[test]
    fn every_box_note_is_in_the_scale() {
        for k in KINDS {
            for root in 0..12 {
                for n in 0..k.boxes() {
                    for s in box_notes(root, k, n, &STD) {
                        let semis = (STD[s.string] + s.fret - root).rem_euclid(12);
                        assert!(k.steps().contains(&semis), "{k:?} root {root} box {n}: {s:?}");
                    }
                }
            }
        }
    }
}
