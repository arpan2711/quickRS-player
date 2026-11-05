//! Takes (new in quickRS, P17.10-P17.12 (Phase 18's P18.2 and P18.5), D29, D30; no Python source): record a
//! stretch of the part (a count-in, then the stretch once at the speed you're at), analyse it in the background
//! (qrs-analysis take.rs) and show what you played: a result card (the score, the timing, a strip of the bars, the
//! problems in words), "your tab" on the tab (the chart's notes coloured right / wrong / missed, the fret you played
//! instead beside a wrong one, extra notes as ghosts), and the take to listen back to over the song.
//! - Capture: every frame the dry guitar is copied off the capture ring into the take (here, on the UI thread: the
//!   audio thread only writes the ring). Where each sample is in the chart is the scorer's own mapping: every pluck
//!   the engine finds carries the song time it was played at (`Onset::song_time`), so sample 0's chart time is the
//!   median over the take's plucks of `chart - (index - start) / rate * speed`; a take without plucks falls back on
//!   the frame's estimate (the song position less the input latency). Chart time = song time - the sync offset + the
//!   guitar timing, as the scorer counts it.
//! - Each take is saved as `<takes>/<artist - title>/<part>-<start>-<end>-<when>.wav` with the analysis JSON
//!   beside it (`<takes>` is `<data>/takes` unless Settings > Library changed it); the last 10 of a stretch
//!   are kept.
//! - Outside a lesson, Ctrl R records the loop (R alone records what you hear: `crate::recording`) and the console shows the take's card; in a lesson the Record step does
//!   (practise.rs, through `take_result`). B listens back.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use qrs_analysis::take::{Status, TakeInput, TakeJob, TakeReport, TakeTiming, Tendency};
use qrs_analysis::{Judgement, SampleSource};
use qrs_audio::rt::Clip;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{ButtonStyle, button, interact};

use super::tab::TabView;
use super::{LOOP_LEAD_IN, PlayerView};
use crate::library::{caps, txt};
use crate::state::AppState;

/// Seconds of guitar kept after the stretch's end (its last note rings, a late pluck still counts).
const TAIL: f32 = 0.45;
/// Listen back: the song's level under the take.
const SONG_UNDER: f32 = 0.55;
/// A hit further off than this (ms) shows as early / late on the tab.
const OFF_TIME_MS: f64 = 100.0;

// -- capture ----------------------------------------------------------------------------------------------------

/// The guitar of a take, copied off the capture ring frame by frame, and what places it in the chart.
#[derive(Clone, Debug)]
pub struct Capture {
    /// The ring index of sample 0, and of the next sample to copy.
    pub start: u64,
    next: u64,
    pub rate: u32,
    pub samples: Vec<f32>,
    /// Samples the ring had already overwritten when they were due (a very long frame): silence in the take.
    pub lost: u64,
    /// Most samples kept.
    max: usize,
    /// Sample 0's chart time, one estimate per pluck, and the first frame's estimate.
    t0s: Vec<f64>,
    frame_t0: Option<f64>,
}

impl Capture {
    /// From ring index `start`, room for `secs` seconds (more is kept up to twice that).
    pub fn new(start: u64, rate: u32, secs: f64) -> Capture {
        let n = (secs.max(1.0) * rate as f64) as usize;
        Capture {
            start,
            next: start,
            rate,
            samples: Vec::with_capacity(n + rate as usize),
            lost: 0,
            max: 2 * n + rate as usize,
            t0s: Vec::new(),
            frame_t0: None,
        }
    }

    /// Copy what the ring has written since the last call; returns how many samples came in. What the ring has
    /// already overwritten comes in as silence (and counts in `lost`).
    pub fn pull(&mut self, src: &dyn SampleSource) -> usize {
        let w = src.written();
        if w <= self.next || self.samples.len() >= self.max {
            return 0;
        }
        let want = ((w - self.next) as usize).min(self.max - self.samples.len());
        let before = self.samples.len();
        self.samples.resize(before + want, 0.0);
        // what can't be read any more (overwritten) is skipped, the oldest half at a time, and left silent
        let mut skip = 0usize;
        while skip < want {
            if src.read(self.next + skip as u64, &mut self.samples[before + skip..]) {
                break;
            }
            self.samples[before + skip..].fill(0.0);
            skip = (skip + (want - skip) / 2).max(skip + 1);
        }
        self.lost += skip as u64;
        self.next += want as u64;
        want
    }

    /// A pluck the engine found at ring index `index`, played at chart time `chart`, the song at `speed`.
    pub fn pluck(&mut self, index: u64, chart: f64, speed: f64) {
        if index >= self.start {
            self.t0s
                .push(t0_from_pluck(index, self.start, self.rate, chart, speed));
        }
    }

    /// The first frame's estimate of sample 0's chart time (kept for a take without plucks).
    pub fn frame(&mut self, t0: f64) {
        self.frame_t0.get_or_insert(t0);
    }

    /// Where the take is in the chart: sample 0 at the plucks' median estimate (else the frame's), at `speed`.
    pub fn timing(&self, speed: f64) -> TakeTiming {
        let t0 = if self.t0s.is_empty() {
            self.frame_t0.unwrap_or(0.0)
        } else {
            let mut v = self.t0s.clone();
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        TakeTiming::linear(self.rate, t0, speed)
    }
}

/// Sample 0's chart time from a pluck at ring index `index` (the take starting at `start`) played at chart time
/// `chart`, the song at `speed`.
pub fn t0_from_pluck(index: u64, start: u64, rate: u32, chart: f64, speed: f64) -> f64 {
    chart - (index as f64 - start as f64) / rate.max(1) as f64 * speed
}

/// Sample 0's chart time from a frame: the chart time at the now-line `now` (song position - sync offset), the
/// guitar timing `input_offset`, the input's `latency` (s: the newest sample was played that long ago), `written`
/// samples in the ring, the take from `start`.
pub fn t0_from_frame(
    now: f64,
    input_offset: f64,
    latency: f64,
    speed: f64,
    written: u64,
    start: u64,
    rate: u32,
) -> f64 {
    let newest = now + input_offset - latency * speed;
    newest - (written.saturating_sub(1) as f64 - start as f64) / rate.max(1) as f64 * speed
}

// -- a take -----------------------------------------------------------------------------------------------------

/// Where a take is.
pub enum Phase {
    /// Counting in (the song from the stretch's lead-in).
    CountIn,
    Recording,
    /// The stretch is over; the guitar is kept a moment longer (until then).
    Tail(Instant),
    /// The analysis being started (its input handed over on another thread).
    Starting(Receiver<TakeJob>),
    Analysing(Box<TakeJob>),
    Done,
    Failed(String),
}

/// A take of [a, b] (chart seconds) at a speed, from the count-in to its report.
pub struct Session {
    pub a: f64,
    pub b: f64,
    pub speed: f64,
    /// The chart it's taken against, as it was when it started (its id, the part, the scorer's group times for
    /// "your tab"): a level-up or another part during it doesn't change what it's checked against.
    chart_id: u64,
    arr: Arc<qrs_formats::Arrangement>,
    group_times: Vec<f64>,
    /// "Bars 5-11", for the card.
    pub label: String,
    /// The last frame (when), to tell a seek from playing on.
    last_wall: Instant,
    /// Started by a lesson's Record step.
    pub lesson: bool,
    pub phase: Phase,
    cap: Option<Capture>,
    last_now: f64,
    pub report: Option<Arc<TakeReport>>,
    /// What was analysed (the take, mono, and where it is): for saving and listening back.
    input: Option<Arc<TakeInput>>,
    /// "Your tab" over the chart: (scorer group, how it went).
    marks: Vec<(usize, Option<Judgement>)>,
    pub overlay: bool,
    listen: Option<Listen>,
    /// The card's progress readout started (for the "Analysing..." bar's floor).
    pub since: Instant,
    /// Screenshot mode: the analysis' progress shown.
    shot_progress: Option<f32>,
}

/// Listening back: the mix being made, then the clip playing (its id, the chart time it starts at, the speed).
struct Listen {
    rx: Option<Receiver<Clip>>,
    id: u64,
    t0: f64,
    speed: f64,
}

impl Session {
    fn new(a: f64, b: f64, speed: f64, lesson: bool) -> Session {
        Session {
            a,
            b,
            speed,
            chart_id: 0,
            arr: Arc::new(qrs_formats::Arrangement::new("")),
            group_times: Vec::new(),
            label: String::new(),
            last_wall: Instant::now(),
            lesson,
            phase: Phase::CountIn,
            cap: None,
            last_now: f64::NEG_INFINITY,
            report: None,
            input: None,
            marks: Vec::new(),
            overlay: false,
            listen: None,
            since: Instant::now(),
            shot_progress: None,
        }
    }

    /// Whether it's counting in, recording or analysing.
    pub fn going(&self) -> bool {
        matches!(
            self.phase,
            Phase::CountIn | Phase::Recording | Phase::Tail(_) | Phase::Starting(_) | Phase::Analysing(_)
        )
    }

    /// Whether it's counting in or recording (the rec lamp).
    pub fn recording(&self) -> bool {
        matches!(self.phase, Phase::CountIn | Phase::Recording | Phase::Tail(_))
    }

    /// Whether it's being listened back to (or the clip is being made).
    pub fn listening(&self) -> bool {
        self.listen.is_some()
    }
    #[cfg(test)]
    /// The speed it's being listened back at (chart seconds a second), while it is.
    pub fn listen_speed(&self) -> Option<f64> {
        self.listen.as_ref().map(|l| l.speed)
    }

    /// Whether there's a take to listen back to.
    pub fn has_input(&self) -> bool {
        self.input.is_some()
    }

    /// The analysis' progress (0..1) while analysing.
    pub fn progress(&self) -> Option<f32> {
        match &self.phase {
            Phase::Analysing(job) => Some(self.shot_progress.unwrap_or_else(|| job.progress())),
            Phase::Starting(_) => Some(0.0),
            _ => self.shot_progress,
        }
    }
}

/// Whether the chart time at the now-line moved further than playing on could take it since the last frame: a seek
/// forward (or a speed change) in the middle of a take. `wall` seconds passed at `speed`.
pub fn jumped(last: f64, now: f64, wall: f64, speed: f64) -> bool {
    now - last > wall * speed + 0.25
}

/// How each chart note of a report shows on the tab: its scorer group (the one at its time) and a judgement (right
/// in time, early or late, wrong or missed; not checked: none).
pub fn tab_marks(report: &TakeReport, group_times: &[f64]) -> Vec<(usize, Option<Judgement>)> {
    let mut out = Vec::new();
    for e in &report.events {
        let k = group_times.partition_point(|&g| g < e.t - 0.02);
        let Some(&g) = group_times.get(k) else { continue };
        if (g - e.t).abs() > 0.02 {
            continue;
        }
        let j = match e.status {
            Status::Hit => match e.offset_ms {
                Some(o) if o > OFF_TIME_MS => Some(Judgement::Late),
                Some(o) if o < -OFF_TIME_MS => Some(Judgement::Early),
                _ => Some(Judgement::Hit),
            },
            Status::Wrong | Status::Missed => Some(Judgement::Miss),
            Status::NotChecked => None,
        };
        out.push((k, j));
    }
    out
}

/// Listen back: the take (mono, `take_rate`) over the song's stereo `song` (at `song_rate`, the same stretch at the
/// same speed), as one stereo clip at the song's rate. Without the song, the take alone.
pub fn mix(take: &[f32], take_rate: u32, song: Option<&[f32]>, song_rate: u32) -> Vec<f32> {
    let take: Vec<f32> = if take_rate == song_rate || song.is_none() {
        take.to_vec()
    } else {
        qrs_audio::decode::resample(take.to_vec(), 1, take_rate, song_rate).unwrap_or_else(|_| take.to_vec())
    };
    let peak = take.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    // (a quiet DI take brought up to a useful level, never past full scale)
    let gain = if peak > 1e-4 {
        (0.7 / peak).clamp(1.0, 8.0)
    } else {
        1.0
    };
    let frames = song.map_or(take.len(), |s| (s.len() / 2).max(take.len()));
    let mut out = vec![0.0f32; frames * 2];
    for (i, o) in out.chunks_mut(2).enumerate() {
        let g = take.get(i).copied().unwrap_or(0.0) * gain;
        let (l, r) = match song {
            Some(s) => (
                s.get(2 * i).copied().unwrap_or(0.0) * SONG_UNDER,
                s.get(2 * i + 1).copied().unwrap_or(0.0) * SONG_UNDER,
            ),
            None => (0.0, 0.0),
        };
        o[0] = (l + g).clamp(-1.0, 1.0);
        o[1] = (r + g).clamp(-1.0, 1.0);
    }
    out
}

/// The report's line about timing: "on time (median +12 ms)", "rushing: 35 ms early on average".
pub fn timing_words(r: &TakeReport) -> String {
    let tm = &r.timing;
    match tm.tendency {
        Tendency::OnTime => format!("on time (median {:+.0} ms)", tm.median_ms),
        Tendency::Rushing => format!("rushing: {:.0} ms early on average", -tm.median_ms),
        Tendency::Dragging => format!("dragging: {:.0} ms late on average", tm.median_ms),
        Tendency::Unknown => "too few notes to tell the timing".into(),
    }
}

/// A bar's colour by its accuracy: right (90%+), close (70%+), off, or nothing checked.
pub fn bar_tone(acc: Option<f64>) -> u8 {
    match acc {
        Some(a) if a >= 0.9 => 0,
        Some(a) if a >= 0.7 => 1,
        Some(_) => 2,
        None => 3,
    }
}

impl PlayerView {
    // -- recording ---------------------------------------------------------------------------------------------

    /// Start a take of [a, b] at the speed the song is at: the count-in from its lead-in. `lesson`: the Record step's.
    pub(super) fn take_begin(&mut self, a: f64, b: f64, lesson: bool) -> bool {
        if !self.has_input() {
            self.say(
                "A take needs your instrument: ",
                "no input (Settings, F2 > Audio input / output)",
            );
            return false;
        }
        self.take_listen_stop();
        self.stop_synth();
        self.nbn_release(false);
        // (a real count-in from silence, whatever was playing)
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
        let speed = self.speed();
        let mut s = Session::new(a, b, speed, lesson);
        s.chart_id = self.chart.id;
        s.arr = Arc::new(self.chart.arr.clone());
        s.group_times = self.scorer.groups.iter().map(|g| g.t).collect();
        s.label = bars_label(&s.arr, a, b);
        self.take = Some(s);
        self.seek((a - LOOP_LEAD_IN).max(0.0));
        self.count_in();
        self.run_scored = false;
        true
    }

    /// Ctrl R outside a lesson: a take of the loop (or the selection).
    pub(super) fn take_record_loop(&mut self) {
        if self.take.as_ref().is_some_and(Session::going) {
            self.take_abort("Take stopped");
            return;
        }
        let span = self.loop_a.zip(self.loop_b).or(self.tab.sel);
        let Some((a, b)) = span.filter(|(a, b)| b > a) else {
            self.say(
                "Set a loop first: ",
                "[ and ] (or L for the section), then Ctrl R records it",
            );
            return;
        };
        if self.loop_a.is_none() {
            self.loop_a = Some(a);
            self.loop_b = Some(b);
        }
        let nbn = self.nbn;
        if self.take_begin(a, b, false) {
            if nbn {
                // (a take is played through: note by note waits until it's over, D121)
                self.say_for(
                    "Take: ",
                    "the loop once, played through (note by note waits until it's over)  ·  Ctrl R stops it",
                    3.0,
                );
            } else if self.countdown_on {
                self.say("Take: ", "a count-in, then the loop once  ·  Ctrl R stops it");
            } else {
                self.say("Take: ", "the loop once  ·  Ctrl R stops it");
            }
        }
    }

    /// Stop a take under way (nothing kept).
    pub(super) fn take_abort(&mut self, why: &str) {
        if let Some(s) = self.take.take()
            && let Phase::Analysing(job) = &s.phase
        {
            job.cancel();
        }
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
        if let Some(c) = self.coach.as_mut() {
            c.take = super::practise::Take::Ready;
        }
        self.say(why, "");
    }

    /// Every frame with the guitar: the take's guitar copied off the ring, its plucks placed in the chart; the
    /// stretch's end noticed (the song stops; a pause before it stops the take).
    pub(super) fn take_frame(&mut self, e: &qrs_audio::Engine, onsets: &[qrs_audio::rt::Onset]) {
        let now = self.now();
        let playing = self.transport.playing() && self.countdown_until.is_none();
        let shift = self.offset - self.input_offset;
        let latency = e.status().input.as_ref().map(|i| i.latency).unwrap_or(0.0);
        let input_offset = self.input_offset;
        let speed_now = self.speed();
        let chart_id = self.chart.id;
        let src = e.capture();
        let mut stop = false;
        let mut abort: Option<&str> = None;
        {
            let Some(s) = self.take.as_mut() else { return };
            match s.phase {
                Phase::CountIn if playing => {
                    let start = src.written();
                    let mut cap = Capture::new(start, src.rate(), (s.b - s.a) / s.speed + LOOP_LEAD_IN + 2.0);
                    cap.frame(t0_from_frame(
                        now,
                        input_offset,
                        latency,
                        s.speed,
                        start,
                        start,
                        src.rate(),
                    ));
                    s.cap = Some(cap);
                    s.phase = Phase::Recording;
                    s.last_now = now;
                    s.last_wall = Instant::now();
                }
                Phase::Recording | Phase::Tail(_) => {
                    if let Some(cap) = s.cap.as_mut() {
                        cap.pull(src);
                        for o in onsets {
                            if let Some(st_) = o.song_time {
                                cap.pluck(o.index, st_ - shift, s.speed);
                            }
                        }
                    }
                    if matches!(s.phase, Phase::Recording) {
                        let wrapped = now < s.last_now - 0.5;
                        let wall = s.last_wall.elapsed().as_secs_f64();
                        if chart_id != s.chart_id {
                            abort = Some("Take stopped: the part changed");
                        } else if (speed_now - s.speed).abs() > 1e-6 {
                            abort = Some("Take stopped: the speed changed");
                        } else if now >= s.b - 0.005 || ((wrapped || !playing) && s.last_now >= s.b - 0.3) {
                            stop = true;
                        } else if wrapped || !playing {
                            abort = Some(if cfg!(feature = "lessons") {
                                "Take stopped part way: R (or Enter in a lesson) for another"
                            } else {
                                "Take stopped part way: R for another"
                            });
                        } else if jumped(s.last_now, now, wall, s.speed) {
                            abort = Some("Take stopped: the song jumped ahead");
                        }
                        s.last_now = s.last_now.max(now);
                    }
                    s.last_wall = Instant::now();
                }
                _ => {}
            }
        }
        if stop {
            if self.transport.playing() {
                self.transport.pause();
            }
            let a = self.take.as_ref().map(|s| s.a).unwrap_or(0.0);
            self.seek((a - LOOP_LEAD_IN).max(0.0));
            if let Some(s) = self.take.as_mut() {
                s.phase = Phase::Tail(Instant::now() + Duration::from_secs_f32(TAIL));
            }
        } else if let Some(why) = abort {
            self.take_abort(why);
        }
    }

    /// Every frame: a take's tail over, the analysis started; its report in, kept, saved and shown (a lesson's
    /// Record step hears of it); "your tab" on the tab until the song plays again; listening back.
    pub(super) fn take_tick(&mut self, st: &mut AppState) {
        // listening back stops when the song plays
        if (self.transport.playing() || self.countdown_until.is_some())
            && self.take.as_ref().is_some_and(Session::listening)
        {
            self.take_listen_stop();
        }
        self.take_listen_tick();
        let mut report_in: Option<TakeReport> = None;
        let mut failed: Option<&str> = None;
        {
            let artist = self.entry.artist.clone();
            let title = self.entry.display_title();
            let part = self.part_name.clone();
            let pitch = self.mixer.pitch;
            let Some(s) = self.take.as_mut() else { return };
            match &mut s.phase {
                Phase::Tail(until) if Instant::now() >= *until => {
                    let cap = s.cap.take();
                    match cap {
                        Some(cap) if !cap.samples.is_empty() => {
                            let timing = cap.timing(s.speed);
                            // (the take is filed under the song and the part as the player names them)
                            let mut arr = (*s.arr).clone();
                            arr.artist = artist;
                            arr.title = title;
                            arr.name = part;
                            if pitch != 0 {
                                // the song was pitch-shifted: the take is checked against what it sounded like
                                arr.tuning.resize(arr.tuning.len().max(6), 0);
                                for t in &mut arr.tuning {
                                    *t += pitch;
                                }
                            }
                            let input = Arc::new(TakeInput {
                                arr,
                                from: s.a,
                                to: s.b,
                                samples: cap.samples,
                                timing,
                            });
                            s.input = Some(input.clone());
                            // (the analysis gets its own copy, made on its thread: no hitch after a long take)
                            let (tx, rx) = std::sync::mpsc::channel();
                            let started =
                                std::thread::Builder::new()
                                    .name("take-start".into())
                                    .spawn(move || {
                                        let _ = tx.send(TakeJob::start((*input).clone()));
                                    });
                            s.phase = match started {
                                Ok(_) => Phase::Starting(rx),
                                Err(_) => Phase::Failed("the analysis couldn't start".into()),
                            };
                            s.since = Instant::now();
                        }
                        _ => {
                            s.phase = Phase::Failed("nothing was recorded".into());
                            failed = Some("Nothing was recorded: ");
                        }
                    }
                }
                Phase::Starting(rx) => match rx.try_recv() {
                    Ok(job) => s.phase = Phase::Analysing(Box::new(job)),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        s.phase = Phase::Failed("the analysis couldn't start".into());
                        failed = Some("Couldn't analyse the take: ");
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                },
                // (screenshot mode's job is only there to be drawn)
                Phase::Analysing(job) if s.shot_progress.is_none() => {
                    if job.failed() {
                        failed = Some("Couldn't analyse the take: ");
                    } else if job.finished()
                        && let Some(r) = job.take()
                    {
                        report_in = Some(r);
                    }
                }
                _ => {}
            }
        }
        if let Some(c) = self.coach.as_mut()
            && self
                .take
                .as_ref()
                .is_some_and(|s| s.lesson && matches!(s.phase, Phase::Starting(_) | Phase::Analysing(_)))
        {
            c.take = super::practise::Take::Analysing;
        }
        if let Some(why) = failed {
            if let Some(s) = self.take.as_mut()
                && !matches!(s.phase, Phase::Failed(_))
            {
                s.phase = Phase::Failed("the analysis didn't finish".into());
            }
            if let Some(c) = self.coach.as_mut() {
                c.take = super::practise::Take::Ready;
            }
            self.say(why, "try another");
        }
        if let Some(r) = report_in {
            self.take_report(st, r);
        }
        // "your tab" stays until the song plays again
        let playing = self.transport.playing() || self.countdown_until.is_some();
        let (chart_id, groups) = (self.chart.id, self.states.len());
        if let Some(s) = self.take.as_mut()
            && s.overlay
        {
            // (another part or level since: its notes aren't the take's)
            let same = s.chart_id == chart_id && s.group_times.len() == groups;
            if (playing && s.listen.is_none() && !s.going()) || !same {
                s.overlay = false;
            } else {
                for (gi, j) in &s.marks {
                    if let Some(x) = self.states.get_mut(*gi) {
                        *x = *j;
                    }
                }
            }
        }
    }

    /// A take's report: kept, put on the tab, saved, and (a lesson's) handed to the Record step.
    fn take_report(&mut self, st: &mut AppState, r: TakeReport) {
        let Some(s) = self.take.as_mut() else { return };
        let marks = tab_marks(&r, &s.group_times);
        if let Some(input) = s.input.clone() {
            // saved by Phase 18's writer (`<takes>/<artist - title>/`, the last 10 of a stretch), off this thread;
            // the takes folder is `<data>/takes` unless Settings > Library changed it (P23.9)
            let (root, report) = (st.config.takes_dir(&st.data.root), r.clone());
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
            let _ = std::thread::Builder::new()
                .name("take-save".into())
                .spawn(move || {
                    if let Err(e) = qrs_analysis::take::save_take(&root, &input, Some(&report), &stamp) {
                        crate::applog::write(&format!("take: not saved: {e}"));
                    }
                });
        }
        s.marks = marks;
        s.overlay = true;
        s.phase = Phase::Done;
        let (lesson, speed) = (s.lesson, s.speed);
        // (None: nothing in the stretch could be checked, never a pass)
        let acc = r.accuracy_checked();
        let problem = r.problems.first().cloned();
        s.report = Some(Arc::new(r));
        if lesson && self.coach.is_some() {
            let pass = self.coach.as_ref().map(|c| c.riff.pass).unwrap_or(0.9);
            let outcome = super::practise::take_outcome(acc, speed, pass);
            self.take_result(st, outcome);
        } else {
            match acc {
                Some(a) => self.say(
                    format!("Take: {}%  ·  ", (a * 100.0).round() as i64),
                    problem.unwrap_or_else(|| "B listens back".into()),
                ),
                None => self.say("Take: ", "nothing here could be checked"),
            }
        }
    }

    // -- listening back ------------------------------------------------------------------------------------------

    /// B: the take over the song (at the same place, at the speed set now: D120), or stop it.
    pub(super) fn take_listen(&mut self) {
        let Some(s) = self.take.as_ref() else {
            self.say("No take to listen to: ", "record one first");
            return;
        };
        if s.listen.is_some() {
            self.take_listen_stop();
            return;
        }
        if s.input.is_none() {
            self.say("No take to listen to yet", "");
            return;
        }
        self.take_listen_from(None);
        self.say("Listening back: ", "the take over the song  \u{b7}  B stops it");
    }

    /// The speed changed while listening back: again from where it had got to, at the new speed.
    pub(super) fn take_listen_again(&mut self) {
        let at = self.take_listen_pos();
        self.take_listen_stop();
        self.take_listen_from(at);
    }

    /// Listen back from chart time `from` (none: the take's start) at the speed set now: the song's stretch at it,
    /// the take stretched from the speed it was played at to it.
    fn take_listen_from(&mut self, from: Option<f64>) {
        let Some(s) = self.take.as_ref() else { return };
        let Some(input) = s.input.clone() else { return };
        let timing = input.timing.clone();
        let played_at = s.speed;
        let speed = self.speed();
        let n = input.samples.len();
        // the first sample at or after `from` (the take's chart times only go up)
        let i0 = match from {
            Some(t) => {
                let (mut lo, mut hi) = (0usize, n);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if timing.chart_time(mid as f64) < t {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                lo.min(n)
            }
            None => 0,
        };
        let t0 = timing.chart_time(i0 as f64);
        let t1 = timing.chart_time(n as f64);
        let samples: Vec<f32> = input.samples[i0..].to_vec();
        self.stop_synth();
        if self.transport.playing() {
            self.transport.pause();
        }
        let song = self.transport.song().cloned();
        let offset = self.offset;
        let semis = self.mixer.pitch as f32;
        let (tx, rx) = std::sync::mpsc::channel();
        let rate = timing.rate;
        let _ = std::thread::Builder::new()
            .name("take-listen".into())
            .spawn(move || {
                let samples = restretch_take(&samples, rate, speed / played_at);
                let clip = match song {
                    Some(song) => {
                        // (the song's stretch from where the take starts, the take's length long)
                        let (a, b) = (t0 + offset, t1 + offset);
                        let lead = (-a).max(0.0);
                        let mut track = vec![0.0f32; 2 * (lead / speed * song.rate as f64) as usize];
                        track.extend(qrs_audio::synth::track_clip(&song, a.max(0.0), b, speed, semis));
                        Clip {
                            rate: song.rate,
                            samples: mix(&samples, rate, Some(&track), song.rate),
                        }
                    }
                    None => Clip {
                        rate,
                        samples: mix(&samples, rate, None, rate),
                    },
                };
                let _ = tx.send(clip);
            });
        if let Some(s) = self.take.as_mut() {
            s.listen = Some(Listen {
                rx: Some(rx),
                id: 0,
                t0,
                speed,
            });
            s.overlay = s.report.is_some();
        }
    }

    pub(super) fn take_listen_stop(&mut self) {
        let e = self.engine().cloned();
        if let Some(s) = self.take.as_mut()
            && let Some(l) = s.listen.take()
            && l.id > 0
            && let Some(e) = e
        {
            e.stop_preview(false);
        }
    }

    /// The listen-back clip, when made, played; its end noticed.
    fn take_listen_tick(&mut self) {
        let Some(e) = self.engine().cloned() else { return };
        let Some(s) = self.take.as_mut() else { return };
        let Some(l) = s.listen.as_mut() else { return };
        if let Some(rx) = &l.rx {
            if let Ok(clip) = rx.try_recv() {
                l.id = e.play_preview(clip, false, false);
                l.rx = None;
            }
            return;
        }
        if e.preview_state(l.id) == qrs_audio::PreviewState::Over {
            s.listen = None;
        }
    }

    /// Where listening back has got to in the chart (the tab's cursor), if it's playing.
    pub(super) fn take_listen_pos(&self) -> Option<f64> {
        let e = self.engine()?;
        let l = self.take.as_ref()?.listen.as_ref()?;
        match e.preview_state(l.id) {
            qrs_audio::PreviewState::Playing if l.id > 0 => Some(l.t0 + e.preview_position().0 * l.speed),
            _ => None,
        }
    }

    /// The take's actions (`take:<what>`).
    pub(super) fn take_act(&mut self, what: &str) {
        match what {
            "record" => self.take_record_loop(),
            "listen" => self.take_listen(),
            "close" => {
                self.take_listen_stop();
                if let Some(s) = self.take.take()
                    && let Phase::Analysing(job) = &s.phase
                {
                    job.cancel();
                }
            }
            _ => {
                if let Some(t0) = what.strip_prefix("bar:").and_then(|x| x.parse::<f64>().ok()) {
                    self.seek((t0 - LOOP_LEAD_IN + self.offset).max(0.0));
                }
            }
        }
    }

    /// Whether the console shows the take's card (a take outside a lesson).
    pub(super) fn take_panel_up(&self) -> bool {
        self.coach.is_none() && self.take.as_ref().is_some_and(|s| !s.lesson)
    }

    // -- drawing --------------------------------------------------------------------------------------------------

    /// "Your tab" over the chart (after a take): the fret played instead beside a wrong note, extra notes as ghosts.
    /// (The chart's notes are coloured through the scorer's states: `take_tick`.)
    pub(super) fn draw_take_overlay(&self, ui: &Ui) {
        let Some(s) = self.take.as_ref() else { return };
        let Some(r) = s.report.as_ref() else { return };
        if !s.overlay {
            return;
        }
        let th = t();
        let p = ui.painter().with_clip_rect(self.tab.view_rect());
        let n = self.chart.n_strings;
        let snap = self.nota.snap;
        for e in r.events.iter().filter(|e| e.status == Status::Wrong) {
            let tt = TabView::tt(&self.chart, snap, e.t);
            for pl in &e.played {
                let Some(at) = self.tab.note_pos(tt, pl.string, n) else {
                    continue;
                };
                let label = pl.fret.to_string();
                let w = text::width(p.ctx(), &label, Style::new(10.0, 800, th.miss)) + px(8.0);
                let r_ = Rect::from_min_size(pos2(at.x + px(9.0), at.y - px(15.0)), vec2(w, px(13.0)));
                p.rect_filled(r_, px(4.0), th.screen);
                p.rect_stroke(
                    r_,
                    px(4.0),
                    Stroke::new(px(1.0), th.miss),
                    egui::StrokeKind::Inside,
                );
                txt(
                    &p,
                    &label,
                    r_.center(),
                    10.0,
                    th.miss,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
        }
        for x in &r.extras {
            let tt = TabView::tt(&self.chart, snap, x.t);
            for pl in &x.played {
                let Some(at) = self.tab.note_pos(tt, pl.string, n) else {
                    continue;
                };
                let col = th.miss.gamma_multiply(0.75);
                p.circle_stroke(at, px(8.0), Stroke::new(px(1.2), col));
                txt(
                    &p,
                    &pl.fret.to_string(),
                    at,
                    10.0,
                    col,
                    700,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
        }
    }

    /// The take's card in the console (outside a lesson): what it is, then counting in / recording / analysing, or
    /// the result; its keys.
    pub(super) fn draw_take_panel(&mut self, ui: &Ui, area: Rect) {
        let Some(s) = self.take.as_ref() else { return };
        let p = ui.painter().clone();
        let th = t();
        paint::display(&p, area, Some(18.0), 6.0);
        let pad = px(16.0);
        let x = area.min.x + pad;
        let w = area.width() - 2.0 * pad;
        let mut y = area.min.y + pad;
        caps(&p, "TAKE", pos2(x, y), 11.0, Align2::LEFT_TOP, Some(w));
        y += px(20.0);
        let head = s.label.clone();
        txt(
            &p,
            &head,
            pos2(x, y),
            18.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(w),
        );
        y += px(26.0);
        txt(
            &p,
            &format!(
                "{} · {:.1}-{:.1} s at {}%",
                self.part_name,
                s.a,
                s.b,
                (s.speed * 100.0).round() as i64
            ),
            pos2(x, y),
            12.0,
            th.caption,
            600,
            Align2::LEFT_TOP,
            Some(w),
        );
        y += px(28.0);
        let bh = px(40.0);
        let bottom = area.max.y - pad;
        let half = (w - px(8.0)) / 2.0;
        let mut acts: Vec<String> = Vec::new();
        let r_row1 = Rect::from_min_size(pos2(x, bottom - 2.0 * bh - px(8.0)), vec2(w, bh));
        let r_left = Rect::from_min_size(pos2(x, bottom - bh), vec2(half, bh));
        let r_right = Rect::from_min_size(pos2(x + half + px(8.0), bottom - bh), vec2(half, bh));
        let going = s.going();
        let done = s.report.is_some();
        let listening = s.listen.is_some();
        let room = Rect::from_min_max(pos2(x, y), pos2(x + w, r_row1.min.y - px(10.0)));
        if let Some(r) = s.report.clone().filter(|_| !going) {
            if let Some(t0) = self.draw_take_report(ui, &p, &r, room) {
                acts.push(format!("take:bar:{t0}"));
            }
        } else {
            self.draw_take_status(&p, s, room);
        }
        let style = |primary: bool| ButtonStyle {
            primary,
            size: 13.0,
            r: Some(12.0),
            ..Default::default()
        };
        // (once it's recorded, the key stops the analysis, as the lesson's take key says: P37.4b)
        let (label, primary) = if going && matches!(s.phase, Phase::Analysing(_)) {
            ("Stop the analysis", false)
        } else if going {
            ("Stop the take", false)
        } else {
            ("Record again", !done)
        };
        if button(ui, r_row1, "take-rec", label, "Ctrl R", style(primary)).clicked() {
            acts.push("take:record".into());
        }
        if button(
            ui,
            r_left,
            "take-listen",
            if listening { "Stop" } else { "Listen back" },
            "B",
            ButtonStyle {
                enabled: done,
                ..style(false)
            },
        )
        .clicked()
        {
            acts.push("take:listen".into());
        }
        if button(ui, r_right, "take-close", "Close", "", style(false)).clicked() {
            acts.push("take:close".into());
        }
        self.acts.extend(acts);
    }

    /// Counting in, recording (how far), analysing (how far), or why it failed, in `r`.
    pub(super) fn draw_take_status(&self, p: &egui::Painter, s: &Session, r: Rect) {
        let th = t();
        let mid = r.center().x;
        let (head, sub, col, k) = match &s.phase {
            Phase::CountIn => (
                "Count-in…".to_string(),
                "then the stretch once".to_string(),
                th.miss,
                None,
            ),
            Phase::Recording | Phase::Tail(_) => (
                "Recording".to_string(),
                "play it through".to_string(),
                th.miss,
                Some(((self.now() - s.a) / (s.b - s.a).max(0.1)).clamp(0.0, 1.0) as f32),
            ),
            Phase::Starting(_) | Phase::Analysing(_) => (
                "Analysing…".to_string(),
                "checking every note against the tab".to_string(),
                th.text,
                Some(s.progress().unwrap_or(0.0).clamp(0.0, 1.0)),
            ),
            Phase::Failed(why) => ("No result".to_string(), why.clone(), th.miss, None),
            Phase::Done => ("Done".to_string(), String::new(), th.hit, None),
        };
        let hy = r.min.y + px(28.0);
        if matches!(s.phase, Phase::CountIn | Phase::Recording | Phase::Tail(_)) {
            let pulse = 0.6 + 0.4 * (p.ctx().input(|i| i.time) as f32 * 4.0).sin();
            p.ctx().request_repaint();
            let hw = text::width(p.ctx(), &head, Style::new(22.0, 800, col));
            paint::led(
                p,
                pos2(mid - hw / 2.0 - px(16.0), hy),
                12.0,
                col.gamma_multiply(pulse),
            );
        }
        txt(
            p,
            &head,
            pos2(mid, hy),
            22.0,
            col,
            800,
            Align2::CENTER_CENTER,
            Some(r.width()),
        );
        txt(
            p,
            &sub,
            pos2(mid, hy + px(24.0)),
            12.0,
            th.text_2,
            700,
            Align2::CENTER_CENTER,
            Some(r.width()),
        );
        if let Some(k) = k {
            let bar = Rect::from_min_size(pos2(r.min.x, hy + px(44.0)), vec2(r.width(), px(6.0)));
            paint::inset(p, bar, None, 1.0, Fill::Base);
            p.rect_filled(
                Rect::from_min_size(bar.min, vec2(bar.width() * k, bar.height())),
                px(3.0),
                if matches!(s.phase, Phase::Starting(_) | Phase::Analysing(_)) {
                    th.accent
                } else {
                    th.miss
                },
            );
            p.ctx().request_repaint();
        }
    }

    /// A take's result in `r`: the score, the timing, a strip of its bars (a click goes to one: returns its start),
    /// the problems in words (as many as fit). Returns the bar clicked.
    pub(super) fn draw_take_report(
        &self,
        ui: &Ui,
        p: &egui::Painter,
        r: &TakeReport,
        area: Rect,
    ) -> Option<f64> {
        let th = t();
        let (x, w) = (area.min.x, area.width());
        let mut y = area.min.y;
        let right = r.count(Status::Hit);
        let checked = r.checked();
        if checked == 0 {
            // (all legato, muted or unclear: no score to give)
            txt(
                p,
                "Nothing here could be checked",
                pos2(x, y),
                18.0,
                th.text,
                800,
                Align2::LEFT_TOP,
                Some(w),
            );
            txt(
                p,
                "no note in the stretch could be told right or wrong: try a stretch with picked notes",
                pos2(x, y + px(26.0)),
                12.0,
                th.caption,
                600,
                Align2::LEFT_TOP,
                Some(w),
            );
            return None;
        }
        let acc = r.accuracy();
        let big = format!("{}%", (acc * 100.0).round() as i64);
        let col = if acc >= 0.9 {
            th.hit
        } else if acc >= 0.7 {
            th.text
        } else {
            th.miss
        };
        let br = txt(p, &big, pos2(x, y), 30.0, col, 800, Align2::LEFT_TOP, None);
        txt(
            p,
            &format!(
                "{right} of {checked} right  ·  at {}%",
                (r.speed * 100.0).round() as i64
            ),
            pos2(br.max.x + px(10.0), br.min.y + px(4.0)),
            12.0,
            th.text_2,
            700,
            Align2::LEFT_TOP,
            Some((x + w - br.max.x - px(10.0)).max(px(20.0))),
        );
        txt(
            p,
            &timing_words(r),
            pos2(br.max.x + px(10.0), br.min.y + px(21.0)),
            12.0,
            th.caption,
            600,
            Align2::LEFT_TOP,
            Some((x + w - br.max.x - px(10.0)).max(px(20.0))),
        );
        y = br.max.y + px(10.0);
        // the bars, coloured by how they went
        let mut clicked = None;
        let n = r.bars.len().max(1);
        let gap = if n > 40 { 1.0 } else { px(2.0) };
        let cw = ((w - (n - 1) as f32 * gap) / n as f32).max(1.0);
        let ch = px(18.0);
        for (i, b) in r.bars.iter().enumerate() {
            let cr = Rect::from_min_size(pos2(x + i as f32 * (cw + gap), y), vec2(cw, ch));
            let c = match bar_tone(b.accuracy()) {
                0 => th.hit,
                1 => th.late,
                2 => th.miss,
                _ => th.unlit,
            };
            p.rect_filled(cr, px(3.0).min(cw / 2.0), c.gamma_multiply(0.85));
            let resp = interact(ui, cr, ("take-bar", i), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                p.rect_stroke(
                    cr,
                    px(3.0).min(cw / 2.0),
                    Stroke::new(px(1.5), th.text),
                    egui::StrokeKind::Outside,
                );
                let tip = match b.accuracy() {
                    Some(a) => format!("bar {}: {}%", b.number, (a * 100.0).round() as i64),
                    None => format!("bar {}: nothing checked", b.number),
                };
                txt(
                    p,
                    &tip,
                    pos2(x + w / 2.0, y + ch + px(14.0)),
                    11.0,
                    th.text,
                    700,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
            if resp.clicked() {
                clicked = Some(b.t0);
            }
        }
        if let (Some(f), Some(l)) = (r.bars.first(), r.bars.last()) {
            txt(
                p,
                &f.number.to_string(),
                pos2(x, y + ch + px(3.0)),
                9.0,
                th.caption,
                700,
                Align2::LEFT_TOP,
                None,
            );
            txt(
                p,
                &l.number.to_string(),
                pos2(x + w, y + ch + px(3.0)),
                9.0,
                th.caption,
                700,
                Align2::RIGHT_TOP,
                None,
            );
        }
        y += ch + px(26.0);
        // the problems, as many lines as fit
        let lh = px(17.0);
        let st = Style::new(12.0, 600, th.text);
        let ctx = p.ctx().clone();
        let mut first = true;
        'outer: for prob in r.problems.iter().take(5) {
            let lines = text::wrap(&ctx, prob, st, w - px(14.0));
            if first {
                if y + px(16.0) + lh > area.max.y {
                    break;
                }
                caps(p, "TO WORK ON", pos2(x, y), 10.0, Align2::LEFT_TOP, None);
                y += px(17.0);
                first = false;
            }
            for (k, line) in lines.iter().enumerate() {
                if y + lh > area.max.y {
                    break 'outer;
                }
                if k == 0 {
                    paint::dot(p, pos2(x + px(4.0), y + px(8.0)), 2.0, th.miss);
                }
                txt(
                    p,
                    line,
                    pos2(x + px(14.0), y),
                    12.0,
                    th.text_2,
                    600,
                    Align2::LEFT_TOP,
                    None,
                );
                y += lh;
            }
            y += px(3.0);
        }
        clicked
    }
}

impl PlayerView {
    /// Screenshot mode: a take of the 8 s before the moment shown: "take_analysing" (outside a lesson, part way
    /// through its analysis), "take_result" (its card and "your tab", a made-up report), or for a lesson's Record
    /// step "lesson" (the result, the coach staged already) and "lesson_analysing".
    pub(super) fn stage_take(&mut self, what: &str) {
        let now = self.now();
        let (a, b) = (now - 9.0, now - 1.0);
        let lesson = what.starts_with("lesson");
        let mut s = Session::new(a, b, 1.0, lesson);
        s.chart_id = self.chart.id;
        s.arr = Arc::new(self.chart.arr.clone());
        s.group_times = self.scorer.groups.iter().map(|g| g.t).collect();
        s.label = bars_label(&s.arr, a, b);
        let rate = 48000;
        s.input = Some(Arc::new(TakeInput {
            arr: self.chart.arr.clone(),
            from: a,
            to: b,
            samples: vec![0.0f32; ((b - a + 1.0) * rate as f64) as usize],
            timing: TakeTiming::linear(rate, a - 0.5, 1.0),
        }));
        if what.ends_with("analysing") {
            // a job on silence that's cancelled straight away: the card shows the progress set here
            let input = TakeInput {
                arr: qrs_formats::Arrangement::new("shot"),
                from: a,
                to: b,
                samples: Vec::new(),
                timing: TakeTiming::linear(rate, a, 1.0),
            };
            let job = TakeJob::start(input);
            job.cancel();
            s.phase = Phase::Analysing(Box::new(job));
            s.shot_progress = Some(0.62);
        } else {
            let r = synthetic_report(&self.chart.arr, &self.scorer.groups, a, b, 1.0);
            s.marks = tab_marks(&r, &s.group_times);
            s.overlay = true;
            s.phase = Phase::Done;
            s.report = Some(Arc::new(r));
        }
        if !lesson {
            self.loop_a = Some(a);
            self.loop_b = Some(b);
        }
        self.take = Some(s);
        // the tab at the take's start, so its rows are in view
        self.transport.seek((a + 0.3 + self.offset).max(0.0));
        self.tab.jump = true;
        self.states = self.scorer.groups.iter().map(|g| g.state).collect();
    }
}

/// A take played at one speed made to sound at another: `k` = the new speed over the one it was played at (1:
/// as it is).
pub fn restretch_take(mono: &[f32], rate: u32, k: f64) -> Vec<f32> {
    if (k - 1.0).abs() < 1e-6 || mono.is_empty() {
        return mono.to_vec();
    }
    let stereo: Vec<f32> = mono.iter().flat_map(|&v| [v, v]).collect();
    qrs_audio::stretch::stretch_clip(&stereo, rate, k, 0.0)
        .chunks_exact(2)
        .map(|f| f[0])
        .collect()
}

/// "Bars 5-11" for the stretch [a, b] of a part ("Bar 5", "The loop").
pub fn bars_label(arr: &qrs_formats::Arrangement, a: f64, b: f64) -> String {
    let bars = qrs_analysis::similar::Bars::new(arr);
    let range = bars.in_span(a, b);
    let (b0, b1) = (
        bars.bars.get(range.start).map(|b| b.number),
        bars.bars.get(range.end.saturating_sub(1)).map(|b| b.number),
    );
    match (b0, b1) {
        (Some(a), Some(b)) if a != b && !range.is_empty() => format!("Bars {a}-{b}"),
        (Some(a), _) if !range.is_empty() => format!("Bar {a}"),
        _ => "The loop".into(),
    }
}

/// Screenshot mode and tests: a report as if [a, b] had been played at `speed` with a few slips (a wrong note now
/// and then, one missed, a late one, an extra), from the scorer's groups of `arr`.
pub fn synthetic_report(
    arr: &qrs_formats::Arrangement,
    groups: &[qrs_analysis::detect::NoteGroup],
    a: f64,
    b: f64,
    speed: f64,
) -> TakeReport {
    use qrs_analysis::take::{BarResult, EventKind, EventResult, Extra, Neck, TimingSummary};
    let neck = Neck::new(arr);
    let bars = qrs_analysis::similar::Bars::new(arr);
    let range = bars.in_span(a, b);
    let bar_of = |t: f64| {
        range
            .clone()
            .position(|i| bars.bars[i].t0 <= t + 1e-6 && t < bars.bars[i].t1)
            .unwrap_or(0)
    };
    let mut events = Vec::new();
    let mut extras = Vec::new();
    for (i, g) in groups
        .iter()
        .filter(|g| g.t >= a - 0.02 && g.t < b - 0.01)
        .enumerate()
    {
        let expected: Vec<_> = g
            .strings
            .iter()
            .zip(&g.midis)
            .map(|(&s, &m)| neck.pos(s, m - neck.opens.get(s as usize).copied().unwrap_or(m)))
            .collect();
        let (status, played, offset) = if i % 11 == 3 {
            let p = expected.first().map(|e| neck.pos(e.string, e.fret + 2));
            (Status::Wrong, p.into_iter().collect(), Some(15.0))
        } else if i % 13 == 7 {
            (Status::Missed, Vec::new(), None)
        } else if i % 9 == 5 {
            (Status::Hit, expected.clone(), Some(140.0))
        } else {
            (Status::Hit, expected.clone(), Some(((i % 5) as f64 - 2.0) * 8.0))
        };
        let kind = match expected.len() {
            0 | 1 => EventKind::Note,
            2 => EventKind::DoubleStop,
            _ => EventKind::Chord,
        };
        events.push(EventResult {
            t: g.t,
            bar: bar_of(g.t),
            kind,
            techniques: qrs_formats::Techs::NONE,
            chord_name: String::new(),
            missing: if status == Status::Missed {
                expected.clone()
            } else {
                Vec::new()
            },
            expected,
            status,
            played,
            offset_ms: offset,
            sample: None,
            note: None,
        });
        if i % 17 == 9
            && let Some(e) = events.last().and_then(|e| e.expected.first())
        {
            extras.push(Extra {
                t: g.t + 0.12,
                sample: 0,
                bar: Some(bar_of(g.t)),
                played: vec![neck.pos(e.string, e.fret + 3)],
            });
        }
    }
    let bar_results: Vec<BarResult> = range
        .clone()
        .enumerate()
        .map(|(k, i)| {
            let bar = &bars.bars[i];
            let mine: Vec<&EventResult> = events.iter().filter(|e| e.bar == k).collect();
            let count = |st: Status| mine.iter().filter(|e| e.status == st).count();
            let offs: Vec<f64> = mine.iter().filter_map(|e| e.offset_ms).collect();
            BarResult {
                number: bar.number,
                index: k + 1,
                t0: bar.t0,
                t1: bar.t1,
                hits: count(Status::Hit),
                wrong: count(Status::Wrong),
                missed: count(Status::Missed),
                not_checked: count(Status::NotChecked),
                extras: extras.iter().filter(|x| x.bar == Some(k)).count(),
                mean_offset_ms: (!offs.is_empty()).then(|| offs.iter().sum::<f64>() / offs.len() as f64),
            }
        })
        .collect();
    let mut offs: Vec<f64> = events
        .iter()
        .filter(|e| e.status == Status::Hit)
        .filter_map(|e| e.offset_ms)
        .collect();
    offs.sort_by(f64::total_cmp);
    let n = offs.len();
    let mean = if n > 0 {
        offs.iter().sum::<f64>() / n as f64
    } else {
        0.0
    };
    let median = offs.get(n / 2).copied().unwrap_or(0.0);
    let spread = if n > 0 {
        (offs.iter().map(|o| (o - mean).powi(2)).sum::<f64>() / n as f64).sqrt()
    } else {
        0.0
    };
    let mut r = TakeReport {
        artist: String::new(),
        title: String::new(),
        part: arr.name.clone(),
        from: a,
        to: b,
        speed,
        rate: 48000,
        seconds: (b - a) / speed,
        neck,
        events,
        extras,
        bars: bar_results,
        timing: TimingSummary {
            n,
            mean_ms: mean,
            median_ms: median,
            spread_ms: spread,
            tendency: Tendency::OnTime,
            by_kind: Vec::new(),
        },
        problems: Vec::new(),
        shift_ms: 0.0,
        plucks_found: n,
        peak: 0.5,
        analysis_ms: 0.0,
    };
    r.problems = qrs_analysis::take::problems(&r);
    r
}

#[cfg(test)]
mod tests;
