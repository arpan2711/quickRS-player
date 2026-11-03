//! A part's practice units (new in quickRS, P29.3, D77): the song cut into ideas that repeat (a riff, a phrase:
//! 2, 4 or 8 bars, each with every place it plays) and the stretches between them (a solo, a fill), so the run's
//! review can say "Riff B, 4 bars, plays 6 times" and loop it once instead of listing each time it comes back.
//!
//! - Every pair of bars is compared (`similar::bar_similarity`, pitches as sounded); a bar without notes matches
//!   another without notes (a rest in a riff) and nothing else.
//! - A window of L bars (8, 4, 2) "plays again" at another start where its bars match on average by `WINDOW_SAME`
//!   and none falls under `BAR_FLOOR` (one bar played a little differently is still the riff).
//! - Greedy cover (COSIATEC-like, on bars): of every window still free, the one covering the most bars by its
//!   repeats (L x (times - 1) x the mean match) is an idea; its places are taken; again until nothing repeats. A
//!   riff of 4 played six times beats the same as 8 bars played three times (20 against 16), so the unit is the
//!   riff. Starts on a section or phrase boundary (the charter's marks: unreliable as names, mostly right as
//!   places) win ties, so a riff isn't found a bar out of phase.
//! - What's left (a solo, a bridge played once) is cut at the boundaries, then into pieces of at most
//!   `LEFTOVER_MAX` bars.

use std::ops::Range;

use crate::similar::{Bars, bar_similarity};

/// A window of bars plays again where its bars match this well on average...
pub const WINDOW_SAME: f64 = 0.75;
/// ...and none worse than this.
pub const BAR_FLOOR: f64 = 0.5;
/// The units tried, longest first.
pub const LENGTHS: [usize; 3] = [8, 4, 2];
/// A stretch that plays once is cut into pieces this long at most.
pub const LEFTOVER_MAX: usize = 4;
/// The weight a boundary start adds (a share of the score).
const ON_BOUNDARY: f64 = 0.15;

/// An idea: its length in bars and where it starts each time (bar indices into `Bars::bars`, in order).
#[derive(Clone, Debug, PartialEq)]
pub struct Idea {
    pub len: usize,
    pub starts: Vec<usize>,
    /// How alike its places are (the mean match; 1 for one that plays once).
    pub likeness: f64,
}

impl Idea {
    pub fn repeats(&self) -> bool {
        self.starts.len() > 1
    }

    /// Its places, as ranges of bars.
    pub fn places(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        self.starts.iter().map(|&s| s..s + self.len)
    }
}

/// Bar-by-bar similarity of a part (`n x n`, symmetric).
pub fn similarity(bars: &Bars) -> Vec<Vec<f64>> {
    let n = bars.prints.len();
    let mut s = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in i..n {
            let (a, b) = (&bars.prints[i], &bars.prints[j]);
            let v = match (a.events.is_empty(), b.events.is_empty()) {
                (true, true) => 1.0,
                (false, false) => bar_similarity(a, b),
                _ => 0.0,
            };
            s[i][j] = v;
            s[j][i] = v;
        }
    }
    s
}

/// The ideas of a part, in the order they first play, every bar with notes in exactly one place of one idea.
/// `boundaries` are bar indices where a section or phrase starts.
pub fn ideas(sim: &[Vec<f64>], has_notes: &[bool], boundaries: &[usize]) -> Vec<Idea> {
    let n = has_notes.len();
    let mut used = vec![false; n];
    let mut out: Vec<Idea> = Vec::new();
    let on_b = |i: usize| boundaries.contains(&i);
    loop {
        // the best window still free: (score, len, start, places, mean)
        let mut best: Option<(f64, usize, usize, Vec<usize>, f64)> = None;
        for &len in &LENGTHS {
            if len > n {
                continue;
            }
            for i in 0..=n - len {
                let w = i..i + len;
                // free, starting and ending on notes, at least half its bars with notes
                if w.clone().any(|k| used[k])
                    || !has_notes[i]
                    || !has_notes[i + len - 1]
                    || w.clone().filter(|&k| has_notes[k]).count() * 2 < len
                {
                    continue;
                }
                let mut places = vec![i];
                let mut total = 0.0;
                let mut j = 0;
                while j + len <= n {
                    if j < i + len && j + len > i || (j..j + len).any(|k| used[k]) {
                        j += 1;
                        continue;
                    }
                    let ms: Vec<f64> = (0..len).map(|k| sim[i + k][j + k]).collect();
                    let mean = ms.iter().sum::<f64>() / len as f64;
                    if has_notes[j] && mean >= WINDOW_SAME && ms.iter().all(|&m| m >= BAR_FLOOR) {
                        places.push(j);
                        total += mean;
                        j += len;
                    } else {
                        j += 1;
                    }
                }
                if places.len() < 2 {
                    continue;
                }
                places.sort_unstable();
                let times = places.len() as f64;
                let mean = total / (times - 1.0);
                let on = places.iter().filter(|&&p| on_b(p)).count() as f64 / times;
                let score = len as f64 * (times - 1.0) * mean * (1.0 + ON_BOUNDARY * on);
                let better = match &best {
                    None => true,
                    Some((bs, bl, bi, _, _)) => {
                        score > bs + 1e-9
                            || ((score - bs).abs() <= 1e-9 && (len > *bl || len == *bl && i < *bi))
                    }
                };
                if better {
                    best = Some((score, len, i, places, mean));
                }
            }
        }
        let Some((_, len, _, places, mean)) = best else {
            break;
        };
        for &p in &places {
            for k in p..p + len {
                used[k] = true;
            }
        }
        out.push(Idea {
            len,
            starts: places,
            likeness: mean,
        });
    }
    // what plays once: runs of free bars with notes, cut at boundaries, then into even pieces
    let mut i = 0;
    while i < n {
        if used[i] || !has_notes[i] {
            i += 1;
            continue;
        }
        let mut e = i;
        while e + 1 < n && !used[e + 1] && (has_notes[e + 1] || e + 2 < n && has_notes[e + 2] && !used[e + 2])
        {
            e += 1;
        }
        // (a run ends on a bar with notes)
        while !has_notes[e] {
            e -= 1;
        }
        let mut cuts: Vec<usize> = vec![i];
        cuts.extend(boundaries.iter().copied().filter(|&b| b > i && b <= e));
        cuts.push(e + 1);
        cuts.dedup();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = b - a;
            let k = len.div_ceil(LEFTOVER_MAX);
            let mut at = a;
            for c in 0..k {
                let size = len / k + usize::from(c < len % k);
                out.push(Idea {
                    len: size,
                    starts: vec![at],
                    likeness: 1.0,
                });
                at += size;
            }
        }
        i = e + 1;
    }
    out.sort_by_key(|d| d.starts[0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A similarity matrix from bar "letters": the same letter 1, different 0.1; '.' a bar without notes.
    fn sim_of(s: &str) -> (Vec<Vec<f64>>, Vec<bool>) {
        let c: Vec<char> = s.chars().collect();
        let m = c
            .iter()
            .map(|a| c.iter().map(|b| if a == b { 1.0 } else { 0.1 }).collect())
            .collect();
        (m, c.iter().map(|&x| x != '.').collect())
    }

    fn summary(d: &[Idea]) -> Vec<(usize, Vec<usize>)> {
        d.iter().map(|i| (i.len, i.starts.clone())).collect()
    }

    #[test]
    fn a_riff_played_over_and_over_is_one_idea() {
        // a four-bar riff six times
        let (s, h) = sim_of(&"abcd".repeat(6));
        let d = ideas(&s, &h, &[]);
        assert_eq!(summary(&d), vec![(4, vec![0, 4, 8, 12, 16, 20])]);
    }

    #[test]
    fn verse_and_chorus_riffs_and_a_solo() {
        // riff A (2 bars) x4, riff B (4 bars) x2, a solo of 6 bars, riff A x2
        let song = "ababababcdefcdefghijklabab";
        let (s, h) = sim_of(song);
        let d = ideas(&s, &h, &[]);
        let sm = summary(&d);
        assert_eq!(sm[0], (2, vec![0, 2, 4, 6, 22, 24]));
        assert_eq!(sm[1], (4, vec![8, 12]));
        // the solo: 6 bars, cut in two pieces of 3
        assert_eq!(&sm[2..], &[(3, vec![16]), (3, vec![19])]);
    }

    #[test]
    fn the_solo_is_cut_at_a_boundary() {
        let (s, h) = sim_of("ababghijkl");
        let d = ideas(&s, &h, &[6]);
        assert_eq!(summary(&d), vec![(2, vec![0, 2]), (2, vec![4]), (4, vec![6])]);
    }

    #[test]
    fn a_boundary_puts_the_riff_in_phase() {
        // "xabab..." with the phrase starting on bar 1: the riff is "ab", not "ba"
        let (s, h) = sim_of("xababab");
        let d = ideas(&s, &h, &[1]);
        assert_eq!(d[1].starts, vec![1, 3, 5]);
        assert_eq!(d[1].len, 2);
    }

    #[test]
    fn one_bar_played_differently_is_still_the_riff() {
        let (mut s, h) = sim_of("abcdabcd");
        // the second time, bar 2 is only 0.6 like the first
        s[2][6] = 0.6;
        s[6][2] = 0.6;
        let d = ideas(&s, &h, &[]);
        assert_eq!(summary(&d), vec![(4, vec![0, 4])]);
    }

    #[test]
    fn a_rest_bar_stays_in_the_riff_but_not_alone() {
        let (s, h) = sim_of("ab.cab.c..");
        let d = ideas(&s, &h, &[]);
        assert_eq!(summary(&d), vec![(4, vec![0, 4])]);
    }
}
