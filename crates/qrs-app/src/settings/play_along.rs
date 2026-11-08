//! The setup's play-along (P38.4, D159-D160; new code, no Python source): the Timing step's default and
//! Settings > Timing's Calibrate timing. The owner: "show the tab and say 'can you play this?'".
//!
//!     PLAY THIS        a tab of the tune (`playcheck.rs`: Ode to Joy, four bars, first position) drawn the way
//!                      the player's tab is (strings named in their colours, fret numbers on knock-outs, the bar
//!                      being played lighter, a glowing cursor), the next note ringed, each note marked as it's
//!                      judged (✓ / early / late / wrong / missed)
//!     what to do       a small picture (a line sweeping across three notes, each lit as it's reached) and a line
//!     Listen first     the tune on the synth guitar with its count-in and click, the cursor following it
//!     Play it          a bar of click, then the tune with the click and soft drums; the scorer judges each note
//!     the result       "You play about 18 ms behind the beat: quickRS shifts your timing by 18 ms" (the delay set
//!                      at once, Undo) and "14 of 15 notes heard; the D on the B string was quiet: pick a bit harder"
//!     Advanced         the click tool (`timing_tool.rs`), for the screen's timing and the fine-tune
//!
//! The jam is taken as the click tool takes it (`timing_tool::Live`: the plucks held from an open song, the jam put
//! back when the run ends or is dropped). A pluck's beat is read off the jam's clock; the scorer gets it at that
//! time plus the delay already set (as a song would score it), the timing is measured on the plucks as heard.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, Rect, Ui, pos2, vec2};
use qrs_analysis::instrument::Instrument;
use qrs_analysis::{Pluck, Scorer};
use qrs_audio::rt::Clip;
use qrs_audio::{Engine, PreviewState};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{judge_color, string_color, t};
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle};

use super::timing::delay_label;
use super::timing_tool::{Live, Mode, save_guitar_run};
use super::{SettingsView, line_h, snap};
use crate::playcheck::{self as pc, Mark, Outcome, TuneNote};
use crate::state::AppState;
use crate::timing_cal as tc;

/// config.json: whether the timing step's Advanced fine-tuning fold is open (additive; absent: rolled up).
pub(super) const FOLD_KEY: &str = "timing_fine_open";

/// "Listen first": the tune rendered off the UI thread, then played as a preview clip.
struct Hear {
    engine: Arc<Engine>,
    ready: Arc<Mutex<Option<Clip>>>,
    id: Option<u64>,
    started: Instant,
}

impl Hear {
    fn start(engine: Arc<Engine>, inst: Instrument) -> Hear {
        let ready = Arc::new(Mutex::new(None));
        let rate = engine.rate();
        let tone = qrs_audio::synth::synth_tone(&engine.tone(), inst.is_bass());
        let slot = ready.clone();
        let _ = std::thread::Builder::new()
            .name("qrs-tune".into())
            .spawn(move || {
                let samples = pc::render(inst, rate, Some(tone));
                if let Ok(mut s) = slot.lock() {
                    *s = Some(Clip { rate, samples });
                }
            });
        Hear {
            engine,
            ready,
            id: None,
            started: Instant::now(),
        }
    }

    /// Once a frame: the clip started when it's ready; false when it's over.
    fn tick(&mut self) -> bool {
        match self.id {
            None => {
                let clip = self.ready.lock().ok().and_then(|mut s| s.take());
                if let Some(c) = clip {
                    self.id = Some(self.engine.play_preview(c, false, false));
                }
                self.started.elapsed() < Duration::from_secs(10)
            }
            Some(id) => self.engine.preview_state(id) != PreviewState::Over,
        }
    }

    /// The beat it's playing (negative in its count-in).
    fn beat(&self) -> f64 {
        match self.id {
            Some(id) if self.engine.preview_state(id) == PreviewState::Playing => {
                self.engine.preview_position().0 / pc::period() - pc::COUNT_IN as f64
            }
            _ => -(pc::COUNT_IN as f64),
        }
    }
}

impl Drop for Hear {
    fn drop(&mut self) {
        if let Some(id) = self.id
            && self.engine.preview_state(id) != PreviewState::Over
        {
            self.engine.stop_preview(false);
        }
    }
}

/// "Play it" under way.
struct Run {
    live: Live,
    scorer: Scorer,
    tune: Vec<TuneNote>,
    /// The plucks (s from the count-in's first click, as heard: before the delay).
    plucks: Vec<f64>,
    /// Per note: the pitch the tuner read just after it, the loudest level around it.
    heard: Vec<Option<f64>>,
    loudest: Vec<f32>,
}

impl Run {
    fn start(engine: Arc<Engine>, inst: Instrument) -> Run {
        engine.set_onset_bass(inst.is_bass());
        let arr = pc::arrangement(inst);
        let mut scorer = Scorer::new(&arr);
        scorer.set_instrument(inst);
        let tune = pc::tune(inst);
        let n = tune.len();
        Run {
            live: Live::start_at(engine, Mode::Guitar, pc::BPM, true),
            scorer,
            tune,
            plucks: Vec::new(),
            heard: vec![None; n],
            loudest: vec![0.0; n],
        }
    }

    /// Once a frame: the plucks to the scorer, the pitch and level around each note (`midi`, `level`). `io`: the
    /// delay set (`input_offset`). True once the tune is over.
    fn tick(&mut self, midi: Option<f64>, level: f32, io: f64) -> bool {
        let Some(e) = self.live.engine.clone() else {
            return false;
        };
        let clock = e.jam_clock();
        let period = pc::period();
        let mut feed = Vec::new();
        for o in e.take_onsets() {
            if !clock.playing {
                continue;
            }
            let s = (clock.at(o.wall_ns) + pc::COUNT_IN as f64) * period;
            self.plucks.push(s);
            feed.push(Pluck {
                index: o.index,
                song_time: tc::chart_pluck(s, 0.0, io),
            });
        }
        self.scorer.feed(feed);
        let beat = self.live.beat();
        let latency = e.status().input.as_ref().map(|i| i.latency).unwrap_or(0.0);
        let live = qrs_analysis::detect::Live {
            source: e.capture(),
            level,
            latency,
        };
        let now = (beat + pc::COUNT_IN as f64) * period;
        self.scorer
            .update(now, Some(&live), qrs_audio::clock::now_ns() as f64 / 1e9);
        for (k, n) in self.tune.iter().enumerate() {
            if beat >= n.beat - 0.15 && beat <= n.beat + 0.9 {
                self.loudest[k] = self.loudest[k].max(level);
            }
            // (the pitch a little after the pluck: the note before has stopped ringing over it)
            if beat >= n.beat + 0.2 && beat <= n.beat + 0.8 && self.heard[k].is_none() {
                self.heard[k] = midi;
            }
        }
        let end = pc::length_beats(&self.tune) + 0.3;
        beat >= end || self.live.started.elapsed().as_secs_f64() > (end + pc::COUNT_IN as f64) * period + 6.0
    }

    /// Each note's mark so far (None: not reached or not judged yet).
    fn marks(&self) -> Vec<Option<Mark>> {
        let times: Vec<f64> = self.tune.iter().map(|n| n.time()).collect();
        let m = pc::match_plucks(&times, &self.plucks);
        self.tune
            .iter()
            .enumerate()
            .map(|(k, n)| {
                let scored = self.scorer.groups.get(k).and_then(|g| g.state)?;
                Some(pc::mark(m.dt[k], Some(scored), n.midi(), None))
            })
            .collect()
    }

    /// The run's verdict. `onset`: the pluck threshold (RMS), for "too soft".
    fn outcome(&self, onset: f32) -> Outcome {
        let times: Vec<f64> = self.tune.iter().map(|n| n.time()).collect();
        let m = pc::match_plucks(&times, &self.plucks);
        let played = self
            .tune
            .iter()
            .enumerate()
            .map(|(k, n)| {
                let dt = m.dt[k];
                let scored = self.scorer.groups.get(k).and_then(|g| g.state);
                let mark = pc::mark(dt, scored, n.midi(), self.heard[k]);
                pc::Played {
                    note: *n,
                    result: tc::NoteResult {
                        expected: n.midi(),
                        dt,
                        plucks: m.plucks[k],
                        // (a note the scorer took as itself is in tune; the tuner's reading may be the one before)
                        heard: if mark.heard() {
                            Some(n.midi() as f64)
                        } else {
                            self.heard[k]
                        },
                        soft: dt.is_none() && self.loudest[k] > onset * 0.316 && self.loudest[k] <= onset,
                    },
                    mark,
                }
            })
            .collect();
        pc::assess(played, m.doubles)
    }
}

/// Screenshots: what's shown.
struct Staged {
    beat: f64,
    listening: bool,
    marks: Vec<Option<Mark>>,
}

/// The play-along's state (the timing tool's: it goes with the guide).
#[derive(Default)]
pub(super) struct PlayAlong {
    hear: Option<Hear>,
    run: Option<Run>,
    pub outcome: Option<Outcome>,
    /// What it last said beyond a result (no input, no engine).
    note: Option<String>,
    /// The delay the last trusted run set (screenshots: as if set).
    set: Option<f64>,
    staged: Option<Staged>,
}

impl PlayAlong {
    /// Screenshots: "playlisten" (Listen first, in bar 2), "playing" (Play it, mid-tune, the notes so far
    /// judged), "playresult" (done: 18 ms behind, one quiet note); else fresh.
    pub fn staged(what: &str) -> PlayAlong {
        let mut p = PlayAlong::default();
        let marks = |v: &[Mark]| -> Vec<Option<Mark>> { (0..15).map(|k| v.get(k).copied()).collect() };
        match what {
            "playlisten" => {
                p.staged = Some(Staged {
                    beat: 5.35,
                    listening: true,
                    marks: marks(&[]),
                })
            }
            "playing" => {
                use Mark::*;
                p.staged = Some(Staged {
                    beat: 6.12,
                    listening: false,
                    marks: marks(&[Hit, Hit, Wrong, Hit, Late, Hit, Hit]),
                })
            }
            "playresult" => {
                // (made for the instrument when it's drawn: `outcome` is set there)
                p.staged = Some(Staged {
                    beat: 99.0,
                    listening: false,
                    marks: Vec::new(),
                });
            }
            _ => {}
        }
        p
    }

    /// Listening or playing.
    pub fn busy(&self) -> bool {
        self.hear.is_some() || self.run.is_some()
    }

    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Stop listening or playing (the jam and the preview put back).
    pub fn stop(&mut self) {
        self.hear = None;
        self.run = None;
    }
}

impl SettingsView {
    /// The play-along's actions: `pa:listen` (again: stop), `play`, `stop`, `advanced` (the click tool, and back).
    pub(super) fn play_act(&mut self, st: &mut AppState, arg: &str) {
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let input = crate::calibrate::current_input(st).is_some();
        let inst = st.instrument();
        let io = st.config.f64("input_offset", 0.0);
        let disp = st.config.f64("display_offset", 0.0);
        let Some(g) = self.setup.as_mut() else { return };
        let staged = g.staged;
        let tool = &mut g.timing;
        match arg {
            "advanced" => {
                // (rolled up or down, remembered: config.json `timing_fine_open`; screenshots keep theirs)
                tool.stop();
                tool.advanced = !tool.advanced;
                if !staged {
                    st.config.set(FOLD_KEY, tool.advanced);
                    st.save_config();
                }
            }
            "stop" => tool.play.stop(),
            "listen" if tool.play.hear.is_some() => tool.play.stop(),
            "listen" | "play" if !staged => {
                let Some(e) = engine else {
                    tool.play.note = Some("The audio engine isn't running.".into());
                    return;
                };
                if arg == "play" && !input {
                    tool.play.note =
                        Some("No input is listening: pick yours in Settings > Sound first.".into());
                    return;
                }
                tool.live = None;
                tool.band = None;
                tool.play.stop();
                tool.play.note = None;
                if arg == "listen" {
                    tool.play.hear = Some(Hear::start(e, inst));
                } else {
                    tool.was.get_or_insert((io, disp));
                    tool.play.outcome = None;
                    tool.play.set = None;
                    tool.play.run = Some(Run::start(e, inst));
                }
            }
            _ => {}
        }
    }

    /// Once a frame on the play-along: the listening and the run moved on; a run that's over judged, and its
    /// timing set when it's trusted.
    fn play_tick(&mut self, st: &mut AppState, ctx: &egui::Context) {
        let io = st.config.f64("input_offset", 0.0);
        let inst = st.instrument();
        let mic = st.mic_input();
        let onset = crate::calibrate::current_input(st)
            .map(|(id, _)| crate::calibrate::load(&st.config, &id).onset_or_default(mic))
            .unwrap_or(crate::calibrate::ONSET_GUITAR);
        let level = st.audio.as_ref().map(|a| a.engine.meters().level).unwrap_or(0.0);
        let Some(g) = self.setup.as_mut() else { return };
        let midi = g.listen.midi;
        let play = &mut g.timing.play;
        if play.staged.as_ref().is_some_and(|s| s.beat > 90.0) && play.outcome.is_none() {
            // (screenshots: the result, for the instrument shown)
            let o = pc::example(inst);
            play.set = Some(tc::guitar_offset(o.fit.offset));
            play.outcome = Some(o);
        }
        if let Some(h) = &mut play.hear {
            ctx.request_repaint();
            if !h.tick() {
                play.hear = None;
            }
        }
        let Some(run) = &mut play.run else { return };
        ctx.request_repaint();
        if !run.tick(midi, level, io) {
            return;
        }
        let o = run.outcome(onset);
        play.run = None;
        if o.ok() {
            play.set = Some(save_guitar_run(st, &o.fit));
        }
        if let Some(g) = self.setup.as_mut() {
            g.timing.play.outcome = Some(o);
        }
    }

    /// The play-along in the step's `area`: the tab card, the result under it, Advanced at the bottom.
    pub(super) fn guide_play(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let ctx = ui.ctx().clone();
        self.play_tick(st, &ctx);
        let th = t();
        let p = ui.painter().clone();
        let inst = st.instrument();
        let tune = pc::tune(inst);
        let Some(g) = self.setup.as_ref() else { return };
        let staged = g.staged;
        let play = &g.timing.play;
        // what's shown: the beat (None: nothing playing), listening or playing, each note's mark
        let (beat, listening, marks): (Option<f64>, bool, Vec<Option<Mark>>) = if let Some(s) = &play.staged
            && s.beat < 90.0
        {
            (Some(s.beat), s.listening, s.marks.clone())
        } else if let Some(r) = &play.run {
            (Some(r.live.beat()), false, r.marks())
        } else if let Some(h) = &play.hear {
            (Some(h.beat()), true, vec![None; tune.len()])
        } else if let Some(o) = &play.outcome {
            (None, false, o.played.iter().map(|p| Some(p.mark)).collect())
        } else {
            (None, false, vec![None; tune.len()])
        };
        let outcome = play.outcome.clone();
        let note = play.note.clone();
        let set = play.set;
        let can_undo = g.timing.was.is_some();
        let running = beat.is_some() && !listening;
        let pad = px(20.0);
        let n_strings = inst.open_strings().len();
        let gap = px(22.0);
        let tab_h = px(44.0) + (n_strings - 1) as f32 * gap + px(40.0);
        let row_h = px(72.0);
        let hh = Self::h_heading(&ctx);
        let card_h = 2.0 * pad + hh + tab_h + px(16.0) + row_h;
        let card = Rect::from_min_size(area.min, vec2(area.width(), card_h.min(area.height())));
        paint::card(&p, card, 24.0, false);
        let (x, w) = (card.min.x + pad, card.width() - 2.0 * pad);
        let yy = self.heading(
            &p,
            "PLAY THIS",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &format!(
                "{} ({}) \u{b7} {} bars \u{b7} {:.0} bpm",
                pc::TUNE_NAME,
                pc::TUNE_BY,
                pc::bars(&tune),
                pc::BPM
            ),
            "",
        );
        let well = Rect::from_min_size(pos2(x, yy), vec2(w, tab_h));
        draw_tab(&p, well, inst, &tune, beat, listening, &marks);
        // what to do: the picture, the words, Listen first and Play it
        let ry = well.max.y + px(16.0);
        let pic = Rect::from_min_size(pos2(x, ry), vec2(px(190.0), row_h));
        let phase = if staged {
            0.62
        } else {
            ctx.request_repaint_after(Duration::from_millis(33));
            (ctx.input(|i| i.time) / 3.0).fract()
        };
        draw_picture(&p, pic, phase as f32);
        let listen_l = if listening { "Stop" } else { "Listen first" };
        let play_l = if running {
            "Stop"
        } else if outcome.is_some() {
            "Play again"
        } else {
            "Play it"
        };
        let bh = px(46.0);
        let pw = widgets::button_width(&ctx, play_l, "Enter", 14.0, false).max(px(140.0));
        let lw = widgets::button_width(&ctx, listen_l, "L", 14.0, false).max(px(140.0));
        let by = ry + (row_h - bh) / 2.0;
        let play_r = Rect::from_min_size(pos2(x + w - pw, by), vec2(pw, bh));
        let listen_r = Rect::from_min_size(pos2(play_r.min.x - px(10.0) - lw, by), vec2(lw, bh));
        let done_ok = outcome.as_ref().is_some_and(|o| o.ok());
        self.button(
            ui,
            listen_r,
            listen_l,
            "L",
            if listening { "pa:stop" } else { "pa:listen" },
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        self.button(
            ui,
            play_r,
            play_l,
            if running {
                "Esc"
            } else if done_ok {
                "P"
            } else {
                "Enter"
            },
            if running { "pa:stop" } else { "pa:play" },
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                primary: !running && !done_ok,
                ..Default::default()
            },
        );
        let tx = pic.max.x + px(18.0);
        let tw = listen_r.min.x - px(18.0) - tx;
        let (head, sub) = match beat {
            Some(b) if b < 0.0 => (
                format!("Get ready: {}", (-b).ceil() as i64),
                if listening {
                    "Four clicks count it in. Listen to how it goes."
                } else {
                    "Four clicks count you in. Play the first note on the next one."
                }
                .to_string(),
            ),
            Some(_) if listening => (
                "Listen: this is how it goes.".to_string(),
                "Then Play it, and play along with the click.".to_string(),
            ),
            Some(_) => (
                "Play each note as the line reaches it.".to_string(),
                "Don't worry about mistakes: keep going to the end.".to_string(),
            ),
            None => (
                "Play the notes as the line reaches them.".to_string(),
                format!(
                    "The numbers are frets: 0 is the open string. Don't worry about mistakes. {}",
                    if inst.is_bass() {
                        "On the D, G and A strings."
                    } else {
                        "All on the G and B strings."
                    }
                ),
            ),
        };
        let hr = text::draw(
            &p,
            &head,
            pos2(tx, ry + px(6.0)),
            Align2::LEFT_TOP,
            Style::new(15.0, 800, th.text),
            Some(tw),
        );
        let sst = Style::new(13.0, 500, th.text_2);
        let mut ly = hr.max.y + px(4.0);
        for l in text::wrap(&ctx, &sub, sst, tw).iter().take(2) {
            ly = text::draw(&p, l, pos2(tx, ly), Align2::LEFT_TOP, sst, None).max.y;
        }
        // the result (or why it couldn't start)
        let mut y = card.max.y + px(16.0);
        let lh = line_h(&ctx, 14.0);
        let undo_w = widgets::button_width(&ctx, "Undo", "", 13.0, false).max(px(110.0));
        let rw = area.width() - px(8.0) - undo_w - px(16.0);
        if let Some(o) = &outcome {
            let delay = delay_label(set.unwrap_or(st.config.f64("input_offset", 0.0)));
            let lines = [
                (
                    pc::timing_words(o, inst, &delay),
                    if o.ok() { th.hit } else { th.late },
                ),
                (
                    pc::notes_words(o, inst),
                    if o.heard() == o.total() {
                        th.text_2
                    } else {
                        th.late
                    },
                ),
            ];
            let y0 = y;
            for (s, col) in lines {
                for l in text::wrap(&ctx, &s, Style::new(14.0, 700, col), rw)
                    .iter()
                    .take(2)
                {
                    y = text::draw(
                        &p,
                        l,
                        pos2(area.min.x + px(4.0), y),
                        Align2::LEFT_TOP,
                        Style::new(14.0, 700, col),
                        None,
                    )
                    .max
                    .y;
                }
                y += px(4.0);
            }
            if o.ok() && can_undo {
                self.button(
                    ui,
                    Rect::from_min_size(pos2(area.max.x - undo_w, y0 - px(2.0)), vec2(undo_w, px(30.0))),
                    "Undo",
                    "",
                    "tcal:undo",
                    ButtonStyle {
                        size: 13.0,
                        r: Some(12.0),
                        plain: true,
                        ..Default::default()
                    },
                );
            }
        } else if let Some(n) = &note {
            text::draw(
                &p,
                n,
                pos2(area.min.x + px(4.0), y),
                Align2::LEFT_TOP,
                Style::new(14.0, 700, th.late),
                Some(rw),
            );
            y += lh;
        }
        // Advanced fine-tuning, rolled up, at the bottom
        let fh = px(widgets::FOLD_ROW_H);
        let fy = (area.max.y - fh).max(y + px(8.0));
        self.timing_fold(
            ui,
            Rect::from_min_size(pos2(area.min.x, fy), vec2(area.width(), fh)),
            false,
        );
    }

    /// The timing step's "Advanced fine-tuning" fold (owner, P38.4: simple first, the rest rolled up on the same
    /// page): its row in `r`, toggled by a click or A.
    pub(super) fn timing_fold(&mut self, ui: &Ui, r: Rect, open: bool) {
        if widgets::fold_row(
            ui,
            r,
            "timing-fine-fold",
            "Advanced fine-tuning",
            "Feel like your playing is a bit off? Fine-tune your exact response time here.",
            open,
        )
        .clicked()
        {
            self.acts.push("pa:advanced".into());
        }
    }

    /// The play-along in one row, above the open fold: the tune, its last result, and the way back to its tab
    /// (which rolls the fold up). Returns its bottom.
    pub(super) fn play_strip(&mut self, ui: &Ui, st: &AppState, area: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let inst = st.instrument();
        let pad = px(18.0);
        let card = Rect::from_min_size(area.min, vec2(area.width(), px(64.0)));
        paint::card(&p, card, 20.0, false);
        let label = "Show the tune";
        let bw = widgets::button_width(&ctx, label, "", 14.0, false).max(px(150.0));
        self.button(
            ui,
            Rect::from_min_size(
                pos2(card.max.x - pad - bw, card.center().y - px(20.0)),
                vec2(bw, px(40.0)),
            ),
            label,
            "",
            "pa:advanced",
            ButtonStyle {
                size: 14.0,
                r: Some(13.0),
                ..Default::default()
            },
        );
        let cr = text::caps(
            &p,
            "PLAY THIS",
            pos2(card.min.x + pad, card.center().y),
            12.0,
            th.caption,
            Align2::LEFT_CENTER,
            None,
        );
        let outcome = self.setup.as_ref().and_then(|g| g.timing.play.outcome.clone());
        let set = self.setup.as_ref().and_then(|g| g.timing.play.set);
        let (line, col) = match &outcome {
            Some(o) => (
                pc::timing_words(
                    o,
                    inst,
                    &delay_label(set.unwrap_or(st.config.f64("input_offset", 0.0))),
                ),
                if o.ok() { th.hit } else { th.late },
            ),
            None => (
                format!(
                    "{} ({}): the step everyone plays. Its tab is rolled up while the fine-tuning is open.",
                    pc::TUNE_NAME,
                    pc::TUNE_BY
                ),
                th.text_2,
            ),
        };
        let tx = cr.max.x + px(16.0);
        text::draw(
            &p,
            &line,
            pos2(tx, card.center().y),
            Align2::LEFT_CENTER,
            Style::new(13.0, 700, col),
            Some(card.max.x - pad - bw - px(16.0) - tx),
        );
        card.max.y
    }
}

/// x of a beat across the tab's notes (`x0` the first beat, `bw` a beat's width).
fn beat_x(x0: f32, bw: f32, beat: f64) -> f32 {
    x0 + beat as f32 * bw
}

/// The tune's tab in `well`, as the player's tab draws: the strings named in their colours (the highest at the
/// top), bar lines with their numbers, the bar under the cursor lighter, fret numbers on knock-outs coloured by
/// how they were played, the note to play next ringed, the cursor (the accent's glowing line; white for the
/// synth, as the tab's selection cursor), a mark under each judged note, the count-in's number.
fn draw_tab(
    p: &egui::Painter,
    well: Rect,
    inst: Instrument,
    tune: &[TuneNote],
    beat: Option<f64>,
    listening: bool,
    marks: &[Option<Mark>],
) {
    let th = t();
    paint::display(p, well, Some(16.0), 3.0);
    let opens = inst.open_strings();
    let n = opens.len();
    let gap = px(22.0);
    let y0 = well.min.y + px(44.0);
    let ys = |s: usize| snap(y0 + (n - 1 - s) as f32 * gap);
    let names_x = well.min.x + px(22.0);
    let left = well.min.x + px(40.0);
    let right = well.max.x - px(16.0);
    let beats = pc::length_beats(tune) as f32;
    let lead = px(22.0);
    let bw = (right - left - lead - px(12.0)) / beats;
    let x0 = left + lead;
    let bar_beats = pc::BEATS_PER_BAR as f64;
    let cur_bar = beat.map(|b| (b.max(0.0) / bar_beats).floor() as usize);
    let staff = Rect::from_min_max(pos2(left, ys(n - 1)), pos2(right, ys(0)));
    // the bar under the cursor: a lighter band
    if let Some(cb) = cur_bar
        && cb < pc::bars(tune)
    {
        let bx0 = beat_x(x0, bw, cb as f64 * bar_beats) - lead * 0.6;
        let bx1 = beat_x(x0, bw, (cb + 1) as f64 * bar_beats) - lead * 0.6;
        let band = Rect::from_min_max(
            pos2(bx0, staff.min.y - px(30.0)),
            pos2(bx1.min(right), staff.max.y + px(14.0)),
        );
        p.rect_filled(band, px(10.0), th.screen_hi);
    }
    // strings and their names ("E A D G B e")
    let names: Vec<&'static str> = opens.iter().map(|&m| qrs_formats::model::note_name(m)).collect();
    let labels = qrs_formats::model::string_labels(&names, inst.is_bass());
    for s in 0..n {
        let y = ys(s);
        p.hline(left..=right, y, (1.0, th.line));
        text::draw(
            p,
            labels[s],
            pos2(names_x, y),
            Align2::CENTER_CENTER,
            Style::new(13.0, 800, string_color(s)),
            None,
        );
    }
    // bar lines and numbers
    for b in 0..=pc::bars(tune) {
        let bx = snap(if b == pc::bars(tune) {
            right
        } else {
            beat_x(x0, bw, b as f64 * bar_beats) - lead * 0.6
        });
        p.vline(bx, staff.min.y..=staff.max.y, (1.0, th.bar_line));
        if b < pc::bars(tune) {
            text::draw(
                p,
                &(b + 1).to_string(),
                pos2(bx + px(6.0), staff.min.y - px(12.0)),
                Align2::LEFT_BOTTOM,
                Style::new(12.0, 700, th.caption),
                None,
            );
        }
    }
    // the cursor, under the notes
    if let Some(b) = beat {
        let cx = snap(beat_x(x0, bw, b.max(-0.6)).min(right));
        let bar = Rect::from_min_max(
            pos2(cx - px(1.5), staff.min.y - gap * 0.64),
            pos2(cx + px(1.5), staff.max.y + gap * 0.46),
        );
        let col = if listening { th.text } else { th.accent };
        if !listening {
            paint::glow(p, bar, th.accent, 6.0, None, None);
        }
        p.rect_filled(bar, px(1.0), col);
        let mk = px(6.0);
        p.add(egui::Shape::convex_polygon(
            vec![
                pos2(cx - mk, bar.min.y - px(8.0)),
                pos2(cx + mk, bar.min.y - px(8.0)),
                pos2(cx, bar.min.y - px(1.0)),
            ],
            col,
            egui::Stroke::NONE,
        ));
    }
    // the note to play next: the first not yet reached (the first of all before a run)
    let next = match beat {
        Some(b) => tune.iter().position(|nt| nt.beat > b - 0.12),
        None if marks.iter().all(Option::is_none) => Some(0),
        None => None,
    };
    let opens_idx = |open: i32| pc::string_of(&opens, open);
    let fret_st = |c: Color32| Style::new(15.0, 800, c);
    for (k, nt) in tune.iter().enumerate() {
        let cx = snap(beat_x(x0, bw, nt.beat));
        let cy = ys(opens_idx(nt.open));
        let now_note = beat.is_some_and(|b| (nt.beat - b).abs() * pc::period() <= 0.06);
        let mark = marks.get(k).copied().flatten();
        let col = if now_note {
            th.on_accent
        } else if let Some(m) = mark {
            judge_color(m.judge())
        } else if beat.is_some_and(|b| nt.beat < b - 0.1) {
            th.caption
        } else {
            th.text
        };
        let chip = Rect::from_center_size(pos2(cx, cy), vec2(px(24.0), px(22.0)));
        if now_note {
            paint::glow(p, chip, th.accent, 6.0, None, Some(11.0));
            p.rect_filled(chip, px(11.0), th.accent);
        } else {
            let in_bar = cur_bar == Some((nt.beat / bar_beats).floor() as usize);
            p.rect_filled(chip, px(11.0), if in_bar { th.screen_hi } else { th.screen });
        }
        if next == Some(k) && !now_note {
            paint::ring(p, chip.expand(px(3.0)), Some(14.0), 2.0, th.accent, false);
        }
        text::draw(
            p,
            &nt.fret.to_string(),
            chip.center(),
            Align2::CENTER_CENTER,
            fret_st(col),
            None,
        );
        // (a held note: its length as a faint line along the string)
        if nt.beats > 1.0 {
            let x1 = beat_x(x0, bw, nt.beat + nt.beats) - lead;
            p.hline(chip.max.x + px(2.0)..=x1, cy, (3.0, th.track));
        }
        if let Some(m) = mark {
            text::draw(
                p,
                m.word(),
                pos2(cx, staff.max.y + px(24.0)),
                Align2::CENTER_CENTER,
                Style::new(11.0, 800, judge_color(m.judge())),
                None,
            );
        }
    }
    // the count-in's number
    if let Some(b) = beat
        && b < 0.0
    {
        text::draw(
            p,
            &format!("{}", (-b).ceil() as i64),
            pos2(well.max.x - px(40.0), well.min.y + px(24.0)),
            Align2::CENTER_CENTER,
            Style::new(28.0, 800, th.accent_ink),
            None,
        );
    }
}

/// What to do, as a picture: a string with three notes and a line sweeping across them (`phase` 0..1 of its pass),
/// each note lit as the line reaches it, a pick's stroke under it.
fn draw_picture(p: &egui::Painter, r: Rect, phase: f32) {
    let th = t();
    paint::display(p, r, Some(14.0), 3.0);
    let y = snap(r.center().y - px(6.0));
    let (l, rr) = (r.min.x + px(16.0), r.max.x - px(16.0));
    p.hline(l..=rr, y, (1.0, th.line));
    let xs = [0.25f32, 0.5, 0.75].map(|f| snap(l + (rr - l) * f));
    let cx = l + (rr - l) * (phase * 1.1 - 0.05).clamp(0.0, 1.0);
    for (i, &x) in xs.iter().enumerate() {
        let lit = (cx - x).abs() < px(8.0);
        let past = cx > x + px(8.0);
        let chip = Rect::from_center_size(pos2(x, y), vec2(px(20.0), px(18.0)));
        if lit {
            paint::glow(p, chip, th.accent, 5.0, None, Some(9.0));
        }
        p.rect_filled(chip, px(9.0), if lit { th.accent } else { th.screen });
        text::draw(
            p,
            ["0", "1", "3"][i],
            chip.center(),
            Align2::CENTER_CENTER,
            Style::new(
                12.0,
                800,
                if lit {
                    th.on_accent
                } else if past {
                    th.hit
                } else {
                    th.text
                },
            ),
            None,
        );
        if past {
            text::draw(
                p,
                "\u{2713}",
                pos2(x, y + px(22.0)),
                Align2::CENTER_CENTER,
                Style::new(11.0, 800, th.hit),
                None,
            );
        }
    }
    let line = Rect::from_min_max(pos2(cx - px(1.0), y - px(18.0)), pos2(cx + px(1.0), y + px(14.0)));
    paint::glow(p, line, th.accent, 4.0, None, None);
    p.rect_filled(line, 0.0, th.accent);
    text::draw(
        p,
        "play as the line reaches each note",
        pos2(r.center().x, r.max.y - px(9.0)),
        Align2::CENTER_CENTER,
        Style::new(10.0, 600, th.caption),
        None,
    );
}
