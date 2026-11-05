//! Note detection and scoring against the chart. Port of rsplay/detect.py (same thresholds).
//!
//! Each note (or chord) in the chart becomes a NoteGroup. A pluck is found by the engine's onset detector; the
//! audio just after it is checked for the expected pitches (fundamental or 2nd harmonic of every expected note).
//! Hammer-ons, pull-offs and taps don't make a clear pluck, so for those the live signal is checked directly.
//! A chord (D46, new in quickRS) counts with half its notes, as RS's do: see `need` and `group_matches_in`.
//!
//! Judgement, relative to the chart time:
//!   |dt| <= PERFECT   hit   (green)
//!   dt  >  PERFECT    late  (yellow)
//!   dt  < -PERFECT    early (yellow)
//!   nothing by +LATE  miss  (red)
//!
//! Note by note (P15.2, new in quickRS): the player holds the song at a note still to be played and `wait_on`
//! checks every pluck against that note alone, whenever it came, until one is right.
//!
//! Bass mode (P32.3, D89): `Scorer::set_instrument` with a bass reads every note over `window_for_inst`'s bass
//! window, judges a single note by the chord's tests (`explain_bass`), and on a guitar part also takes the note an
//! octave down (a bass's 2nd harmonic is the guitar's note).
//! A guitar on a bass part needs nothing new: its note an octave up is the bass note's 2nd harmonic, which
//! `notes_present` already takes. With a guitar (the default) nothing here differs from before.

use std::collections::{BTreeSet, HashSet};

use qrs_formats::model::{Arrangement, Note, Techs};

use crate::instrument::Instrument;
use crate::pitch::midi_to_hz;
use crate::spectrum::{Spectra, median};

pub const EARLY: f64 = 0.20;
pub const LATE: f64 = 0.35;
pub const PERFECT: f64 = 0.10;
/// Seconds after the pluck before analysing (skips the pick noise).
pub const ATTACK_SKIP: f64 = 0.015;

/// The score multiplier for a streak: x2 at 10 notes, x3 at 20, x4 from 30 (the streak tiers' first steps, D113).
pub fn multiplier(streak: u32) -> u32 {
    (1 + streak / 10).min(4)
}
const LEGATO: Techs = Techs(Techs::HAMMER_ON.0 | Techs::PULL_OFF.0 | Techs::TAP.0);
/// Natural harmonic fret → semitones above the open string.
const HARMONIC_SEMITONES: [(i32, i32); 9] = [
    (12, 12),
    (7, 19),
    (19, 19),
    (5, 24),
    (24, 24),
    (4, 28),
    (9, 28),
    (16, 28),
    (3, 31),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Judgement {
    Hit,
    Late,
    Early,
    Miss,
}

impl Judgement {
    pub fn points(self) -> u32 {
        match self {
            Judgement::Hit => 100,
            Judgement::Late | Judgement::Early => 50,
            Judgement::Miss => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteGroup {
    pub t: f64,
    pub midis: Vec<i32>,
    pub strings: Vec<i32>,
    pub muted: bool,
    pub legato: bool,
    pub state: Option<Judgement>,
    pub dt: f64,
    /// Wall time (seconds, the app's clock) of the judgement, for hit/miss animations.
    pub judged_wall: f64,
    pub last_check: f64,
    /// Pitches one of the previous two groups also had.
    pub repeats: HashSet<i32>,
    /// Pitches of the previous two groups (may still be sounding).
    pub ringing: Vec<i32>,
}

/// The pitch a chart note sounds (MIDI; a natural harmonic sounds above its fret).
pub fn note_midi(opens: &[i32], n: &Note) -> i32 {
    let base = opens
        .get(n.string.max(0) as usize)
        .or(opens.last())
        .copied()
        .unwrap_or(40);
    if n.has(Techs::HARMONIC)
        && let Some(&(_, s)) = HARMONIC_SEMITONES.iter().find(|(f, _)| *f == n.fret)
    {
        return base + s;
    }
    base + n.fret
}

pub fn build_groups(arr: &Arrangement) -> Vec<NoteGroup> {
    let opens = arr.open_string_midi();
    let midi = |n: &Note| note_midi(&opens, n);
    let mut events: Vec<(f64, Vec<&Note>, Techs)> = Vec::new();
    for n in &arr.notes {
        events.push((n.time, vec![n], n.techniques));
    }
    for c in &arr.chords {
        if !c.notes.is_empty() {
            events.push((c.time, c.notes.iter().collect(), c.techniques));
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut groups: Vec<NoteGroup> = Vec::new();
    for (t, notes, techs) in events {
        let notes: Vec<&Note> = notes.into_iter().filter(|n| !n.has(Techs::IGNORE)).collect();
        if notes.is_empty() || techs.has(Techs::IGNORE) {
            continue;
        }
        let merge = groups
            .last()
            .is_some_and(|g| (g.t - t).abs() < 0.015 && g.state.is_none());
        if !merge {
            groups.push(NoteGroup {
                t,
                midis: vec![],
                strings: vec![],
                muted: true,
                legato: false,
                state: None,
                dt: 0.0,
                judged_wall: 0.0,
                last_check: -1.0,
                repeats: HashSet::new(),
                ringing: vec![],
            });
        }
        let g = groups.last_mut().unwrap();
        for n in notes {
            let tech = n.techniques | techs;
            if !tech.has(Techs::FRET_HAND_MUTE) {
                g.muted = false;
                g.midis.push(midi(n));
            }
            if !(tech & LEGATO).is_empty() {
                g.legato = true;
            }
            g.strings.push(n.string);
        }
    }
    for i in 0..groups.len() {
        let recent: BTreeSet<i32> = groups[i.saturating_sub(2)..i]
            .iter()
            .flat_map(|r| r.midis.iter().copied())
            .collect();
        let own: HashSet<i32> = groups[i].midis.iter().copied().collect();
        groups[i].ringing = recent.iter().filter(|m| !own.contains(m)).copied().collect();
        groups[i].repeats = if i == 0 {
            HashSet::new()
        } else {
            own.iter().filter(|m| recent.contains(m)).copied().collect()
        };
    }
    groups
}

fn peak(mag: &[f64], binhz: f64, f: f64, tol: f64) -> f64 {
    let lo = (f * (1.0 - tol) / binhz) as usize;
    let hi = (f * (1.0 + tol) / binhz) as usize + 1;
    if hi >= mag.len() || lo < 1 {
        return 0.0;
    }
    mag[lo..hi].iter().copied().fold(0.0, f64::max)
}

/// As `peak`, but only a peak that is a local maximum (the band's loudest bin is higher than the bins either side of
/// it, even outside the band): a louder note a semitone away has a skirt reaching into the band (an 85 ms window's
/// main lobe is +-24 Hz, more than a semitone below 400 Hz), a skirt that `peak` alone takes for the note.
fn sharp_peak(mag: &[f64], binhz: f64, f: f64, tol: f64) -> f64 {
    let lo = (f * (1.0 - tol) / binhz) as usize;
    let hi = (f * (1.0 + tol) / binhz) as usize + 1;
    if hi + 1 >= mag.len() || lo < 2 {
        return 0.0;
    }
    let (i, &p) = mag[lo..hi]
        .iter()
        .enumerate()
        .fold((0, &0.0), |b, (i, v)| if *v > *b.1 { (i, v) } else { b });
    let i = lo + i;
    if mag[i - 1] <= p && mag[i + 1] <= p {
        p
    } else {
        0.0
    }
}

/// How many of the expected notes are audibly present in `x`. With `before` (the audio just before the pluck), a
/// note only counts if it got louder at the pluck, so a previous note still ringing doesn't count as the new one;
/// pitches in `repeats` (the chart plays the same pitch again) skip that test.
pub fn notes_present(
    sp: &mut Spectra,
    x: &[f32],
    rate: u32,
    midis: &[i32],
    before: Option<&[f32]>,
    repeats: &HashSet<i32>,
) -> usize {
    let (mag, binhz) = sp.spectrum(x, rate);
    let prev = before
        .filter(|b| b.len() == x.len())
        .map(|b| sp.spectrum(b, rate).0);
    notes_present_in(&mag, binhz, midis, prev.as_deref(), repeats)
}

/// `notes_present` on spectra already made (`Spectra::spectrum` of the window and of the one before it).
pub fn notes_present_in(
    mag: &[f64],
    binhz: f64,
    midis: &[i32],
    prev: Option<&[f64]>,
    repeats: &HashSet<i32>,
) -> usize {
    let Some(level) = Level::of(mag, binhz) else {
        return 0;
    };
    midis
        .iter()
        .filter(|&&m| level.present(mag, binhz, m, prev, repeats.contains(&m), true))
        .count()
}

/// Whether one expected pitch (fundamental or 2nd harmonic) stands out of `mag` (and, with `prev` and not a
/// `repeat`, got louder than it was).
pub fn note_present_in(mag: &[f64], binhz: f64, m: i32, prev: Option<&[f64]>, repeat: bool) -> bool {
    Level::of(mag, binhz).is_some_and(|l| l.present(mag, binhz, m, prev, repeat, true))
}

/// As `note_present_in`, by the fundamental alone (a chord's note an octave or a fifth above another has its 2nd
/// harmonic among the other's partials, so this is the test of whether that string sounded).
pub fn fundamental_present_in(mag: &[f64], binhz: f64, m: i32, prev: Option<&[f64]>, repeat: bool) -> bool {
    Level::of(mag, binhz).is_some_and(|l| l.present(mag, binhz, m, prev, repeat, false))
}

/// A spectrum's strongest peak and median (35 Hz to 5 kHz), for the presence test.
struct Level {
    reference: f64,
    floor: f64,
}

impl Level {
    fn of(mag: &[f64], binhz: f64) -> Option<Level> {
        let lo = (35.0 / binhz) as usize;
        let hi = ((5000.0 / binhz) as usize).min(mag.len());
        let band = &mag[lo.min(hi)..hi];
        let reference = band.iter().copied().fold(0.0, f64::max);
        (reference > 0.0).then(|| Level {
            reference,
            floor: median(band),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn present_as(
        &self,
        mag: &[f64],
        binhz: f64,
        m: i32,
        prev: Option<&[f64]>,
        repeat: bool,
        second: bool,
        sharp: bool,
    ) -> bool {
        let f = midi_to_hz(m as f64);
        let at = |v: &[f64], f: f64| {
            if sharp {
                sharp_peak(v, binhz, f, 0.03)
            } else {
                peak(v, binhz, f, 0.03)
            }
        };
        let p = at(mag, f).max(if second { at(mag, 2.0 * f) } else { 0.0 });
        let h2 = |v: &[f64]| {
            if second {
                peak(v, binhz, 2.0 * f, 0.03)
            } else {
                0.0
            }
        };
        if p < 0.12 * self.reference || p < 4.0 * self.floor {
            return false;
        }
        if let Some(prev) = prev
            && !repeat
        {
            let q = peak(prev, binhz, f, 0.03).max(h2(prev));
            if p < 1.2 * q {
                return false;
            }
        }
        true
    }

    fn present(
        &self,
        mag: &[f64],
        binhz: f64,
        m: i32,
        prev: Option<&[f64]>,
        repeat: bool,
        second: bool,
    ) -> bool {
        self.present_as(mag, binhz, m, prev, repeat, second, false)
    }
}

/// Share of the strong peaks the expected notes may fail to explain.
pub const FOREIGN_MAX: f64 = 0.2;
const PEAK_FRAC: f64 = 0.15;
const HARMONIC_TOL: f64 = 0.025;
/// Hz; wrong notes show up clearly below this.
const FOREIGN_TOP: f64 = 1200.0;

/// Share (by magnitude) of the strong spectral peaks that are not a harmonic of any expected note (or a sum of
/// two expected fundamentals, which distortion makes). A wrong fret or chord shape puts strong peaks there.
pub fn foreign_share(sp: &mut Spectra, x: &[f32], rate: u32, midis: &[i32], ringing: &[i32]) -> f64 {
    let (mag, binhz) = sp.spectrum(x, rate);
    foreign_share_in(&mag, binhz, midis, ringing)
}

/// `foreign_share` on a spectrum already made.
pub fn foreign_share_in(mag: &[f64], binhz: f64, midis: &[i32], ringing: &[i32]) -> f64 {
    foreign_impl(mag, binhz, midis, ringing, None)
}

/// The foreign share; with `prev` (the spectrum before the pluck), a peak that only a ringing note explains counts
/// as foreign if it got louder at the pluck (it was struck again: the previous chord played instead of this one).
fn foreign_impl(mag: &[f64], binhz: f64, midis: &[i32], ringing: &[i32], prev: Option<&[f64]>) -> f64 {
    let lo = (60.0 / binhz) as usize;
    let hi = ((FOREIGN_TOP / binhz) as usize).min(mag.len());
    if hi <= lo + 2 {
        return 0.0;
    }
    let band = &mag[lo..hi];
    let bmax = band.iter().copied().fold(0.0, f64::max);
    if bmax <= 0.0 {
        return 0.0;
    }
    let floor = median(band);
    let thresh = (PEAK_FRAC * bmax).max(8.0 * floor);
    let mut idx: Vec<usize> = (1..band.len() - 1)
        .filter(|&i| band[i] > band[i - 1] && band[i] >= band[i + 1] && band[i] >= thresh)
        .collect();
    if idx.is_empty() {
        return 0.0;
    }
    // the 12 strongest (numpy argsort reversed: ties keep the later index first)
    idx.sort_by(|&a, &b| band[b].total_cmp(&band[a]).then(b.cmp(&a)));
    idx.truncate(12);
    let own = midis.len();
    let base: Vec<f64> = midis
        .iter()
        .chain(ringing)
        .map(|&m| midi_to_hz(m as f64))
        .collect();
    // (centre, tolerance, made only by ringing notes)
    let mut centres: Vec<(f64, f64, bool)> = Vec::new();
    for (k, &f0) in base.iter().enumerate() {
        let hs = (FOREIGN_TOP * 1.05 / f0) as usize;
        for h in 1..=hs {
            let f = h as f64 * f0;
            centres.push((f, (f * HARMONIC_TOL).min(0.25 * f0), k >= own));
        }
    }
    for (ka, &a) in base.iter().enumerate() {
        for (kb, &b) in base.iter().enumerate() {
            centres.push((a + b, (a + b) * HARMONIC_TOL, ka >= own && kb >= own));
        }
    }
    let (mut total, mut foreign) = (0.0, 0.0);
    for &i in &idx {
        let f = (i + lo) as f64 * binhz;
        let w = band[i];
        total += w;
        let mut by_own = false;
        let mut by_ringing = false;
        for &(c, t, rung) in &centres {
            if (f - c).abs() < t {
                if rung {
                    by_ringing = true;
                } else {
                    by_own = true;
                }
            }
        }
        let explained = by_own
            || (by_ringing
                && prev.is_none_or(|prev| {
                    let j = i + lo;
                    let q = prev[j.saturating_sub(2)..(j + 3).min(prev.len())]
                        .iter()
                        .copied()
                        .fold(0.0, f64::max);
                    w < 1.5 * q
                }));
        if !explained {
            foreign += w;
        }
    }
    if total > 0.0 { foreign / total } else { 0.0 }
}

pub fn group_matches(sp: &mut Spectra, x: &[f32], rate: u32, g: &NoteGroup, before: Option<&[f32]>) -> bool {
    let (mag, binhz) = sp.spectrum(x, rate);
    let prev = before
        .filter(|b| b.len() == x.len())
        .map(|b| sp.spectrum(b, rate).0);
    group_matches_in(&mag, binhz, g, prev.as_deref())
}

/// How many of a group's pitches must sound (D46). A single note: itself; two (a double stop, a power chord's root
/// and fifth): both; a chord of three or more: half of them, at least two. RS counts a chord whose strings
/// weren't all struck (its developers eased off requiring every note; players get chords with a few strings), and
/// a strum's upstroke often catches only the top two or three strings.
pub fn need(g: &NoteGroup) -> usize {
    let n = g.midis.len();
    if n <= 2 { n.max(1) } else { n.div_ceil(2).max(2) }
}

/// `group_matches` on spectra already made. A single note (port of rsplay/detect.py): present (and got louder),
/// and no more than `FOREIGN_MAX` of the strong peaks unexplained. A chord (D46): `need` of its pitches present
/// (and got louder, unless the chord before had them), each a real peak rather than the skirt of a note a semitone
/// away, and no more than `FOREIGN_MAX` of the strong peaks unexplained, where a note of the chords before only
/// explains a peak that didn't get louder at the strum.
pub fn group_matches_in(mag: &[f64], binhz: f64, g: &NoteGroup, prev: Option<&[f64]>) -> bool {
    explain(mag, binhz, g, prev).ok
}

/// What `group_matches_in` saw (and the chord bench prints: `quickrs-cli chords --why`).
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub expected: usize,
    pub need: usize,
    /// Expected pitches present (and got louder).
    pub present: usize,
    pub foreign: f64,
    pub ok: bool,
}

impl Verdict {
    pub fn describe(&self) -> String {
        format!(
            "{} {}/{} present, need {}, foreign {:.2}",
            if self.ok { "yes" } else { "no" },
            self.present,
            self.expected,
            self.need,
            self.foreign
        )
    }
}

/// The group's pitches that are present (and got louder, unless a repeat), as `group_matches_in` counts them: for
/// a chord each must be a real peak.
pub fn group_present_in(mag: &[f64], binhz: f64, g: &NoteGroup, prev: Option<&[f64]>) -> Vec<i32> {
    group_present_as(mag, binhz, g, prev, g.midis.len() >= 2)
}

/// `group_present_in`, each pitch a real peak when `chord` (a chord's test; a bass's for every note).
fn group_present_as(mag: &[f64], binhz: f64, g: &NoteGroup, prev: Option<&[f64]>, chord: bool) -> Vec<i32> {
    match Level::of(mag, binhz) {
        None => vec![],
        Some(level) => g
            .midis
            .iter()
            .copied()
            .filter(|&m| level.present_as(mag, binhz, m, prev, g.repeats.contains(&m), true, chord))
            .collect(),
    }
}

/// Whether `present` (of `group_present_in`) is enough of the group: `need` of its pitches, and for a chord of
/// two or more pitch classes, two of them (a power chord's root and its octave alone are one note, not the chord).
pub fn enough(g: &NoteGroup, present: &[i32]) -> bool {
    let classes = |v: &[i32]| v.iter().map(|m| m.rem_euclid(12)).collect::<BTreeSet<_>>().len();
    present.len() >= need(g) && classes(present) >= classes(&g.midis).min(2)
}

pub fn explain(mag: &[f64], binhz: f64, g: &NoteGroup, prev: Option<&[f64]>) -> Verdict {
    explain_as(mag, binhz, g, prev, false)
}

/// `explain` as a bass's notes are judged (P32.3, D89): a single note by the chord's tests too. Its pitch must be a
/// real peak (low down a semitone is 2-4 Hz, inside even a 170 ms window's main lobe, so a note a semitone away
/// reaches into the band), and a peak only a note just played explains must not have got louder (it was played
/// again instead of this one).
pub fn explain_bass(mag: &[f64], binhz: f64, g: &NoteGroup, prev: Option<&[f64]>) -> Verdict {
    explain_as(mag, binhz, g, prev, true)
}

fn explain_as(mag: &[f64], binhz: f64, g: &NoteGroup, prev: Option<&[f64]>, strict: bool) -> Verdict {
    let present = group_present_as(mag, binhz, g, prev, strict || g.midis.len() >= 2);
    let foreign = if strict || g.midis.len() >= 2 {
        foreign_impl(mag, binhz, &g.midis, &g.ringing, prev)
    } else {
        foreign_share_in(mag, binhz, &g.midis, &g.ringing)
    };
    Verdict {
        expected: g.midis.len(),
        need: need(g),
        present: present.len(),
        foreign,
        ok: enough(g, &present) && foreign <= FOREIGN_MAX,
    }
}

/// Samples to analyse after a pluck (bass needs a longer window, a chord a little longer).
pub fn window_for(g: &NoteGroup, rate: u32) -> usize {
    let low = g.midis.iter().copied().min().unwrap_or(60);
    // a chord: longer, for a strum's spread (its last strings come up to 60 ms after the first) and for a finer
    // spectrum (D46: 110 ms measured best on the chord bench, and still ends before the next eighth note at 160 bpm)
    let w = if g.midis.len() >= 2 { 0.11 } else { 0.085 };
    (rate as f64 * if low < 40 { 0.17 } else { w }) as usize
}

/// `window_for` for an instrument: a guitar's is `window_for`'s. A bass's: 170 ms under E2 (as before), and 120 ms
/// above it (a bass note has fewer partials over its fundamental to tell a semitone by than a guitar's, and a
/// longer window parts them better; 120 ms still ends before the next sixteenth note at 120 bpm).
pub fn window_for_inst(g: &NoteGroup, rate: u32, inst: Instrument) -> usize {
    if !inst.is_bass() {
        return window_for(g, rate);
    }
    let low = g.midis.iter().copied().min().unwrap_or(60);
    (rate as f64 * if low < 40 { 0.17 } else { BASS_WINDOW }) as usize
}

/// A bass note's window over E2 (s).
pub const BASS_WINDOW: f64 = 0.12;

/// The guitar recording, addressed by absolute sample index (the engine's capture ring implements this).
pub trait SampleSource {
    fn rate(&self) -> u32;
    fn written(&self) -> u64;
    /// Samples [start, start + out.len()); false if not recorded yet or already gone.
    fn read(&self, start: u64, out: &mut [f32]) -> bool;
    /// The latest out.len() samples.
    fn latest(&self, out: &mut [f32]) -> bool;
}

/// A pluck, as the scorer needs it: where it is in the recording and when it was heard in the song.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pluck {
    pub index: u64,
    pub song_time: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    pub text: &'static str,
    pub judgement: Judgement,
    pub string: i32,
    pub created: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub hit: u32,
    pub late: u32,
    pub early: u32,
    pub miss: u32,
}

impl Counts {
    pub fn judged(&self) -> u32 {
        self.hit + self.late + self.early + self.miss
    }
}

/// What the input looks like right now, for the legato check.
pub struct Live<'a> {
    pub source: &'a dyn SampleSource,
    /// The guitar's level (RMS), 0..1.
    pub level: f32,
    /// Input latency (seconds) to subtract from "now" for legato notes.
    pub latency: f64,
}

pub struct Scorer {
    pub groups: Vec<NoteGroup>,
    times: Vec<f64>,
    ptr: usize,
    pub streak: u32,
    pub best_streak: u32,
    pub popups: Vec<Popup>,
    used: HashSet<u64>,
    pending: Vec<Pluck>,
    sp: Spectra,
    buf: Vec<f32>,
    before: Vec<f32>,
    /// Wall time the held note was last looked for in the live signal (note by note).
    held_check: f64,
    /// Semitones every expected pitch is moved by (the song's audio is pitch-shifted).
    shift: i32,
    /// Capture index up to which plucks belong to the strum that just played a chord (a slow strum's later strings
    /// can make a pluck of their own, which mustn't go on to the next chord).
    strum_end: u64,
    /// The instrument heard (P32.3), and whether a note an octave down also counts (a bass on a guitar part).
    inst: Instrument,
    part_is_bass: bool,
    octave_down: bool,
}

/// Seconds after a chord's pluck that further plucks are taken as the same strum.
pub const STRUM: f64 = 0.07;

/// What became of the note the song is held at (note by note).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// Nothing new heard (or its sound isn't in yet).
    Waiting,
    /// A pluck since the hold that wasn't the note.
    Wrong,
    /// Played: the note is judged a hit and the song can go on.
    Played,
}

/// Whether a pluck played a note group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Yes,
    No,
    /// Its sound isn't recorded yet.
    Wait,
    /// Its sound is already gone from the recording.
    Gone,
}

impl Scorer {
    pub fn new(arr: &Arrangement) -> Scorer {
        let groups = build_groups(arr);
        let times = groups.iter().map(|g| g.t).collect();
        Scorer {
            groups,
            times,
            ptr: 0,
            streak: 0,
            best_streak: 0,
            popups: Vec::new(),
            used: HashSet::new(),
            pending: Vec::new(),
            sp: Spectra::new(),
            buf: Vec::new(),
            before: Vec::new(),
            held_check: f64::NEG_INFINITY,
            shift: 0,
            strum_end: 0,
            inst: Instrument::Guitar,
            part_is_bass: arr.is_bass,
            octave_down: false,
        }
    }

    /// The instrument the input plays (P32.3, a guitar by default): a bass's notes are read over its own window,
    /// and on a guitar part a note an octave down counts too.
    pub fn set_instrument(&mut self, inst: Instrument) {
        self.inst = inst;
        self.octave_down = inst.is_bass() && !self.part_is_bass;
    }

    pub fn instrument(&self) -> Instrument {
        self.inst
    }

    /// The analysis window for group `gi`.
    fn window(&self, gi: usize, rate: u32) -> usize {
        window_for_inst(&self.groups[gi], rate, self.inst)
    }

    /// Whether `x` (with `before`) plays group `g`; a bass on a guitar part may play it an octave down.
    fn matches(&mut self, x: &[f32], rate: u32, g: &NoteGroup, before: Option<&[f32]>) -> bool {
        if !self.inst.is_bass() {
            return group_matches(&mut self.sp, x, rate, g, before);
        }
        let (mag, binhz) = self.sp.spectrum(x, rate);
        let prev = before
            .filter(|b| b.len() == x.len())
            .map(|b| self.sp.spectrum(b, rate).0);
        if explain_bass(&mag, binhz, g, prev.as_deref()).ok {
            return true;
        }
        if !self.octave_down {
            return false;
        }
        let mut low = g.clone();
        for m in low.midis.iter_mut().chain(low.ringing.iter_mut()) {
            *m -= 12;
        }
        low.repeats = g.repeats.iter().map(|m| m - 12).collect();
        explain_bass(&mag, binhz, &low, prev.as_deref()).ok
    }

    /// Expect every note `semitones` from where it's charted (the song plays pitch-shifted, and the player follows
    /// it with the tab as written on a capo'd or retuned guitar). Absolute: a later call replaces the earlier shift.
    pub fn transpose(&mut self, semitones: i32) {
        let d = semitones - self.shift;
        if d == 0 {
            return;
        }
        self.shift = semitones;
        for g in &mut self.groups {
            for m in &mut g.midis {
                *m += d;
            }
            for m in &mut g.ringing {
                *m += d;
            }
            g.repeats = g.repeats.iter().map(|m| m + d).collect();
        }
    }

    pub fn shift(&self) -> i32 {
        self.shift
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for g in &self.groups {
            match g.state {
                Some(Judgement::Hit) => c.hit += 1,
                Some(Judgement::Late) => c.late += 1,
                Some(Judgement::Early) => c.early += 1,
                Some(Judgement::Miss) => c.miss += 1,
                None => {}
            }
        }
        c
    }

    pub fn score(&self) -> u32 {
        self.groups
            .iter()
            .filter_map(|g| g.state)
            .map(Judgement::points)
            .sum()
    }

    pub fn accuracy(&self) -> f64 {
        let c = self.counts();
        let judged = c.judged();
        if judged == 0 {
            0.0
        } else {
            (c.hit as f64 + 0.5 * (c.late + c.early) as f64) / judged as f64
        }
    }

    pub fn multiplier(&self) -> u32 {
        multiplier(self.streak)
    }

    /// The streak and best streak worked out again from the judgements as they stand, in time order (a hit adds one,
    /// a miss breaks it, late and early leave it, as `judge` / `miss` count them): for judgements set from outside
    /// (screenshot staging).
    pub fn recount_streak(&mut self) {
        let mut order: Vec<usize> = (0..self.groups.len())
            .filter(|&i| self.groups[i].state.is_some())
            .collect();
        order.sort_by(|&a, &b| self.groups[a].t.total_cmp(&self.groups[b].t));
        let (mut run, mut best) = (0u32, 0u32);
        for i in order {
            match self.groups[i].state {
                Some(Judgement::Hit) => {
                    run += 1;
                    best = best.max(run);
                }
                Some(Judgement::Miss) => run = 0,
                _ => {}
            }
        }
        self.streak = run;
        self.best_streak = best;
    }

    /// Plucks heard (song times already shifted by the guitar-timing offset). Kept until their audio is in.
    pub fn feed(&mut self, plucks: impl IntoIterator<Item = Pluck>) {
        self.pending.extend(plucks);
        self.pending.sort_by_key(|p| p.index);
    }

    /// Forget judgements from time t on (after a seek or a loop pass).
    pub fn reset_from(&mut self, t: f64) {
        for g in &mut self.groups {
            if g.t >= t - EARLY {
                g.state = None;
                g.last_check = -1.0;
                g.judged_wall = 0.0;
            }
        }
        self.ptr = 0;
        while self.ptr < self.groups.len() && self.groups[self.ptr].state.is_some() {
            self.ptr += 1;
        }
        while self.ptr < self.groups.len() && self.groups[self.ptr].t < t - LATE {
            self.ptr += 1;
        }
        self.streak = 0;
        self.used.clear();
        self.pending.clear();
    }

    fn judge(&mut self, gi: usize, dt: f64, now_wall: f64) {
        let state = if dt > PERFECT {
            Judgement::Late
        } else if dt < -PERFECT {
            Judgement::Early
        } else {
            Judgement::Hit
        };
        let g = &mut self.groups[gi];
        g.dt = dt;
        g.judged_wall = now_wall;
        g.state = Some(state);
        let string = g.strings.iter().copied().min().unwrap_or(0);
        if state == Judgement::Hit {
            self.streak += 1;
            self.best_streak = self.best_streak.max(self.streak);
        }
        let label = match state {
            Judgement::Late => "Late",
            Judgement::Early => "Early",
            _ => "",
        };
        if !label.is_empty() {
            self.popup(label, Judgement::Late, string, now_wall);
        }
    }

    fn miss(&mut self, gi: usize, now_wall: f64) {
        let g = &mut self.groups[gi];
        g.state = Some(Judgement::Miss);
        g.judged_wall = now_wall;
        let string = g.strings.iter().copied().min().unwrap_or(0);
        self.streak = 0;
        self.popup("Miss", Judgement::Miss, string, now_wall);
    }

    fn popup(&mut self, text: &'static str, judgement: Judgement, string: i32, now_wall: f64) {
        if self
            .popups
            .iter()
            .any(|p| p.string == string && now_wall - p.created < 0.15)
        {
            return; // one label at a time per string
        }
        self.popups.push(Popup {
            text,
            judgement,
            string,
            created: now_wall,
        });
    }

    /// Advance to chart time `now` (the now-line). `live` is None when there's no guitar input: notes are still
    /// marked missed as they pass (so the song can be played along without scoring the input).
    pub fn update(&mut self, now: f64, live: Option<&Live>, now_wall: f64) {
        if let Some(live) = live {
            let pending = std::mem::take(&mut self.pending);
            let mut keep = Vec::new();
            let mut waiting = false;
            for p in pending {
                if waiting || self.used.contains(&p.index) {
                    if waiting {
                        keep.push(p);
                    }
                    continue;
                }
                if self.place(p, live.source, now_wall) == Place::Wait {
                    waiting = true;
                    keep.push(p);
                }
            }
            self.pending = keep;
            self.legato(now, live, now_wall);
        }
        let mut i = self.ptr;
        while i < self.groups.len() && self.groups[i].t + LATE < now {
            if self.groups[i].state.is_none() {
                self.miss(i, now_wall);
            }
            i += 1;
        }
        while self.ptr < self.groups.len() && self.groups[self.ptr].state.is_some() {
            self.ptr += 1;
        }
        self.popups.retain(|p| now_wall - p.created < 0.8);
        if self.used.len() > 4096 {
            self.used.clear();
        }
    }

    /// Unjudged groups whose window contains st, nearest first (at most 2).
    fn candidates(&self, st: f64) -> Vec<usize> {
        let lo = self.times.partition_point(|&t| t < st - LATE);
        let hi = self.times.partition_point(|&t| t <= st + EARLY);
        let mut c: Vec<usize> = (lo..hi).filter(|&i| self.groups[i].state.is_none()).collect();
        c.sort_by(|&a, &b| {
            (self.groups[a].t - st)
                .abs()
                .total_cmp(&(self.groups[b].t - st).abs())
        });
        c.truncate(2);
        c
    }

    /// Whether the pluck at capture index `index` played group `gi` (any pluck plays a muted one), from the sound
    /// just after it.
    fn check(&mut self, gi: usize, index: u64, src: &dyn SampleSource) -> Check {
        if self.groups[gi].muted {
            return Check::Yes;
        }
        let rate = src.rate();
        let n = self.window(gi, rate);
        let skip = (ATTACK_SKIP * rate as f64) as u64;
        self.buf.resize(n, 0.0);
        if !src.read(index + skip, &mut self.buf) {
            return if index + skip + n as u64 > src.written() {
                Check::Wait
            } else {
                Check::Gone
            };
        }
        self.before.resize(n, 0.0);
        let have_before = index >= n as u64 && src.read(index - n as u64, &mut self.before);
        let g = self.groups[gi].clone();
        let x = std::mem::take(&mut self.buf);
        let before = std::mem::take(&mut self.before);
        let ok = self.matches(&x, rate, &g, have_before.then_some(&before[..]));
        self.buf = x;
        self.before = before;
        if ok { Check::Yes } else { Check::No }
    }

    fn place(&mut self, p: Pluck, src: &dyn SampleSource, now_wall: f64) -> Place {
        if p.index < self.strum_end {
            self.used.insert(p.index); // the rest of a strum already judged
            return Place::None;
        }
        let cands = self.candidates(p.song_time);
        if cands.is_empty() {
            self.used.insert(p.index);
            return Place::None;
        }
        for gi in cands {
            match self.check(gi, p.index, src) {
                Check::Yes => {
                    self.used.insert(p.index);
                    if self.groups[gi].midis.len() >= 2 {
                        self.strum_end = p.index + (STRUM * src.rate() as f64) as u64;
                    }
                    let dt = p.song_time - self.groups[gi].t;
                    self.judge(gi, dt, now_wall);
                    return Place::Judged;
                }
                Check::Wait => return Place::Wait, // not recorded yet; try again next frame
                Check::Gone => {
                    self.used.insert(p.index); // already overwritten: drop it
                    return Place::None;
                }
                Check::No => {}
            }
        }
        self.used.insert(p.index); // a pluck that matched nothing (wrong note)
        Place::None
    }

    /// A pluck heard at song time `from` or later whose sound isn't read yet (it may still play the next note).
    pub fn in_flight(&self, from: f64) -> bool {
        self.pending
            .iter()
            .any(|p| p.song_time >= from && !self.used.contains(&p.index))
    }

    /// Note by note: group `gi` passed over without being played (it counts as missed).
    pub fn pass(&mut self, gi: usize, now_wall: f64) {
        if self.groups.get(gi).is_some_and(|g| g.state.is_none()) {
            self.miss(gi, now_wall);
        }
    }

    /// Note by note: the song is held at group `gi` until it's played. Every pluck fed is checked against it alone,
    /// whenever it came (the song isn't moving, so its time doesn't count); a hammer-on, pull-off or tap is also
    /// looked for in the live signal (unless its pitch was just played, which would still be ringing). Played, it's
    /// judged a hit. Only plucks at capture index `since` or later count as wrong (one just before the hold was
    /// likely meant for the note before). Plucks after the one that played it stay for the scorer.
    pub fn wait_on(&mut self, gi: usize, since: u64, live: &Live, now_wall: f64) -> Held {
        let Some(g) = self.groups.get(gi) else {
            return Held::Waiting;
        };
        if g.state.is_some() {
            return Held::Played;
        }
        let (legato, live_ok) = (g.legato && !g.muted, g.repeats.is_empty());
        let mut out = Held::Waiting;
        let pending = std::mem::take(&mut self.pending);
        let mut rest = pending.into_iter();
        for p in rest.by_ref() {
            if self.used.contains(&p.index) {
                continue;
            }
            match self.check(gi, p.index, live.source) {
                Check::Yes => {
                    self.used.insert(p.index);
                    self.judge(gi, 0.0, now_wall);
                    out = Held::Played;
                    break;
                }
                Check::Wait => {
                    self.pending.push(p);
                    break;
                }
                Check::Gone => {
                    self.used.insert(p.index);
                }
                Check::No => {
                    self.used.insert(p.index);
                    if p.index >= since {
                        out = Held::Wrong;
                    }
                }
            }
        }
        self.pending.extend(rest);
        if out == Held::Played || !legato || !live_ok {
            return out;
        }
        if now_wall - self.held_check >= 0.02 && live.level > 0.002 {
            self.held_check = now_wall;
            let n = self.window(gi, live.source.rate());
            self.buf.resize(n, 0.0);
            if live.source.latest(&mut self.buf) {
                let g = self.groups[gi].clone();
                let x = std::mem::take(&mut self.buf);
                let ok = self.matches(&x, live.source.rate(), &g, None);
                self.buf = x;
                if ok {
                    self.judge(gi, 0.0, now_wall);
                    return Held::Played;
                }
            }
        }
        out
    }

    fn legato(&mut self, now: f64, live: &Live, now_wall: f64) {
        let rate = live.source.rate();
        let mut i = self.ptr;
        while i < self.groups.len() && self.groups[i].t - 0.05 <= now {
            let gi = i;
            i += 1;
            let g = &self.groups[gi];
            if g.state.is_some() || !g.legato || g.muted || now > g.t + LATE || now - g.last_check < 0.02 {
                continue;
            }
            self.groups[gi].last_check = now;
            let n = self.window(gi, rate);
            self.buf.resize(n, 0.0);
            if live.level > 0.002 && live.source.latest(&mut self.buf) {
                let g = self.groups[gi].clone();
                let x = std::mem::take(&mut self.buf);
                let ok = self.matches(&x, rate, &g, None);
                self.buf = x;
                if ok {
                    let heard_at = now - live.latency - n as f64 / rate as f64 / 2.0;
                    self.judge(gi, (heard_at - g.t).max(-PERFECT), now_wall);
                }
            }
        }
    }
}

#[derive(PartialEq)]
enum Place {
    Judged,
    None,
    Wait,
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_formats::model::{Beat, Chord};

    struct Rec {
        rate: u32,
        x: Vec<f32>,
    }
    impl SampleSource for Rec {
        fn rate(&self) -> u32 {
            self.rate
        }
        fn written(&self) -> u64 {
            self.x.len() as u64
        }
        fn read(&self, start: u64, out: &mut [f32]) -> bool {
            let s = start as usize;
            if s + out.len() > self.x.len() {
                return false;
            }
            out.copy_from_slice(&self.x[s..s + out.len()]);
            true
        }
        fn latest(&self, out: &mut [f32]) -> bool {
            if out.len() > self.x.len() {
                return false;
            }
            let s = self.x.len() - out.len();
            out.copy_from_slice(&self.x[s..]);
            true
        }
    }

    /// A plucked guitar note (fundamental + harmonics, decaying).
    fn add_note(x: &mut [f32], rate: u32, at: f64, midi: i32, amp: f32) {
        let f = midi_to_hz(midi as f64);
        let start = (at * rate as f64) as usize;
        for i in 0..(rate as usize) {
            if start + i >= x.len() {
                break;
            }
            let t = i as f64 / rate as f64;
            let w = std::f64::consts::TAU * f * t;
            let v = (w.sin() + 0.5 * (2.0 * w).sin() + 0.25 * (3.0 * w).sin()) * (-t * 3.0).exp();
            x[start + i] += amp * v as f32;
        }
    }

    fn arr(notes: &[(f64, i32, i32)]) -> Arrangement {
        let mut a = Arrangement::new("Lead");
        a.notes = notes.iter().map(|&(t, s, f)| Note::new(t, s, f)).collect();
        a.beats = (0..20)
            .map(|i| Beat {
                time: i as f64 * 0.5,
                measure: if i % 4 == 0 { 1 } else { -1 },
            })
            .collect();
        a
    }

    #[test]
    fn groups_merge_double_stops_and_track_ringing() {
        let mut a = arr(&[(1.0, 0, 3), (1.005, 1, 5), (2.0, 0, 3)]);
        a.chords.push(Chord {
            time: 3.0,
            chord_id: 0,
            name: "x".into(),
            notes: vec![Note {
                techniques: Techs::FRET_HAND_MUTE,
                ..Note::new(3.0, 0, 0)
            }],
            techniques: Techs::NONE,
            sustain: 0.0,
            frets: vec![],
            fingers: vec![],
            high_density: false,
        });
        let g = build_groups(&a);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].midis, vec![43, 50]); // E2+3 = G2, A2+5 = D3
        assert!(g[1].repeats.contains(&43));
        assert!(g[2].muted);
    }

    #[test]
    fn right_note_on_time_is_a_hit_wrong_note_is_not() {
        let rate = 48000;
        let mut x = vec![0.0f32; rate as usize * 6];
        add_note(&mut x, rate, 1.0, 43, 0.3); // G2: right
        add_note(&mut x, rate, 2.05, 46, 0.3); // chart wants A2 (45) at 2.0: wrong note (A#2)
        add_note(&mut x, rate, 3.25, 47, 0.3); // B2 at 3.25 for 3.0: late
        let a = arr(&[(1.0, 0, 3), (2.0, 0, 5), (3.0, 0, 7)]);
        let mut s = Scorer::new(&a);
        let src = Rec { rate, x };
        s.feed([
            Pluck {
                index: rate as u64,
                song_time: 1.0,
            },
            Pluck {
                index: (2.05 * rate as f64) as u64,
                song_time: 2.05,
            },
            Pluck {
                index: (3.25 * rate as f64) as u64,
                song_time: 3.25,
            },
        ]);
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        s.update(5.0, Some(&live), 5.0);
        let states: Vec<_> = s.groups.iter().map(|g| g.state).collect();
        assert_eq!(
            states,
            vec![Some(Judgement::Hit), Some(Judgement::Miss), Some(Judgement::Late)]
        );
        assert_eq!(s.score(), 150);
        assert!((s.accuracy() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_transposed_scorer_wants_the_shifted_pitch() {
        let rate = 48000;
        let mut x = vec![0.0f32; rate as usize * 4];
        add_note(&mut x, rate, 1.0, 45, 0.3); // the chart's G2 (43) two semitones up
        add_note(&mut x, rate, 2.05, 48, 0.3); // the chart's C3 (48) as written: wrong once shifted (D3)
        let a = arr(&[(1.0, 0, 3), (2.0, 0, 8)]);
        let mut s = Scorer::new(&a);
        s.transpose(5);
        s.transpose(2); // absolute, not cumulative
        assert_eq!(s.shift(), 2);
        assert_eq!(s.groups[0].midis, vec![45]);
        let src = Rec { rate, x };
        let plucks = [
            Pluck {
                index: rate as u64,
                song_time: 1.0,
            },
            Pluck {
                index: (2.05 * rate as f64) as u64,
                song_time: 2.05,
            },
        ];
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        s.feed(plucks);
        s.update(4.0, Some(&live), 4.0);
        let states: Vec<_> = s.groups.iter().map(|g| g.state).collect();
        assert_eq!(states, vec![Some(Judgement::Hit), Some(Judgement::Miss)]);
        // back to the chart's own pitch: the first note is now the wrong one, the second right
        let mut s = Scorer::new(&a);
        s.transpose(2);
        s.transpose(0);
        s.feed(plucks);
        s.update(4.0, Some(&live), 4.0);
        let states: Vec<_> = s.groups.iter().map(|g| g.state).collect();
        assert_eq!(states, vec![Some(Judgement::Miss), Some(Judgement::Hit)]);
    }

    #[test]
    fn plucks_wait_for_their_audio() {
        let rate = 48000;
        let a = arr(&[(1.0, 0, 3)]);
        let mut s = Scorer::new(&a);
        let src = Rec {
            rate,
            x: vec![0.0; rate as usize + 100],
        }; // the window after the pluck isn't recorded yet
        s.feed([Pluck {
            index: rate as u64,
            song_time: 1.0,
        }]);
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        s.update(1.1, Some(&live), 1.1);
        assert_eq!(s.pending.len(), 1);
        assert!(s.groups[0].state.is_none());
    }

    /// Note by note: held at a note, a wrong pluck says so, the right one (whenever it comes) plays it; a pluck
    /// whose sound isn't in yet waits; a chord needs its shape; plucks after the one that played it are kept.
    #[test]
    fn a_held_note_waits_for_the_right_pluck() {
        let rate = 48000;
        let at = |t: f64| (t * rate as f64) as u64;
        let mut x = vec![0.0f32; rate as usize * 8];
        add_note(&mut x, rate, 2.0, 46, 0.3); // A#2: wrong (the chart wants G2 at 1.0)
        add_note(&mut x, rate, 3.0, 43, 0.3); // G2: right
        add_note(&mut x, rate, 4.0, 45, 0.3); // the next note, played straight after
        let a = arr(&[(1.0, 0, 3), (1.5, 0, 5)]);
        let mut s = Scorer::new(&a);
        let src = Rec { rate, x };
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        assert_eq!(s.wait_on(0, at(1.5), &live, 1.0), Held::Waiting);
        let pluck = |t: f64| Pluck {
            index: at(t),
            song_time: 1.0,
        };
        s.feed([pluck(2.0)]);
        assert_eq!(s.wait_on(0, at(1.5), &live, 2.2), Held::Wrong);
        assert!(s.groups[0].state.is_none());
        s.feed([pluck(3.0), pluck(4.0)]);
        assert_eq!(s.wait_on(0, at(1.5), &live, 3.2), Held::Played);
        assert_eq!(s.groups[0].state, Some(Judgement::Hit));
        assert_eq!(s.pending.len(), 1, "the next note's pluck is kept");
        assert_eq!(s.wait_on(0, at(1.5), &live, 3.3), Held::Played);

        // a wrong pluck from before the hold isn't called wrong
        let mut s = Scorer::new(&a);
        s.feed([pluck(2.0)]);
        assert_eq!(s.wait_on(0, at(2.5), &live, 2.6), Held::Waiting);

        // the right pluck, its sound not in yet
        let short = Rec {
            rate,
            x: src.x[..at(3.05) as usize].to_vec(),
        };
        let live_short = Live {
            source: &short,
            level: 0.1,
            latency: 0.0,
        };
        let mut s = Scorer::new(&a);
        s.feed([pluck(3.0)]);
        assert_eq!(s.wait_on(0, 0, &live_short, 3.05), Held::Waiting);
        assert_eq!(s.pending.len(), 1);
        assert_eq!(s.wait_on(0, 0, &live, 3.2), Held::Played);

        // a chord: G5 (G2 D3 G3) strummed right, then a wrong shape (A5) isn't it
        let mut x = vec![0.0f32; rate as usize * 4];
        for m in [45, 52, 57] {
            add_note(&mut x, rate, 1.0, m, 0.2);
        }
        for m in [43, 50, 55] {
            add_note(&mut x, rate, 2.5, m, 0.2);
        }
        let src = Rec { rate, x };
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        let a = arr(&[(1.0, 0, 3), (1.0, 1, 5), (1.0, 2, 5)]);
        let mut s = Scorer::new(&a);
        assert_eq!(s.groups.len(), 1);
        s.feed([pluck(1.0)]);
        assert_eq!(s.wait_on(0, 0, &live, 1.2), Held::Wrong);
        s.feed([pluck(2.5)]);
        assert_eq!(s.wait_on(0, 0, &live, 2.7), Held::Played);
    }

    /// A held hammer-on is heard from the live signal without a pluck; a picked note isn't (a pluck is needed).
    #[test]
    fn a_held_hammer_on_is_heard_without_a_pluck() {
        let rate = 48000;
        let mut x = vec![0.0f32; rate as usize * 2];
        add_note(&mut x, rate, 1.2, 45, 0.3); // A2 sounding now
        let src = Rec { rate, x };
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        let mut a = arr(&[(1.0, 0, 3), (1.5, 0, 5)]);
        a.notes[1].techniques = Techs::HAMMER_ON;
        let mut s = Scorer::new(&a);
        assert_eq!(s.wait_on(1, 0, &live, 1.4), Held::Played);
        assert_eq!(s.groups[1].state, Some(Judgement::Hit));
        let a = arr(&[(1.0, 0, 3), (1.5, 0, 5)]);
        let mut s = Scorer::new(&a);
        assert_eq!(s.wait_on(1, 0, &live, 1.4), Held::Waiting);
    }

    /// A chord chart: each (time, [(string, fret)]).
    fn chords(list: &[(f64, &[(i32, i32)])]) -> Arrangement {
        let mut a = arr(&[]);
        for &(t, shape) in list {
            a.chords.push(Chord {
                time: t,
                chord_id: 0,
                name: String::new(),
                notes: shape.iter().map(|&(s, f)| Note::new(t, s, f)).collect(),
                techniques: Techs::NONE,
                sustain: 0.0,
                frets: vec![],
                fingers: vec![],
                high_density: false,
            });
        }
        a
    }

    /// A strum: the strings one after another over `spread` seconds (in the order given), uneven levels.
    fn add_strum(x: &mut [f32], rate: u32, at: f64, shape: &[(i32, i32)], spread: f64) {
        let opens = [40, 45, 50, 55, 59, 64];
        let k = shape.len().max(2) - 1;
        for (j, &(s, f)) in shape.iter().enumerate() {
            let amp = [0.2, 0.12, 0.17, 0.1, 0.15, 0.13][j % 6];
            add_note(
                x,
                rate,
                at + spread * j as f64 / k as f64,
                opens[s as usize] + f,
                amp,
            );
        }
    }

    /// Score strums (pluck at each strum's start) against a chart: the groups' judgements.
    fn score_strums(a: &Arrangement, x: Vec<f32>, plucks: &[f64]) -> Vec<Option<Judgement>> {
        let rate = 48000;
        let mut s = Scorer::new(a);
        let src = Rec { rate, x };
        s.feed(plucks.iter().map(|&t| Pluck {
            index: (t * rate as f64) as u64,
            song_time: t,
        }));
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        s.update(30.0, Some(&live), 30.0);
        s.groups.iter().map(|g| g.state).collect()
    }

    const BM: [(i32, i32); 6] = [(0, 7), (1, 9), (2, 9), (3, 7), (4, 7), (5, 7)];
    const E_OPEN: [(i32, i32); 6] = [(0, 0), (1, 2), (2, 2), (3, 1), (4, 0), (5, 0)];
    const A5: [(i32, i32); 3] = [(0, 5), (1, 7), (2, 7)];

    /// D46: a chord counts with half its strings (at least two): a downstrum missing the top two strings, an
    /// upstrum catching only the top three of a chord repeated, a slow (50 ms) strum.
    #[test]
    fn partial_and_slow_strums_count() {
        let rate = 48000;
        let mut x = vec![0.0f32; rate as usize * 6];
        add_strum(&mut x, rate, 1.0, &BM[..4], 0.03); // 4 of 6
        let top: Vec<_> = BM[3..].iter().rev().copied().collect();
        add_strum(&mut x, rate, 2.0, &top, 0.015); // upstrum, 3 of 6 (the same chord again)
        add_strum(&mut x, rate, 3.0, &E_OPEN, 0.05); // a slow full strum
        let a = chords(&[(1.0, &BM), (2.0, &BM), (3.0, &E_OPEN)]);
        assert_eq!(
            score_strums(&a, x, &[1.0, 2.0, 3.0]),
            vec![Some(Judgement::Hit); 3]
        );
        // (before D46 a chord of six needed four: the upstrum's three weren't enough)
        let g = &build_groups(&a)[1];
        assert_eq!(need(g), 3);
        assert_eq!(need(&build_groups(&chords(&[(1.0, &A5[..2])]))[0]), 2);
    }

    /// The other side: a shape a fret off, another chord, the previous chord strummed again when the next is due,
    /// and a power chord's root with its octave alone (one note, not the chord) aren't the chord.
    #[test]
    fn wrong_shapes_still_miss() {
        let rate = 48000;
        let mut x = vec![0.0f32; rate as usize * 8];
        let up: Vec<_> = BM.iter().map(|&(s, f)| (s, f + 1)).collect();
        add_strum(&mut x, rate, 1.0, &up, 0.02); // Cm for Bm
        add_strum(&mut x, rate, 2.0, &E_OPEN, 0.02); // E for Bm
        add_strum(&mut x, rate, 3.0, &BM, 0.02); // Bm (right)
        add_strum(&mut x, rate, 3.5, &BM, 0.02); // Bm again where E is due
        let a = chords(&[(1.0, &BM), (2.0, &BM), (3.0, &BM), (3.5, &E_OPEN)]);
        let s = score_strums(&a, x, &[1.0, 2.0, 3.0, 3.5]);
        let miss = Some(Judgement::Miss);
        assert_eq!(s, vec![miss, miss, Some(Judgement::Hit), miss]);
        // (a root's 3rd harmonic is its fifth, so this is decided on what's heard, not shown by a strum here)
        let g = &build_groups(&chords(&[(1.0, &A5)]))[0];
        assert!(!enough(g, &[45, 57]));
        assert!(enough(g, &[45, 52]));
    }

    /// A slow strum can make a second pluck 50 ms in (its later strings); it is the same strum, not an early
    /// strum of the chord after.
    #[test]
    fn a_strums_second_pluck_is_the_same_strum() {
        let rate = 48000;
        let mut x = vec![0.0f32; rate as usize * 4];
        add_strum(&mut x, rate, 1.0, &E_OPEN, 0.06);
        add_strum(&mut x, rate, 1.25, &E_OPEN, 0.02);
        let a = chords(&[(1.0, &E_OPEN), (1.25, &E_OPEN)]);
        let s = score_strums(&a, x, &[1.0, 1.055, 1.25]);
        assert_eq!(s, vec![Some(Judgement::Hit), Some(Judgement::Hit)]);
    }

    // -- bass mode (P32.3, D89) ------------------------------------------------------------------------------

    use crate::instrument::Instrument;
    use crate::testsig;

    /// A bass part: (time, string, fret), its strings tuned by `tuning` (semitones from E standard).
    fn bass_part(notes: &[(f64, i32, i32)], tuning: i32) -> Arrangement {
        let mut a = arr(notes);
        a.is_bass = true;
        a.tuning = vec![tuning; 4];
        a
    }

    /// Score plucked notes (midi, at) against `a` with `inst`: the judgements. `ks`: a Karplus-Strong string with a
    /// finger's attack, else soft additive notes whose 2nd harmonic is louder than the fundamental. Each note stops
    /// the one before (a 5 ms fade), in hiss at -74 dBFS.
    fn score_played(
        a: &Arrangement,
        inst: Instrument,
        played: &[(f64, f64)],
        amp: f32,
        ks: bool,
    ) -> Vec<Option<Judgement>> {
        let rate = 48000;
        let end = played.iter().map(|p| p.1).fold(0.0, f64::max) + 2.0;
        let mut x = vec![0.0f32; (end * rate as f64) as usize];
        for (k, &(m, at)) in played.iter().enumerate() {
            let note = if ks {
                testsig::ks_pluck(m, rate, 1.0, amp, 0.75, k as u32)
            } else {
                testsig::additive_pluck(m, rate, 1.0, amp, &[0.4, 1.0, 0.5, 0.3, 0.1], 0.01, 1.5)
            };
            let a0 = (at * rate as f64) as usize;
            for (i, v) in x.iter_mut().skip(a0).enumerate() {
                *v *= 1.0 - (i as f32 / 240.0).min(1.0);
            }
            testsig::mix_at(&mut x, a0, &note);
        }
        let hiss = testsig::hiss(x.len(), 0.0002, 4);
        testsig::mix_at(&mut x, 0, &hiss);
        let mut s = Scorer::new(a);
        s.set_instrument(inst);
        let src = Rec { rate, x };
        s.feed(played.iter().map(|&(_, t)| Pluck {
            index: (t * rate as f64) as u64,
            song_time: t,
        }));
        let live = Live {
            source: &src,
            level: 0.1,
            latency: 0.0,
        };
        s.update(end + 5.0, Some(&live), end + 5.0);
        s.groups.iter().map(|g| g.state).collect()
    }

    /// A bass part on a bass, every string and down to a low B (B standard), soft to loud, two timbres: the right
    /// notes are hits, a note a semitone off (either way) is a miss.
    #[test]
    fn bass_notes_are_scored_on_a_bass() {
        let hit = Some(Judgement::Hit);
        let miss = Some(Judgement::Miss);
        for tuning in [0, -5] {
            // open, 3rd, 5th, 7th fret on each string, a note every 0.4 s
            let chart: Vec<(f64, i32, i32)> = (0..16)
                .map(|k| (1.0 + 0.4 * k as f64, (k / 4) as i32, [0, 3, 5, 7][k % 4]))
                .collect();
            let a = bass_part(&chart, tuning);
            let opens = a.open_string_midi();
            let right: Vec<(f64, f64)> = chart
                .iter()
                .map(|&(t, st, f)| ((opens[st as usize] + f) as f64, t))
                .collect();
            for ks in [true, false] {
                for amp in [0.01f32, 0.1, 0.5] {
                    let got = score_played(&a, Instrument::Bass, &right, amp, ks);
                    assert_eq!(got, vec![hit; 16], "tuning {tuning} ks {ks} amp {amp}");
                    // every note a semitone off: up on even notes, down on odd
                    let wrong: Vec<(f64, f64)> = right
                        .iter()
                        .enumerate()
                        .map(|(k, &(m, t))| (m + if k % 2 == 0 { 1.0 } else { -1.0 }, t))
                        .collect();
                    let got = score_played(&a, Instrument::Bass, &wrong, amp, ks);
                    let misses = got.iter().filter(|g| **g == miss).count();
                    assert_eq!(misses, 16, "tuning {tuning} ks {ks} amp {amp}: {got:?}");
                }
            }
        }
    }

    /// A guitar can play a bass part (an octave up, on its low strings): hits, as before. A bass on a guitar part
    /// (the user's choice): the note an octave down counts in bass mode, not with a guitar set.
    #[test]
    fn a_guitar_on_a_bass_part_and_a_bass_on_a_guitar_part() {
        let hit = Some(Judgement::Hit);
        let chart: Vec<(f64, i32, i32)> = (0..8i32)
            .map(|k| (1.0 + 0.4 * k as f64, k / 2, 2 * (k % 2) + 3))
            .collect();
        let a = bass_part(&chart, 0);
        let opens = a.open_string_midi();
        let up: Vec<(f64, f64)> = chart
            .iter()
            .map(|&(t, st, f)| ((opens[st as usize] + f + 12) as f64, t))
            .collect();
        assert_eq!(score_played(&a, Instrument::Guitar, &up, 0.2, true), vec![hit; 8]);
        // the guitar part, played an octave down on a bass
        let g = arr(&chart);
        let gopens = g.open_string_midi();
        let down: Vec<(f64, f64)> = chart
            .iter()
            .map(|&(t, st, f)| ((gopens[st as usize] + f - 12) as f64, t))
            .collect();
        assert_eq!(score_played(&g, Instrument::Bass, &down, 0.2, true), vec![hit; 8]);
        let on_guitar = score_played(&g, Instrument::Guitar, &down, 0.2, true);
        assert!(
            on_guitar.iter().filter(|j| **j == hit).count() <= 2,
            "{on_guitar:?}"
        );
        // and at pitch on a bass, too
        let same: Vec<(f64, f64)> = chart
            .iter()
            .map(|&(t, st, f)| ((gopens[st as usize] + f) as f64, t))
            .collect();
        assert_eq!(score_played(&g, Instrument::Bass, &same, 0.2, true), vec![hit; 8]);
    }

    /// With a guitar (the default), the scorer is what it was: the same windows, the same verdicts and timings on
    /// guitar notes, chords and strums, set or not.
    #[test]
    fn guitar_scoring_is_unchanged() {
        let shapes = chords(&[(1.0, &BM), (2.0, &E_OPEN), (3.0, &A5)]);
        let single = arr(&[(1.0, 0, 0), (2.0, 5, 12)]);
        for g in build_groups(&shapes).iter().chain(build_groups(&single).iter()) {
            assert_eq!(
                window_for_inst(g, 48000, Instrument::Guitar),
                window_for(g, 48000)
            );
        }
        let rate = 48000;
        let mut x = vec![0.0f32; rate as usize * 6];
        add_strum(&mut x, rate, 1.0, &BM[..4], 0.03);
        add_strum(&mut x, rate, 2.0, &E_OPEN, 0.02);
        add_strum(&mut x, rate, 3.0, &A5, 0.02);
        add_note(&mut x, rate, 4.0, 43, 0.3);
        let a = chords(&[(1.0, &BM), (2.0, &BM), (3.0, &A5), (4.0, &[(0, 3)])]);
        let plucks = [1.0, 2.0, 3.0, 4.0];
        let run = |set: bool| {
            let mut s = Scorer::new(&a);
            if set {
                s.set_instrument(Instrument::Guitar);
            }
            let src = Rec { rate, x: x.clone() };
            s.feed(plucks.iter().map(|&t| Pluck {
                index: (t * rate as f64) as u64,
                song_time: t,
            }));
            let live = Live {
                source: &src,
                level: 0.1,
                latency: 0.0,
            };
            s.update(30.0, Some(&live), 30.0);
            s.groups
                .iter()
                .map(|g| (g.state, g.dt.to_bits()))
                .collect::<Vec<_>>()
        };
        let before = run(false);
        assert_eq!(before, run(true));
        let states: Vec<_> = before.iter().map(|b| b.0).collect();
        let (hit, miss) = (Some(Judgement::Hit), Some(Judgement::Miss));
        assert_eq!(states, vec![hit, miss, hit, hit]);
    }

    #[test]
    fn reset_from_clears_later_judgements() {
        let a = arr(&[(1.0, 0, 3), (2.0, 0, 5)]);
        let mut s = Scorer::new(&a);
        s.update(5.0, None, 5.0);
        assert_eq!(s.counts().miss, 2);
        s.reset_from(1.8);
        assert_eq!(s.counts().miss, 1);
        assert_eq!(s.streak, 0);
    }

    #[test]
    fn the_multiplier_steps_every_ten_to_four() {
        assert_eq!(
            [0, 9, 10, 19, 20, 29, 30, 500].map(multiplier),
            [1, 1, 2, 2, 3, 3, 4, 4]
        );
    }

    #[test]
    fn the_streak_recounts_from_judgements_set_outside() {
        let a = arr(&[(1.0, 0, 3), (2.0, 0, 5), (3.0, 0, 7), (4.0, 0, 3), (5.0, 0, 5)]);
        let mut s = Scorer::new(&a);
        let [h, l, m] = [Judgement::Hit, Judgement::Late, Judgement::Miss];
        for (g, j) in s.groups.iter_mut().zip([h, h, l, m, h]) {
            g.state = Some(j);
        }
        s.recount_streak();
        // late keeps the streak going without adding (as judging does), the miss breaks it
        assert_eq!((s.streak, s.best_streak), (1, 2));
    }
}
