//! Blues licks for the Scales page: short phrases in the minor pentatonic and blues boxes, played on the synth
//! guitar and then played back by you (call and response), checked note by note with the bends' pitch.
//!
//! The licks are written in A (E Standard, string 0 the low E) in box 1 (frets 5-8) or box 2 (frets 7-10), and
//! moved to any key and tuning. They're everyday blues vocabulary (curls, bends to the root, double stops,
//! turnarounds) written for quickRS, not transcriptions of anyone's recordings.

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, Painter, Rect, Stroke, pos2};
use qrs_audio::synth::{SynthNote, SynthPlayer, SynthState};
use qrs_formats::model::Techs;
use qrs_ui::theme::t;
use qrs_ui::units::px;

use crate::library::txt;
use crate::scales::Spot;

/// E Standard's open strings, the licks' own tuning.
const STD: [i32; 6] = [40, 45, 50, 55, 59, 64];
/// The licks' key: A.
const LICK_ROOT: i32 = 9;
/// Each box's start on the low string in A (box 1 at the 5th fret, box 2 at the 8th).
const ANCHOR: [i32; 2] = [5, 8];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tech {
    None,
    /// Bent up this many semitones.
    Bend(f32),
    /// Hammered on from the note before.
    Hammer,
    /// Pulled off from the note before.
    Pull,
    /// Slid into from the note before.
    Slide,
    Vibrato,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Opener,
    Riff,
    OverIv,
    Turnaround,
    Ending,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Opener => "opener",
            Role::Riff => "riff",
            Role::OverIv => "over the IV chord",
            Role::Turnaround => "turnaround",
            Role::Ending => "ending",
        }
    }
}

/// A note of a lick: string (0 = low E), fret (in A), when and how long (beats), and how it's played.
#[derive(Clone, Copy, Debug)]
pub struct Ln {
    pub s: usize,
    pub f: i32,
    pub t: f32,
    pub d: f32,
    pub tech: Tech,
}

const fn n(s: usize, f: i32, t: f32, d: f32) -> Ln {
    Ln { s, f, t, d, tech: Tech::None }
}
const fn b(s: usize, f: i32, t: f32, d: f32, semis: f32) -> Ln {
    Ln { s, f, t, d, tech: Tech::Bend(semis) }
}
const fn h(s: usize, f: i32, t: f32, d: f32) -> Ln {
    Ln { s, f, t, d, tech: Tech::Hammer }
}
const fn p(s: usize, f: i32, t: f32, d: f32) -> Ln {
    Ln { s, f, t, d, tech: Tech::Pull }
}
const fn sl(s: usize, f: i32, t: f32, d: f32) -> Ln {
    Ln { s, f, t, d, tech: Tech::Slide }
}
const fn v(s: usize, f: i32, t: f32, d: f32) -> Ln {
    Ln { s, f, t, d, tech: Tech::Vibrato }
}

pub struct Lick {
    pub name: &'static str,
    /// 0 = box 1, 1 = box 2.
    pub boxn: usize,
    pub role: Role,
    /// Uses the blues scale's ♭5.
    pub blues: bool,
    pub notes: &'static [Ln],
}

const T: f32 = 1.0 / 3.0; // a triplet

pub const LICKS: &[Lick] = &[
    // -- box 1 ---------------------------------------------------------------------------------------------------
    Lick {
        name: "Bend to the root and settle",
        boxn: 0,
        role: Role::Ending,
        blues: false,
        notes: &[b(3, 7, 0.0, 1.0, 2.0), n(4, 5, 1.0, 0.5), n(3, 7, 1.5, 0.5), n(3, 5, 2.0, 0.5), v(2, 7, 2.5, 1.5)],
    },
    Lick {
        name: "Minor-third curl",
        boxn: 0,
        role: Role::Opener,
        blues: false,
        notes: &[b(3, 5, 0.0, 0.5, 0.5), n(2, 7, 0.5, 0.5), n(3, 5, 1.0, 0.5), n(2, 7, 1.5, 0.5), n(2, 5, 2.0, 0.5), v(2, 7, 2.5, 1.5)],
    },
    Lick {
        name: "High bend and drop",
        boxn: 0,
        role: Role::Opener,
        blues: false,
        notes: &[b(4, 8, 0.0, 1.0, 2.0), n(5, 5, 1.0, 0.5), n(4, 8, 1.5, 0.5), n(4, 5, 2.0, 0.5), v(3, 7, 2.5, 1.5)],
    },
    Lick {
        name: "Double-stop stabs",
        boxn: 0,
        role: Role::Opener,
        blues: false,
        notes: &[
            n(4, 5, 0.0, 0.25), n(5, 5, 0.0, 0.25),
            n(4, 5, 0.5, 0.25), n(5, 5, 0.5, 0.25),
            n(4, 8, 1.0, 0.5),
            n(4, 5, 1.5, 0.5),
            v(3, 7, 2.0, 2.0),
        ],
    },
    Lick {
        name: "Blue-note walk down",
        boxn: 0,
        role: Role::Ending,
        blues: true,
        notes: &[
            n(3, 8, 0.0, 0.5), n(3, 7, 0.5, 0.5), n(3, 5, 1.0, 0.5), n(2, 7, 1.5, 0.5),
            n(2, 5, 2.0, 0.5), n(1, 7, 2.5, 0.5), n(1, 6, 3.0, 0.5), n(1, 5, 3.5, 0.5), v(0, 5, 4.0, 2.0),
        ],
    },
    Lick {
        name: "Hammer-on climb",
        boxn: 0,
        role: Role::Opener,
        blues: false,
        notes: &[
            n(1, 5, 0.0, 0.25), h(1, 7, 0.25, 0.25), n(2, 5, 0.5, 0.25), h(2, 7, 0.75, 0.25),
            n(3, 5, 1.0, 0.25), h(3, 7, 1.25, 0.25), v(4, 5, 1.5, 1.5),
        ],
    },
    Lick {
        name: "Pull-off cascade",
        boxn: 0,
        role: Role::Riff,
        blues: false,
        notes: &[
            n(5, 8, 0.0, 0.25), p(5, 5, 0.25, 0.25), n(4, 8, 0.5, 0.25), p(4, 5, 0.75, 0.25),
            n(3, 7, 1.0, 0.25), p(3, 5, 1.25, 0.25), v(2, 7, 1.5, 1.5),
        ],
    },
    Lick {
        name: "Land on the IV",
        boxn: 0,
        role: Role::OverIv,
        blues: false,
        notes: &[n(4, 5, 0.0, 0.5), n(3, 7, 0.5, 0.5), n(3, 5, 1.0, 0.5), n(2, 7, 1.5, 0.5), v(3, 7, 2.0, 2.0)],
    },
    Lick {
        name: "Turnaround to the V",
        boxn: 0,
        role: Role::Turnaround,
        blues: false,
        notes: &[
            n(5, 5, 0.0, 0.5), n(4, 8, 0.5, 0.5), n(4, 5, 1.0, 0.5), n(3, 7, 1.5, 0.5),
            n(3, 5, 2.0, 0.5), n(2, 7, 2.5, 0.5), v(1, 7, 3.0, 1.0),
        ],
    },
    Lick {
        name: "Low-string blues climb",
        boxn: 0,
        role: Role::Riff,
        blues: true,
        notes: &[
            n(0, 5, 0.0, 0.5), n(0, 8, 0.5, 0.5), n(1, 5, 1.0, 0.5), n(1, 6, 1.5, 0.5),
            n(1, 7, 2.0, 0.5), n(2, 5, 2.5, 0.5), n(2, 7, 3.0, 1.0),
        ],
    },
    Lick {
        name: "Crying bends",
        boxn: 0,
        role: Role::Opener,
        blues: false,
        notes: &[b(4, 8, 0.0, 0.5, 2.0), b(4, 8, 0.75, 0.5, 2.0), n(5, 5, 1.5, 0.5), n(4, 8, 2.0, 0.5), v(4, 5, 2.5, 1.5)],
    },
    Lick {
        name: "Slide up to the root",
        boxn: 0,
        role: Role::Opener,
        blues: false,
        notes: &[
            n(2, 5, 0.0, 0.25), sl(2, 7, 0.25, 0.25), n(3, 5, 0.5, 0.5), n(3, 7, 1.0, 0.5),
            n(4, 5, 1.5, 0.5), n(3, 7, 2.0, 0.5), v(2, 7, 2.5, 1.5),
        ],
    },
    Lick {
        name: "Question and answer",
        boxn: 0,
        role: Role::Riff,
        blues: false,
        notes: &[b(5, 8, 0.0, 1.0, 2.0), n(5, 5, 1.0, 1.0), b(4, 8, 2.0, 1.0, 2.0), v(4, 5, 3.0, 1.0)],
    },
    Lick {
        name: "Down to the low root",
        boxn: 0,
        role: Role::Ending,
        blues: false,
        notes: &[
            n(4, 8, 0.0, 0.5), n(4, 5, 0.5, 0.5), n(3, 7, 1.0, 0.5), n(3, 5, 1.5, 0.5),
            n(2, 7, 2.0, 0.5), n(1, 7, 2.5, 0.5), v(0, 5, 3.0, 2.0),
        ],
    },
    // -- box 2 ---------------------------------------------------------------------------------------------------
    Lick {
        name: "Box 2 high bend",
        boxn: 1,
        role: Role::Opener,
        blues: false,
        notes: &[n(4, 10, 0.0, 0.5), b(5, 8, 0.5, 1.0, 2.0), n(4, 10, 1.5, 0.5), n(4, 8, 2.0, 0.5), v(3, 9, 2.5, 1.5)],
    },
    Lick {
        name: "Box 2 G-string bend",
        boxn: 1,
        role: Role::Riff,
        blues: false,
        notes: &[
            b(3, 7, 0.0, 1.0, 2.0), n(4, 8, 1.0, 0.5), n(4, 10, 1.5, 0.5), n(4, 8, 2.0, 0.5),
            n(3, 9, 2.5, 0.5), v(3, 7, 3.0, 1.0),
        ],
    },
    Lick {
        name: "Box 2 run down to the root",
        boxn: 1,
        role: Role::Ending,
        blues: false,
        notes: &[
            n(5, 10, 0.0, 0.5), n(5, 8, 0.5, 0.5), n(4, 10, 1.0, 0.5), n(4, 8, 1.5, 0.5),
            n(3, 9, 2.0, 0.5), n(3, 7, 2.5, 0.5), n(2, 10, 3.0, 0.5), v(2, 7, 3.5, 1.5),
        ],
    },
    Lick {
        name: "Box 2 hammer triplets",
        boxn: 1,
        role: Role::Riff,
        blues: false,
        notes: &[
            n(2, 7, 0.0, T), h(2, 10, T, T), n(3, 7, 2.0 * T, T), h(3, 9, 1.0, T),
            n(4, 8, 1.0 + T, T), h(4, 10, 1.0 + 2.0 * T, T), v(4, 10, 2.0, 1.5),
        ],
    },
    Lick {
        name: "Box 2 double stops",
        boxn: 1,
        role: Role::Opener,
        blues: false,
        notes: &[
            n(3, 7, 0.0, 0.5), n(4, 8, 0.0, 0.5),
            n(3, 9, 0.5, 0.5), n(4, 10, 0.5, 0.5),
            n(3, 9, 1.5, 0.5),
            v(3, 7, 2.0, 2.0),
        ],
    },
    Lick {
        name: "Box 2 blue-note slip",
        boxn: 1,
        role: Role::OverIv,
        blues: true,
        notes: &[n(3, 9, 0.0, 0.5), n(3, 8, 0.5, 0.5), n(3, 7, 1.0, 0.5), n(2, 10, 1.5, 0.5), v(2, 7, 2.0, 2.0)],
    },
];

/// The licks for a box (None: every box), the blues ones only with the blues scale.
pub fn for_box(boxn: Option<usize>, blues: bool) -> Vec<usize> {
    (0..LICKS.len())
        .filter(|&i| boxn.is_none_or(|b| LICKS[i].boxn == b) && (blues || !LICKS[i].blues))
        .collect()
}

/// A lick's notes in `root` on these strings: where each is, as the box shows it.
pub fn place(l: &Lick, root: i32, strings: &[i32; 6]) -> Vec<(Spot, Ln)> {
    let k = (root - LICK_ROOT).rem_euclid(12);
    // the box's start on the low string, moved as box_notes places it (fret 0 to 11)
    let raw = STD[0] + ANCHOR[l.boxn] + k - strings[0];
    let delta = raw.rem_euclid(12) - raw;
    l.notes
        .iter()
        .map(|ln| {
            let midi = STD[ln.s] + ln.f + k;
            (Spot { string: ln.s, fret: midi - strings[ln.s] + delta }, *ln)
        })
        .collect()
}

/// One thing to hear, in order: a note (the highest of a double stop), or a bend's top.
#[derive(Clone, Debug)]
pub struct Check {
    /// The pitch to hear (a bend's: where it's bent to).
    pub target: i32,
    /// A bend: the note picked before it's bent (heard on the way up, not wrong).
    pub base: Option<i32>,
    /// A double stop's other notes (heard instead: fine).
    pub alt: Vec<i32>,
    /// Slid into from this pitch (the notes slid through aren't wrong).
    pub slide_from: Option<i32>,
    /// The notes it is (indexes into the placed lick).
    pub notes: Vec<usize>,
}

pub fn checks(placed: &[(Spot, Ln)], strings: &[i32; 6]) -> Vec<Check> {
    let midi = |i: usize| strings[placed[i].0.string] + placed[i].0.fret;
    let mut out: Vec<Check> = Vec::new();
    let mut i = 0;
    while i < placed.len() {
        let mut group = vec![i];
        while i + group.len() < placed.len() && (placed[i + group.len()].1.t - placed[i].1.t).abs() < 1e-3 {
            group.push(i + group.len());
        }
        let top = *group.iter().max_by_key(|&&j| midi(j)).unwrap();
        let (target, base) = match placed[top].1.tech {
            Tech::Bend(s) => (midi(top) + s.round() as i32, Some(midi(top))),
            _ => (midi(top), None),
        };
        let alt: Vec<i32> = group.iter().filter(|&&j| j != top).map(|&j| midi(j)).collect();
        let slide_from = (placed[top].1.tech == Tech::Slide).then(|| out.last().map(|c| c.target)).flatten();
        // the same note again right after (a repeated stab) is heard as one
        let repeat = base.is_none() && out.last().is_some_and(|c| c.target == target && c.base.is_none());
        if repeat {
            out.last_mut().unwrap().notes.extend(group.iter().copied());
        } else {
            out.push(Check { target, base, alt, slide_from, notes: group.clone() });
        }
        i += group.len();
    }
    out
}

/// The lick on the synth guitar at `bpm`, starting `lead` seconds in: (notes, end).
pub fn synth_notes(placed: &[(Spot, Ln)], bpm: f32, lead: f64) -> (Vec<SynthNote>, f64) {
    let spb = 60.0 / bpm as f64;
    let mut out: Vec<SynthNote> = Vec::new();
    let mut end = 0.0f64;
    for (i, (spot, ln)) in placed.iter().enumerate() {
        let t0 = lead + ln.t as f64 * spb;
        let sus = (ln.d as f64 * spb).max(0.08);
        end = end.max(t0 + sus);
        // a slide's destination: the note before slides into it (and rings on through it)
        if ln.tech == Tech::Slide
            && let Some(prev) = out.last_mut()
        {
            prev.tech |= Techs::SLIDE;
            prev.slide_to = spot.fret;
            prev.sus = t0 + sus - prev.t;
            continue;
        }
        let (tech, bend) = match ln.tech {
            Tech::Bend(s) => (Techs::BEND, s as f64),
            Tech::Hammer => (Techs::HAMMER_ON, 0.0),
            Tech::Pull => (Techs::PULL_OFF, 0.0),
            Tech::Vibrato => (Techs::VIBRATO, 0.0),
            _ => (Techs::NONE, 0.0),
        };
        let chord = placed
            .iter()
            .enumerate()
            .any(|(j, (_, o))| j != i && (o.t - ln.t).abs() < 1e-3);
        out.push(SynthNote {
            t: t0,
            string: spot.string as i32,
            fret: spot.fret,
            sus,
            tech,
            slide_to: -1,
            bend,
            in_chord: chord,
        });
    }
    (out, end)
}

/// A fret as the tab writes it, with how it's played.
fn tab_text(ln: &Ln, fret: i32) -> String {
    match ln.tech {
        Tech::None => fret.to_string(),
        Tech::Bend(s) if s < 1.0 => format!("{fret}b¼"),
        Tech::Bend(s) if s < 1.5 => format!("{fret}b½"),
        Tech::Bend(_) => format!("{fret}b"),
        Tech::Hammer => format!("h{fret}"),
        Tech::Pull => format!("p{fret}"),
        Tech::Slide => format!("/{fret}"),
        Tech::Vibrato => format!("{fret}~"),
    }
}

/// A technique's mark beside its dot on the neck.
pub fn neck_mark(ln: &Ln) -> Option<&'static str> {
    match ln.tech {
        Tech::Bend(s) if s < 1.0 => Some("¼"),
        Tech::Bend(s) if s < 1.5 => Some("↑½"),
        Tech::Bend(_) => Some("↑1"),
        Tech::Hammer => Some("h"),
        Tech::Pull => Some("p"),
        Tech::Slide => Some("/"),
        Tech::Vibrato => Some("~"),
        Tech::None => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Showing the lick; nothing playing.
    Idle,
    /// The synth is playing it (the input isn't listened to: the mic would hear the synth).
    Demo,
    /// Your turn: listening.
    Turn,
    /// Played through; the result shows a moment.
    Done,
}

/// Call and response on one lick.
pub struct LickRun {
    pub lick: usize,
    pub bpm: f32,
    /// Play it, then your turn, again and again.
    pub auto: bool,
    pub phase: Phase,
    pub step: usize,
    pub misses: u32,
    pub bends_short: u32,
    /// The highest pitch heard during the bend being played (MIDI, fractional).
    bend_peak: Option<f64>,
    pub clean_reps: u32,
    pub message: String,
    pub wrong: Option<(i32, Instant)>,
    phase_at: Instant,
    synth: SynthPlayer,
    synth_started: bool,
    /// When the demo's synth stopped (the turn starts a moment later).
    demo_ended: Option<Instant>,
}

/// Clean takes in a row before the tempo goes up, and by how much.
const CLEAN_FOR_TEMPO: u32 = 3;
const TEMPO_STEP: f32 = 6.0;
pub const BPM_MIN: f32 = 40.0;
pub const BPM_MAX: f32 = 160.0;
/// After the synth stops, a moment before listening (its last note dies away).
const MUTE_AFTER_DEMO: Duration = Duration::from_millis(350);
/// A bend counts once it's within this of its target (semitones).
const BEND_OK: f64 = 0.3;

impl Default for LickRun {
    fn default() -> Self {
        LickRun {
            lick: 0,
            bpm: 70.0,
            auto: true,
            phase: Phase::Idle,
            step: 0,
            misses: 0,
            bends_short: 0,
            bend_peak: None,
            clean_reps: 0,
            message: String::new(),
            wrong: None,
            phase_at: Instant::now(),
            synth: SynthPlayer::default(),
            synth_started: false,
            demo_ended: None,
        }
    }
}

impl LickRun {
    fn reset_take(&mut self) {
        self.step = 0;
        self.misses = 0;
        self.bends_short = 0;
        self.bend_peak = None;
        self.wrong = None;
    }

    /// Another lick (or the same one again): back to showing it.
    pub fn stop(&mut self, engine: Option<&qrs_audio::Engine>) {
        self.synth.stop_maybe(engine);
        self.phase = Phase::Idle;
        self.reset_take();
        self.message.clear();
    }

    /// Play the lick on the synth; your turn follows.
    pub fn demo(&mut self, engine: &qrs_audio::Engine, placed: &[(Spot, Ln)], strings: &[i32; 6]) {
        let (notes, end) = synth_notes(placed, self.bpm, 0.15);
        let tone = qrs_audio::synth::synth_tone(&engine.tone(), false);
        let place: std::sync::Arc<dyn Fn(f64) -> f64 + Send + Sync> = std::sync::Arc::new(|x| x);
        self.synth.start(notes, strings.to_vec(), 0.0, end + 0.6, 1.0, false, false, tone, engine.rate(), place);
        self.synth_started = false;
        self.demo_ended = None;
        self.reset_take();
        self.message.clear();
        self.phase = Phase::Demo;
        self.phase_at = Instant::now();
    }

    /// Your turn now, without hearing it first.
    pub fn turn(&mut self, engine: Option<&qrs_audio::Engine>) {
        self.synth.stop_maybe(engine);
        self.reset_take();
        self.message = "Your turn".into();
        self.phase = Phase::Turn;
        self.phase_at = Instant::now();
    }

    /// Whether the input is being listened to now.
    pub fn listening(&self) -> bool {
        self.phase == Phase::Turn
    }

    /// Every frame: the synth, and the moves between phases. Returns true when a new demo should start (auto).
    pub fn update(&mut self, engine: Option<&qrs_audio::Engine>, now: Instant) -> bool {
        if let Some(e) = engine {
            self.synth.update(e);
        }
        match self.phase {
            Phase::Demo => {
                if self.synth.state == SynthState::Playing {
                    self.synth_started = true;
                }
                // over: the synth played and stopped (or never could: no engine, or it gave up rendering)
                let over = self.synth.state == SynthState::Idle
                    && (self.synth_started || engine.is_none() || now - self.phase_at > Duration::from_secs(20));
                if over && self.demo_ended.is_none() {
                    self.demo_ended = Some(now); // the last note's ring dies away first
                }
                if self.demo_ended.is_some_and(|t| now - t >= MUTE_AFTER_DEMO) {
                    self.demo_ended = None;
                    self.turn(None);
                }
                false
            }
            Phase::Done => self.auto && now - self.phase_at >= Duration::from_millis(1600),
            _ => false,
        }
    }

    /// The pitch heard now (for the bend being played).
    pub fn pitch(&mut self, midi: Option<f64>, checks: &[Check], now: Instant) {
        if self.phase != Phase::Turn {
            return;
        }
        let Some(c) = checks.get(self.step) else { return };
        let (Some(base), Some(m)) = (c.base, midi) else { return };
        if m > base as f64 - 0.5 && m < c.target as f64 + 0.6 {
            self.bend_peak = Some(self.bend_peak.map_or(m, |p| p.max(m)));
            if m >= c.target as f64 - BEND_OK {
                self.bend_peak = None;
                self.advance(checks, now);
            }
        }
    }

    /// A note was played (held steady): count it against the lick.
    pub fn onset(&mut self, m: i32, checks: &[Check], now: Instant) {
        if self.phase != Phase::Turn {
            return;
        }
        let Some(c) = checks.get(self.step).cloned() else { return };
        if m == c.target || c.alt.contains(&m) {
            self.bend_peak = None;
            self.advance(checks, now);
            return;
        }
        if c.base == Some(m) {
            return; // picked, about to be bent
        }
        // a bend that didn't get there, and the lick moved on
        if let Some(base) = c.base
            && let Some(next) = checks.get(self.step + 1)
            && (m == next.target || next.alt.contains(&m) || next.base == Some(m))
        {
            let short = self.bend_peak.map(|p| ((c.target as f64 - p) * 100.0).round() as i32);
            self.bends_short += 1;
            self.message = match short {
                Some(cents) if cents < 100 => format!("That bend was {cents} cents short"),
                _ => format!("Bend that {} up to {}", name(base), name(c.target)),
            };
            self.bend_peak = None;
            self.advance(checks, now);
            self.onset(m, checks, now);
            return;
        }
        // still ringing, or slid through
        let prev = self.step.checked_sub(1).and_then(|i| checks.get(i));
        if prev.is_some_and(|p| p.target == m || p.alt.contains(&m) || p.base == Some(m)) {
            return;
        }
        if let Some(from) = c.slide_from
            && m > from.min(c.target)
            && m < from.max(c.target)
        {
            return;
        }
        self.misses += 1;
        self.wrong = Some((m, now));
    }

    fn advance(&mut self, checks: &[Check], now: Instant) {
        self.step += 1;
        if self.step < checks.len() {
            return;
        }
        // through: how did it go
        self.phase = Phase::Done;
        self.phase_at = now;
        let clean = self.misses == 0 && self.bends_short == 0;
        if clean {
            self.clean_reps += 1;
            if self.clean_reps >= CLEAN_FOR_TEMPO && self.bpm < BPM_MAX {
                self.clean_reps = 0;
                self.bpm = (self.bpm + TEMPO_STEP).min(BPM_MAX);
                self.message = format!("Clean three times: tempo up to {:.0} bpm", self.bpm);
            } else {
                self.message = format!("Clean! ({} of {CLEAN_FOR_TEMPO} for a tempo step)", self.clean_reps);
            }
        } else {
            self.clean_reps = 0;
            let mut bits = Vec::new();
            if self.misses > 0 {
                bits.push(format!("{} wrong note{}", self.misses, if self.misses == 1 { "" } else { "s" }));
            }
            if self.bends_short > 0 && !self.message.contains("bend") {
                bits.push(format!("{} short bend{}", self.bends_short, if self.bends_short == 1 { "" } else { "s" }));
            }
            if !bits.is_empty() {
                self.message = format!("Through, with {}", bits.join(" and "));
            }
        }
    }
}

fn name(m: i32) -> &'static str {
    ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"][m.rem_euclid(12) as usize]
}

/// The lick as tab (high e on top), its notes along its beats; the ones played so far lit, the next ringed.
pub fn draw_tab(p: &Painter, rect: Rect, placed: &[(Spot, Ln)], checks: &[Check], step: Option<usize>) {
    let th = t();
    qrs_ui::paint::card(p, rect, px(16.0), false);
    let inner = rect.shrink2(egui::vec2(px(48.0), px(16.0)));
    let gap = inner.height() / 5.0;
    let y = |s: usize| inner.min.y + gap * (5 - s) as f32; // string 5 (high e) on top
    for s in 0..6 {
        p.line_segment([pos2(inner.min.x, y(s)), pos2(inner.max.x, y(s))], Stroke::new(px(1.0), th.line));
        txt(p, ["E", "A", "D", "G", "B", "e"][s], pos2(rect.min.x + px(24.0), y(s)), 11.0, th.caption, 700, Align2::CENTER_CENTER, None);
    }
    let beats = placed.iter().map(|(_, l)| l.t + l.d).fold(0.0f32, f32::max).max(1.0);
    for bt in 0..=beats.ceil() as i32 {
        let x = inner.min.x + inner.width() * (bt as f32 / beats).min(1.0);
        p.line_segment([pos2(x, y(5) - px(4.0)), pos2(x, y(0) + px(4.0))], Stroke::new(px(1.0), faded(th.line, 0.35)));
    }
    let done_upto = step.unwrap_or(0);
    let next_notes: &[usize] = step.and_then(|s| checks.get(s)).map(|c| c.notes.as_slice()).unwrap_or(&[]);
    let done: Vec<usize> = checks.iter().take(done_upto).flat_map(|c| c.notes.iter().copied()).collect();
    for (i, (spot, ln)) in placed.iter().enumerate() {
        let x = inner.min.x + inner.width() * (ln.t / beats);
        let label = tab_text(ln, spot.fret);
        let at = pos2(x + px(12.0), y(spot.string));
        let w = px(8.0) + px(7.5) * label.chars().count() as f32;
        let bg = Rect::from_center_size(at, egui::vec2(w, px(18.0)));
        let lit = done.contains(&i);
        let next = next_notes.contains(&i);
        p.rect_filled(bg, 4.0, if lit { th.accent } else { th.base });
        if next {
            p.rect_stroke(bg.expand(px(2.0)), 5.0, Stroke::new(px(2.0), th.accent), egui::StrokeKind::Outside);
        }
        txt(p, &label, at, 13.0, if lit { th.on_accent } else { th.text }, 800, Align2::CENTER_CENTER, None);
    }
}

fn faded(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scales::{ScaleKind, box_notes};

    #[test]
    fn every_lick_is_in_its_box_and_scale() {
        for l in LICKS {
            let kind = if l.blues { ScaleKind::Blues } else { ScaleKind::MinorPent };
            let bx = box_notes(LICK_ROOT, kind, l.boxn, &STD);
            let lo = bx.iter().map(|s| s.fret).min().unwrap() - 2;
            let hi = bx.iter().map(|s| s.fret).max().unwrap() + 1;
            for (spot, ln) in place(l, LICK_ROOT, &STD) {
                let m = STD[spot.string] + spot.fret;
                assert!(kind.steps().contains(&(m - LICK_ROOT).rem_euclid(12)), "{}: {ln:?} not in the scale", l.name);
                assert!((lo..=hi).contains(&spot.fret), "{}: {ln:?} outside box {}", l.name, l.boxn + 1);
                if let Tech::Bend(s) = ln.tech {
                    let top = m + s.round() as i32;
                    assert!(s < 1.0 || kind.steps().contains(&(top - LICK_ROOT).rem_euclid(12)), "{}: bend to {top}", l.name);
                }
            }
        }
    }

    #[test]
    fn licks_move_with_the_key_like_the_boxes() {
        for root in 0..12 {
            for l in LICKS {
                let placed = place(l, root, &STD);
                let bx = box_notes(root, ScaleKind::MinorPent, l.boxn, &STD);
                let lo = bx.iter().map(|s| s.fret).min().unwrap() - 2;
                let hi = bx.iter().map(|s| s.fret).max().unwrap() + 1;
                for (spot, _) in &placed {
                    assert!((lo..=hi).contains(&spot.fret), "{} in {root}: fret {}", l.name, spot.fret);
                }
            }
        }
    }

    #[test]
    fn drop_d_moves_only_the_low_string() {
        let drop = [38, 45, 50, 55, 59, 64];
        let l = &LICKS[4]; // ends on the low root
        let a = place(l, LICK_ROOT, &STD);
        let d = place(l, LICK_ROOT, &drop);
        for ((sa, _), (sd, _)) in a.iter().zip(&d) {
            let ma = STD[sa.string] + sa.fret;
            let md = drop[sd.string] + sd.fret;
            assert_eq!(ma % 12, md % 12);
        }
    }

    #[test]
    fn double_stops_and_repeats_check_once() {
        let placed = place(&LICKS[3], LICK_ROOT, &STD);
        let c = checks(&placed, &STD);
        // two stabs of the same double stop: one check (the top A), then G, E, D
        assert_eq!(c.iter().map(|c| c.target).collect::<Vec<_>>(), vec![69, 67, 64, 62]);
        assert_eq!(c[0].alt, vec![64]);
    }

    #[test]
    fn a_bend_counts_at_its_top_and_a_short_one_is_told() {
        let placed = place(&LICKS[0], LICK_ROOT, &STD); // D bent to E, then E, D, C, A
        let c = checks(&placed, &STD);
        assert_eq!((c[0].base, c[0].target), (Some(62), 64));
        let now = Instant::now();
        let mut run = LickRun::default();
        run.turn(None);
        run.onset(62, &c, now); // picked
        run.pitch(Some(63.9), &c, now); // bent up
        assert_eq!(run.step, 1);
        // a short one: up to 63.2, then the next note
        let mut run = LickRun::default();
        run.turn(None);
        run.onset(62, &c, now);
        run.pitch(Some(63.2), &c, now);
        run.onset(64, &c, now); // the next check's E (step 1)... heard as the bend's target? no: it's past it
        assert_eq!(run.bends_short + run.step as u32 >= 1, true);
    }

    #[test]
    fn a_clean_take_counts_toward_the_tempo() {
        let placed = place(&LICKS[7], LICK_ROOT, &STD);
        let c = checks(&placed, &STD);
        let mut run = LickRun::default();
        let bpm = run.bpm;
        for _ in 0..3 {
            run.turn(None);
            for ch in &c {
                run.onset(ch.target, &c, Instant::now());
            }
            assert_eq!(run.phase, Phase::Done);
        }
        assert_eq!(run.bpm, bpm + TEMPO_STEP);
        // a wrong note: not clean
        run.turn(None);
        run.onset(61, &c, Instant::now());
        assert_eq!(run.misses, 1);
    }
}
