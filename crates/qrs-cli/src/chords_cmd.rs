//! `quickrs-cli chords`: a bench for chord scoring (new in quickRS, 2026-10-26, D46).
//!
//! Strummed chords are made on the synth's strings (Karplus-Strong, synth.rs `string_tone`) the way a person plays
//! them: the strings struck one after another over 10-60 ms (down and up strums), uneven levels, a little out of
//! tune, some strings missed, palm mutes, pick noise, a string struck again cutting its old note, a shape change
//! damping the fretted strings it lets go. Each case is played as a run of chords (with repeats and changes) and
//! scored live: the input's pluck detector (rt/onset.rs) finds the strums in 128-sample blocks and the Scorer
//! (detect.rs) is updated every 10 ms with only the audio recorded so far, as the app does. Positives should be
//! hits, negatives (a wrong shape, a single wrong note, a muted strum, the previous chord left ringing) misses.
//! Optionally through a pedalboard preset (an amp modeller's send) and on a real chart's chords (`--song`).
//! Nothing is written: the audio is made on the fly.

use std::path::PathBuf;

use qrs_analysis::detect::{self, Judgement, Live, Pluck, SampleSource, Scorer};
use qrs_audio::board;
use qrs_audio::rt::onset::OnsetDetector;
use qrs_audio::synth::{Rng, midi_hz, string_tone};
use qrs_formats::model::{Arrangement, Beat, Chord, Note, Techs};

#[derive(clap::Args)]
pub struct Args {
    /// a song whose chords to play too (psarc or feedpak; nothing is saved)
    #[arg(long)]
    pub song: Option<PathBuf>,
    /// its part (default: the first with the most chords)
    #[arg(long)]
    pub arr: Option<String>,
    /// chords per case
    #[arg(long, default_value_t = 160)]
    pub n: usize,
    #[arg(long, default_value_t = 1)]
    pub seed: u64,
    /// only the cases whose name contains this
    #[arg(long)]
    pub only: Option<String>,
    /// say why each missed positive was missed (and each accepted negative passed)
    #[arg(long)]
    pub why: bool,
    /// take the plucks from where the strums were played instead of the pluck detector (to tell the two apart)
    #[arg(long)]
    pub oracle: bool,
    /// write each case's take as a WAV here (for listening)
    #[arg(long)]
    pub wav: Option<PathBuf>,
    /// a real recording (a take's WAV, its .json beside it) scored live against --song's part, chord by chord,
    /// instead of the synth's cases
    #[arg(long)]
    pub take: Option<PathBuf>,
    /// with --take and no .json beside it: the chart time of the WAV's first sample
    #[arg(long)]
    pub start: Option<f64>,
}

const RATE: u32 = 48000;
const OPENS: [i32; 6] = [40, 45, 50, 55, 59, 64];

/// A chord shape: (string, fret), low string first.
type Shape = Vec<(i32, i32)>;

fn shape(frets: &str) -> Shape {
    // "x32010": low E first
    frets
        .chars()
        .enumerate()
        .filter_map(|(s, c)| c.to_digit(10).map(|f| (s as i32, f as i32)))
        .collect()
}

fn shape_wide(frets: &[i32]) -> Shape {
    frets
        .iter()
        .enumerate()
        .filter(|(_, f)| **f >= 0)
        .map(|(s, f)| (s as i32, *f))
        .collect()
}

fn open_chords() -> Vec<Shape> {
    [
        "x32010", "320003", "xx0232", "022000", "x02210", "022100", "x02220", "xx0231",
    ]
    .iter()
    .map(|s| shape(s))
    .collect()
}

fn barre_chords() -> Vec<Shape> {
    vec![
        shape_wide(&[1, 3, 3, 2, 1, 1]),
        shape_wide(&[-1, 2, 4, 4, 3, 2]),
        shape_wide(&[7, 9, 9, 7, 7, 7]),
        shape_wide(&[0, 7, 9, 9, 9, -1]),
        shape_wide(&[3, 5, 5, 4, 3, 3]),
        shape_wide(&[-1, 5, 7, 7, 7, 5]),
        shape_wide(&[5, 7, 7, 5, 5, 5]),
        shape_wide(&[-1, 3, 5, 5, 5, 3]),
    ]
}

fn power_chords() -> Vec<Shape> {
    vec![
        shape_wide(&[0, 2, 2, -1, -1, -1]),
        shape_wide(&[-1, 0, 2, 2, -1, -1]),
        shape_wide(&[3, 5, 5, -1, -1, -1]),
        shape_wide(&[-1, 5, 7, 7, -1, -1]),
        shape_wide(&[2, 4, -1, -1, -1, -1]),
        shape_wide(&[-1, 2, 4, -1, -1, -1]),
        shape_wide(&[5, 7, 7, -1, -1, -1]),
        shape_wide(&[-1, 7, 9, -1, -1, -1]),
    ]
}

fn midi_of(s: i32, f: i32) -> i32 {
    OPENS[s as usize] + f
}

/// A run of strums on the synth for the chord-name bench: each shape strummed twice (down, up; or `sloppy`, the
/// strings a hurried strum catches) every `gap` seconds. The take and each first strum's time and (string, fret)s.
pub(crate) fn strummed(
    shapes: &[Vec<i32>],
    gap: f64,
    sloppy: bool,
    seed: u64,
) -> (Vec<f32>, Vec<(f64, Shape)>) {
    let mut r = Rand(Rng::new(seed));
    let mut strums = Vec::new();
    let mut at = Vec::new();
    for (i, frets) in shapes.iter().enumerate() {
        let want = shape_wide(frets);
        let t = 1.0 + i as f64 * gap;
        for (k, dir) in [Dir::Down, Dir::Up].into_iter().enumerate() {
            let keep = if sloppy {
                partial_keep(want.len(), &mut r)
            } else {
                want.len()
            };
            let s = strum_of(&want, keep, t + k as f64 * gap / 2.0, dir, false, &mut r);
            if k == 0 {
                at.push((t, s.notes.clone()));
            }
            strums.push(s);
        }
        let _ = i;
    }
    let len = 1.0 + shapes.len() as f64 * gap + 1.0;
    (render(&strums, len, &mut r, false), at)
}

/// How one strum is played.
#[derive(Clone, Debug)]
struct Strum {
    /// When it starts (seconds in the take).
    at: f64,
    /// (string, fret) struck, in the order struck.
    notes: Vec<(i32, i32)>,
    /// Seconds from the first string to the last.
    spread: f64,
    palm: bool,
    /// A fret-hand muted "chk": strings damped, no pitch.
    dead: bool,
}

struct Rand(Rng);
impl Rand {
    fn u(&mut self) -> f64 {
        self.0.uniform()
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.u()
    }
    fn int(&mut self, n: usize) -> usize {
        ((self.u() * n as f64) as usize).min(n.saturating_sub(1))
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.int(v.len())]
    }
    fn chance(&mut self, p: f64) -> bool {
        self.u() < p
    }
    fn gauss(&mut self) -> f64 {
        let (a, b) = (self.u().max(1e-12), self.u());
        (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
    }
}

/// The take: every strum's strings, each cut by the next strike on its string or damped when the shape lets it go.
fn render(strums: &[Strum], len: f64, r: &mut Rand, let_ring: bool) -> Vec<f32> {
    let n = (len * RATE as f64) as usize;
    let mut x = vec![0.0f32; n];
    struct Ev {
        string: i32,
        at: f64,
        midi: f64,
        amp: f64,
        t60: f64,
        bright: f64,
        fretted: bool,
    }
    let mut evs: Vec<(usize, Ev)> = Vec::new();
    for (si, s) in strums.iter().enumerate() {
        let k = s.notes.len().max(1);
        let level = r.range(0.7, 1.0);
        for (j, &(string, fret)) in s.notes.iter().enumerate() {
            let at = s.at + s.spread * j as f64 / (k - 1).max(1) as f64 + r.range(-0.002, 0.002);
            // the strings struck first get more of the pick; then uneven by up to +-4 dB
            let lead = 1.0 + 0.25 * (1.0 - j as f64 / k as f64);
            let amp = level * lead * 10f64.powf(r.range(-4.0, 4.0) / 20.0);
            let detune = r.range(-15.0, 15.0) / 100.0;
            let (t60, bright) = if s.dead {
                (0.015, 0.2)
            } else if s.palm {
                (r.range(0.12, 0.3), r.range(0.15, 0.3))
            } else {
                (3.5 - 0.3 * string as f64, r.range(0.45, 0.75))
            };
            evs.push((
                si,
                Ev {
                    string,
                    at,
                    // a damped string has no clear pitch of its own: a thud somewhere along it
                    midi: if s.dead {
                        midi_of(string, r.int(13) as i32) as f64 + r.range(-0.5, 0.5)
                    } else {
                        midi_of(string, fret) as f64 + detune
                    },
                    amp: if s.dead { amp * 0.6 } else { amp },
                    t60,
                    bright,
                    fretted: fret > 0,
                },
            ));
        }
    }
    for (i, (si, e)) in evs.iter().enumerate() {
        // its end: the next strike on the string, or (fretted) the next strum that doesn't play this string
        let mut end = len;
        if let Some((_, nx)) = evs[i + 1..].iter().find(|(_, o)| o.string == e.string) {
            end = nx.at;
        }
        if !let_ring
            && e.fretted
            && let Some(ns) = strums[si + 1..]
                .iter()
                .find(|s| !s.notes.iter().any(|&(st, _)| st == e.string))
        {
            let lift = ns.at - 0.03;
            if lift > e.at && lift < end {
                end = lift + 0.05;
            }
        }
        let fade = if end < len { 0.006 } else { 0.0 };
        let secs = (end - e.at).clamp(0.01, 4.0);
        let tone = string_tone(midi_hz(e.midi), secs, RATE, None, e.t60, e.bright, &mut r.0);
        let start = (e.at * RATE as f64) as usize;
        let fade_n = (fade * RATE as f64) as usize;
        let total = tone.len();
        for (k, v) in tone.iter().enumerate() {
            let idx = start + k;
            if idx >= n {
                break;
            }
            let g = if fade_n > 0 && k + fade_n > total {
                (total - k) as f64 / fade_n as f64
            } else {
                1.0
            };
            x[idx] += (e.amp * 0.12 * v * g) as f32;
        }
        // the pick on the string: a short bright click
        let click = (0.003 * RATE as f64) as usize;
        let mut prev = 0.0;
        for k in 0..click {
            let idx = start + k;
            if idx >= n {
                break;
            }
            let w = r.range(-1.0, 1.0);
            x[idx] += (e.amp * 0.03 * (w - prev) * (1.0 - k as f64 / click as f64)) as f32;
            prev = w;
        }
    }
    // the input's noise floor (-70 dBFS) and a little mains hum
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f64 / RATE as f64;
        *v += (r.range(-1.0, 1.0) * 3e-4 + 2e-4 * (std::f64::consts::TAU * 50.0 * t).sin()) as f32;
    }
    x
}

/// One chord of a case: what the chart wants, what's played, whether it should count.
struct Item {
    t: f64,
    want: Shape,
    /// The strum (None: nothing played).
    play: Option<Strum>,
    /// Counted in the case's numbers (others set the scene).
    judged: bool,
    palm: bool,
}

struct Case {
    name: String,
    positive: bool,
    items: Vec<Item>,
    let_ring: bool,
    board: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Down,
    Up,
}

/// The strings a strum strikes: all of `want` (or `keep` of them: a downstrum misses top strings, an upstrum the
/// bottom ones, sometimes one in the middle), in strum order.
fn strike(want: &Shape, keep: usize, dir: Dir, r: &mut Rand) -> Vec<(i32, i32)> {
    let mut v = want.clone();
    let keep = keep.clamp(1, v.len());
    while v.len() > keep {
        let roll = r.u();
        if roll < 0.15 && v.len() > 2 {
            let i = 1 + r.int(v.len() - 2);
            v.remove(i);
        } else if (dir == Dir::Down) ^ (roll > 0.85) {
            v.pop();
        } else {
            v.remove(0);
        }
    }
    if dir == Dir::Up {
        v.reverse();
    }
    v
}

fn spread(r: &mut Rand) -> f64 {
    r.range(0.010, 0.060)
}

fn human(t: f64, r: &mut Rand) -> f64 {
    t + (r.gauss() * 0.015).clamp(-0.05, 0.05)
}

/// How many strings a sloppy strum of an n-note chord catches: at least half, at least 2.
fn partial_keep(n: usize, r: &mut Rand) -> usize {
    let lo = n.div_ceil(2).max(2).min(n);
    let hi = n.saturating_sub(1).max(lo);
    lo + r.int(hi - lo + 1)
}

/// A run of chords: `each` picks the shape (from the previous one) and builds the item.
fn run(
    r: &mut Rand,
    count: usize,
    gap: (f64, f64),
    mut each: impl FnMut(&mut Rand, usize, f64) -> Item,
) -> Vec<Item> {
    let mut t = 1.0;
    let mut v = Vec::new();
    for i in 0..count {
        v.push(each(r, i, t));
        t += r.range(gap.0, gap.1);
    }
    v
}

fn strum_of(want: &Shape, keep: usize, at: f64, dir: Dir, palm: bool, r: &mut Rand) -> Strum {
    Strum {
        at: human(at, r),
        notes: strike(want, keep, dir, r),
        spread: if palm { r.range(0.008, 0.025) } else { spread(r) },
        palm,
        dead: false,
    }
}

/// Chords to play: the bench's own sets and, with a song, its chord shapes in order.
struct Sets {
    open: Vec<Shape>,
    barre: Vec<Shape>,
    power: Vec<Shape>,
    all: Vec<Shape>,
}

fn positive_cases(r: &mut Rand, n: usize, sets: &Sets) -> Vec<Case> {
    let mut cases = Vec::new();
    let mk = |name: &str, items: Vec<Item>| Case {
        name: name.into(),
        positive: true,
        items,
        let_ring: false,
        board: None,
    };
    // full strums of changing chords, down and up
    for (label, set) in [
        ("open", &sets.open),
        ("barre", &sets.barre),
        ("power", &sets.power),
    ] {
        let items = run(r, n, (0.45, 0.9), |r, i, t| {
            let want = r.pick(set).clone();
            let dir = if i % 3 == 2 { Dir::Up } else { Dir::Down };
            let play = Some(strum_of(&want, want.len(), t, dir, false, r));
            Item {
                t,
                want,
                play,
                judged: true,
                palm: false,
            }
        });
        cases.push(mk(&format!("full {label}"), items));
    }
    // sloppy: half to all-but-one of the strings
    for (label, set) in [("open", &sets.open), ("barre", &sets.barre)] {
        let items = run(r, n, (0.45, 0.9), |r, i, t| {
            let want = r.pick(set).clone();
            let dir = if i % 3 == 2 { Dir::Up } else { Dir::Down };
            let keep = partial_keep(want.len(), r);
            let play = Some(strum_of(&want, keep, t, dir, false, r));
            Item {
                t,
                want,
                play,
                judged: true,
                palm: false,
            }
        });
        cases.push(mk(&format!("partial {label}"), items));
    }
    // 4 of 6 exactly (barre chords)
    let six: Vec<Shape> = sets.all.iter().filter(|s| s.len() == 6).cloned().collect();
    let items = run(r, n, (0.45, 0.9), |r, i, t| {
        let want = r.pick(&six).clone();
        let dir = if i % 2 == 1 { Dir::Up } else { Dir::Down };
        let play = Some(strum_of(&want, 4, t, dir, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("4 of 6", items));
    // rhythm: eighths at ~160 bpm, two bars a chord, upstrums catching the top 2-4 strings
    let items = run(r, n, (0.18, 0.2), |r, i, t| {
        let bar = i / 8;
        let want = sets.all[(bar * 7 + 3) % sets.all.len()].clone();
        let dir = if i % 2 == 1 { Dir::Up } else { Dir::Down };
        let keep = if dir == Dir::Up {
            (2 + r.int(3)).min(want.len())
        } else {
            partial_keep(want.len(), r).max(want.len().saturating_sub(1))
        };
        let play = Some(strum_of(&want, keep, t, dir, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("eighths", items));
    // palm-muted power chords, eighths
    let items = run(r, n, (0.18, 0.24), |r, i, t| {
        let want = sets.power[(i / 8 * 3) % sets.power.len()].clone();
        let play = Some(strum_of(&want, want.len(), t, Dir::Down, true, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: true,
        }
    });
    cases.push(mk("palm-muted power", items));
    // single notes (to check nothing changed for them): a riff, sometimes the same note again
    let mut last = (0, 5);
    let items = run(r, n, (0.1, 0.3), |r, _, t| {
        if !r.chance(0.3) {
            last = (r.int(6) as i32, r.int(13) as i32);
        }
        let want = vec![last];
        let play = Some(strum_of(&want, 1, t, Dir::Down, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("single notes", items));
    cases
}

fn negative_cases(r: &mut Rand, n: usize, sets: &Sets) -> Vec<Case> {
    let mut cases = Vec::new();
    let mk = |name: &str, items: Vec<Item>, let_ring: bool| Case {
        name: name.into(),
        positive: false,
        items,
        let_ring,
        board: None,
    };
    let in_reach = |s: &Shape| {
        s.iter()
            .all(|&(st, f)| (0..6).contains(&st) && (0..=22).contains(&f))
    };
    // single notes a fret off (to check nothing changed for them)
    let items = run(r, n, (0.15, 0.35), |r, _, t| {
        let want = vec![(r.int(6) as i32, 1 + r.int(12) as i32)];
        let wrong = vec![(want[0].0, want[0].1 + *r.pick(&[-1, 1]))];
        let play = Some(strum_of(&wrong, 1, t, Dir::Down, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("single notes a fret off", items, false));
    // the shape a fret or two off (every string)
    let items = run(r, n, (0.5, 0.9), |r, _, t| {
        let want = r.pick(&sets.all).clone();
        let mut d = *r.pick(&[-2, -1, 1, 2]);
        let mut wrong: Shape = want.iter().map(|&(s, f)| (s, f + d)).collect();
        if !in_reach(&wrong) {
            d = -d;
            wrong = want.iter().map(|&(s, f)| (s, f + d)).collect();
        }
        let play = Some(strum_of(&wrong, wrong.len(), t, Dir::Down, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("shape a fret off", with_context(items, sets, r), false));
    // the shape a string over (same frets)
    let items = run(r, n, (0.5, 0.9), |r, _, t| {
        let want = r.pick(&sets.all).clone();
        let lo = want.iter().map(|p| p.0).min().unwrap_or(0);
        let hi = want.iter().map(|p| p.0).max().unwrap_or(5);
        let d = if hi < 5 && (lo == 0 || r.chance(0.5)) {
            1
        } else {
            -1
        };
        let wrong: Shape = want
            .iter()
            .map(|&(s, f)| (s + d, f))
            .filter(|p| (0..6).contains(&p.0))
            .collect();
        let play = Some(strum_of(&wrong, wrong.len(), t, Dir::Down, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("shape a string over", with_context(items, sets, r), false));
    // another chord of the set (sharing at most half its pitch classes)
    let items = run(r, n, (0.5, 0.9), |r, _, t| {
        let want = r.pick(&sets.all).clone();
        let pcs = |s: &Shape| {
            s.iter()
                .map(|&(a, b)| midi_of(a, b) % 12)
                .collect::<std::collections::BTreeSet<_>>()
        };
        let pw = pcs(&want);
        let mut wrong = want.clone();
        for _ in 0..50 {
            let c = r.pick(&sets.all).clone();
            let shared = pcs(&c).intersection(&pw).count();
            if shared * 2 <= pw.len() {
                wrong = c;
                break;
            }
        }
        let play = Some(strum_of(&wrong, wrong.len(), t, Dir::Down, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("another chord", with_context(items, sets, r), false));
    // a single note that isn't in the chord
    let items = run(r, n, (0.5, 0.9), |r, _, t| {
        let want = r.pick(&sets.all).clone();
        let pcs: Vec<i32> = want.iter().map(|&(a, b)| midi_of(a, b) % 12).collect();
        let mut note = (0, 1);
        for _ in 0..50 {
            let c = (r.int(6) as i32, r.int(12) as i32);
            if !pcs.contains(&(midi_of(c.0, c.1) % 12)) {
                note = c;
                break;
            }
        }
        let play = Some(Strum {
            at: human(t, r),
            notes: vec![note],
            spread: 0.0,
            palm: false,
            dead: false,
        });
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("a single wrong note", with_context(items, sets, r), false));
    // a fret-hand muted strum (a "chk", no pitch)
    let items = run(r, n, (0.5, 0.9), |r, _, t| {
        let want = r.pick(&sets.all).clone();
        let mut s = strum_of(&want, want.len(), t, Dir::Down, false, r);
        s.dead = true;
        Item {
            t,
            want,
            play: Some(s),
            judged: true,
            palm: false,
        }
    });
    cases.push(mk("muted strum", with_context(items, sets, r), false));
    // the previous chord left ringing, the next not played
    let items = run(r, n * 2, (0.45, 0.8), |r, i, t| {
        let want = r.pick(&sets.all).clone();
        if i % 2 == 0 {
            let play = Some(strum_of(&want, want.len(), t, Dir::Down, false, r));
            Item {
                t,
                want,
                play,
                judged: false,
                palm: false,
            }
        } else {
            Item {
                t,
                want,
                play: None,
                judged: true,
                palm: false,
            }
        }
    });
    let items = distinct_neighbours(items, sets, r);
    cases.push(mk("previous ringing, next silent", items, true));
    // rhythm with gaps: the same chord due again (eighths) but a strum left out, the last one still ringing
    let items = run(r, n * 3, (0.18, 0.2), |r, i, t| {
        let want = sets.all[(i / 12 * 5 + 1) % sets.all.len()].clone();
        if i % 3 == 2 {
            Item {
                t,
                want,
                play: None,
                judged: true,
                palm: false,
            }
        } else {
            let dir = if i % 3 == 1 { Dir::Up } else { Dir::Down };
            let play = Some(strum_of(&want, want.len(), t, dir, false, r));
            Item {
                t,
                want,
                play,
                judged: false,
                palm: false,
            }
        }
    });
    cases.push(mk("repeat left out (ringing)", items, true));
    // the previous chord strummed again when the next (a different one) is due
    let mut items = run(r, n * 2, (0.45, 0.8), |r, i, t| {
        let want = r.pick(&sets.all).clone();
        let play = Some(strum_of(&want, want.len(), t, Dir::Down, false, r));
        Item {
            t,
            want,
            play,
            judged: i % 2 == 1,
            palm: false,
        }
    });
    items = distinct_neighbours(items, sets, r);
    for i in (1..items.len()).step_by(2) {
        let prev = items[i - 1].want.clone();
        if let Some(p) = items[i].play.as_mut() {
            p.notes = strike(&prev, prev.len(), Dir::Down, r);
        }
    }
    cases.push(mk("previous chord again", items, false));
    // informative: one string a fret off (Rocksmith lets that pass; so may we)
    let items = run(r, n, (0.5, 0.9), |r, _, t| {
        let want = r.pick(&sets.all).clone();
        let mut wrong = want.clone();
        let i = r.int(wrong.len());
        wrong[i].1 = if wrong[i].1 == 0 {
            1
        } else {
            wrong[i].1 + *r.pick(&[-1, 1])
        };
        let play = Some(strum_of(&wrong, wrong.len(), t, Dir::Down, false, r));
        Item {
            t,
            want,
            play,
            judged: true,
            palm: false,
        }
    });
    cases.push(mk(
        "(info) one string a fret off",
        with_context(items, sets, r),
        false,
    ));
    cases
}

/// Each wrong strum after a right one (a chord of the set played as charted, 0.3 s before): a player who fumbles
/// one chord of a run, the right one still ringing.
fn with_context(items: Vec<Item>, sets: &Sets, r: &mut Rand) -> Vec<Item> {
    let mut out = Vec::new();
    let mut shift = 0.0;
    for mut it in items {
        shift += 0.3;
        it.t += shift;
        if let Some(p) = it.play.as_mut() {
            p.at += shift;
        }
        let want = r.pick(&sets.all).clone();
        let t = it.t - 0.3;
        let play = Some(strum_of(&want, want.len(), t, Dir::Down, false, r));
        out.push(Item {
            t,
            want,
            play,
            judged: false,
            palm: false,
        });
        out.push(it);
    }
    out
}

/// Each judged chord made different from the one before it (sharing at most half its pitch classes).
fn distinct_neighbours(mut items: Vec<Item>, sets: &Sets, r: &mut Rand) -> Vec<Item> {
    let pcs = |s: &Shape| {
        s.iter()
            .map(|&(a, b)| midi_of(a, b) % 12)
            .collect::<std::collections::BTreeSet<_>>()
    };
    for i in 1..items.len() {
        if !items[i].judged {
            continue;
        }
        let p = pcs(&items[i - 1].want);
        for _ in 0..60 {
            let c = r.pick(&sets.all).clone();
            let pc = pcs(&c);
            if pc.intersection(&p).count() * 2 <= pc.len().min(p.len()) {
                let want = items[i].want.clone();
                if let Some(s) = items[i].play.as_mut()
                    && s.notes.iter().all(|n| want.contains(n))
                {
                    s.notes = strike(&c, c.len(), Dir::Down, r);
                }
                items[i].want = c;
                break;
            }
        }
    }
    items
}

/// The chart of a case: each item's chord (palm-muted ones flagged).
fn chart(items: &[Item]) -> Arrangement {
    let mut a = Arrangement::new("Rhythm");
    for it in items {
        let tech = if it.palm { Techs::PALM_MUTE } else { Techs::NONE };
        let mut frets = vec![-1; 6];
        for &(s, f) in &it.want {
            frets[s as usize] = f;
        }
        a.chords.push(Chord {
            time: it.t,
            chord_id: 0,
            name: String::new(),
            notes: it
                .want
                .iter()
                .map(|&(s, f)| Note {
                    techniques: tech,
                    ..Note::new(it.t, s, f)
                })
                .collect(),
            techniques: tech,
            sustain: 0.0,
            frets,
            fingers: vec![],
            high_density: false,
        });
    }
    let end = items.last().map(|i| i.t + 2.0).unwrap_or(2.0);
    a.beats = (0..(end * 2.0) as usize)
        .map(|i| Beat {
            time: i as f64 * 0.5,
            measure: if i % 4 == 0 { 1 } else { -1 },
        })
        .collect();
    a
}

/// The recording up to `now` (what the capture ring holds at that moment).
struct Rec<'a> {
    x: &'a [f32],
    now: usize,
}
impl SampleSource for Rec<'_> {
    fn rate(&self) -> u32 {
        RATE
    }
    fn written(&self) -> u64 {
        self.now as u64
    }
    fn read(&self, start: u64, out: &mut [f32]) -> bool {
        let s = start as usize;
        if s + out.len() > self.now {
            return false;
        }
        out.copy_from_slice(&self.x[s..s + out.len()]);
        true
    }
    fn latest(&self, out: &mut [f32]) -> bool {
        if out.len() > self.now {
            return false;
        }
        out.copy_from_slice(&self.x[self.now - out.len()..self.now]);
        true
    }
}

/// Score a take live: the pluck detector in 128-sample blocks, the scorer every 10 ms. Returns each group's
/// judgement and the plucks found.
fn score(arr: &Arrangement, x: &[f32], oracle: Option<Vec<u64>>) -> (Vec<Option<Judgement>>, Vec<u64>) {
    let mut plucks = Vec::new();
    if let Some(o) = oracle {
        plucks = o;
    } else {
        let mut det = OnsetDetector::new(RATE);
        for (k, c) in x.chunks(128).enumerate() {
            det.feed(c, (k * 128) as u64, |i, _| plucks.push(i));
        }
    }
    let mut s = Scorer::new(arr);
    let step = (0.01 * RATE as f64) as usize;
    let mut fed = 0;
    let mut now = 0;
    while now < x.len() {
        now = (now + step).min(x.len());
        let new: Vec<Pluck> = plucks
            .iter()
            .skip(fed)
            .take_while(|&&i| (i as usize) + 256 <= now)
            .map(|&i| Pluck {
                index: i,
                song_time: i as f64 / RATE as f64,
            })
            .collect();
        fed += new.len();
        s.feed(new);
        let rec = Rec { x, now };
        let live = Live {
            source: &rec,
            level: 0.05,
            latency: 0.0,
        };
        let t = now as f64 / RATE as f64;
        s.update(t, Some(&live), t);
    }
    (s.groups.iter().map(|g| g.state).collect(), plucks)
}

/// Why a chord wasn't heard (or was): the scorer's tests at the best pluck in its window.
fn why(arr: &Arrangement, x: &[f32], plucks: &[u64], gi: usize) -> String {
    let groups = detect::build_groups(arr);
    let g = &groups[gi];
    let mut sp = qrs_analysis::spectrum::Spectra::new();
    let n = detect::window_for(g, RATE);
    let skip = (detect::ATTACK_SKIP * RATE as f64) as usize;
    let mut best = String::from("no pluck");
    let mut best_score = -1.0;
    let near: Vec<String> = plucks
        .iter()
        .map(|&p| (p as f64 / RATE as f64 - g.t) * 1000.0)
        .filter(|d| d.abs() < 400.0)
        .map(|d| format!("{d:+.0}"))
        .collect();
    for &p in plucks {
        let st = p as f64 / RATE as f64;
        if st < g.t - detect::EARLY || st > g.t + detect::LATE {
            continue;
        }
        let p = p as usize;
        if p + skip + n > x.len() || p < n {
            continue;
        }
        let (mag, binhz) = sp.spectrum(&x[p + skip..p + skip + n], RATE);
        let prev = sp.spectrum(&x[p - n..p], RATE).0;
        let v = detect::explain(&mag, binhz, g, Some(&prev));
        let sc = v.present as f64 - v.foreign;
        if sc > best_score {
            best_score = sc;
            best = format!("dt {:+.0} ms: {}", (st - g.t) * 1000.0, v.describe());
        }
    }
    format!("{best} (plucks at {})", near.join(" "))
}

#[derive(Default, Clone)]
struct Tally {
    judged: u32,
    counted: u32,
}

fn run_case(case: &Case, verbose: bool, wav: Option<&std::path::Path>, oracle: bool) -> Tally {
    let arr = chart(&case.items);
    let strums: Vec<Strum> = case.items.iter().filter_map(|i| i.play.clone()).collect();
    let len = case.items.last().map(|i| i.t + 1.5).unwrap_or(2.0);
    let mut r = Rand(Rng::new(case.name.len() as u64 * 7919 + 11));
    let mut x = render(&strums, len, &mut r, case.let_ring);
    if let Some(name) = case.board {
        let b = board::presets()
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, b)| b)
            .expect("preset");
        x = board::render(&b, RATE, &x);
    }
    if let Some(dir) = wav {
        let name: String = case
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let _ = std::fs::create_dir_all(dir);
        let _ = qrs_analysis::take::write_wav_mono(&dir.join(format!("{name}.wav")), RATE, &x);
    }
    let oracle = oracle.then(|| {
        strums
            .iter()
            .map(|s| (s.at.max(0.0) * RATE as f64) as u64)
            .collect()
    });
    let (states, plucks) = score(&arr, &x, oracle);
    let groups = detect::build_groups(&arr);
    let mut t = Tally::default();
    for (gi, g) in groups.iter().enumerate() {
        let Some(item) = case.items.iter().find(|i| (i.t - g.t).abs() < 1e-6) else {
            continue;
        };
        if !item.judged {
            continue;
        }
        t.judged += 1;
        let counted = !matches!(states[gi], Some(Judgement::Miss) | None);
        if counted {
            t.counted += 1;
        }
        if verbose && counted != case.positive {
            let played = item
                .play
                .as_ref()
                .map(|s| {
                    format!(
                        "{:?}",
                        s.notes.iter().map(|&(a, b)| midi_of(a, b)).collect::<Vec<_>>()
                    )
                })
                .unwrap_or_else(|| "-".into());
            println!(
                "    {} t {:.2} want {:?} played {}: {}",
                if counted { "ACCEPTED" } else { "missed" },
                g.t,
                g.midis,
                played,
                why(&arr, &x, &plucks, gi)
            );
        }
    }
    t
}

/// A real chart's chords played (clean or sloppy), in its own timing.
fn song_cases(arr: &Arrangement, r: &mut Rand, limit: usize) -> Vec<Case> {
    let chords: Vec<&Chord> = arr
        .chords
        .iter()
        .filter(|c| c.notes.len() >= 2)
        .take(limit)
        .collect();
    let mut cases = Vec::new();
    for (label, sloppy) in [("full", false), ("sloppy", true)] {
        let mut items = Vec::new();
        let mut last_t = -10.0;
        let mut up = false;
        for c in &chords {
            let want: Shape = c.notes.iter().map(|n| (n.string, n.fret)).collect();
            let gap = c.time - last_t;
            up = gap < 0.26 && !up;
            last_t = c.time;
            let dir = if up { Dir::Up } else { Dir::Down };
            let keep = if !sloppy {
                want.len()
            } else if up {
                (2 + r.int(3)).min(want.len())
            } else {
                partial_keep(want.len(), r)
            };
            let palm = c.techniques.has(Techs::PALM_MUTE) || c.notes.iter().any(|n| n.has(Techs::PALM_MUTE));
            let play = Some(strum_of(&want, keep, c.time, dir, palm, r));
            items.push(Item {
                t: c.time,
                want,
                play,
                judged: true,
                palm,
            });
        }
        // the chart's time from a second before its first chord
        let t0 = items.first().map(|i| i.t - 1.0).unwrap_or(0.0);
        for it in &mut items {
            it.t -= t0;
            if let Some(p) = it.play.as_mut() {
                p.at -= t0;
            }
        }
        cases.push(Case {
            name: format!("song {label}"),
            positive: true,
            items,
            let_ring: false,
            board: None,
        });
    }
    cases
}

pub fn run_bench(song: Option<Arrangement>, args: &Args) -> anyhow::Result<()> {
    if let Some(take) = &args.take {
        let Some(arr) = song else {
            anyhow::bail!("--take needs --song (and --arr: the part played)");
        };
        return run_take(&arr, take, args);
    }
    let mut r = Rand(Rng::new(args.seed));
    let mut sets = Sets {
        open: open_chords(),
        barre: barre_chords(),
        power: power_chords(),
        all: vec![],
    };
    let mut song_shapes = Vec::new();
    if let Some(a) = &song {
        let mut seen = std::collections::BTreeSet::new();
        for c in &a.chords {
            let s: Shape = c.notes.iter().map(|n| (n.string, n.fret)).collect();
            if s.len() >= 2 && seen.insert(s.clone()) {
                song_shapes.push(s);
            }
        }
        println!(
            "song: {} chords, {} shapes: {:?}",
            a.chords.len(),
            song_shapes.len(),
            song_shapes
        );
    }
    sets.all = [&sets.open[..], &sets.barre[..], &sets.power[..], &song_shapes[..]].concat();
    let mut cases = positive_cases(&mut r, args.n, &sets);
    if let Some(a) = &song {
        cases.extend(song_cases(a, &mut r, args.n * 2));
    }
    let neg = negative_cases(&mut r, args.n, &sets);
    let mut distorted = Vec::new();
    for preset in ["Crunch", "Metal"] {
        for c in cases.iter().chain(&neg) {
            if matches!(
                c.name.as_str(),
                "full barre"
                    | "partial open"
                    | "eighths"
                    | "song sloppy"
                    | "shape a fret off"
                    | "another chord"
                    | "a single wrong note"
            ) {
                distorted.push(Case {
                    name: format!("{} [{preset}]", c.name),
                    positive: c.positive,
                    items: c
                        .items
                        .iter()
                        .map(|i| Item {
                            t: i.t,
                            want: i.want.clone(),
                            play: i.play.clone(),
                            judged: i.judged,
                            palm: i.palm,
                        })
                        .collect(),
                    let_ring: c.let_ring,
                    board: Some(preset),
                });
            }
        }
    }
    cases.extend(neg);
    cases.extend(distorted);
    println!("{:<40} {:>6} {:>8}", "case", "chords", "counted");
    for case in &cases {
        if args.only.as_deref().is_some_and(|o| !case.name.contains(o)) {
            continue;
        }
        let t = run_case(case, args.why, args.wav.as_deref(), args.oracle);
        let pct = 100.0 * t.counted as f64 / t.judged.max(1) as f64;
        println!(
            "{:<40} {:>6} {:>7.1}%  {}",
            case.name,
            t.judged,
            pct,
            if case.positive {
                "hit rate"
            } else {
                "false accepts"
            }
        );
    }
    Ok(())
}

/// A real take scored the way the app scores it live, on the chart's own times. Twice: with the plucks the input's
/// detector finds, and with a pluck put at every chart group's time (as if every strum were found right on time), so
/// a miss can be told apart as "the strum wasn't heard" or "the chord test said no".
fn run_take(arr: &Arrangement, path: &std::path::Path, args: &Args) -> anyhow::Result<()> {
    let (x, rate) = crate::take_cmd::load_mono(path)?;
    let x = if rate == RATE { x } else { resample(&x, rate, RATE) };
    let side: Option<serde_json::Value> = std::fs::read_to_string(path.with_extension("json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let timing = side
        .as_ref()
        .and_then(|v| v.get("timing"))
        .and_then(qrs_analysis::take::TakeTiming::from_json);
    let (t0, speed) = match (&timing, args.start) {
        (_, Some(s)) => (s, 1.0),
        (Some(t), None) => (t.chart_time(0.0), t.speed),
        (None, None) => {
            anyhow::bail!("no .json beside the take: give --start (the chart time of its first sample)")
        }
    };
    if (speed - 1.0).abs() > 0.01 {
        anyhow::bail!(
            "this take was played at {:.0}% speed: record it at 100%",
            speed * 100.0
        );
    }
    // sample i is chart time i / RATE: the take put at its chart time (silence before, or its head cut)
    let lead = (t0 * RATE as f64).round() as i64;
    let mut y = vec![0.0f32; lead.max(0) as usize];
    y.extend_from_slice(&x[((-lead).max(0) as usize).min(x.len())..]);
    let (from, to) = (t0.max(0.0), y.len() as f64 / RATE as f64);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!(
        "take: {:.1} s, chart {from:.2}-{to:.2} s, peak {:.0} dBFS{}",
        x.len() as f64 / RATE as f64,
        20.0 * peak.max(1e-9).log10(),
        if peak > 0.98 { " (clipping)" } else { "" }
    );
    let groups = detect::build_groups(arr);
    let inside: Vec<usize> = (0..groups.len())
        .filter(|&i| groups[i].t >= from + 0.2 && groups[i].t <= to - 0.3)
        .collect();
    let (live, plucks) = score(arr, &y, None);
    let oracle: Vec<u64> = inside
        .iter()
        .map(|&i| (groups[i].t * RATE as f64) as u64)
        .collect();
    let (ideal, _) = score(arr, &y, Some(oracle));
    let counted = |st: &Option<Judgement>| !matches!(st, Some(Judgement::Miss) | None);
    let name = |t: f64| {
        arr.chords
            .iter()
            .find(|c| (c.time - t).abs() < 0.002)
            .map(|c| c.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "?".into())
    };
    for (label, chords) in [("chords", true), ("single notes", false)] {
        let ids: Vec<usize> = inside
            .iter()
            .copied()
            .filter(|&i| (groups[i].midis.len() >= 2) == chords)
            .collect();
        if ids.is_empty() {
            continue;
        }
        let n = ids.len() as f64;
        let a = ids.iter().filter(|&&i| counted(&live[i])).count();
        let b = ids.iter().filter(|&&i| counted(&ideal[i])).count();
        println!(
            "{label}: {} | live {a} ({:.0}%) | every strum found on time {b} ({:.0}%)",
            ids.len(),
            100.0 * a as f64 / n,
            100.0 * b as f64 / n
        );
    }
    let near = inside
        .iter()
        .filter(|&&i| {
            plucks
                .iter()
                .any(|&p| (p as f64 / RATE as f64 - groups[i].t).abs() < 0.1)
        })
        .count();
    println!(
        "plucks found: {} ({near} of {} chart groups have one within 100 ms)",
        plucks.iter().filter(|&&p| p as f64 / RATE as f64 >= from).count(),
        inside.len()
    );
    if args.why {
        for &i in &inside {
            if counted(&live[i]) {
                continue;
            }
            let g = &groups[i];
            println!(
                "  missed {:>7.2} {:<8} {:?} {}: {}",
                g.t,
                if g.midis.len() >= 2 {
                    name(g.t)
                } else {
                    "note".into()
                },
                g.midis,
                if counted(&ideal[i]) {
                    "(heard if found on time)"
                } else {
                    "(chord test says no)"
                },
                why(arr, &y, &plucks, i)
            );
        }
    }
    Ok(())
}

/// Linear resampling (enough for the scorer's spectra).
fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    let n = (x.len() as f64 * to as f64 / from as f64) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * from as f64 / to as f64;
            let k = p as usize;
            let f = (p - k as f64) as f32;
            let a = x.get(k).copied().unwrap_or(0.0);
            let b = x.get(k + 1).copied().unwrap_or(a);
            a + (b - a) * f
        })
        .collect()
}
