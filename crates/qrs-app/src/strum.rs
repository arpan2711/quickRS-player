//! The Strumming page: a one-bar pattern (eight eighth-note slots in 4/4) played against the click, on one chord or
//! through a chord progression. Each strum is timed against the beat and, on the down strums, its chord is checked
//! with the song scorer's own test. Which way the pick moved can't be heard, so a slot only asks for a strum then.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use eframe::egui;
use egui::{Align2, Color32, Event, Key, Painter, Pos2, Rect, Stroke, Ui, pos2, vec2};
use qrs_analysis::detect::{self, NoteGroup};
use qrs_analysis::spectrum::Spectra;
use qrs_audio::Engine;
use qrs_audio::jam::theory::{Chord, Quality, Scale, parse_progression};
use qrs_audio::jam::{BackingKind, BackingSpec, ClickParams, DrumParams};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{ButtonStyle, button};

use crate::library::{LibView, View, page_rects, txt};
use crate::scales::TUNINGS;
use crate::state::AppState;

const NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const COUNTS: [&str; 8] = ["1", "&", "2", "&", "3", "&", "4", "&"];
const STD: [i32; 6] = [40, 45, 50, 55, 59, 64];
const GREEN: Color32 = Color32::from_rgb(0x4c, 0xc0, 0x7d);
const AMBER: Color32 = Color32::from_rgb(0xf0, 0xb0, 0x4a);
const RED: Color32 = Color32::from_rgb(0xff, 0x5f, 0x5f);

/// The 32 patterns: D down, U up, . nothing, on 1 & 2 & 3 & 4 &.
pub const PATTERNS: [&str; 32] = [
    "D.D.D.D.", "D.D.D.DU", "DUDUDUDU", "D.DUD.DU", "DUD.DUD.", "D.DU.UD.", "DU.UDU.U", "D.D.DUDU",
    "D.DUDUDU", "DU.U.U.U", "DU.U.UDU", ".UDUDU.U", "D.DUD.D.", ".U.U.U.U", ".U.U.UDU", ".UDU.UDU",
    "D...D...", "..D...D.", "DUD.D.D.", "DU.UD.DU", "D....UDU", "D..UD.D.", "DUDUD.D.", ".UD.D.D.",
    "DUDUD..U", "DU..DU..", ".UDUDUDU", ".UD.D.DU", "DUDU.UDU", "D.D..UDU", "DUDUDUD.", ".UDU.UD.",
];

/// The progressions: (the label, its chords a bar each, the key's scale).
pub const PROGRESSIONS: [(&str, &str, Scale); 8] = [
    ("I IV V", "I IV V I", Scale::Major),
    ("I V vi IV", "I V vi IV", Scale::Major),
    ("vi IV I V", "vi IV I V", Scale::Major),
    ("I vi IV V", "I vi IV V", Scale::Major),
    ("ii V I", "ii V I I", Scale::Major),
    ("12-bar blues", "I7 I7 I7 I7 IV7 IV7 I7 I7 V7 IV7 I7 V7", Scale::Major),
    ("i VI III VII", "i VI III VII", Scale::NaturalMinor),
    ("i iv v", "i iv v i", Scale::NaturalMinor),
];

pub const QUALITIES: [(Quality, &str); 8] = [
    (Quality::Major, "Major"),
    (Quality::Minor, "Minor"),
    (Quality::Dom7, "7"),
    (Quality::Min7, "m7"),
    (Quality::Maj7, "maj7"),
    (Quality::Sus2, "sus2"),
    (Quality::Sus4, "sus4"),
    (Quality::Power, "5"),
];

/// An open shape in E Standard, low string first (-1: not played).
fn open_shape(root: u8, q: Quality) -> Option<[i32; 6]> {
    use Quality::*;
    Some(match (root, q) {
        (0, Major) => [-1, 3, 2, 0, 1, 0],
        (0, Dom7) => [-1, 3, 2, 3, 1, 0],
        (0, Maj7) => [-1, 3, 2, 0, 0, 0],
        (2, Major) => [-1, -1, 0, 2, 3, 2],
        (2, Minor) => [-1, -1, 0, 2, 3, 1],
        (2, Dom7) => [-1, -1, 0, 2, 1, 2],
        (2, Min7) => [-1, -1, 0, 2, 1, 1],
        (2, Maj7) => [-1, -1, 0, 2, 2, 2],
        (2, Sus2) => [-1, -1, 0, 2, 3, 0],
        (2, Sus4) => [-1, -1, 0, 2, 3, 3],
        (4, Major) => [0, 2, 2, 1, 0, 0],
        (4, Minor) => [0, 2, 2, 0, 0, 0],
        (4, Dom7) => [0, 2, 0, 1, 0, 0],
        (4, Min7) => [0, 2, 0, 0, 0, 0],
        (4, Sus4) => [0, 2, 2, 2, 0, 0],
        (5, Maj7) => [-1, -1, 3, 2, 1, 0],
        (7, Major) => [3, 2, 0, 0, 0, 3],
        (7, Dom7) => [3, 2, 0, 0, 0, 1],
        (7, Maj7) => [3, 2, 0, 0, 0, 2],
        (9, Major) => [-1, 0, 2, 2, 2, 0],
        (9, Minor) => [-1, 0, 2, 2, 1, 0],
        (9, Dom7) => [-1, 0, 2, 0, 2, 0],
        (9, Min7) => [-1, 0, 2, 0, 1, 0],
        (9, Maj7) => [-1, 0, 2, 1, 2, 0],
        (9, Sus2) => [-1, 0, 2, 2, 0, 0],
        (9, Sus4) => [-1, 0, 2, 2, 3, 0],
        _ => return None,
    })
}

/// A movable shape from the low string's root (frets above the root's; -1: not played).
fn e_shape(q: Quality) -> Option<[i32; 6]> {
    use Quality::*;
    Some(match q {
        Major => [0, 2, 2, 1, 0, 0],
        Minor => [0, 2, 2, 0, 0, 0],
        Dom7 => [0, 2, 0, 1, 0, 0],
        Min7 => [0, 2, 0, 0, 0, 0],
        Maj7 => [0, -1, 1, 1, 0, -1],
        Sus4 => [0, 2, 2, 2, 0, 0],
        Power => [0, 2, 2, -1, -1, -1],
        _ => return None,
    })
}

/// A movable shape from the A string's root.
fn a_shape(q: Quality) -> Option<[i32; 6]> {
    use Quality::*;
    Some(match q {
        Major => [-1, 0, 2, 2, 2, 0],
        Minor => [-1, 0, 2, 2, 1, 0],
        Dom7 => [-1, 0, 2, 0, 2, 0],
        Min7 => [-1, 0, 2, 0, 1, 0],
        Maj7 => [-1, 0, 2, 1, 2, 0],
        Sus2 => [-1, 0, 2, 2, 0, 0],
        Sus4 => [-1, 0, 2, 2, 3, 0],
        Power => [-1, 0, 2, 2, -1, -1],
        _ => return None,
    })
}

/// Where to put your fingers for `chord` on these strings (-1: not played): an open shape where there's one, else
/// the barre lowest on the neck. A tuning a whole string set lower takes the shape that sounds right there; a string
/// tuned apart from the rest (Drop D's low string) is left out.
pub fn shape(chord: Chord, strings: &[i32; 6]) -> Option<[i32; 6]> {
    let offset = strings[5] - STD[5];
    let root = (chord.root as i32 - offset).rem_euclid(12) as u8;
    let frets = open_shape(root, chord.quality).or_else(|| {
        let at = |s: Option<[i32; 6]>, r: i32| s.map(|sh| (r, sh));
        let e = at(e_shape(chord.quality), (root as i32 - 4).rem_euclid(12));
        let a = at(a_shape(chord.quality), (root as i32 - 9).rem_euclid(12));
        // (with the low string left out, an E shape can lose its only root)
        let low_apart = strings[0] - offset != STD[0];
        let (r, sh) = match (e, a) {
            (Some(_), Some(y)) if low_apart => y,
            (Some(x), Some(y)) => {
                if x.0 <= y.0 { x } else { y }
            }
            (x, y) => x.or(y)?,
        };
        Some(sh.map(|f| if f < 0 { -1 } else { f + r }))
    })?;
    let mut out = [-1; 6];
    for s in 0..6 {
        if frets[s] >= 0 && strings[s] - offset == STD[s] {
            out[s] = frets[s];
        }
    }
    Some(out)
}

/// The pitches a shape sounds.
fn sounding(sh: &[i32; 6], strings: &[i32; 6]) -> Vec<i32> {
    (0..6).filter(|&s| sh[s] >= 0).map(|s| strings[s] + sh[s]).collect()
}

fn strokes(pattern: usize) -> [u8; 8] {
    let mut out = [b'.'; 8];
    out.copy_from_slice(&PATTERNS[pattern].as_bytes()[..8]);
    out
}

/// What a slot of a bar got.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Milliseconds from the slot: + late, - early.
    pub ms: f32,
    /// The chord's check (None: not judged, or not in yet).
    pub chord: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct BarScore {
    pub bar: i64,
    pub slots: [Option<Hit>; 8],
    /// Strums where the pattern has none: where in the bar (eighths).
    pub extras: Vec<f32>,
}

/// The score of a run: pure, so it's tested without an engine.
#[derive(Clone, Debug, Default)]
pub struct StrumScore {
    pub bars: VecDeque<BarScore>,
    next_close: i64,
    pub bars_done: u32,
    pub bars_clean: u32,
    pub streak: u32,
    pub hits: u32,
    pub missed: u32,
    pub extra: u32,
    pub wrong_chords: u32,
    pub chords_judged: u32,
    pub sum_ms: f64,
    pub tight: u32,
}

/// Within this it's on the beat (ms).
pub const TIGHT_MS: f32 = 35.0;

impl StrumScore {
    fn bar_mut(&mut self, bar: i64) -> &mut BarScore {
        if let Some(i) = self.bars.iter().position(|b| b.bar == bar) {
            return &mut self.bars[i];
        }
        self.bars.push_back(BarScore { bar, ..Default::default() });
        while self.bars.len() > 3 {
            self.bars.pop_front();
        }
        self.bars.back_mut().unwrap()
    }

    pub fn bar(&self, bar: i64) -> Option<&BarScore> {
        self.bars.iter().find(|b| b.bar == bar)
    }

    /// A strum at `beat` (beats from bar 0): the slot it filled, if it filled one.
    pub fn strum(&mut self, beat: f64, bpm: f64, pattern: &[u8; 8]) -> Option<(i64, usize)> {
        let eighth = beat * 2.0;
        let n = eighth.round() as i64;
        if n < 0 {
            return None; // the count-in
        }
        let (bar, slot) = (n.div_euclid(8), n.rem_euclid(8) as usize);
        if bar < self.next_close {
            return None; // (already scored)
        }
        let eighth_s = 30.0 / bpm;
        let ms = ((eighth - n as f64) * eighth_s * 1000.0) as f32;
        let window = (0.45 * eighth_s * 1000.0).min(90.0) as f32;
        let b = self.bar_mut(bar);
        if pattern[slot] != b'.' && b.slots[slot].is_none() && ms.abs() <= window {
            b.slots[slot] = Some(Hit { ms, chord: None });
            self.hits += 1;
            self.sum_ms += ms as f64;
            if ms.abs() <= TIGHT_MS {
                self.tight += 1;
            }
            return Some((bar, slot));
        }
        // a second detection of the strum just counted isn't an extra strum
        if b.slots[slot].is_some_and(|h| (ms - h.ms).abs() < 60.0) {
            return None;
        }
        b.extras.push(eighth.rem_euclid(8.0) as f32);
        self.extra += 1;
        None
    }

    pub fn chord_result(&mut self, bar: i64, slot: usize, ok: bool) {
        self.chords_judged += 1;
        if !ok {
            self.wrong_chords += 1;
        }
        if let Some(b) = self.bars.iter_mut().find(|b| b.bar == bar)
            && let Some(h) = b.slots[slot].as_mut()
        {
            h.chord = Some(ok);
        }
    }

    /// Score the bars that have gone by at `beat` (half a beat later, for a late last strum).
    pub fn close(&mut self, beat: f64, pattern: &[u8; 8]) {
        while (self.next_close + 1) as f64 * 4.0 + 0.5 <= beat {
            let bar = self.next_close;
            self.next_close += 1;
            let b = self.bar_mut(bar).clone();
            let mut clean = b.extras.is_empty();
            for s in 0..8 {
                if pattern[s] == b'.' {
                    continue;
                }
                match b.slots[s] {
                    None => {
                        self.missed += 1;
                        clean = false;
                    }
                    Some(h) if h.chord == Some(false) => clean = false,
                    _ => {}
                }
            }
            self.bars_done += 1;
            if clean {
                self.bars_clean += 1;
                self.streak += 1;
            } else {
                self.streak = 0;
            }
        }
    }

    pub fn mean_ms(&self) -> Option<f64> {
        (self.hits > 0).then(|| self.sum_ms / self.hits as f64)
    }
}

/// A strum waiting for enough of the recording to check its chord.
struct Pending {
    index: u64,
    bar: i64,
    slot: usize,
    group: NoteGroup,
}

/// What the jam was set to before this page took it.
struct Saved {
    bpm: f64,
    meter: u32,
    click: ClickParams,
    drums: DrumParams,
    backing: Option<BackingSpec>,
}

pub struct StrumView {
    loaded: bool,
    progression_mode: bool,
    root: u8,
    quality: usize,
    progression: usize,
    bars_per_chord: u32,
    pattern: usize,
    bpm: f64,
    tuning: usize,
    click: bool,
    backing: bool,
    input_offset: f64,
    running: bool,
    /// Started, but the audio thread hasn't restarted the jam's clock yet: its beat is still the last run's.
    awaiting_clock: bool,
    /// When the page last drew (wall ns): see `watch`.
    last_frame: Option<Arc<AtomicU64>>,
    saved: Option<Saved>,
    score: StrumScore,
    pending: Vec<Pending>,
    sp: Spectra,
    /// The chord the last strum was judged against (a chord's first strum: the old one's notes may be ringing).
    last_chord: Option<(i64, Vec<i32>)>,
}

impl Default for StrumView {
    fn default() -> Self {
        StrumView {
            loaded: false,
            progression_mode: false,
            root: 7,
            quality: 0,
            progression: 1,
            bars_per_chord: 1,
            pattern: 0,
            bpm: 70.0,
            tuning: 0,
            click: true,
            backing: false,
            input_offset: 0.0,
            running: false,
            awaiting_clock: false,
            last_frame: None,
            saved: None,
            score: StrumScore::default(),
            pending: Vec::new(),
            sp: Spectra::new(),
            last_chord: None,
        }
    }
}

/// The audio goes on while the app is in the background, but the page doesn't draw: stop the click once it hasn't
/// for a moment. The thread ends with the run (when the view lets go of the time it shares with it).
fn watch(e: Arc<Engine>) -> Arc<AtomicU64> {
    let last = Arc::new(AtomicU64::new(qrs_audio::clock::now_ns()));
    let seen = last.clone();
    std::thread::spawn(move || {
        while Arc::strong_count(&seen) > 1 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if qrs_audio::clock::now_ns().saturating_sub(seen.load(Ordering::Relaxed)) > 1_500_000_000 {
                e.jam_stop();
                return;
            }
        }
    });
    last
}

fn cap(k: &'static str) -> &'static str {
    if cfg!(target_os = "android") { "" } else { k }
}

impl StrumView {
    fn load(&mut self, st: &AppState) {
        self.loaded = true;
        let c = &st.config;
        // through a microphone the speakers would be heard as strums: quiet until asked for
        let mic = st.mic_input();
        self.progression_mode = c.bool("strum_progression_mode", false);
        self.root = c.i64("strum_root", 7).rem_euclid(12) as u8;
        self.quality = (c.i64("strum_quality", 0).max(0) as usize).min(QUALITIES.len() - 1);
        self.progression = (c.i64("strum_progression", 1).max(0) as usize).min(PROGRESSIONS.len() - 1);
        self.bars_per_chord = c.i64("strum_bars_per_chord", 1).clamp(1, 2) as u32;
        self.pattern = (c.i64("strum_pattern", 0).max(0) as usize).min(PATTERNS.len() - 1);
        self.bpm = c.f64("strum_bpm", 70.0).clamp(40.0, 200.0);
        self.tuning = (c.i64("strum_tuning", 0).max(0) as usize).min(TUNINGS.len() - 1);
        self.click = c.bool("strum_click", !mic);
        self.backing = c.bool("strum_backing", false);
        self.input_offset = c.f64("input_offset", 0.0);
    }

    fn save(&self, st: &mut AppState) {
        let c = &mut st.config;
        c.set("strum_progression_mode", self.progression_mode);
        c.set("strum_root", self.root as i64);
        c.set("strum_quality", self.quality as i64);
        c.set("strum_progression", self.progression as i64);
        c.set("strum_bars_per_chord", self.bars_per_chord as i64);
        c.set("strum_pattern", self.pattern as i64);
        c.set("strum_bpm", self.bpm);
        c.set("strum_tuning", self.tuning as i64);
        c.set("strum_click", self.click);
        c.set("strum_backing", self.backing);
        c.set("input_offset", (self.input_offset * 1000.0).round() / 1000.0);
        st.save_config();
    }

    fn strings(&self) -> &'static [i32; 6] {
        &TUNINGS[self.tuning].1
    }

    /// The chords, a loop of one each `bars_per_chord` bars.
    fn chords(&self) -> Vec<Chord> {
        if self.progression_mode {
            let (_, text, scale) = PROGRESSIONS[self.progression];
            parse_progression(text, self.root, scale).unwrap_or_default()
        } else {
            vec![Chord::new(self.root, QUALITIES[self.quality].0)]
        }
    }

    fn chord_at(&self, chords: &[Chord], bar: i64) -> Option<Chord> {
        if chords.is_empty() {
            return None;
        }
        let k = bar.max(0).div_euclid(self.bars_per_chord as i64) as usize;
        Some(chords[k % chords.len()])
    }

    fn engine(st: &AppState) -> Option<Arc<Engine>> {
        st.audio.as_ref().map(|a| a.engine.clone())
    }

    /// The click and the backing as chosen, on the engine.
    fn send_sound(&self, e: &Engine) {
        e.set_click(ClickParams { on: self.click, ..ClickParams::default() });
        e.set_backing(self.backing.then(|| BackingSpec {
            kind: BackingKind::Chords,
            key: self.root,
            scale: if self.progression_mode { PROGRESSIONS[self.progression].2 } else { Scale::Major },
            chords: self.chords(),
            beats_per_chord: 4 * self.bars_per_chord,
            bass: true,
        }));
    }

    fn start(&mut self, st: &AppState) {
        let Some(e) = Self::engine(st) else { return };
        if self.saved.is_none() {
            self.saved = Some(Saved {
                bpm: e.jam_tempo(),
                meter: e.jam().beats_per_bar,
                click: e.click(),
                drums: e.drums(),
                backing: e.backing(),
            });
        }
        e.set_jam_tempo(self.bpm);
        e.set_jam_meter(4);
        e.set_drums(DrumParams { on: false, ..e.drums() });
        self.send_sound(&e);
        self.reset();
        let _ = e.take_onsets();
        e.jam_play(1);
        self.running = true;
        self.awaiting_clock = true;
        self.last_frame = Some(watch(e));
    }

    /// Stop, and give the jam back as it was.
    pub fn stop(&mut self, st: &AppState) {
        self.running = false;
        self.last_frame = None;
        self.pending.clear();
        let Some(e) = Self::engine(st) else { return };
        e.jam_stop();
        if let Some(s) = self.saved.take() {
            e.set_backing(s.backing);
            e.set_click(s.click);
            e.set_drums(s.drums);
            e.set_jam_meter(s.meter);
            e.set_jam_tempo(s.bpm);
        }
    }

    fn reset(&mut self) {
        self.score = StrumScore::default();
        self.pending.clear();
        self.last_chord = None;
    }

    fn changed(&mut self, st: &mut AppState) {
        if self.running {
            self.stop(st);
            self.start(st);
        }
        self.save(st);
    }

    /// Every frame while running: strums against the beat, and the chords of those that have rung long enough.
    fn listen(&mut self, st: &AppState) {
        let Some(e) = Self::engine(st) else { return };
        let jam = e.jam();
        if self.awaiting_clock {
            // the count-in is the first thing the restarted clock shows
            if !(jam.running && jam.beat < 0.0) {
                let _ = e.take_onsets();
                return;
            }
            self.awaiting_clock = false;
        }
        if let Some(f) = &self.last_frame {
            f.store(qrs_audio::clock::now_ns(), Ordering::Relaxed);
        }
        // `watch` stopped it while the app was away: nothing played then is to be scored
        if !jam.running {
            self.stop(st);
            return;
        }
        let bpm = self.bpm;
        let beats_per_s = bpm / 60.0;
        let now = qrs_audio::clock::now_ns();
        let pattern = strokes(self.pattern);
        let chords = self.chords();
        let strings = *self.strings();
        for o in e.take_onsets() {
            let ago = now.saturating_sub(o.wall_ns) as f64 * 1e-9;
            let beat = jam.beat - ago * beats_per_s + self.input_offset * beats_per_s;
            let Some((bar, slot)) = self.score.strum(beat, bpm, &pattern) else { continue };
            // up strums often catch only the top strings: only the down strums' chords are judged
            if pattern[slot] != b'D' {
                continue;
            }
            let Some(sh) = self.chord_at(&chords, bar).and_then(|c| shape(c, &strings)) else { continue };
            let midis = sounding(&sh, &strings);
            let chord_key = bar.div_euclid(self.bars_per_chord as i64);
            let ringing = match &self.last_chord {
                Some((k, m)) if *k != chord_key => m.clone(),
                _ => Vec::new(),
            };
            let same = self.last_chord.as_ref().is_some_and(|(k, _)| *k == chord_key);
            let repeats: HashSet<i32> =
                midis.iter().copied().filter(|m| same || ringing.contains(m)).collect();
            self.last_chord = Some((chord_key, midis.clone()));
            self.pending.push(Pending {
                index: o.index,
                bar,
                slot,
                group: NoteGroup {
                    t: 0.0,
                    strings: (0..6).filter(|&s| sh[s] >= 0).map(|s| s as i32).collect(),
                    midis,
                    muted: false,
                    legato: false,
                    state: None,
                    dt: 0.0,
                    judged_wall: 0.0,
                    last_check: 0.0,
                    repeats,
                    ringing,
                },
            });
        }
        // the chords whose window has been recorded
        let cap = e.capture();
        let rate = cap.rate();
        let mut i = 0;
        while i < self.pending.len() {
            let w = detect::window_for(&self.pending[i].group, rate);
            let index = self.pending[i].index;
            if cap.written() < index + w as u64 {
                i += 1;
                continue;
            }
            let p = self.pending.remove(i);
            let (mut x, mut before) = (vec![0.0; w], vec![0.0; w]);
            if !cap.read(index, &mut x) {
                continue;
            }
            let had_before = index >= w as u64 && cap.read(index - w as u64, &mut before);
            let ok = detect::group_matches(
                &mut self.sp,
                &x,
                rate,
                &p.group,
                had_before.then_some(before.as_slice()),
            );
            self.score.chord_result(p.bar, p.slot, ok);
        }
        self.score.close(jam.beat, &pattern);
    }

    pub fn handle(&mut self, lv: &mut LibView, st: &mut AppState, ev: &Event) -> bool {
        let Event::Key { key, pressed: true, modifiers, .. } = ev else {
            return false;
        };
        if modifiers.command {
            return false;
        }
        match key {
            Key::Space => {
                if self.running {
                    self.stop(st);
                } else {
                    self.start(st);
                }
                return true;
            }
            Key::OpenBracket | Key::ArrowLeft => {
                self.pattern = (self.pattern + PATTERNS.len() - 1) % PATTERNS.len();
            }
            Key::CloseBracket | Key::ArrowRight => self.pattern = (self.pattern + 1) % PATTERNS.len(),
            Key::Minus => self.bpm = (self.bpm - 5.0).max(40.0),
            Key::Plus | Key::Equals => self.bpm = (self.bpm + 5.0).min(200.0),
            Key::Escape => {
                self.stop(st);
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
        if lv.view != View::Strum {
            self.stop(st); // the sidebar went somewhere else
            return;
        }
        if self.running {
            self.listen(st);
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        let mut changed = false;
        let gap = px(6.0);
        let row_h = px(40.0);

        // header: the title, then the two modes
        let hy = main.min.y + px(26.0);
        txt(&p, "Strumming", pos2(main.min.x, hy), 28.0, th.text, 800, Align2::LEFT_CENTER, None);
        let mut mx = main.max.x;
        for (on_prog, label) in [(true, "Progression"), (false, "One chord")] {
            let w = px(128.0);
            mx -= w;
            let r = Rect::from_min_size(pos2(mx, hy - px(20.0)), vec2(w, row_h));
            let on = self.progression_mode == on_prog;
            if button(ui, r, ("strum-mode", on_prog), label, "", ButtonStyle::active(on)).clicked() && !on {
                self.progression_mode = on_prog;
                changed = true;
            }
            mx -= gap;
        }

        let label_w = px(64.0);
        let x0 = main.min.x + label_w;
        let row_w = main.max.x - x0;
        let row_label = |y: f32, s: &str| {
            txt(&p, s, pos2(main.min.x, y + row_h / 2.0), 11.0, th.caption, 800, Align2::LEFT_CENTER, None);
        };
        let mut y = hy + px(34.0);

        // the chord's root, or the progression's key
        row_label(y, if self.progression_mode { "KEY" } else { "CHORD" });
        let w = (row_w - gap * 11.0) / 12.0;
        for k in 0..12u8 {
            let r = Rect::from_min_size(pos2(x0 + k as f32 * (w + gap), y), vec2(w, row_h));
            let on = self.root == k;
            if button(ui, r, ("strum-root", k), NAMES[k as usize], "", ButtonStyle::active(on)).clicked() && !on {
                self.root = k;
                changed = true;
            }
        }
        y += row_h + gap;

        // the chord's kind, or the progression
        if self.progression_mode {
            row_label(y, "CHORDS");
            let n = PROGRESSIONS.len() as f32 + 1.0;
            let w = (row_w - gap * (n - 1.0)) / n;
            for (i, (label, _, _)) in PROGRESSIONS.iter().enumerate() {
                let r = Rect::from_min_size(pos2(x0 + i as f32 * (w + gap), y), vec2(w, row_h));
                let on = self.progression == i;
                if button(ui, r, ("strum-prog", i), label, "", ButtonStyle::active(on).size(11.0)).clicked() && !on {
                    self.progression = i;
                    changed = true;
                }
            }
            let r = Rect::from_min_size(pos2(x0 + (n - 1.0) * (w + gap), y), vec2(w, row_h));
            let bars = if self.bars_per_chord == 1 { "1 bar each" } else { "2 bars each" };
            if button(ui, r, "strum-bars", bars, "", ButtonStyle::default().size(11.0)).clicked() {
                self.bars_per_chord = 3 - self.bars_per_chord;
                changed = true;
            }
        } else {
            row_label(y, "TYPE");
            let n = QUALITIES.len() as f32;
            let w = (row_w - gap * (n - 1.0)) / n;
            for (i, (_, label)) in QUALITIES.iter().enumerate() {
                let r = Rect::from_min_size(pos2(x0 + i as f32 * (w + gap), y), vec2(w, row_h));
                let on = self.quality == i;
                if button(ui, r, ("strum-quality", i), label, "", ButtonStyle::active(on)).clicked() && !on {
                    self.quality = i;
                    changed = true;
                }
            }
        }
        y += row_h + gap;

        // the 32 patterns
        row_label(y, "PATTERN");
        let ph = px(32.0);
        let w = (row_w - gap * 15.0) / 16.0;
        for i in 0..PATTERNS.len() {
            let (col, line) = (i % 16, i / 16);
            let r = Rect::from_min_size(
                pos2(x0 + col as f32 * (w + gap), y + line as f32 * (ph + gap)),
                vec2(w, ph),
            );
            let on = self.pattern == i;
            let style = if on { ButtonStyle::primary() } else { ButtonStyle::default() };
            if button(ui, r, ("strum-pattern", i), &(i + 1).to_string(), "", style.size(12.0)).clicked() && !on {
                self.pattern = i;
                changed = true;
            }
        }
        y += (ph + gap) * 2.0 + gap;

        let controls_h = px(118.0);
        let content = Rect::from_min_max(pos2(main.min.x, y), pos2(main.max.x, main.max.y - controls_h - px(10.0)));
        let chord_w = px(310.0);
        let lane = Rect::from_min_max(content.min, pos2(content.max.x - chord_w - px(10.0), content.max.y));
        let chord_card = Rect::from_min_max(pos2(content.max.x - chord_w, content.min.y), content.max);
        let jam = Self::engine(st).filter(|_| self.running).map(|e| e.jam());
        self.draw_lane(&p, lane, jam.as_ref().map(|j| j.beat));
        self.draw_chords(&p, chord_card, jam.as_ref().map(|j| j.beat));
        let controls = Rect::from_min_max(pos2(main.min.x, main.max.y - controls_h), main.max);
        changed |= self.draw_controls(ui, st, &p, controls);

        if changed {
            self.changed(st);
        }
    }

    /// The bar's eight slots: the arrows, the count, the moving line, and how each strum went.
    fn draw_lane(&self, p: &Painter, area: Rect, beat: Option<f64>) {
        let th = t();
        qrs_ui::paint::card(p, area, px(18.0), false);
        let pattern = strokes(self.pattern);
        let inner = area.shrink2(vec2(px(20.0), px(14.0)));
        let col = inner.width() / 8.0;
        let x = |e: f32| inner.min.x + col * (e + 0.5);
        let room = inner.height() - px(94.0);
        let arrow_h = room.clamp(px(40.0), px(240.0));
        let top = inner.min.y + px(26.0) + (room - arrow_h).max(0.0) / 2.0;
        txt(&p, &format!("PATTERN {}", self.pattern + 1), pos2(inner.min.x, inner.min.y + px(8.0)), 11.0, th.caption, 800, Align2::LEFT_CENTER, None);
        let bar = beat.map(|b| (b / 4.0).floor() as i64);
        let shown = bar.and_then(|b| self.score.bar(b));
        for s in 0..8 {
            let cx = x(s as f32);
            let result = shown.and_then(|b| b.slots[s]);
            let past = bar.is_some_and(|br| br >= 0) && beat.is_some_and(|b| b.rem_euclid(4.0) * 2.0 > s as f64 + 0.5);
            let ink = match (pattern[s] != b'.', result) {
                (_, Some(h)) if h.chord == Some(false) => AMBER,
                (_, Some(h)) if h.ms.abs() > TIGHT_MS => AMBER,
                (_, Some(_)) => GREEN,
                (true, None) if past => RED,
                _ if pattern[s] == b'D' => th.text,
                _ => th.caption,
            };
            let a = pos2(cx, top);
            let b = pos2(cx, top + arrow_h);
            match pattern[s] {
                b'D' => arrow(p, a, b, px(6.0), ink),
                b'U' => arrow(p, b, a, px(3.0), ink),
                _ => {}
            }
            let on_beat = s % 2 == 0;
            txt(p, COUNTS[s], pos2(cx, top + arrow_h + px(18.0)), if on_beat { 17.0 } else { 13.0 }, if on_beat { th.text } else { th.caption }, 800, Align2::CENTER_CENTER, None);
            if let Some(h) = result {
                let note = match h.chord {
                    Some(false) => "chord?".to_string(),
                    _ => format!("{:+.0}", h.ms),
                };
                txt(p, &note, pos2(cx, top + arrow_h + px(42.0)), 14.0, ink, 700, Align2::CENTER_CENTER, None);
            }
        }
        if let Some(b) = shown {
            for e in &b.extras {
                let c = pos2(x(*e), top + arrow_h / 2.0);
                let d = px(8.0);
                p.line_segment([c - vec2(d, d), c + vec2(d, d)], Stroke::new(px(3.0), RED));
                p.line_segment([c - vec2(d, -d), c + vec2(d, -d)], Stroke::new(px(3.0), RED));
            }
        }
        // the moving line (eighths are centred in their columns)
        if let Some(b) = beat.filter(|b| *b >= 0.0) {
            let lx = x((b.rem_euclid(4.0) * 2.0) as f32);
            p.line_segment([pos2(lx, top - px(8.0)), pos2(lx, top + arrow_h + px(30.0))], Stroke::new(px(3.0), th.accent));
        }
    }

    /// The chord to play now and the next, with where the fingers go; in the count-in, the count.
    fn draw_chords(&self, p: &Painter, area: Rect, beat: Option<f64>) {
        let th = t();
        qrs_ui::paint::card(p, area, px(18.0), false);
        let chords = self.chords();
        let strings = self.strings();
        let b = beat.unwrap_or(0.0);
        let bar = (b / 4.0).floor() as i64;
        let now = self.chord_at(&chords, bar.max(0));
        let next_bar = (bar.max(0).div_euclid(self.bars_per_chord as i64) + 1) * self.bars_per_chord as i64;
        let next = self.chord_at(&chords, next_bar);
        let pad = px(14.0);
        let half = (area.width() - pad * 2.0) / 2.0;
        let head = if b < 0.0 && beat.is_some() {
            format!("GET READY: {}", (-b).ceil() as i32)
        } else {
            "NOW".to_string()
        };
        txt(p, &head, pos2(area.min.x + pad, area.min.y + px(18.0)), 11.0, if b < 0.0 { th.accent } else { th.caption }, 800, Align2::LEFT_CENTER, None);
        let dia_h = area.height() - px(82.0);
        if let Some(c) = now {
            txt(p, &c.name(), pos2(area.min.x + pad, area.min.y + px(46.0)), 26.0, th.accent, 800, Align2::LEFT_CENTER, None);
            let r = Rect::from_min_size(pos2(area.min.x + pad, area.min.y + px(68.0)), vec2(half - px(10.0), dia_h));
            draw_shape(p, r, shape(c, strings), th.text);
        }
        if self.progression_mode && let Some(c) = next {
            let x = area.min.x + pad + half;
            txt(p, "NEXT", pos2(x, area.min.y + px(18.0)), 11.0, th.caption, 800, Align2::LEFT_CENTER, None);
            txt(p, &c.name(), pos2(x, area.min.y + px(46.0)), 20.0, th.text, 800, Align2::LEFT_CENTER, None);
            let r = Rect::from_min_size(pos2(x, area.min.y + px(68.0)), vec2(half - px(10.0), dia_h));
            draw_shape(p, r, shape(c, strings), th.caption);
        }
    }

    /// Start, tempo, sound, timing, and how it's going. Returns whether a choice changed.
    fn draw_controls(&mut self, ui: &Ui, st: &mut AppState, p: &Painter, area: Rect) -> bool {
        let th = t();
        qrs_ui::paint::card(p, area, px(18.0), false);
        let pad = px(14.0);
        let gap = px(6.0);
        let bh = px(40.0);
        let y = area.min.y + px(12.0);
        let mut x = area.min.x + pad;
        let mut changed = false;
        let mut place = |w: f32| {
            let r = Rect::from_min_size(pos2(x, y), vec2(w, bh));
            x += w + gap;
            r
        };
        let label = if self.running { "Stop" } else { "Start" };
        if button(ui, place(px(104.0)), "strum-start", label, cap("Space"), ButtonStyle::primary()).clicked() {
            if self.running {
                self.stop(st);
            } else {
                self.start(st);
            }
        }
        if button(ui, place(px(40.0)), "strum-slower", "−", "", ButtonStyle::default()).clicked() {
            self.bpm = (self.bpm - 5.0).max(40.0);
            changed = true;
        }
        button(ui, place(px(92.0)), "strum-bpm", &format!("{:.0} bpm", self.bpm), "", ButtonStyle::value());
        if button(ui, place(px(40.0)), "strum-faster", "+", "", ButtonStyle::default()).clicked() {
            self.bpm = (self.bpm + 5.0).min(200.0);
            changed = true;
        }
        if button(ui, place(px(84.0)), "strum-click", "Click", "", ButtonStyle::active(self.click)).clicked() {
            self.click = !self.click;
            changed = true;
        }
        if button(ui, place(px(96.0)), "strum-backing", "Backing", "", ButtonStyle::active(self.backing)).clicked() {
            self.backing = !self.backing;
            changed = true;
        }
        if button(ui, place(px(126.0)), "strum-tuning", TUNINGS[self.tuning].0, "", ButtonStyle::default().size(12.0)).clicked() {
            self.tuning = (self.tuning + 1) % TUNINGS.len();
            changed = true;
        }
        // the guitar's timing, shared with the player (Ctrl+, and Ctrl+. there)
        let mut nudge = 0.0;
        if button(ui, place(px(40.0)), "strum-earlier", "‹", "", ButtonStyle::default()).clicked() {
            nudge = -0.01;
        }
        button(ui, place(px(84.0)), "strum-offset", &format!("{:+.0} ms", self.input_offset * 1000.0), "", ButtonStyle::value());
        if button(ui, place(px(40.0)), "strum-later", "›", "", ButtonStyle::default()).clicked() {
            nudge = 0.01;
        }
        if nudge != 0.0 {
            self.input_offset += nudge;
            self.save(st); // (no restart for this)
        }

        let s = &self.score;
        let tx = area.min.x + pad;
        let ty = y + bh + px(16.0);
        let input = st.audio.as_ref().is_some_and(|a| a.engine.status().input.is_some());
        let total = s.hits + s.missed;
        let line = if !input {
            "No guitar input: choose one in Settings › Audio".to_string()
        } else if s.bars_done == 0 && s.hits == 0 {
            "Press Start: a bar's count, then play. The direction arrows are your guide; only the timing and the chord are checked.".to_string()
        } else {
            let timing = match s.mean_ms() {
                Some(m) if m > 8.0 => format!(" · {m:.0} ms late on average"),
                Some(m) if m < -8.0 => format!(" · {:.0} ms early on average", -m),
                Some(_) => " · on the beat on average".to_string(),
                None => String::new(),
            };
            let chords = if s.chords_judged > 0 {
                format!(" · chord right {}/{}", s.chords_judged - s.wrong_chords, s.chords_judged)
            } else {
                String::new()
            };
            format!(
                "Clean bars {}/{} (streak {}) · strums {}/{} · {} extra{}{}",
                s.bars_clean, s.bars_done, s.streak, s.hits, total, s.extra, timing, chords
            )
        };
        txt(p, &line, pos2(tx, ty), 13.0, th.text, 600, Align2::LEFT_CENTER, Some(area.width() - pad * 2.0));
        if st.mic_input() && (self.click || self.backing) {
            txt(p, "On the built-in mic, sound from the speakers can count as strums: use headphones, or turn Click and Backing off and follow the moving line.", pos2(tx, ty + px(22.0)), 11.0, AMBER, 600, Align2::LEFT_CENTER, Some(area.width() - pad * 2.0));
        }
        changed
    }
}

fn arrow(p: &Painter, from: Pos2, to: Pos2, w: f32, c: Color32) {
    let d = (to - from).normalized();
    let head = w * 2.6;
    let side = vec2(-d.y, d.x) * head * 0.8;
    p.line_segment([from, to - d * head], Stroke::new(w, c));
    p.add(egui::Shape::convex_polygon(vec![to, to - d * head + side, to - d * head - side], c, Stroke::NONE));
}

/// A chord box: strings upright (low on the left), the nut or the fret it starts at, a dot per finger.
fn draw_shape(p: &Painter, r: Rect, sh: Option<[i32; 6]>, ink: Color32) {
    let Some(sh) = sh else { return };
    let top_row = px(14.0);
    let grid = Rect::from_min_max(pos2(r.min.x + px(18.0), r.min.y + top_row), pos2(r.max.x - px(6.0), r.max.y - px(2.0)));
    let high = sh.iter().copied().max().unwrap_or(0);
    let low = sh.iter().copied().filter(|&f| f > 0).min().unwrap_or(1);
    let first = if high <= 4 { 1 } else { low };
    let rows = 4;
    let sx = |s: usize| grid.min.x + grid.width() * s as f32 / 5.0;
    let fy = |f: i32| grid.min.y + grid.height() * f as f32 / rows as f32;
    for s in 0..6 {
        p.line_segment([pos2(sx(s), grid.min.y), pos2(sx(s), grid.max.y)], Stroke::new(px(1.2), ink));
    }
    for f in 0..=rows {
        let w = if f == 0 && first == 1 { px(4.0) } else { px(1.2) };
        p.line_segment([pos2(grid.min.x, fy(f)), pos2(grid.max.x, fy(f))], Stroke::new(w, ink));
    }
    if first > 1 {
        txt(p, &first.to_string(), pos2(r.min.x + px(7.0), fy(0) + grid.height() / 8.0), 11.0, ink, 700, Align2::CENTER_CENTER, None);
    }
    let dot = (grid.width() / 5.0 * 0.36).min(grid.height() / 8.0);
    for s in 0..6 {
        match sh[s] {
            -1 => {
                txt(p, "×", pos2(sx(s), r.min.y + px(5.0)), 12.0, ink, 700, Align2::CENTER_CENTER, None);
            }
            0 => {
                p.circle_stroke(pos2(sx(s), r.min.y + px(5.0)), px(4.0), Stroke::new(px(1.4), ink));
            }
            f => {
                p.circle_filled(pos2(sx(s), fy(f - first) + grid.height() / 8.0), dot, ink);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DROP_D: [i32; 6] = [38, 45, 50, 55, 59, 64];

    #[test]
    fn the_chart_as_written() {
        assert_eq!(PATTERNS.len(), 32);
        for p in PATTERNS {
            assert_eq!(p.len(), 8, "{p}");
            for (i, c) in p.bytes().enumerate() {
                // downs on the beat, ups on the "and", as the chart has them
                assert!(c == b'.' || c == if i % 2 == 0 { b'D' } else { b'U' }, "{p}");
            }
        }
        assert_eq!(PATTERNS[0], "D.D.D.D.");
        assert_eq!(PATTERNS[12], "D.DUD.D.");
        assert_eq!(PATTERNS[13], ".U.U.U.U");
        assert_eq!(PATTERNS[31], ".UDU.UD.");
    }

    #[test]
    fn every_chord_has_a_shape_with_only_its_notes_and_its_root() {
        for (q, _) in QUALITIES {
            for root in 0..12u8 {
                let c = Chord::new(root, q);
                for strings in [&TUNINGS[0].1, &TUNINGS[1].1, &TUNINGS[2].1, &DROP_D] {
                    let sh = shape(c, strings).unwrap_or_else(|| panic!("{}: no shape", c.name()));
                    let notes = sounding(&sh, strings);
                    assert!(notes.len() >= 2, "{} {sh:?}", c.name());
                    let pcs: Vec<u8> = notes.iter().map(|m| m.rem_euclid(12) as u8).collect();
                    for pc in &pcs {
                        assert!(c.tones().contains(pc), "{} {sh:?} has {pc} on {strings:?}", c.name());
                    }
                    assert!(pcs.contains(&root), "{} {sh:?} has no root on {strings:?}", c.name());
                    let frets: Vec<i32> = sh.iter().copied().filter(|&f| f > 0).collect();
                    if let (Some(lo), Some(hi)) = (frets.iter().min(), frets.iter().max()) {
                        assert!(hi - lo <= 4, "{} {sh:?} is a stretch", c.name());
                    }
                }
            }
        }
    }

    #[test]
    fn the_progressions_read_as_chords() {
        let names = |i: usize, key: u8| -> Vec<String> {
            let (_, text, scale) = PROGRESSIONS[i];
            parse_progression(text, key, scale).unwrap().iter().map(Chord::name).collect()
        };
        assert_eq!(names(1, 0), ["C", "G", "Am", "F"]);
        assert_eq!(names(4, 0), ["Dm", "G", "C", "C"]);
        assert_eq!(names(6, 9), ["Am", "F", "C", "G"]);
        assert_eq!(names(5, 9)[..5], ["A7", "A7", "A7", "A7", "D7"]);
        assert_eq!(names(5, 9).len(), 12);
        for i in 0..PROGRESSIONS.len() {
            for key in 0..12 {
                let (_, text, scale) = PROGRESSIONS[i];
                for c in parse_progression(text, key, scale).unwrap() {
                    assert!(shape(c, &STD).is_some(), "{}", c.name());
                }
            }
        }
    }

    #[test]
    fn strums_on_time_fill_their_slots_and_a_bar_is_clean() {
        let pat = strokes(1); // D.D.D.DU
        let mut s = StrumScore::default();
        for beat in [0.0, 1.0, 2.0, 3.0, 3.5] {
            assert!(s.strum(beat, 60.0, &pat).is_some());
        }
        s.close(4.6, &pat);
        assert_eq!((s.bars_done, s.bars_clean, s.hits, s.missed, s.extra), (1, 1, 5, 0, 0));
    }

    #[test]
    fn a_late_strum_says_how_late_and_early_ones_reach_the_next_bar() {
        let pat = strokes(0);
        let mut s = StrumScore::default();
        s.strum(1.05, 60.0, &pat); // 50 ms late at 60 bpm
        let h = s.bar(0).unwrap().slots[2].unwrap();
        assert!((h.ms - 50.0).abs() < 1.0);
        s.strum(3.97, 60.0, &pat); // 30 ms early for bar 1's beat 1
        assert!((s.bar(1).unwrap().slots[0].unwrap().ms + 30.0).abs() < 1.0);
        assert!(s.mean_ms().unwrap() > 0.0);
    }

    #[test]
    fn a_missed_slot_and_an_extra_strum_spoil_the_bar() {
        let pat = strokes(0); // the four beats
        let mut s = StrumScore::default();
        for beat in [0.0, 1.0, 2.0] {
            s.strum(beat, 60.0, &pat);
        }
        s.close(4.6, &pat);
        assert_eq!((s.missed, s.bars_clean, s.streak), (1, 0, 0));
        // bar 1: all there, and one on an "and" that isn't in the pattern
        for beat in [4.0, 5.0, 6.0, 7.0, 7.5] {
            s.strum(beat, 60.0, &pat);
        }
        s.close(8.6, &pat);
        assert_eq!((s.extra, s.bars_clean), (1, 0));
    }

    #[test]
    fn the_count_in_doesnt_count_and_a_double_detection_isnt_extra() {
        let pat = strokes(0);
        let mut s = StrumScore::default();
        assert!(s.strum(-1.0, 60.0, &pat).is_none());
        assert_eq!((s.hits, s.extra), (0, 0));
        s.strum(0.0, 60.0, &pat);
        s.strum(0.04, 60.0, &pat); // 40 ms on: the same strum seen twice
        assert_eq!((s.hits, s.extra), (1, 0));
    }

    #[test]
    fn a_wrong_chord_spoils_the_bar() {
        let pat = strokes(16); // beats 1 and 3
        let mut s = StrumScore::default();
        let (bar, slot) = s.strum(0.0, 60.0, &pat).unwrap();
        s.strum(2.0, 60.0, &pat);
        s.chord_result(bar, slot, false);
        s.close(4.6, &pat);
        assert_eq!((s.bars_clean, s.wrong_chords, s.chords_judged), (0, 1, 1));
    }
}
