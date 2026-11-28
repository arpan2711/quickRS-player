//! The timing tool (P36.2, D95; new code, no Python source; the setup review's P1.1, RS2014 / Guitar Hero
//! style): a step of the guided guitar setup (before Done) and, on its own, Settings > Timing's first card.
//!
//! Since P38.4 (D159) the step opens on the play-along (`play_along.rs`: a tiny tune on a tab, which sets the
//! delay from the notes); what's below is its "Advanced: measure with clicks" (`TimingTool::advanced`).
//!
//!     1 YOUR TIMING     strum a muted chord (a bass: pluck the open E) on every click (133 bpm, 4 clicks in, then
//!                       on until the estimate is precise enough: 16 strums at least, 42 clicks at most; what it says
//!                       along the way is about how steady the strums are, never early or late, and the strip is
//!                       centred on your own middle): Guitar / Bass delay (`input_offset`, shown as the delay:
//!                       -input_offset, D132)
//!     2 SCREEN TIMING   no sound: strum the moment each dot fills the ring: Screen (`display_offset`)
//!     FINE-TUNE         click and dots together, live; the delay and Screen in 5 ms steps (Shift: 1 ms), the last
//!                       strums as they'd score
//!
//! The clicks are the jam's (`Engine::jam_play`): they are heard on the jam's clock, which the engine stamps with
//! when each block is heard, and a pluck's `wall_ns` is when it was played, so `jam_clock().at(wall)` is the beat
//! a pluck landed on, in the same terms as a song (`timing_cal.rs` has the maths). While it runs the plucks are the
//! tool's (`timing_cal::hold_onsets`), the jam's own settings are put back after, and both happen in `Live`'s drop
//! too, however the setup is left.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_audio::Engine;
use qrs_audio::jam::{BackingSpec, ClickParams, DrumParams};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle};

use super::timing::{delay_label, ms_label};
use super::{SettingsView, centre_y, line_h, snap};
use crate::state::AppState;
use crate::timing_cal::{self as tc, COUNT_IN, Record};
use qrs_analysis::instrument::Instrument;

/// What a run is measuring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// Strum on the clicks.
    Guitar,
    /// No sound: strum the moment each dot fills the ring.
    Screen,
    /// Click and gems together, until stopped: the fine-tune.
    Along,
}

/// The jam's settings before the tool took it.
struct JamSaved {
    bpm: f64,
    meter: u32,
    click: ClickParams,
    drums: DrumParams,
    backing: Option<BackingSpec>,
}

/// A run under way (the play-along's too).
pub(super) struct Live {
    mode: Mode,
    pub engine: Option<Arc<Engine>>,
    saved: Option<JamSaved>,
    /// The plucks (s from the count-in's first click).
    plucks: Vec<f64>,
    /// The fine-tune's last strums as they'd score (s, + late).
    recent: VecDeque<f64>,
    /// Where a click-tool run stands as of the last frame: the strums so far, what it says, whether it is done.
    track: Option<tc::Track>,
    /// What it is saying and since when: a message of one kind holds for a second before another kind replaces it
    /// (the counts in "about 6 more" change every strum, and do not wait).
    said: Option<(tc::Say, Instant)>,
    pub started: Instant,
    /// Screenshots: the beat it's held on.
    staged_beat: Option<f64>,
}

impl Live {
    fn start(engine: Arc<Engine>, mode: Mode) -> Live {
        Live::start_at(engine, mode, tc::BPM, false)
    }

    /// The jam taken for a run at `bpm` (a bar of `COUNT_IN` clicks first), the drums along when `drums`.
    pub fn start_at(engine: Arc<Engine>, mode: Mode, bpm: f64, drums: bool) -> Live {
        let jam = engine.jam();
        let saved = JamSaved {
            bpm: engine.jam_tempo(),
            meter: jam.beats_per_bar,
            click: engine.click(),
            drums: engine.drums(),
            backing: engine.backing(),
        };
        if jam.running {
            engine.jam_stop();
        }
        engine.set_backing(None);
        engine.set_drums(DrumParams {
            on: drums,
            style: qrs_audio::jam::DrumStyle::Rock,
            volume: 0.45,
            fill_every: 0,
            follow: false,
            ..saved.drums
        });
        engine.set_jam_meter(COUNT_IN as u32);
        engine.set_jam_tempo(bpm);
        engine.set_click(ClickParams {
            on: mode != Mode::Screen,
            accent: false,
            volume: 0.8,
        });
        let _ = engine.take_onsets();
        tc::hold_onsets(true);
        engine.jam_play(1);
        Live {
            mode,
            engine: Some(engine),
            saved: Some(saved),
            plucks: Vec::new(),
            recent: VecDeque::new(),
            track: None,
            said: None,
            started: Instant::now(),
            staged_beat: None,
        }
    }

    /// The strums so far, about how many it will take, and what it says.
    fn progress(&self) -> Option<(usize, usize, tc::Say)> {
        let t = self.track?;
        Some((t.n, t.total, self.said.map_or(t.say, |(s, _)| s)))
    }

    /// The beat heard now (negative in the count-in).
    pub fn beat(&self) -> f64 {
        if let Some(b) = self.staged_beat {
            return b;
        }
        match &self.engine {
            Some(e) => {
                let j = e.jam();
                if j.running { j.beat } else { -(COUNT_IN as f64) }
            }
            None => 0.0,
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        tc::hold_onsets(false);
        let (Some(e), Some(s)) = (&self.engine, self.saved.take()) else {
            return;
        };
        e.jam_stop();
        e.set_click(s.click);
        e.set_drums(s.drums);
        e.set_jam_meter(s.meter);
        e.set_jam_tempo(s.bpm);
        if s.backing.is_some() {
            e.set_backing(s.backing);
        }
    }
}

/// The tool's state (held by the guide: it goes with it).
#[derive(Default)]
pub(super) struct TimingTool {
    pub guitar: Option<tc::Run>,
    pub screen: Option<tc::Run>,
    /// The delay (`input_offset`) and Screen as they were before this visit (Undo).
    pub was: Option<(f64, f64)>,
    pub live: Option<Live>,
    /// The last thing it said (why a run can't be trusted, what was set), and whether it is a problem.
    note: Option<String>,
    note_bad: bool,
    /// The play-along (P38.4, the step's default) and whether the click tool shows instead (Advanced).
    pub play: super::play_along::PlayAlong,
    pub advanced: bool,
    /// The Output step's drums to balance against (P36.5).
    pub band: Option<Band>,
    /// Screenshots: the delay and Screen as the staged runs set them.
    set: (Option<f64>, Option<f64>),
    /// Screenshots: both runs shown done (the note says what they set, in the instrument's words).
    staged_result: bool,
}

/// "27 ms early" / "12 ms late" / "on time" for a pluck's offset (s, + late).
fn early_late(secs: f64) -> String {
    let ms = (secs * 1000.0).round();
    if ms == 0.0 {
        "on time".into()
    } else if ms < 0.0 {
        format!("{:.0} ms early", -ms)
    } else {
        format!("{ms:.0} ms late")
    }
}

/// What a screen run set, said so that it makes sense: `v` the new Screen setting, `was` the old one, `total` how
/// far your strums landed from the dots and `own` how far they land from the clicks (both s, + late: `own` is the
/// run 1 measurement, and is what your playing is like before any screen is in the way). What's left, `total - own`,
/// is the screen: strumming later on the dots than on the sound means the picture shows each dot late, so Screen
/// (`display_offset`, + draws the picture further on) draws it that much sooner. The run drew the dots with the old
/// Screen in place, so what is left is on top of it ("still"). Two lines at most: it shows under the cards, where
/// the room is short.
fn screen_note(_inst: Instrument, v: f64, was: f64, total: f64, own: f64) -> String {
    let rest = total - own;
    let ms = (rest * 1000.0).round().abs();
    let still = if was.abs() >= 0.0005 { "still " } else { "" };
    let verdict = if ms < 3.0 {
        "so your screen keeps up with the sound and the picture stays as it is.".to_string()
    } else if rest > 0.0 {
        format!("so the screen is {still}{ms:.0} ms behind the sound, and quickRS now draws it {ms:.0} ms sooner.")
    } else {
        format!("so the screen is {still}{ms:.0} ms ahead of the sound, and quickRS now draws it {ms:.0} ms later.")
    };
    format!(
        "Screen set to {} (was {}). To the dots you strummed {}, to the clicks alone {}: {verdict}",
        ms_label(v),
        ms_label(was),
        early_late(total),
        early_late(own),
    )
}

/// What run 1 set, in a line.
fn delay_note(inst: Instrument, v: f64, was: f64) -> String {
    format!(
        "Set {} delay: {} (was {}). Your notes count that much earlier.",
        inst.noun_cap(),
        delay_label(v),
        delay_label(was)
    )
}

impl TimingTool {
    /// Screenshots: the click tool's "measure" (guitar timing, 9 clicks in), "screenmeasure", "result" (both runs
    /// done) and "timingclicks" (fresh); the play-along's "playlisten", "playing", "playresult"; else the
    /// play-along fresh.
    pub fn staged(what: &str) -> TimingTool {
        let mut tool = TimingTool {
            advanced: matches!(what, "measure" | "screenmeasure" | "result" | "timingclicks"),
            play: super::play_along::PlayAlong::staged(what),
            ..Default::default()
        };
        let ok = |off: f64, n: usize| tc::Run {
            fit: tc::Fit {
                offset: off,
                spread: 0.009,
                n,
                outliers: 0,
                ci: 0.0044,
            },
            doubles: 0,
            strays: 0,
            // (a few strums round the offset, for the strip of a done run)
            residuals: (0..n).map(|i| off + 0.004 * ((i % 5) as f64 - 2.0)).collect(),
            problem: None,
        };
        match what {
            "measure" | "screenmeasure" => {
                let screen = what == "screenmeasure";
                if screen {
                    tool.guitar = Some(ok(0.018, 16));
                }
                let offs = [0.021, 0.012, 0.025, 0.016, 0.019, 0.028, 0.014, 0.020, 0.017];
                let clicks = tc::clicks();
                tool.live = Some(Live {
                    mode: if screen { Mode::Screen } else { Mode::Guitar },
                    engine: None,
                    saved: None,
                    plucks: offs
                        .iter()
                        .enumerate()
                        .map(|(i, o)| clicks[COUNT_IN + i] + o)
                        .collect(),
                    recent: VecDeque::new(),
                    track: None,
                    said: None,
                    started: Instant::now(),
                    staged_beat: Some(8.06),
                });
            }
            "result" => {
                tool.guitar = Some(ok(0.018, 15));
                tool.screen = Some(ok(0.030, 14));
                tool.staged_result = true;
                tool.was = Some((0.0, 0.0));
                tool.set = (Some(-0.018), Some(0.012));
            }
            _ => {}
        }
        tool
    }

    pub fn running(&self) -> bool {
        self.live.is_some()
    }

    /// Stop a run or the play-along (the jam put back).
    pub fn stop(&mut self) {
        self.live = None;
        self.band = None;
        self.play.stop();
    }
}


/// A trusted guitar run (the click tool's or the play-along's): the delay set from it (`input_offset = -median`)
/// and the run kept in config.json `timing_cal` with the devices it was measured on. Returns the new
/// `input_offset`.
pub(super) fn save_guitar_run(st: &mut AppState, fit: &tc::Fit) -> f64 {
    let v = tc::guitar_offset(fit.offset);
    st.config.set("input_offset", v);
    let mut rec = Record::load(&st.config).unwrap_or_default();
    rec.guitar_ms = Some(fit.offset * 1000.0);
    rec.spread_ms = Some(fit.spread * 1000.0);
    rec.n = Some(fit.n);
    rec.route = route(st);
    rec.at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    rec.save(&mut st.config);
    st.save_config();
    v
}

/// The route timing is measured on ("NUX Audio -> Speakers (Realtek) (WASAPI shared)"), "" with no devices.
pub(super) fn route(st: &AppState) -> String {
    let Some(a) = &st.audio else {
        return String::new();
    };
    let s = a.engine.status();
    match (&s.input, &s.output) {
        (Some(i), Some(o)) => tc::route_of(&i.device_name, &o.device_name, &o.mode),
        _ => String::new(),
    }
}

/// The last run, in a line ("Guitar delay 18 ms, screen +12 ms, measured on NUX Audio -> ..."), None: never.
pub(super) fn summary(st: &AppState) -> Option<String> {
    let r = Record::load(&st.config)?;
    let io = st.config.f64("input_offset", 0.0);
    let disp = st.config.f64("display_offset", 0.0);
    let mut s = format!("{} delay {}", st.instrument().noun_cap(), delay_label(io));
    if r.screen_ms.is_some() || disp.abs() >= 0.0005 {
        s += &format!(", screen {}", ms_label(disp));
    }
    if !r.route.is_empty() {
        s += &format!(", measured on {}", r.route);
    }
    if tc::timing_stale(&r.route, &route(st)) {
        s += " (other devices now: check it again)";
    }
    Some(s)
}

/// The guitar's measured lateness to set Screen against: this visit's run, else the last one saved.
fn guitar_m(tool: &TimingTool, st: &AppState) -> Option<f64> {
    tool.guitar
        .as_ref()
        .filter(|r| r.ok())
        .map(|r| r.fit.offset)
        .or_else(|| {
            Record::load(&st.config)
                .and_then(|r| r.guitar_ms)
                .map(|m| m / 1000.0)
        })
}

impl SettingsView {
    fn tool_mut(&mut self) -> Option<&mut TimingTool> {
        self.setup.as_mut().map(|g| &mut g.timing)
    }

    /// The tool's actions: `tcal:guitar` / `screen` / `along` start a run, `stop` ends one, `undo` puts the delay
    /// and Screen back as they were.
    pub(super) fn timing_act(&mut self, st: &mut AppState, arg: &str) {
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let input = crate::calibrate::current_input(st).is_some();
        let gm = self.setup.as_ref().and_then(|g| guitar_m(&g.timing, st));
        let staged = self.setup.as_ref().is_some_and(|g| g.staged);
        let io = st.config.f64("input_offset", 0.0);
        let disp = st.config.f64("display_offset", 0.0);
        let inst = st.instrument();
        let Some(tool) = self.tool_mut() else { return };
        let mode = match arg {
            "guitar" => Mode::Guitar,
            "screen" => Mode::Screen,
            "along" => Mode::Along,
            "stop" => {
                tool.live = None;
                return;
            }
            "undo" => {
                if let Some((a, b)) = tool.was.take() {
                    st.config.set("input_offset", a);
                    st.config.set("display_offset", b);
                    st.save_config();
                    tool.note = Some(format!(
                        "Back to {} delay {}, Screen {}",
                        inst.noun_cap(),
                        delay_label(a),
                        ms_label(b)
                    ));
                }
                return;
            }
            _ => return,
        };
        if staged {
            return;
        }
        let Some(e) = engine else {
            tool.note = Some("The audio engine isn't running.".into());
            return;
        };
        if !input {
            tool.note = Some("No input is listening: pick yours in Settings > Sound first.".into());
            return;
        }
        if mode == Mode::Screen && gm.is_none() {
            tool.note = Some("Run 1, Your timing, first: Screen timing is measured against it.".into());
            return;
        }
        tool.was.get_or_insert((io, disp));
        tool.live = None; // (a run before it puts the jam back first)
        tool.play.stop();
        tool.note = None;
        tool.live = Some(Live::start(e, mode));
    }

    /// Once a frame on the timing step: the plucks in, and a run that has had its clicks finished.
    pub(super) fn timing_tick(&mut self, st: &mut AppState, ctx: &egui::Context) {
        let gm = self.setup.as_ref().and_then(|g| guitar_m(&g.timing, st));
        let io = st.config.f64("input_offset", 0.0);
        let disp = st.config.f64("display_offset", 0.0);
        let rt = route(st);
        let inst = st.instrument();
        let Some(tool) = self.tool_mut() else { return };
        let Some(live) = &mut tool.live else { return };
        if live.staged_beat.is_some() {
            return;
        }
        ctx.request_repaint();
        let Some(e) = live.engine.clone() else { return };
        let clock = e.jam_clock();
        let period = tc::period();
        for o in e.take_onsets() {
            if !clock.playing {
                continue;
            }
            let beat = clock.at(o.wall_ns);
            let s = (beat + COUNT_IN as f64) * period;
            live.plucks.push(s);
            if live.mode == Mode::Along {
                let r = (beat - beat.round()) * period;
                if r.abs() < tc::window(period) && beat > -0.5 {
                    live.recent.push_back(r + io);
                    while live.recent.len() > 8 {
                        live.recent.pop_front();
                    }
                }
            }
        }
        let beat = live.beat();
        let timed_out = live.started.elapsed().as_secs_f64() > (COUNT_IN + tc::MAX_SCORED) as f64 * period + 6.0;
        if live.mode == Mode::Along {
            return;
        }
        // the run so far, every frame (a few dozen strums): what it says, and whether it has enough. It goes on
        // from 16 strums until the estimate is precise enough (or it is clear it never will be), and no longer.
        let m = tc::match_plucks(&tc::clicks_to(tc::MAX_SCORED), &live.plucks, COUNT_IN);
        let over = ((beat - tc::window(period) / period).floor() + 1.0).clamp(0.0, tc::MAX_SCORED as f64) as usize;
        let tr = tc::track(&m.residuals, &m.hit_clicks, over);
        let kind = |s: tc::Say| std::mem::discriminant(&s);
        live.said = match live.said {
            // (another kind of message waits out the second the last one has; a verdict does not wait)
            Some((old, since))
                if kind(old) != kind(tr.say) && since.elapsed().as_secs_f64() < 1.0 && tr.verdict.is_none() =>
            {
                Some((old, since))
            }
            Some((old, since)) if kind(old) == kind(tr.say) => Some((tr.say, since)),
            _ => Some((tr.say, Instant::now())),
        };
        live.track = Some(tr);
        if tr.verdict.is_none() && !timed_out {
            return;
        }
        // the clicks are done: what they say
        let mode = live.mode;
        // (a screen run's offset holds your own timing as well as the screen's: only its screen share is judged)
        let screen_run = mode == Mode::Screen;
        let limit = if screen_run { f64::INFINITY } else { tc::offset_limit(&rt) };
        let mut run = tc::measure_for(&tc::clicks_to(tc::MAX_SCORED), &live.plucks, COUNT_IN, limit);
        let screen_rest = run.fit.offset - gm.unwrap_or(0.0);
        if screen_run && run.ok() && screen_rest.abs() > tc::SCREEN_SANE {
            run.problem = Some(tc::Problem::TooFar);
        }
        tool.live = None;
        tool.note_bad = !run.ok();
        if run.ok() {
            match mode {
                Mode::Guitar => {
                    let v = save_guitar_run(st, &run.fit);
                    tool.note = Some(delay_note(inst, v, io));
                }
                Mode::Screen => {
                    let v = tc::screen_offset(disp, run.fit.offset, gm.unwrap_or(0.0));
                    st.config.set("display_offset", v);
                    let mut rec = Record::load(&st.config).unwrap_or_default();
                    rec.screen_ms = Some(v * 1000.0);
                    rec.route = rt;
                    rec.at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    rec.save(&mut st.config);
                    st.save_config();
                    tool.note = Some(screen_note(inst, v, disp, run.fit.offset, gm.unwrap_or(0.0)));
                }
                Mode::Along => {}
            }
        } else if screen_run && run.problem == Some(tc::Problem::TooFar) {
            tool.note = Some(inst.say(&tc::screen_too_far_words(screen_rest)));
        } else {
            tool.note = Some(inst.say(&run.words_for(screen_run)));
        }
        match mode {
            Mode::Guitar => tool.guitar = Some(run),
            Mode::Screen => tool.screen = Some(run),
            Mode::Along => {}
        }
    }

    /// The timing step: the play-along (P38.4) with its "Advanced fine-tuning" fold rolled up under it; rolled
    /// down, the fold holds the two click runs side by side and the fine-tune under them (the play-along shrinks
    /// to a row above it when there's room, else the fold's row rolls it back up).
    pub(super) fn guide_timing(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let fold_open = st.config.bool(super::play_along::FOLD_KEY, false);
        let Some(g) = self.setup.as_mut() else { return };
        if !g.staged {
            g.timing.advanced = fold_open;
        }
        if !g.timing.advanced {
            return self.guide_play(ui, st, area);
        }
        let mut top = area.min.y;
        if area.height() > px(640.0) {
            top = self.play_strip(ui, st, area) + px(12.0);
        }
        let fh = px(widgets::FOLD_ROW_H);
        self.timing_fold(
            ui,
            Rect::from_min_size(pos2(area.min.x, top), vec2(area.width(), fh)),
            true,
        );
        let area = Rect::from_min_max(pos2(area.min.x, top + fh + px(12.0)), area.max);
        let ctx = ui.ctx().clone();
        self.timing_tick(st, &ctx);
        let th = t();
        let p = ui.painter().clone();
        let mut tool = match self.setup.as_mut() {
            Some(g) => std::mem::take(&mut g.timing),
            None => return,
        };
        let inst = st.instrument();
        let bass = inst.is_bass();
        if tool.staged_result && tool.note.is_none() {
            // (the last run's note, as after a real Screen run)
            tool.note = Some(screen_note(inst, 0.012, 0.0, 0.030, 0.018));
        }
        let pad = px(20.0);
        let hh = Self::h_heading(&ctx);
        let cap_h = line_h(&ctx, 12.0) + px(6.0);
        let fine_base = 2.0 * pad + hh + cap_h + px(46.0);
        let fine_full = fine_base + px(12.0) + 2.0 * line_h(&ctx, 12.0);
        // (the note line only takes room when there is a note: the fold leaves the cards little, P38.4)
        let note_w = area.width() - px(160.0);
        let note_lines: Vec<String> = tool
            .note
            .as_ref()
            .map(|n| text::wrap(&ctx, n, Style::new(13.0, 700, th.hit), note_w))
            .unwrap_or_default();
        let note_h = if tool.note.is_some() {
            note_lines.len().clamp(1, 4) as f32 * line_h(&ctx, 13.0) + px(12.0)
        } else {
            0.0
        };
        // (a long note takes the room the fine-tune's two lines of explanation had: they go before the card does)
        let short = area.height() - fine_full - px(16.0) - note_h < px(200.0);
        let along_now = tool.live.as_ref().is_some_and(|l| l.mode == Mode::Along);
        let fine_h = if !short {
            fine_full
        } else if along_now {
            fine_base + px(12.0) + px(28.0) // (the strums' strip is kept)
        } else {
            fine_base
        };
        let card_h = (area.height() - fine_h - px(16.0) - note_h).clamp(px(200.0), px(380.0));
        let (a, b) = self.two_cards(&p, area, card_h);
        // (screenshots save nothing: what a staged run set is shown as set)
        let io = tool.set.0.unwrap_or(st.config.f64("input_offset", 0.0));
        let disp = tool.set.1.unwrap_or(st.config.f64("display_offset", 0.0));
        let live_mode = tool.live.as_ref().map(|l| l.mode);
        // 1 YOUR TIMING
        let words = if bass {
            "Pluck the open E, short and muted, on every click."
        } else {
            "Strum a muted chord, short, on every click."
        };
        self.run_card(
            ui,
            inst,
            a,
            ("1", "YOUR TIMING", words),
            tool.guitar.as_ref(),
            (live_mode == Some(Mode::Guitar))
                .then_some(tool.live.as_ref())
                .flatten(),
            ("tcal:guitar", "1"),
            match &tool.guitar {
                Some(r) if r.ok() => {
                    format!("\u{2713} delay {}", delay_label(tc::guitar_offset(r.fit.offset)))
                }
                _ => String::new(),
            },
            disp,
        );
        // 2 SCREEN TIMING
        let have_m = guitar_m(&tool, st).is_some();
        let screen_words = inst.say(if have_m {
            "Optional. Silent: strum as each dot fills the ring."
        } else {
            "After 1. Silent: strum as each dot fills the ring."
        });
        self.run_card(
            ui,
            inst,
            b,
            ("2", "SCREEN TIMING", &screen_words),
            tool.screen.as_ref(),
            // (the fine-tune plays here: its gems and click lamp)
            matches!(live_mode, Some(Mode::Screen | Mode::Along))
                .then_some(tool.live.as_ref())
                .flatten(),
            ("tcal:screen", "2"),
            match &tool.screen {
                Some(r) if r.ok() => format!("\u{2713} screen {}", ms_label(disp)),
                _ => String::new(),
            },
            disp,
        );
        // what it last said
        let ny = a.max.y + px(12.0);
        if tool.note.is_some() {
            let bad = tool.note_bad;
            let mut ly = ny;
            for l in note_lines.iter().take(4) {
                ly = text::draw(
                    &p,
                    l,
                    pos2(area.min.x + px(4.0), ly),
                    Align2::LEFT_TOP,
                    Style::new(13.0, 700, if bad { th.late } else { th.hit }),
                    None,
                )
                .max
                .y;
            }
            if tool.was.is_some() && !bad {
                let bw = widgets::button_width(&ctx, "Undo", "", 13.0, false).max(px(110.0));
                self.button(
                    ui,
                    Rect::from_min_size(pos2(area.max.x - bw, ny - px(4.0)), vec2(bw, px(28.0))),
                    "Undo",
                    "",
                    "tcal:undo",
                    ButtonStyle {
                        size: 13.0,
                        r: Some(12.0),
                        ..Default::default()
                    },
                );
            }
        }
        // FINE-TUNE
        let f = Rect::from_min_size(
            pos2(area.min.x, a.max.y + px(16.0) + note_h),
            vec2(area.width(), fine_h),
        );
        if f.max.y <= area.max.y + px(24.0) {
            paint::card(&p, f, 24.0, false);
            let x = f.min.x + pad;
            let along = live_mode == Some(Mode::Along);
            let yy = self.heading(
                &p,
                "FINE-TUNE",
                x,
                f.min.y + pad,
                Some(f.max.x - pad),
                &inst.say("Play along: click and dots together, strum and watch your dots"),
                "",
            );
            // (four columns: the delay, the screen, the song sync, Play along; P38.4 added the song sync here)
            let colw = snap((f.width() - 2.0 * pad - px(24.0) * 3.0) / 4.0);
            let av = st.config.f64("av_offset", 0.0);
            let delay_cap = format!("{} DELAY", inst.noun_upper());
            for (k, label) in [delay_cap.as_str(), "SCREEN", "SONG SYNC"].iter().enumerate() {
                text::caps(
                    &p,
                    label,
                    pos2(x + k as f32 * (colw + px(24.0)), yy),
                    11.0,
                    th.caption,
                    Align2::LEFT_TOP,
                    None,
                );
            }
            let yy = yy + cap_h;
            self.stepper(
                ui,
                x,
                yy,
                colw,
                // (the delay is -input_offset: − shortens it, the notes counting later)
                ("\u{2212}", ",", "ms:input_offset:5"),
                &delay_label(io),
                ("+", ".", "ms:input_offset:-5"),
            );
            self.stepper(
                ui,
                x + colw + px(24.0),
                yy,
                colw,
                ("\u{2212}", ";", "ms:display_offset:-5"),
                &ms_label(disp),
                ("+", "'", "ms:display_offset:5"),
            );
            self.stepper(
                ui,
                x + 2.0 * (colw + px(24.0)),
                yy,
                colw,
                ("\u{2212}", "", "ms:av_offset:-5"),
                &ms_label(av),
                ("+", "", "ms:av_offset:5"),
            );
            // Play along / Stop, and while it plays the last strums as they'd score in place of the words
            let rx = x + 3.0 * (colw + px(24.0));
            self.button(
                ui,
                Rect::from_min_size(pos2(rx, yy), vec2(f.max.x - pad - rx, px(46.0))),
                if along { "Stop" } else { "Play along" },
                "P",
                if along { "tcal:stop" } else { "tcal:along" },
                ButtonStyle {
                    size: 14.0,
                    r: Some(14.0),
                    ..Default::default()
                },
            );
            let cy = yy + px(46.0) + px(12.0);
            if along {
                let recent: Vec<f64> = tool
                    .live
                    .as_ref()
                    .map(|l| l.recent.iter().copied().collect())
                    .unwrap_or_default();
                let strip = Rect::from_min_size(pos2(x, cy), vec2(f.width() - 2.0 * pad, px(28.0)));
                dot_strip(&p, strip, &recent);
            } else if !short {
                super::caption_lines(
                    &p,
                    &format!(
                        "{} delay: how late your playing reaches quickRS (taken off when a {} counts). Screen: \
                         moves only the picture, for a screen that lags. Song sync: the songs' notes against \
                         their music, for charts that run ahead. 5 ms a step, Shift for 1 ms.",
                        inst.noun_cap(),
                        inst.strike()
                    ),
                    x,
                    cy,
                    f.width() - 2.0 * pad,
                    12.0,
                    2,
                );
            }
        }
        if let Some(g) = self.setup.as_mut() {
            g.timing = tool;
        }
    }

    /// A run's card: its number and name (and the result at the right), what to do, the lane (the clicks' lamp and
    /// the dots, live), how far it is and the strums so far, Start.
    #[allow(clippy::too_many_arguments)]
    fn run_card(
        &mut self,
        ui: &Ui,
        inst: Instrument,
        card: Rect,
        (num, name, words): (&str, &str, &str),
        result: Option<&tc::Run>,
        live: Option<&Live>,
        (act, key): (&str, &str),
        chip: String,
        disp: f64,
    ) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let (x, w) = (card.min.x + pad, card.width() - 2.0 * pad);
        let yy = self.heading(&p, &format!("{num}  {name}"), x, card.min.y + pad, None, "", "");
        if !chip.is_empty() {
            text::draw(
                &p,
                &chip,
                pos2(card.max.x - pad, card.min.y + pad + px(6.0)),
                Align2::RIGHT_CENTER,
                Style::new(15.0, 800, th.hit),
                None,
            );
        }
        let tr = text::draw(
            &p,
            words,
            pos2(x, yy),
            Align2::LEFT_TOP,
            Style::new(14.0, 600, th.text),
            Some(w),
        );
        let bh = px(46.0);
        let foot = card.max.y - pad - bh;
        let lane = Rect::from_min_max(
            pos2(x, tr.max.y + px(12.0)),
            pos2(x + w, foot - px(14.0) - px(22.0) - px(10.0)),
        );
        let screen = name.starts_with("SCREEN");
        paint::display(&p, lane, Some(16.0), 3.0);
        match live {
            Some(l) => draw_lane(&p, lane, l, disp),
            None => {
                // (a result is said under the cards, where it has room to wrap: here only the idle hint)
                let msg = match result {
                    Some(_) => String::new(),
                    None if screen => {
                        "The clicks stay silent: strum as each dot fills the ring, until it says it has enough."
                            .to_string()
                    }
                    None => inst.say("4 clicks to count you in, then strum on each one until it says it has enough."),
                };
                let lines = text::wrap(&ctx, &msg, Style::new(13.0, 700, th.caption), lane.width() - px(36.0));
                let lh = line_h(&ctx, 13.0);
                let mut ly = lane.center().y - lines.len() as f32 * lh / 2.0;
                for l in lines.iter().take(4).filter(|_| !msg.is_empty()) {
                    ly = text::draw(
                        &p,
                        l,
                        pos2(lane.center().x, ly),
                        Align2::CENTER_TOP,
                        Style::new(13.0, 700, th.caption),
                        None,
                    )
                    .max
                    .y;
                }
            }
        }
        // how far it is, and the strums so far as dots
        let by = lane.max.y + px(10.0);
        let bar = Rect::from_min_size(pos2(x, by), vec2(w, px(14.0)));
        paint::inset(&p, bar, Some(7.0), 2.0, Fill::Base);
        let (frac, label, status) = match live {
            Some(l) => {
                let b = l.beat();
                if b < 0.0 {
                    (0.0, format!("counting in: {}", (-b).ceil() as i64), "Get ready…".to_string())
                } else if let Some((n, total, say)) = l.progress() {
                    // (the bar is how far to a precise enough estimate: the strums so far of the strums it expects)
                    (
                        (n as f32 / total.max(1) as f32).min(1.0),
                        format!("{n} of about {total} strums"),
                        say.words(screen),
                    )
                } else {
                    (0.0, String::new(), String::new())
                }
            }
            None => match result {
                Some(r) => (1.0, format!("{} strums", r.fit.n), String::new()),
                None => (0.0, String::new(), String::new()),
            },
        };
        if frac > 0.0 {
            let fill = Rect::from_min_size(bar.min, vec2((bar.width() * frac).max(px(14.0)), bar.height()));
            p.rect_filled(fill.shrink(px(3.0)), px(4.0), th.accent);
        }
        let lr = text::draw(
            &p,
            &label,
            pos2(x + w, bar.max.y + px(11.0)),
            Align2::RIGHT_CENTER,
            Style::new(11.0, 600, th.caption),
            None,
        );
        if !status.is_empty() {
            // (what the run says while it goes: about how steady the strums are, never about early or late)
            text::draw(
                &p,
                &status,
                pos2(x, bar.max.y + px(11.0)),
                Align2::LEFT_CENTER,
                Style::new(12.0, 700, th.text),
                Some((w - lr.width() - px(16.0)).max(px(60.0))),
            );
        }
        let label_running = live.is_some();
        let bl = if label_running {
            "Stop"
        } else if result.is_some() {
            "Again"
        } else {
            "Start"
        };
        let bw = widgets::button_width(&ctx, bl, key, 14.0, false).max(px(140.0));
        self.button(
            ui,
            Rect::from_min_size(pos2(x, foot), vec2(bw, bh)),
            bl,
            if label_running { "" } else { key },
            if label_running { "tcal:stop" } else { act },
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                primary: !label_running && result.is_none_or(|r| !r.ok()) && !screen,
                ..Default::default()
            },
        );
        // the strums so far, or the finished run's
        let strip = Rect::from_min_size(
            pos2(x + bw + px(16.0), foot + px(8.0)),
            vec2((w - bw - px(16.0)).max(px(40.0)), px(30.0)),
        );
        match (live, result) {
            (Some(l), _) if l.mode != Mode::Along => {
                let res = tc::match_plucks(&tc::clicks_to(tc::MAX_SCORED), &l.plucks, COUNT_IN).residuals;
                run_strip(&p, strip, &res, None);
            }
            (Some(_), _) => dot_strip(&p, strip, &[]),
            (None, Some(r)) => run_strip(&p, strip, &r.residuals, Some(r.fit.offset)),
            _ => {}
        }
    }
}

/// How long a landing's flash lasts (s).
const FLASH_S: f64 = 0.30;

/// The flash when a dot lands on the target, or a click sounds: the target burns bright for a moment and a thin
/// ring spreads out from it, fast at first and then slowing, thinning and fading. All circles round the target's own
/// centre, so it is round whatever the lane's height; `reach` is how far the ring may spread (a little past the
/// lane, into the card's margins above and below, as it fades). `age`: seconds since it landed.
fn landing_flash(p: &egui::Painter, c: egui::Pos2, rad: f32, ring_r: f32, reach: f32, age: f64, color: egui::Color32) {
    let u = (age / FLASH_S).clamp(0.0, 1.0) as f32;
    let ease = 1.0 - (1.0 - u).powi(3);
    let fade = (1.0 - u).powf(1.4);
    p.circle_filled(c, rad, color.gamma_multiply(0.9 * (1.0 - u).powi(2)));
    p.circle_stroke(c, ring_r + ease * reach, (px(3.0) * (1.0 - u) + px(1.0), color.gamma_multiply(fade)));
}

/// The lane: a click lamp (it flashes on each click; dark in Screen timing) and dots coming in to a ring, one a
/// beat, drawn `disp` ahead (the screen's lag) in Screen timing and the fine-tune.
fn draw_lane(p: &egui::Painter, lane: Rect, l: &Live, disp: f64) {
    let th = t();
    let beat = l.beat();
    let period = tc::period();
    let draw_beat = if l.mode == Mode::Guitar {
        beat
    } else {
        beat + disp / period
    };
    // the dots (not in Guitar timing: there you strum on the sound): one a beat, coming in from the right along the
    // lane (a short lane has the width and not the height) to a ring; strum the moment a dot fills it. The ring
    // lights as each one arrives (the picture's own, so it moves with Screen like the dots do).
    let cy = lane.center().y;
    let rad = (lane.height() * 0.5 - px(6.0)).clamp(px(5.0), px(15.0));
    let ring_x = lane.min.x + px(96.0);
    let step = px(150.0);
    if l.mode != Mode::Guitar {
        let clip = p.with_clip_rect(lane.shrink(px(3.0)));
        let first = draw_beat.floor() as i64 - 1;
        // how long ago the newest dot reached the ring (s), while its flash lasts
        let mut age: Option<f64> = None;
        for k in first..first + 8 {
            if k < 0 || (l.mode == Mode::Screen && k >= tc::MAX_SCORED as i64) {
                continue;
            }
            let d = k as f64 - draw_beat; // beats still to come (- = gone by)
            let x = ring_x + d as f32 * step;
            if x > lane.max.x + rad {
                continue;
            }
            if d <= 0.0 && -d * period <= FLASH_S {
                age = Some(age.map_or(-d * period, |a: f64| a.min(-d * period)));
            }
            // (gone by: dim, and fading out; far ahead: fading in)
            let c = if d < -0.12 {
                th.track.gamma_multiply((1.0 + d as f32 * 1.5).clamp(0.0, 1.0))
            } else {
                th.accent.gamma_multiply((1.0 - (d as f32 - 3.0) * 0.25).clamp(0.2, 1.0))
            };
            clip.circle_filled(pos2(x, cy), rad, c);
        }
        let ring_c = pos2(ring_x, cy);
        let ring_r = rad + px(3.0);
        // (the flash as a dot lands)
        if let Some(a) = age {
            let reach = (lane.height() * 0.5 + px(9.0) - ring_r).max(px(6.0));
            landing_flash(p, ring_c, rad, ring_r, reach, a, th.accent);
        }
        p.circle_stroke(ring_c, ring_r, (px(2.0), th.text));
    }
    // the lamp (a click's, lit for the first part of each beat; dark in Screen timing), or the count-in's number
    let lamp = pos2(lane.min.x + px(28.0), cy);
    if l.mode != Mode::Guitar {
        let lit = if l.mode == Mode::Screen {
            0.0
        } else {
            (1.0 - (beat - beat.floor()) * 5.0).clamp(0.0, 1.0) as f32
        };
        if beat < 0.0 {
            text::draw(
                p,
                &format!("{}", (-beat).ceil() as i64),
                lamp,
                Align2::CENTER_CENTER,
                Style::new(18.0, 800, th.accent),
                None,
            );
        } else if lit > 0.05 {
            paint::led(p, lamp, 14.0, th.accent.gamma_multiply(0.3 + 0.7 * lit));
        } else {
            paint::dot(p, lamp, px(6.0), th.unlit);
        }
        return;
    }
    let frac = beat - beat.floor();
    if l.mode == Mode::Guitar {
        // (a short lane, as under the open fold: the lamp smaller and its count beside it, P38.4)
        let short = lane.height() < px(110.0);
        let rad = if short {
            (lane.height() * 0.3).min(px(30.0))
        } else {
            px(30.0)
        };
        let c = if short {
            pos2(lane.center().x - px(40.0), lane.center().y)
        } else {
            pos2(lane.center().x, lane.min.y + lane.height() * 0.42)
        };
        paint::dot(p, c, rad, th.unlit);
        // (the same round flash as the dots' landing, for each click)
        if (frac as f64) * period <= FLASH_S {
            let reach = ((c.y - lane.min.y).min(lane.max.y - c.y) + px(9.0) - rad).max(px(6.0));
            landing_flash(p, c, rad, rad, reach, frac as f64 * period, th.accent);
        }
        let what = if beat < 0.0 {
            format!("{}", (-beat).ceil() as i64)
        } else {
            format!("click {}", (beat.floor() as i64 + 1).min(tc::MAX_SCORED as i64))
        };
        text::draw(
            p,
            &what,
            if short {
                pos2(c.x + rad + px(36.0), c.y)
            } else {
                pos2(c.x, lane.max.y - px(22.0))
            },
            Align2::CENTER_CENTER,
            Style::new(14.0, 800, th.text_2),
            None,
        );
        return;
    }
}

/// A click run's strums as dots, -100 .. +100 ms. While it runs it is centred on the player's own running middle,
/// so the dots show how steady the strums are and not how far they are from the click: a player shown that starts to
/// correct it, and the run would measure the correction and not the setup. Once it is done (`done`: the offset it
/// found) it is centred on the click, with where the middle fell marked, and says early and late.
fn run_strip(p: &egui::Painter, strip: Rect, res: &[f64], done: Option<f64>) {
    let th = t();
    paint::inset(p, strip, Some(strip.height() / 2.0), 2.0, Fill::Base);
    let centre = if done.is_some() { 0.0 } else { tc::median_of(res) };
    let cx = strip.center().x;
    // (the words at the ends take room: the dots stay inside them, and not over them)
    let half = strip.width() / 2.0 - px(if done.is_some() { 46.0 } else { 58.0 });
    let at = |s: f64| cx + (((s - centre) * 1000.0 / 100.0).clamp(-1.0, 1.0) as f32) * half;
    p.vline(cx, strip.min.y + px(4.0)..=strip.max.y - px(4.0), (1.5, th.caption));
    if let Some(m) = done {
        let mx = at(m);
        p.vline(mx, strip.min.y + px(2.0)..=strip.max.y - px(2.0), (px(2.5), th.accent));
    }
    let n = res.len();
    for (i, r) in res.iter().enumerate() {
        let a = 0.35 + 0.65 * (i + 1) as f32 / n.max(1) as f32;
        let near = (r - done.unwrap_or(centre)).abs() <= 0.020;
        let col = if near { th.hit } else { th.late };
        paint::dot(p, pos2(at(*r), centre_y(strip)), px(4.0), col.gamma_multiply(a));
    }
    let (lo, hi) = if done.is_some() { ("early", "late") } else { ("earlier", "later") };
    for (w, al) in [(lo, Align2::LEFT_CENTER), (hi, Align2::RIGHT_CENTER)] {
        text::draw(
            p,
            w,
            pos2(
                if al == Align2::LEFT_CENTER {
                    strip.min.x + px(10.0)
                } else {
                    strip.max.x - px(10.0)
                },
                centre_y(strip),
            ),
            al,
            Style::new(10.0, 600, th.caption),
            None,
        );
    }
}

/// Residuals as dots on -60 .. +60 ms, the centre marked (late to the right); the newest brightest.
fn dot_strip(p: &egui::Painter, strip: Rect, res: &[f64]) {
    let th = t();
    paint::inset(p, strip, Some(strip.height() / 2.0), 2.0, Fill::Base);
    let cx = strip.center().x;
    p.vline(
        cx,
        strip.min.y + px(4.0)..=strip.max.y - px(4.0),
        (1.5, th.caption),
    );
    let at = |ms: f64| cx + ((ms / 60.0).clamp(-1.0, 1.0) as f32) * (strip.width() / 2.0 - px(10.0));
    let n = res.len();
    for (i, r) in res.iter().enumerate() {
        let a = 0.35 + 0.65 * (i + 1) as f32 / n.max(1) as f32;
        let col = if (r * 1000.0).abs() <= 10.0 {
            th.hit
        } else {
            th.late
        };
        paint::dot(
            p,
            pos2(at(r * 1000.0), centre_y(strip)),
            px(4.5),
            col.gamma_multiply(a),
        );
    }
    for (ms, al) in [(-60.0, Align2::LEFT_CENTER), (60.0, Align2::RIGHT_CENTER)] {
        text::draw(
            p,
            if ms < 0.0 { "early" } else { "late" },
            pos2(
                if ms < 0.0 {
                    strip.min.x + px(10.0)
                } else {
                    strip.max.x - px(10.0)
                },
                centre_y(strip),
            ),
            al,
            Style::new(10.0, 600, th.caption),
            None,
        );
    }
}

// -- the Output step's balance (P36.5) ---------------------------------------------------------------------------

/// The jam's drums playing to balance the guitar against (the song's level is the song volume).
pub(super) struct Band {
    engine: Arc<Engine>,
    saved: Option<JamSaved>,
}

impl Band {
    fn start(engine: Arc<Engine>) -> Band {
        let saved = JamSaved {
            bpm: engine.jam_tempo(),
            meter: engine.jam().beats_per_bar,
            click: engine.click(),
            drums: engine.drums(),
            backing: engine.backing(),
        };
        engine.set_click(ClickParams {
            on: false,
            ..saved.click
        });
        engine.set_drums(DrumParams {
            on: true,
            volume: 0.9,
            ..saved.drums
        });
        engine.set_jam_meter(4);
        engine.set_jam_tempo(100.0);
        engine.jam_play(0);
        Band {
            engine,
            saved: Some(saved),
        }
    }
}

impl Drop for Band {
    fn drop(&mut self) {
        let Some(s) = self.saved.take() else { return };
        let e = &self.engine;
        e.jam_stop();
        e.set_click(s.click);
        e.set_drums(s.drums);
        e.set_jam_meter(s.meter);
        e.set_jam_tempo(s.bpm);
        if s.backing.is_some() {
            e.set_backing(s.backing);
        }
    }
}

impl SettingsView {
    /// `band:toggle`: the drums on or off.
    pub(super) fn band_act(&mut self, st: &AppState) {
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let Some(g) = self.setup.as_mut() else { return };
        if g.timing.band.is_some() {
            g.timing.band = None;
        } else if let Some(e) = engine.filter(|_| !g.staged) {
            g.timing.live = None;
            g.timing.play.stop();
            g.timing.band = Some(Band::start(e));
        }
    }

    /// The height `draw_balance` takes.
    pub(super) fn h_balance(ctx: &egui::Context) -> f32 {
        px(24.0)
            + Self::h_heading(ctx)
            + line_h(ctx, 15.0)
            + px(16.0)
            + px(12.0)
            + px(14.0)
            + px(10.0)
            + 2.0 * line_h(ctx, 13.0)
    }

    /// BALANCE, under Hearing your guitar (from `y`, `w` wide): the song's volume against the guitar's, with drums
    /// to play along to. Returns its bottom.
    pub(super) fn draw_balance(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let y = y + px(24.0);
        p.hline(x..=x + w, y - px(12.0), (1.0, th.divider));
        let playing = self.setup.as_ref().is_some_and(|g| g.timing.band.is_some());
        let label = if playing { "Stop drums" } else { "Play drums" };
        let bw = widgets::button_width(&ctx, label, "D", 13.0, false).max(px(130.0));
        let yy = self.heading(&p, "BALANCE", x, y, None, "", "");
        self.button(
            ui,
            Rect::from_min_size(pos2(x + w - bw, y - px(6.0)), vec2(bw, px(32.0))),
            label,
            "D",
            "band:toggle",
            ButtonStyle {
                size: 13.0,
                r: Some(11.0),
                ..Default::default()
            },
        );
        let vol = st
            .audio
            .as_ref()
            .map(|a| a.engine.song_gain())
            .unwrap_or(st.config.f64("volume", 0.8) as f32);
        let sh = self.slider_row(
            ui,
            x,
            yy + px(4.0),
            w,
            "Song volume",
            &format!("{}%", (vol * 100.0).round() as i64),
            vol,
            "vol",
            "9 0",
        );
        super::caption_lines(
            &p,
            "Play along with the drums: you should hear yourself clearly. Turn the Song down before the guitar \
             goes past 150%.",
            x,
            yy + px(4.0) + sh + px(10.0),
            w,
            13.0,
            2,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_note_makes_sense_of_the_numbers() {
        let g = Instrument::Guitar;
        // the owner's run: strums 15 ms early on the dots, 27 ms early on the clicks: the screen runs 12 ms behind
        let w = screen_note(g, 0.012, 0.0, -0.015, -0.027);
        assert!(w.starts_with("Screen set to +12 ms (was 0 ms)."), "{w}");
        assert!(w.contains("15 ms early") && w.contains("27 ms early"), "{w}");
        assert!(w.contains("12 ms behind the sound") && w.contains("12 ms sooner"), "{w}");
        assert!(!w.contains("still"), "{w}");
        // with a Screen already set, the run drew the dots with it: what is left is "still" there
        let w = screen_note(g, 0.020, 0.008, 0.010, -0.002);
        assert!(w.contains("still 12 ms behind") && w.contains("+8 ms"), "{w}");
        // a screen that keeps up
        let w = screen_note(g, 0.0, 0.0, 0.020, 0.019);
        assert!(w.contains("keeps up with the sound"), "{w}");
        // the other way
        let w = screen_note(g, -0.01, 0.0, 0.010, 0.020);
        assert!(w.contains("10 ms ahead of the sound") && w.contains("10 ms later"), "{w}");
        // short enough for two lines under the cards
        assert!(screen_note(g, -0.011, -0.050, 0.019, -0.020).chars().count() < 230);
    }
}
