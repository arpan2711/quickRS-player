//! Chord names from the sound alone (new in quickRS, P24; no Python source): the notes sounding in a frame of the
//! guitar input, the chord they make, and a shape on the neck that plays them, for the note widget's chord strip.
//! No chart is involved: this is a guess, made the way chord recognisers do it (Klapuri's iterative multi-pitch
//! estimation, a chord dictionary scored like Chordino's).
//!
//! - `notes`: the spectrum's peaks (170 ms, Hann, zero-padded 4x, parabolic interpolation), their amplitudes
//!   compressed (square root), the whole guitar's tuning offset from them; then one note at a time, the candidate
//!   (the lowest string to fret 24 of the high string) whose harmonics sum highest, with its fundamental present,
//!   its harmonics taken out of the peaks (by their smoothed level, so a partial another note shares keeps the rest),
//!   until the next is under a fifth of the first. Octave doublings may fall out; they don't change the chord.
//! - `name`: every root and quality scored: the strengths of the notes in the chord, less those outside it, less
//!   each chord tone missing (the fifth hardly counts), less the quality's complexity; the bass note as the root
//!   wins ties (C6 or Am7), any other bass makes a slash chord.
//! - `Listener`: frames 20 times a second; a chord is shown once two of the last three frames name it, kept while the
//!   guitar is quiet or plays single notes.
//! - `shape`: the strings and frets (a 4-fret hand, fingers or a barre, open strings) that play the chord's tones
//!   with the bass lowest, matching the notes heard, near the last shape.

use std::collections::VecDeque;

use crate::keyguess::{Key, Mode};

use crate::spectrum::Spectra;

pub use qrs_formats::model::NOTE_NAMES;
/// The frame (seconds), and the frequency range read.
pub const FRAME: f64 = 0.17;
const F_LO: f64 = 55.0;
const F_HI: f64 = 5000.0;
/// The highest note looked for: fret 24 of the high E string.
const TOP_MIDI: i32 = 88;
/// A note's next note must reach this share of the first's salience.
const NEXT_SHARE: f64 = 0.2;
const MAX_NOTES: usize = 6;
/// The level (RMS) under which a frame is quiet.
const QUIET: f64 = 0.002;

/// A note heard in a frame: its MIDI number and its salience (the first, strongest note is 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeardNote {
    pub midi: i32,
    pub strength: f64,
}

#[derive(Clone, Copy, Debug)]
struct Peak {
    hz: f64,
    amp: f64,
}

/// The spectrum's peaks (sorted by frequency): local maxima within ±9 Hz, over -44 dB of the loudest and clear of
/// the Hann window's sidelobes, their frequency and level interpolated on the log spectrum.
fn peaks(mag: &[f64], binhz: f64) -> Vec<Peak> {
    let lo = ((F_LO / binhz) as usize).max(2);
    let hi = ((F_HI / binhz) as usize).min(mag.len().saturating_sub(3));
    if hi <= lo {
        return vec![];
    }
    let top = mag[lo..hi].iter().fold(0.0f64, |m, &v| m.max(v));
    if top <= 0.0 {
        return vec![];
    }
    let near = ((9.0 / binhz).round() as usize).max(2);
    let wide = ((24.0 / binhz).round() as usize).max(near + 1);
    let mut out = Vec::new();
    for i in lo..hi {
        let m = mag[i];
        if m < top * 0.006 || m <= mag[i - 1] || m < mag[i + 1] {
            continue;
        }
        let (a0, a1) = (i.saturating_sub(near), (i + near).min(mag.len() - 1));
        if mag[a0..=a1].iter().any(|&v| v > m) {
            continue;
        }
        let (b0, b1) = (i.saturating_sub(wide), (i + wide).min(mag.len() - 1));
        if mag[b0..=b1].iter().fold(0.0f64, |x, &v| x.max(v)) * 0.05 > m {
            continue; // a sidelobe of something much louder
        }
        let (a, b, c) = (mag[i - 1].max(1e-12).ln(), m.ln(), mag[i + 1].max(1e-12).ln());
        let d = a - 2.0 * b + c;
        let p = if d.abs() > 1e-12 {
            (0.5 * (a - c) / d).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        out.push(Peak {
            hz: (i as f64 + p) * binhz,
            amp: ((b - 0.25 * (a - c) * p).exp() / top).sqrt(),
        });
    }
    out
}

/// The tuning's offset (cents, -50 to 50) from 440 Hz: the circular mean of the strong peaks' deviations from
/// the nearest semitone (those under 1.5 kHz, where strings are near harmonic).
fn tuning(peaks: &[Peak]) -> f64 {
    let (mut s, mut c) = (0.0, 0.0);
    for p in peaks.iter().filter(|p| p.hz < 1500.0 && p.amp > 0.2) {
        let cents = 1200.0 * (p.hz / 440.0).log2();
        let a = cents / 100.0 * std::f64::consts::TAU;
        s += p.amp * a.sin();
        c += p.amp * a.cos();
    }
    if s == 0.0 && c == 0.0 {
        0.0
    } else {
        s.atan2(c) / std::f64::consts::TAU * 100.0
    }
}

/// A candidate's harmonics in the peaks: (salience, the peak each harmonic took, the fundamental's level and the
/// strongest harmonic's level). Klapuri's weights (f0 + 52) / (h f0 + 320); the window widens with the harmonic
/// number, as strings run sharp up there.
fn harmonics(peaks: &[Peak], amps: &[f64], f0: f64) -> (f64, Vec<(usize, usize)>, f64, f64) {
    let mut sal = 0.0;
    let mut took = Vec::new();
    let (mut first, mut most) = (0.0f64, 0.0f64);
    for h in 1..=60usize {
        let fh = f0 * h as f64;
        if fh > F_HI {
            break;
        }
        let tol = 2f64.powf((25.0 + 3.0 * h as f64).min(60.0) / 1200.0);
        let (lo, hi) = (fh / tol, fh * tol);
        let start = peaks.partition_point(|p| p.hz < lo);
        let best = (start..peaks.len())
            .take_while(|&j| peaks[j].hz <= hi)
            .max_by(|&a, &b| amps[a].total_cmp(&amps[b]));
        if let Some(j) = best
            && amps[j] > 0.0
        {
            sal += (f0 + 52.0) / (fh + 320.0) * amps[j];
            took.push((h, j));
            if h == 1 {
                first = amps[j];
            }
            most = most.max(amps[j]);
        }
    }
    (sal, took, first, most)
}

/// The notes sounding in `x` (best about `FRAME` long), strongest first; `lowest` is the lowest open string's MIDI.
pub fn notes(sp: &mut Spectra, x: &[f32], rate: u32, lowest: i32) -> Vec<HeardNote> {
    let (mag, binhz) = sp.spectrum(x, rate);
    let pk = peaks(&mag, binhz);
    if pk.is_empty() {
        return vec![];
    }
    let tune = tuning(&pk);
    let mut amps: Vec<f64> = pk.iter().map(|p| p.amp).collect();
    let mut used = vec![false; pk.len()];
    let mut out: Vec<HeardNote> = Vec::new();
    let mut first = 0.0;
    while out.len() < MAX_NOTES {
        let mut best: Option<(f64, i32, Vec<(usize, usize)>)> = None;
        for m in lowest.min(TOP_MIDI)..=TOP_MIDI {
            if out.iter().any(|n| n.midi == m) {
                continue;
            }
            let f0 = 440.0 * 2f64.powf((m as f64 - 69.0) / 12.0 + tune / 1200.0);
            let (sal, took, h1, most) = harmonics(&pk, &amps, f0);
            // the fundamental must be there (else it's an octave or fifth below the real note)
            if h1 < 0.25 * most || took.len() < 2 {
                continue;
            }
            // and most of it must be its own: a note made of what's left of others' harmonics (the third
            // harmonic's fifth above, an octave) isn't one
            let own: f64 = took
                .iter()
                .filter(|&&(_, j)| !used[j])
                .map(|&(h, j)| (f0 + 52.0) / (f0 * h as f64 + 320.0) * amps[j])
                .sum();
            if own < 0.4 * sal {
                continue;
            }
            // and its odd harmonics there too: with only the even ones, it's the note an octave up
            let (mut odd, mut even) = (0.0, 0.0);
            for &(h, j) in took.iter().filter(|t| t.0 <= 8) {
                if h % 2 == 1 {
                    odd += amps[j]
                } else {
                    even += amps[j]
                }
            }
            if odd < 0.35 * even {
                continue;
            }
            if best.as_ref().is_none_or(|b| sal > b.0) {
                best = Some((sal, m, took));
            }
        }
        let Some((sal, m, took)) = best else { break };
        if out.is_empty() {
            first = sal;
        } else if sal < NEXT_SHARE * first {
            break;
        }
        out.push(HeardNote {
            midi: m,
            strength: sal / first,
        });
        // take its harmonics out by their smoothed level (what's left belongs to another note)
        let level: Vec<f64> = took.iter().map(|&(_, j)| amps[j]).collect();
        for (k, &(_, j)) in took.iter().enumerate() {
            let around = &level[k.saturating_sub(1)..(k + 2).min(level.len())];
            let smooth = around.iter().sum::<f64>() / around.len() as f64;
            amps[j] -= amps[j].min(smooth);
            used[j] = true;
        }
    }
    out
}

/// A chord quality: its name after the root, its tones (semitones over the root, each with the cost of leaving it
/// out), and its complexity cost.
struct Quality {
    suffix: &'static str,
    tones: &'static [(u8, f64)],
    cost: f64,
}

const R: f64 = 1.0; // (the root must sound anyway)
const T3: f64 = 1.0;
const T5: f64 = 0.3;
const TX: f64 = 0.8;

const QUALITIES: [Quality; 19] = [
    Quality {
        suffix: "",
        tones: &[(0, R), (4, T3), (7, T5)],
        cost: 0.0,
    },
    Quality {
        suffix: "m",
        tones: &[(0, R), (3, T3), (7, T5)],
        cost: 0.0,
    },
    Quality {
        suffix: "5",
        tones: &[(0, R), (7, 1.0)],
        cost: 0.05,
    },
    Quality {
        suffix: "7",
        tones: &[(0, R), (4, T3), (7, T5), (10, TX)],
        cost: 0.15,
    },
    Quality {
        suffix: "maj7",
        tones: &[(0, R), (4, T3), (7, T5), (11, TX)],
        cost: 0.15,
    },
    Quality {
        suffix: "m7",
        tones: &[(0, R), (3, T3), (7, T5), (10, TX)],
        cost: 0.15,
    },
    Quality {
        suffix: "sus4",
        tones: &[(0, R), (5, TX), (7, T5)],
        cost: 0.15,
    },
    Quality {
        suffix: "sus2",
        tones: &[(0, R), (2, TX), (7, T5)],
        cost: 0.2,
    },
    Quality {
        suffix: "6",
        tones: &[(0, R), (4, T3), (7, T5), (9, TX)],
        cost: 0.2,
    },
    Quality {
        suffix: "add9",
        tones: &[(0, R), (2, TX), (4, T3), (7, T5)],
        cost: 0.2,
    },
    Quality {
        suffix: "m6",
        tones: &[(0, R), (3, T3), (7, T5), (9, TX)],
        cost: 0.25,
    },
    Quality {
        suffix: "madd9",
        tones: &[(0, R), (2, TX), (3, T3), (7, T5)],
        cost: 0.25,
    },
    Quality {
        suffix: "7sus4",
        tones: &[(0, R), (5, TX), (7, T5), (10, TX)],
        cost: 0.25,
    },
    Quality {
        suffix: "dim",
        tones: &[(0, R), (3, T3), (6, TX)],
        cost: 0.25,
    },
    Quality {
        suffix: "9",
        tones: &[(0, R), (2, TX), (4, T3), (7, T5), (10, TX)],
        cost: 0.3,
    },
    Quality {
        suffix: "m7b5",
        tones: &[(0, R), (3, T3), (6, TX), (10, TX)],
        cost: 0.3,
    },
    Quality {
        suffix: "aug",
        tones: &[(0, R), (4, T3), (8, TX)],
        cost: 0.3,
    },
    Quality {
        suffix: "m9",
        tones: &[(0, R), (2, TX), (3, T3), (7, T5), (10, TX)],
        cost: 0.35,
    },
    Quality {
        suffix: "dim7",
        tones: &[(0, R), (3, T3), (6, TX), (9, TX)],
        cost: 0.35,
    },
];

/// A pitch class counts as sounding from this strength; a chord's added tone (a 7th, 6th, 9th, sus) only from
/// `ADDED` (overdrive makes faint notes that aren't played); the bass is the lowest note from `BASS`.
const PRESENT: f64 = 0.12;
const ADDED: f64 = 0.3;
const BASS: f64 = 0.2;
/// A bass other than the root makes a slash chord from this strength.
const SLASH: f64 = 0.4;

/// A chord guess: root, quality, bass (a slash chord when it isn't the root), score and the margin over the next
/// best chord.
#[derive(Clone, Debug, PartialEq)]
pub struct Guess {
    pub root: u8,
    pub suffix: &'static str,
    pub bass: u8,
    /// The chord's pitch classes, root first, each with its interval name ("R", "3", "b7"...).
    pub tones: Vec<(u8, &'static str)>,
    pub score: f64,
    pub margin: f64,
}

impl Guess {
    pub fn name(&self) -> String {
        let mut s = format!("{}{}", NOTE_NAMES[self.root as usize], self.suffix);
        if self.bass != self.root {
            s += "/";
            s += NOTE_NAMES[self.bass as usize];
        }
        s
    }

    /// The interval name of a pitch class in the chord, None outside it.
    pub fn interval(&self, pc: u8) -> Option<&'static str> {
        self.tones.iter().find(|t| t.0 == pc % 12).map(|t| t.1)
    }

    /// Which chord of `key` it is (P25, D65): the root's degree as a Roman numeral, lower case for a minor chord,
    /// ° for a diminished one, + augmented, with its 7th / 6th / 9th; a power chord or a sus takes the case the
    /// key's own chord there has. A root outside the scale is named from the degree next to it (bVII, #IV).
    pub fn numeral(&self, (tonic, mode): Key) -> Numeral {
        const ROMAN: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
        let (scale, minor_at): ([u8; 7], [bool; 7]) = match mode {
            Mode::Major => (
                [0, 2, 4, 5, 7, 9, 11],
                [false, true, true, false, false, true, true],
            ),
            Mode::Minor => (
                [0, 2, 3, 5, 7, 8, 10],
                [true, true, false, true, true, false, false],
            ),
        };
        let rel = (self.root + 12 - tonic % 12) % 12;
        let (accidental, d) = match scale.iter().position(|&s| s == rel) {
            Some(d) => ("", d),
            None => match scale.iter().position(|&s| s == (rel + 1) % 12) {
                Some(d) => ("b", d),
                None => ("#", scale.iter().position(|&s| s == (rel + 11) % 12).unwrap_or(0)),
            },
        };
        let (lower, mark) = match self.suffix {
            "m" | "m7" | "m6" | "madd9" | "m9" => (true, &self.suffix[1..]),
            "dim" => (true, "\u{b0}"),
            "dim7" => (true, "\u{b0}7"),
            "m7b5" => (true, "\u{f8}7"),
            "aug" => (false, "+"),
            "5" => (minor_at[d] && accidental.is_empty(), ""),
            "sus4" | "sus2" | "7sus4" => (minor_at[d] && accidental.is_empty(), self.suffix),
            s => (false, s),
        };
        let r = if lower {
            ROMAN[d].to_lowercase()
        } else {
            ROMAN[d].to_string()
        };
        let in_scale = |pc: u8| scale.contains(&((pc + 12 - tonic % 12) % 12));
        Numeral {
            text: format!("{accidental}{r}{mark}"),
            degree: accidental.is_empty().then_some(d as u8 + 1),
            diatonic: self.tones.iter().all(|&(pc, _)| in_scale(pc)),
        }
    }
}

/// A chord's place in a key (`Guess::numeral`).
#[derive(Clone, Debug, PartialEq)]
pub struct Numeral {
    /// "I", "ii", "V7", "vii°", "bVII"...
    pub text: String,
    /// The root's degree in the scale (1 to 7), None when it's outside it.
    pub degree: Option<u8>,
    /// Every note of the chord is in the key.
    pub diatonic: bool,
}

/// A chart's chord name ("F#5/C#", "Bbm7", "Dsus2") as a number in `key`: the root's numeral as `Guess::numeral`
/// gives it (a quality it doesn't know keeps its own spelling, its case from a leading "m"), and a slash chord's
/// bass as its degree ("I/5", "V/b7"). The name back unchanged when it doesn't start with a note.
pub fn name_in_key(name: &str, key: Key) -> String {
    let pc = |s: &str| -> Option<(u8, usize)> {
        let base = match s.chars().next()? {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        match s[1..].chars().next() {
            Some('#' | '♯') => Some(((base + 1) % 12, 1 + s[1..].chars().next()?.len_utf8())),
            Some('b' | '♭') => Some(((base + 11) % 12, 1 + s[1..].chars().next()?.len_utf8())),
            _ => Some((base, 1)),
        }
    };
    let (head, bass) = match name.split_once('/') {
        Some((h, b)) => (h, pc(b.trim()).map(|p| p.0)),
        None => (name, None),
    };
    let Some((root, used)) = pc(head.trim()) else {
        return name.to_string();
    };
    let suffix = &head.trim()[used..];
    let known = QUALITIES.iter().find(|q| q.suffix == suffix);
    let minor = suffix.starts_with('m') && !suffix.starts_with("maj");
    let guess = Guess {
        root,
        suffix: known.map_or(if minor { "m" } else { "" }, |q| q.suffix),
        bass: root,
        tones: known.map_or_else(Vec::new, |q| {
            q.tones
                .iter()
                .map(|&(iv, _)| ((root + iv) % 12, interval_name(iv)))
                .collect()
        }),
        score: 0.0,
        margin: 0.0,
    };
    let mut out = guess.numeral(key).text;
    if known.is_none() {
        out += if minor { &suffix[1..] } else { suffix };
    }
    if let Some(b) = bass.filter(|&b| b != root) {
        // the bass's degree in the key, flat or sharp when it's outside the scale
        let scale: [u8; 7] = match key.1 {
            Mode::Major => [0, 2, 4, 5, 7, 9, 11],
            Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
        };
        let rel = (b + 12 - key.0 % 12) % 12;
        let deg = match scale.iter().position(|&s| s == rel) {
            Some(d) => format!("{}", d + 1),
            None => match scale.iter().position(|&s| s == (rel + 1) % 12) {
                Some(d) => format!("b{}", d + 1),
                None => format!(
                    "#{}",
                    scale.iter().position(|&s| s == (rel + 11) % 12).unwrap_or(0) + 1
                ),
            },
        };
        out += "/";
        out += &deg;
    }
    out
}

fn interval_name(semis: u8) -> &'static str {
    ["R", "b9", "9", "b3", "3", "4", "b5", "5", "#5", "6", "b7", "7"][semis as usize % 12]
}

/// The chord the notes make; None for silence, one pitch class (a note or its octaves), or nothing that fits.
pub fn name(heard: &[HeardNote]) -> Option<Guess> {
    let mut w = [0.0f64; 12];
    for n in heard {
        let pc = n.midi.rem_euclid(12) as usize;
        w[pc] = w[pc].max(n.strength.min(1.0));
    }
    if w.iter().filter(|&&v| v >= PRESENT).count() < 2 {
        return None;
    }
    let low = heard
        .iter()
        .filter(|n| n.strength >= BASS)
        .min_by_key(|n| n.midi)?;
    let bass = low.midi.rem_euclid(12) as u8;
    let mut scored: Vec<(f64, u8, &Quality)> = Vec::new();
    for root in 0..12u8 {
        if w[root as usize] < ADDED {
            continue;
        }
        for q in &QUALITIES {
            let mut inside = [false; 12];
            let mut s = -q.cost;
            for &(iv, miss) in q.tones {
                let pc = ((root + iv) % 12) as usize;
                inside[pc] = true;
                let need = if miss == TX { ADDED } else { PRESENT };
                s += if w[pc] >= need { w[pc] } else { -miss };
            }
            s -= (0..12).filter(|&pc| !inside[pc]).map(|pc| w[pc]).sum::<f64>();
            s += if bass == root {
                0.3
            } else if inside[bass as usize] {
                0.0
            } else {
                -0.2
            };
            scored.push((s, root, q));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let &(score, root, q) = scored.first()?;
    let margin = scored.get(1).map_or(score, |s| score - s.0);
    // a slash chord only for a bass that's played: not the fifth (an open string ringing under its chord, D over
    // the open A), nor a faint one
    let fifth = q.tones.iter().any(|&(iv, _)| iv == 7 && (root + 7) % 12 == bass);
    let bass = if bass != root && (fifth || low.strength < SLASH) {
        root
    } else {
        bass
    };
    Some(Guess {
        root,
        suffix: q.suffix,
        bass,
        tones: q
            .tones
            .iter()
            .map(|&(iv, _)| ((root + iv) % 12, interval_name(iv)))
            .collect(),
        score,
        margin,
    })
}

/// What the chord strip shows: the chord, the notes heard while it sounded, and when it was last heard.
#[derive(Clone, Debug)]
pub struct Current {
    pub guess: Guess,
    pub heard: Vec<HeardNote>,
    pub at: f64,
    pub last: f64,
}

/// Names the chord as the input comes: feed it a frame every 50 ms or so.
pub struct Listener {
    sp: Spectra,
    recent: VecDeque<Option<(Guess, Vec<HeardNote>)>>,
    pub current: Option<Current>,
    /// The chords shown, newest last (at most `HISTORY`).
    pub history: VecDeque<String>,
}

const HISTORY: usize = 8;
const VOTES: usize = 3;

impl Default for Listener {
    fn default() -> Self {
        Listener {
            sp: Spectra::new(),
            recent: VecDeque::new(),
            current: None,
            history: VecDeque::new(),
        }
    }
}

impl Listener {
    /// The samples a frame wants at `rate`.
    pub fn frame_len(rate: u32) -> usize {
        (rate as f64 * FRAME).round() as usize
    }

    /// One frame (the latest samples) at time `t` (seconds); true when the chord shown changed.
    pub fn feed(&mut self, x: &[f32], rate: u32, lowest: i32, t: f64) -> bool {
        let tail = &x[x.len() * 3 / 4..];
        let rms = (tail.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / tail.len().max(1) as f64).sqrt();
        let frame = if rms < QUIET {
            None
        } else {
            let heard = notes(&mut self.sp, x, rate, lowest);
            name(&heard).map(|g| (g, heard))
        };
        self.recent.push_back(frame);
        while self.recent.len() > VOTES {
            self.recent.pop_front();
        }
        // the chord two of the last three frames name
        let named: Vec<&(Guess, Vec<HeardNote>)> = self.recent.iter().flatten().collect();
        let winner = named
            .iter()
            .rev()
            .find(|(g, _)| named.iter().filter(|(o, _)| o.name() == g.name()).count() >= 2);
        let Some((g, heard)) = winner.map(|w| (w.0.clone(), w.1.clone())) else {
            return false;
        };
        match &mut self.current {
            Some(c) if c.guess.name() == g.name() => {
                c.last = t;
                // the notes heard over the chord's life (a strum rings out string by string)
                for n in heard {
                    if !c.heard.iter().any(|h| h.midi == n.midi) {
                        c.heard.push(n);
                    }
                }
                c.heard.truncate(10);
                false
            }
            _ => {
                self.history.push_back(g.name());
                while self.history.len() > HISTORY {
                    self.history.pop_front();
                }
                self.current = Some(Current {
                    guess: g,
                    heard,
                    at: t,
                    last: t,
                });
                true
            }
        }
    }

    pub fn reset(&mut self) {
        self.recent.clear();
        self.current = None;
        self.history.clear();
    }
}

/// A shape: each string's fret, low string first (-1 = not played).
pub type Shape = Vec<i32>;

/// The shape that plays `g` on `strings` (open MIDI, low first): every string muted, open or fretted in a 4-fret
/// hand, the bass the lowest note, scored on the chord's tones covered, the notes heard matched, a full strum of
/// neighbouring strings, a hand that fingers it (four fingers, or a barre at its lowest fret), low on the neck and
/// near `near` (the last shape's lowest fret).
pub fn shape(g: &Guess, heard: &[HeardNote], strings: &[i32], near: Option<i32>) -> Option<Shape> {
    let heard: Vec<i32> = heard.iter().map(|h| h.midi).collect();
    let mut best: Option<(f64, Shape)> = None;
    each_shape(g, strings, |_, s| {
        if let Some(v) = score_shape(s, g, &heard, strings, near)
            && best.as_ref().is_none_or(|b| v > b.0)
        {
            best = Some((v, s.clone()));
        }
    });
    best.map(|b| b.1)
}

/// Every place on the neck `g` is played (P25, D65): for each string the bass can be on, the best shape at each hand
/// position (by the chord alone, not what was heard), each with all the chord's tones but perhaps the fifth; then
/// one per stretch of the neck (a position kept only if no shape from the same string within two frets scores
/// better), low on the neck first. The open shapes count as the first position.
pub fn shapes(g: &Guess, strings: &[i32]) -> Vec<Shape> {
    // the best at each (bass string, position); the bass on one of the lowest four strings (a chord rooted on the
    // top two is a scrap, not a voicing)
    let mut at: Vec<Vec<Option<(f64, Shape)>>> = vec![vec![None; 22]; strings.len()];
    each_shape(g, strings, |lo, s| {
        let played: Vec<usize> = (0..s.len()).filter(|&i| s[i] >= 0).collect();
        let Some(&bass) = played.first() else { return };
        // three strings or more, side by side; open strings only in an open shape (no fret above the fourth)
        let together = played[played.len() - 1] - bass + 1 == played.len();
        let opens_ok = !s.contains(&0) || s.iter().all(|&f| f <= 4);
        if bass + 2 >= strings.len() || played.len() < 3 || !together || !opens_ok {
            return;
        }
        let pcs: Vec<u8> = played
            .iter()
            .map(|&i| (strings[i] + s[i]).rem_euclid(12) as u8)
            .collect();
        if g.tones.iter().any(|&(pc, iv)| iv != "5" && !pcs.contains(&pc)) {
            return;
        }
        let slot = &mut at[bass][lo as usize];
        if let Some(v) = score_shape(s, g, &[], strings, None)
            && slot.as_ref().is_none_or(|b| v > b.0)
        {
            *slot = Some((v, s.clone()));
        }
    });
    let mut out = Vec::new();
    for row in &mut at {
        // (the score's tilt to the low neck is for naming one shape; here every position is as good)
        let score =
            |row: &[Option<(f64, Shape)>], lo: usize| row[lo].as_ref().map(|b| b.0 + 0.08 * lo as f64);
        let keep: Vec<usize> = (1..=21)
            .filter(|&lo| {
                let Some(v) = score(row, lo) else { return false };
                (lo.saturating_sub(2).max(1)..=(lo + 2).min(21))
                    .filter(|&k| k != lo)
                    .all(|k| score(row, k).is_none_or(|o| v > o || (v == o && lo < k)))
            })
            .collect();
        out.extend(keep.into_iter().filter_map(|lo| row[lo].take().map(|b| b.1)));
    }
    // a part of a bigger shape (its notes all in it, as x x 7 6 5 5 in 5 7 7 6 5 5) is that shape
    let within = |a: &Shape, b: &Shape| a != b && (0..a.len()).all(|i| a[i] < 0 || a[i] == b[i]);
    let all = out.clone();
    out.retain(|s| !all.iter().any(|b| within(s, b)));
    let low = |s: &Shape| s.iter().copied().filter(|&f| f > 0).min().unwrap_or(0);
    out.sort_by_key(|s| (low(s), !s.contains(&0), s.iter().position(|&f| f >= 0)));
    out
}

/// Every candidate shape of `g` on `strings`, with the hand position it's at (its lowest fret; open-only shapes at
/// the first): each string muted, open or fretted in a 4-fret hand from fret `lo`, the chord's notes only.
fn each_shape(g: &Guess, strings: &[i32], mut f: impl FnMut(i32, &Shape)) {
    let in_chord = |m: i32| g.interval(m.rem_euclid(12) as u8).is_some();
    for lo in 1..=21 {
        let options: Vec<Vec<i32>> = strings
            .iter()
            .map(|&open| {
                let mut o = vec![-1];
                if in_chord(open) {
                    o.push(0);
                }
                o.extend((lo..lo + 4).filter(|&f| f <= 24 && in_chord(open + f)));
                o
            })
            .collect();
        let mut pick = vec![0usize; strings.len()];
        'combos: loop {
            let s: Shape = pick.iter().enumerate().map(|(i, &k)| options[i][k]).collect();
            // each shape scored once: at its lowest fret's position (open-only shapes at the first)
            if s.iter().copied().filter(|&f| f > 0).min().unwrap_or(1) == lo {
                f(lo, &s);
            }
            // the next combination
            for i in 0..pick.len() {
                pick[i] += 1;
                if pick[i] < options[i].len() {
                    continue 'combos;
                }
                pick[i] = 0;
            }
            break;
        }
    }
}

fn score_shape(s: &Shape, g: &Guess, heard: &[i32], strings: &[i32], near: Option<i32>) -> Option<f64> {
    let played: Vec<usize> = (0..s.len()).filter(|&i| s[i] >= 0).collect();
    if played.len() < 2 {
        return None;
    }
    let midi = |i: usize| strings[i] + s[i];
    let low = played.iter().map(|&i| midi(i)).min()?;
    if low.rem_euclid(12) as u8 != g.bass {
        return None;
    }
    // a hand: four fingers, the lowest fret as a barre if more than one string needs it
    let fretted: Vec<i32> = played.iter().map(|&i| s[i]).filter(|&f| f > 0).collect();
    if let Some(&minf) = fretted.iter().min() {
        let fingers = fretted.iter().filter(|&&f| f > minf).count() + 1;
        let at_min = fretted.iter().filter(|&&f| f == minf).count();
        if fingers > 4 || (at_min == 1 && fretted.len() > 4) {
            return None;
        }
        // a barre can't leave a string open above its lowest string
        if at_min > 1 && fretted.len() > 4 {
            let first = played.iter().find(|&&i| s[i] == minf).copied().unwrap_or(0);
            if played.iter().any(|&i| i > first && s[i] == 0) {
                return None;
            }
        }
    }
    let mut v = 0.0;
    let pcs: Vec<u8> = played.iter().map(|&i| midi(i).rem_euclid(12) as u8).collect();
    for &(pc, iv) in &g.tones {
        let have = pcs.contains(&pc);
        v += match (have, iv) {
            (true, _) => 1.0,
            (false, "5") => -0.4,
            (false, _) => -1.6,
        };
    }
    for &h in heard {
        v += if played.iter().any(|&i| midi(i) == h) {
            1.2
        } else {
            -0.5
        };
    }
    for &i in &played {
        if !heard.contains(&midi(i)) {
            v -= 0.15;
        }
    }
    v += 0.25 * played.len() as f64;
    // the same note on two strings (an open string and a fret under it) isn't how chords are held
    for (k, &i) in played.iter().enumerate() {
        if played[k + 1..].iter().any(|&j| midi(j) == midi(i)) {
            v -= 1.0;
        }
    }
    // a stretch over four frets
    if fretted
        .iter()
        .max()
        .zip(fretted.iter().min())
        .is_some_and(|(a, b)| a - b >= 3)
    {
        v -= 0.3;
    }
    let (a, b) = (played[0], played[played.len() - 1]);
    v -= 1.2 * (a..=b).filter(|&i| s[i] < 0).count() as f64;
    let minf = fretted.iter().min().copied().unwrap_or(0);
    v -= 0.08 * minf as f64;
    if minf <= 2 {
        v += 0.1 * played.iter().filter(|&&i| s[i] == 0).count() as f64;
    } else {
        v -= 0.3 * played.iter().filter(|&&i| s[i] == 0).count() as f64;
    }
    if let Some(n) = near {
        v -= (0.15 * (minf - n).abs() as f64).min(1.5);
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENS: [i32; 6] = [40, 45, 50, 55, 59, 64];
    const RATE: u32 = 48000;

    /// Plucked strings, roughly: each note 16 harmonics falling off as a guitar's, a little stretched, decaying;
    /// `frets` low string first (-1 = muted), struck 15 ms apart and heard 0.25 s after.
    fn strum(frets: [i32; 6]) -> Vec<f32> {
        let n = Listener::frame_len(RATE);
        let mut x = vec![0.0f32; n];
        for (s, &f) in frets.iter().enumerate().filter(|(_, f)| **f >= 0) {
            let f0 = 440.0 * 2f64.powf((OPENS[s] + f - 69) as f64 / 12.0);
            let t0 = 0.25 - 0.015 * s as f64;
            for h in 1..=16 {
                let fh = f0 * h as f64 * (1.0 + 0.0002 * (h * h) as f64).sqrt();
                let a = 0.8f64.powi(h - 1) / (1.0 + s as f64 * 0.1);
                for (i, v) in x.iter_mut().enumerate() {
                    let t = t0 + i as f64 / RATE as f64;
                    *v += (a * (-t * (1.0 + h as f64 * 0.4)).exp() * (std::f64::consts::TAU * fh * t).sin())
                        as f32
                        * 0.05;
                }
            }
        }
        x
    }

    fn named(frets: [i32; 6]) -> String {
        let heard = notes(&mut Spectra::new(), &strum(frets), RATE, 40);
        name(&heard).map(|g| g.name()).unwrap_or_else(|| "-".into())
    }

    #[test]
    fn open_chords_are_named() {
        for (frets, want) in [
            ([3, 2, 0, 0, 0, 3], "G"),
            ([-1, 3, 2, 0, 1, 0], "C"),
            ([-1, -1, 0, 2, 3, 2], "D"),
            ([-1, 0, 2, 2, 1, 0], "Am"),
            ([0, 2, 2, 0, 0, 0], "Em"),
            ([0, 2, 2, 1, 0, 0], "E"),
            ([-1, 0, 2, 2, 2, 0], "A"),
            ([-1, -1, 0, 2, 3, 1], "Dm"),
            ([-1, 0, 2, 0, 2, 0], "A7"),
            ([-1, 3, 2, 0, 0, 0], "Cmaj7"),
        ] {
            assert_eq!(named(frets), want, "{frets:?}");
        }
    }

    #[test]
    fn barre_power_and_slash_chords_are_named() {
        assert_eq!(named([1, 3, 3, 2, 1, 1]), "F");
        assert_eq!(named([-1, 2, 4, 4, 3, 2]), "Bm");
        assert_eq!(named([3, 5, 5, -1, -1, -1]), "G5");
        assert_eq!(named([0, 2, -1, -1, -1, -1]), "E5");
        assert_eq!(named([2, -1, 0, 2, 3, 2]), "D/F#");
    }

    #[test]
    fn a_single_note_is_no_chord() {
        assert_eq!(named([-1, -1, -1, -1, 3, -1]), "-");
        assert!(notes(&mut Spectra::new(), &vec![0.0; 8160], RATE, 40).is_empty());
    }

    #[test]
    fn notes_are_heard_at_their_octave() {
        let heard = notes(&mut Spectra::new(), &strum([3, 5, 5, -1, -1, -1]), RATE, 40);
        // (the octave G3 may fall out: it's all G2's harmonics)
        let m: Vec<i32> = heard.iter().map(|n| n.midi).collect();
        assert!(m.contains(&43) && m.contains(&50), "{heard:?}");
        assert!(m.iter().all(|x| [43, 50, 55].contains(x)), "{heard:?}");
    }

    fn guess(root: u8, suffix: &'static str, bass: u8, ivs: &[(u8, &'static str)]) -> Guess {
        Guess {
            root,
            suffix,
            bass,
            tones: ivs.iter().map(|&(i, n)| ((root + i) % 12, n)).collect(),
            score: 0.0,
            margin: 0.0,
        }
    }

    fn heard(m: &[i32]) -> Vec<HeardNote> {
        m.iter().map(|&midi| HeardNote { midi, strength: 1.0 }).collect()
    }

    #[test]
    fn shapes_are_the_usual_ones() {
        let triad = [(0, "R"), (4, "3"), (7, "5")];
        let minor = [(0, "R"), (3, "b3"), (7, "5")];
        let g = guess(7, "", 7, &triad);
        assert_eq!(
            shape(&g, &heard(&[43, 47, 50, 55, 59, 67]), &OPENS, None),
            Some(vec![3, 2, 0, 0, 0, 3])
        );
        let c = guess(0, "", 0, &triad);
        assert_eq!(
            shape(&c, &heard(&[48, 52, 55, 60, 64]), &OPENS, None),
            Some(vec![-1, 3, 2, 0, 1, 0])
        );
        let am = guess(9, "m", 9, &minor);
        assert_eq!(
            shape(&am, &heard(&[45, 52, 57, 60]), &OPENS, None),
            Some(vec![-1, 0, 2, 2, 1, 0])
        );
        // F up at the first fret as a barre; B minor a barre at the second
        let f = guess(5, "", 5, &triad);
        assert_eq!(
            shape(&f, &heard(&[41, 48, 53, 57]), &OPENS, None),
            Some(vec![1, 3, 3, 2, 1, 1])
        );
        let bm = guess(11, "m", 11, &minor);
        assert_eq!(
            shape(&bm, &heard(&[47, 54, 59, 62]), &OPENS, None),
            Some(vec![-1, 2, 4, 4, 3, 2])
        );
        // a power chord where it was heard
        let g5 = guess(7, "5", 7, &[(0, "R"), (7, "5")]);
        let s = shape(&g5, &heard(&[43, 50, 55]), &OPENS, None).unwrap();
        assert_eq!(&s[..3], &[3, 5, 5]);
    }

    /// All of a chord's places on the neck: A5 open, at the fifth fret on the low E, at the seventh on the D string
    /// (as Someday plays it) and at the twelfth; A major's open shape and the barres up the neck.
    #[test]
    fn every_place_a_chord_is_played() {
        let a5 = guess(9, "5", 9, &[(0, "R"), (7, "5")]);
        let all = shapes(&a5, &OPENS);
        let lows: Vec<i32> = all
            .iter()
            .map(|s| s.iter().copied().filter(|&f| f > 0).min().unwrap_or(0))
            .collect();
        assert!(all.len() >= 3 && all.len() <= 10, "{all:?}");
        assert!(all[0][0] == -1 && all[0][1] == 0, "open A first: {all:?}");
        for want in [5, 7] {
            assert!(
                lows.iter().any(|&l| (l - want).abs() <= 1),
                "near fret {want}: {all:?}"
            );
        }
        assert!(lows.windows(2).all(|w| w[0] <= w[1]), "low on the neck first");
        // each shape plays only A and E, the lowest an A
        for s in &all {
            let notes: Vec<i32> = (0..6).filter(|&i| s[i] >= 0).map(|i| OPENS[i] + s[i]).collect();
            assert!(notes.iter().all(|m| [9, 4].contains(&m.rem_euclid(12))), "{s:?}");
            assert_eq!(notes.iter().min().unwrap().rem_euclid(12), 9, "{s:?}");
        }
        let a = guess(9, "", 9, &[(0, "R"), (4, "3"), (7, "5")]);
        let all = shapes(&a, &OPENS);
        assert!(all.len() >= 5 && all.len() <= 9, "{all:?}");
        assert_eq!(all[0], vec![-1, 0, 2, 2, 2, 0], "{all:?}");
        assert!(
            all.iter().any(|s| s[..] == [5, 7, 7, 6, 5, 5]),
            "the E-shape barre: {all:?}"
        );
    }

    /// A chart's names as numbers: Someday in A major (D A A5/E E E7 F#m F#5/C# C#m), a flat, an unknown quality, a
    /// name that isn't a chord.
    #[test]
    fn chart_names_are_numbered_in_the_key() {
        let a = (9, Mode::Major);
        let n = |s: &str| name_in_key(s, a);
        assert_eq!(n("D"), "IV");
        assert_eq!(n("A"), "I");
        assert_eq!(n("A5/E"), "I/5");
        assert_eq!(n("E7"), "V7");
        assert_eq!(n("F#m"), "vi");
        assert_eq!(n("F#5/C#"), "vi/3");
        assert_eq!(n("C#m"), "iii");
        assert_eq!(n("Bbmaj7"), "bIImaj7");
        assert_eq!(n("Bm7(11)"), "ii7(11)");
        assert_eq!(n("G/F#"), "bVII/6");
        assert_eq!(n("N.C."), "N.C.");
    }

    /// Chords as numbers in a key: Someday's A5 E7 F#m in A major are I V7 vi; in A minor the C is III and the G
    /// VII; a Bb in A major is bII, outside the key.
    #[test]
    fn chords_are_numbered_in_the_key() {
        let triad = [(0, "R"), (4, "3"), (7, "5")];
        let minor = [(0, "R"), (3, "b3"), (7, "5")];
        let n = |g: &Guess, key: Key| g.numeral(key).text;
        let a_major = (9, Mode::Major);
        assert_eq!(n(&guess(9, "5", 9, &[(0, "R"), (7, "5")]), a_major), "I");
        let e7 = guess(4, "7", 4, &[(0, "R"), (4, "3"), (7, "5"), (10, "b7")]);
        assert_eq!(
            e7.numeral(a_major),
            Numeral {
                text: "V7".into(),
                degree: Some(5),
                diatonic: true
            }
        );
        assert_eq!(n(&guess(6, "m", 6, &minor), a_major), "vi");
        // F#5: a power chord takes the key's case (vi is minor)
        assert_eq!(n(&guess(6, "5", 6, &[(0, "R"), (7, "5")]), a_major), "vi");
        assert_eq!(
            n(
                &guess(1, "dim", 1, &[(0, "R"), (3, "b3"), (6, "b5")]),
                (2, Mode::Major)
            ),
            "vii\u{b0}"
        );
        let a_minor = (9, Mode::Minor);
        assert_eq!(n(&guess(0, "", 0, &triad), a_minor), "III");
        assert_eq!(n(&guess(7, "", 7, &triad), a_minor), "VII");
        let bb = guess(10, "", 10, &triad).numeral(a_major);
        assert_eq!((bb.text.as_str(), bb.degree, bb.diatonic), ("bII", None, false));
        // the root in the scale but a note outside it: numbered, not diatonic
        let d7 = guess(2, "7", 2, &[(0, "R"), (4, "3"), (7, "5"), (10, "b7")]).numeral(a_major);
        assert_eq!((d7.text.as_str(), d7.diatonic), ("IV7", false));
    }

    #[test]
    fn the_listener_holds_a_chord_two_frames_name() {
        let mut l = Listener::default();
        let x = strum([-1, 0, 2, 2, 1, 0]);
        assert!(!l.feed(&x, RATE, 40, 0.0));
        assert!(l.feed(&x, RATE, 40, 0.05));
        assert_eq!(l.current.as_ref().unwrap().guess.name(), "Am");
        // quiet: it stays
        let q = vec![0.0; x.len()];
        for i in 0..5 {
            l.feed(&q, RATE, 40, 0.1 + i as f64 * 0.05);
        }
        assert_eq!(l.current.as_ref().unwrap().guess.name(), "Am");
        let y = strum([3, 2, 0, 0, 0, 3]);
        l.feed(&y, RATE, 40, 1.0);
        assert!(l.feed(&y, RATE, 40, 1.05));
        assert_eq!(l.history.iter().cloned().collect::<Vec<_>>(), ["Am", "G"]);
    }
}
