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
pub const BPM: f64 = 133.0;
pub const COUNT_IN: usize = 4;
/// The fewest strums a run counts (it goes on from here until the estimate is good enough, up to `MAX_SCORED`).
pub const SCORED: usize = 16;
/// The most clicks a run goes on for: at 133 bpm about 19 seconds after the count-in.
pub const MAX_SCORED: usize = 42;
/// Clicks that must be hit, of `SCORED`.
pub const NEED: usize = 12;
/// The estimate's 95% interval (the median's: 1.96 x 1.253 x spread / √n) must be within this (s): a 25 ms spread over
/// 16 strums gives ±15 ms, a 40 ms one ±25 ms. Twenty is a fifth of the scorer's perfect window (D95): it was ten,
/// which needed a spread under about 16 ms over 15 notes, tighter than players keep in a song (30-45 ms), so
/// players were turned away again and again, and only a lucky, unusually even run (often one steady habit of
/// playing ahead of or behind the click) was ever accepted.
pub const MAX_CI: f64 = 0.020;
/// A measured offset beyond this (s) is not set by itself: no ordinary device delays a guitar's signal by that much
/// after the engine has taken off what the driver reports, so it is more likely how the run was played (reading
/// the tab ahead of the click, say) than the setup, and a song would score it wrongly (a take of 31 s was 150 ms
/// late after a run of -116 ms set the delay).
pub const SANE_OFFSET: f64 = 0.060;
/// A screen run's result beyond this (s: the screen's own share, what is left of the strums' offset on the dots after
/// the player's own timing on the clicks is taken off) is not set by itself: a screen that lags this much is rare, and
/// most often the dots were struck to the beat of the song's habit, or the guitar's own timing (run 1) is out.
pub const SCREEN_SANE: f64 = 0.120;
/// The offsets' ranges (s).
pub const GUITAR_RANGE: (f64, f64) = (-0.25, 0.25);
pub const SCREEN_RANGE: (f64, f64) = (-0.05, 0.25);

/// How far a measured offset may be before it is held back, on this route ("input -> output (mode)"): a slow output
/// (Bluetooth, a TV: `slow_output`) really does add a delay no driver reports, so it may be as large as the range.
pub fn offset_limit(route: &str) -> f64 {
    if slow_output(route) { GUITAR_RANGE.1 } else { SANE_OFFSET }
}

/// The click period (s).
pub fn period() -> f64 {
    60.0 / BPM
}

/// The click times (s from the first click of the count-in): `COUNT_IN + SCORED` of them; the first `COUNT_IN`
/// don't count.
pub fn clicks() -> Vec<f64> {
    clicks_to(SCORED)
}

/// The click times for a run of up to `scored` clicks that count.
pub fn clicks_to(scored: usize) -> Vec<f64> {
    (0..COUNT_IN + scored).map(|k| k as f64 * period()).collect()
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
    /// Which scored click (0 = the first after the count-in) each of those residuals is of.
    pub hit_clicks: Vec<usize>,
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
            m.hit_clicks.push(k - skip);
        }
    }
    m
}

/// The median of `v` (0 for none).
pub fn median_of(v: &[f64]) -> f64 {
    median(v)
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
    /// Steady, but further off than a setup's delay can be (`SANE_OFFSET`): not set by itself.
    TooFar,
}

/// What to say when a steady run is further off than the route's limit (`offset_limit`): `offset` is its median (s, + late).
pub fn too_far_words(offset: f64) -> String {
    let ms = (offset * 1000.0).abs().round();
    let dir = if offset >= 0.0 { "behind" } else { "ahead of" };
    format!(
        "You played about {ms:.0} ms {dir} the beat: more than a setup normally delays you, so your delay is left \
         as it was. Try again, listening to the click and drums; if your setup really is that slow, set the delay \
         by hand in Fine-tune."
    )
}

/// A run's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub fit: Fit,
    pub doubles: usize,
    pub strays: usize,
    /// Every strum's offset (s, + late: the first on each click it hit, in order), as measured.
    pub residuals: Vec<f64>,
    /// None: trusted.
    pub problem: Option<Problem>,
}

impl Run {
    pub fn ok(&self) -> bool {
        self.problem.is_none()
    }

    /// What it says, in a line (the result's words), for the guitar run (strum on the clicks).
    pub fn words(&self) -> String {
        self.words_for(false)
    }

    /// `words`, for the run `screen` (strum as each dot fills the ring: silent, so no clicks to say anything about).
    pub fn words_for(&self, screen: bool) -> String {
        let ms = (self.fit.offset * 1000.0).round();
        let dir = if ms >= 0.0 { "late" } else { "early" };
        let (things, on) = if screen {
            ("dots", "as each dot fills the ring")
        } else {
            ("clicks", "on every click")
        };
        match self.problem {
            None => format!(
                "Your strums landed {:.0} ms {dir} on average (±{:.0} ms; {} strums, {:.0} ms apart).",
                ms.abs(),
                (self.fit.ci * 1000.0).max(1.0),
                self.fit.n,
                self.fit.spread * 1000.0
            ),
            Some(Problem::Missed) if self.doubles >= 3 => format!(
                "Only {} of {SCORED} {things} counted, and {} strums counted twice: lower the note sensitivity (Settings > Sound), then try again.",
                self.fit.n, self.doubles
            ),
            Some(Problem::Missed) => format!(
                "Only {} of {SCORED} {things} heard a strum. Strum a little harder, {on}, or raise the note sensitivity (Settings > Sound), then try again.",
                self.fit.n
            ),
            Some(Problem::Scattered) => format!(
                "The strums were spread too widely to tell ({:.0} ms apart). Strum short and muted, {}, and try again.",
                self.fit.spread * 1000.0,
                if screen { "the moment each dot fills the ring" } else { "right on the click" }
            ),
            Some(Problem::TooFar) => too_far_words(self.fit.offset),
        }
    }
}

/// What to say when a screen run says the screen lags (+) or runs ahead of (-) the sound by more than `SCREEN_SANE`:
/// `rest` is the screen's share (s).
pub fn screen_too_far_words(rest: f64) -> String {
    let ms = (rest * 1000.0).abs().round();
    let dir = if rest >= 0.0 { "behind" } else { "ahead of" };
    format!(
        "Your screen looks about {ms:.0} ms {dir} the sound: more than a screen usually is, so Screen is left as \
         it was. Strum the moment each dot fills the ring (the clicks are silent here) and try again; if it is \
         really that far off, set Screen by hand in Fine-tune."
    )
}

// -- a run under way: what it says, and when it has enough ---------------------------------------------------------

/// What a run says while the strums come in. Never about early or late: a player told that starts to correct it,
/// and the calibration would then measure the correction and not the setup. Only how steady the strums are and how
/// many more it needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Say {
    Listening,
    /// Three clicks in a row with no strum.
    Missing,
    /// About this many more strums.
    Good(usize),
    LockingIn,
    Uneven,
    Drifting,
    Erratic,
}

/// How a run ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Enough strums, steady and precise enough: judge it now.
    Accept,
    /// Too uneven to ever be precise enough in `MAX_SCORED` clicks: stop and say so (it would only waste time).
    Erratic,
    /// The clicks ran out, or too few strums came: judge it as it stands.
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub say: Say,
    pub verdict: Option<Verdict>,
    /// Strums counted so far, and about how many it will take (at least `SCORED`, at most `MAX_SCORED`).
    pub n: usize,
    pub total: usize,
    pub spread: f64,
}

/// A spread (s) this wide after 12 strums ends the run.
pub const ERRATIC_SPREAD: f64 = 0.080;
/// Beyond this (s) the strums are called a bit uneven (and the run still goes on).
pub const UNEVEN_SPREAD: f64 = 0.060;

/// Whether the latest strums sit away from the earlier ones (a tiring or speeding player): the median of the last 6
/// against the rest, further than 30 ms and further than 3 standard errors.
fn drifting(res: &[f64], spread: f64) -> bool {
    let n = res.len();
    if n < 12 {
        return false;
    }
    let (before, recent) = res.split_at(n - 6);
    let se = spread * 1.25 * (1.0 / 6.0 + 1.0 / before.len() as f64).sqrt();
    (median(recent) - median(before)).abs() > (3.0 * se).max(0.030)
}

/// The run so far: `res` the strums' offsets in order, `hits` the click each is of, `passed` how many scored clicks
/// are over. What it says, and whether it is done.
pub fn track(res: &[f64], hits: &[usize], passed: usize) -> Track {
    let n = res.len();
    let f = fit(res);
    let miss_run = match hits.last() {
        Some(&k) => passed.saturating_sub(k + 1),
        None => passed,
    };
    let need = if n >= 4 {
        ((1.96 * 1.253 * f.spread / MAX_CI).powi(2)).ceil() as usize
    } else {
        SCORED
    };
    let total = need.clamp(SCORED, MAX_SCORED).max(n);
    // (the estimate has stopped moving: without the last 4 strums it is within 10 ms)
    let settled = n < 5 || (fit(&res[..n - 4]).offset - f.offset).abs() < 0.010;
    let drift = drifting(res, f.spread);
    let (say, mut verdict) = if n >= SCORED && f.ci <= MAX_CI && settled && !drift {
        (Say::LockingIn, Some(Verdict::Accept))
    } else if (n >= 12 && f.spread > ERRATIC_SPREAD) || (n >= SCORED && need > MAX_SCORED) {
        (Say::Erratic, Some(Verdict::Erratic))
    } else if miss_run >= 3 {
        (Say::Missing, None)
    } else if n < 6 {
        (Say::Listening, None)
    } else if drift {
        (Say::Drifting, None)
    } else if f.spread > UNEVEN_SPREAD {
        (Say::Uneven, None)
    } else if total > n {
        (Say::Good(total - n), None)
    } else {
        (Say::LockingIn, None)
    };
    if verdict.is_none() && (passed >= MAX_SCORED || (passed >= 24 && n < 10)) {
        verdict = Some(Verdict::Out);
    }
    Track {
        say,
        verdict,
        n,
        total,
        spread: f.spread,
    }
}

impl Say {
    /// The words (`screen`: the run is silent, the strums are to the dots).
    pub fn words(self, screen: bool) -> String {
        match self {
            Say::Listening => "Listening…".into(),
            Say::Missing => {
                if screen {
                    "Strum with every dot.".into()
                } else {
                    "Strum on every click.".into()
                }
            }
            Say::Good(more) if more >= 2 => format!("Good timing! About {more} more."),
            Say::Good(_) => "Good timing! Almost there.".into(),
            Say::LockingIn => "Locking in…".into(),
            Say::Uneven => "A little uneven: short and steady, keep going.".into(),
            Say::Drifting => "Your timing is drifting…".into(),
            Say::Erratic => "Timing too erratic to pin down: try again.".into(),
        }
    }
}

/// Plucks against clicks: the estimate, and whether it's trusted (at least `NEED` clicks hit, its interval within
/// `MAX_CI`, its size within `SANE_OFFSET`).
pub fn measure(clicks: &[f64], plucks: &[f64], skip: usize) -> Run {
    measure_for(clicks, plucks, skip, SANE_OFFSET)
}

/// `measure`, with the size limit of the route (`offset_limit`).
pub fn measure_for(clicks: &[f64], plucks: &[f64], skip: usize, limit: f64) -> Run {
    let m = match_plucks(clicks, plucks, skip);
    let f = fit(&m.residuals);
    let problem = if f.n < NEED {
        Some(Problem::Missed)
    } else if f.ci > MAX_CI {
        Some(Problem::Scattered)
    } else if f.offset.abs() > limit {
        Some(Problem::TooFar)
    } else {
        None
    };
    Run {
        fit: f,
        doubles: m.doubles,
        strays: m.strays,
        residuals: m.residuals,
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

/// After a take (the song's notes, played to what you hear): when the timed notes sit one way, the amount to add to
/// `input_offset` (s) to centre them. At least 30 timed notes, the
/// median 15 ms or more off (below that it is the player's feel, and inside the noise), the spread (standard
/// deviation, as the report gives it) under 70 ms. A take is a better witness than the calibrator: 30-60 notes of
/// the actual song, played as you play it.
pub fn take_offer(n: usize, median_ms: f64, spread_ms: f64) -> Option<f64> {
    (n >= 30 && median_ms.abs() >= 15.0 && spread_ms < 70.0).then(|| ms_round(-median_ms / 1000.0))
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
    fn an_ordinary_human_spread_is_accepted() {
        // 20 ms apart is a steady player (a song's takes run 30-45 ms): it used to be turned away
        for seed in 1..8 {
            let run = measure(&clicks(), &plucks(-0.012, 0.020, seed), COUNT_IN);
            assert!(run.ok(), "seed {seed}: {run:?}");
            assert!(run.fit.ci <= MAX_CI);
            // the words say how sure it is
            assert!(run.words().contains("ms early") && run.words().contains('±'), "{}", run.words());
        }
    }

    #[test]
    fn a_steady_but_enormous_offset_is_held_back_not_set() {
        // -116 ms from a steady player: not a device delay, so nothing is set
        let run = measure(&clicks(), &plucks(-0.116, 0.008, 3), COUNT_IN);
        assert_eq!(run.problem, Some(Problem::TooFar), "{run:?}");
        assert!(!run.ok());
        let w = run.words();
        assert!(w.contains(" ms ahead of the beat") && w.contains("about 11"), "{w}");
        assert!(w.contains("left as it was"), "{w}");
        // just inside the limit is set
        assert!(measure(&clicks(), &plucks(0.055, 0.008, 3), COUNT_IN).ok());
        assert!(measure(&clicks(), &plucks(-0.055, 0.008, 3), COUNT_IN).ok());
        // a slow output (Bluetooth) is allowed its real delay
        let slow = offset_limit("Guitar -> Headphones (WH-1000XM4 Hands-Free AG Audio) (WASAPI shared)");
        assert!(measure_for(&clicks(), &plucks(0.150, 0.008, 3), COUNT_IN, slow).ok());
        assert_eq!(offset_limit("Katana -> Speakers (Realtek(R) Audio) (WASAPI exclusive)"), SANE_OFFSET);
    }

    #[test]
    fn the_screen_run_never_talks_about_clicks() {
        // the screen run is silent: its words are about the dots, whatever went wrong
        for run in [
            measure(&clicks(), &plucks(0.0, 0.040, 7), COUNT_IN),
            measure(&clicks(), &plucks(0.012, 0.004, 3)[..10], COUNT_IN),
        ] {
            let w = run.words_for(true);
            assert!(!w.contains("click"), "{w}");
            assert!(w.contains("dot"), "{w}");
            // (and the guitar run's own words are as they were)
            assert!(run.words().contains("click"), "{}", run.words());
        }
        let w = screen_too_far_words(0.15);
        assert!(w.contains("150 ms behind the sound") && w.contains("silent"), "{w}");
        assert!(!w.contains("listening") && !w.contains("drums"), "{w}");
        assert!(screen_too_far_words(-0.15).contains("ahead of the sound"));
    }

    /// A run of `strums` clicks played with `off` + `sd` jitter, `shift` added from strum `shift_at` on: the tracker
    /// after every click, until it says it is done. (The strums so far, what it said last, its verdict.)
    fn simulate(off: f64, sd: f64, seed: u64, shift: Option<(usize, f64)>) -> (usize, Say, Option<Verdict>) {
        let mut r = Rng(seed);
        let (mut res, mut hits) = (Vec::new(), Vec::new());
        let mut last = track(&res, &hits, 0);
        for j in 0..MAX_SCORED {
            let extra = shift.map_or(0.0, |(at, d)| if j >= at { d } else { 0.0 });
            res.push(off + extra + r.normal(sd));
            hits.push(j);
            last = track(&res, &hits, j + 1);
            if last.verdict.is_some() {
                return (j + 1, last.say, last.verdict);
            }
        }
        (MAX_SCORED, last.say, last.verdict)
    }

    #[test]
    fn a_run_goes_on_until_it_is_precise_enough_and_no_longer() {
        // a steady player is done at the minimum
        for seed in 1..9 {
            let (n, _, v) = simulate(-0.027, 0.012, seed, None);
            assert_eq!(v, Some(Verdict::Accept), "seed {seed}");
            assert!((SCORED..=SCORED + 4).contains(&n), "seed {seed}: {n} strums");
        }
        // the owner's 40 ms spread is accepted before the cap: the old fixed 16 turned it away
        for seed in 1..9 {
            let (n, _, v) = simulate(-0.015, 0.040, seed, None);
            assert_eq!(v, Some(Verdict::Accept), "seed {seed}: {n} strums");
            assert!(n <= MAX_SCORED, "seed {seed}: {n} strums");
        }
        // the estimate it ends with is the right one, for everyone it accepts
        let mut r = Rng(5);
        let (mut res, mut hits) = (Vec::new(), Vec::new());
        for j in 0..MAX_SCORED {
            res.push(0.031 + r.normal(0.035));
            hits.push(j);
            if track(&res, &hits, j + 1).verdict == Some(Verdict::Accept) {
                break;
            }
        }
        assert!((fit(&res).offset - 0.031).abs() < 0.025, "{}", fit(&res).offset);
    }

    #[test]
    fn a_hopeless_run_stops_early_and_says_so() {
        // 100 ms apart: no 42 clicks would make it precise: it ends at 12 strums, not 42
        for seed in 1..6 {
            let (n, say, v) = simulate(0.0, 0.100, seed, None);
            assert_eq!(v, Some(Verdict::Erratic), "seed {seed}");
            assert_eq!(say, Say::Erratic);
            assert!(n <= 16, "seed {seed}: {n} strums");
        }
        // 60 ms apart would need 55 strums: stopped at 16 instead of making the player strum 42
        let (n, _, v) = simulate(0.0, 0.065, 3, None);
        assert_eq!(v, Some(Verdict::Erratic));
        assert!(n <= 20, "{n}");
    }

    #[test]
    fn the_words_follow_the_strums_and_never_name_a_direction() {
        // the first strums: listening
        let t = track(&[0.01, -0.02, 0.0], &[0, 1, 2], 3);
        assert_eq!(t.say, Say::Listening);
        // nothing for three clicks
        let t = track(&[0.01, -0.02, 0.0, 0.01, 0.0, 0.02, 0.01], &[0, 1, 2, 3, 4, 5, 6], 11);
        assert_eq!(t.say, Say::Missing);
        // a drift: 14 steady strums, then six 60 ms later
        let mut res: Vec<f64> = (0..14).map(|k| if k % 2 == 0 { 0.008 } else { -0.008 }).collect();
        res.extend([0.060; 6]);
        let hits: Vec<usize> = (0..res.len()).collect();
        assert_eq!(track(&res, &hits, res.len()).say, Say::Drifting);
        // a first-half-versus-second-half jump in the simulated run shows up too
        let (_, say, _) = simulate(0.0, 0.010, 4, Some((10, 0.07)));
        assert!(matches!(say, Say::Drifting | Say::LockingIn | Say::Good(_) | Say::Uneven | Say::Erratic), "{say:?}");
        // good timing counts down
        let t = track(&[0.004, -0.006, 0.002, 0.0, -0.003, 0.005, -0.001], &[0, 1, 2, 3, 4, 5, 6], 7);
        assert!(matches!(t.say, Say::Good(m) if m >= 2), "{t:?}");
        // no wording says early or late
        for say in [
            Say::Listening,
            Say::Missing,
            Say::Good(5),
            Say::Good(1),
            Say::LockingIn,
            Say::Uneven,
            Say::Drifting,
            Say::Erratic,
        ] {
            for screen in [false, true] {
                let w = say.words(screen).to_lowercase();
                assert!(!w.contains("early") && !w.contains("late") && !w.contains("ahead"), "{w}");
            }
        }
        // the silent run's words are about dots
        assert!(Say::Missing.words(true).contains("dot") && Say::Missing.words(false).contains("click"));
    }

    #[test]
    fn the_clicks_run_out_or_too_few_strums_end_it_as_it_stands() {
        // only a third of the clicks hit: out at 24 clicks, not 42
        let res = vec![0.0, 0.01, -0.01, 0.0, 0.005, 0.0, -0.01, 0.01];
        let hits: Vec<usize> = (0..8).map(|k| k * 3).collect();
        let t = track(&res, &hits, 24);
        assert_eq!(t.verdict, Some(Verdict::Out), "{t:?}");
        // the cap
        let t = track(&[0.0, 0.01, -0.01, 0.0, 0.005, 0.03, 0.04, 0.05, 0.06, 0.07, 0.0, 0.0], &(0..12).collect::<Vec<_>>(), MAX_SCORED);
        assert_eq!(t.verdict, Some(Verdict::Out));
    }

    #[test]
    fn a_take_offers_the_delay_that_centres_it() {
        // 31 s of takes: 56-66 timed notes, 140-190 ms late, as the owner's first take was: 150 ms earlier
        assert_eq!(take_offer(56, 153.0, 33.5), Some(-0.153));
        // early plucks: count them later (+)
        assert_eq!(take_offer(66, -30.0, 43.0), Some(0.030));
        // already centred (a median of -11 ms is feel, not a setup), too few notes, too scattered: no offer
        assert_eq!(take_offer(66, -11.2, 43.0), None);
        assert_eq!(take_offer(20, 80.0, 30.0), None);
        assert_eq!(take_offer(60, 80.0, 90.0), None);
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
        p.push(clicks()[COUNT_IN + 10] + 0.20);
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
        // 0.20 s late at 0.45 s a click: a stray, never read as 0.25 s early on the next click
        let run = measure(&clicks(), &plucks(0.20, 0.0, 1), COUNT_IN);
        assert!(!run.ok());
        assert_eq!(run.problem, Some(Problem::Missed));
        assert!(run.fit.n == 0);
        assert!((window(period()) - (period() / 2.0 - 0.05)).abs() < 1e-12);
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
