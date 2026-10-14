//! Note detection and scoring against the chart. Port of rsplay/detect.py (same thresholds).
//!
//! Each note (or chord) in the chart becomes a NoteGroup. A pluck is found by the engine's onset detector; the
//! audio just after it is checked for the expected pitches (fundamental or 2nd harmonic of every expected note).
//! Hammer-ons, pull-offs and taps don't make a clear pluck, so for those the live signal is checked directly.
//!
//! Judgement, relative to the chart time:
//!   |dt| <= PERFECT   hit   (green)
//!   dt  >  PERFECT    late  (yellow)
//!   dt  < -PERFECT    early (yellow)
//!   nothing by +LATE  miss  (red)

use std::collections::{BTreeSet, HashSet};

use qrs_formats::model::{Arrangement, Note, Techs};

use crate::pitch::midi_to_hz;
use crate::spectrum::{Spectra, median};

pub const EARLY: f64 = 0.20;
pub const LATE: f64 = 0.35;
pub const PERFECT: f64 = 0.10;
/// Seconds after the pluck before analysing (skips the pick noise).
pub const ATTACK_SKIP: f64 = 0.015;
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

pub fn build_groups(arr: &Arrangement) -> Vec<NoteGroup> {
    let opens = arr.open_string_midi();
    let midi = |n: &Note| -> i32 {
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
    };
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
    let lo = (35.0 / binhz) as usize;
    let hi = ((5000.0 / binhz) as usize).min(mag.len());
    let band = &mag[lo.min(hi)..hi];
    let reference = band.iter().copied().fold(0.0, f64::max);
    let floor = median(band);
    if reference <= 0.0 {
        return 0;
    }
    let prev = before
        .filter(|b| b.len() == x.len())
        .map(|b| sp.spectrum(b, rate).0);
    let mut count = 0;
    for &m in midis {
        let f = midi_to_hz(m as f64);
        let p = peak(&mag, binhz, f, 0.03).max(peak(&mag, binhz, 2.0 * f, 0.03));
        if p < 0.12 * reference || p < 4.0 * floor {
            continue;
        }
        if let Some(prev) = &prev
            && !repeats.contains(&m)
        {
            let q = peak(prev, binhz, f, 0.03).max(peak(prev, binhz, 2.0 * f, 0.03));
            if p < 1.2 * q {
                continue;
            }
        }
        count += 1;
    }
    count
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
    let base: Vec<f64> = midis
        .iter()
        .chain(ringing)
        .map(|&m| midi_to_hz(m as f64))
        .collect();
    let mut centres = Vec::new();
    let mut tols = Vec::new();
    for &f0 in &base {
        let hs = (FOREIGN_TOP * 1.05 / f0) as usize;
        for h in 1..=hs {
            let f = h as f64 * f0;
            centres.push(f);
            tols.push((f * HARMONIC_TOL).min(0.25 * f0));
        }
    }
    for &a in &base {
        for &b in &base {
            centres.push(a + b);
            tols.push((a + b) * HARMONIC_TOL);
        }
    }
    let (mut total, mut foreign) = (0.0, 0.0);
    for &i in &idx {
        let f = (i + lo) as f64 * binhz;
        let w = band[i];
        total += w;
        let explained = centres.iter().zip(&tols).any(|(c, t)| (f - c).abs() < *t);
        if !explained {
            foreign += w;
        }
    }
    if total > 0.0 { foreign / total } else { 0.0 }
}

pub fn group_matches(sp: &mut Spectra, x: &[f32], rate: u32, g: &NoteGroup, before: Option<&[f32]>) -> bool {
    let need = ((g.midis.len() as f64 * 0.6).ceil() as usize).max(1);
    if notes_present(sp, x, rate, &g.midis, before, &g.repeats) < need {
        return false;
    }
    foreign_share(sp, x, rate, &g.midis, &g.ringing) <= FOREIGN_MAX
}

/// Samples to analyse after a pluck (bass needs a longer window).
pub fn window_for(g: &NoteGroup, rate: u32) -> usize {
    let low = g.midis.iter().copied().min().unwrap_or(60);
    (rate as f64 * if low < 40 { 0.17 } else { 0.085 }) as usize
}

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
        }
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
        (1 + self.streak / 10).min(4)
    }

    /// Plucks heard (song times already shifted by the guitar-timing offset). Kept until their audio is in.
    pub fn feed(&mut self, plucks: impl IntoIterator<Item = Pluck>) {
        self.pending.extend(plucks);
        self.pending.sort_by_key(|p| p.index);
    }

    /// Forget plucks heard so far (e.g. when switching parts mid-song: they belonged to the old part).
    pub fn ignore_pending(&mut self) {
        self.pending.clear();
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

    fn place(&mut self, p: Pluck, src: &dyn SampleSource, now_wall: f64) -> Place {
        let rate = src.rate();
        let cands = self.candidates(p.song_time);
        if cands.is_empty() {
            self.used.insert(p.index);
            return Place::None;
        }
        for gi in cands {
            if self.groups[gi].muted {
                self.used.insert(p.index);
                let dt = p.song_time - self.groups[gi].t;
                self.judge(gi, dt, now_wall);
                return Place::Judged;
            }
            let n = window_for(&self.groups[gi], rate);
            let skip = (ATTACK_SKIP * rate as f64) as u64;
            self.buf.resize(n, 0.0);
            if !src.read(p.index + skip, &mut self.buf) {
                if p.index + skip + n as u64 > src.written() {
                    return Place::Wait; // not recorded yet; try again next frame
                }
                self.used.insert(p.index); // already overwritten: drop it
                return Place::None;
            }
            self.before.resize(n, 0.0);
            let have_before = p.index >= n as u64 && src.read(p.index - n as u64, &mut self.before);
            let g = self.groups[gi].clone();
            let x = std::mem::take(&mut self.buf);
            let before = std::mem::take(&mut self.before);
            let ok = group_matches(&mut self.sp, &x, rate, &g, have_before.then_some(&before[..]));
            self.buf = x;
            self.before = before;
            if ok {
                self.used.insert(p.index);
                self.judge(gi, p.song_time - g.t, now_wall);
                return Place::Judged;
            }
        }
        self.used.insert(p.index); // a pluck that matched nothing (wrong note)
        Place::None
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
            let n = window_for(&self.groups[gi], rate);
            self.buf.resize(n, 0.0);
            if live.level > 0.002 && live.source.latest(&mut self.buf) {
                let g = self.groups[gi].clone();
                let x = std::mem::take(&mut self.buf);
                let ok = group_matches(&mut self.sp, &x, rate, &g, None);
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
}
