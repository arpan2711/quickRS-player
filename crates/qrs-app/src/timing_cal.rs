//! The timing tool's maths (P36.1, D95; new code, no Python source; the setup review's "proposed timing step"):
//! plucks against clicks, a robust estimate of how late they land, whether to trust it, and what to set.
//!
//! The three offsets (config.json, seconds):
//! - `input_offset` (shown as the Guitar / Bass delay, -input_offset, D132): the scorer counts a pluck heard at song time `st` at `st - av_offset +
//!   input_offset` (`chart_pluck`); + counts your playing later.
//! - `display_offset` (Screen): the picture only, `draw_now = position - av_offset + display_offset`; + draws the
//!   notes that much further on (a screen that lags).
//! - `av_offset` (Chart sync): a chart against its own music. The timing tool never sets it.
//!
//! Guitar timing: strum on each click. The clicks are audio, heard on the jam's clock (the engine stamps output
//! blocks with when they're heard and input with when it was played, so device latency it knows of is already out);
//! each pluck's residual is `pluck - click`. What's left is unreported latency + the detector's bias + your feel,
//! and `input_offset = -median` makes your hits count where you feel them. Screen timing: no sound, strum as each
//! gem crosses the line; `display_offset += m_screen - m_guitar` (the feel and the detector cancel).

use serde_json::{Value, json};

/// The timing tool or the note check is reading the plucks (P36.2): an open song (paused under Settings) leaves them
/// to it. Set and cleared by the tool's run (cleared when it's dropped, however the setup is left).
static ONSETS_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn onsets_held() -> bool {
    ONSETS_HELD.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn hold_onsets(on: bool) {
    ONSETS_HELD.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The click's tempo (bpm), the count-in and the clicks that count.
pub const BPM: f64 = 100.0;
pub const COUNT_IN: usize = 4;
pub const SCORED: usize = 16;
/// Clicks that must be hit, of `SCORED`.
pub const NEED: usize = 12;
/// The estimate's 95% interval (the median's: 1.96 x 1.253 x spread / √n) must be within this (s): a 12 ms spread over
/// 16 strums gives ±7.5 ms, a 40 ms one ±25 ms. Ten is a tenth of the scorer's perfect window (D95).
pub const MAX_CI: f64 = 0.010;
/// The offsets' ranges (s).
pub const GUITAR_RANGE: (f64, f64) = (-0.25, 0.25);
pub const SCREEN_RANGE: (f64, f64) = (-0.05, 0.25);

/// The click period (s).
pub fn period() -> f64 {
    60.0 / BPM
}

/// The click times (s from the first click of the count-in): `COUNT_IN + SCORED` of them; the first `COUNT_IN`
/// don't count.
pub fn clicks() -> Vec<f64> {
    (0..COUNT_IN + SCORED).map(|k| k as f64 * period()).collect()
}

/// A pluck further than this from its click is a stray, not a late or early hit (s): under half a period, so a
/// big offset can't wrap round to the next click.
pub fn window(t: f64) -> f64 {
    0.25f64.min(t / 2.0 - 0.05)
}

/// The plucks matched to clicks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Matched {
    /// The first pluck's residual (pluck - click, s) per click hit.
    pub residuals: Vec<f64>,
    /// Plucks after the first on the same click.
    pub doubles: usize,
    /// Plucks near no click (or in the count-in).
    pub strays: usize,
}

/// Match plucks (s, any order) to `clicks` (sorted), skipping the first `skip` clicks (the count-in).
pub fn match_plucks(clicks: &[f64], plucks: &[f64], skip: usize) -> Matched {
    let mut m = Matched::default();
    if clicks.len() < 2 {
        return m;
    }
    let t = clicks[1] - clicks[0];
    let w = window(t);
    let mut ps: Vec<f64> = plucks.iter().copied().filter(|p| p.is_finite()).collect();
    ps.sort_by(f64::total_cmp);
    let mut taken = vec![false; clicks.len()];
    for p in ps {
        // the nearest click
        let i = clicks.partition_point(|c| *c < p);
        let k = [i.checked_sub(1), (i < clicks.len()).then_some(i)]
            .into_iter()
            .flatten()
            .min_by(|a, b| (clicks[*a] - p).abs().total_cmp(&(clicks[*b] - p).abs()));
        let Some(k) = k else { continue };
        let r = p - clicks[k];
        if r.abs() >= w || k < skip {
            m.strays += 1;
        } else if taken[k] {
            m.doubles += 1;
        } else {
            taken[k] = true;
            m.residuals.push(r);
        }
    }
    m
}

fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// The robust spread: 1.4826 x the median absolute deviation (a normal distribution's sigma).
fn spread(v: &[f64], m: f64) -> f64 {
    let d: Vec<f64> = v.iter().map(|x| (x - m).abs()).collect();
    1.4826 * median(&d)
}

/// A robust estimate of a set of residuals.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fit {
    /// The median residual (s; + late) and the spread (s).
    pub offset: f64,
    pub spread: f64,
    /// Residuals kept, and dropped as outliers.
    pub n: usize,
    pub outliers: usize,
    /// The estimate's 95% interval (s, ±): 1.96 x 1.253 x spread / √n (the median's standard error).
    pub ci: f64,
}

/// Median and spread, outliers (over max(3 spread, 60 ms) off) dropped, then again on the rest.
pub fn fit(residuals: &[f64]) -> Fit {
    if residuals.is_empty() {
        return Fit::default();
    }
    let m0 = median(residuals);
    let s0 = spread(residuals, m0);
    let lim = (3.0 * s0).max(0.06);
    let kept: Vec<f64> = residuals
        .iter()
        .copied()
        .filter(|r| (r - m0).abs() <= lim)
        .collect();
    let m = median(&kept);
    let s = spread(&kept, m);
    let n = kept.len();
    Fit {
        offset: m,
        spread: s,
        n,
        outliers: residuals.len() - n,
        ci: 1.96 * 1.253 * s / (n.max(1) as f64).sqrt(),
    }
}

/// Why a run can't be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Too few clicks hit (n of `SCORED`).
    Missed,
    /// The strums too far apart to say where they land.
    Scattered,
}

/// A run's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub fit: Fit,
    pub doubles: usize,
    pub strays: usize,
    /// None: trusted.
    pub problem: Option<Problem>,
}

impl Run {
    pub fn ok(&self) -> bool {
        self.problem.is_none()
    }

    /// What it says, in a line (the result's words).
    pub fn words(&self) -> String {
        let ms = (self.fit.offset * 1000.0).round();
        let dir = if ms >= 0.0 { "late" } else { "early" };
        match self.problem {
            None => format!(
                "Your strums landed {:.0} ms {dir} on average ({} of {SCORED} clicks, ±{:.0} ms).",
                ms.abs(),
                self.fit.n,
                (self.fit.ci * 1000.0).max(1.0)
            ),
            Some(Problem::Missed) if self.doubles >= 3 => format!(
                "Only {} of {SCORED} clicks counted, and {} strums counted twice: lower the note sensitivity \
                 (Settings > Sound), then try again.",
                self.fit.n, self.doubles
            ),
            Some(Problem::Missed) => format!(
                "Only {} of {SCORED} clicks heard a strum. Strum a little harder, on every click, or raise the \
                 note sensitivity (Settings > Sound), then try again.",
                self.fit.n
            ),
            Some(Problem::Scattered) => format!(
                "The strums were spread too widely to tell ({:.0} ms apart). Strum short and muted, right on the \
                 click, and try again.",
                self.fit.spread * 1000.0
            ),
        }
    }
}

/// Plucks against clicks: the estimate, and whether it's trusted (at least `NEED` clicks hit, its interval within
/// `MAX_CI`).
pub fn measure(clicks: &[f64], plucks: &[f64], skip: usize) -> Run {
    let m = match_plucks(clicks, plucks, skip);
    let f = fit(&m.residuals);
    let problem = if f.n < NEED {
        Some(Problem::Missed)
    } else if f.ci > MAX_CI {
        Some(Problem::Scattered)
    } else {
        None
    };
    Run {
        fit: f,
        doubles: m.doubles,
        strays: m.strays,
        problem,
    }
}

/// Round to 1 ms.
pub fn ms_round(s: f64) -> f64 {
    (s * 1000.0).round() / 1000.0
}

/// Your hits after a guitar run: the plucks land `m` late, so count them `m` earlier (absolute: what the run
/// measured excludes the old `input_offset`).
pub fn guitar_offset(m: f64) -> f64 {
    ms_round((-m).clamp(GUITAR_RANGE.0, GUITAR_RANGE.1))
}

/// The screen's offset after a screen run (`m_screen`, drawn with `old` in place) and a guitar run (`m_guitar`).
pub fn screen_offset(old: f64, m_screen: f64, m_guitar: f64) -> f64 {
    ms_round((old + m_screen - m_guitar).clamp(SCREEN_RANGE.0, SCREEN_RANGE.1))
}

/// When the scorer counts a pluck heard at song time `st` (chart time; `player/mod.rs`'s feed).
pub fn chart_pluck(st: f64, av: f64, io: f64) -> f64 {
    st - av + io
}

/// The chart time the picture shows at song position `pos` (the player's draw).
pub fn draw_now(pos: f64, av: f64, disp: f64) -> f64 {
    pos - av + disp
}

// -- what was measured, and on what ------------------------------------------------------------------------------

/// config.json `timing_cal`: the last run, and the devices it was measured on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    /// "input name -> output name (mode)".
    pub route: String,
    /// Seconds since 1970.
    pub at: f64,
    pub guitar_ms: Option<f64>,
    pub spread_ms: Option<f64>,
    pub n: Option<usize>,
    pub screen_ms: Option<f64>,
}

pub const KEY: &str = "timing_cal";

impl Record {
    pub fn load(cfg: &qrs_library::Config) -> Option<Record> {
        let v = cfg.get(KEY)?;
        let f = |k: &str| v.get(k).and_then(Value::as_f64);
        Some(Record {
            route: v.get("route").and_then(Value::as_str).unwrap_or("").to_string(),
            at: f("measured_at").unwrap_or(0.0),
            guitar_ms: f("m_a"),
            spread_ms: f("s_a"),
            n: v.get("n_a").and_then(Value::as_u64).map(|n| n as usize),
            screen_ms: f("m_v"),
        })
    }

    pub fn save(&self, cfg: &mut qrs_library::Config) {
        let mut v = json!({"route": self.route, "measured_at": self.at.round()});
        for (k, x) in [
            ("m_a", self.guitar_ms),
            ("s_a", self.spread_ms),
            ("m_v", self.screen_ms),
        ] {
            if let Some(x) = x {
                v[k] = json!((x * 10.0).round() / 10.0);
            }
        }
        if let Some(n) = self.n {
            v["n_a"] = json!(n);
        }
        cfg.set(KEY, v);
    }
}

/// Whether timing measured on `saved` should be checked again on `now` (another input or output device, or the
/// output mode; a buffer's own latency is reported and allowed for, so it isn't part of a route).
pub fn timing_stale(saved: &str, now: &str) -> bool {
    !saved.is_empty() && !now.is_empty() && saved != now
}

/// Whether an output's name says it adds a delay no driver reports (Bluetooth, a TV over HDMI).
pub fn slow_output(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "bluetooth",
        "hands-free",
        "handsfree",
        "airpods",
        "buds",
        "headset (",
        "hdmi",
        "nvidia high definition",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// The route as `Record` keeps it.
pub fn route_of(input: &str, output: &str, mode: &str) -> String {
    format!("{input} -> {output} ({mode})")
}

// -- the results card's offer (P2.4) -----------------------------------------------------------------------------

/// After a run of a song: when its hits (each pluck - its note, s) sit consistently one way, the Your hits that
/// would centre them. At least 30 hits, the median over 20 ms off, their spread under 30 ms.
pub fn offer(hit_dts: &[f64]) -> Option<f64> {
    if hit_dts.len() < 30 {
        return None;
    }
    let m = median(hit_dts);
    let s = spread(hit_dts, m);
    (m.abs() > 0.020 && s < 0.030).then(|| ms_round(-m))
}

// -- the note check's diagnosis (P1.3) ---------------------------------------------------------------------------

/// One note of the note check, as it was played.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteResult {
    /// The note expected (MIDI; the lowest of a chord).
    pub expected: i32,
    /// The first pluck in its window (pluck - note, s), None: no pluck counted.
    pub dt: Option<f64>,
    /// Plucks counted in its window.
    pub plucks: usize,
    /// The pitch heard around it (MIDI, fractional), if any.
    pub heard: Option<f64>,
    /// The loudest level around it was under the pluck threshold, but within 10 dB of it.
    pub soft: bool,
}

/// What the note check says about the misses.
#[derive(Clone, Debug, PartialEq)]
pub enum Diagnosis {
    /// Every note heard on time and in tune.
    AllGood,
    /// Nothing heard at all.
    Nothing,
    /// Played, but under the pluck threshold.
    Sensitivity,
    /// A note heard as another: (expected, heard).
    Tuning(i32, f64),
    /// Each note counted twice.
    TooSensitive,
    /// Every pluck the same way off (s, + late): the setup's latency, Apply is `-offset`.
    Offset(f64),
    /// Detection and latency fine; the timing varies note to note.
    Player,
}

// (the note check's own lines; since P38.4 the play-along words its result itself, `playcheck::notes_words`)
#[cfg(test)]
impl Diagnosis {
    /// The line it says.
    pub fn words(&self, bass: bool) -> String {
        let name = |m: f64| qrs_analysis::pitch::note_label(m);
        match self {
            Diagnosis::AllGood => "Every note heard, in tune and on time: quickRS hears you well.".into(),
            Diagnosis::Nothing => {
                "Nothing was heard: check the input (step 1) and play each note as it reaches \
                                   the line."
                    .into()
            }
            Diagnosis::Sensitivity => format!(
                "Played too softly for the note sensitivity: your {} was heard, just under it. Raise Note \
                 sensitivity (back at Your input settings, or Settings > Sound).",
                if bass { "bass" } else { "guitar" }
            ),
            Diagnosis::Tuning(e, h) => {
                let cents = ((h - h.round()) * 100.0).round();
                format!(
                    "Heard {} ({cents:+.0} cents) for {}: check the tuning or the string.",
                    name(h.round()),
                    name(*e as f64)
                )
            }
            Diagnosis::TooSensitive => {
                "Each note counted twice: the note sensitivity is too high, or the input \
                                        is noisy. Lower Note sensitivity a little."
                    .into()
            }
            // (it only reports: the Timing step after it measures the delay and sets it, D132)
            Diagnosis::Offset(o) => format!(
                "Your notes land {:.0} ms {} every time: that's your setup's delay, not you. The next step, \
                 Timing, measures it and sets {} delay.",
                (o * 1000.0).abs(),
                if *o > 0.0 { "late" } else { "early" },
                if bass { "Bass" } else { "Guitar" }
            ),
            Diagnosis::Player => "Detection and latency look fine; the timing varies note to note, which is \
                                  practice."
                .into(),
        }
    }
}

/// The diagnosis of a note check: the first that fits of nothing heard, too soft, doubles, a wrong pitch, a
/// steady offset (6 of 8 the same way, the median over 20 ms), a spread over 35 ms, else all good.
pub fn diagnose(notes: &[NoteResult]) -> Diagnosis {
    let played: Vec<&NoteResult> = notes.iter().filter(|n| n.plucks > 0).collect();
    if played.is_empty() && !notes.iter().any(|n| n.soft || n.heard.is_some()) {
        return Diagnosis::Nothing;
    }
    let missed_soft = notes.iter().filter(|n| n.plucks == 0 && n.soft).count();
    if missed_soft >= 2 || (played.is_empty() && missed_soft > 0) {
        return Diagnosis::Sensitivity;
    }
    if notes.iter().filter(|n| n.plucks >= 2).count() * 2 >= notes.len().max(1) {
        return Diagnosis::TooSensitive;
    }
    // a different note (over 40 cents off; an octave either way is the same note to the scorer)
    for n in notes {
        if let Some(h) = n.heard {
            let d = h - n.expected as f64;
            let d12 = d - 12.0 * (d / 12.0).round();
            if d12.abs() > 0.4 {
                return Diagnosis::Tuning(n.expected, h);
            }
        }
    }
    let dts: Vec<f64> = played.iter().filter_map(|n| n.dt).collect();
    if dts.len() >= 4 {
        let m = median(&dts);
        let same = dts.iter().filter(|d| d.signum() == m.signum()).count();
        let need = (notes.len() * 3).div_ceil(4).min(dts.len());
        if same >= need && m.abs() > 0.020 {
            return Diagnosis::Offset(m);
        }
        if spread(&dts, m) > 0.035 {
            return Diagnosis::Player;
        }
    }
    if played.len() < notes.len() {
        // (missed ones not soft: the player, most likely)
        return Diagnosis::Player;
    }
    Diagnosis::AllGood
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

    fn plucks(off: f64, sd: f64, seed: u64) -> Vec<f64> {
        let mut r = Rng(seed);
        clicks().iter().map(|c| c + off + r.normal(sd)).collect()
    }

    #[test]
    fn a_steady_late_strummer_is_measured() {
        for seed in 1..6 {
            let run = measure(&clicks(), &plucks(0.023, 0.012, seed), COUNT_IN);
            assert!(run.ok(), "{run:?}");
            assert!((run.fit.offset - 0.023).abs() < 0.008, "{}", run.fit.offset);
            assert_eq!(run.fit.n, SCORED);
            assert_eq!(run.strays, COUNT_IN); // (the count-in's strums don't count)
            assert_eq!(guitar_offset(run.fit.offset), ms_round(-run.fit.offset));
        }
        // no jitter: exact
        let run = measure(&clicks(), &plucks(-0.031, 0.0, 1), COUNT_IN);
        assert!(run.ok());
        assert!((run.fit.offset + 0.031).abs() < 1e-9);
        assert!(run.words().contains("31 ms early"));
    }

    #[test]
    fn a_scattered_strummer_is_told_why() {
        let run = measure(&clicks(), &plucks(0.0, 0.040, 7), COUNT_IN);
        assert_eq!(run.problem, Some(Problem::Scattered), "{run:?}");
        assert!(run.words().contains("spread"));
    }

    #[test]
    fn misses_doubles_and_strays_leave_the_estimate_alone() {
        let mut p = plucks(0.023, 0.004, 3);
        // 3 missed (scored clicks 5, 9, 13), 2 doubles 40 ms after their first, 1 stray 250 ms off a click
        for k in [COUNT_IN + 13, COUNT_IN + 9, COUNT_IN + 5] {
            p.remove(k);
        }
        p.push(clicks()[COUNT_IN + 2] + 0.063);
        p.push(clicks()[COUNT_IN + 7] + 0.063);
        p.push(clicks()[COUNT_IN + 10] + 0.29);
        let run = measure(&clicks(), &p, COUNT_IN);
        assert!(run.ok(), "{run:?}");
        assert!((run.fit.offset - 0.023).abs() < 0.003);
        assert_eq!(run.doubles, 2);
        assert_eq!(run.fit.n, SCORED - 3);
        // an outlier inside the window is dropped from the estimate
        let mut q = plucks(0.010, 0.003, 4);
        q[COUNT_IN + 4] += 0.15;
        let run = measure(&clicks(), &q, COUNT_IN);
        assert_eq!(run.fit.outliers, 1);
        assert!((run.fit.offset - 0.010).abs() < 0.003);
        // 6 missed: not enough
        let mut p = plucks(0.023, 0.004, 5);
        for k in (0..6).rev() {
            p.remove(COUNT_IN + 2 * k);
        }
        let run = measure(&clicks(), &p, COUNT_IN);
        assert_eq!(run.problem, Some(Problem::Missed));
        assert!(run.words().contains("10 of 16"));
    }

    #[test]
    fn a_big_offset_does_not_wrap_to_the_next_click() {
        // 0.28 s late at 0.6 s a click: never read as 0.32 s early
        let run = measure(&clicks(), &plucks(0.28, 0.0, 1), COUNT_IN);
        assert!(!run.ok());
        assert_eq!(run.problem, Some(Problem::Missed));
        assert!(run.fit.n == 0);
        assert!((window(period()) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn the_suggestion_lines_plucks_up_whatever_the_chart_sync() {
        let m = 0.037;
        let io = guitar_offset(m);
        for av in [-0.05, 0.0, 0.05] {
            for click in [1.2, 7.8, 33.0] {
                // a note on the click's audio sits at chart time click - av
                let dt = chart_pluck(click + m, av, io) - (click - av);
                assert!(dt.abs() < 0.001, "{dt}");
            }
        }
        assert_eq!(guitar_offset(0.4), -0.25); // (clamped)
    }

    #[test]
    fn the_screen_offset_moves_the_picture_only() {
        let d = screen_offset(0.0, 0.042, 0.010);
        assert!((d - 0.032).abs() < 1e-9);
        // measured again with it in place: the screen run now lands where the guitar one does
        assert!((screen_offset(d, 0.010, 0.010) - 0.032).abs() < 1e-9);
        assert!((draw_now(10.0, 0.0, d) - 10.032).abs() < 1e-9);
        // the scoring doesn't change with it
        assert_eq!(chart_pluck(10.0, 0.0, 0.0), 10.0);
        assert_eq!(screen_offset(0.0, 0.9, 0.0), SCREEN_RANGE.1);
    }

    #[test]
    fn the_record_round_trips_and_a_route_change_is_stale() {
        let mut cfg = qrs_library::Config::default();
        assert!(Record::load(&cfg).is_none());
        let r = Record {
            route: route_of("NUX Audio", "Speakers (Realtek)", "WASAPI shared"),
            at: 1_762_300_000.0,
            guitar_ms: Some(18.0),
            spread_ms: Some(9.4),
            n: Some(15),
            screen_ms: Some(12.0),
        };
        r.save(&mut cfg);
        assert_eq!(Record::load(&cfg), Some(r.clone()));
        assert!(!timing_stale(&r.route, &r.route));
        assert!(timing_stale(
            &r.route,
            &route_of("NUX Audio", "Buds (Bluetooth)", "WASAPI shared")
        ));
        assert!(!timing_stale("", &r.route));
        assert!(slow_output("Headphones (WH-1000XM4 Hands-Free AG Audio)"));
        assert!(slow_output("LG TV (NVIDIA High Definition Audio)"));
        assert!(slow_output("Galaxy Buds2 Pro"));
        assert!(!slow_output("Speakers (Realtek(R) Audio)"));
    }

    #[test]
    fn the_results_card_offers_only_a_steady_offset() {
        let mut r = Rng(9);
        let steady: Vec<f64> = (0..40).map(|_| 0.032 + r.normal(0.010)).collect();
        let o = offer(&steady).unwrap();
        assert!((o + 0.032).abs() < 0.006, "{o}");
        assert_eq!(offer(&steady[..20]), None); // too few
        let wide: Vec<f64> = (0..40).map(|_| 0.032 + r.normal(0.060)).collect();
        assert_eq!(offer(&wide), None);
        let centred: Vec<f64> = (0..40).map(|_| r.normal(0.010)).collect();
        assert_eq!(offer(&centred), None);
    }

    fn note(expected: i32, dt: Option<f64>) -> NoteResult {
        NoteResult {
            expected,
            dt,
            plucks: dt.is_some() as usize,
            heard: dt.map(|_| expected as f64 + 0.03),
            soft: false,
        }
    }
    const NOTES: [i32; 8] = [40, 45, 50, 55, 50, 62, 43, 45];

    #[test]
    fn the_note_check_says_whose_fault_a_miss_is() {
        // all +35 ms: the setup
        let late: Vec<_> = NOTES.iter().map(|&m| note(m, Some(0.035))).collect();
        match diagnose(&late) {
            Diagnosis::Offset(o) => assert!((o - 0.035).abs() < 1e-9),
            d => panic!("{d:?}"),
        }
        assert!(diagnose(&late).words(false).contains("35 ms late"));
        // one wrong pitch: tuning (D#3 for D3)
        let mut tune: Vec<_> = NOTES.iter().map(|&m| note(m, Some(0.004))).collect();
        tune[2].heard = Some(50.98);
        assert_eq!(diagnose(&tune), Diagnosis::Tuning(50, 50.98));
        assert!(
            diagnose(&tune).words(false).contains("for D3"),
            "{}",
            diagnose(&tune).words(false)
        );
        // an octave off isn't a wrong note
        tune[2].heard = Some(62.0);
        assert_eq!(diagnose(&tune), Diagnosis::AllGood);
        // under the threshold: sensitivity
        let mut soft: Vec<_> = NOTES.iter().map(|&m| note(m, Some(0.003))).collect();
        for n in &mut soft[..3] {
            *n = NoteResult {
                expected: n.expected,
                soft: true,
                ..Default::default()
            };
        }
        assert_eq!(diagnose(&soft), Diagnosis::Sensitivity);
        // scattered with the right pitches: the player
        let mut r = Rng(11);
        let wild: Vec<_> = NOTES
            .iter()
            .enumerate()
            .map(|(i, &m)| {
                note(
                    m,
                    Some(if i % 2 == 0 { 1.0 } else { -1.0 } * (0.03 + r.uniform() * 0.06)),
                )
            })
            .collect();
        assert_eq!(diagnose(&wild), Diagnosis::Player);
        // two plucks a note: too sensitive
        let doubles: Vec<_> = NOTES
            .iter()
            .map(|&m| NoteResult {
                plucks: 2,
                ..note(m, Some(0.0))
            })
            .collect();
        assert_eq!(diagnose(&doubles), Diagnosis::TooSensitive);
        // nothing at all
        let none: Vec<_> = NOTES.iter().map(|&m| note(m, None)).collect();
        assert_eq!(diagnose(&none), Diagnosis::Nothing);
        // good
        let good: Vec<_> = NOTES.iter().map(|&m| note(m, Some(0.006))).collect();
        assert_eq!(diagnose(&good), Diagnosis::AllGood);
    }
}
