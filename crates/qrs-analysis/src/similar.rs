//! Similar bars and sections of a chart (new in quickRS, P18.1, D30): which parts of a song repeat, so a repeated
//! verse is taught once, and which bars of a stretch are new compared with what earlier lesson items taught.
//!
//! - Bars come from the chart's beats: a bar starts at each downbeat (a beat with a measure number) and holds the
//!   beats up to the next. Notes before the first downbeat make a pickup bar (number 0).
//! - A bar's fingerprint is its events (a note, a double stop or a chord, merged as the scorer merges them) with
//!   their position in the bar, in twelfths of a beat (fine enough for sixteenths, triplets and shuffle eighths,
//!   and measured against the bar's own beats, so a tempo change doesn't move it), and their pitches.
//! - `bar_similarity` (0..1) aligns two bars' events in order: an event pair counts by how close their positions
//!   are (same twelfth 1, one off 0.85, two off 0.5, further 0) times how alike their pitches are (the share of
//!   pitches in common; a muted strum against a muted strum is 1). The sum is divided by the longer bar's event
//!   count, so a missing or extra note costs. Two empty bars are the same (1).
//! - `bar_similarity_transposed` also tries the second bar a few semitones away and returns the best shift (a
//!   12-bar's IV and V bars are the I bar's pattern moved up a 4th and a 5th: shift +5 / +7).
//! - Stretches (sections, or any spans) are compared bar by bar with the best alignment (a verse with an extra bar
//!   still lines up), giving a score and the bars that differ.
//! - `novel_bars` marks the bars of a stretch that nothing in a set of learned stretches resembles ("new here").
//!
//! Pitches are compared as sounding pitches (string and fret don't matter: the same notes played elsewhere on the
//! neck are the same music); bends, slides and sustains are not part of the fingerprint.

use std::ops::Range;

use qrs_formats::ascii_tab::section_label;
use qrs_formats::model::{Arrangement, note_name};

use crate::detect::build_groups;

/// Positions in a bar are counted in twelfths of a beat.
pub const STEPS_PER_BEAT: i32 = 12;
/// Bars at least this similar are "the same".
pub const SAME: f64 = 0.9;
/// Bars of a stretch less covered than this by everything learned are "new here".
pub const NOVEL_BELOW: f64 = 0.85;
/// ... and so are bars whose likeness (symmetric, `bar_similarity`) to every learned bar is under this: a sparser
/// version of a learned bar (a stop-time bar, a hit or two, against the shuffle bar it thins out) is covered by it
/// but is something new to play.
pub const NOVEL_SPARSE_BELOW: f64 = 0.6;
/// Sections at least this similar are grouped as repeats.
pub const SECTION_SAME: f64 = 0.75;
/// An event this close before a downbeat belongs to the bar starting there.
const EARLY_EPS: f64 = 0.02;

#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// Index in `Bars::bars`.
    pub index: usize,
    /// The chart's measure number (0 for a pickup bar).
    pub number: i32,
    pub t0: f64,
    pub t1: f64,
    /// Beat times in the bar, the first at t0.
    pub beats: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BarEvent {
    /// Chart time.
    pub t: f64,
    /// Position in the bar, in twelfths of a beat.
    pub pos: i32,
    /// Sounding pitches (MIDI), sorted, without repeats; empty for a muted strum.
    pub pitches: Vec<i32>,
    pub muted: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fingerprint {
    pub beats: usize,
    pub events: Vec<BarEvent>,
}

impl Fingerprint {
    /// Only the events in [a, b) (a stretch that starts or ends inside the bar; both edges a hair early, as a
    /// note pushed just ahead of a downbeat belongs to the bar starting there).
    pub fn clipped(&self, a: f64, b: f64) -> Fingerprint {
        Fingerprint {
            beats: self.beats,
            events: self
                .events
                .iter()
                .filter(|e| e.t >= a - EARLY_EPS && e.t < b - EARLY_EPS)
                .cloned()
                .collect(),
        }
    }

    /// "Bb2 . D3+F3 . ..." for the CLI.
    pub fn describe(&self) -> String {
        self.events
            .iter()
            .map(|e| {
                if e.muted {
                    "x".to_string()
                } else {
                    e.pitches
                        .iter()
                        .map(|&m| format!("{}{}", note_name(m), m.div_euclid(12) - 1))
                        .collect::<Vec<_>>()
                        .join("+")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A chart's bars and their fingerprints.
#[derive(Clone, Debug, Default)]
pub struct Bars {
    pub bars: Vec<Bar>,
    pub prints: Vec<Fingerprint>,
}

/// The pairing of two bars' events.
#[derive(Clone, Debug, PartialEq)]
pub struct BarMatch {
    pub score: f64,
    /// (event in a, event in b, how alike 0..1).
    pub pairs: Vec<(usize, usize, f64)>,
}

fn time_weight(d: i32) -> f64 {
    match d.abs() {
        0 => 1.0,
        1 => 0.85,
        2 => 0.5,
        _ => 0.0,
    }
}

fn pitch_weight(a: &BarEvent, b: &BarEvent, shift: Option<i32>) -> f64 {
    let Some(shift) = shift else {
        return 1.0; // rhythm only
    };
    match (a.muted, b.muted) {
        (true, true) => 1.0,
        (true, false) | (false, true) => 0.25,
        _ => {
            let common = a
                .pitches
                .iter()
                .filter(|&&p| b.pitches.contains(&(p + shift)))
                .count();
            let union = a.pitches.len() + b.pitches.len() - common;
            if union == 0 {
                1.0
            } else {
                common as f64 / union as f64
            }
        }
    }
}

/// Align two bars' events (in order) to the most total likeness; `shift` moves a's pitches (None: rhythm only).
pub fn match_bars(a: &Fingerprint, b: &Fingerprint, shift: Option<i32>) -> BarMatch {
    let (na, nb) = (a.events.len(), b.events.len());
    if na == 0 && nb == 0 {
        return BarMatch {
            score: 1.0,
            pairs: vec![],
        };
    }
    if na == 0 || nb == 0 {
        return BarMatch {
            score: 0.0,
            pairs: vec![],
        };
    }
    let w = |i: usize, j: usize| {
        let (ea, eb) = (&a.events[i], &b.events[j]);
        let t = time_weight(ea.pos - eb.pos);
        if t == 0.0 {
            0.0
        } else {
            t * pitch_weight(ea, eb, shift)
        }
    };
    // dp[i][j]: best total for a[..i], b[..j]
    let mut dp = vec![vec![0.0f64; nb + 1]; na + 1];
    for i in 1..=na {
        for j in 1..=nb {
            dp[i][j] = dp[i - 1][j]
                .max(dp[i][j - 1])
                .max(dp[i - 1][j - 1] + w(i - 1, j - 1));
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (na, nb);
    while i > 0 && j > 0 {
        let here = w(i - 1, j - 1);
        if here > 0.0 && (dp[i][j] - (dp[i - 1][j - 1] + here)).abs() < 1e-12 {
            pairs.push((i - 1, j - 1, here));
            i -= 1;
            j -= 1;
        } else if (dp[i][j] - dp[i - 1][j]).abs() < 1e-12 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    pairs.reverse();
    BarMatch {
        score: dp[na][nb] / na.max(nb) as f64,
        pairs,
    }
}

/// How alike two bars are, 0..1 (same notes at the same places = 1).
pub fn bar_similarity(a: &Fingerprint, b: &Fingerprint) -> f64 {
    match_bars(a, b, Some(0)).score
}

/// The best likeness with a's pitches moved by up to an octave either way, and that shift (b = a + shift); the
/// smallest shift wins a tie.
pub fn bar_similarity_transposed(a: &Fingerprint, b: &Fingerprint) -> (f64, i32) {
    let mut best = (match_bars(a, b, Some(0)).score, 0);
    for d in 1..=12 {
        for s in [d, -d] {
            let sc = match_bars(a, b, Some(s)).score;
            if sc > best.0 + 1e-9 {
                best = (sc, s);
            }
        }
    }
    best
}

/// The rhythm alone (positions, not pitches).
pub fn rhythm_similarity(a: &Fingerprint, b: &Fingerprint) -> f64 {
    match_bars(a, b, None).score
}

/// Two stretches compared bar by bar.
#[derive(Clone, Debug, PartialEq)]
pub struct StretchCompare {
    /// 0..1: the aligned bars' likeness over the longer stretch's bar count.
    pub score: f64,
    /// (bar of a, bar of b, likeness); None where one stretch has a bar the other hasn't.
    pub pairs: Vec<(Option<usize>, Option<usize>, f64)>,
}

impl StretchCompare {}

impl Bars {
    pub fn new(arr: &Arrangement) -> Bars {
        let bars = make_bars(arr);
        let groups = build_groups(arr);
        let mut prints: Vec<Fingerprint> = bars
            .iter()
            .map(|b| Fingerprint {
                beats: b.beats.len(),
                events: vec![],
            })
            .collect();
        let starts: Vec<f64> = bars.iter().map(|b| b.t0).collect();
        for g in &groups {
            // the bar this event is in (one starting just after it counts: a note pushed a hair early)
            let k = starts.partition_point(|&t0| t0 <= g.t + EARLY_EPS);
            if k == 0 {
                continue;
            }
            let bar = &bars[k - 1];
            if g.t >= bar.t1 - EARLY_EPS && k == bars.len() {
                continue; // past the last bar
            }
            let mut pitches = g.midis.clone();
            pitches.sort_unstable();
            pitches.dedup();
            prints[k - 1].events.push(BarEvent {
                t: g.t,
                pos: position(bar, g.t),
                pitches,
                muted: g.muted,
            });
        }
        Bars { bars, prints }
    }

    /// Bars whose start is in [a, b) or that hold an event in it.
    pub fn in_span(&self, a: f64, b: f64) -> Range<usize> {
        let lo = self
            .bars
            .iter()
            .position(|bar| {
                bar.t1 > a + 1e-6 && (bar.t0 >= a - EARLY_EPS || self.has_event_in(bar.index, a, b))
            })
            .unwrap_or(self.bars.len());
        let hi = self
            .bars
            .iter()
            .rposition(|bar| bar.t0 < b - EARLY_EPS || self.has_event_in(bar.index, a, b))
            .map_or(lo, |i| i + 1);
        lo..hi.max(lo)
    }

    fn has_event_in(&self, i: usize, a: f64, b: f64) -> bool {
        self.prints[i]
            .events
            .iter()
            .any(|e| e.t >= a - EARLY_EPS && e.t < b - EARLY_EPS)
    }

    /// The bar holding chart time t.
    pub fn at(&self, t: f64) -> Option<usize> {
        let k = self.bars.partition_point(|b| b.t0 <= t + EARLY_EPS);
        (k > 0 && t < self.bars[k - 1].t1).then(|| k - 1)
    }

    /// Bars compared in order with the best alignment (a bar may be left out on either side). With `transposed`,
    /// each pair is compared at its best shift.
    pub fn compare(&self, a: Range<usize>, b: Range<usize>, transposed: bool) -> StretchCompare {
        self.compare_prints(
            &self.span_prints(a.clone(), None),
            &self.span_prints(b.clone(), None),
            a,
            b,
            transposed,
        )
    }

    /// As `compare`, for two chart spans (bar fingerprints clipped to the spans).
    pub fn compare_spans(&self, a: (f64, f64), b: (f64, f64), transposed: bool) -> StretchCompare {
        let (ra, rb) = (self.in_span(a.0, a.1), self.in_span(b.0, b.1));
        self.compare_prints(
            &self.span_prints(ra.clone(), Some(a)),
            &self.span_prints(rb.clone(), Some(b)),
            ra,
            rb,
            transposed,
        )
    }

    fn span_prints(&self, r: Range<usize>, clip: Option<(f64, f64)>) -> Vec<Fingerprint> {
        r.map(|i| match clip {
            Some((a, b)) => self.prints[i].clipped(a, b),
            None => self.prints[i].clone(),
        })
        .collect()
    }

    fn compare_prints(
        &self,
        pa: &[Fingerprint],
        pb: &[Fingerprint],
        ra: Range<usize>,
        rb: Range<usize>,
        transposed: bool,
    ) -> StretchCompare {
        let (na, nb) = (pa.len(), pb.len());
        if na == 0 || nb == 0 {
            let mut pairs: Vec<_> = ra.map(|i| (Some(i), None, 0.0)).collect();
            pairs.extend(rb.map(|j| (None, Some(j), 0.0)));
            return StretchCompare {
                score: if na == nb { 1.0 } else { 0.0 },
                pairs,
            };
        }
        let sim: Vec<Vec<f64>> = pa
            .iter()
            .map(|x| {
                pb.iter()
                    .map(|y| {
                        if transposed {
                            bar_similarity_transposed(x, y).0
                        } else {
                            bar_similarity(x, y)
                        }
                    })
                    .collect()
            })
            .collect();
        // Needleman-Wunsch, no gap cost (a left-out bar just doesn't add), pairs only where likeness > 0
        let mut dp = vec![vec![0.0f64; nb + 1]; na + 1];
        for i in 1..=na {
            for j in 1..=nb {
                dp[i][j] = dp[i - 1][j]
                    .max(dp[i][j - 1])
                    .max(dp[i - 1][j - 1] + sim[i - 1][j - 1]);
            }
        }
        let mut pairs = Vec::new();
        let (mut i, mut j) = (na, nb);
        let ia: Vec<usize> = ra.collect();
        let jb: Vec<usize> = rb.collect();
        while i > 0 || j > 0 {
            if i > 0 && j > 0 && (dp[i][j] - (dp[i - 1][j - 1] + sim[i - 1][j - 1])).abs() < 1e-12 {
                pairs.push((Some(ia[i - 1]), Some(jb[j - 1]), sim[i - 1][j - 1]));
                i -= 1;
                j -= 1;
            } else if i > 0 && (j == 0 || (dp[i][j] - dp[i - 1][j]).abs() < 1e-12) {
                pairs.push((Some(ia[i - 1]), None, 0.0));
                i -= 1;
            } else {
                pairs.push((None, Some(jb[j - 1]), 0.0));
                j -= 1;
            }
        }
        pairs.reverse();
        StretchCompare {
            score: dp[na][nb] / na.max(nb) as f64,
            pairs,
        }
    }

    /// Each bar's group label, in order of first appearance: "A", "B", ...; a bar that is an earlier group moved
    /// by n semitones is "A+5"; a bar with no notes is "-".
    pub fn labels(&self) -> Vec<BarLabel> {
        let mut reps: Vec<usize> = Vec::new(); // representative bar of each group
        let mut out = Vec::with_capacity(self.bars.len());
        for i in 0..self.bars.len() {
            if self.prints[i].events.is_empty() {
                out.push(BarLabel {
                    group: None,
                    shift: 0,
                    similarity: 1.0,
                    like: None,
                });
                continue;
            }
            let mut best: Option<(usize, f64)> = None;
            for (g, &r) in reps.iter().enumerate() {
                let s = bar_similarity(&self.prints[r], &self.prints[i]);
                if s >= SAME && best.is_none_or(|b| s > b.1) {
                    best = Some((g, s));
                }
            }
            if let Some((g, s)) = best {
                out.push(BarLabel {
                    group: Some(g),
                    shift: 0,
                    similarity: s,
                    like: Some(reps[g]),
                });
                continue;
            }
            let mut moved: Option<(usize, f64, i32)> = None;
            for (g, &r) in reps.iter().enumerate() {
                let (s, shift) = bar_similarity_transposed(&self.prints[r], &self.prints[i]);
                if s >= SAME && shift != 0 && moved.is_none_or(|b| s > b.1) {
                    moved = Some((g, s, shift));
                }
            }
            if let Some((g, s, shift)) = moved {
                out.push(BarLabel {
                    group: Some(g),
                    shift,
                    similarity: s,
                    like: Some(reps[g]),
                });
                continue;
            }
            reps.push(i);
            out.push(BarLabel {
                group: Some(reps.len() - 1),
                shift: 0,
                similarity: 1.0,
                like: None,
            });
        }
        out
    }
}

/// A bar's place among the chart's repeating bars.
#[derive(Clone, Debug, PartialEq)]
pub struct BarLabel {
    /// None: no notes in the bar.
    pub group: Option<usize>,
    /// Semitones from the group's first bar (0: the same notes).
    pub shift: i32,
    /// Likeness to the group's first bar.
    pub similarity: f64,
    /// The group's first bar (None for that bar itself).
    pub like: Option<usize>,
}

impl BarLabel {
    pub fn text(&self) -> String {
        match self.group {
            None => "-".into(),
            Some(g) if self.shift == 0 => letters(g),
            Some(g) => format!("{}{:+}", letters(g), self.shift),
        }
    }
}

/// 0 -> "A", 25 -> "Z", 26 -> "AA".
pub fn letters(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn make_bars(arr: &Arrangement) -> Vec<Bar> {
    let beats: Vec<f64> = arr.beats.iter().map(|b| b.time).collect();
    if beats.len() < 2 {
        return vec![];
    }
    let mut downs: Vec<usize> = arr
        .beats
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_downbeat())
        .map(|(i, _)| i)
        .collect();
    if downs.is_empty() {
        downs = (0..beats.len()).step_by(4).collect(); // no measures marked: bars of four
    }
    let last_gap = beats[beats.len() - 1] - beats[beats.len() - 2];
    let mut bars = Vec::new();
    let first_note = arr
        .notes
        .iter()
        .map(|n| n.time)
        .chain(arr.chords.iter().map(|c| c.time))
        .fold(f64::INFINITY, f64::min);
    if downs[0] > 0 && first_note < beats[downs[0]] - EARLY_EPS {
        bars.push(Bar {
            index: 0,
            number: 0,
            t0: beats[0].min(first_note),
            t1: beats[downs[0]],
            beats: beats[..downs[0]].to_vec(),
        });
    }
    for (k, &d) in downs.iter().enumerate() {
        let next = downs.get(k + 1).copied();
        let bt: Vec<f64> = beats[d..next.unwrap_or(beats.len())].to_vec();
        let t1 = match next {
            Some(n) => beats[n],
            None => beats[beats.len() - 1] + last_gap,
        };
        let number = arr.beats[d].measure.max(0);
        bars.push(Bar {
            index: bars.len(),
            number: if arr.beats[d].measure >= 0 {
                number
            } else {
                k as i32 + 1
            },
            t0: beats[d],
            t1,
            beats: bt,
        });
    }
    bars
}

/// Twelfths of a beat from the bar's start, measured on its own beats.
fn position(bar: &Bar, t: f64) -> i32 {
    let n = bar.beats.len();
    if n == 0 || t <= bar.t0 {
        return 0;
    }
    let j = bar.beats.partition_point(|&b| b <= t).max(1) - 1;
    let b0 = bar.beats[j];
    let b1 = bar.beats.get(j + 1).copied().unwrap_or(bar.t1);
    let frac = if b1 > b0 { (t - b0) / (b1 - b0) } else { 0.0 };
    ((j as f64 + frac) * STEPS_PER_BEAT as f64).round() as i32
}

/// A bar of a stretch that nothing learned resembles.
#[derive(Clone, Debug, PartialEq)]
pub struct NovelBar {
    /// The chart's measure number.
    pub number: i32,
    pub t0: f64,
    pub t1: f64,
    /// How much of the bar the most similar learned bar covers, 0..1: its events matched (by position and pitch)
    /// over this bar's event count (0 when nothing was learned). A bar that is part of a learned bar is covered.
    pub best_similarity: f64,
    /// Where that bar starts (None when nothing was learned).
    pub best_at: Option<f64>,
    /// Chart times of the bar's events that bar doesn't have (all of them when nothing was learned).
    pub new_events: Vec<f64>,
}

/// The bars of `span` (chart seconds) that no bar of `learned_spans` (each clipped to its span) covers to
/// `NOVEL_BELOW`, or that are no more than `NOVEL_SPARSE_BELOW` alike (symmetric likeness) to any of them (a
/// sparser version of a learned bar), with the events that make them new. Bars without notes are never new.
pub fn novel_bars(arr: &Arrangement, learned_spans: &[(f64, f64)], span: (f64, f64)) -> Vec<NovelBar> {
    novel_bars_in(&Bars::new(arr), learned_spans, span)
}

/// `novel_bars` with the bars already made.
pub fn novel_bars_in(bars: &Bars, learned_spans: &[(f64, f64)], span: (f64, f64)) -> Vec<NovelBar> {
    let learned: Vec<(usize, Fingerprint)> = learned_spans
        .iter()
        .flat_map(|&(a, b)| bars.in_span(a, b).map(move |i| (i, bars.prints[i].clipped(a, b))))
        .filter(|(_, p)| !p.events.is_empty())
        .collect();
    let mut out = Vec::new();
    for i in bars.in_span(span.0, span.1) {
        let p = bars.prints[i].clipped(span.0, span.1);
        if p.events.is_empty() {
            continue;
        }
        let mut best: Option<(usize, BarMatch)> = None;
        let mut alike = 0.0f64;
        let n = p.events.len() as f64;
        // the part of the bar the span covers (a span starting or ending mid-bar), in twelfths
        let bar = &bars.bars[i];
        let lo = position(bar, span.0.max(bar.t0)) - 1;
        let hi = if span.1 < bar.t1 - EARLY_EPS {
            position(bar, span.1)
        } else {
            i32::MAX
        };
        for (k, (_, q)) in learned.iter().enumerate() {
            let mut m = match_bars(&p, q, Some(0));
            // symmetric likeness over the larger bar's count, the learned bar cut to the same part
            let part = Fingerprint {
                beats: q.beats,
                events: q
                    .events
                    .iter()
                    .filter(|e| e.pos >= lo && e.pos < hi)
                    .cloned()
                    .collect(),
            };
            alike = alike.max(match_bars(&p, &part, Some(0)).score);
            m.score = m.pairs.iter().map(|x| x.2).sum::<f64>() / n; // coverage of this bar
            if best.as_ref().is_none_or(|b| m.score > b.1.score) {
                best = Some((k, m));
            }
        }
        let score = best.as_ref().map_or(0.0, |b| b.1.score);
        if score >= NOVEL_BELOW && alike >= NOVEL_SPARSE_BELOW {
            continue;
        }
        let new_events = p
            .events
            .iter()
            .enumerate()
            .filter(|(e, _)| {
                best.as_ref()
                    .is_none_or(|b| !b.1.pairs.iter().any(|&(x, _, w)| x == *e && w >= 0.5))
            })
            .map(|(_, ev)| ev.t)
            .collect();
        let bar = &bars.bars[i];
        out.push(NovelBar {
            number: bar.number,
            t0: bar.t0,
            t1: bar.t1,
            best_similarity: score,
            best_at: best.map(|b| bars.bars[learned[b.0].0].t0),
            new_events,
        });
    }
    out
}

/// A chart section with its bars.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionBars {
    /// "Verse 2"
    pub label: String,
    pub start: f64,
    pub end: f64,
    pub bars: Range<usize>,
}

/// Sections that repeat: the first is the one to teach, the others with their likeness to it and the bars where
/// they differ (1-based within the section).
#[derive(Clone, Debug, PartialEq)]
pub struct SectionGroup {
    pub first: usize,
    /// (section, likeness, differing bars as 1-based positions in that section).
    pub repeats: Vec<(usize, f64, Vec<usize>)>,
}

/// The chart's sections with their bars (sections with no notes left out).
pub fn sections(arr: &Arrangement, bars: &Bars) -> Vec<SectionBars> {
    arr.sections
        .iter()
        .map(|s| SectionBars {
            label: section_label(s),
            start: s.start,
            end: s.end,
            bars: bars.in_span(s.start, s.end),
        })
        .filter(|s| s.bars.clone().any(|i| bars.has_event_in(i, s.start, s.end)))
        .collect()
}

/// The 1-based positions (in the second stretch) of the bars that differ.
pub fn differing_positions(c: &StretchCompare, b: &Range<usize>) -> Vec<usize> {
    c.pairs
        .iter()
        .filter(|p| p.2 < SAME)
        .filter_map(|p| p.1)
        .map(|j| j - b.start + 1)
        .collect()
}

/// Sections grouped as repeats (likeness at least `SECTION_SAME` to the group's first section).
pub fn section_groups(bars: &Bars, secs: &[SectionBars]) -> Vec<SectionGroup> {
    let mut groups: Vec<SectionGroup> = Vec::new();
    for (i, s) in secs.iter().enumerate() {
        let mut best: Option<(usize, f64, Vec<usize>)> = None;
        for (g, grp) in groups.iter().enumerate() {
            let f = &secs[grp.first];
            let c = bars.compare_spans((f.start, f.end), (s.start, s.end), false);
            if c.score >= SECTION_SAME && best.as_ref().is_none_or(|b| c.score > b.1) {
                best = Some((g, c.score, differing_positions(&c, &s.bars)));
            }
        }
        match best {
            Some((g, score, diff)) => groups[g].repeats.push((i, score, diff)),
            None => groups.push(SectionGroup {
                first: i,
                repeats: vec![],
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_formats::model::{Beat, Chord, Note, Section, Techs};

    /// 4/4 at 120 bpm (bars of 2 s) for `bars` bars.
    fn grid(a: &mut Arrangement, bars: usize) {
        a.beats = (0..bars * 4)
            .map(|i| Beat {
                time: i as f64 * 0.5,
                measure: if i % 4 == 0 { (i / 4 + 1) as i32 } else { -1 },
            })
            .collect();
    }

    /// A boogie bar on the A string from `root_fret` (string 1) at bar `bar`: 8 straight eighths, double stops.
    fn boogie(a: &mut Arrangement, bar: usize, string: i32, root: i32, swing: bool) {
        for k in 0..8 {
            let beat = k / 2;
            let off = if k % 2 == 0 {
                0.0
            } else if swing {
                2.0 / 3.0
            } else {
                0.5
            };
            let t = bar as f64 * 2.0 + (beat as f64 + off) * 0.5;
            let top = [2, 2, 4, 4, 5, 5, 4, 4][k];
            a.chords.push(Chord {
                time: t,
                chord_id: 0,
                name: String::new(),
                notes: vec![Note::new(t, string, root), Note::new(t, string + 1, root + top)],
                techniques: Techs::NONE,
                sustain: 0.0,
                frets: vec![],
                fingers: vec![],
                high_density: false,
            });
        }
    }

    fn twelve_bar() -> Arrangement {
        let mut a = Arrangement::new("Rhythm");
        grid(&mut a, 12);
        // I I I I IV IV I I V IV I I, the I on the low E string (fret 5 = A), IV on the A string (fret 5 = D),
        // V on the A string (fret 7 = E)
        for (bar, (s, f)) in [
            (0, 5),
            (0, 5),
            (0, 5),
            (0, 5),
            (1, 5),
            (1, 5),
            (0, 5),
            (0, 5),
            (1, 7),
            (1, 5),
            (0, 5),
            (0, 5),
        ]
        .into_iter()
        .enumerate()
        {
            boogie(&mut a, bar, s, f, true);
        }
        a
    }

    #[test]
    fn bars_and_positions() {
        let a = twelve_bar();
        let b = Bars::new(&a);
        assert_eq!(b.bars.len(), 12);
        assert_eq!(b.bars[0].number, 1);
        assert_eq!(b.bars[3].t0, 6.0);
        let p = &b.prints[0];
        assert_eq!(p.beats, 4);
        assert_eq!(p.events.len(), 8);
        // shuffle eighths: on the beat and two thirds through it
        let pos: Vec<i32> = p.events.iter().map(|e| e.pos).collect();
        assert_eq!(pos, vec![0, 8, 12, 20, 24, 32, 36, 44]);
        assert_eq!(p.events[0].pitches, vec![45, 52]); // A2 + E3
        assert_eq!(b.at(6.5), Some(3));
        assert_eq!(b.in_span(2.0, 6.0), 1..3);
    }

    #[test]
    fn same_bar_is_1_transposed_bar_found_with_its_shift() {
        let a = twelve_bar();
        let b = Bars::new(&a);
        assert!((bar_similarity(&b.prints[0], &b.prints[1]) - 1.0).abs() < 1e-9);
        // IV: same pattern a 4th up
        let s = bar_similarity(&b.prints[0], &b.prints[4]);
        assert!(s < 0.1, "{s}");
        let (s, shift) = bar_similarity_transposed(&b.prints[0], &b.prints[4]);
        assert!((s - 1.0).abs() < 1e-9);
        assert_eq!(shift, 5);
        assert_eq!(bar_similarity_transposed(&b.prints[0], &b.prints[8]).1, 7);
        assert!((rhythm_similarity(&b.prints[0], &b.prints[8]) - 1.0).abs() < 1e-9);
        let labels: Vec<String> = b.labels().iter().map(BarLabel::text).collect();
        assert_eq!(
            labels,
            ["A", "A", "A", "A", "A+5", "A+5", "A", "A", "A+7", "A+5", "A", "A"]
        );
    }

    #[test]
    fn timing_tolerance_and_swing() {
        let mut a = Arrangement::new("Rhythm");
        grid(&mut a, 3);
        boogie(&mut a, 0, 0, 5, true);
        boogie(&mut a, 1, 0, 5, false); // straight eighths: the off-beats two twelfths early
        boogie(&mut a, 2, 0, 5, true);
        // one note of bar 3 a hair late (a twelfth of a beat)
        let c = a
            .chords
            .iter_mut()
            .find(|c| (c.time - (4.0 + 0.5 * (2.0 / 3.0))).abs() < 1e-6);
        let c = c.unwrap();
        c.time += 0.5 / 12.0;
        for n in &mut c.notes {
            n.time = c.time;
        }
        let b = Bars::new(&a);
        let straight = bar_similarity(&b.prints[0], &b.prints[1]);
        assert!((straight - 0.75).abs() < 1e-9, "{straight}"); // 4 on the beat, 4 at half weight
        let late = bar_similarity(&b.prints[0], &b.prints[2]);
        assert!((late - (7.0 + 0.85) / 8.0).abs() < 1e-9, "{late}");
    }

    #[test]
    fn missing_extra_and_wrong_notes_cost() {
        let mut a = Arrangement::new("Lead");
        grid(&mut a, 2);
        for bar in 0..2 {
            for k in 0..4 {
                let t = bar as f64 * 2.0 + k as f64 * 0.5;
                let fret = if bar == 1 && k == 2 { 6 } else { 5 + k };
                a.notes.push(Note::new(t, 3, fret));
            }
        }
        a.notes.push(Note::new(2.25, 3, 9)); // an extra note in bar 2
        a.notes.sort_by(|x, y| x.time.total_cmp(&y.time));
        let b = Bars::new(&a);
        // bar 2: 3 of 4 the same, one wrong, one extra: 3 / 5
        let s = bar_similarity(&b.prints[0], &b.prints[1]);
        assert!((s - 0.6).abs() < 1e-9, "{s}");
        // empty bars
        assert_eq!(
            bar_similarity(&Fingerprint::default(), &Fingerprint::default()),
            1.0
        );
        assert_eq!(bar_similarity(&b.prints[0], &Fingerprint::default()), 0.0);
        // chords: half the pitches shared
        let x = BarEvent {
            t: 0.0,
            pos: 0,
            pitches: vec![40, 47, 52],
            muted: false,
        };
        let y = BarEvent {
            pitches: vec![40, 47, 53],
            ..x.clone()
        };
        assert!((pitch_weight(&x, &y, Some(0)) - 0.5).abs() < 1e-9);
    }

    /// Verse 1 = verse 2 except one bar; a chorus is different; a verse 3 with an extra bar still lines up.
    #[test]
    fn sections_grouped_with_their_differences() {
        let mut a = Arrangement::new("Rhythm");
        grid(&mut a, 16);
        let verse = |a: &mut Arrangement, start_bar: usize, n: usize, odd: Option<usize>| {
            for k in 0..n {
                let (s, f) = if Some(k) == odd { (1, 7) } else { (0, 5) };
                boogie(a, start_bar + k, s, f, true);
            }
        };
        verse(&mut a, 0, 4, None);
        // chorus: whole-note chords
        for bar in 4..6 {
            let t = bar as f64 * 2.0;
            a.chords.push(Chord {
                time: t,
                chord_id: 1,
                name: "D".into(),
                notes: vec![Note::new(t, 3, 2), Note::new(t, 4, 3), Note::new(t, 5, 2)],
                techniques: Techs::NONE,
                sustain: 1.9,
                frets: vec![],
                fingers: vec![],
                high_density: false,
            });
        }
        verse(&mut a, 6, 4, Some(2));
        verse(&mut a, 10, 5, None);
        a.sections = vec![
            Section {
                name: "verse".into(),
                number: 1,
                start: 0.0,
                end: 8.0,
            },
            Section {
                name: "chorus".into(),
                number: 1,
                start: 8.0,
                end: 12.0,
            },
            Section {
                name: "verse".into(),
                number: 2,
                start: 12.0,
                end: 20.0,
            },
            Section {
                name: "verse".into(),
                number: 3,
                start: 20.0,
                end: 30.0,
            },
        ];
        let b = Bars::new(&a);
        let secs = sections(&a, &b);
        assert_eq!(secs.len(), 4);
        assert_eq!(secs[0].bars, 0..4);
        let groups = section_groups(&b, &secs);
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(groups[0].first, 0);
        let (s2, score2, diff2) = &groups[0].repeats[0];
        assert_eq!(*s2, 2);
        assert!(*score2 > 0.75 && *score2 < 0.8, "{score2}"); // one bar a V (it shares a note now and then)
        assert_eq!(diff2, &vec![3]);
        let (s3, score3, diff3) = &groups[0].repeats[1];
        assert_eq!(*s3, 3);
        assert!((score3 - 0.8).abs() < 1e-9, "{score3}"); // 4 of 5 bars
        assert_eq!(diff3.len(), 1);
        assert_eq!(groups[1].first, 1);
    }

    #[test]
    fn novel_bars_against_learned_spans() {
        let a = twelve_bar();
        // learned: bars 1-4 (the I); the whole song: the IV and V bars are new, the I bars aren't
        let novel = novel_bars(&a, &[(0.0, 8.0)], (0.0, 24.0));
        let numbers: Vec<i32> = novel.iter().map(|n| n.number).collect();
        assert_eq!(numbers, vec![5, 6, 9, 10]);
        assert_eq!(novel[0].new_events.len(), 8);
        assert!(novel[0].best_similarity < 0.1);
        // learned the IV too: only the V is new
        let novel = novel_bars(&a, &[(0.0, 8.0), (8.0, 12.0)], (0.0, 24.0));
        assert_eq!(novel.iter().map(|n| n.number).collect::<Vec<_>>(), vec![9]);
        // nothing learned: every bar with notes is new
        assert_eq!(novel_bars(&a, &[], (0.0, 24.0)).len(), 12);
        // a span starting mid-bar only counts the notes in it
        let mut a2 = a.clone();
        a2.chords.retain(|c| c.time < 2.0 || c.time >= 4.0);
        let novel = novel_bars(&a2, &[(1.0, 2.0)], (0.0, 2.0));
        assert_eq!(novel.len(), 1);
        assert_eq!(novel[0].new_events.len(), 4, "{novel:?}");
        // the other way round: half a learned bar is covered by it, and half alike (not new)
        assert!(novel_bars(&a, &[(0.0, 2.0)], (1.0, 2.0)).is_empty());
        // a stop-time bar (two hits of the shuffle bar learned) is covered by it but new to play
        let mut a4 = a.clone();
        a4.chords.retain(|c| {
            c.time < 2.0 || c.time >= 4.0 || (c.time - 2.0).abs() < 1e-9 || (c.time - 3.0).abs() < 1e-9
        });
        let novel = novel_bars(&a4, &[(0.0, 2.0)], (2.0, 4.0));
        assert_eq!(novel.len(), 1, "{novel:?}");
        assert!(novel[0].best_similarity > 0.99 && novel[0].new_events.is_empty());
        // a note pushed a hair ahead of the downbeat stays in its bar when the span starts on the downbeat
        let mut a3 = a.clone();
        for c in a3.chords.iter_mut().filter(|c| (c.time - 2.0).abs() < 1e-9) {
            c.time = 1.99;
            for n in &mut c.notes {
                n.time = 1.99;
            }
        }
        assert!(novel_bars(&a3, &[(0.0, 2.0)], (2.0, 4.0)).is_empty());
    }

    #[test]
    fn letters_go_past_z() {
        assert_eq!(letters(0), "A");
        assert_eq!(letters(25), "Z");
        assert_eq!(letters(26), "AA");
        assert_eq!(letters(27), "AB");
    }

    #[test]
    fn pickup_bar_and_no_measures() {
        let mut a = Arrangement::new("Lead");
        a.beats = (0..9)
            .map(|i| Beat {
                time: i as f64 * 0.5,
                measure: if i >= 1 && (i - 1) % 4 == 0 {
                    (i - 1) / 4 + 1
                } else {
                    -1
                },
            })
            .collect();
        a.notes = vec![Note::new(0.0, 0, 3), Note::new(0.5, 0, 5), Note::new(1.0, 0, 5)];
        let b = Bars::new(&a);
        assert_eq!(b.bars[0].number, 0);
        assert_eq!(b.prints[0].events.len(), 1);
        assert_eq!(b.prints[1].events.len(), 2);
        let mut a = Arrangement::new("Lead");
        a.beats = (0..9)
            .map(|i| Beat {
                time: i as f64 * 0.5,
                measure: -1,
            })
            .collect();
        assert_eq!(Bars::new(&a).bars.len(), 3);
    }
}
