//! Take analysis (new in quickRS, P18.3, D30): a recording of the guitar over a stretch of the chart, checked note
//! by note against the chart, with what was played instead ("your tab"), bar by bar results, the timing tendency and
//! the few problems most worth working on, in words. No AI: score-informed signal analysis.
//!
//! How it works:
//! 1. Plucks are found in the whole take (onsets.rs: spectral flux, timed by the energy rise).
//! 2. Every chart event in the stretch (a note, double stop or chord, merged as the scorer merges them) gets the
//!    plucks within EARLY / LATE of where it falls in the take (`TakeTiming`, wall-clock windows whatever the
//!    speed) as candidates, each checked with the scorer's own test (detect.rs `group_matches`: the expected
//!    pitches present and louder than before, no strong foreign peaks); a chord is also tried a little later
//!    (a slow strum).
//! 3. Events and plucks are aligned in order (each pluck plays at most one event): a right pluck is worth much
//!    more than a wrong one, both a little less the further off time. A first pass gives the take's overall
//!    offset; the second pass centres the windows on it (a take recorded 80 ms late still lines up), while the
//!    offsets reported stay the raw ones.
//! 4. A matched pluck that isn't right is transcribed: one expected note by the McLeod pitch method (a harmonic
//!    product spectrum guess below clarity 0.8), two or more by the strongest harmonic series in the spectrum
//!    (`pitches`), compared note by note with the chord. An event with no pluck is looked for in the sound at its
//!    time (a hammer-on or pull-off makes no pluck; a soft pick may not either); not there, it's missed (a
//!    legato note or a pinch harmonic we can't hear clearly is "not checked", never called wrong).
//! 5. Bends: the pitch is followed through the note; short of the target by more than half a semitone is wrong,
//!    untrackable is a note "bend not checked". Slides are checked where they start.
//! 6. Plucks left over are extra notes (not the second half of a strum, not a bend's wobble), with their pitch.
//! 7. Every pitch heard gets a string and fret: the chart note's string when the pitch is on it, else the
//!    position nearest the hand (the chart's frets around it).
//!
//! Timing offsets are in wall-clock milliseconds (what the player feels; at half speed a 40 ms late note is 20 ms
//! of chart time), positive = late. Accuracy = right / (right + wrong + missed); "not checked" doesn't count.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use qrs_formats::model::{Arrangement, Note, Techs, note_name};
use serde_json::{Value, json};

use crate::detect::{
    self, ATTACK_SKIP, NoteGroup, build_groups, group_matches_in, note_present_in, window_for,
};
use crate::onsets::{self, TakeOnset};
use crate::pitch::{hz_to_midi, midi_to_hz, mpm_pitch};
use crate::similar::Bars;
use crate::spectrum::{Spectra, median};

/// Seconds (wall clock) a pluck may come before / after its note and still be taken for it.
pub const EARLY: f64 = 0.2;
pub const LATE: f64 = 0.35;
/// Within this many ms (median) the take is "on time".
pub const ON_TIME_MS: f64 = 20.0;
const MAX_FRET: i32 = 24;

// -- the time mapping -------------------------------------------------------------------------------------------

/// Where each sample of a take is in the chart: (take sample, chart time) anchors, interpolated between and
/// extrapolated at `speed` beyond them. Chart time here already has the app's offsets applied (the song time
/// heard when the sample was captured, minus the sync offset, plus the guitar timing), as the scorer counts it.
#[derive(Clone, Debug, PartialEq)]
pub struct TakeTiming {
    pub rate: u32,
    /// Chart seconds per wall second.
    pub speed: f64,
    pub anchors: Vec<(u64, f64)>,
}

impl TakeTiming {
    /// From (take sample, chart time) points taken while recording: kept in order, a point that goes back in
    /// samples or chart time dropped, thinned to one every 50 ms of take (they're interpolated between).
    pub fn from_anchors(rate: u32, speed: f64, points: &[(u64, f64)]) -> TakeTiming {
        let mut anchors: Vec<(u64, f64)> = Vec::new();
        let min_gap = (rate as u64 / 20).max(1);
        for &(k, t) in points {
            match anchors.last() {
                Some(&(k0, t0)) if k <= k0 || t < t0 => continue,
                Some(&(k0, _)) if k - k0 < min_gap => continue,
                _ => anchors.push((k, t)),
            }
        }
        if let (Some(&last), Some(&(k, t))) = (anchors.last(), points.last())
            && k > last.0
            && t >= last.1
        {
            anchors.push((k, t)); // the end exactly
        }
        TakeTiming { rate, speed, anchors }
    }

    /// Sample 0 at chart time `t0`, playing at `speed`.
    pub fn linear(rate: u32, t0: f64, speed: f64) -> TakeTiming {
        TakeTiming {
            rate,
            speed,
            anchors: vec![(0, t0)],
        }
    }

    pub fn chart_time(&self, sample: f64) -> f64 {
        let (a, r) = (&self.anchors, self.rate.max(1) as f64);
        if a.is_empty() {
            return sample / r * self.speed;
        }
        let k = a.partition_point(|p| (p.0 as f64) <= sample);
        if k == 0 {
            return a[0].1 + (sample - a[0].0 as f64) / r * self.speed;
        }
        if k == a.len() {
            let l = a[k - 1];
            return l.1 + (sample - l.0 as f64) / r * self.speed;
        }
        let (p, q) = (a[k - 1], a[k]);
        p.1 + (sample - p.0 as f64) / (q.0 - p.0) as f64 * (q.1 - p.1)
    }

    /// The take sample at chart time t (the first, where the song stood still).
    pub fn sample_at(&self, t: f64) -> f64 {
        let (a, r) = (&self.anchors, self.rate.max(1) as f64);
        let sp = self.speed.max(1e-6);
        if a.is_empty() {
            return t / sp * r;
        }
        let k = a.partition_point(|p| p.1 < t);
        if k == 0 {
            return a[0].0 as f64 + (t - a[0].1) / sp * r;
        }
        if k == a.len() {
            let l = a[k - 1];
            return l.0 as f64 + (t - l.1) / sp * r;
        }
        let (p, q) = (a[k - 1], a[k]);
        if q.1 <= p.1 {
            return q.0 as f64;
        }
        p.0 as f64 + (t - p.1) / (q.1 - p.1) * (q.0 - p.0) as f64
    }

    pub fn to_json(&self) -> Value {
        json!({
            "rate": self.rate,
            "speed": self.speed,
            "anchors": self.anchors.iter().map(|&(s, t)| json!([s, (t * 1e6).round() / 1e6])).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(v: &Value) -> Option<TakeTiming> {
        Some(TakeTiming {
            rate: v.get("rate")?.as_u64()? as u32,
            speed: v.get("speed")?.as_f64()?,
            anchors: v
                .get("anchors")?
                .as_array()?
                .iter()
                .filter_map(|p| Some((p.get(0)?.as_u64()?, p.get(1)?.as_f64()?)))
                .collect(),
        })
    }
}

// -- the neck ---------------------------------------------------------------------------------------------------

/// A pitch at a place on the neck. Strings as in the chart: 0 is the lowest-pitched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pos {
    pub string: i32,
    pub fret: i32,
    pub midi: i32,
}

impl Pos {
    /// "Db4"
    pub fn name(&self) -> String {
        format!("{}{}", note_name(self.midi), self.midi.div_euclid(12) - 1)
    }
}

/// The part's strings: their open pitches and names ("low E", "A", ..., "high E").
#[derive(Clone, Debug, PartialEq)]
pub struct Neck {
    pub opens: Vec<i32>,
    pub labels: Vec<String>,
}

impl Neck {
    pub fn new(arr: &Arrangement) -> Neck {
        let opens = arr.open_string_midi();
        let names = arr.string_names();
        let labels = (0..names.len())
            .map(|s| {
                let same: Vec<usize> = (0..names.len()).filter(|&k| names[k] == names[s]).collect();
                if same.len() > 1 && same[0] == s {
                    format!("low {}", names[s])
                } else if same.len() > 1 && same[same.len() - 1] == s {
                    format!("high {}", names[s])
                } else {
                    names[s].to_string()
                }
            })
            .collect();
        Neck { opens, labels }
    }

    pub fn label(&self, string: i32) -> &str {
        self.labels
            .get(string.max(0) as usize)
            .map_or("?", String::as_str)
    }

    pub fn pos(&self, string: i32, fret: i32) -> Pos {
        let open = self.opens.get(string.max(0) as usize).copied().unwrap_or(40);
        Pos {
            string,
            fret,
            midi: open + fret,
        }
    }

    /// Where `midi` was most likely played: on `prefer`'s string if it's there, else the place nearest the hand
    /// (`hand`: the fret the hand is around), a string near `prefer` breaking ties.
    pub fn place(&self, midi: i32, prefer: Option<i32>, hand: f64) -> Option<Pos> {
        if let Some(s) = prefer
            && let Some(&open) = self.opens.get(s.max(0) as usize)
            && (0..=MAX_FRET).contains(&(midi - open))
        {
            return Some(self.pos(s, midi - open));
        }
        let mut best: Option<(f64, Pos)> = None;
        for (s, &open) in self.opens.iter().enumerate() {
            let fret = midi - open;
            if !(0..=MAX_FRET).contains(&fret) {
                continue;
            }
            let reach = if fret == 0 {
                hand.min(4.0)
            } else {
                (fret as f64 - hand).abs()
            };
            let cost = reach + 0.3 * prefer.map_or(0.0, |p| (s as f64 - p as f64).abs());
            if best.is_none_or(|b| cost < b.0) {
                best = Some((cost, self.pos(s as i32, fret)));
            }
        }
        best.map(|b| b.1)
    }

    /// "Db (B string, fret 2)", "E (high E string, open)"
    pub fn describe(&self, p: &Pos) -> String {
        let fret = if p.fret == 0 {
            "open".to_string()
        } else {
            format!("fret {}", p.fret)
        };
        format!("{} ({} string, {fret})", note_name(p.midi), self.label(p.string))
    }
}

// -- the report -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// The right pitches (timing is reported separately).
    Hit,
    /// A pluck there, but not the chart's pitches (`played` says what it was).
    Wrong,
    /// Nothing played for it.
    Missed,
    /// Couldn't tell (a legato note or a pinch harmonic not clearly heard); not counted.
    NotChecked,
}

impl Status {
    pub fn key(self) -> &'static str {
        match self {
            Status::Hit => "hit",
            Status::Wrong => "wrong",
            Status::Missed => "missed",
            Status::NotChecked => "not_checked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EventKind {
    Note,
    DoubleStop,
    Chord,
    /// A fret-hand-muted strum (any pluck plays it).
    Muted,
}

impl EventKind {
    pub fn key(self) -> &'static str {
        match self {
            EventKind::Note => "note",
            EventKind::DoubleStop => "double_stop",
            EventKind::Chord => "chord",
            EventKind::Muted => "muted",
        }
    }
    pub fn plural(self) -> &'static str {
        match self {
            EventKind::Note => "single notes",
            EventKind::DoubleStop => "double stops",
            EventKind::Chord => "chords",
            EventKind::Muted => "muted strums",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventResult {
    /// Chart time.
    pub t: f64,
    /// Index into `TakeReport::bars`.
    pub bar: usize,
    pub kind: EventKind,
    pub techniques: Techs,
    pub chord_name: String,
    pub expected: Vec<Pos>,
    pub status: Status,
    /// What was heard: the expected notes on a hit, what was played instead when wrong (empty: no clear pitch).
    pub played: Vec<Pos>,
    /// Expected notes not heard (a chord missing a string).
    pub missing: Vec<Pos>,
    /// Wall-clock ms from the chart time to the pluck (+ late); None without a pluck (a hammer-on).
    pub offset_ms: Option<f64>,
    /// The take sample of its pluck.
    pub sample: Option<usize>,
    /// A remark: "bend reached +1.0 of +2", "hammer-on heard without a pluck", "slide not checked"...
    pub note: Option<String>,
}

/// A pluck that played no chart note.
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    pub t: f64,
    pub sample: usize,
    pub bar: Option<usize>,
    pub played: Vec<Pos>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BarResult {
    /// The chart's measure number.
    pub number: i32,
    /// 1-based position in the take's stretch.
    pub index: usize,
    pub t0: f64,
    pub t1: f64,
    pub hits: usize,
    pub wrong: usize,
    pub missed: usize,
    pub not_checked: usize,
    pub extras: usize,
    pub mean_offset_ms: Option<f64>,
}

impl BarResult {
    pub fn checked(&self) -> usize {
        self.hits + self.wrong + self.missed
    }
    /// Right notes over notes checked (None for a bar with nothing checked).
    pub fn accuracy(&self) -> Option<f64> {
        let n = self.checked();
        (n > 0).then(|| self.hits as f64 / n as f64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tendency {
    OnTime,
    Rushing,
    Dragging,
    /// Too few timed notes to say.
    Unknown,
}

impl Tendency {
    pub fn key(self) -> &'static str {
        match self {
            Tendency::OnTime => "on time",
            Tendency::Rushing => "rushing",
            Tendency::Dragging => "dragging",
            Tendency::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimingSummary {
    /// Timed right notes.
    pub n: usize,
    pub mean_ms: f64,
    pub median_ms: f64,
    /// Standard deviation.
    pub spread_ms: f64,
    pub tendency: Tendency,
    /// (kind, notes, median ms) for each kind with two or more timed notes.
    pub by_kind: Vec<(EventKind, usize, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TakeReport {
    pub artist: String,
    pub title: String,
    pub part: String,
    pub from: f64,
    pub to: f64,
    pub speed: f64,
    pub rate: u32,
    /// Length of the take (s).
    pub seconds: f64,
    pub neck: Neck,
    pub events: Vec<EventResult>,
    pub extras: Vec<Extra>,
    pub bars: Vec<BarResult>,
    pub timing: TimingSummary,
    /// The few things most worth working on, most important first.
    pub problems: Vec<String>,
    /// The take's overall offset the windows were centred on (ms, + late).
    pub shift_ms: f64,
    pub plucks_found: usize,
    pub peak: f32,
    pub analysis_ms: f64,
}

impl TakeReport {
    pub fn count(&self, s: Status) -> usize {
        self.events.iter().filter(|e| e.status == s).count()
    }

    /// Notes that could be checked: right + wrong + missed ("not checked" ones aside). 0: nothing in the stretch
    /// could be checked (all legato or muted, say); the app says so rather than showing 0%.
    pub fn checked(&self) -> usize {
        self.count(Status::Hit) + self.count(Status::Wrong) + self.count(Status::Missed)
    }

    /// Right notes over notes checked, or None when nothing could be checked.
    pub fn accuracy_checked(&self) -> Option<f64> {
        (self.checked() > 0).then(|| self.accuracy())
    }

    /// Right notes over notes checked (0..1; 0 when nothing could be checked: see `checked`).
    pub fn accuracy(&self) -> f64 {
        let (h, w, m) = (
            self.count(Status::Hit),
            self.count(Status::Wrong),
            self.count(Status::Missed),
        );
        if h + w + m == 0 {
            0.0
        } else {
            h as f64 / (h + w + m) as f64
        }
    }

    /// Whether the take scores at least `pass` (the lesson item's pass mark, 0..1). False when nothing could be
    /// checked (`checked() == 0`).
    pub fn passed(&self, pass: f64) -> bool {
        self.checked() > 0 && self.accuracy() + 1e-9 >= pass
    }

    /// Notes right and within `PERFECT` (100 ms) of the chart.
    pub fn on_time_share(&self) -> f64 {
        let hits: Vec<&EventResult> = self.events.iter().filter(|e| e.status == Status::Hit).collect();
        if hits.is_empty() {
            return 0.0;
        }
        hits.iter()
            .filter(|e| e.offset_ms.is_none_or(|o| o.abs() <= detect::PERFECT * 1000.0))
            .count() as f64
            / hits.len() as f64
    }
}

// -- what was played: pitches in a spectrum -----------------------------------------------------------------------

/// The strongest harmonic series in `x` (up to `max_n` notes between MIDI `lo` and `hi`), strongest first, each
/// with its salience: the candidate whose harmonics (weighted 1/sqrt(h)) add up highest and whose fundamental (or,
/// below 100 Hz, second harmonic) is really there; its harmonics are then taken out (the fundamental fully, the
/// rest mostly, as a chord's notes share partials) and the next found. Stops below a third of the first's salience.
pub fn pitches(sp: &mut Spectra, x: &[f32], rate: u32, lo: i32, hi: i32, max_n: usize) -> Vec<(i32, f64)> {
    let (mag, binhz) = sp.spectrum(x, rate);
    pitches_in(&mag, binhz, lo, hi, max_n)
}

/// `pitches` on a magnitude spectrum already made (`Spectra::spectrum`, or what got louder between two).
pub fn pitches_in(mag: &[f64], binhz: f64, lo: i32, hi: i32, max_n: usize) -> Vec<(i32, f64)> {
    let mut m: Vec<f64> = mag.iter().map(|v| v.sqrt()).collect();
    let band_hi = ((5000.0 / binhz) as usize).min(m.len());
    let band_lo = (50.0 / binhz) as usize;
    let len = m.len();
    let range = |f: f64| -> (usize, usize) {
        let tol = (f * 0.015).max(1.5 * binhz);
        (
            ((f - tol) / binhz).max(1.0) as usize,
            (((f + tol) / binhz) as usize + 1).min(len),
        )
    };
    let mut out: Vec<(i32, f64)> = Vec::new();
    let mut first = 0.0;
    for _ in 0..max_n {
        let top = m[band_lo.min(band_hi)..band_hi]
            .iter()
            .copied()
            .fold(0.0, f64::max);
        if top <= 0.0 {
            break;
        }
        let peak_at = |m: &[f64], f: f64| -> f64 {
            let (a, b) = range(f);
            if a >= b {
                0.0
            } else {
                m[a..b].iter().copied().fold(0.0, f64::max)
            }
        };
        let mut best: Option<(i32, f64)> = None;
        for cand in lo..=hi {
            // (a semitone from a note found is that note's leakage, not a chord tone)
            if out.iter().any(|o| (o.0 - cand).abs() <= 1) {
                continue;
            }
            let f0 = midi_to_hz(cand as f64);
            let fund = peak_at(&m, f0).max(if f0 < 100.0 { peak_at(&m, 2.0 * f0) } else { 0.0 });
            if fund < 0.12 * top {
                continue;
            }
            // an octave below a note (its even harmonics line up) needs a fundamental of its own
            if f0 >= 100.0 && peak_at(&m, f0) < 0.25 * peak_at(&m, 2.0 * f0) {
                continue;
            }
            let mut sal = 0.0;
            for h in 1..=10 {
                let f = f0 * h as f64;
                if f > 5000.0 {
                    break;
                }
                sal += peak_at(&m, f) / (h as f64).sqrt();
            }
            if best.is_none_or(|b| sal > b.1) {
                best = Some((cand, sal));
            }
        }
        let Some((cand, sal)) = best else { break };
        if out.is_empty() {
            first = sal;
        } else if sal < first / 3.0 {
            break;
        }
        out.push((cand, sal));
        let f0 = midi_to_hz(cand as f64);
        for h in 1..=16 {
            let (a, b) = range(f0 * h as f64);
            for v in &mut m[a.min(b)..b] {
                *v *= if h == 1 { 0.0 } else { 0.35 };
            }
        }
    }
    out
}

// -- the analysis -----------------------------------------------------------------------------------------------

/// What to analyse: the part, the stretch (chart seconds), the take (mono) and where it is in the chart.
#[derive(Clone, Debug)]
pub struct TakeInput {
    pub arr: Arrangement,
    pub from: f64,
    pub to: f64,
    pub samples: Vec<f32>,
    pub timing: TakeTiming,
}

/// A chart note as the analysis needs it.
#[derive(Clone, Debug)]
struct ENote {
    pos: Pos,
    techs: Techs,
    bend: f64,
    slide_to: i32,
    sustain: f64,
    /// The note before on its string links into this one (it isn't picked again).
    linked: bool,
}

impl ENote {
    /// The pitches a bend or a pitched slide passes through (it may be heard anywhere on the way).
    fn glide(&self) -> Option<(i32, i32)> {
        if self.bend >= 0.5 {
            return Some((self.pos.midi, self.pos.midi + self.bend.round() as i32));
        }
        if self.slide_to >= 0 && self.slide_to != self.pos.fret && !self.techs.has(Techs::UNPITCHED_SLIDE) {
            let to = self.pos.midi - self.pos.fret + self.slide_to;
            return Some((self.pos.midi.min(to), self.pos.midi.max(to)));
        }
        None
    }
}

#[derive(Clone, Debug)]
struct Ev {
    t: f64,
    notes: Vec<ENote>,
    techs: Techs,
    name: String,
    group: NoteGroup,
    kind: EventKind,
    /// For a cluster (a grace note, a rake): each member's own group, any of which may be what's heard.
    alts: Vec<NoteGroup>,
}

impl Ev {
    fn legato(&self) -> bool {
        self.group.legato
    }
    fn sustain(&self) -> f64 {
        self.notes.iter().map(|n| n.sustain).fold(0.0, f64::max)
    }
    fn expected(&self) -> Vec<Pos> {
        let mut v: Vec<Pos> = self
            .notes
            .iter()
            .filter(|n| !n.techs.has(Techs::FRET_HAND_MUTE))
            .map(|n| n.pos)
            .collect();
        v.sort_by_key(|p| p.string);
        v
    }
    /// Mean fret of its fretted notes (where the hand is).
    fn hand(&self) -> Option<f64> {
        let f: Vec<f64> = self
            .notes
            .iter()
            .filter(|n| n.pos.fret > 0)
            .map(|n| n.pos.fret as f64)
            .collect();
        (!f.is_empty()).then(|| f.iter().sum::<f64>() / f.len() as f64)
    }
}

/// The chart's events, merged exactly as `build_groups` merges them, with their notes.
fn chart_events(arr: &Arrangement) -> Vec<Ev> {
    let groups = build_groups(arr);
    let opens = arr.open_string_midi();
    let mut raw: Vec<(f64, Vec<&Note>, Techs, &str)> = Vec::new();
    for n in &arr.notes {
        raw.push((n.time, vec![n], n.techniques, ""));
    }
    for c in &arr.chords {
        if !c.notes.is_empty() {
            raw.push((c.time, c.notes.iter().collect(), c.techniques, c.name.as_str()));
        }
    }
    raw.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut evs: Vec<Ev> = Vec::new();
    // each string's last note: (time, whether it links into the next)
    let mut last: HashMap<i32, (f64, bool)> = HashMap::new();
    for (t, notes, techs, name) in raw {
        let notes: Vec<&Note> = notes.into_iter().filter(|n| !n.has(Techs::IGNORE)).collect();
        if notes.is_empty() || techs.has(Techs::IGNORE) {
            continue;
        }
        let merge = evs.last().is_some_and(|e| (e.t - t).abs() < 0.015);
        if !merge {
            let g = groups.get(evs.len()).cloned();
            let Some(group) = g else { break };
            evs.push(Ev {
                t,
                notes: vec![],
                techs: Techs::NONE,
                name: String::new(),
                group,
                kind: EventKind::Note,
                alts: vec![],
            });
        }
        let e = evs.last_mut().unwrap();
        if e.name.is_empty() {
            e.name = name.to_string();
        }
        e.techs |= techs;
        for n in notes {
            let midi = detect::note_midi(&opens, n);
            let linked = last
                .get(&n.string)
                .is_some_and(|&(t0, link)| link && n.time - t0 < 2.0 && n.time > t0);
            last.insert(n.string, (n.time, (n.techniques | techs).has(Techs::LINK_NEXT)));
            e.notes.push(ENote {
                linked,
                pos: Pos {
                    string: n.string,
                    fret: n.fret,
                    midi,
                },
                techs: n.techniques | techs,
                bend: n.bend,
                slide_to: if n.slide_to >= 0 {
                    n.slide_to
                } else {
                    n.slide_unpitch_to
                },
                sustain: n.sustain,
            });
        }
    }
    for e in &mut evs {
        // a note slid or linked into isn't picked: like a hammer-on, no pluck to wait for
        if !e.notes.is_empty() && e.notes.iter().all(|n| n.linked) {
            e.group.legato = true;
        }
        e.kind = kind_of(&e.group);
    }
    evs
}

fn kind_of(g: &NoteGroup) -> EventKind {
    if g.muted {
        EventKind::Muted
    } else {
        match g.midis.len() {
            0 | 1 => EventKind::Note,
            2 => EventKind::DoubleStop,
            _ => EventKind::Chord,
        }
    }
}

/// Events close together (wall clock) are matched as one: a grace note hammered straight
/// after its pick, a rake across the strings written note by note. Returns the units and each one's events.
fn clusters(evs: &[Ev], speed: f64) -> (Vec<Ev>, Vec<Vec<usize>>) {
    let gap = 0.05 * speed.max(0.1);
    let gap35 = 0.035 * speed.max(0.1);
    let mut units: Vec<Ev> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (i, e) in evs.iter().enumerate() {
        // (up to 65 ms for a hammered grace note or a note on another string; a picked note on the same
        // string that soon is a fast run, kept apart down to 35 ms)
        let join = members.last().is_some_and(|m| {
            let last = &evs[m[m.len() - 1]];
            let other_string = e.group.strings.iter().all(|s| !last.group.strings.contains(s));
            let dt = e.t - last.t;
            (dt < gap35 || (dt < gap * 1.3 && (e.legato() || other_string)))
                && m.len() < 6
                && !e.group.muted
                && !last.group.muted
        });
        if !join {
            units.push(e.clone());
            members.push(vec![i]);
            continue;
        }
        let u = units.last_mut().unwrap();
        if u.alts.is_empty() {
            u.alts.push(u.group.clone());
        }
        u.alts.push(e.group.clone());
        u.notes.extend(e.notes.iter().cloned());
        u.techs |= e.techs;
        for &m in &e.group.midis {
            if !u.group.midis.contains(&m) {
                u.group.midis.push(m);
            }
        }
        u.group.strings.extend(e.group.strings.iter().copied());
        u.group.repeats.extend(e.group.repeats.iter().copied());
        let own = u.group.midis.clone();
        u.group.ringing.retain(|m| !own.contains(m));
        u.kind = kind_of(&u.group);
        members.last_mut().unwrap().push(i);
    }
    (units, members)
}

/// Spectra of take windows, kept for a while (neighbouring events check the same plucks).
struct SpecCache {
    map: HashMap<(usize, usize), Arc<Vec<f64>>>,
    binhz: f64,
}

impl SpecCache {
    fn get(
        &mut self,
        sp: &mut Spectra,
        x: &[f32],
        rate: u32,
        start: usize,
        n: usize,
    ) -> Option<Arc<Vec<f64>>> {
        if start + n > x.len() || n < 64 {
            return None;
        }
        if let Some(v) = self.map.get(&(start, n)) {
            return Some(v.clone());
        }
        if self.map.len() > 192 {
            self.map.clear();
        }
        let (mag, binhz) = sp.spectrum(&x[start..start + n], rate);
        self.binhz = binhz;
        let v = Arc::new(mag);
        self.map.insert((start, n), v.clone());
        Some(v)
    }
}

struct Ctx<'a> {
    x: &'a [f32],
    rate: u32,
    sp: Spectra,
    cache: SpecCache,
}

impl Ctx<'_> {
    fn spec(&mut self, start: usize, n: usize) -> Option<Arc<Vec<f64>>> {
        self.cache.get(&mut self.sp, self.x, self.rate, start, n)
    }

    /// The scorer's test of group `g` on the sound from `at` (with the sound before `before_at` as the "before").
    fn matches(&mut self, g: &NoteGroup, at: usize, before_at: usize) -> bool {
        if g.muted {
            return true;
        }
        let n = window_for(g, self.rate);
        let Some(mag) = self.spec(at, n) else { return false };
        let prev = if before_at >= n {
            self.spec(before_at - n, n)
        } else {
            None
        };
        let binhz = self.cache.binhz;
        if group_matches_in(&mag, binhz, g, prev.as_deref().map(|v| &v[..])) {
            return true;
        }
        // something else still ringing from before (an extra note, a let-ring) isn't foreign: judge the foreign
        // peaks on what got louder at the pluck
        let Some(prev) = prev else { return false };
        if !detect::enough(g, &detect::group_present_in(&mag, binhz, g, Some(&prev))) {
            return false;
        }
        let grown: Vec<f64> = mag
            .iter()
            .zip(prev.iter())
            .map(|(a, b)| (a - b).max(0.0))
            .collect();
        detect::foreign_share_in(&grown, binhz, &g.midis, &g.ringing) <= detect::FOREIGN_MAX
    }

    /// Whether the pluck at sample `s` played event `e`.
    fn check(&mut self, e: &Ev, s: usize) -> bool {
        let skip = (ATTACK_SKIP * self.rate as f64) as usize;
        if self.matches(&e.group, s + skip, s) {
            return true;
        }
        if e.group.midis.len() >= 2 {
            for later in [0.03, 0.06] {
                if self.matches(&e.group, s + skip + (later * self.rate as f64) as usize, s) {
                    return true; // a slow strum
                }
            }
        }
        // a cluster: any one of its notes heard as written (the grace note may be gone by the window)
        for g in &e.alts {
            if self.matches(g, s + skip, s) {
                return true;
            }
        }
        // a bent double stop: both bends anywhere on their way, together
        if e.notes.len() >= 2 && e.notes.iter().any(|n| n.bend >= 0.5) {
            let top = e.notes.iter().map(|n| n.bend.round() as i32).max().unwrap_or(0);
            let mut g = e.group.clone();
            for k in 1..=top {
                g.midis = e
                    .notes
                    .iter()
                    .filter(|n| !n.techs.has(Techs::FRET_HAND_MUTE))
                    .map(|n| n.pos.midi + k.min(n.bend.round() as i32))
                    .collect();
                if self.matches(&g, s + skip, s) {
                    return true;
                }
            }
        }
        // a shape sliding as one (a double stop slid up): the shape anywhere on its way
        if e.notes.len() >= 2 && e.notes.iter().all(|n| n.bend < 0.5) {
            let d: Vec<i32> = e
                .notes
                .iter()
                .map(|n| {
                    if n.slide_to >= 0 {
                        n.slide_to - n.pos.fret
                    } else {
                        0
                    }
                })
                .collect();
            if d[0] != 0 && d.iter().all(|&x| x == d[0]) {
                let mut g = e.group.clone();
                for k in 1..=d[0].abs() {
                    g.midis = e.group.midis.iter().map(|m| m + k * d[0].signum()).collect();
                    if self.matches(&g, s + skip, s) {
                        return true;
                    }
                }
            }
        }
        // a bend or slide already on its way (or a pre-bend): any pitch it passes through
        if let [n] = &e.notes[..]
            && let Some((lo, hi)) = n.glide()
        {
            let mut g = e.group.clone();
            for m in lo..=hi {
                if m == n.pos.midi {
                    continue;
                }
                g.midis = vec![m];
                if self.matches(&g, s + skip, s) {
                    return true;
                }
            }
        }
        false
    }
}

/// A prefix-max Fenwick tree over pluck indices: (best total, entry) among chains ending before a pluck.
struct Fenwick {
    t: Vec<(f64, usize)>,
}

impl Fenwick {
    fn new(n: usize) -> Fenwick {
        Fenwick {
            t: vec![(f64::NEG_INFINITY, usize::MAX); n + 1],
        }
    }
    /// The best over plucks [0, i).
    fn query(&self, i: usize) -> (f64, usize) {
        let mut best = (f64::NEG_INFINITY, usize::MAX);
        let mut k = i;
        while k > 0 {
            if self.t[k].0 > best.0 {
                best = self.t[k];
            }
            k &= k - 1;
        }
        best
    }
    fn update(&mut self, i: usize, v: (f64, usize)) {
        let mut k = i + 1;
        while k < self.t.len() {
            if v.0 > self.t[k].0 {
                self.t[k] = v;
            }
            k += k & k.wrapping_neg();
        }
    }
}

/// Events matched to plucks in order (each pluck at most once): (pluck for each event, whether it was right).
fn align(
    evs: &[Ev],
    exp: &[f64],
    ons: &[TakeOnset],
    shift: f64,
    rate: u32,
    ctx: &mut Ctx,
    checked: &mut HashMap<(usize, usize), bool>,
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
    p0: f32,
    p1: f32,
) -> Option<Vec<Option<(usize, bool)>>> {
    let (early, late) = (EARLY * rate as f64, LATE * rate as f64);
    let samples: Vec<f64> = ons.iter().map(|o| o.sample as f64).collect();
    struct Entry {
        event: usize,
        onset: usize,
        ok: bool,
        prev: usize,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut fw = Fenwick::new(ons.len());
    for (i, e) in evs.iter().enumerate() {
        if i % 16 == 0 {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            progress(p0 + (p1 - p0) * i as f32 / evs.len().max(1) as f32);
        }
        let centre = exp[i] + shift;
        let a = samples.partition_point(|&s| s < centre - early);
        let b = samples.partition_point(|&s| s <= centre + late);
        let mut new = Vec::new();
        for j in a..b {
            let ok = *checked
                .entry((i, j))
                .or_insert_with(|| ctx.check(e, ons[j].sample));
            let d = samples[j] - centre;
            let off = if d < 0.0 { -d / early } else { d / late };
            // a muted strum takes any pluck, so it's worth less than a pitch heard right
            let right = if e.group.muted { 1.2 } else { 2.0 };
            // timing counts a lot: a repeated shape mustn't slide onto its neighbour's pluck to look right
            let v = if ok { right - 1.6 * off } else { 0.9 - 1.8 * off };
            let (pv, pid) = fw.query(j);
            let (total, prev) = if pv > 0.0 { (pv + v, pid) } else { (v, usize::MAX) };
            new.push((j, total, ok, prev));
        }
        for (j, total, ok, prev) in new {
            entries.push(Entry {
                event: i,
                onset: j,
                ok,
                prev,
            });
            fw.update(j, (total, entries.len() - 1));
        }
    }
    let mut out = vec![None; evs.len()];
    let (_, mut id) = fw.query(ons.len());
    while id != usize::MAX {
        let en = &entries[id];
        out[en.event] = Some((en.onset, en.ok));
        id = en.prev;
    }
    Some(out)
}

/// Analyse a take. `progress` gets 0..1; returns None when cancelled.
pub fn analyse(input: &TakeInput, progress: &dyn Fn(f32), cancel: &AtomicBool) -> Option<TakeReport> {
    let started = Instant::now();
    let arr = &input.arr;
    let timing = &input.timing;
    let rate = timing.rate.max(1);
    let x = &input.samples[..];
    let neck = Neck::new(arr);
    progress(0.0);
    let ons = onsets::detect(x, rate);
    progress(0.2);
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let orig: Vec<Ev> = chart_events(arr)
        .into_iter()
        .filter(|e| e.t >= input.from - 1e-3 && e.t < input.to)
        .collect();
    let (evs, members) = clusters(&orig, timing.speed);
    let exp: Vec<f64> = evs.iter().map(|e| timing.sample_at(e.t)).collect();
    let mut ctx = Ctx {
        x,
        rate,
        sp: Spectra::new(),
        cache: SpecCache {
            map: HashMap::new(),
            binhz: 1.0,
        },
    };
    let mut checked = HashMap::new();
    // pass 1, then centred on the take's own offset
    let first = align(
        &evs,
        &exp,
        &ons,
        0.0,
        rate,
        &mut ctx,
        &mut checked,
        cancel,
        progress,
        0.2,
        0.5,
    )?;
    let mut d: Vec<f64> = first
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.filter(|m| m.1).map(|m| ons[m.0].sample as f64 - exp[i]))
        .collect();
    let shift = if d.len() >= 4 {
        d.sort_by(|a, b| a.total_cmp(b));
        d[d.len() / 2].clamp(-0.25 * rate as f64, 0.25 * rate as f64)
    } else {
        0.0
    };
    let matched = if shift.abs() > 0.01 * rate as f64 {
        let mut fresh = HashMap::new();
        align(
            &evs, &exp, &ons, shift, rate, &mut ctx, &mut fresh, cancel, progress, 0.5, 0.75,
        )?
    } else {
        first
    };
    progress(0.75);

    // bars of the stretch
    let all_bars = Bars::new(arr);
    let span = all_bars.in_span(input.from, input.to);
    let mut bars: Vec<BarResult> = span
        .clone()
        .enumerate()
        .map(|(k, i)| {
            let b = &all_bars.bars[i];
            BarResult {
                number: b.number,
                index: k + 1,
                t0: b.t0,
                t1: b.t1,
                hits: 0,
                wrong: 0,
                missed: 0,
                not_checked: 0,
                extras: 0,
                mean_offset_ms: None,
            }
        })
        .collect();
    if bars.is_empty() {
        bars.push(BarResult {
            number: 1,
            index: 1,
            t0: input.from,
            t1: input.to,
            hits: 0,
            wrong: 0,
            missed: 0,
            not_checked: 0,
            extras: 0,
            mean_offset_ms: None,
        });
    }
    let bar_of = |t: f64| -> usize {
        match all_bars.at(t) {
            Some(i) if span.contains(&i) => i - span.start,
            _ => bars
                .iter()
                .rposition(|b| b.t0 <= t + 0.02)
                .unwrap_or(0)
                .min(bars.len() - 1),
        }
    };

    let skip = (ATTACK_SKIP * rate as f64) as usize;
    let mut used: HashSet<usize> = HashSet::new();
    let mut results: Vec<EventResult> = Vec::with_capacity(evs.len());
    for (i, e) in evs.iter().enumerate() {
        if i % 16 == 0 {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            progress(0.75 + 0.2 * i as f32 / evs.len().max(1) as f32);
        }
        let expected = e.expected();
        let hand = e.hand().or_else(|| neighbour_hand(&evs, i)).unwrap_or(5.0);
        let mut r = EventResult {
            t: e.t,
            bar: bar_of(e.t),
            kind: e.kind,
            techniques: e.techs | e.notes.iter().fold(Techs::NONE, |a, n| a | n.techs),
            chord_name: e.name.clone(),
            expected: expected.clone(),
            status: Status::Missed,
            played: vec![],
            missing: vec![],
            offset_ms: None,
            sample: None,
            note: None,
        };
        // the next pluck after this event's (bounds the window for transcribing)
        match matched[i] {
            Some((j, ok)) => {
                used.insert(j);
                let s = ons[j].sample;
                // a strum is timed by its first string: an unclaimed pluck just before it is its start
                let mut first = j;
                let claimed_before = i > 0 && matched[i - 1].is_some_and(|m| m.0 + 1 >= first);
                if e.group.midis.len() >= 2 && !claimed_before {
                    while first > 0
                        && s - ons[first - 1].sample <= (0.06 * rate as f64) as usize
                        && !(i > 0 && matched[i - 1].is_some_and(|m| m.0 == first - 1))
                    {
                        first -= 1;
                        used.insert(first);
                    }
                }
                let s0 = ons[first].sample;
                r.sample = Some(s0);
                r.offset_ms = Some((s0 as f64 - exp[i]) / rate as f64 * 1000.0);
                let next = ons.get(j + 1).map_or(x.len(), |o| o.sample);
                // a single note repeating the one before passes the scorer's test on that note still ringing:
                // listen to what got louder instead
                let repeat = expected.len() == 1 && e.group.repeats.contains(&expected[0].midi);
                // and a single note confirmed by its own pitch: the scorer's test lets through a neighbouring
                // semitone that was just played (it counts as "still ringing", and leaks into the expected bin)
                let confirmed = !ok || expected.len() != 1 || {
                    let want = expected[0].midi;
                    let glide = e.notes.first().and_then(ENote::glide);
                    hear(&mut ctx, s, s + skip, next, 1).first().is_none_or(|&m| {
                        m == want
                            || (m - want).abs() == 12
                            || glide.is_some_and(|(lo, hi)| (lo..=hi).contains(&m))
                    })
                };
                if ok && (repeat || !confirmed) {
                    transcribe(&mut ctx, e, &neck, s + skip, next, hand, &mut r);
                    if r.note.as_deref() == Some("right pitch, sound not clear") {
                        r.note = None;
                    }
                    if r.status == Status::Wrong && r.played.is_empty() {
                        // it passed the scorer's test and nothing else was heard clearly
                        r.status = Status::Hit;
                        r.played = expected.clone();
                        r.note = None;
                    }
                } else if ok {
                    r.status = Status::Hit;
                    r.played = expected.clone();
                    if expected.len() >= 2 {
                        chord_notes(&mut ctx, e, s + skip, &mut r);
                    }
                } else {
                    transcribe(&mut ctx, e, &neck, s + skip, next, hand, &mut r);
                }
                if r.status == Status::Hit {
                    // the note rings until the next chart note's pluck (a bend's own wobble may look like plucks)
                    let until = (i + 1..evs.len())
                        .find_map(|k| matched[k].map(|m| ons[m.0].sample))
                        .or_else(|| exp.get(i + 1).map(|&e| (e + shift).max(0.0) as usize))
                        .unwrap_or(x.len());
                    follow_bend_and_slide(&mut ctx, e, &neck, s, until, timing.speed, &mut r);
                }
            }
            None => {
                let at = (exp[i] + shift).max(0.0) as usize;
                let probe = at + (0.02 * rate as f64) as usize;
                let mut g = e.group.clone();
                if !e.legato() {
                    g.repeats.clear(); // a dropped repeat mustn't pass on the note before still ringing
                }
                // a soft pick with no pluck found still makes the sound louder; a legato note needn't
                let n = (0.06 * rate as f64) as usize;
                let rms = |a: usize| {
                    let w = &x[a.min(x.len())..(a + n).min(x.len())];
                    (w.iter().map(|v| v * v).sum::<f32>() / w.len().max(1) as f32).sqrt()
                };
                let louder = at >= n && rms(probe) > 1.3 * rms(at - n);
                if !g.muted
                    && (e.legato() || louder)
                    && (ctx.matches(&g, probe, at) || (e.legato() && ctx.matches(&g, probe + skip * 3, at)))
                {
                    r.status = Status::Hit;
                    r.played = expected.clone();
                    r.note = Some(if e.legato() {
                        "heard without a pluck (legato)".into()
                    } else {
                        "heard, but no clear pluck".into()
                    });
                } else if e.legato() || e.group.muted || e.techs.has(Techs::PINCH_HARMONIC) {
                    r.status = Status::NotChecked;
                    r.note = Some(if e.group.muted {
                        "muted strum not heard (too quiet to tell)".into()
                    } else if e.legato() {
                        "hammer-on / pull-off / slide not clearly heard".into()
                    } else {
                        "pinch harmonic not clearly heard".into()
                    });
                }
            }
        }
        results.push(r);
    }
    let results = expand(results, &members, &orig, &bar_of);

    // extras: plucks that played nothing
    let used_samples: Vec<usize> = {
        let mut v: Vec<usize> = used.iter().map(|&j| ons[j].sample).collect();
        v.sort_unstable();
        v
    };
    let lo_s = timing.sample_at(input.from - 0.15);
    let hi_s = timing.sample_at(input.to);
    let mut extras = Vec::new();
    for (j, o) in ons.iter().enumerate() {
        if used.contains(&j) || (o.sample as f64) < lo_s || (o.sample as f64) > hi_s {
            continue;
        }
        let s = o.sample;
        let k = used_samples.partition_point(|&u| u <= s);
        let after_used = k > 0 && s - used_samples[k - 1] <= (0.06 * rate as f64) as usize;
        let before_used = k < used_samples.len() && used_samples[k] - s <= (0.06 * rate as f64) as usize;
        if after_used || before_used || o.strength < 0.04 {
            continue;
        }
        let next = ons.get(j + 1).map_or(x.len(), |o| o.sample);
        let heard = if o.attack {
            // one note first (an extra is most often a single note), then a chord's worth
            let one = hear(&mut ctx, s, s + skip, next, 1);
            if one.is_empty() {
                hear(&mut ctx, s, s + skip, next, 3)
            } else {
                one
            }
        } else {
            // no attack: only a clear single new pitch counts (a hammered extra note), not a ringing chord's
            // partials beating
            let n = (0.08 * rate as f64) as usize;
            if s + skip + n > x.len() {
                vec![]
            } else {
                let (hz, clarity) =
                    mpm_pitch(&mut ctx.sp, &x[s + skip..s + skip + n], rate, 60.0, 1400.0, 0.9);
                if hz > 0.0 && clarity >= 0.9 {
                    // the pitch itself from what got louder (a time-domain pitch of a mix slips octaves)
                    let one = hear(&mut ctx, s, s + skip, next, 1);
                    if one.is_empty() {
                        vec![hz_to_midi(hz).round() as i32]
                    } else {
                        one
                    }
                } else {
                    vec![]
                }
            }
        };
        if heard.is_empty() {
            continue;
        }
        let t = timing.chart_time(s as f64);
        // a bend or slide moving (or vibrato) within a note that's ringing isn't a new note
        let moving = results.iter().any(|r| {
            r.status == Status::Hit
                && r.sample.is_some_and(|rs| rs < s)
                && (r.techniques & (Techs::BEND | Techs::VIBRATO | Techs::SLIDE | Techs::UNPITCHED_SLIDE))
                    != Techs::NONE
                && t <= r.t + orig.iter().find(|e| e.t == r.t).map_or(0.0, Ev::sustain) + 0.1
                && heard
                    .iter()
                    .any(|&m| r.expected.iter().any(|p| (p.midi - m).abs() <= 3))
        });
        if moving {
            continue;
        }
        // no attack and only notes already ringing: the sound changing (beats, a let-ring), not a note
        if !o.attack
            && heard.iter().all(|m| {
                results
                    .iter()
                    .filter(|r| r.status == Status::Hit && r.t <= t && t - r.t < 3.0)
                    .any(|r| r.played.iter().any(|p| p.midi == *m))
            })
        {
            continue;
        }
        let near = nearest_event(&orig, t);
        let prefer = near.and_then(|e| (e.notes.len() == 1).then(|| e.notes[0].pos.string));
        let hand = near.and_then(Ev::hand).unwrap_or(5.0);
        let played = heard
            .iter()
            .filter_map(|&m| neck.place(m, prefer, hand))
            .collect::<Vec<_>>();
        if played.is_empty() {
            continue;
        }
        let bar = (t >= input.from - 0.15 && t < input.to).then(|| bar_of(t.max(input.from)));
        extras.push(Extra {
            t,
            sample: s,
            bar,
            played,
        });
    }

    // bars and timing
    for r in &results {
        let b = &mut bars[r.bar];
        match r.status {
            Status::Hit => b.hits += 1,
            Status::Wrong => b.wrong += 1,
            Status::Missed => b.missed += 1,
            Status::NotChecked => b.not_checked += 1,
        }
    }
    for e in &extras {
        if let Some(b) = e.bar {
            bars[b].extras += 1;
        }
    }
    for (k, b) in bars.iter_mut().enumerate() {
        let o: Vec<f64> = results
            .iter()
            .filter(|r| r.bar == k && r.status == Status::Hit)
            .filter_map(|r| r.offset_ms)
            .collect();
        if !o.is_empty() {
            b.mean_offset_ms = Some(o.iter().sum::<f64>() / o.len() as f64);
        }
    }
    let timing_summary = summarise_timing(&results);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let mut report = TakeReport {
        artist: arr.artist.clone(),
        title: arr.title.clone(),
        part: arr.name.clone(),
        from: input.from,
        to: input.to,
        speed: timing.speed,
        rate,
        seconds: x.len() as f64 / rate as f64,
        neck,
        events: results,
        extras,
        bars,
        timing: timing_summary,
        problems: vec![],
        shift_ms: shift / rate as f64 * 1000.0,
        plucks_found: ons.len(),
        peak,
        analysis_ms: 0.0,
    };
    report.problems = problems(&report);
    report.analysis_ms = started.elapsed().as_secs_f64() * 1000.0;
    progress(1.0);
    Some(report)
}

/// Each cluster's result handed to its events: right if the cluster was, or (when wrong) if the event's own
/// notes were heard; the first event carries the timing, the others are "with the note before".
fn expand(
    unit_results: Vec<EventResult>,
    members: &[Vec<usize>],
    orig: &[Ev],
    bar_of: &dyn Fn(f64) -> usize,
) -> Vec<EventResult> {
    let mut out = Vec::with_capacity(orig.len());
    for (r, mem) in unit_results.into_iter().zip(members) {
        if mem.len() == 1 {
            out.push(r);
            continue;
        }
        for (k, &i) in mem.iter().enumerate() {
            let e = &orig[i];
            let expected = e.expected();
            let mut er = EventResult {
                t: e.t,
                bar: bar_of(e.t),
                kind: e.kind,
                techniques: e.techs | e.notes.iter().fold(Techs::NONE, |a, n| a | n.techs),
                chord_name: e.name.clone(),
                expected: expected.clone(),
                status: r.status,
                played: vec![],
                missing: vec![],
                offset_ms: if k == 0 { r.offset_ms } else { None },
                sample: r.sample,
                note: if k == 0 {
                    r.note.clone()
                } else {
                    Some("with the note before (a grace note or a rake)".into())
                },
            };
            match r.status {
                Status::Hit => er.played = expected.clone(),
                Status::Wrong => {
                    let missing: Vec<Pos> = expected
                        .iter()
                        .filter(|p| r.missing.contains(p) || !r.played.iter().any(|q| q.midi == p.midi))
                        .copied()
                        .collect();
                    // a grace note hammered straight on (a later note of the cluster on its string): too short
                    // to tell, given the benefit
                    let grace = mem[k + 1..].iter().any(|&j| {
                        orig[j].legato() && orig[j].group.strings.iter().any(|s| e.group.strings.contains(s))
                    });
                    if missing.is_empty() || grace {
                        er.status = Status::Hit;
                        er.played = expected.clone();
                        if grace && !missing.is_empty() {
                            er.note = Some("grace note, too short to hear clearly".into());
                        }
                    } else {
                        er.played = r
                            .played
                            .iter()
                            .filter(|q| expected.iter().any(|p| p.string == q.string))
                            .copied()
                            .collect();
                        if er.played.is_empty() && k == 0 {
                            er.played = r.played.clone();
                        }
                        er.missing = missing;
                    }
                }
                _ => {}
            }
            out.push(er);
        }
    }
    out
}

fn nearest_event(evs: &[Ev], t: f64) -> Option<&Ev> {
    evs.iter()
        .min_by(|a, b| (a.t - t).abs().total_cmp(&(b.t - t).abs()))
}

/// Where the hand is around event i (its neighbours' frets), for an event of open strings.
fn neighbour_hand(evs: &[Ev], i: usize) -> Option<f64> {
    let t = evs[i].t;
    let f: Vec<f64> = evs
        .iter()
        .filter(|e| (e.t - t).abs() < 2.0)
        .filter_map(Ev::hand)
        .collect();
    (!f.is_empty()).then(|| f.iter().sum::<f64>() / f.len() as f64)
}

/// Pitches heard from `at` (until `next`, the next pluck; the pluck itself at `pluck`): one by MPM when clear and
/// nothing much was ringing before, else the strongest harmonic series (up to `max_n`) in what got louder.
fn hear(ctx: &mut Ctx, pluck: usize, at: usize, next: usize, max_n: usize) -> Vec<i32> {
    let rate = ctx.rate as f64;
    let n = ((next.saturating_sub(at)) as f64)
        .clamp(0.04 * rate, 0.12 * rate)
        .min(ctx.x.len().saturating_sub(at) as f64) as usize;
    if n < (0.03 * rate) as usize {
        return vec![];
    }
    let rms = |w: &[f32]| (w.iter().map(|v| v * v).sum::<f32>() / w.len().max(1) as f32).sqrt();
    let level = rms(&ctx.x[at..at + n]);
    if level < 1e-4 {
        return vec![];
    }
    let before = if pluck >= n {
        rms(&ctx.x[pluck - n..pluck])
    } else {
        0.0
    };
    if before > 0.3 * level
        && let (Some(mag), Some(prev)) = (ctx.spec(at, n), ctx.spec(pluck - n, n))
    {
        let grown: Vec<f64> = mag
            .iter()
            .zip(prev.iter())
            .map(|(a, b)| (a - b).max(0.0))
            .collect();
        let found = pitches_in(&grown, ctx.cache.binhz, 28, 90, max_n);
        if !found.is_empty() {
            return found.into_iter().map(|p| p.0).collect();
        }
    }
    let w = &ctx.x[at..at + n];
    let (hz, clarity) = mpm_pitch(&mut ctx.sp, w, ctx.rate, 60.0, 1400.0, 0.9);
    if max_n == 1 && hz > 0.0 && clarity >= 0.8 {
        return vec![hz_to_midi(hz).round() as i32];
    }
    let found = pitches(&mut ctx.sp, w, ctx.rate, 28, 90, max_n);
    if found.is_empty() && hz > 0.0 && clarity >= 0.8 {
        return vec![hz_to_midi(hz).round() as i32];
    }
    if max_n > 1 && hz > 0.0 && clarity >= 0.9 && found.len() <= 1 {
        return vec![hz_to_midi(hz).round() as i32];
    }
    found.into_iter().map(|p| p.0).collect()
}

/// A chord or double stop that passed the scorer's test (`detect::need` of its notes): which notes didn't sound. A
/// double stop missing one is wrong; a chord of three or more stays right with as many missing as the scorer
/// forgives (`detect::enough`: half of them, D46, and two pitch classes, so a power chord without its fifth is
/// still wrong: another sound), noted, unless a wrong fret sounded instead.
fn chord_notes(ctx: &mut Ctx, e: &Ev, at: usize, r: &mut EventResult) {
    let n = window_for(&e.group, ctx.rate);
    let skip = (ATTACK_SKIP * ctx.rate as f64) as usize;
    let pluck = at.saturating_sub(skip);
    let prev = if pluck >= n { ctx.spec(pluck - n, n) } else { None };
    let mut missing: Vec<Pos> = r.expected.clone();
    // any of the windows a strum may need
    for later in [0.0, 0.03, 0.06] {
        let Some(mag) = ctx.spec(at + (later * ctx.rate as f64) as usize, n) else {
            continue;
        };
        let binhz = ctx.cache.binhz;
        missing.retain(|p| {
            // by its fundamental (below 100 Hz a guitar's fundamental is weak: its 2nd harmonic counts too,
            // unless that's another note of the chord)
            let octave_in_chord = r.expected.iter().any(|q| q.midi == p.midi + 12);
            let test = if midi_to_hz(p.midi as f64) < 100.0 && !octave_in_chord {
                note_present_in
            } else {
                detect::fundamental_present_in
            };
            // (without the louder-than-before test too: a bend's top may still be ringing on that pitch)
            let repeat = e.group.repeats.contains(&p.midi);
            !test(
                &mag,
                binhz,
                p.midi,
                prev.as_deref().map(|v| &v[..]),
                repeat,
            ) && !test(&mag, binhz, p.midi, None, true)
                // (a chord struck again: its quiet low string is still a peak of its own)
                && !(repeat && !octave_in_chord && detect::fundamental_ringing_in(&mag, binhz, p.midi))
        });
    }
    if missing.is_empty() {
        return;
    }
    r.played.retain(|p| !missing.contains(p));
    // a missing note with a fret or two either side of it sounding instead: a wrong fret on that string
    let mut wrong_fret = Vec::new();
    if let Some(mag) = ctx.spec(at, n) {
        let binhz = ctx.cache.binhz;
        for p in &missing {
            for d in [-1, 1, -2, 2] {
                let m = p.midi + d;
                if p.fret + d >= 0
                    && !r.expected.iter().any(|q| q.midi == m)
                    && detect::fundamental_present_in(&mag, binhz, m, prev.as_deref().map(|v| &v[..]), false)
                {
                    wrong_fret.push(Pos {
                        string: p.string,
                        fret: p.fret + d,
                        midi: m,
                    });
                    break;
                }
            }
        }
    }
    // as many as the scorer forgives (`detect::enough`: D46, half of the notes and two of their pitch classes, so
    // a three-note power chord without its root, struck again within a quarter second, is still the chord: the
    // lowest string of a quick strum often barely rings), unless a wrong fret sounded instead
    let sounded: Vec<i32> = r
        .expected
        .iter()
        .filter(|p| !missing.contains(p))
        .map(|p| p.midi)
        .collect();
    let forgiven = wrong_fret.is_empty() && detect::enough(&e.group, &sounded);
    r.played.extend(wrong_fret);
    r.played.sort_by_key(|p| p.string);
    if !forgiven {
        r.status = Status::Wrong;
    } else {
        r.note = Some(format!(
            "{} didn't sound",
            missing.iter().map(Pos::name).collect::<Vec<_>>().join(", ")
        ));
    }
    r.missing = missing;
}

/// A matched pluck that failed the check: what was played instead.
fn transcribe(ctx: &mut Ctx, e: &Ev, neck: &Neck, at: usize, next: usize, hand: f64, r: &mut EventResult) {
    let expected = r.expected.clone();
    if e.group.muted {
        r.status = Status::Hit;
        return;
    }
    let single = expected.len() == 1;
    let pluck = at.saturating_sub((ATTACK_SKIP * ctx.rate as f64) as usize);
    let heard = hear(ctx, pluck, at, next, if single { 1 } else { expected.len() + 1 });
    if single {
        let want = expected[0];
        let glide = e.notes.first().and_then(ENote::glide);
        match heard.first() {
            // (an octave off is the pitch reading's slip far more often than the player's)
            Some(&m) if m == want.midi || (m - want.midi).abs() == 12 => {
                r.status = Status::Hit;
                r.played = expected;
                r.note = Some("right pitch, sound not clear".into());
            }
            Some(&m) if glide.is_some_and(|(lo, hi)| (lo..=hi).contains(&m)) => {
                r.status = Status::Hit;
                r.played = expected;
                r.note = Some(if e.notes[0].bend >= 0.5 {
                    "heard on its way up the bend".into()
                } else {
                    "heard on its way along the slide".into()
                });
            }
            Some(&m) => {
                r.status = Status::Wrong;
                r.played = neck.place(m, Some(want.string), hand).into_iter().collect();
            }
            None => {
                r.status = Status::Wrong;
                r.note = Some("no clear pitch (muted or dead note?)".into());
            }
        }
        return;
    }
    // two or more: which of the chord's notes are there, and what else
    let n = window_for(&e.group, ctx.rate);
    let mag = ctx.spec(at, n);
    let before_at = at.saturating_sub((ATTACK_SKIP * ctx.rate as f64) as usize);
    let prev = if before_at >= n {
        ctx.spec(before_at - n, n)
    } else {
        None
    };
    let binhz = ctx.cache.binhz;
    let mut present: Vec<Pos> = Vec::new();
    let mut missing: Vec<Pos> = Vec::new();
    for p in &expected {
        let by_peak = mag.as_ref().is_some_and(|m| {
            note_present_in(
                m,
                binhz,
                p.midi,
                prev.as_deref().map(|v| &v[..]),
                e.group.repeats.contains(&p.midi),
            )
        });
        // the same pitch still ringing from before can't be told from it played again: given the benefit
        let ringing_same = mag
            .as_ref()
            .is_some_and(|m| note_present_in(m, binhz, p.midi, None, true))
            && prev
                .as_ref()
                .is_some_and(|m| note_present_in(m, binhz, p.midi, None, true));
        if by_peak || heard.contains(&p.midi) || ringing_same {
            present.push(*p);
        } else {
            missing.push(*p);
        }
    }
    // a bend or slide on its way: the pitches it passes through belong to it
    let glides: Vec<(Pos, (i32, i32))> = e
        .notes
        .iter()
        .filter_map(|n| n.glide().map(|g| (n.pos, g)))
        .collect();
    for (p, (lo, hi)) in &glides {
        if missing.contains(p) && heard.iter().any(|m| (*lo..=*hi).contains(m)) {
            missing.retain(|q| q != p);
            present.push(*p);
        }
    }
    let foreign: Vec<i32> = heard
        .iter()
        .copied()
        .filter(|m| !expected.iter().any(|p| p.midi == *m))
        .filter(|m| !glides.iter().any(|(_, (lo, hi))| (*lo..=*hi).contains(m)))
        // an octave above a note that's there is most likely its harmonic
        .filter(|m| !present.iter().any(|p| m - p.midi == 12 || m - p.midi == 19))
        .collect();
    if foreign.is_empty() && missing.is_empty() {
        r.status = Status::Hit;
        r.played = expected;
        r.note = Some(if glides.is_empty() {
            "all the notes heard, sound not clear".into()
        } else {
            "heard on its way through the bend / slide".into()
        });
        return;
    }
    r.status = Status::Wrong;
    // a foreign pitch a fret or two from a chart note that MPM / the harmonic sums didn't hear: that string a
    // fret off (the chart note only looked present by the window's leakage)
    for &m in &foreign {
        if let Some(k) = present
            .iter()
            .position(|p| (p.midi - m).abs() <= 2 && !heard.contains(&p.midi))
        {
            missing.push(present.remove(k));
        }
    }
    let mut played = present.clone();
    for m in foreign {
        // on the string of the expected note nearest in pitch, if it fits there
        let near = expected
            .iter()
            .min_by_key(|p| (p.midi - m).abs())
            .map(|p| p.string);
        if let Some(p) = neck.place(m, near, hand)
            && !played.iter().any(|q| q.string == p.string)
        {
            played.push(p);
        } else if let Some(p) = neck.place(m, None, hand) {
            played.push(p);
        }
    }
    played.sort_by_key(|p| p.string);
    r.played = played;
    r.missing = missing;
}

/// A right note's bend (followed to its top) and slide (where it ends).
fn follow_bend_and_slide(
    ctx: &mut Ctx,
    e: &Ev,
    neck: &Neck,
    s: usize,
    next: usize,
    speed: f64,
    r: &mut EventResult,
) {
    let [n] = &e.notes[..] else { return };
    let rate = ctx.rate as f64;
    let dur = (n.sustain / speed.max(0.1)).min(1.5);
    let end = (s + (dur * rate) as usize).min(next).min(ctx.x.len());
    let start = s + (0.03 * rate) as usize;
    let base = n.pos.midi as f64;
    if n.bend >= 0.5 {
        let ups = track(ctx, start, end, base, -0.7, n.bend + 1.5);
        if ups.len() < 3 {
            r.note = Some("bend not checked".into());
        } else {
            let mut sorted = ups.clone();
            sorted.sort_by(|a, b| a.total_cmp(b));
            let top = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)];
            if top < n.bend - 0.5 {
                r.status = Status::Wrong;
                r.note = Some(format!(
                    "bend reached +{:.1} of +{}",
                    top.max(0.0),
                    fmt_steps(n.bend)
                ));
                r.played = vec![Pos {
                    midi: n.pos.midi + top.round() as i32,
                    ..n.pos
                }];
            } else if top > n.bend + 0.6 {
                r.note = Some(format!("bend went past: +{top:.1} for +{}", fmt_steps(n.bend)));
            }
        }
    } else if n.slide_to >= 0 && n.slide_to != n.pos.fret && !n.techs.has(Techs::UNPITCHED_SLIDE) {
        let want = neck.pos(n.pos.string, n.slide_to).midi;
        let span = (want - n.pos.midi) as f64;
        let path = track(ctx, start, end, base, span.min(0.0) - 1.5, span.max(0.0) + 1.5);
        let tail = &path[path.len() * 2 / 3..];
        if tail.len() < 2 {
            r.note = Some("slide not checked".into());
        } else {
            let got = n.pos.midi + median(tail).round() as i32;
            // (a slide often arrives in the last moment: a semitone short is fine)
            if (got - want).abs() > 1 {
                r.note = Some(format!(
                    "slide ended on {} rather than {}",
                    note_name(got),
                    note_name(want)
                ));
            }
        }
    }
}

/// A note's pitch through [start, end), as semitones from `base` within lo..hi, every 15 ms where it's clearly
/// there: the strongest of the fundamental plus second harmonic over that range (other strings ringing elsewhere
/// don't disturb it, as a time-domain pitch of the mix would be).
fn track(ctx: &mut Ctx, start: usize, end: usize, base: f64, lo: f64, hi: f64) -> Vec<f64> {
    let rate = ctx.rate as f64;
    let frame = (0.04 * rate) as usize;
    let step = (0.015 * rate) as usize;
    let mut out = Vec::new();
    let mut k = start;
    while k + frame <= end {
        let (mag, binhz) = ctx.sp.spectrum(&ctx.x[k..k + frame], ctx.rate);
        let at = |f: f64| -> f64 {
            let i = (f / binhz).round() as usize;
            if i < 1 || i + 1 >= mag.len() {
                0.0
            } else {
                mag[i - 1].max(mag[i]).max(mag[i + 1])
            }
        };
        let band = &mag[(60.0 / binhz) as usize..((2500.0 / binhz) as usize).min(mag.len())];
        let top = band.iter().copied().fold(0.0, f64::max);
        let mut best = (0.0, lo);
        let mut d = lo;
        while d <= hi + 1e-9 {
            let f = midi_to_hz(base + d);
            let v = at(f) + at(2.0 * f);
            if v > best.0 {
                best = (v, d);
            }
            d += 0.1;
        }
        // the exact peak near the best's second harmonic (parabolic between bins: a hundredth of a semitone)
        let f2 = 2.0 * midi_to_hz(base + best.1);
        let c = (f2 / binhz).round() as usize;
        if c > 4 && c + 4 < mag.len() {
            let k = (c - 3..=c + 3)
                .max_by(|&a, &b| mag[a].total_cmp(&mag[b]))
                .unwrap_or(c);
            let (a, b, cc) = (
                (mag[k - 1] + 1e-12).ln(),
                (mag[k] + 1e-12).ln(),
                (mag[k + 1] + 1e-12).ln(),
            );
            let den = a - 2.0 * b + cc;
            let kf = k as f64 + if den != 0.0 { 0.5 * (a - cc) / den } else { 0.0 };
            let d = 12.0 * (kf * binhz / 2.0 / midi_to_hz(base)).log2();
            if (d - best.1).abs() <= 0.15 {
                best.1 = d;
            }
        }
        let f = midi_to_hz(base + best.1);
        if top > 0.0 && at(f).max(at(2.0 * f)) >= 0.2 * top {
            out.push(best.1);
        }
        k += step;
    }
    out
}

fn fmt_steps(b: f64) -> String {
    if (b - b.round()).abs() < 0.05 {
        format!("{}", b.round() as i64)
    } else {
        format!("{b:.1}")
    }
}

fn summarise_timing(results: &[EventResult]) -> TimingSummary {
    let timed: Vec<(EventKind, f64)> = results
        .iter()
        .filter(|r| r.status == Status::Hit)
        .filter_map(|r| r.offset_ms.map(|o| (r.kind, o)))
        .collect();
    let o: Vec<f64> = timed.iter().map(|t| t.1).collect();
    if o.is_empty() {
        return TimingSummary {
            n: 0,
            mean_ms: 0.0,
            median_ms: 0.0,
            spread_ms: 0.0,
            tendency: Tendency::Unknown,
            by_kind: vec![],
        };
    }
    let mean = o.iter().sum::<f64>() / o.len() as f64;
    let med = median(&o);
    let spread = (o.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / o.len() as f64).sqrt();
    let tendency = if o.len() < 3 {
        Tendency::Unknown
    } else if med > ON_TIME_MS {
        Tendency::Dragging
    } else if med < -ON_TIME_MS {
        Tendency::Rushing
    } else {
        Tendency::OnTime
    };
    let mut kinds: BTreeMap<EventKind, Vec<f64>> = BTreeMap::new();
    for (k, v) in &timed {
        kinds.entry(*k).or_default().push(*v);
    }
    TimingSummary {
        n: o.len(),
        mean_ms: mean,
        median_ms: med,
        spread_ms: spread,
        tendency,
        by_kind: kinds
            .into_iter()
            .filter(|(_, v)| v.len() >= 2)
            .map(|(k, v)| (k, v.len(), median(&v)))
            .collect(),
    }
}

// -- the words --------------------------------------------------------------------------------------------------

fn list_bars(v: &[usize]) -> String {
    let mut v = v.to_vec();
    v.sort_unstable();
    v.dedup();
    let s: Vec<String> = v.iter().map(|b| b.to_string()).collect();
    match s.len() {
        0 => String::new(),
        1 => format!("bar {}", s[0]),
        2..=6 => format!("bars {} and {}", s[..s.len() - 1].join(", "), s[s.len() - 1]),
        n => format!("bars {} and {} more", s[..5].join(", "), n - 5),
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn names(neck: &Neck, v: &[Pos]) -> String {
    v.iter().map(|p| neck.describe(p)).collect::<Vec<_>>().join(" + ")
}

/// The few problems most worth working on, in words, most important first (at most five).
pub fn problems(r: &TakeReport) -> Vec<String> {
    let neck = &r.neck;
    let mut out: Vec<(f64, String)> = Vec::new();
    // bars by the chart's measure numbers (as the tab and the bar strip show them)
    let bar_no = |e: &EventResult| r.bars[e.bar].number.max(0) as usize;
    // wrong notes, the same mistake gathered
    let mut subs: BTreeMap<(Vec<(i32, i32)>, Vec<(i32, i32)>), Vec<&EventResult>> = BTreeMap::new();
    for e in r.events.iter().filter(|e| e.status == Status::Wrong) {
        let key = (
            e.expected.iter().map(|p| (p.string, p.fret)).collect(),
            e.played.iter().map(|p| (p.string, p.fret)).collect(),
        );
        subs.entry(key).or_default().push(e);
    }
    for v in subs.values() {
        let e = v[0];
        let bars: Vec<usize> = v.iter().map(|e| bar_no(e)).collect();
        let times = if v.len() > 1 {
            format!(" ({} times)", v.len())
        } else {
            String::new()
        };
        let at = format!("{} ({:.1} s)", capital(&list_bars(&bars)), e.t);
        let text = if let Some(note) = e.note.as_ref().filter(|n| n.starts_with("bend")) {
            format!("{at}: the bend fell short, {note}{times}.")
        } else if e.played.is_empty() {
            format!(
                "{at}: no clear note where the chart has {}{times} (muted or dead?).",
                names(neck, &e.expected)
            )
        } else if e.kind == EventKind::Note {
            format!(
                "{at}: you played {} where the chart has {}{times}.",
                names(neck, &e.played),
                names(neck, &e.expected)
            )
        } else {
            let what = if e.chord_name.trim().is_empty() {
                format!(
                    "the {}",
                    if e.kind == EventKind::Chord {
                        "chord"
                    } else {
                        "double stop"
                    }
                )
            } else {
                format!("the {} chord", e.chord_name.trim())
            };
            let extra: Vec<Pos> = e
                .played
                .iter()
                .filter(|p| !e.expected.iter().any(|q| q.midi == p.midi))
                .copied()
                .collect();
            let mut parts = Vec::new();
            // a string a fret off: "had X instead of Y"
            let mut missing = e.missing.clone();
            let mut extra = extra;
            let swaps: Vec<(Pos, Pos)> = missing
                .iter()
                .filter_map(|m| extra.iter().find(|x| x.string == m.string).map(|x| (*x, *m)))
                .collect();
            for (x, m) in &swaps {
                parts.push(format!(
                    "had {} instead of {}",
                    neck.describe(x),
                    neck.describe(m)
                ));
                missing.retain(|q| q != m);
                extra.retain(|q| q != x);
            }
            if missing.is_empty() && extra.is_empty() && swaps.is_empty() {
                parts.push(format!("didn't sound right ({})", names(neck, &e.played)));
            }
            if !missing.is_empty() {
                parts.push(format!("was missing {}", names(neck, &missing)));
            }
            if !extra.is_empty() {
                parts.push(format!("had {} in it", names(neck, &extra)));
            }
            format!("{at}: {what} {}{times}.", parts.join(" and "))
        };
        out.push((3.0 * v.len() as f64 + 0.5 - e.t * 1e-6, text)); // ties: earliest first
    }
    // bars with notes missed
    for b in &r.bars {
        let total = b.checked() + b.not_checked;
        if b.missed >= 2 && (b.missed as f64 >= 0.3 * total as f64) {
            out.push((
                2.0 * b.missed as f64,
                format!(
                    "Bar {} ({:.1} s): {} of {} notes missed.",
                    b.number,
                    b.t0.max(r.from),
                    b.missed,
                    total
                ),
            ));
        }
    }
    let lone: Vec<&EventResult> = r
        .events
        .iter()
        .filter(|e| e.status == Status::Missed)
        .filter(|e| r.bars[e.bar].missed < 2)
        .collect();
    if !lone.is_empty() {
        let bars: Vec<usize> = lone.iter().map(|e| bar_no(e)).collect();
        let first = lone[0];
        out.push((
            1.5 * lone.len() as f64,
            if lone.len() == 1 {
                format!(
                    "{} ({:.1} s): you missed {}.",
                    capital(&list_bars(&bars)),
                    first.t,
                    names(neck, &first.expected)
                )
            } else {
                format!(
                    "{} notes missed here and there ({}).",
                    lone.len(),
                    list_bars(&bars)
                )
            },
        ));
    }
    // timing
    let tm = &r.timing;
    if tm.n >= 3 {
        // the kind of event most off from the others (a muted strum's timing is too rough to compare)
        let by: Vec<&(EventKind, usize, f64)> =
            tm.by_kind.iter().filter(|k| k.0 != EventKind::Muted).collect();
        let mut kind_said = false;
        let mut worst: Option<(f64, &(EventKind, usize, f64), f64)> = None;
        if by.len() >= 2 {
            for k in &by {
                let others: Vec<f64> = by.iter().filter(|o| o.0 != k.0).map(|o| o.2).collect();
                let rest = others.iter().sum::<f64>() / others.len() as f64;
                let diff = (k.2 - rest).abs();
                if k.2.abs() >= 25.0 && diff >= 20.0 && worst.is_none_or(|w| diff > w.0) {
                    worst = Some((diff, k, rest));
                }
            }
        }
        if let Some((diff, k, rest)) = worst {
            let way = if k.2 > 0.0 { "late" } else { "early" };
            out.push((
                diff / 8.0,
                format!(
                    "You're about {:.0} ms {way} on the {} ({} on the rest).",
                    k.2.abs(),
                    k.0.plural(),
                    if rest.abs() < ON_TIME_MS {
                        "on time".to_string()
                    } else {
                        format!("{rest:+.0} ms")
                    }
                ),
            ));
            kind_said = true;
        }
        if !kind_said && tm.tendency != Tendency::OnTime && tm.tendency != Tendency::Unknown {
            let way = if tm.median_ms > 0.0 {
                "late (dragging)"
            } else {
                "early (rushing)"
            };
            out.push((
                tm.median_ms.abs() / 8.0,
                format!("You're about {:.0} ms {way} overall.", tm.median_ms.abs()),
            ));
        }
        if tm.spread_ms > 45.0 {
            out.push((
                (tm.spread_ms - 30.0) / 6.0,
                format!(
                    "Your timing wanders: notes land within about ±{:.0} ms of the beat.",
                    tm.spread_ms
                ),
            ));
        }
    }
    // extras
    if r.extras.len() >= 2 || (r.extras.len() == 1 && r.events.len() < 30) {
        let bars: Vec<usize> = r
            .extras
            .iter()
            .filter_map(|e| e.bar.map(|b| r.bars[b].number.max(0) as usize))
            .collect();
        let mut count: BTreeMap<usize, usize> = BTreeMap::new();
        for b in &bars {
            *count.entry(*b).or_default() += 1;
        }
        let mut top: Vec<(usize, usize)> = count.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let where_ = top.iter().take(3).map(|t| t.0).collect::<Vec<_>>();
        let e = &r.extras[0];
        out.push((
            1.2 * r.extras.len() as f64,
            format!(
                "{} extra note{}{} (e.g. {} at {:.1} s).",
                r.extras.len(),
                if r.extras.len() == 1 { "" } else { "s" },
                if where_.is_empty() {
                    String::new()
                } else {
                    format!(", mostly in {}", list_bars(&where_))
                },
                names(neck, &e.played),
                e.t
            ),
        ));
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut v: Vec<String> = out.into_iter().take(5).map(|o| o.1).collect();
    if v.is_empty() && !r.events.is_empty() {
        v.push(if r.accuracy() >= 0.999 {
            "Clean take: every note right and in time.".into()
        } else {
            "Nothing stands out: a few notes here and there.".into()
        });
    }
    v
}

// -- tabs as text ----------------------------------------------------------------------------------------------

impl TakeReport {
    /// The chart's tab and yours under it, bar by bar, wrapped at `width` columns. Under your tab: `.` right,
    /// `X` wrong, `o` missed, `?` not checked, `+` an extra note.
    pub fn ascii_tabs(&self, width: usize) -> String {
        struct Col {
            bar: usize,
            chart: HashMap<i32, String>,
            yours: HashMap<i32, String>,
            mark: char,
        }
        let mut cols: Vec<(f64, Col)> = Vec::new();
        for e in &self.events {
            let mut chart = HashMap::new();
            for p in &e.expected {
                chart.insert(p.string, p.fret.to_string());
            }
            if e.kind == EventKind::Muted {
                chart.insert(e.expected.first().map_or(0, |p| p.string), "x".into());
            }
            let mut yours = HashMap::new();
            match e.status {
                Status::Hit | Status::Wrong => {
                    for p in &e.played {
                        yours.insert(p.string, p.fret.to_string());
                    }
                    if e.played.is_empty() {
                        yours.insert(e.expected.first().map_or(0, |p| p.string), "x".into());
                    }
                }
                _ => {}
            }
            let mark = match e.status {
                Status::Hit => '.',
                Status::Wrong => 'X',
                Status::Missed => 'o',
                Status::NotChecked => '?',
            };
            cols.push((
                e.t,
                Col {
                    bar: e.bar,
                    chart,
                    yours,
                    mark,
                },
            ));
        }
        for x in &self.extras {
            let mut yours = HashMap::new();
            for p in &x.played {
                yours.insert(p.string, p.fret.to_string());
            }
            let bar = x.bar.unwrap_or_else(|| {
                if x.t < self.from {
                    0
                } else {
                    self.bars.len().saturating_sub(1)
                }
            });
            cols.push((
                x.t,
                Col {
                    bar,
                    chart: HashMap::new(),
                    yours,
                    mark: '+',
                },
            ));
        }
        cols.sort_by(|a, b| a.0.total_cmp(&b.0));
        let strings = self.neck.opens.len() as i32;
        let lab_w = 2;
        // build each bar as text pieces per row (strings high to low for chart, yours, then the marks)
        let rows = (strings * 2 + 1) as usize;
        let mut bars_txt: Vec<Vec<String>> = Vec::new();
        let mut current: Option<usize> = None;
        for (_, c) in &cols {
            if current != Some(c.bar) {
                current = Some(c.bar);
                bars_txt.push(vec![String::new(); rows]);
            }
            let w = c
                .chart
                .values()
                .chain(c.yours.values())
                .map(|s| s.len())
                .max()
                .unwrap_or(1);
            let cur = bars_txt.last_mut().unwrap();
            for s in 0..strings {
                let row = (strings - 1 - s) as usize;
                let cell = |m: &HashMap<i32, String>| m.get(&s).cloned().unwrap_or_default();
                let (a, b) = (cell(&c.chart), cell(&c.yours));
                cur[row].push_str(&format!("-{}{}", a, "-".repeat(w - a.len())));
                cur[strings as usize + row].push_str(&format!("-{}{}", b, "-".repeat(w - b.len())));
            }
            cur[rows - 1].push_str(&format!(" {}{}", c.mark, " ".repeat(w - 1)));
        }
        let mut out = String::new();
        let mut line: Vec<String> = vec![String::new(); rows];
        let names: Vec<String> = (0..strings)
            .rev()
            .map(|s| {
                let l = self.neck.label(s);
                let short = l.trim_start_matches("low ").trim_start_matches("high ");
                let short = if l.starts_with("high") {
                    short.to_lowercase()
                } else {
                    short.to_string()
                };
                format!("{short:>lab_w$}")
            })
            .collect();
        let flush = |line: &mut Vec<String>, out: &mut String| {
            if line[0].is_empty() {
                return;
            }
            out.push_str("chart\n");
            for (k, n) in names.iter().enumerate() {
                out.push_str(&format!("{n}|{}|\n", line[k]));
            }
            out.push_str("you\n");
            for (k, n) in names.iter().enumerate() {
                out.push_str(&format!("{n}|{}|\n", line[strings as usize + k]));
            }
            out.push_str(&format!("{:lab_w$} {}\n\n", "", line[rows - 1]));
            for l in line.iter_mut() {
                l.clear();
            }
        };
        for b in bars_txt {
            let blen = b[0].len() + 1;
            if !line[0].is_empty() && line[0].len() + blen + lab_w + 2 > width {
                flush(&mut line, &mut out);
            }
            for (k, piece) in b.iter().enumerate() {
                if !line[k].is_empty() {
                    line[k].push(if k == rows - 1 { ' ' } else { '|' });
                }
                line[k].push_str(piece);
            }
        }
        flush(&mut line, &mut out);
        out
    }
}

// -- JSON for the teacher ---------------------------------------------------------------------------------------

fn pos_json(neck: &Neck, p: &Pos) -> Value {
    json!({
        "string": p.string,
        "string_name": neck.label(p.string),
        "fret": p.fret,
        "note": p.name(),
    })
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

impl TakeReport {
    /// Everything, self-explanatory (the teacher reads these files later).
    pub fn to_json(&self) -> Value {
        let n = &self.neck;
        json!({
            "about": "A take: the guitar recorded over a stretch of the chart and checked note by note against it \
                      (quickRS, no AI: onsets + pitch analysis). Times are chart seconds; offsets are wall-clock ms, \
                      + = late; strings are numbered 0 = the lowest-pitched (low E), with names given. status: hit = \
                      right pitches, wrong = a pluck with other pitches ('played' = what was heard there: your tab), \
                      missed = nothing played, not_checked = couldn't tell (legato or pinch harmonic not clearly \
                      heard; not counted). accuracy = hit / (hit + wrong + missed) ('checked' = that sum; 0 = \
                      nothing here could be checked). 'bar' is 1-based within the stretch; 'measure' is the \
                      chart's measure number, which the problems use.",
            "song": {"artist": self.artist, "title": self.title, "part": self.part,
                     "tuning": n.labels.clone()},
            "stretch": {"from": r3(self.from), "to": r3(self.to)},
            "speed": self.speed,
            "take": {"seconds": r3(self.seconds), "rate": self.rate, "peak": r3(self.peak as f64),
                     "clipped": self.peak >= 0.999, "plucks_found": self.plucks_found,
                     "overall_offset_ms": r1(self.shift_ms)},
            "summary": {
                "accuracy": r3(self.accuracy()),
                "checked": self.checked(),
                "on_time_share": r3(self.on_time_share()),
                "events": self.events.len(),
                "hit": self.count(Status::Hit),
                "wrong": self.count(Status::Wrong),
                "missed": self.count(Status::Missed),
                "not_checked": self.count(Status::NotChecked),
                "extras": self.extras.len(),
                "timing": {
                    "timed_notes": self.timing.n,
                    "median_ms": r1(self.timing.median_ms),
                    "mean_ms": r1(self.timing.mean_ms),
                    "spread_ms": r1(self.timing.spread_ms),
                    "tendency": self.timing.tendency.key(),
                    "by_kind": self.timing.by_kind.iter().map(|(k, c, m)| json!({"kind": k.key(), "notes": c, "median_ms": r1(*m)})).collect::<Vec<_>>(),
                },
            },
            "problems": self.problems,
            "bars": self.bars.iter().map(|b| json!({
                "bar": b.index, "measure": b.number, "from": r3(b.t0), "to": r3(b.t1),
                "hit": b.hits, "wrong": b.wrong, "missed": b.missed, "not_checked": b.not_checked,
                "extras": b.extras,
                "accuracy": b.accuracy().map(r3),
                "mean_offset_ms": b.mean_offset_ms.map(r1),
            })).collect::<Vec<_>>(),
            "events": self.events.iter().map(|e| {
                let mut v = json!({
                    "t": r3(e.t),
                    "bar": self.bars[e.bar].index,
                    "measure": self.bars[e.bar].number,
                    "kind": e.kind.key(),
                    "expected": e.expected.iter().map(|p| pos_json(n, p)).collect::<Vec<_>>(),
                    "status": e.status.key(),
                    "played": e.played.iter().map(|p| pos_json(n, p)).collect::<Vec<_>>(),
                    "offset_ms": e.offset_ms.map(r1),
                });
                if !e.chord_name.trim().is_empty() {
                    v["chord"] = json!(e.chord_name.trim());
                }
                if !e.techniques.is_empty() {
                    v["techniques"] = json!(e.techniques.names());
                }
                if !e.missing.is_empty() {
                    v["missing"] = json!(e.missing.iter().map(|p| pos_json(n, p)).collect::<Vec<_>>());
                }
                if let Some(note) = &e.note {
                    v["note"] = json!(note);
                }
                v
            }).collect::<Vec<_>>(),
            "extras": self.extras.iter().map(|x| json!({
                "t": r3(x.t),
                "bar": x.bar.map(|b| self.bars[b].index),
                "measure": x.bar.map(|b| self.bars[b].number),
                "played": x.played.iter().map(|p| pos_json(n, p)).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "tab": self.ascii_tabs(110),
            "analysis_ms": r1(self.analysis_ms),
        })
    }

    /// The report as text: the summary, the problems, bar by bar, both tabs.
    pub fn text(&self, width: usize) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "{} - {} ({}), {:.2}-{:.2} s at {:.0}%: {} notes\n",
            self.artist,
            self.title,
            self.part,
            self.from,
            self.to,
            self.speed * 100.0,
            self.events.len()
        ));
        s.push_str(&format!(
            "accuracy {:.0}%: {} right, {} wrong, {} missed, {} not checked; {} extra; timing {} (median {:+.0} ms, spread {:.0} ms)\n",
            self.accuracy() * 100.0,
            self.count(Status::Hit),
            self.count(Status::Wrong),
            self.count(Status::Missed),
            self.count(Status::NotChecked),
            self.extras.len(),
            self.timing.tendency.key(),
            self.timing.median_ms,
            self.timing.spread_ms
        ));
        s.push('\n');
        for p in &self.problems {
            s.push_str(&format!("  * {p}\n"));
        }
        s.push_str("\nbar  measure   right  wrong missed  extra  timing\n");
        for b in &self.bars {
            s.push_str(&format!(
                "{:>3}  {:>7}   {:>2}/{:<2}  {:>5} {:>6} {:>6}  {}\n",
                b.index,
                b.number,
                b.hits,
                b.checked(),
                b.wrong,
                b.missed,
                b.extras,
                b.mean_offset_ms
                    .map(|m| format!("{m:+.0} ms"))
                    .unwrap_or_default()
            ));
        }
        s.push('\n');
        s.push_str(&self.ascii_tabs(width));
        s.push_str(&format!(
            "({} plucks found; analysed in {:.0} ms)\n",
            self.plucks_found, self.analysis_ms
        ));
        s
    }
}

// -- saved takes ------------------------------------------------------------------------------------------------

/// Takes kept per stretch (the oldest go first).
pub const KEEP_PER_STRETCH: usize = 10;

fn file_safe(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    t.trim().trim_end_matches('.').to_string()
}

/// "20261015-213005" (UTC) for a file name, from the system clock.
pub fn stamp_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil from days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// `<takes>/<artist - title>/` and the file stem `<part>-<from>-<to>-<stamp>` of a take (times to 0.1 s).
/// `takes_root` is the takes folder (`<data>/takes` unless config.json's `takes_dir` says otherwise, P23.9).
pub fn take_place(
    takes_root: &std::path::Path,
    artist: &str,
    title: &str,
    part: &str,
    from: f64,
    to: f64,
    stamp: &str,
) -> (std::path::PathBuf, String) {
    let dir = takes_root.join(file_safe(&format!("{artist} - {title}")));
    (dir, file_safe(&format!("{part}-{from:.1}-{to:.1}-{stamp}")))
}

/// A mono 16-bit PCM WAV.
pub fn write_wav_mono(path: &std::path::Path, rate: u32, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &v in samples {
        out.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out)
}

/// Save a take: `<stem>.wav` (the dry guitar, mono 16-bit) and `<stem>.json` (the report's JSON, or only the
/// take's details when there's no report, plus "timing": the take-to-chart mapping and "wav": the file name)
/// under `<takes_root>/<artist - title>/`, keeping the last `KEEP_PER_STRETCH` takes of the same part and stretch.
/// `stamp` names it (e.g. `stamp_now()`, or the local time "%Y%m%d-%H%M%S"). Returns the JSON's path.
pub fn save_take(
    takes_root: &std::path::Path,
    input: &TakeInput,
    report: Option<&TakeReport>,
    stamp: &str,
) -> std::io::Result<std::path::PathBuf> {
    let a = &input.arr;
    let (dir, stem) = take_place(
        takes_root, &a.artist, &a.title, &a.name, input.from, input.to, stamp,
    );
    std::fs::create_dir_all(&dir)?;
    let wav = dir.join(format!("{stem}.wav"));
    write_wav_mono(&wav, input.timing.rate, &input.samples)?;
    let mut v = match report {
        Some(r) => r.to_json(),
        None => json!({
            "about": "A take (the guitar recorded over a stretch of the chart); not analysed.",
            "song": {"artist": a.artist, "title": a.title, "part": a.name},
            "stretch": {"from": r3(input.from), "to": r3(input.to)},
            "speed": input.timing.speed,
        }),
    };
    v["timing"] = input.timing.to_json();
    v["wav"] = json!(format!("{stem}.wav"));
    v["recorded"] = json!(stamp);
    let path = dir.join(format!("{stem}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default())?;
    // keep the last few of this stretch (the stamps sort in time order)
    let prefix = stem[..stem.len() - stamp.len()].to_string();
    let mut same: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.starts_with(&prefix) && n.ends_with(".json"))
        .collect();
    same.sort();
    if same.len() > KEEP_PER_STRETCH {
        for old in &same[..same.len() - KEEP_PER_STRETCH] {
            let _ = std::fs::remove_file(dir.join(old));
            let _ = std::fs::remove_file(dir.join(old.replace(".json", ".wav")));
        }
    }
    Ok(path)
}

/// The takes saved for a song (their JSON files), newest first.
pub fn saved_takes(takes_root: &std::path::Path, artist: &str, title: &str) -> Vec<std::path::PathBuf> {
    let (dir, _) = take_place(takes_root, artist, title, "", 0.0, 0.0, "");
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|r| {
            r.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|p| {
        std::cmp::Reverse(
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.get(s.len().saturating_sub(15)..))
                .unwrap_or("")
                .to_string(),
        )
    });
    v
}

// -- in the background ------------------------------------------------------------------------------------------

/// A take being analysed on its own thread.
pub struct TakeJob {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    rx: std::sync::mpsc::Receiver<Option<TakeReport>>,
    result: Option<Option<TakeReport>>,
}

impl TakeJob {
    pub fn start(input: TakeInput) -> TakeJob {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let (p, c) = (progress.clone(), cancel.clone());
        let spawned = std::thread::Builder::new()
            .name("take-analysis".into())
            .spawn(move || {
                // a panic in the analysis ends the job with no report (the app says it couldn't analyse it)
                let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    analyse(&input, &|f| p.store(f.to_bits(), Ordering::Relaxed), &c)
                }))
                .unwrap_or(None);
                let _ = tx.send(report);
            });
        if spawned.is_err() {
            // no thread: report nothing (the caller shows it failed)
            let (tx, rx2) = std::sync::mpsc::channel();
            let _ = tx.send(None);
            return TakeJob {
                progress,
                cancel,
                rx: rx2,
                result: None,
            };
        }
        TakeJob {
            progress,
            cancel,
            rx,
            result: None,
        }
    }

    /// 0..1
    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }

    /// Whether the analysis has finished (successfully or not).
    pub fn finished(&mut self) -> bool {
        if self.result.is_none() {
            match self.rx.try_recv() {
                Ok(r) => self.result = Some(r),
                // the thread is gone without a word: finished, with nothing
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.result = Some(None),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.result.is_some()
    }

    /// Finished without a report (failed or cancelled): the app says it couldn't analyse the take.
    pub fn failed(&mut self) -> bool {
        self.finished() && matches!(self.result, Some(None))
    }

    /// The report, once (None while running, after a failure or cancel, or once taken).
    pub fn take(&mut self) -> Option<TakeReport> {
        self.finished();
        self.result.as_mut().and_then(Option::take)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for TakeJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
