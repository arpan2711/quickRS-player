//! A guitar synth for hearing one passage of the tab on its own. Port of rsplay/guitarsynth.py.
//!
//! Each note is a Karplus-Strong pluck (a short burst fed back through a delay line one string-period long, which
//! rings and darkens like a plucked string). Bends, slides and vibrato read the pluck back at a varying speed. The
//! passage goes through the guitar tone (clean when the tone is "as is", and for bass) so it sounds like you.
//!
//! Changes from the Python version (docs/BUGS.md, "Selection synth sound"; FEEDPAK.md "Bend units"):
//! - chart bends are in half steps (RS and feedpak alike), so they're no longer doubled;
//! - looping folds the whole ring-out back onto the start with a fade, so there's no click at the loop point;
//! - a render that has been superseded stops early (`cancel`);
//! - the engine cross-fades a replaced clip (no pop when restarting).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use parking_lot::Mutex;
use qrs_formats::model::Techs;

use crate::rt::Clip;
use crate::rt::ToneProcessor;
use crate::tone::{Preset, ToneChain, ToneParams};

/// Seconds between strings when a chord is strummed (low string first).
pub const STRUM: f64 = 0.008;
/// Seconds of ring-out after the last note (folded back onto the start when looping).
pub const TAIL: f64 = 1.0;
pub const MAX_SECONDS: f64 = 60.0;
/// Harmonic nodes: fret → semitones above the open string.
const HARMONIC_NODES: [(i32, f64); 10] = [
    (12, 12.0),
    (7, 19.0),
    (19, 19.0),
    (5, 24.0),
    (24, 24.0),
    (4, 28.0),
    (9, 28.0),
    (16, 28.0),
    (3, 31.0),
    (15, 31.0),
];

/// A note as the tab draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthNote {
    pub t: f64,
    pub string: i32,
    pub fret: i32,
    pub sus: f64,
    pub tech: Techs,
    pub slide_to: i32,
    /// Half steps.
    pub bend: f64,
    pub in_chord: bool,
}

/// The notes of an arrangement in time order, chord notes flagged.
pub fn notes_of(arr: &qrs_formats::Arrangement) -> Vec<SynthNote> {
    // an unpitched slide still goes somewhere (it fades as it goes)
    let slide = |n: &qrs_formats::Note| {
        if n.slide_to >= 0 {
            n.slide_to
        } else {
            n.slide_unpitch_to
        }
    };
    let mut v: Vec<SynthNote> = Vec::new();
    for n in &arr.notes {
        v.push(SynthNote {
            t: n.time,
            string: n.string,
            fret: n.fret,
            sus: n.sustain,
            tech: n.techniques,
            slide_to: slide(n),
            bend: n.bend,
            in_chord: false,
        });
    }
    for c in &arr.chords {
        for n in &c.notes {
            v.push(SynthNote {
                t: n.time,
                string: n.string,
                fret: n.fret,
                sus: n.sustain.max(c.sustain),
                tech: n.techniques | c.techniques,
                slide_to: slide(n),
                bend: n.bend,
                in_chord: true,
            });
        }
    }
    v.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.string.cmp(&b.string)));
    v
}

pub fn midi_hz(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

/// A small fast RNG (xorshift64*).
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Karplus-Strong: n samples of a string whose loop is `period` samples, ringing down 60 dB in t60 s. `bright`
/// (0..1) is how much of the pick's high end the burst keeps.
pub fn pluck(period: usize, n: usize, t60: f64, bright: f64, rng: &mut Rng, rate: u32) -> Vec<f64> {
    let period = period.max(2);
    let f0 = rate as f64 / (period as f64 + 0.5);
    // the pick burst from its harmonics: falling off like a plucked string, rolled off above a cutoff set by
    // `bright`, with a dip where picking near the bridge cancels them; random phases (an inverse DFT)
    let half = period / 2;
    let mut exc = vec![0.0f64; period];
    for k in 1..=half {
        let kf = k as f64;
        let mut amp = kf.powf(-0.9) / (1.0 + (kf * f0 / (900.0 + 4500.0 * bright)).powi(2));
        amp *= 0.4 + 0.6 * (std::f64::consts::PI * kf * 0.14).sin().abs();
        let ph = std::f64::consts::TAU * rng.uniform();
        for (i, e) in exc.iter_mut().enumerate() {
            *e += amp * (std::f64::consts::TAU * kf * i as f64 / period as f64 + ph).cos();
        }
    }
    let peak = exc.iter().fold(1e-9f64, |m, v| m.max(v.abs()));
    for e in &mut exc {
        *e /= peak;
    }
    let rho = if t60 > 0.0 {
        10f64.powf(-3.0 / (t60 * f0).max(0.02))
    } else {
        0.0
    };
    let mut y = vec![0.0f64; n + period + 1];
    y[1..=period].copy_from_slice(&exc);
    for i in period + 1..n + 1 {
        y[i] = rho * 0.5 * (y[i - period] + y[i - period - 1]);
    }
    y[1..n + 1].to_vec()
}

/// One plucked note, `seconds` long, following the pitch curve `semis` (semitones, one per sample).
pub fn string_tone(
    f_hz: f64,
    seconds: f64,
    rate: u32,
    semis: Option<&[f64]>,
    t60: f64,
    bright: f64,
    rng: &mut Rng,
) -> Vec<f64> {
    let n = ((seconds * rate as f64) as usize).max(1);
    let period = ((rate as f64 / f_hz - 0.5) as usize).max(2);
    let ratio = f_hz * (period as f64 + 0.5) / rate as f64;
    let mut pos = Vec::with_capacity(n);
    match semis {
        None => pos.extend((0..n).map(|i| i as f64 * ratio)),
        Some(s) => {
            let mut acc = 0.0;
            for i in 0..n {
                pos.push(acc);
                acc += ratio * 2f64.powf(s.get(i).copied().unwrap_or(0.0) / 12.0);
            }
        }
    }
    let src = pluck(period, *pos.last().unwrap() as usize + 3, t60, bright, rng, rate);
    pos.iter()
        .map(|&p| {
            let i = p.floor() as usize;
            let f = p - i as f64;
            let a = src.get(i).copied().unwrap_or(0.0);
            let b = src.get(i + 1).copied().unwrap_or(0.0);
            a + (b - a) * f
        })
        .collect()
}

fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Semitone offsets over the note for bends, slides and vibrato (None when the pitch stays put).
pub fn pitch_curve(g: &SynthNote, dur: f64, rate: u32) -> Option<Vec<f64>> {
    let bend = if g.tech.has(Techs::BEND) && g.bend > 0.0 {
        g.bend
    } else {
        0.0
    };
    let slide = if g.slide_to >= 0 && g.slide_to != g.fret {
        (g.slide_to - g.fret) as f64
    } else {
        0.0
    };
    let vib = g.tech.has(Techs::VIBRATO);
    if bend == 0.0 && slide == 0.0 && !vib {
        return None;
    }
    let n = ((dur * rate as f64) as usize).max(1);
    let rise = (0.35 * dur).clamp(0.06, 0.2);
    let (a, b) = (0.25 * dur, 0.75 * dur); // a slide arrives with time left to hear the target fret
    Some(
        (0..n)
            .map(|i| {
                let t = i as f64 / rate as f64;
                let mut v = 0.0;
                if bend > 0.0 {
                    v += bend * smooth((t - 0.02) / rise);
                }
                if slide != 0.0 {
                    let prog = ((t - a) / (b - a).max(1e-3)).clamp(0.0, 1.0);
                    v += (prog * slide).round_ties_even(); // fret by fret
                }
                if vib {
                    v += 0.35 * (std::f64::consts::TAU * 5.5 * t).sin() * smooth((t - 0.12) / 0.2);
                }
                v
            })
            .collect(),
    )
}

fn note_pitch(g: &SynthNote, opens: &[i32]) -> f64 {
    let open = opens.get(g.string.max(0) as usize).copied().unwrap_or(40) as f64;
    if g.tech.has(Techs::HARMONIC) {
        return open
            + HARMONIC_NODES
                .iter()
                .find(|(f, _)| *f == g.fret)
                .map(|x| x.1)
                .unwrap_or(12.0);
    }
    open + g.fret as f64
}

pub struct RenderOptions<'a> {
    pub rate: u32,
    /// Fraction of the chart's tempo (pitch unchanged).
    pub speed: f64,
    pub bass: bool,
    /// The tone to play through (None: dry).
    pub tone: Option<ToneParams>,
    /// The clip is exactly the selection long and its ring-out wraps round onto the start.
    pub looped: bool,
    pub seed: u64,
    /// Where a note is drawn (the tab snaps notes onto the beat): a note is in the selection when its drawn time
    /// is, so what you hear matches what's highlighted.
    pub place: &'a dyn Fn(f64) -> f64,
    /// Set when the render is no longer wanted.
    pub cancel: &'a AtomicBool,
}

/// The notes in [start, end) as interleaved stereo f32 (None if cancelled).
pub fn render(
    notes: &[SynthNote],
    opens: &[i32],
    start: f64,
    end: f64,
    o: &RenderOptions,
) -> Option<Vec<f32>> {
    let rate = o.rate;
    let rf = rate as f64;
    let end = end.min(start + MAX_SECONDS);
    let mut rng = Rng::new(o.seed);
    let sel: Vec<&SynthNote> = notes
        .iter()
        .filter(|g| start <= (o.place)(g.t) && (o.place)(g.t) < end)
        .collect();
    let length = ((end - start) / o.speed * rf) as usize;
    let mut buf = vec![0.0f64; length + (TAIL * rf) as usize + rate as usize];
    let ok = pluck_notes(
        &sel,
        opens,
        start,
        o.speed,
        o.bass,
        rate,
        &mut rng,
        o.cancel,
        None,
        |k, v| {
            if let Some(slot) = buf.get_mut(k) {
                *slot += v;
            }
        },
    );
    if !ok {
        return None;
    }
    let peak = buf.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        for v in &mut buf {
            *v *= 0.35 / peak; // about guitar-input level, which the tones are voiced for
        }
    }
    let mut mono: Vec<f32> = buf.iter().map(|&v| v as f32).collect();
    if let Some(p) = o.tone {
        let mut chain = ToneChain::new(rate, p);
        for c in mono.chunks_mut(512) {
            chain.process(c);
        }
    }
    let total = length + (TAIL * rf) as usize;
    let mut out: Vec<f32> = if o.looped && length > 0 {
        // the clip is the selection; everything that rings past the end wraps round onto the start, faded out
        // over the wrapped part so the loop point is seamless
        let mut clip = mono[..length.min(mono.len())].to_vec();
        clip.resize(length, 0.0);
        let tail = &mono[length.min(mono.len())..total.min(mono.len())];
        let fold = tail.len();
        for (k, v) in tail.iter().enumerate() {
            let w = if fold > 1 {
                1.0 - k as f32 / fold as f32
            } else {
                1.0
            };
            clip[k % length] += v * w;
        }
        clip
    } else {
        let mut v = mono[..total.min(mono.len())].to_vec();
        let fade = v.len().min((0.3 * rf) as usize);
        let n = v.len();
        for k in 0..fade {
            v[n - fade + k] *= 1.0 - k as f32 / fade.max(1) as f32;
        }
        v
    };
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        for v in &mut out {
            *v *= 0.8 / peak;
        }
    }
    Some(out.iter().flat_map(|&v| [v, v]).collect())
}

/// Plucks each of `sel` (in time order) into `add(sample, value)`: a note at chart time t lands at
/// (t - start) / speed seconds (a chord's notes strummed). `progress` (if any) counts up to 1000 as it goes. False
/// if cancelled.
#[allow(clippy::too_many_arguments)]
fn pluck_notes(
    sel: &[&SynthNote],
    opens: &[i32],
    start: f64,
    speed: f64,
    bass: bool,
    rate: u32,
    rng: &mut Rng,
    cancel: &AtomicBool,
    progress: Option<&AtomicU32>,
    mut add: impl FnMut(usize, f64),
) -> bool {
    let rf = rate as f64;
    let mut onsets: Vec<f64> = sel.iter().map(|g| g.t).collect();
    onsets.dedup_by(|a, b| a == b);
    // chord strums: notes sharing a time are spread low string → high string
    let mut order = vec![0usize; sel.len()];
    for i in 1..sel.len() {
        if sel[i - 1].t == sel[i].t {
            order[i] = order[i - 1] + 1;
        }
    }
    for (i, g) in sel.iter().enumerate() {
        if let Some(p) = progress {
            p.store((i * 1000 / sel.len().max(1)) as u32, Ordering::Relaxed);
        }
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let tech = g.tech;
        if tech.has(Techs::IGNORE) {
            continue;
        }
        let nxt_same = sel[i + 1..]
            .iter()
            .find(|h| h.string == g.string && h.t > g.t)
            .map(|h| h.t);
        let j = onsets.partition_point(|&t| t <= g.t);
        let nxt_any = onsets.get(j).copied();
        // how long it rings (chart seconds): its sustain, else until the next notes come in (chords and sustains
        // ring on a little), never past the next note on the same string
        let mut dur = if g.sus > 0.05 {
            g.sus + 0.12
        } else if let Some(n) = nxt_any {
            ((n - g.t) + if g.in_chord { 0.5 } else { 0.15 }).min(1.6)
        } else {
            1.6
        };
        if let Some(n) = nxt_same {
            dur = dur.min(n - g.t + 0.01);
        }
        if tech.has(Techs::PALM_MUTE) {
            dur = dur.min(0.22);
        }
        let dur_s = (dur / speed).max(0.03);
        let f = midi_hz(note_pitch(g, opens));
        let legato = tech.has(Techs::HAMMER_ON) || tech.has(Techs::PULL_OFF) || tech.has(Techs::TAP);
        let (mut t60, mut bright, mut gain) = if bass {
            if tech.has(Techs::SLAP) || tech.has(Techs::POP) {
                (3.5, 0.85, 1.2)
            } else {
                (3.5, 0.35, 1.0)
            }
        } else {
            (
                if f < 200.0 {
                    4.0
                } else if f < 500.0 {
                    2.6
                } else {
                    1.6
                },
                0.6,
                1.0,
            )
        };
        if g.in_chord {
            gain *= 0.7;
        }
        if legato {
            bright *= 0.5;
            gain *= 0.6;
        }
        if tech.has(Techs::PALM_MUTE) {
            t60 = 0.22;
            bright = 0.2;
        }
        if tech.has(Techs::ACCENT) {
            gain *= 1.3;
        }
        let at = ((g.t - start) / speed).max(0.0) + order[i] as f64 * STRUM;
        let mut tone: Vec<f64> = if tech.has(Techs::FRET_HAND_MUTE) {
            // a dead "chk": a fast-dying, dark burst
            string_tone(f, 0.05, rate, None, 0.04, 0.25, rng)
                .iter()
                .map(|v| v * 0.6 * gain)
                .collect()
        } else if tech.has(Techs::TREMOLO) {
            let hits = ((dur_s / 0.075) as usize).max(1);
            (0..hits)
                .flat_map(|_| string_tone(f, 0.075, rate, None, t60, bright, rng))
                .map(|v| v * gain)
                .collect()
        } else {
            let semis = pitch_curve(g, dur_s, rate);
            let mut t = if tech.has(Techs::HARMONIC) {
                string_tone(f, dur_s, rate, semis.as_deref(), t60 * 1.3, 0.9, rng)
                    .iter()
                    .map(|v| v * 0.7 * gain)
                    .collect::<Vec<_>>()
            } else {
                let mut t: Vec<f64> = string_tone(f, dur_s, rate, semis.as_deref(), t60, bright, rng)
                    .iter()
                    .map(|v| v * gain)
                    .collect();
                if tech.has(Techs::PINCH_HARMONIC) {
                    // the squeal rides on top of the fretted note
                    let sq = string_tone(
                        f * 2f64.powf(19.0 / 12.0),
                        dur_s,
                        rate,
                        semis.as_deref(),
                        t60,
                        0.95,
                        rng,
                    );
                    for (a, b) in t.iter_mut().zip(sq) {
                        *a += b * 0.6 * gain;
                    }
                }
                t
            };
            if tech.has(Techs::UNPITCHED_SLIDE) {
                let n = t.len().max(1);
                for (k, v) in t.iter_mut().enumerate() {
                    *v *= (1.0 - k as f64 / n as f64).sqrt();
                }
            }
            t
        };
        // a short release so cut-off notes don't click
        let rel = tone.len().min((0.03 * rf) as usize);
        if rel > 1 {
            let n = tone.len();
            for k in 0..rel {
                tone[n - rel + k] *= 1.0 - k as f64 / (rel - 1) as f64;
            }
        }
        let a = (at * rf) as usize;
        for (k, v) in tone.iter().enumerate() {
            add(a + k, *v);
        }
    }
    true
}

/// The tone the synth plays through: your monitoring tone, or Clean when that is "as is" (and for bass).
pub fn synth_tone(spec: &crate::tone::ToneSpec, bass: bool) -> ToneParams {
    match spec.params() {
        Some(p) if !bass => p,
        _ => Preset::Clean.params(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynthState {
    Idle,
    Rendering,
    Playing,
}

/// Plays a tab selection through the synth (or from the song's own track), rendering in the background; the
/// newest request wins and older renders are cancelled.
pub struct SynthPlayer {
    pub state: SynthState,
    pub looped: bool,
    pub a: f64,
    pub b: f64,
    pub speed: f64,
    pub error: String,
    generation: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    ready: Arc<Mutex<Option<(u64, Result<Clip, String>)>>>,
    /// The engine's id for the clip playing.
    clip_id: u64,
}

impl Default for SynthPlayer {
    fn default() -> Self {
        SynthPlayer {
            state: SynthState::Idle,
            looped: false,
            a: 0.0,
            b: 0.0,
            speed: 1.0,
            error: String::new(),
            generation: Arc::new(AtomicU64::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            ready: Arc::new(Mutex::new(None)),
            clip_id: 0,
        }
    }
}

impl SynthPlayer {
    fn begin(&mut self, a: f64, b: f64, speed: f64, looped: bool) -> (u64, Arc<AtomicBool>) {
        self.cancel.store(true, Ordering::Release); // stop the previous render
        self.cancel = Arc::new(AtomicBool::new(false));
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.a = a;
        self.b = b.min(a + MAX_SECONDS);
        self.speed = speed;
        self.looped = looped;
        self.state = SynthState::Rendering;
        self.error.clear();
        (generation, self.cancel.clone())
    }

    /// Render the notes of [a, b) at `speed` through `tone` and play them.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        notes: Vec<SynthNote>,
        opens: Vec<i32>,
        a: f64,
        b: f64,
        speed: f64,
        bass: bool,
        looped: bool,
        tone: ToneParams,
        rate: u32,
        place: Arc<dyn Fn(f64) -> f64 + Send + Sync>,
    ) {
        let (generation, cancel) = self.begin(a, b, speed, looped);
        let ready = self.ready.clone();
        let (a, b) = (self.a, self.b);
        let spawned = std::thread::Builder::new()
            .name("qrs-synth".into())
            .spawn(move || {
                let o = RenderOptions {
                    rate,
                    speed,
                    bass,
                    tone: Some(tone),
                    looped,
                    seed: 1,
                    place: &*place,
                    cancel: &cancel,
                };
                if let Some(samples) = render(&notes, &opens, a, b, &o) {
                    *ready.lock() = Some((generation, Ok(Clip { rate, samples })));
                }
            });
        if let Err(e) = spawned {
            self.error = e.to_string();
            self.state = SynthState::Idle;
        }
    }

    /// Play chart time [a, b) from the song itself (`offset` = song time - chart time), slowed when speed < 1 and
    /// shifted by `semitones`.
    #[allow(clippy::too_many_arguments)]
    pub fn start_track(
        &mut self,
        song: Arc<crate::decode::SongBuffer>,
        a: f64,
        b: f64,
        offset: f64,
        speed: f64,
        semitones: f32,
        looped: bool,
    ) {
        let (generation, cancel) = self.begin(a, b, speed, looped);
        let ready = self.ready.clone();
        let (a, b) = (self.a, self.b);
        let spawned = std::thread::Builder::new()
            .name("qrs-synth".into())
            .spawn(move || {
                let samples = track_clip(&song, a + offset, b + offset, speed, semitones);
                if !cancel.load(Ordering::Relaxed) {
                    *ready.lock() = Some((
                        generation,
                        Ok(Clip {
                            rate: song.rate,
                            samples,
                        }),
                    ));
                }
            });
        if let Err(e) = spawned {
            self.error = e.to_string();
            self.state = SynthState::Idle;
        }
    }

    /// Call every frame: starts a finished render, notices the end of playback.
    pub fn update(&mut self, engine: &crate::Engine) {
        match self.state {
            SynthState::Rendering => {
                let done = self.ready.lock().take();
                if let Some((generation, res)) = done
                    && generation == self.generation.load(Ordering::Acquire)
                {
                    match res {
                        Ok(clip) => {
                            self.clip_id = engine.play_preview(clip, false, self.looped);
                            self.state = SynthState::Playing;
                        }
                        Err(e) => {
                            self.error = e;
                            self.state = SynthState::Idle;
                        }
                    }
                }
            }
            SynthState::Playing => {
                if engine.preview_state(self.clip_id) == crate::PreviewState::Over {
                    self.state = SynthState::Idle;
                }
            }
            SynthState::Idle => {}
        }
    }

    /// Where in the chart the synth is (seconds), or None when it isn't playing.
    pub fn position(&self, engine: &crate::Engine) -> Option<f64> {
        if self.state != SynthState::Playing {
            return None;
        }
        match engine.preview_state(self.clip_id) {
            crate::PreviewState::Pending => Some(self.a),
            crate::PreviewState::Playing => {
                let (pos, _) = engine.preview_position();
                let t = self.a + pos * self.speed;
                (t <= self.b).then_some(t)
            }
            crate::PreviewState::Over => None,
        }
    }

    /// Stop rendering, and fade out what's playing when there's an engine.
    pub fn stop_maybe(&mut self, engine: Option<&crate::Engine>) {
        match engine {
            Some(e) => self.stop(e),
            None => {
                self.cancel.store(true, Ordering::Release);
                self.generation.fetch_add(1, Ordering::AcqRel);
                self.state = SynthState::Idle;
            }
        }
    }

    /// Stop rendering and fade out what's playing.
    pub fn stop(&mut self, engine: &crate::Engine) {
        self.cancel.store(true, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
        if self.state == SynthState::Playing
            && engine.preview_state(self.clip_id) != crate::PreviewState::Over
        {
            engine.stop_preview(false);
        }
        self.state = SynthState::Idle;
    }
}

/// A part's passage [from, to) played on the synth as one mono take at `speed`, any length (rendered in
/// pieces of under a minute, each note in exactly one piece, their ring-outs overlapping), for checking the take
/// analysis (`quickrs-cli take-check`, the take tests). Sample 0 is chart time `from`.
pub fn render_mono(
    arr: &qrs_formats::Arrangement,
    from: f64,
    to: f64,
    speed: f64,
    tone: Option<ToneParams>,
    rate: u32,
) -> Vec<f32> {
    let notes = notes_of(arr);
    let opens = arr.open_string_midi();
    let cancel = AtomicBool::new(false);
    let place = |t: f64| t;
    let o = RenderOptions {
        rate,
        speed,
        bass: arr.is_bass,
        tone,
        looped: false,
        seed: 7,
        place: &place,
        cancel: &cancel,
    };
    let piece = (MAX_SECONDS - 10.0) * speed.min(1.0);
    let mut out: Vec<f32> = Vec::new();
    let mut a = from;
    while a < to {
        let b = (a + piece).min(to);
        let st = render(&notes, &opens, a, b, &o).unwrap_or_default();
        let at = ((a - from) / speed * rate as f64).round() as usize;
        let n = st.len() / 2;
        if out.len() < at + n {
            out.resize(at + n, 0.0);
        }
        for (k, f) in st.chunks_exact(2).enumerate() {
            out[at + k] += 0.5 * (f[0] + f[1]);
        }
        a = b;
    }
    out
}

/// A whole part on the synth, to play in place of the song's guitar (P23.11): mono at `rate`, `frames` long, chart
/// time t at sample (t + offset) x rate (`offset` = song time - chart time), at the chart's tempo, through `tone`
/// (dry when None). Unlike `render` it has no length limit and doesn't loop; it's left at about guitar-input level
/// (the dry peak at 0.35 before the tone) for the caller to set against the song. `progress` counts to 1000.
/// None if cancelled. Off the audio thread (it allocates the whole part).
#[allow(clippy::too_many_arguments)]
pub fn render_part(
    notes: &[SynthNote],
    opens: &[i32],
    bass: bool,
    tone: Option<ToneParams>,
    rate: u32,
    frames: usize,
    offset: f64,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Option<Vec<f32>> {
    let mut sel: Vec<&SynthNote> = notes.iter().collect();
    sel.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.string.cmp(&b.string)));
    let mut rng = Rng::new(1);
    let mut buf = vec![0.0f32; frames];
    let ok = pluck_notes(
        &sel,
        opens,
        -offset,
        1.0,
        bass,
        rate,
        &mut rng,
        cancel,
        Some(progress),
        |k, v| {
            if let Some(slot) = buf.get_mut(k) {
                *slot += v as f32;
            }
        },
    );
    if !ok {
        return None;
    }
    let peak = buf.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        for v in &mut buf {
            *v *= 0.35 / peak;
        }
    }
    if let Some(p) = tone {
        let mut chain = ToneChain::new(rate, p);
        for (i, c) in buf.chunks_mut(512).enumerate() {
            if i % 256 == 0 && cancel.load(Ordering::Relaxed) {
                return None;
            }
            chain.process(c);
        }
    }
    // a short fade at the very end, so a part that rings past the song's end doesn't click off
    let n = buf.len();
    let fade = n.min((0.05 * rate as f64) as usize);
    for k in 0..fade {
        buf[n - fade + k] *= 1.0 - k as f32 / fade as f32;
    }
    progress.store(1000, Ordering::Relaxed);
    Some(buf)
}

/// Song seconds [t0, t1) of the song's own audio as a stereo clip, slowed to `speed` and shifted by `semitones`,
/// with short fades at the ends so neither end (nor a loop coming round) clicks.
pub fn track_clip(
    song: &crate::decode::SongBuffer,
    t0: f64,
    t1: f64,
    speed: f64,
    semitones: f32,
) -> Vec<f32> {
    let rate = song.rate as f64;
    let i0 = ((t0 * rate).max(0.0) as usize).min(song.frames());
    let i1 = ((t1 * rate).max(0.0) as usize).clamp(i0, song.frames());
    let mut clip: Vec<f32> = song.samples[2 * i0..2 * i1]
        .iter()
        .map(|&s| s as f32 / 32768.0)
        .collect();
    // slower or faster (a take listened back at 105-150% too), and shifted
    if ((speed - 1.0).abs() > 1e-3 || semitones != 0.0) && !clip.is_empty() {
        clip = crate::stretch::stretch_clip(&clip, song.rate, speed, semitones);
    }
    let frames = clip.len() / 2;
    let n = (frames / 2).min((0.006 * rate) as usize);
    for k in 0..n {
        let w = k as f32 / n as f32;
        for c in 0..2 {
            clip[2 * k + c] *= w;
            clip[2 * (frames - 1 - k) + c] *= w;
        }
    }
    clip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_clip_is_stretched_faster_too() {
        let song = crate::decode::SongBuffer {
            rate: 48000,
            samples: std::sync::Arc::new(vec![1000i16; 48000 * 2 * 2]),
        };
        for (speed, want) in [(1.0, 1.0), (0.5, 2.0), (1.25, 0.8), (1.5, 1.0 / 1.5)] {
            let clip = track_clip(&song, 0.5, 1.5, speed, 0.0);
            let secs = clip.len() as f64 / 2.0 / 48000.0;
            assert!((secs - want).abs() < 0.01, "{speed}: {secs}");
        }
    }

    fn n(t: f64, string: i32, fret: i32, tech: Techs) -> SynthNote {
        SynthNote {
            t,
            string,
            fret,
            sus: 0.0,
            tech,
            slide_to: -1,
            bend: 0.0,
            in_chord: false,
        }
    }

    #[test]
    fn pluck_has_the_right_pitch() {
        let mut rng = Rng::new(3);
        let rate = 48000;
        let x = string_tone(110.0, 0.5, rate, None, 2.0, 0.6, &mut rng);
        let xf: Vec<f32> = x[4000..12192].iter().map(|&v| v as f32).collect();
        let mut sp = qrs_analysis::Spectra::new();
        let (f, _) = qrs_analysis::pitch::pitch(&mut sp, &xf, rate);
        assert!((1200.0 * (f / 110.0).log2()).abs() < 5.0, "{f}");
    }

    /// A recording that grows as it's "played in".
    struct Rec {
        rate: u32,
        data: Vec<f32>,
        written: u64,
    }

    impl qrs_analysis::SampleSource for Rec {
        fn rate(&self) -> u32 {
            self.rate
        }
        fn written(&self) -> u64 {
            self.written
        }
        fn read(&self, start: u64, out: &mut [f32]) -> bool {
            let end = start as usize + out.len();
            if end as u64 > self.written || end > self.data.len() {
                return false;
            }
            out.copy_from_slice(&self.data[start as usize..end]);
            true
        }
        fn latest(&self, out: &mut [f32]) -> bool {
            let w = self.written as usize;
            w >= out.len() && self.read((w - out.len()) as u64, out)
        }
    }

    /// The ear trainer's pluck reader (qrs-analysis ear.rs, P13.2) on plucked strings through the clean amp: Smoke
    /// on the Water's riff on the G string (lesson 1's first, at its chart's timing, the quick 6-5 included), the
    /// sound coming in 10 ms at a time and each onset reported 8 ms late, as the engine's are. Every pluck is read
    /// once, as its note.
    #[test]
    fn the_ear_reader_hears_a_plucked_riff() {
        let rate = 48000;
        let riff: [(f64, i32); 12] = [
            (0.0, 0),
            (0.537, 3),
            (1.093, 5),
            (1.906, 0),
            (2.448, 3),
            (2.990, 6),
            (3.261, 5),
            (4.309, 0),
            (4.851, 3),
            (5.419, 5),
            (6.218, 3),
            (6.752, 0),
        ];
        let notes: Vec<SynthNote> = riff
            .iter()
            .map(|&(t, fret)| SynthNote {
                sus: 0.5,
                ..n(1.0 + t, 3, fret, Techs::NONE)
            })
            .collect();
        let cancel = AtomicBool::new(false);
        let place = |t: f64| t;
        let o = RenderOptions {
            rate,
            speed: 1.0,
            bass: false,
            tone: Some(Preset::Clean.params()),
            looped: false,
            seed: 7,
            place: &place,
            cancel: &cancel,
        };
        let opens = [40, 45, 50, 55, 59, 64];
        let clip = render(&notes, &opens, 1.0, 9.0, &o).unwrap();
        let mono: Vec<f32> = clip.chunks(2).map(|f| f[0]).collect();
        let mut rec = Rec {
            rate,
            data: mono,
            written: 0,
        };
        let mut reader = qrs_analysis::ear::PluckReader::default();
        let mut read: Vec<(u64, Option<f64>)> = Vec::new();
        let onsets: Vec<u64> = riff.iter().map(|&(t, _)| (t * rate as f64) as u64).collect();
        let (step, late) = (rate as u64 / 100, (0.008 * rate as f64) as u64);
        let mut next = 0;
        while rec.written < rec.data.len() as u64 {
            rec.written = (rec.written + step).min(rec.data.len() as u64);
            while next < onsets.len() && onsets[next] + late <= rec.written {
                reader.push(onsets[next], rate);
                next += 1;
            }
            read.extend(reader.poll(&rec));
        }
        assert_eq!(read.len(), riff.len(), "{read:?}");
        for ((index, midi), &(t, fret)) in read.iter().zip(&riff) {
            assert_eq!(*index, (t * rate as f64) as u64);
            let m = midi.unwrap_or_else(|| panic!("the pluck at {t} s wasn't read"));
            assert_eq!(m.round() as i32, 55 + fret, "the pluck at {t} s read as {m:.2}");
        }
    }

    #[test]
    fn render_selection_and_loop() {
        let notes = vec![
            n(1.0, 0, 3, Techs::NONE),
            n(1.5, 1, 5, Techs::BEND),
            n(2.0, 2, 7, Techs::PALM_MUTE),
            n(5.0, 0, 0, Techs::NONE),
        ];
        let cancel = AtomicBool::new(false);
        let place = |t: f64| t;
        let o = RenderOptions {
            rate: 48000,
            speed: 1.0,
            bass: false,
            tone: Some(Preset::Clean.params()),
            looped: false,
            seed: 1,
            place: &place,
            cancel: &cancel,
        };
        let clip = render(&notes, &[40, 45, 50, 55, 59, 64], 1.0, 3.0, &o).unwrap();
        assert_eq!(clip.len() / 2, (2.0 * 48000.0) as usize + 48000);
        assert!(clip.iter().all(|v| v.is_finite() && v.abs() <= 0.8001));
        let o2 = RenderOptions { looped: true, ..o };
        let lc = render(&notes, &[40, 45, 50, 55, 59, 64], 1.0, 3.0, &o2).unwrap();
        assert_eq!(lc.len() / 2, 96000);
        // the end of the loop meets its start without a jump
        let jump = (lc[lc.len() - 2] - lc[0]).abs();
        assert!(jump < 0.1, "{jump}");
    }

    #[test]
    fn cancelled_render_stops() {
        let notes: Vec<SynthNote> = (0..200).map(|i| n(i as f64 * 0.1, 0, 3, Techs::NONE)).collect();
        let cancel = AtomicBool::new(true);
        let place = |t: f64| t;
        let o = RenderOptions {
            rate: 48000,
            speed: 1.0,
            bass: false,
            tone: None,
            looped: false,
            seed: 1,
            place: &place,
            cancel: &cancel,
        };
        assert!(render(&notes, &[40, 45, 50, 55, 59, 64], 0.0, 20.0, &o).is_none());
    }

    #[test]
    fn a_whole_part_plays_on_the_song_s_clock() {
        // two notes, the song 0.5 s ahead of the chart, longer than a selection can be
        let rate = 48000;
        let notes = vec![n(1.0, 0, 5, Techs::NONE), n(70.0, 0, 7, Techs::NONE)];
        let opens = [40, 45, 50, 55, 59, 64];
        let (cancel, prog) = (AtomicBool::new(false), AtomicU32::new(0));
        let frames = 75 * rate as usize;
        let out = render_part(&notes, &opens, false, None, rate, frames, 0.5, &cancel, &prog).unwrap();
        assert_eq!(out.len(), frames, "as long as the song");
        assert_eq!(prog.load(Ordering::Relaxed), 1000);
        let first = out.iter().position(|v| v.abs() > 1e-3).unwrap();
        assert!((first as f64 / rate as f64 - 1.5).abs() < 0.01, "{first}");
        // silent between the notes, then the second at 70.5 s with its pitch (string 0 fret 7: B2)
        let gap = &out[(5 * rate) as usize..(70 * rate) as usize];
        assert!(gap.iter().all(|v| v.abs() < 1e-3));
        let at = (70.6 * rate as f64) as usize;
        let xf = &out[at..at + 8192];
        let mut sp = qrs_analysis::Spectra::new();
        let (f, _) = qrs_analysis::pitch::pitch(&mut sp, xf, rate);
        assert!((1200.0 * (f / midi_hz(47.0)).log2()).abs() < 10.0, "{f}");
        assert!(out.iter().all(|v| v.is_finite() && v.abs() < 1.0));
        // cancelled: nothing
        cancel.store(true, Ordering::Relaxed);
        assert!(render_part(&notes, &opens, false, None, rate, frames, 0.5, &cancel, &prog).is_none());
    }

    #[test]
    fn bends_are_half_steps() {
        let mut g = n(0.0, 0, 5, Techs::BEND);
        g.bend = 2.0; // a full-step bend in the chart
        let c = pitch_curve(&g, 1.0, 48000).unwrap();
        assert!((c[40000] - 2.0).abs() < 1e-6, "{}", c[40000]);
    }
}
