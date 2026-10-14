//! Rhythm notation (like Songsterr's stems under the tab), inferred from the chart's timed notes. Port of
//! rsplay/rhythm.py (exact fractions, Python rounding).
//!
//! Charts give each note's time and the song's beat grid, but not written note values, so they're worked out:
//! 1. snap: every onset snaps to the nearest slot of its beat: sixteenths, or eighth-note triplets when those
//!    fit the beat clearly better (`triplets`);
//! 2. lengths: a note lasts until the next onset. With `rests`, a short note followed by a long gap is written
//!    as the note plus rests, using the chart's sustain to decide how long the note really rings;
//! 3. notation: each note or rest is split at bar lines (and at beat lines when it starts off the beat), then
//!    written as whole/half/quarter/eighth/sixteenth values with dots; split notes are tied.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::sng::py_round;

/// An exact fraction (always normalised, denominator > 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frac {
    pub n: i64,
    pub d: i64,
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs().max(1) } else { gcd(b, a % b) }
}

impl Frac {
    pub fn new(n: i64, d: i64) -> Frac {
        let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
        let g = gcd(n.abs(), d);
        Frac { n: n / g, d: d / g }
    }
    pub fn int(n: i64) -> Frac {
        Frac { n, d: 1 }
    }
    /// floor (Python's int() for positives)
    pub fn floor(self) -> i64 {
        self.n.div_euclid(self.d)
    }
    pub fn to_f64(self) -> f64 {
        self.n as f64 / self.d as f64
    }
}

impl std::ops::Add for Frac {
    type Output = Frac;
    fn add(self, o: Frac) -> Frac {
        Frac::new(self.n * o.d + o.n * self.d, self.d * o.d)
    }
}
impl std::ops::Sub for Frac {
    type Output = Frac;
    fn sub(self, o: Frac) -> Frac {
        Frac::new(self.n * o.d - o.n * self.d, self.d * o.d)
    }
}
impl std::ops::Mul for Frac {
    type Output = Frac;
    fn mul(self, o: Frac) -> Frac {
        Frac::new(self.n * o.n, self.d * o.d)
    }
}
impl PartialOrd for Frac {
    fn partial_cmp(&self, o: &Frac) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Frac {
    fn cmp(&self, o: &Frac) -> Ordering {
        (self.n as i128 * o.d as i128).cmp(&(o.n as i128 * self.d as i128))
    }
}

/// value in beats → (flags, dots); flags: -2 whole, -1 half, 0 quarter, 1 eighth, 2 sixteenth, 3 32nd
const STRAIGHT: [((i64, i64), i32, i32); 10] = [
    ((4, 1), -2, 0),
    ((3, 1), -1, 1),
    ((2, 1), -1, 0),
    ((3, 2), 0, 1),
    ((1, 1), 0, 0),
    ((3, 4), 1, 1),
    ((1, 2), 1, 0),
    ((3, 8), 2, 1),
    ((1, 4), 2, 0),
    ((1, 8), 3, 0),
];
const TRIPLET: [((i64, i64), i32, i32); 3] = [((2, 3), 0, 0), ((1, 3), 1, 0), ((1, 6), 2, 0)];

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Start, in beats from the first beat.
    pub pos: Frac,
    /// Notated length.
    pub beats: Frac,
    pub rest: bool,
    pub flags: i32,
    pub dots: i32,
    pub triplet: bool,
    /// Continues the previous note (a tie, no new fret number).
    pub tied: bool,
    /// The chart time of the note this piece starts (None for ties and rests).
    pub onset: Option<f64>,
    /// Time of `pos`, for placing it under the tab.
    pub t: f64,
}

impl Piece {
    pub fn beat(&self) -> i64 {
        self.pos.floor()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub snap: bool,
    pub rests: bool,
    /// Beams and ties (off: flags only, no tie arcs).
    pub beams: bool,
    pub triplets: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            snap: true,
            rests: true,
            beams: true,
            triplets: true,
        }
    }
}

pub struct Rhythm {
    bt: Vec<f64>,
    pub opts: Options,
    pub bars: Vec<i64>,
    /// chart time (as bits) → snapped time (for placing tab numbers)
    pub snapped: BTreeMap<u64, f64>,
    pub pieces: Vec<Piece>,
    pub piece_times: Vec<f64>,
}

impl Rhythm {
    /// `onsets`: (time, sustain seconds) of each distinct note/chord time, sorted.
    pub fn new(beat_times: Vec<f64>, downbeat_idx: &[usize], onsets: &[(f64, f64)], opts: Options) -> Rhythm {
        let mut bars: Vec<i64> = downbeat_idx.iter().map(|&i| i as i64).collect();
        bars.sort_unstable();
        bars.dedup();
        let mut r = Rhythm {
            bt: beat_times,
            opts,
            bars,
            snapped: BTreeMap::new(),
            pieces: Vec::new(),
            piece_times: Vec::new(),
        };
        if r.bt.len() >= 2 && !onsets.is_empty() {
            r.build(onsets);
        }
        r.piece_times = r.pieces.iter().map(|p| p.t).collect();
        r
    }

    /// The snapped time of a chart time (or the time itself).
    pub fn snapped_time(&self, t: f64) -> f64 {
        self.snapped.get(&t.to_bits()).copied().unwrap_or(t)
    }

    pub fn time_of(&self, pos: Frac) -> f64 {
        let i = (pos.floor().max(0) as usize).min(self.bt.len() - 2);
        let frac = (pos - Frac::int(i as i64)).to_f64();
        self.bt[i] + frac * (self.bt[i + 1] - self.bt[i])
    }

    pub fn beat_len(&self, i: i64) -> f64 {
        let i = i.clamp(0, self.bt.len() as i64 - 2) as usize;
        self.bt[i + 1] - self.bt[i]
    }

    fn snap(&self, onsets: &[(f64, f64)]) -> Vec<(Frac, f64, f64)> {
        let mut by_beat: BTreeMap<i64, Vec<(f64, f64, f64)>> = BTreeMap::new();
        let mut order: Vec<i64> = Vec::new();
        for &(t, sus) in onsets {
            if t < self.bt[0] - 0.05 || t >= *self.bt.last().unwrap() {
                continue;
            }
            let i = (self.bt.partition_point(|&b| b <= t) as i64 - 1).max(0);
            let frac = (t - self.bt[i as usize]) / self.beat_len(i);
            if !by_beat.contains_key(&i) {
                order.push(i);
            }
            by_beat.entry(i).or_default().push((frac.max(0.0), t, sus));
        }
        let mut out: Vec<(Frac, f64, f64)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for i in order {
            let evs = &by_beat[&i];
            let mut grid = 4i64;
            if self.opts.triplets && evs.len() >= 2 {
                let e4: f64 = evs
                    .iter()
                    .map(|(f, _, _)| (f * 4.0 - (f * 4.0).round_ties_even()).abs() / 4.0)
                    .sum();
                let e3: f64 = evs
                    .iter()
                    .map(|(f, _, _)| (f * 3.0 - (f * 3.0).round_ties_even()).abs() / 3.0)
                    .sum();
                if e3 + 0.01 < e4 * 0.5 {
                    grid = 3;
                }
            }
            for &(f, t, sus) in evs {
                let pos = Frac::int(i) + Frac::new(py_round(f * grid as f64) as i64, grid);
                if seen.insert(pos) {
                    out.push((pos, t, sus));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
        out
    }

    fn build(&mut self, onsets: &[(f64, f64)]) {
        let snapped = self.snap(onsets);
        for &(pos, t, _) in &snapped {
            let st = self.time_of(pos);
            self.snapped.insert(t.to_bits(), st);
        }
        let end = Frac::int(self.bt.len() as i64 - 1);
        for k in 0..snapped.len() {
            let (pos, t, sus) = snapped[k];
            let nxt = if k + 1 < snapped.len() {
                snapped[k + 1].0
            } else {
                end.min(self.bar_end(pos))
            };
            let gap = nxt - pos;
            if gap <= Frac::int(0) {
                continue;
            }
            let mut note = gap;
            if self.opts.rests {
                let unit = if pos.d == 3 {
                    Frac::new(1, 3)
                } else {
                    Frac::new(1, 4)
                };
                let sus_beats =
                    Frac::int(py_round(sus / self.beat_len(pos.floor()) / unit.to_f64()) as i64) * unit;
                let to_beat_end = Frac::int(pos.floor() + 1) - pos;
                note = gap.min(sus_beats.max(unit).max(to_beat_end));
            }
            self.emit(pos, note, false, Some(t));
            if gap > note {
                self.emit(pos + note, gap - note, true, None);
            }
        }
        for i in 0..self.pieces.len() {
            let t = self.time_of(self.pieces[i].pos);
            self.pieces[i].t = t;
        }
    }

    fn bar_end(&self, pos: Frac) -> Frac {
        let p = pos.floor();
        let i = self.bars.partition_point(|&b| b <= p);
        if i < self.bars.len() {
            Frac::int(self.bars[i])
        } else {
            Frac::int(self.bt.len() as i64 - 1)
        }
    }

    /// Split at bars, and at beats when starting off the beat or inside a triplet beat; then write values.
    fn emit(&mut self, mut pos: Frac, mut length: Frac, rest: bool, onset: Option<f64>) {
        let mut first = true;
        let zero = Frac::int(0);
        while length > zero {
            let mut stop = (pos + length).min(self.bar_end(pos));
            if pos.d != 1 {
                // starts off the beat: end this piece at the beat (tied on if longer)
                stop = stop.min(Frac::int(pos.floor() + 1));
            }
            let seg = stop - pos;
            if seg <= zero {
                break;
            }
            let triplet = pos.d == 3 || seg.d == 3;
            for (value, flags, dots) in values(seg, triplet) {
                self.pieces.push(Piece {
                    pos,
                    beats: value,
                    rest,
                    flags,
                    dots,
                    triplet: triplet && value < Frac::int(1),
                    tied: !rest && !first,
                    onset: if first && !rest { onset } else { None },
                    t: 0.0,
                });
                pos = pos + value;
                first = false;
            }
            pos = stop;
            length = length - seg;
        }
    }

    /// Text rendering for checking: e.g. "q  e e  s s e  | ...".
    pub fn describe(&self, start: f64, stop: f64) -> String {
        let name = |f: i32| match f {
            -2 => "w",
            -1 => "h",
            0 => "q",
            1 => "e",
            2 => "s",
            _ => "t",
        };
        let mut out: Vec<String> = Vec::new();
        let mut beat: Option<i64> = None;
        for p in &self.pieces {
            if !(start <= p.t && p.t < stop) {
                continue;
            }
            if let Some(b) = beat
                && p.beat() != b
            {
                out.push(if self.bars.contains(&p.beat()) {
                    " |".into()
                } else {
                    " ".into()
                });
            }
            beat = Some(p.beat());
            let sym = format!(
                "{}{}{}",
                name(p.flags),
                ".".repeat(p.dots as usize),
                if p.triplet { "3" } else { "" }
            );
            out.push(if p.rest {
                format!("r{sym}")
            } else if p.tied {
                format!("~{sym}")
            } else {
                sym
            });
        }
        out.join(" ")
    }
}

fn values(length: Frac, triplet: bool) -> Vec<(Frac, i32, i32)> {
    let table: Vec<((i64, i64), i32, i32)> = if triplet {
        TRIPLET.iter().chain(STRAIGHT.iter()).copied().collect()
    } else {
        STRAIGHT.to_vec()
    };
    let mut out = Vec::new();
    let mut left = length;
    let zero = Frac::int(0);
    while left > zero {
        let mut took = false;
        for &((n, d), flags, dots) in &table {
            let v = Frac::new(n, d);
            if v <= left {
                out.push((v, flags, dots));
                left = left - v;
                took = true;
                break;
            }
        }
        if !took {
            break; // a leftover sliver: absorbed into the previous value
        }
    }
    out
}

/// The rhythm for an arrangement: its beats, downbeats and distinct note/chord times with their sustains.
pub fn for_arrangement(arr: &crate::model::Arrangement, opts: Options) -> Rhythm {
    let beats: Vec<f64> = arr.beats.iter().map(|b| b.time).collect();
    let downs: Vec<usize> = arr
        .beats
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_downbeat())
        .map(|(i, _)| i)
        .collect();
    // as the Python app: onset times rounded to the millisecond, the longest sustain at each
    let mut onsets: BTreeMap<u64, (f64, f64)> = BTreeMap::new();
    let r3 = |t: f64| -> f64 { format!("{t:.3}").parse().unwrap_or(t) };
    for (t, sus) in arr
        .notes
        .iter()
        .map(|n| (n.time, n.sustain))
        .chain(arr.chords.iter().map(|c| (c.time, c.sustain)))
    {
        let t = r3(t);
        let entry = onsets.entry(t.to_bits()).or_insert((t, 0.0));
        entry.1 = entry.1.max(sus);
    }
    let mut list: Vec<(f64, f64)> = onsets.into_values().collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0));
    Rhythm::new(beats, &downs, &list, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize) -> (Vec<f64>, Vec<usize>) {
        (
            (0..n).map(|i| i as f64 * 0.5).collect(),
            (0..n).step_by(4).collect(),
        )
    }

    #[test]
    fn quarters_eighths_and_sixteenths() {
        let (bt, downs) = grid(9);
        // beat 0: quarter; beat 1: two eighths; beat 2: four sixteenths; beat 3: quarter
        let on: Vec<(f64, f64)> = [0.0, 0.5, 0.75, 1.0, 1.125, 1.25, 1.375, 1.5]
            .iter()
            .map(|&t| (t, 0.0))
            .collect();
        let r = Rhythm::new(
            bt,
            &downs,
            &on,
            Options {
                rests: false,
                ..Default::default()
            },
        );
        assert_eq!(r.describe(0.0, 2.0), "q   e e   s s s s   q"); // as rhythm.py prints it
    }

    #[test]
    fn triplets_are_found() {
        let (bt, downs) = grid(9);
        let on: Vec<(f64, f64)> = [0.0, 1.0 / 6.0, 2.0 / 6.0, 0.5]
            .iter()
            .map(|&t| (t, 0.0))
            .collect();
        let r = Rhythm::new(
            bt,
            &downs,
            &on,
            Options {
                rests: false,
                ..Default::default()
            },
        );
        assert!(
            r.describe(0.0, 0.5).contains("e3 e3 e3"),
            "{}",
            r.describe(0.0, 0.5)
        );
    }

    #[test]
    fn short_notes_get_rests() {
        let (bt, downs) = grid(9);
        let r = Rhythm::new(bt, &downs, &[(0.0, 0.1), (2.0, 0.1)], Options::default());
        // one quarter note then rests to the next bar
        assert_eq!(r.describe(0.0, 2.0), "q   rh.");
    }

    #[test]
    fn fractions() {
        assert_eq!(Frac::new(2, 4), Frac::new(1, 2));
        assert_eq!((Frac::new(1, 3) + Frac::new(1, 6)), Frac::new(1, 2));
        assert!(Frac::new(2, 3) > Frac::new(1, 2));
        assert_eq!(Frac::new(7, 3).floor(), 2);
    }
}
