//! The setup's play-along (P38.4, D159-D160; new code, no Python source; owner: "show the tab and say 'can you
//! play this?'"): a tiny public-domain tune on a tab, played along with a click, that checks quickRS hears every
//! note and measures how late your playing reaches it.
//!
//! The tune is the opening of Beethoven's "Ode to Joy" (public domain), simplified to four bars of quarter notes
//! (the last a half note) at 76 bpm after a bar of click: on a guitar in G on the B and G strings (open strings and
//! frets 1-3), on a bass in C on the D and G strings (and the A string's 3rd fret).
//!
//! What it measures is the timing tool's (D95, `timing_cal.rs`): each pluck read off the jam's clock, matched to the
//! nearest note within min(250 ms, the gap to its neighbour / 2 - 50 ms), only the first pluck a note counts, the
//! median and spread with outliers dropped; trusted with two thirds of the notes heard and the median's 95% interval
//! within 10 ms. It sets the Guitar / Bass delay (`input_offset = -median`). Each note is judged by the scorer (the
//! songs' own: hit, early, late, or a pluck that wasn't the note) and the misses get the note check's diagnosis
//! (P36.4: too soft, doubles, tuning).

use qrs_analysis::Judgement;
use qrs_analysis::instrument::Instrument;
use qrs_formats::model::{Arrangement, Beat, Note};

use crate::timing_cal as tc;

/// The tempo (bpm), the count-in (beats: one bar of click) and the bar.
pub const BPM: f64 = 76.0;
pub const COUNT_IN: usize = 4;
pub const BEATS_PER_BAR: usize = 4;
pub const TUNE_NAME: &str = "Ode to Joy";
pub const TUNE_BY: &str = "Beethoven";

/// The click period (s).
pub fn period() -> f64 {
    60.0 / BPM
}

/// A note of the tune: on beat `beat` (0 = the first after the count-in), `beats` long, on the string tuned to
/// `open` (MIDI) at `fret`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TuneNote {
    pub beat: f64,
    pub beats: f64,
    pub open: i32,
    pub fret: i32,
}

impl TuneNote {
    pub fn midi(&self) -> i32 {
        self.open + self.fret
    }

    /// When it's played (s from the count-in's first click: the jam's clock as the timing tool reads it).
    pub fn time(&self) -> f64 {
        (self.beat + COUNT_IN as f64) * period()
    }
}

/// The melody as scale degrees (1-5) and lengths in beats: "Ode to Joy"'s first phrase, the last bar evened out
/// to quarter, quarter, half.
const MELODY: [(usize, f64); 15] = [
    (3, 1.0),
    (3, 1.0),
    (4, 1.0),
    (5, 1.0),
    (5, 1.0),
    (4, 1.0),
    (3, 1.0),
    (2, 1.0),
    (1, 1.0),
    (1, 1.0),
    (2, 1.0),
    (3, 1.0),
    (3, 1.0),
    (2, 1.0),
    (2, 2.0),
];

/// Where each degree is played, (open string MIDI, fret): a guitar's G A B C D on the G and B strings, a bass's
/// C D E F G on the A, D and G strings. First position, no stretch past the 3rd fret.
fn degrees(inst: Instrument) -> [(i32, i32); 5] {
    if inst.is_bass() {
        [(33, 3), (38, 0), (38, 2), (38, 3), (43, 0)]
    } else {
        [(55, 0), (55, 2), (59, 0), (59, 1), (59, 3)]
    }
}

/// The tune for the instrument.
pub fn tune(inst: Instrument) -> Vec<TuneNote> {
    let deg = degrees(inst);
    let mut beat = 0.0;
    MELODY
        .iter()
        .map(|&(d, len)| {
            let (open, fret) = deg[d - 1];
            let n = TuneNote {
                beat,
                beats: len,
                open,
                fret,
            };
            beat += len;
            n
        })
        .collect()
}

/// The beats the tune takes (after the count-in).
pub fn length_beats(tune: &[TuneNote]) -> f64 {
    tune.last().map(|n| n.beat + n.beats).unwrap_or(0.0)
}

/// Its bars.
pub fn bars(tune: &[TuneNote]) -> usize {
    (length_beats(tune) / BEATS_PER_BAR as f64).ceil() as usize
}

/// The string tuned to `open` among `opens` (low to high), or the nearest below it.
pub fn string_of(opens: &[i32], open: i32) -> usize {
    opens.iter().rposition(|&o| o <= open).unwrap_or(0)
}

/// The tune as a chart for the scorer: E standard (a bass's four strings), notes at `TuneNote::time`, a beat grid
/// from the count-in's first click.
pub fn arrangement(inst: Instrument) -> Arrangement {
    let t = tune(inst);
    let opens: &[i32] = if inst.is_bass() {
        &qrs_analysis::instrument::BASS4
    } else {
        &[40, 45, 50, 55, 59, 64]
    };
    let total = COUNT_IN + length_beats(&t).ceil() as usize;
    let beats = (0..=total)
        .map(|k| Beat {
            time: k as f64 * period(),
            measure: if k % BEATS_PER_BAR == 0 {
                (k / BEATS_PER_BAR) as i32
            } else {
                -1
            },
        })
        .collect();
    let notes = t
        .iter()
        .map(|n| {
            let mut note = Note::new(n.time(), string_of(opens, n.open) as i32, n.fret);
            note.sustain = (n.beats - 0.25) * period();
            note
        })
        .collect();
    Arrangement {
        name: if inst.is_bass() { "Bass" } else { "Lead" }.into(),
        title: TUNE_NAME.into(),
        artist: TUNE_BY.into(),
        tuning: vec![0; 6],
        is_bass: inst.is_bass(),
        song_length: total as f64 * period() + 1.0,
        beats,
        notes,
        mastery: 1.0,
        ..Default::default()
    }
}

/// "Listen first": the tune on the synth guitar (through `tone`) with its count-in and a click on every beat, as
/// interleaved stereo at `rate`; sample 0 is the count-in's first click, so the clip's time is `TuneNote::time`'s.
/// Off the UI thread (it renders the whole tune).
pub fn render(inst: Instrument, rate: u32, tone: Option<qrs_audio::tone::ToneParams>) -> Vec<f32> {
    use qrs_audio::synth::{self, SynthNote};
    let arr = arrangement(inst);
    let opens = arr.open_string_midi();
    let notes: Vec<SynthNote> = arr
        .notes
        .iter()
        .map(|n| SynthNote {
            t: n.time,
            string: n.string,
            fret: n.fret,
            sus: n.sustain,
            tech: qrs_formats::Techs::NONE,
            slide_to: -1,
            bend: 0.0,
            in_chord: false,
        })
        .collect();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let place = |t: f64| t;
    let beats = COUNT_IN + length_beats(&tune(inst)).ceil() as usize;
    let end = beats as f64 * period();
    let o = synth::RenderOptions {
        rate,
        speed: 1.0,
        bass: inst.is_bass(),
        tone,
        looped: false,
        seed: 3,
        place: &place,
        cancel: &cancel,
    };
    let mut out = synth::render(&notes, &opens, 0.0, end, &o).unwrap_or_default();
    // the click: a short tick on every beat, the bar's first higher (the count-in's and the tune's)
    let rf = rate as f64;
    let tick = (0.03 * rf) as usize;
    for k in 0..beats {
        let at = (k as f64 * period() * rf) as usize;
        let hz = if k % BEATS_PER_BAR == 0 { 2000.0 } else { 1500.0 };
        for i in 0..tick {
            let env = (-(i as f64) / (0.006 * rf)).exp();
            let v = (0.22 * env * (std::f64::consts::TAU * hz * i as f64 / rf).sin()) as f32;
            if let Some(s) = out.get_mut(2 * (at + i)..2 * (at + i) + 2) {
                s[0] += v;
                s[1] += v;
            }
        }
    }
    out
}

// -- plucks against the notes --------------------------------------------------------------------------------------

/// How far from note `k` a pluck still belongs to it (s): min(250 ms, the gap to its nearer neighbour / 2 - 50
/// ms), so a big offset can't wrap round onto the next note (`timing_cal::window` for uneven gaps).
pub fn window_at(times: &[f64], k: usize) -> f64 {
    let mut gap = f64::INFINITY;
    if k > 0 {
        gap = gap.min(times[k] - times[k - 1]);
    }
    if k + 1 < times.len() {
        gap = gap.min(times[k + 1] - times[k]);
    }
    if gap.is_finite() { tc::window(gap) } else { 0.25 }
}

/// The plucks matched to the notes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Matched {
    /// Per note: the first pluck's residual (pluck - note, s).
    pub dt: Vec<Option<f64>>,
    /// Per note: plucks inside its window.
    pub plucks: Vec<usize>,
    /// Plucks after the first on a note, and plucks near no note (the count-in's among them).
    pub doubles: usize,
    pub strays: usize,
}

/// Match plucks (s, any order) to note times (sorted).
pub fn match_plucks(times: &[f64], plucks: &[f64]) -> Matched {
    let mut m = Matched {
        dt: vec![None; times.len()],
        plucks: vec![0; times.len()],
        ..Default::default()
    };
    let mut ps: Vec<f64> = plucks.iter().copied().filter(|p| p.is_finite()).collect();
    ps.sort_by(f64::total_cmp);
    for p in ps {
        let i = times.partition_point(|t| *t < p);
        let k = [i.checked_sub(1), (i < times.len()).then_some(i)]
            .into_iter()
            .flatten()
            .min_by(|a, b| (times[*a] - p).abs().total_cmp(&(times[*b] - p).abs()));
        let Some(k) = k else { continue };
        let r = p - times[k];
        if r.abs() >= window_at(times, k) {
            m.strays += 1;
            continue;
        }
        m.plucks[k] += 1;
        if m.dt[k].is_none() {
            m.dt[k] = Some(r);
        } else {
            m.doubles += 1;
        }
    }
    m
}

// -- each note's verdict -------------------------------------------------------------------------------------------

/// What became of a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Heard, on time as the scorer counts it.
    Hit,
    Early,
    Late,
    /// A pluck on time that wasn't the note.
    Wrong,
    /// No pluck near it.
    Missed,
}

impl Mark {
    /// The note was heard as itself.
    pub fn heard(self) -> bool {
        matches!(self, Mark::Hit | Mark::Early | Mark::Late)
    }

    /// The word under the note on the tab.
    pub fn word(self) -> &'static str {
        match self {
            Mark::Hit => "\u{2713}",
            Mark::Early => "early",
            Mark::Late => "late",
            Mark::Wrong => "wrong",
            Mark::Missed => "missed",
        }
    }

    /// The tab's judgement colour for it.
    pub fn judge(self) -> &'static str {
        match self {
            Mark::Hit => "hit",
            Mark::Early => "early",
            Mark::Late => "late",
            Mark::Wrong | Mark::Missed => "miss",
        }
    }
}

/// Whether a pitch heard (fractional MIDI) is another note than `expected` (over 40 cents off; an octave either way
/// is the same note to the scorer).
pub fn other_note(expected: i32, heard: f64) -> bool {
    let d = heard - expected as f64;
    (d - 12.0 * (d / 12.0).round()).abs() > 0.4
}

/// A note's mark from its pluck (`dt`, as matched), the scorer's judgement (None: it didn't judge, as in the
/// screenshots) and the pitch heard after it.
pub fn mark(dt: Option<f64>, scored: Option<Judgement>, expected: i32, heard: Option<f64>) -> Mark {
    if dt.is_none() && !matches!(scored, Some(Judgement::Hit | Judgement::Early | Judgement::Late)) {
        return Mark::Missed;
    }
    match scored {
        Some(Judgement::Hit) => Mark::Hit,
        Some(Judgement::Early) => Mark::Early,
        Some(Judgement::Late) => Mark::Late,
        Some(Judgement::Miss) => Mark::Wrong,
        None if heard.is_some_and(|h| other_note(expected, h)) => Mark::Wrong,
        None => {
            let d = dt.unwrap_or(0.0);
            if d > qrs_analysis::detect::PERFECT {
                Mark::Late
            } else if d < -qrs_analysis::detect::PERFECT {
                Mark::Early
            } else {
                Mark::Hit
            }
        }
    }
}

/// One note as it was played.
#[derive(Clone, Debug, PartialEq)]
pub struct Played {
    pub note: TuneNote,
    /// For the diagnosis: dt (s, as matched, before the delay), plucks, the pitch heard, too soft.
    pub result: tc::NoteResult,
    pub mark: Mark,
}

/// Of `n` notes, how many must be heard to set the timing from them: two thirds (10 of 15).
pub fn need(n: usize) -> usize {
    (2 * n).div_ceil(3)
}

/// A play-along's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub played: Vec<Played>,
    pub fit: tc::Fit,
    pub doubles: usize,
    /// None: the timing is trusted (and set).
    pub problem: Option<tc::Problem>,
    pub diagnosis: tc::Diagnosis,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.problem.is_none()
    }

    /// Notes heard as themselves, of all.
    pub fn heard(&self) -> usize {
        self.played.iter().filter(|p| p.mark.heard()).count()
    }

    pub fn total(&self) -> usize {
        self.played.len()
    }
}

/// The run's verdict: the plucks' timing (every note a pluck landed on, the wrong ones too: it's the timing that's
/// measured), whether it's trusted, and the note check's diagnosis.
pub fn assess(played: Vec<Played>, doubles: usize) -> Outcome {
    let dts: Vec<f64> = played.iter().filter_map(|p| p.result.dt).collect();
    let fit = tc::fit(&dts);
    let problem = if fit.n < need(played.len()) {
        Some(tc::Problem::Missed)
    } else if fit.ci > tc::MAX_CI {
        Some(tc::Problem::Scattered)
    } else {
        None
    };
    let results: Vec<tc::NoteResult> = played.iter().map(|p| p.result.clone()).collect();
    Outcome {
        diagnosis: tc::diagnose(&results),
        played,
        fit,
        doubles,
        problem,
    }
}

/// Screenshots: a run about 18 ms behind with a little jitter, the 4th note (a guitar's D on the B string, a bass's
/// open G) not heard, played too softly.
pub fn example(inst: Instrument) -> Outcome {
    const JITTER: [f64; 15] = [
        0.004, -0.003, 0.006, 0.0, -0.005, 0.002, 0.007, -0.002, 0.001, -0.004, 0.003, 0.005, -0.006, 0.002,
        0.0,
    ];
    let played = tune(inst)
        .iter()
        .enumerate()
        .map(|(k, n)| {
            let dt = (k != 3).then(|| 0.0165 + JITTER[k]);
            let heard = dt.map(|_| n.midi() as f64 + 0.03);
            Played {
                note: *n,
                result: tc::NoteResult {
                    expected: n.midi(),
                    dt,
                    plucks: dt.is_some() as usize,
                    heard,
                    soft: dt.is_none(),
                },
                mark: mark(dt, None, n.midi(), heard),
            }
        })
        .collect();
    assess(played, 0)
}

// -- the words -----------------------------------------------------------------------------------------------------

/// A note's name without its octave ("D", "F#").
fn name(m: i32) -> &'static str {
    qrs_formats::model::note_name(m)
}

/// "the B string", "the low E string" (a guitar's two Es told apart).
pub fn string_words(inst: Instrument, open: i32) -> String {
    let n = name(open);
    if !inst.is_bass() && n == "E" {
        return if open < 50 {
            "the low E string"
        } else {
            "the high E string"
        }
        .into();
    }
    format!("the {n} string")
}

/// "the D on the B string" / "the open G string".
pub fn note_words(inst: Instrument, n: &TuneNote) -> String {
    if n.fret == 0 {
        format!("the open {}", &string_words(inst, n.open)[4..])
    } else {
        format!("the {} on {}", name(n.midi()), string_words(inst, n.open))
    }
}

/// Where to play a note ("fret 3 on the B string", "the open G string").
pub fn where_words(inst: Instrument, n: &TuneNote) -> String {
    if n.fret == 0 {
        format!("the open {}", &string_words(inst, n.open)[4..])
    } else {
        format!("fret {} on {}", n.fret, string_words(inst, n.open))
    }
}

/// The timing line: how late the playing reached quickRS and what was set (`delay`: the new delay's label), or why
/// it couldn't be told.
pub fn timing_words(o: &Outcome, inst: Instrument, delay: &str) -> String {
    let ms = (o.fit.offset * 1000.0).round();
    match o.problem {
        None if ms.abs() < 3.0 => format!("You play right on the beat: {} delay {delay}.", inst.noun_cap()),
        None => format!(
            "You play about {:.0} ms {} the beat: quickRS shifts your timing by {:.0} ms ({} delay {delay}).",
            ms.abs(),
            if ms > 0.0 { "behind" } else { "ahead of" },
            ms.abs(),
            inst.noun_cap()
        ),
        Some(tc::Problem::Missed) => format!(
            "Only {} of {} notes were played near their beat: not enough to set your timing. Play along again, \
             each note as the line reaches it.",
            o.fit.n,
            o.total(),
        ),
        Some(tc::Problem::Scattered) => format!(
            "Your notes were spread too widely to tell ({:.0} ms apart): play along again, right on the click. \
             Your {} delay is left as it was.",
            o.fit.spread * 1000.0,
            inst.noun()
        ),
    }
}

/// The notes line: how many were heard, and what the first problem says to do.
pub fn notes_words(o: &Outcome, inst: Instrument) -> String {
    let (heard, total) = (o.heard(), o.total());
    let first = |f: &dyn Fn(&Played) -> bool| o.played.iter().find(|p| f(p));
    let why = match &o.diagnosis {
        tc::Diagnosis::Nothing => {
            return "Nothing was heard: check the input (Settings > Sound) and play each note as the line reaches \
                    it."
                .into();
        }
        tc::Diagnosis::Sensitivity => first(&|p| p.result.plucks == 0 && p.result.soft).map(|p| {
            format!(
                "{} was quiet: pick a bit harder, or raise Note sensitivity (Settings > Sound).",
                note_words(inst, &p.note)
            )
        }),
        tc::Diagnosis::TooSensitive => {
            Some("notes counted twice: lower Note sensitivity a little (Settings > Sound).".into())
        }
        tc::Diagnosis::Tuning(e, h) => first(&|p| p.note.midi() == *e).map(|p| {
            format!(
                "{} sounded as {} ({:+.0} cents): check your tuning.",
                note_words(inst, &p.note),
                name(h.round() as i32),
                ((h - h.round()) * 100.0).round()
            )
        }),
        _ => None,
    };
    let why = why
        .or_else(|| {
            first(&|p| p.mark == Mark::Wrong).map(|p| {
                format!(
                    "{} didn't sound like a {}: it's {}.",
                    note_words(inst, &p.note),
                    name(p.note.midi()),
                    where_words(inst, &p.note)
                )
            })
        })
        .or_else(|| {
            first(&|p| p.mark == Mark::Missed).map(|p| {
                if p.result.soft {
                    format!(
                        "{} was quiet: pick a bit harder, or raise Note sensitivity (Settings > Sound).",
                        note_words(inst, &p.note)
                    )
                } else {
                    format!("{} wasn't heard.", note_words(inst, &p.note))
                }
            })
        });
    let line = match why {
        None if heard == total => format!("All {total} notes heard: quickRS hears you well."),
        None => format!("{heard} of {total} notes heard."),
        Some(w) => format!("{heard} of {total} notes heard; {w}"),
    };
    inst.say(&line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded normal (Box-Muller on a small LCG): the tests' jitter.
    struct Rng(u64);
    impl Rng {
        fn uniform(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }
        fn normal(&mut self, sd: f64) -> f64 {
            let (u, v) = (self.uniform(), self.uniform());
            sd * (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
        }
    }

    fn times(t: &[TuneNote]) -> Vec<f64> {
        t.iter().map(|n| n.time()).collect()
    }

    /// A run played from plucks: each matched, judged as the scorer would with its pitch right unless `wrong`.
    fn run(t: &[TuneNote], plucks: &[f64], wrong: &[usize], soft: &[usize]) -> Outcome {
        let m = match_plucks(&times(t), plucks);
        let played = t
            .iter()
            .enumerate()
            .map(|(k, n)| {
                let dt = m.dt[k];
                let heard = dt.map(|_| n.midi() as f64 + if wrong.contains(&k) { 2.0 } else { 0.03 });
                let result = tc::NoteResult {
                    expected: n.midi(),
                    dt,
                    plucks: m.plucks[k],
                    heard,
                    soft: dt.is_none() && soft.contains(&k),
                };
                Played {
                    note: *n,
                    mark: mark(dt, None, n.midi(), heard),
                    result,
                }
            })
            .collect();
        assess(played, m.doubles)
    }

    fn plucks(t: &[TuneNote], off: f64, sd: f64, seed: u64) -> Vec<f64> {
        let mut r = Rng(seed);
        times(t).iter().map(|x| x + off + r.normal(sd)).collect()
    }

    #[test]
    fn the_tune_is_small_easy_and_in_reach() {
        for inst in Instrument::ALL {
            let t = tune(inst);
            assert_eq!(t.len(), 15);
            assert_eq!(bars(&t), 4);
            assert_eq!(length_beats(&t), 16.0);
            // first position on strings the instrument has, single notes, in time order
            let opens = inst.open_strings();
            for (k, n) in t.iter().enumerate() {
                assert!(n.fret <= 3, "{inst:?} {n:?}");
                assert!(opens.contains(&n.open), "{inst:?} {n:?}");
                if k > 0 {
                    assert!(n.beat > t[k - 1].beat);
                }
            }
            assert!(t.iter().filter(|n| n.fret == 0).count() >= 4, "open strings");
            // ~76 bpm: the whole thing with the count-in in under 20 s
            assert!(t.last().unwrap().time() + period() < 20.0);
        }
        // a guitar plays G A B C D on the G and B strings: B B C D D C B A G G A B B A A
        let g: Vec<i32> = tune(Instrument::Guitar).iter().map(|n| n.midi()).collect();
        assert_eq!(g, [59, 59, 60, 62, 62, 60, 59, 57, 55, 55, 57, 59, 59, 57, 57]);
        // a bass the same in C, low: E E F G G F E D C C D E E D D
        let b: Vec<i32> = tune(Instrument::Bass).iter().map(|n| n.midi()).collect();
        assert_eq!(b, [40, 40, 41, 43, 43, 41, 40, 38, 36, 36, 38, 40, 40, 38, 38]);
        assert_eq!(
            tune(Instrument::Bass5)
                .iter()
                .map(|n| n.midi())
                .collect::<Vec<_>>(),
            b
        );
    }

    #[test]
    fn the_tunes_chart_scores_like_a_song() {
        for inst in Instrument::ALL {
            let arr = arrangement(inst);
            let t = tune(inst);
            assert_eq!(arr.is_bass, inst.is_bass());
            assert_eq!(arr.notes.len(), t.len());
            let opens = arr.open_string_midi();
            for (n, tn) in arr.notes.iter().zip(&t) {
                assert_eq!(opens[n.string as usize] + n.fret, tn.midi());
                assert!((n.time - tn.time()).abs() < 1e-9);
            }
            // bar lines every 4 beats from the count-in's first click; the tune starts on bar 1
            let downs: Vec<f64> = arr
                .beats
                .iter()
                .filter(|b| b.is_downbeat())
                .map(|b| b.time)
                .collect();
            assert!((downs[1] - t[0].time()).abs() < 1e-9);
            assert!((downs[1] - downs[0] - 4.0 * period()).abs() < 1e-9);
            let groups = qrs_analysis::detect::build_groups(&arr);
            assert_eq!(groups.len(), t.len());
            assert!(groups.iter().zip(&t).all(|(g, n)| g.midis == [n.midi()]));
        }
    }

    #[test]
    fn a_steady_late_player_is_measured_and_told() {
        let t = tune(Instrument::Guitar);
        for seed in 1..6 {
            let o = run(&t, &plucks(&t, 0.018, 0.008, seed), &[], &[]);
            assert!(o.ok(), "{o:?}");
            assert!((o.fit.offset - 0.018).abs() < 0.007, "{}", o.fit.offset);
            assert_eq!(o.heard(), 15);
            assert_eq!(tc::guitar_offset(o.fit.offset), tc::ms_round(-o.fit.offset));
        }
        // exact
        let o = run(&t, &plucks(&t, 0.018, 0.0, 1), &[], &[]);
        assert!((o.fit.offset - 0.018).abs() < 1e-9);
        assert_eq!(
            timing_words(&o, Instrument::Guitar, "18 ms"),
            "You play about 18 ms behind the beat: quickRS shifts your timing by 18 ms (Guitar delay 18 ms)."
        );
        assert_eq!(
            notes_words(&o, Instrument::Guitar),
            "All 15 notes heard: quickRS hears you well."
        );
        let early = run(&t, &plucks(&t, -0.012, 0.0, 1), &[], &[]);
        assert!(timing_words(&early, Instrument::Bass, "\u{2212}12 ms").contains("12 ms ahead of the beat"));
        assert!(timing_words(&early, Instrument::Bass, "\u{2212}12 ms").contains("Bass delay"));
        let on = run(&t, &plucks(&t, 0.002, 0.0, 1), &[], &[]);
        assert!(timing_words(&on, Instrument::Guitar, "2 ms").starts_with("You play right on the beat"));
    }

    #[test]
    fn misses_wrong_notes_doubles_and_strays_leave_the_timing_alone() {
        let t = tune(Instrument::Guitar);
        let tm = times(&t);
        let mut p = plucks(&t, 0.025, 0.006, 3);
        // notes 4 and 9 not played; a double 60 ms after note 2; a stray in the count-in and one between notes
        p.remove(9);
        p.remove(4);
        p.push(tm[2] + 0.085);
        p.push(0.4);
        p.push(tm[6] + 0.39);
        // note 7 played on time, but another note
        let o = run(&t, &p, &[7], &[]);
        assert!(o.ok(), "{o:?}");
        assert!((o.fit.offset - 0.025).abs() < 0.005, "{}", o.fit.offset);
        assert_eq!(o.doubles, 1);
        assert_eq!(o.played[4].mark, Mark::Missed);
        assert_eq!(o.played[7].mark, Mark::Wrong);
        assert_eq!(o.heard(), 12);
        // (the wrong note is a tuning problem to the diagnosis: a whole tone off)
        let w = notes_words(&o, Instrument::Guitar);
        assert!(w.starts_with("12 of 15 notes heard; "), "{w}");
        // an outlier inside a window is dropped
        let mut q = plucks(&t, 0.010, 0.003, 4);
        q[5] += 0.16;
        let o = run(&t, &q, &[], &[]);
        assert_eq!(o.fit.outliers, 1);
        assert!((o.fit.offset - 0.010).abs() < 0.003);
        // a big offset never wraps onto the next note: 0.3 s late at 0.79 s a note is a stray, not 0.49 s early
        let m = match_plucks(&tm, &plucks(&t, 0.30, 0.0, 1));
        assert!(m.dt.iter().all(Option::is_none));
        assert!((window_at(&tm, 0) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn too_few_or_too_scattered_is_not_trusted() {
        let t = tune(Instrument::Bass);
        // 6 of 15 missed: under two thirds
        let mut p = plucks(&t, 0.02, 0.004, 5);
        for k in [13, 11, 9, 7, 5, 3] {
            p.remove(k);
        }
        let o = run(&t, &p, &[], &[]);
        assert_eq!(o.problem, Some(tc::Problem::Missed));
        assert_eq!(need(15), 10);
        let w = timing_words(&o, Instrument::Bass, "");
        assert!(
            w.starts_with("Only 9 of 15 notes were played near their beat"),
            "{w}"
        );
        // wide timing
        let o = run(&t, &plucks(&t, 0.0, 0.045, 7), &[], &[]);
        assert_eq!(o.problem, Some(tc::Problem::Scattered), "{o:?}");
        assert!(timing_words(&o, Instrument::Bass, "").contains("spread too widely"));
    }

    #[test]
    fn the_notes_line_says_what_to_do() {
        let g = Instrument::Guitar;
        let t = tune(g);
        // two quiet misses: the first named, pick harder (note 3 is the D on the B string, note 7 the A on the G)
        let mut p = plucks(&t, 0.01, 0.003, 2);
        p.remove(7);
        p.remove(3);
        let o = run(&t, &p, &[], &[3, 7]);
        assert_eq!(o.diagnosis, tc::Diagnosis::Sensitivity);
        assert_eq!(
            notes_words(&o, g),
            "13 of 15 notes heard; the D on the B string was quiet: pick a bit harder, or raise Note sensitivity \
             (Settings > Sound)."
        );
        // one missed, not quiet
        let mut p = plucks(&t, 0.01, 0.003, 2);
        p.remove(8);
        let o = run(&t, &p, &[], &[]);
        assert_eq!(
            notes_words(&o, g),
            "14 of 15 notes heard; the open G string wasn't heard."
        );
        // a semitone sharp on one: tuning
        let m = match_plucks(&times(&t), &plucks(&t, 0.0, 0.0, 1));
        let played: Vec<Played> = t
            .iter()
            .enumerate()
            .map(|(k, n)| {
                let heard = Some(n.midi() as f64 + if k == 2 { 0.96 } else { 0.0 });
                Played {
                    note: *n,
                    result: tc::NoteResult {
                        expected: n.midi(),
                        dt: m.dt[k],
                        plucks: 1,
                        heard,
                        soft: false,
                    },
                    mark: mark(m.dt[k], None, n.midi(), heard),
                }
            })
            .collect();
        let o = assess(played, 0);
        assert_eq!(
            notes_words(&o, g),
            "14 of 15 notes heard; the C on the B string sounded as C# (-4 cents): check your tuning."
        );
        // the scorer's word wins: a pluck it didn't take for the note is a wrong note
        assert_eq!(
            mark(Some(0.01), Some(Judgement::Miss), 60, Some(60.0)),
            Mark::Wrong
        );
        assert_eq!(mark(Some(0.15), Some(Judgement::Late), 60, None), Mark::Late);
        assert_eq!(mark(None, Some(Judgement::Miss), 60, None), Mark::Missed);
        assert_eq!(mark(Some(-0.12), None, 60, None), Mark::Early);
        // a bass's words
        let b = Instrument::Bass;
        let tb = tune(b);
        let mut p = plucks(&tb, 0.01, 0.003, 2);
        p.remove(8);
        let o = run(&tb, &p, &[], &[]);
        assert_eq!(
            notes_words(&o, b),
            "14 of 15 notes heard; the C on the A string wasn't heard."
        );
        assert_eq!(string_words(g, 40), "the low E string");
        assert_eq!(where_words(g, &t[3]), "fret 3 on the B string");
        // nothing at all
        let o = run(&t, &[], &[], &[]);
        assert_eq!(o.diagnosis, tc::Diagnosis::Nothing);
        assert!(notes_words(&o, g).starts_with("Nothing was heard"));
        // the screenshots' run: one quiet note, 18 ms behind
        let o = example(g);
        assert!(o.ok());
        assert_eq!(
            notes_words(&o, g),
            "14 of 15 notes heard; the D on the B string was quiet: pick a bit harder, or raise Note sensitivity \
             (Settings > Sound)."
        );
        assert!(timing_words(&o, g, "18 ms").starts_with("You play about 18 ms behind the beat"));
        assert!(notes_words(&example(b), b).contains("the open G string was quiet"));
    }

    #[test]
    fn listen_first_plays_the_count_in_and_the_tune() {
        let rate = 24_000;
        let t = tune(Instrument::Guitar);
        let clip = render(Instrument::Guitar, rate, None);
        let at = |s: f64| (s * rate as f64) as usize * 2;
        // at least the count-in and the tune long
        assert!(clip.len() >= at(t.last().unwrap().time() + 2.0 * period()));
        let rms = |a: f64, b: f64| {
            let s = &clip[at(a)..at(b)];
            (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
        };
        // the count-in: clicks and nothing else between them; the tune: a note sounding
        let p = period();
        assert!(rms(0.0, 0.03) > 0.02);
        assert!(rms(0.2, p - 0.05) < 1e-4, "{}", rms(0.2, p - 0.05));
        assert!(rms(t[0].time() + 0.1, t[0].time() + 0.5) > 0.01);
    }
}
