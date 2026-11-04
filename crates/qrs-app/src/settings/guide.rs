//! The guided guitar setup, drawn in Settings > Audio (P23.8, owner request; new code, no Python source). Eight
//! steps on the Audio page's own pieces: the input (its device list and channel meters), the output (and hearing
//! the guitar), a level check in words, the kind of input (D33), Calibrate (P23.10: Rocksmith's way, knobs up and
//! the hardest strumming you can do), the settings it recommends as sliders to adjust while you play, a note check
//! (the tuner's pitch reading) and a summary. Back, Skip and Next on every step; Enter, Backspace, S and Esc as keys,
//! digits for a step's choices. Everything it sets is saved per input as the Audio page saves it (`input_cal`,
//! `audio`, `tone_by_input`); Finish records config.json `guitar_setup` = "done" (`guitar_setup.rs`).
//!
//! The Audio page's own Calibrate runs the same two steps (Calibrate, then the settings) on their own.

use std::time::Instant;

use eframe::egui;
use egui::{Align2, CursorIcon, Event, Key, Rect, Sense, Ui, pos2, vec2};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::knobs;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle, interact};

use super::audio::{device_status, input_rows, level_meter, output_rows, status_line};
use super::{SettingsView, centre_y, line_h, snap};
use crate::calibrate::{self, InputCal, Kind, Outcome, Phase, Run, Verdict};
use crate::guitar_setup::{self as gs, Flow, Level, LevelWatch, Mark, Step};
use crate::listen::Listen;
use crate::state::AppState;

/// The setup under way.
pub(super) struct Guide {
    pub flow: Flow,
    watch: LevelWatch,
    listen: Listen,
    /// The open strings heard in the note check, and whether any note was.
    heard: [bool; 6],
    any_note: bool,
    t0: Instant,
    /// What the level check last said (not "play your guitar"), for the summary.
    level: Option<Level>,
    /// Screenshots: a level, a calibration and notes made up for the step shown.
    staged: bool,
    /// The Audio page's Calibrate: only Calibrate and the settings.
    only_cal: bool,
    /// Screenshots: the calibration the sliders show.
    staged_cal: Option<InputCal>,
    /// When a pluck last went over the threshold (the settings' pluck light).
    pluck_at: Option<Instant>,
    /// Run inside the first-time setup's guitar page (P23.14): Close closes the setup, Back from the first step and
    /// Finish go back to it (`SettingsView::guide_exit`).
    pub embedded: bool,
}

/// How a guide run inside the first-time setup was left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuideExit {
    /// Close (Esc): the whole setup closes.
    Close,
    /// Back from its first step: the guitar page's start.
    Back,
    /// Finish on its summary: on to the setup's next page.
    Finish,
}

/// The steps the Audio page's Calibrate shows.
const CAL_STEPS: [Step; 2] = [Step::Calibrate, Step::Settings];

impl Guide {
    pub fn new(step: Step, staged: bool, only_cal: bool) -> Guide {
        let mut g = Guide {
            flow: Flow::at(step),
            watch: LevelWatch::default(),
            listen: Listen::default(),
            heard: [false; 6],
            any_note: false,
            t0: Instant::now(),
            level: None,
            staged,
            only_cal,
            staged_cal: None,
            pluck_at: None,
            embedded: false,
        };
        if staged {
            let steps = g.steps();
            for s in steps.iter().take_while(|s| **s != step) {
                g.flow.marks[s.index()] = Mark::Done;
            }
            if step == Step::Done {
                g.flow.marks[Step::Notes.index()] = Mark::Done;
                g.heard = [true, true, true, true, true, false];
                g.any_note = true;
            }
            if step == Step::Notes {
                g.listen.stage(-20.0, 45.04, false);
                g.heard = [true, true, false, false, false, false];
                g.any_note = true;
            }
            g.level = Some(Level::Good);
            let mut c = staged_cal();
            if only_cal {
                // (the Audio page's: the trim moved by hand, so Back to recommended shows)
                c.trim_db += 2.5;
            }
            g.staged_cal = Some(c);
        }
        g
    }

    /// The steps this run walks through.
    fn steps(&self) -> &'static [Step] {
        if self.only_cal { &CAL_STEPS } else { &Step::ALL }
    }

    /// Where `step` is among them (0-based), and how many there are.
    fn place(&self, step: Step) -> (usize, usize) {
        let s = self.steps();
        (s.iter().position(|x| *x == step).unwrap_or(0), s.len())
    }

    /// Whether `step` is the last one (Finish / Done).
    fn last(&self, step: Step) -> bool {
        self.steps().last() == Some(&step)
    }
}

/// (peak dBFS, the made-up calibration) the screenshots show: a healthy interface in a quiet room.
const STAGED_PEAK_DB: f32 = -15.0;
fn staged_outcome() -> Outcome {
    let peaks: Vec<f32> = (0..40)
        .map(|i| lin(if i % 5 == 0 { -21.0 } else { -15.0 }))
        .collect();
    let levels = peaks.iter().map(|p| p * lin(-9.0)).collect();
    calibrate::recommend(
        &calibrate::Measure {
            quiet: vec![lin(-74.0); 60],
            hum: Some(0.1),
            peaks,
            levels,
            clip_windows: 0,
        },
        Kind::Auto,
    )
}
fn staged_cal() -> InputCal {
    staged_outcome().cal.unwrap_or_default()
}

fn lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn db(v: f32) -> f32 {
    20.0 * v.max(1e-7).log10()
}

/// The words for a kind of input, as the setup offers them: (label, one line on what it is and does).
fn kind_rows(guess: Kind) -> [(String, String); 4] {
    let guess_word = match guess {
        Kind::Mic => "a microphone",
        Kind::Modeller => "an amp modeller",
        _ => "a guitar straight in",
    };
    [
        (
            "Automatic".into(),
            format!("quickRS decides from the device's name and kind: it looks like {guess_word}."),
        ),
        (
            "Guitar straight in".into(),
            "A guitar cable into an audio interface, or a Real Tone Cable. The pedalboard gives it its tone.".into(),
        ),
        (
            "Amp modeller or processed".into(),
            "A modeller, multi-effects or an amp's line out. Heard as it comes in, no pedalboard.".into(),
        ),
        (
            "Microphone".into(),
            "A mic in front of an acoustic guitar or an amp. Heard as it is; plucks count over the room's noise."
                .into(),
        ),
    ]
}

const KINDS: [Kind; 4] = [Kind::Auto, Kind::Dry, Kind::Modeller, Kind::Mic];

/// What a kind is called in the summary.
fn kind_name(k: Kind, guess: Kind) -> String {
    match k {
        Kind::Auto => format!("Automatic ({})", kind_name(guess, Kind::Dry).to_lowercase()),
        Kind::Dry => "Guitar straight in".into(),
        Kind::Modeller => "Amp modeller or processed".into(),
        Kind::Mic => "Microphone".into(),
    }
}

/// The trim's and the pluck threshold's words under their sliders.
fn trim_words(c: &InputCal) -> String {
    let rec = match c.rec_trim_db {
        Some(r) if (r - c.trim_db).abs() >= 0.05 => format!("Recommended {r:+.1} dB. "),
        Some(_) => "Recommended. ".into(),
        None => String::new(),
    };
    match c.peak_db {
        Some(pk) => format!(
            "{rec}Your hardest strum goes into the tones at {:.0} dB; they're voiced for {:.0}.",
            pk + c.trim_db,
            calibrate::TARGET_DB
        ),
        None => format!("{rec}How loud your guitar goes into the tones. Calibrate sets it for you."),
    }
}

fn pluck_words(c: &InputCal, mic: bool) -> String {
    let o = db(c.onset_or_default(mic));
    let rec = match c.rec_onset {
        Some(r) if (db(r) - o).abs() >= 0.25 => format!("Recommended {:.0} dB. ", db(r)),
        Some(_) => "Recommended. ".into(),
        None => String::new(),
    };
    match (c.floor_db, c.soft_db) {
        (Some(f), Some(s)) => format!(
            "{rec}A pluck counts from {o:.0} dB: over the noise ({f:.0}), under soft playing (about {s:.0})."
        ),
        (Some(f), None) => format!("{rec}A pluck counts from {o:.0} dB, over the noise ({f:.0})."),
        _ => format!(
            "{rec}A pluck counts from {o:.0} dB (the default for a {}).",
            if mic { "microphone" } else { "guitar input" }
        ),
    }
}

/// A step's line under its heading.
fn intro(step: Step, only_cal: bool) -> &'static str {
    match step {
        Step::Input => {
            "Plug your guitar in: a cable into an audio interface (such as a Focusrite Scarlett), a Real Tone Cable, \
             or play to a microphone. Automatic picks the best input it finds; pick yours if it chose wrong."
        }
        Step::Output => {
            "Where the song and your guitar play: speakers or headphones. Automatic follows the computer's default \
             output."
        }
        Step::Level => "Play your guitar and watch the meter: it says what to change, if anything.",
        Step::Kind => {
            "This decides whether quickRS gives your guitar its tone (the pedalboard) or plays it as it comes in."
        }
        Step::Calibrate => {
            "As in Rocksmith: quickRS listens to your loudest playing and sets the level from it, so nothing clips \
             and soft notes still count. About 10 seconds."
        }
        Step::Settings if only_cal => {
            "What calibration recommends for this input. Play and watch the meters; move a slider if you like."
        }
        Step::Settings => {
            "What calibration recommends. Play and watch the meters; move a slider if you like, or keep going."
        }
        Step::Notes => "Play the open strings, one at a time. Each lights up when quickRS hears it.",
        Step::Done => "Saved for this input: plug in another and set it up the same way.",
    }
}

impl SettingsView {
    /// Open the setup (Settings > Audio's Set up, or the first-run prompt), on its first step.
    pub(super) fn open_guide(&mut self, step: Step, staged: bool) {
        self.setup = Some(Guide::new(step, staged, false));
        self.cal_run = None;
        self.cal_out = (staged && step != Step::Calibrate).then(staged_outcome);
        self.scroll = 0.0;
    }

    /// The setup inside the first-time setup's guitar page (P23.14).
    pub(super) fn open_guide_embedded(&mut self, step: Step, staged: bool) {
        self.open_guide(step, staged);
        self.guide_exit = None;
        if let Some(g) = &mut self.setup {
            g.embedded = true;
        }
    }

    /// The Audio page's Calibrate: the setup's Calibrate and settings steps on their own.
    pub(super) fn open_cal_guide(&mut self, step: Step, staged: bool) {
        let step = if CAL_STEPS.contains(&step) {
            step
        } else {
            Step::Calibrate
        };
        self.setup = Some(Guide::new(step, staged, true));
        self.cal_run = None;
        self.cal_out = (staged && step != Step::Calibrate).then(staged_outcome);
        self.scroll = 0.0;
    }

    /// Screenshots: calibrating, held mid-strum.
    pub(super) fn open_staged_measure(&mut self, only_cal: bool) {
        self.setup = Some(Guide::new(Step::Calibrate, true, only_cal));
        self.cal_out = None;
        self.cal_run = Some(Run::staged(Phase::Strum, 0.55, -13.5));
        self.scroll = 0.0;
    }

    /// The setup's keys; true when the key was its own.
    pub(super) fn guide_key(&mut self, ev: &Event) -> bool {
        let Some(g) = &self.setup else { return false };
        let step = g.flow.step;
        let last = g.last(step);
        let only_cal = g.only_cal;
        let Event::Key {
            key, pressed: true, ..
        } = ev
        else {
            return matches!(ev, Event::MouseWheel { .. });
        };
        let digit = match key {
            Key::Num1 => Some(0),
            Key::Num2 => Some(1),
            Key::Num3 => Some(2),
            Key::Num4 => Some(3),
            Key::Num5 => Some(4),
            Key::Num6 => Some(5),
            Key::Num7 => Some(6),
            Key::Num8 => Some(7),
            Key::Num9 => Some(8),
            _ => None,
        };
        let running = self.cal_run.is_some();
        // (on Calibrate, Enter starts it until it has run; then it's Next)
        let start_first = step == Step::Calibrate && self.cal_out.is_none();
        let sliders = step == Step::Settings || step == Step::Done;
        let act = match key {
            Key::Escape => Some("setup:close".to_string()),
            Key::Enter if step == Step::Calibrate && running => None,
            Key::Enter if start_first => Some("cal:start".into()),
            Key::Enter | Key::ArrowRight => Some(if last {
                "setup:finish".into()
            } else {
                "setup:next".into()
            }),
            Key::ArrowLeft | Key::Backspace => Some("setup:back".into()),
            Key::S if !last && !only_cal => Some("setup:skip".into()),
            Key::C if step == Step::Calibrate => Some(if running {
                "cal:cancel".into()
            } else {
                "cal:start".into()
            }),
            Key::C if step == Step::Settings => Some("setup:again".into()),
            Key::Minus if sliders => Some("trimstep:-0.5".into()),
            Key::Equals | Key::Plus if sliders => Some("trimstep:0.5".into()),
            Key::Comma if sliders => Some("pluckstep:1".into()),
            Key::Period if sliders => Some("pluckstep:-1".into()),
            Key::R if sliders => Some("calrec:".into()),
            Key::M => Some("monitor".into()),
            _ => digit.and_then(|i| match step {
                Step::Input => Some(format!("input:{i}")),
                Step::Output => Some(format!("output:{i}")),
                Step::Kind => KINDS.get(i).map(|k| format!("calkind:{}", k.key())),
                _ => None,
            }),
        };
        if let Some(a) = act {
            self.acts.push(a);
        }
        true
    }

    /// Back, Skip, Next, Finish, Close, Calibrate again and the Practice page (from the summary).
    pub(super) fn guide_act(&mut self, st: &mut AppState, arg: &str) {
        if arg == "open" {
            self.open_guide(Step::Input, false);
            return;
        }
        if arg == "calibrate" {
            self.open_cal_guide(Step::Calibrate, false);
            return;
        }
        let Some(g) = &mut self.setup else { return };
        let was = g.flow.step;
        let steps = g.steps();
        let (i, n) = g.place(was);
        // (inside the first-time setup: Close, Back from the first step and Finish leave it for the setup's page)
        let exit = match arg {
            "close" if g.embedded => Some(GuideExit::Close),
            "back" if g.embedded && i == 0 => Some(GuideExit::Back),
            "finish" if g.embedded => Some(GuideExit::Finish),
            _ => None,
        };
        if let Some(e) = exit {
            if e == GuideExit::Finish {
                gs::record(&mut st.config, "done");
                st.save_config();
            }
            self.setup = None;
            self.cal_run = None;
            self.guide_exit = Some(e);
            return;
        }
        match arg {
            "next" | "skip" if g.only_cal => {
                if i + 1 < n {
                    g.flow.next();
                } else {
                    return self.guide_act(st, "finish");
                }
            }
            "next" => g.flow.next(),
            "skip" => g.flow.skip(),
            "back" if g.only_cal => {
                if i > 0 {
                    g.flow.step = steps[i - 1];
                }
            }
            "back" => {
                g.flow.back();
            }
            "again" => {
                g.flow.step = Step::Calibrate;
                self.cal_run = None;
                self.acts.push("cal:start".into());
            }
            "close" => {
                self.setup = None;
                self.cal_run = None;
                return;
            }
            "finish" => {
                let only_cal = g.only_cal;
                self.setup = None;
                self.cal_run = None;
                if only_cal {
                    self.say(match calibrate::current_input(st) {
                        Some((_, name)) => format!("Saved for {name}"),
                        None => "Saved".into(),
                    });
                    return;
                }
                gs::record(&mut st.config, "done");
                st.save_config();
                self.say(match calibrate::current_input(st) {
                    Some((_, name)) => format!("Your guitar is set up: saved for {name}"),
                    None => "Setup done".into(),
                });
                return;
            }
            "practice" => {
                self.setup = None;
                self.show("practice");
                return;
            }
            _ => return,
        }
        let Some(g) = &mut self.setup else { return };
        let now = g.flow.step;
        if now != was {
            // a calibration doesn't run on into another step; the note check starts afresh
            if was == Step::Calibrate && arg != "again" {
                self.cal_run = None;
            }
            if now == Step::Notes && !g.staged {
                g.heard = [false; 6];
                g.any_note = false;
            }
        }
    }

    /// The setup in the Audio page's place: the step's heading, its body, and Back / Skip / Next at the bottom.
    /// Returns the bottom of the body (it fits the panel: nothing scrolls).
    pub(super) fn draw_guide(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let finished = self.feed_calibration(st, &ctx);
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let input = calibrate::current_input(st);
        let Some(g) = &mut self.setup else {
            return body.min.y;
        };
        // a calibration that went well moves on to its settings by itself; one that asks for another go stays
        if let Some(o) = finished
            && g.flow.step == Step::Calibrate
            && o.cal.is_some()
            && !o.verdict.retry()
        {
            g.flow.next();
        }
        // this frame's level and note
        let now = g.t0.elapsed().as_secs_f64();
        let (peak, level, clips) = match (&engine, g.staged) {
            (_, true) => (lin(STAGED_PEAK_DB), lin(STAGED_PEAK_DB - 9.0), 0),
            (Some(e), false) if input.is_some() => {
                let m = e.meters();
                (m.peak, m.level, m.clips)
            }
            _ => (0.0, 0.0, 0),
        };
        g.watch.feed(now, peak, clips);
        g.listen.update(engine.as_deref());
        let step = g.flow.step;
        if step == Step::Notes
            && let Some(m) = g.listen.midi
        {
            g.any_note = true;
            if let Some(s) = gs::open_string(m) {
                g.heard[s] = true;
            }
        }
        let verdict = if g.staged {
            Level::Good
        } else {
            g.watch.verdict(now, input.is_some())
        };
        if step == Step::Level && verdict != Level::Waiting {
            g.level = Some(verdict);
        }
        let flow = g.flow.clone();
        let staged = g.staged;
        let only_cal = g.only_cal;
        let embedded = g.embedded;
        let steps = g.steps();
        let (si, n) = g.place(step);
        let last = g.last(step);

        // the heading: the step of all (a dot each), its title and what it's for
        let (x, w) = (body.min.x, body.width());
        let mut y = body.min.y;
        let cr = text::caps(
            &p,
            &format!(
                "{}  ·  STEP {} OF {n}",
                if only_cal {
                    "CALIBRATE YOUR INPUT"
                } else if embedded {
                    "YOUR GUITAR"
                } else {
                    "SET UP YOUR GUITAR"
                },
                si + 1
            ),
            pos2(x, y),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        let dot_gap = px(22.0);
        for (i, s) in steps.iter().enumerate() {
            let c = pos2(x + w - px(8.0) - (n - 1 - i) as f32 * dot_gap, centre_y(cr));
            if *s == step {
                paint::led(&p, c, 10.0, th.accent);
            } else {
                match flow.mark(*s) {
                    Mark::Done => paint::dot(&p, c, px(4.5), th.hit),
                    Mark::Skipped => paint::dot(&p, c, px(4.5), th.late),
                    Mark::Todo => paint::dot(&p, c, px(4.5), th.track),
                }
            }
            let hr = Rect::from_center_size(c, vec2(dot_gap, dot_gap));
            interact(ui, hr, ("setup-dot", i), Sense::hover()).on_hover_text(s.short());
        }
        y = cr.max.y + px(10.0);
        let tr = text::draw(
            &p,
            if only_cal && step == Step::Calibrate {
                "Calibrate your input"
            } else {
                step.title()
            },
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(26.0, 800, th.text),
            Some(w),
        );
        y = tr.max.y + px(6.0);
        let ist = Style::new(14.0, 500, th.text_2);
        for line in text::wrap(&ctx, intro(step, only_cal), ist, w).iter().take(2) {
            y = text::draw(&p, line, pos2(x, y), Align2::LEFT_TOP, ist, None)
                .max
                .y;
        }
        y += px(18.0);

        // the footer: Close at the left; Back, Skip and Next (Finish on the last step) at the right
        let bh = px(46.0);
        let fy = body.max.y - bh;
        let bst = |primary: bool, enabled: bool| ButtonStyle {
            size: 14.0,
            r: Some(14.0),
            primary,
            enabled,
            ..Default::default()
        };
        let cw = widgets::button_width(&ctx, "Close", "Esc", 14.0, false);
        self.button(
            ui,
            Rect::from_min_size(pos2(x, fy), vec2(cw, bh)),
            "Close",
            "Esc",
            "setup:close",
            bst(false, true),
        );
        let mut right = x + w;
        let mut buttons: Vec<(&str, &str, &str, bool, bool)> = Vec::new();
        // (on Calibrate before it has run, Enter is its Start: Next is quieter then)
        let start_first = step == Step::Calibrate && self.cal_out.is_none() && !staged;
        let running = self.cal_run.is_some();
        if last {
            buttons.push((
                if only_cal { "Done" } else { "Finish" },
                "Enter",
                "setup:finish",
                true,
                true,
            ));
        } else {
            let primary =
                !(step == Step::Calibrate && (start_first || running || staged && self.cal_out.is_none()));
            buttons.push((
                "Next",
                if primary { "Enter" } else { "" },
                "setup:next",
                primary,
                true,
            ));
            if !only_cal {
                buttons.push(("Skip", "S", "setup:skip", false, true));
            }
        }
        buttons.push(("Back", "", "setup:back", false, si > 0 || embedded));
        for (label, key, act, primary, enabled) in buttons {
            let bw = widgets::button_width(&ctx, label, key, 14.0, false).max(px(110.0));
            let r = Rect::from_min_size(pos2(right - bw, fy), vec2(bw, bh));
            self.button(ui, r, label, key, act, bst(primary, enabled));
            right = r.min.x - px(10.0);
        }
        if embedded {
            // (the first-time setup's line on every page: where this lives later)
            let lx = x + cw + px(18.0);
            text::draw(
                &p,
                &crate::setup::Page::Guitar.later(),
                pos2(lx, fy + bh / 2.0),
                Align2::LEFT_CENTER,
                Style::new(13.0, 500, th.caption),
                Some((right - px(8.0) - lx).max(px(40.0))),
            );
        }
        let area = Rect::from_min_max(pos2(x, y), pos2(x + w, fy - px(20.0)));
        match step {
            Step::Input => self.guide_input(ui, st, area),
            Step::Output => self.guide_output(ui, st, area),
            Step::Level => self.guide_level(ui, st, area, verdict, peak),
            Step::Kind => self.guide_kind(ui, st, area),
            Step::Calibrate => self.guide_calibrate(ui, st, area, peak, staged),
            Step::Settings => self.guide_settings(ui, st, area, (peak, level), staged),
            Step::Notes => self.guide_notes(ui, area),
            Step::Done => self.guide_done(ui, st, area, (peak, level)),
        }
        body.max.y
    }

    /// Two cards side by side in `area` (`left`'s share of the width), `h` high; returns their rects.
    fn two_cards(&self, p: &egui::Painter, area: Rect, h: f32) -> (Rect, Rect) {
        let gap = px(24.0);
        let cw = snap((area.width() - gap) / 2.0);
        let a = Rect::from_min_size(area.min, vec2(cw, h));
        let b = Rect::from_min_size(
            pos2(area.min.x + cw + gap, area.min.y),
            vec2(area.width() - cw - gap, h),
        );
        paint::card(p, a, 24.0, false);
        paint::card(p, b, 24.0, false);
        (a, b)
    }

    fn guide_input(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let p = ui.painter().clone();
        let Some(audio) = &st.audio else {
            no_audio(&p, area);
            return;
        };
        let ctx = ui.ctx().clone();
        let (pad, hh) = (px(20.0), Self::h_heading(&ctx));
        let rows = input_rows(audio, &audio.prefs()).0.len() as f32;
        let row_h = 44.0;
        let list_h = 2.0 * px(8.0) + rows * px(row_h) + (rows - 1.0) * px(4.0);
        let left_h = pad + hh + list_h + px(14.0) + line_h(&ctx, 13.0) + pad;
        let gap = px(24.0);
        let channel_w = area.width() - snap((area.width() - gap) / 2.0) - gap - 2.0 * pad;
        let right_h =
            pad + hh + Self::h_channel(st, &ctx, channel_w) + px(16.0) + 3.0 * line_h(&ctx, 13.0) + pad;
        let (a, b) = self.two_cards(&p, area, left_h.max(right_h).min(area.height()));
        let yy = self.heading(
            &p,
            "GUITAR INPUT",
            a.min.x + pad,
            a.min.y + pad,
            Some(a.max.x - pad),
            "or press 1 to 9",
            "",
        );
        let lh = self.device_list(ui, st, true, (a.min.x + pad, yy, a.width() - 2.0 * pad), row_h);
        let stat = st
            .audio
            .as_ref()
            .map(|au| device_status(st, au, true))
            .unwrap_or_default();
        status_line(
            &p,
            a.min.x + pad + px(4.0),
            yy + lh + px(14.0),
            a.width() - 2.0 * pad - px(4.0),
            stat.0,
            &stat.1,
        );
        let yy = self.heading(
            &p,
            "INPUT CHANNEL",
            b.min.x + pad,
            b.min.y + pad,
            Some(b.max.x - pad),
            "pick the one whose meter moves",
            "",
        );
        let yy = self.draw_channel(ui, st, b.min.x + pad, yy, b.width() - 2.0 * pad) + px(16.0);
        super::caption_lines(
            &p,
            "Automatic picks the channel your guitar is on from the first clear notes. \
             On a two-input interface, pick the input your cable is in.",
            b.min.x + pad,
            yy,
            b.width() - 2.0 * pad,
            13.0,
            3,
        );
    }

    fn guide_output(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let p = ui.painter().clone();
        let Some(audio) = &st.audio else {
            no_audio(&p, area);
            return;
        };
        let ctx = ui.ctx().clone();
        let th = t();
        let (pad, hh) = (px(20.0), Self::h_heading(&ctx));
        let rows = output_rows(audio, &audio.prefs()).0.len() as f32;
        let row_h = 44.0;
        let list_h = 2.0 * px(8.0) + rows * px(row_h) + (rows - 1.0) * px(4.0);
        let left_h = pad + hh + list_h + px(14.0) + line_h(&ctx, 13.0) + pad;
        let slider_h = line_h(&ctx, 15.0) + px(16.0) + px(12.0) + px(14.0);
        let right_h = pad
            + hh
            + Self::h_switch("lg", true)
            + px(20.0)
            + slider_h
            + px(20.0)
            + 3.0 * line_h(&ctx, 13.0)
            + pad;
        let (a, b) = self.two_cards(&p, area, left_h.max(right_h).min(area.height()));
        let yy = self.heading(
            &p,
            "OUTPUT",
            a.min.x + pad,
            a.min.y + pad,
            Some(a.max.x - pad),
            "where you hear the song",
            "",
        );
        let lh = self.device_list(ui, st, false, (a.min.x + pad, yy, a.width() - 2.0 * pad), row_h);
        let stat = st
            .audio
            .as_ref()
            .map(|au| device_status(st, au, false))
            .unwrap_or_default();
        status_line(
            &p,
            a.min.x + pad + px(4.0),
            yy + lh + px(14.0),
            a.width() - 2.0 * pad - px(4.0),
            stat.0,
            &stat.1,
        );
        let mut yy = self.heading(
            &p,
            "HEARING YOUR GUITAR",
            b.min.x + pad,
            b.min.y + pad,
            None,
            "",
            "",
        );
        let (bx, bw) = (b.min.x + pad, b.width() - 2.0 * pad);
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let monitor = engine.as_ref().is_some_and(|e| e.monitor());
        let mic = st.mic_input();
        yy += self.switch_row(
            ui,
            bx,
            yy,
            bw,
            "Hear guitar",
            if mic {
                "a microphone: off at first, it can feed back"
            } else {
                "play your guitar through the app"
            },
            monitor,
            "monitor",
            "lg",
            "M",
            700,
        ) + px(20.0);
        let gvol = engine.as_ref().map(|e| e.guitar_gain()).unwrap_or(0.8);
        yy += self.slider_row(
            ui,
            bx,
            yy,
            bw,
            "Guitar volume",
            &format!("{}%", (gvol * 100.0).round() as i64),
            gvol / 2.0,
            "gvol",
            "",
        ) + px(20.0);
        let lines = text::wrap(
            &ctx,
            "Through an amp modeller with its own speaker or headphones, you may want Hear guitar off: you'd hear \
             it twice.",
            Style::new(13.0, 500, th.caption),
            bw,
        );
        for line in lines.iter().take(3) {
            yy = text::draw(
                &p,
                line,
                pos2(bx, yy),
                Align2::LEFT_TOP,
                Style::new(13.0, 500, th.caption),
                None,
            )
            .max
            .y;
        }
    }

    fn guide_level(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect, verdict: Level, peak: f32) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (pad, hh) = (px(20.0), Self::h_heading(&ctx));
        let input = calibrate::current_input(st);
        let cal = input
            .as_ref()
            .map(|(id, _)| calibrate::load(&st.config, id))
            .unwrap_or_default();
        let detail_st = Style::new(14.0, 500, th.text_2);
        let (head, detail) = verdict.words();
        let lines = text::wrap(&ctx, detail, detail_st, area.width() - 2.0 * pad - px(40.0));
        // (room for the longest advice, three lines, so the card doesn't jump as the verdict changes)
        let ch = pad
            + hh
            + px(30.0)
            + px(6.0)
            + line_h(&ctx, 11.0)
            + px(22.0)
            + px(30.0)
            + 3.0 * line_h(&ctx, 14.0)
            + pad;
        let card = Rect::from_min_size(area.min, vec2(area.width(), ch));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let note = match &input {
            Some((_, name)) => name.clone(),
            None => "no guitar input".into(),
        };
        let mut yy = self.heading(
            &p,
            "INPUT LEVEL",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &note,
            "",
        );
        let groove = Rect::from_min_size(pos2(x, yy), vec2(wd, px(30.0)));
        let _ = level_meter(&p, groove, peak, cal.trim_db);
        yy = groove.max.y + px(6.0) + line_h(&ctx, 11.0) + px(22.0);
        let col = match verdict {
            Level::Good => th.hit,
            Level::Hot | Level::Quiet => th.late,
            Level::Clipping | Level::Silent | Level::NoInput => th.miss,
            Level::Waiting => th.text_2,
        };
        let lc = pos2(x + px(9.0), yy + px(15.0));
        match verdict {
            Level::Waiting => paint::dot(&p, lc, px(5.0), th.unlit),
            _ => paint::led(&p, lc, 12.0, col),
        }
        text::draw(
            &p,
            head,
            pos2(x + px(30.0), lc.y),
            Align2::LEFT_CENTER,
            Style::new(22.0, 800, if verdict == Level::Waiting { th.text } else { col }),
            Some(wd - px(30.0)),
        );
        let pk = self.setup.as_ref().map(|g| g.watch.peak_db()).unwrap_or(-140.0);
        if pk > gs::HEARD_DB {
            text::draw(
                &p,
                &format!("peaks at {pk:.0} dB"),
                pos2(card.max.x - pad, lc.y),
                Align2::RIGHT_CENTER,
                Style::new(13.0, 600, th.caption),
                None,
            );
        }
        yy += px(30.0) + px(4.0);
        for line in lines.iter().take(3) {
            yy = text::draw(
                &p,
                line,
                pos2(x + px(30.0), yy),
                Align2::LEFT_TOP,
                detail_st,
                None,
            )
            .max
            .y;
        }
        // the channels' meters: the one that moves is your guitar's
        let card2_h = 2.0 * pad + hh + Self::h_channel(st, &ctx, area.width() - 2.0 * pad);
        let c2 = Rect::from_min_size(
            pos2(area.min.x, card.max.y + px(24.0)),
            vec2(area.width(), card2_h),
        );
        if c2.max.y <= area.max.y + px(20.0) {
            paint::card(&p, c2, 24.0, false);
            let yy = self.heading(
                &p,
                "INPUT CHANNEL",
                c2.min.x + pad,
                c2.min.y + pad,
                Some(c2.max.x - pad),
                "nothing moving? try another channel",
                "",
            );
            self.draw_channel(ui, st, c2.min.x + pad, yy, c2.width() - 2.0 * pad);
        }
    }

    fn guide_kind(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let Some((id, name)) = calibrate::current_input(st) else {
            no_input(&p, area);
            return;
        };
        let cal = calibrate::load(&st.config, &id);
        let guess = calibrate::auto_kind(st, &name);
        let rows = kind_rows(guess);
        let cur = KINDS.iter().position(|k| *k == cal.kind);
        let row_h = px(64.0);
        let list_h = 2.0 * px(8.0) + 4.0 * row_h + 3.0 * px(4.0);
        let card_h = pad + Self::h_heading(&ctx) + list_h + px(16.0) + line_h(&ctx, 13.0) + pad;
        let card = Rect::from_min_size(area.min, vec2(area.width(), card_h.min(area.height())));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let yy = self.heading(
            &p,
            "THIS INPUT IS",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &name,
            "",
        );
        let (h, clicked) = choice_rows(
            ui,
            Rect::from_min_size(pos2(x, yy), vec2(wd, 0.0)),
            &rows,
            cur,
            row_h,
        );
        if let Some(i) = clicked {
            self.acts.push(format!("calkind:{}", KINDS[i].key()));
        }
        text::draw(
            &p,
            "Saved for this input; it's also in Settings > Audio input / output > Input level.",
            pos2(x, yy + h + px(16.0)),
            Align2::LEFT_TOP,
            Style::new(13.0, 500, th.caption),
            Some(wd),
        );
    }

    /// Calibrate (P23.10): what to do first (the guitar's knobs up, hands off, then strum hard), Start; while it
    /// runs, the step it's on in big words, its bar and a live meter; after one that asks for another go, why.
    fn guide_calibrate(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect, peak: f32, staged: bool) {
        let p = ui.painter().clone();
        let Some((_, name)) = calibrate::current_input(st).or_else(|| staged.then(Default::default)) else {
            no_input(&p, area);
            return;
        };
        if self.cal_run.is_some() {
            self.cal_measuring(ui, area, &name, peak);
        } else {
            self.cal_instructions(ui, st, area, &name, staged);
        }
    }

    fn cal_instructions(&mut self, ui: &mut Ui, st: &AppState, area: Rect, name: &str, staged: bool) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let card = Rect::from_min_size(area.min, vec2(area.width(), area.height().min(px(340.0))));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let yy = self.heading(
            &p,
            "BEFORE YOU START",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            name,
            "",
        );
        // the picture: a guitar's volume and tone knobs, turned all the way up
        let pic = Rect::from_min_max(pos2(x, yy), pos2(x + px(300.0), card.max.y - pad));
        paint::display(&p, pic, Some(18.0), 3.0);
        for (i, label) in ["VOLUME", "TONE"].iter().enumerate() {
            let c = pos2(
                snap(pic.min.x + pic.width() * (0.29 + 0.42 * i as f32)),
                snap(pic.min.y + pic.height() * 0.42),
            );
            knobs::knob(&p, c, 64.0, 1.0, knobs::Scale::default(), None);
            text::caps(
                &p,
                label,
                pos2(c.x, c.y + px(54.0)),
                11.0,
                th.caption,
                Align2::CENTER_TOP,
                None,
            );
        }
        text::draw(
            &p,
            "Both all the way up",
            pos2(pic.center().x, pic.max.y - px(22.0)),
            Align2::CENTER_CENTER,
            Style::new(14.0, 800, th.accent_ink),
            None,
        );
        // what to do, step by step
        let rx = pic.max.x + px(28.0);
        let rw = x + wd - rx;
        let mut ry = yy;
        let retry = self.cal_out.as_ref().filter(|o| o.verdict.retry()).cloned();
        if let Some(o) = &retry {
            // the last go asked for a change on the interface
            let col = th.miss;
            let lc = pos2(rx + px(9.0), ry + px(12.0));
            paint::led(&p, lc, 12.0, col);
            text::draw(
                &p,
                o.verdict.head(),
                pos2(rx + px(28.0), lc.y),
                Align2::LEFT_CENTER,
                Style::new(18.0, 800, col),
                Some(rw - px(28.0)),
            );
            ry += px(30.0);
            let ast = Style::new(14.0, 500, th.text_2);
            for line in text::wrap(&ctx, &o.advice, ast, rw - px(28.0)).iter().take(3) {
                ry = text::draw(&p, line, pos2(rx + px(28.0), ry), Align2::LEFT_TOP, ast, None)
                    .max
                    .y;
            }
            ry += px(16.0);
        }
        let items = [
            "Turn your guitar's volume and tone knobs all the way up.",
            "Take your hands off the strings: it listens to the quiet for 2 seconds.",
            "When it says Go, strum as hard as you can on all strings, again and again, until the bar fills.",
        ];
        let ist = Style::new(15.0, 600, th.text);
        let lh = line_h(&ctx, 15.0);
        for (i, item) in items.iter().enumerate() {
            let c = pos2(rx + px(13.0), ry + snap(lh / 2.0));
            paint::dot(&p, c, px(13.0), th.accent);
            text::draw(
                &p,
                &(i + 1).to_string(),
                c,
                Align2::CENTER_CENTER,
                Style::new(13.0, 800, th.on_accent),
                None,
            );
            for line in text::wrap(&ctx, item, ist, rw - px(40.0)).iter().take(2) {
                ry = text::draw(&p, line, pos2(rx + px(40.0), ry), Align2::LEFT_TOP, ist, None)
                    .max
                    .y;
            }
            ry += px(14.0);
        }
        // Start, and how this input stands
        let cal = self.shown_cal(st);
        let label = if retry.is_some() {
            "Try again"
        } else if cal.peak_db.is_some() && !staged {
            "Calibrate again"
        } else {
            "Start"
        };
        let key = if self.cal_out.is_none() { "Enter" } else { "C" };
        let bw = widgets::button_width(&ctx, label, key, 15.0, false).max(px(180.0));
        let br = Rect::from_min_size(pos2(rx, card.max.y - pad - px(50.0)), vec2(bw, px(50.0)));
        self.button(
            ui,
            br,
            label,
            key,
            "cal:start",
            ButtonStyle {
                size: 15.0,
                r: Some(14.0),
                primary: true,
                ..Default::default()
            },
        );
        let note = match (&self.cal_out, cal.peak_db) {
            (Some(o), _) if !o.verdict.retry() => format!("Done: {}", o.verdict.head().to_lowercase()),
            (_, Some(pk)) => format!(
                "Calibrated before: peaks at {pk:.0} dB, trim {:+.1} dB.",
                cal.trim_db
            ),
            _ => "About 10 seconds. C starts it, or cancels it while it runs.".into(),
        };
        text::draw(
            &p,
            &note,
            pos2(br.max.x + px(20.0), centre_y(br)),
            Align2::LEFT_CENTER,
            Style::new(13.0, 500, th.caption),
            Some(x + wd - br.max.x - px(20.0)),
        );
    }

    fn cal_measuring(&mut self, ui: &mut Ui, area: Rect, name: &str, peak: f32) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let Some(run) = &self.cal_run else { return };
        let pr = run.prompt();
        let loudest = run.loudest();
        let hh = Self::h_heading(&ctx);
        let ch = 2.0 * pad
            + hh
            + px(64.0)
            + px(10.0)
            + line_h(&ctx, 15.0)
            + px(24.0)
            + px(18.0)
            + px(30.0)
            + line_h(&ctx, 12.0)
            + px(10.0)
            + px(30.0)
            + px(6.0)
            + line_h(&ctx, 11.0);
        let card = Rect::from_min_size(area.min, vec2(area.width(), area.height().min(ch)));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let step_no = match pr.phase {
            Phase::Quiet => 1,
            Phase::Count => 2,
            Phase::Strum => 3,
        };
        let mut yy = self.heading(
            &p,
            &format!("LISTENING  ·  {step_no} OF 3"),
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            name,
            "",
        );
        // the step in big words (the countdown's number on a small screen before it), Cancel at the right
        let row = Rect::from_min_size(pos2(x, yy), vec2(wd, px(64.0)));
        let bw = widgets::button_width(&ctx, "Cancel", "C", 14.0, false).max(px(130.0));
        self.button(
            ui,
            Rect::from_min_size(pos2(row.max.x - bw, centre_y(row) - px(23.0)), vec2(bw, px(46.0))),
            "Cancel",
            "C",
            "cal:cancel",
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        let mut hx = x;
        if let Some(n) = pr.count {
            let scr = Rect::from_min_size(row.min, vec2(px(64.0), px(64.0)));
            paint::display(&p, scr, Some(16.0), 3.0);
            text::draw(
                &p,
                &n.to_string(),
                scr.center(),
                Align2::CENTER_CENTER,
                Style::new(36.0, 800, th.accent_ink),
                None,
            );
            hx = scr.max.x + px(20.0);
        }
        let col = if pr.phase == Phase::Strum {
            th.hit
        } else {
            th.accent_ink
        };
        text::draw(
            &p,
            pr.head,
            pos2(hx, centre_y(row)),
            Align2::LEFT_CENTER,
            Style::new(30.0, 800, col),
            Some(row.max.x - bw - px(20.0) - hx),
        );
        yy = row.max.y + px(10.0);
        text::draw(
            &p,
            pr.sub,
            pos2(x, yy),
            Align2::LEFT_TOP,
            Style::new(15.0, 600, th.text_2),
            Some(wd),
        );
        yy += line_h(&ctx, 15.0) + px(24.0);
        // the bar: time for the quiet and the countdown, playing for the strum
        let bar = Rect::from_min_size(pos2(x, yy), vec2(wd, px(18.0)));
        paint::inset(&p, bar, Some(9.0), 2.0, Fill::Base);
        let fill = Rect::from_min_size(bar.min, vec2((bar.width() * pr.frac).max(px(18.0)), bar.height()));
        p.rect_filled(fill.shrink(px(3.0)), px(6.0), col);
        yy = bar.max.y + px(30.0);
        // the live meter, with the loudest so far
        let cr = text::caps(
            &p,
            "YOUR GUITAR, AS IT COMES IN",
            pos2(x, yy),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        if loudest > 1e-6 {
            text::draw(
                &p,
                &format!("loudest so far {:.0} dB", db(loudest)),
                pos2(x + wd, centre_y(cr)),
                Align2::RIGHT_CENTER,
                Style::new(12.0, 700, th.text_2),
                None,
            );
        }
        yy = cr.max.y + px(10.0);
        let groove = Rect::from_min_size(pos2(x, yy), vec2(wd, px(30.0)));
        let at = level_meter(&p, groove, peak, 0.0);
        if loudest > 1e-6 {
            let lx = at(db(loudest));
            p.rect_filled(
                Rect::from_min_size(
                    pos2(lx - px(1.5), groove.min.y - px(4.0)),
                    vec2(px(3.0), groove.height() + px(8.0)),
                ),
                1.0,
                th.text,
            );
        }
    }

    /// The calibration the settings show: the made-up one in screenshots, else the input's.
    fn shown_cal(&self, st: &AppState) -> InputCal {
        if let Some(c) = self.setup.as_ref().and_then(|g| g.staged_cal.clone()) {
            return c;
        }
        calibrate::current_input(st)
            .map(|(id, _)| calibrate::load(&st.config, &id))
            .unwrap_or_default()
    }

    /// The step after Calibrate: what it heard (a verdict, what to do, the numbers) and the settings it recommends
    /// as sliders, each with a live meter under it, and Back to recommended.
    fn guide_settings(
        &mut self,
        ui: &mut Ui,
        st: &mut AppState,
        area: Rect,
        meters: (f32, f32),
        staged: bool,
    ) {
        let p = ui.painter().clone();
        let Some((_, name)) = calibrate::current_input(st).or_else(|| staged.then(Default::default)) else {
            no_input(&p, area);
            return;
        };
        let cal = self.shown_cal(st);
        let bottom = self.cal_heard_card(ui, area, &cal, &name);
        let rest = Rect::from_min_max(pos2(area.min.x, bottom + px(24.0)), area.max);
        self.cal_sliders_card(ui, rest, &cal, st.mic_input(), meters, true);
    }

    /// What calibration heard: its verdict and advice, Calibrate again, and the numbers. Returns the card's bottom.
    fn cal_heard_card(&mut self, ui: &mut Ui, area: Rect, cal: &InputCal, name: &str) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let (head, advice, col): (String, String, _) = match &self.cal_out {
            Some(o) => (
                o.verdict.head().into(),
                o.advice.clone(),
                match o.verdict {
                    Verdict::Good => th.hit,
                    Verdict::Hot | Verdict::Hum | Verdict::Noisy => th.late,
                    _ => th.miss,
                },
            ),
            None if cal.peak_db.is_some() => (
                "Calibrated before".into(),
                "Calibrate again after changing the interface's gain or the guitar.".into(),
                th.hit,
            ),
            None => (
                "Not calibrated yet".into(),
                "These are the defaults. Calibrate sets them from your playing.".into(),
                th.late,
            ),
        };
        // the numbers: from this run, else from the last one kept
        let found = self
            .cal_out
            .as_ref()
            .filter(|o| o.cal.is_some())
            .map(|o| &o.found);
        let mut tiles: Vec<(&str, String)> = Vec::new();
        if let Some(f) = found {
            tiles.push(("LOUDEST PLAYING", format!("{:.0} dB", f.loud_db)));
            tiles.push(("SOFT PLAYING", format!("about {:.0} dB", f.soft_db)));
            tiles.push(("NOISE, HANDS OFF", format!("{:.0} dB", f.floor_db)));
            tiles.push((
                "CLIPPED",
                match f.clip_windows {
                    0 => "never".into(),
                    1 => "once".into(),
                    n => format!("{n} times"),
                },
            ));
        } else if let (Some(pk), Some(fl)) = (cal.peak_db, cal.floor_db) {
            tiles.push(("LOUDEST PLAYING", format!("{pk:.0} dB")));
            if let Some(s) = cal.soft_db {
                tiles.push(("SOFT PLAYING", format!("about {s:.0} dB")));
            }
            tiles.push(("NOISE, HANDS OFF", format!("{fl:.0} dB")));
        }
        let ast = Style::new(14.0, 500, th.text_2);
        let tile_h = px(64.0);
        let ch = 2.0 * pad
            + Self::h_heading(&ctx)
            + px(40.0)
            + px(4.0)
            + 2.0 * line_h(&ctx, 14.0)
            + if tiles.is_empty() { 0.0 } else { px(16.0) + tile_h };
        let card = Rect::from_min_size(area.min, vec2(area.width(), ch.min(area.height())));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let mut yy = self.heading(
            &p,
            "WHAT IT HEARD",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            name,
            "",
        );
        let row = Rect::from_min_size(pos2(x, yy), vec2(wd, px(40.0)));
        let label = "Calibrate again";
        let bw = widgets::button_width(&ctx, label, "C", 13.0, false).max(px(170.0));
        self.button(
            ui,
            Rect::from_min_size(pos2(row.max.x - bw, row.min.y), vec2(bw, px(40.0))),
            label,
            "C",
            "setup:again",
            ButtonStyle {
                size: 13.0,
                r: Some(12.0),
                ..Default::default()
            },
        );
        let lc = pos2(x + px(9.0), centre_y(row));
        paint::led(&p, lc, 12.0, col);
        text::draw(
            &p,
            &head,
            pos2(x + px(28.0), lc.y),
            Align2::LEFT_CENTER,
            Style::new(20.0, 800, col),
            Some(wd - bw - px(48.0)),
        );
        yy = row.max.y + px(4.0);
        for line in text::wrap(&ctx, &advice, ast, wd - bw - px(48.0)).iter().take(2) {
            yy = text::draw(&p, line, pos2(x + px(28.0), yy), Align2::LEFT_TOP, ast, None)
                .max
                .y;
        }
        if !tiles.is_empty() {
            let ty = card.max.y - pad - tile_h;
            let gap = px(14.0);
            let n = tiles.len() as f32;
            let tw = snap((wd - (n - 1.0) * gap) / n);
            for (i, (cap, val)) in tiles.iter().enumerate() {
                let r = Rect::from_min_size(pos2(x + i as f32 * (tw + gap), ty), vec2(tw, tile_h));
                paint::well(&p, r, 16.0);
                text::caps(
                    &p,
                    cap,
                    pos2(r.min.x + px(16.0), r.min.y + px(12.0)),
                    11.0,
                    th.caption,
                    Align2::LEFT_TOP,
                    None,
                );
                text::draw(
                    &p,
                    val,
                    pos2(r.min.x + px(16.0), r.max.y - px(12.0)),
                    Align2::LEFT_BOTTOM,
                    Style::new(19.0, 800, th.text),
                    Some(tw - px(32.0)),
                );
            }
        }
        let _ = yy;
        card.max.y
    }

    /// The trim and the pluck sensitivity as sliders (their keys, what they mean in words), Back to recommended,
    /// and (`live`) a meter under each: the level into the tones, and the playing's level against the pluck
    /// threshold with a light when a pluck counts. Returns the card's bottom.
    fn cal_sliders_card(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        cal: &InputCal,
        mic: bool,
        (peak, level): (f32, f32),
        live: bool,
    ) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let slider_h = line_h(&ctx, 15.0) + px(16.0) + px(12.0) + px(14.0);
        let cst = Style::new(13.0, 500, th.caption);
        let meter_h = px(18.0) + line_h(&ctx, 12.0) + px(8.0) + px(18.0) + px(6.0) + line_h(&ctx, 11.0);
        let ch = 2.0 * pad
            + Self::h_heading(&ctx)
            + slider_h
            + if live {
                px(8.0) + 2.0 * line_h(&ctx, 13.0) + meter_h
            } else {
                0.0
            };
        let card = Rect::from_min_size(area.min, vec2(area.width(), ch.min(area.height().max(px(120.0)))));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let hr = text::caps(
            &p,
            "YOUR SETTINGS",
            pos2(x, card.min.y + pad),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        text::draw(
            &p,
            "saved for this input",
            pos2(hr.max.x + px(14.0), centre_y(hr)),
            Align2::LEFT_CENTER,
            Style::new(12.0, 500, th.caption),
            None,
        );
        // Back to recommended, at the heading's right (only when there's one to go back to)
        let label = "Back to recommended";
        let bw = widgets::button_width(&ctx, label, "R", 13.0, false).max(px(200.0));
        let differs = cal.has_recommendation() && !cal.is_recommended();
        self.button(
            ui,
            Rect::from_min_size(
                pos2(card.max.x - pad - bw, centre_y(hr) - px(18.0)),
                vec2(bw, px(36.0)),
            ),
            label,
            "R",
            "calrec:",
            ButtonStyle {
                size: 13.0,
                r: Some(12.0),
                primary: differs,
                enabled: differs,
                ..Default::default()
            },
        );
        let yy = card.min.y + pad + Self::h_heading(&ctx);
        let gap = px(48.0);
        let half = snap((wd - gap) / 2.0);
        let px2 = x + half + gap;
        // the trim
        let u = (cal.trim_db - calibrate::TRIM.0) / (calibrate::TRIM.1 - calibrate::TRIM.0);
        self.slider_row(
            ui,
            x,
            yy,
            half,
            "Trim",
            &format!("{:+.1} dB", cal.trim_db),
            u,
            "trim",
            "- =",
        );
        // the pluck sensitivity (more to the right: the threshold lower)
        let onset = cal.onset_or_default(mic);
        self.slider_row(
            ui,
            px2,
            yy,
            half,
            "Pluck sensitivity",
            &format!("{:.0} dB", db(onset)),
            calibrate::sensitivity_of(onset),
            "pluck",
            ", .",
        );
        // (the summary: just the sliders, its rows say the rest)
        if !live {
            return card.max.y;
        }
        let wy = yy + slider_h + px(8.0);
        for (cx, words) in [(x, trim_words(cal)), (px2, pluck_words(cal, mic))] {
            let mut ly = wy;
            for line in text::wrap(&ctx, &words, cst, half).iter().take(2) {
                ly = text::draw(&p, line, pos2(cx, ly), Align2::LEFT_TOP, cst, None)
                    .max
                    .y;
            }
        }
        // the live meters: into the tones (after the trim), and the playing against the pluck threshold
        let my = wy + 2.0 * line_h(&ctx, 13.0) + px(18.0);
        let into = peak * lin(cal.trim_db);
        let cr = text::caps(
            &p,
            "INTO THE TONES",
            pos2(x, my),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        if peak > 1e-6 {
            text::draw(
                &p,
                &format!("{:.0} dB", db(into)),
                pos2(x + half, centre_y(cr)),
                Align2::RIGHT_CENTER,
                Style::new(12.0, 700, th.text_2),
                None,
            );
        }
        let g1 = Rect::from_min_size(pos2(x, cr.max.y + px(8.0)), vec2(half, px(18.0)));
        let _ = level_meter(&p, g1, into, 0.0);
        // the pluck meter: the level, the threshold as a tick, a light while a pluck counts
        let counts = level > onset;
        let now = Instant::now();
        let lit = if let Some(g) = &mut self.setup {
            if counts {
                g.pluck_at = Some(now);
            }
            g.pluck_at
                .is_some_and(|t0| now.duration_since(t0).as_millis() < 250)
        } else {
            counts
        };
        let cr2 = text::caps(
            &p,
            "PLUCKS",
            pos2(px2, my),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        let lc = pos2(px2 + half - px(8.0), centre_y(cr2));
        if lit {
            paint::led(&p, lc, 10.0, th.hit);
        } else {
            paint::dot(&p, lc, px(4.5), th.unlit);
        }
        text::draw(
            &p,
            if lit { "counts" } else { "a pluck lights it" },
            pos2(lc.x - px(16.0), lc.y),
            Align2::RIGHT_CENTER,
            Style::new(12.0, 700, if lit { th.hit } else { th.caption }),
            None,
        );
        let g2 = Rect::from_min_size(pos2(px2, cr2.max.y + px(8.0)), vec2(half, px(18.0)));
        pluck_meter(&p, g2, level, onset);
        card.max.y
    }

    fn guide_notes(&mut self, ui: &mut Ui, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let Some(g) = &self.setup else { return };
        let ch = 2.0 * pad + Self::h_heading(&ctx) + px(200.0);
        let card = Rect::from_min_size(area.min, vec2(area.width(), area.height().min(ch)));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let yy = self.heading(
            &p,
            "THE NOTE YOU PLAY",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            "as the tuner hears it",
            "",
        );
        // the note on a small screen, with the tuner's gauge
        let screen = Rect::from_min_size(pos2(x, yy), vec2(px(240.0), px(200.0)));
        paint::display(&p, screen, Some(18.0), 3.0);
        let midi = g.listen.midi;
        match midi {
            Some(m) => {
                let r = m.round();
                let cents = ((m - r) * 100.0) as f32;
                let col = if cents.abs() <= 10.0 { th.hit } else { th.late };
                text::draw(
                    &p,
                    &qrs_analysis::pitch::note_label(m),
                    pos2(screen.center().x, screen.min.y + px(64.0)),
                    Align2::CENTER_CENTER,
                    Style::new(52.0, 800, th.accent_ink),
                    None,
                );
                let gauge = Rect::from_min_size(
                    pos2(screen.min.x + px(30.0), screen.min.y + px(112.0)),
                    vec2(screen.width() - px(60.0), px(54.0)),
                );
                knobs::arc_gauge(&p, gauge, Some(cents), col);
                text::draw(
                    &p,
                    &format!("{:+.0} cents", cents),
                    pos2(screen.center().x, screen.max.y - px(18.0)),
                    Align2::CENTER_CENTER,
                    Style::new(12.0, 700, th.caption),
                    None,
                );
            }
            None => {
                text::draw(
                    &p,
                    "play a note",
                    screen.center(),
                    Align2::CENTER_CENTER,
                    Style::new(16.0, 700, th.caption),
                    None,
                );
            }
        }
        // the open strings, lit once heard
        let sx = screen.max.x + px(28.0);
        let sw = x + wd - sx;
        let gap = px(10.0);
        let cw = snap((sw - 5.0 * gap) / 6.0);
        let count = g.heard.iter().filter(|h| **h).count();
        text::caps(
            &p,
            "OPEN STRINGS, E STANDARD",
            pos2(sx, yy),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        let cy = yy + px(28.0);
        for (i, name) in gs::OPEN_NAMES.iter().enumerate() {
            let r = Rect::from_min_size(pos2(sx + i as f32 * (cw + gap), cy), vec2(cw, px(84.0)));
            let on = g.heard[i];
            let now = midi.and_then(gs::open_string) == Some(i);
            if now {
                paint::inset(&p, r, Some(16.0), 3.0, Fill::Base);
            } else {
                paint::raised(&p, r, Some(16.0), 4.0, Fill::Base, 0.0);
            }
            let lc = pos2(r.center().x, r.min.y + px(18.0));
            if on {
                paint::led(&p, lc, 9.0, th.hit);
            } else {
                paint::dot(&p, lc, px(4.0), th.track);
            }
            text::draw(
                &p,
                name,
                pos2(r.center().x, r.min.y + px(46.0)),
                Align2::CENTER_CENTER,
                Style::new(22.0, 800, if on || now { th.text } else { th.text_2 }),
                None,
            );
            text::draw(
                &p,
                &qrs_analysis::pitch::note_label(gs::OPEN[i] as f64),
                pos2(r.center().x, r.max.y - px(14.0)),
                Align2::CENTER_CENTER,
                Style::new(11.0, 600, th.caption),
                None,
            );
        }
        let wy = cy + px(84.0) + px(22.0);
        text::draw(
            &p,
            gs::notes_words(count, g.any_note),
            pos2(sx, wy),
            Align2::LEFT_TOP,
            Style::new(17.0, 800, if count >= 3 { th.hit } else { th.text }),
            Some(sw),
        );
        let cst = Style::new(13.0, 500, th.caption);
        let mut ly = wy + px(30.0);
        for line in text::wrap(
            &ctx,
            "Tuned another way? Any note you play shows on the left; only E standard lights the strings. The Tuner \
             tile in a song listens the same way.",
            cst,
            sw,
        )
        .iter()
        .take(3)
        {
            ly = text::draw(&p, line, pos2(sx, ly), Align2::LEFT_TOP, cst, None).max.y;
        }
    }

    fn guide_done(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect, meters: (f32, f32)) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let Some(g) = &self.setup else { return };
        let flow = g.flow.clone();
        let embedded = g.embedded;
        let heard = g.heard.iter().filter(|h| **h).count();
        let level = g.level;
        let audio = st.audio.as_ref();
        let input = calibrate::current_input(st);
        let prefs = audio.map(|a| a.prefs());
        let status = audio.map(|a| a.engine.status());
        let plan = audio.map(|a| a.plan());
        let skipped = |s: Step| flow.mark(s) == Mark::Skipped;
        // (the step, ok / not / nothing to say, what's set)
        let mut rows: Vec<(Step, Option<bool>, String)> = Vec::new();
        let in_name = input.as_ref().map(|i| i.1.clone());
        let channel = match (
            plan.as_ref().and_then(|p| p.route.input.as_ref()),
            prefs.as_ref().map(|p| p.channel),
        ) {
            (Some(r), Some(None)) if r.channel >= 0 => format!("channel {} (found by itself)", r.channel + 1),
            (Some(r), _) if r.channel >= 0 => format!("channel {}", r.channel + 1),
            (Some(_), _) => "every channel mixed".into(),
            _ => String::new(),
        };
        rows.push((
            Step::Input,
            Some(in_name.is_some()),
            match &in_name {
                Some(n) if channel.is_empty() => n.clone(),
                Some(n) => format!("{n}  ·  {channel}"),
                None => "no guitar input".into(),
            },
        ));
        rows.push((
            Step::Output,
            status.as_ref().map(|s| s.output.is_some() && !s.silent_output),
            status
                .as_ref()
                .and_then(|s| s.output.as_ref().map(|o| o.device_name.clone()))
                .unwrap_or_else(|| "no sound device".into()),
        ));
        rows.push((
            Step::Level,
            level.map(|l| l.ok()),
            match level {
                Some(l) => l.words().0.to_string(),
                None if skipped(Step::Level) => "skipped".into(),
                None => "not checked".into(),
            },
        ));
        let cal = self.shown_cal(st);
        let guess = in_name
            .as_deref()
            .map(|n| calibrate::auto_kind(st, n))
            .unwrap_or(Kind::Dry);
        rows.push((Step::Kind, Some(true), kind_name(cal.kind, guess)));
        rows.push((
            Step::Calibrate,
            cal.peak_db.map(|_| true),
            match cal.peak_db {
                Some(pk) => format!(
                    "trim {:+.1} dB  ·  plucks from {:.0} dB  ·  peaks at {pk:.0} dB{}",
                    cal.trim_db,
                    db(cal.onset_or_default(st.mic_input())),
                    if cal.is_recommended() {
                        ""
                    } else {
                        "  ·  set by hand"
                    }
                ),
                None => "not calibrated (Calibrate any time in Settings > Audio input / output)".into(),
            },
        ));
        rows.push((
            Step::Notes,
            (heard > 0 || g.any_note).then_some(true),
            if heard > 0 {
                format!("{heard} of 6 open strings heard")
            } else if g.any_note {
                "notes heard".into()
            } else if skipped(Step::Notes) {
                "skipped".into()
            } else {
                "not checked".into()
            },
        ));
        let row_h = px(34.0);
        let card = Rect::from_min_size(
            area.min,
            vec2(area.width(), 2.0 * pad + rows.len() as f32 * row_h),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let lw = px(170.0);
        for (i, (s, ok, what)) in rows.iter().enumerate() {
            let cy = card.min.y + pad + i as f32 * row_h + snap(row_h / 2.0);
            if i > 0 {
                p.hline(x..=x + wd, cy - snap(row_h / 2.0), (1.0, th.divider));
            }
            let c = pos2(x + px(8.0), cy);
            match ok {
                Some(true) => paint::led(&p, c, 9.0, th.hit),
                Some(false) => paint::led(&p, c, 9.0, th.late),
                None => paint::dot(&p, c, px(4.0), th.unlit),
            }
            text::draw(
                &p,
                s.short(),
                pos2(x + px(26.0), cy),
                Align2::LEFT_CENTER,
                Style::new(14.0, 700, th.text),
                Some(lw - px(30.0)),
            );
            text::draw(
                &p,
                what,
                pos2(x + lw, cy),
                Align2::LEFT_CENTER,
                Style::new(14.0, 500, th.text_2),
                Some(wd - lw),
            );
        }
        // the calibration's sliders, to change them here too (with their meters when there's room)
        let mic = st.mic_input();
        let rest = Rect::from_min_max(pos2(area.min.x, card.max.y + px(24.0)), area.max);
        let below = self.cal_sliders_card(ui, rest, &cal, mic, meters, false);
        // timing: the settings that exist for it (not inside the first-time setup: its pages go on from here)
        if embedded {
            return;
        }
        let t_card = Rect::from_min_size(pos2(area.min.x, below + px(24.0)), vec2(area.width(), px(118.0)));
        if t_card.max.y > area.max.y + px(20.0) {
            return;
        }
        paint::card(&p, t_card, 24.0, false);
        let yy = self.heading(&p, "TIMING", x, t_card.min.y + pad, None, "", "");
        let bw = widgets::button_width(&ctx, "Practice settings", "", 14.0, false).max(px(170.0));
        let cst = Style::new(13.0, 500, th.text_2);
        let mut ly = yy;
        for line in text::wrap(
            &ctx,
            "If your hits show as late or early in a song, Settings > Practice > Guitar timing moves them (Ctrl+, \
             and Ctrl+.). Crackles: raise the Buffer on the Audio page.",
            cst,
            wd - bw - px(32.0),
        )
        .iter()
        .take(3)
        {
            ly = text::draw(&p, line, pos2(x, ly), Align2::LEFT_TOP, cst, None).max.y;
        }
        let _ = ly;
        self.button(
            ui,
            Rect::from_min_size(
                pos2(
                    x + wd - bw,
                    t_card.min.y + snap((t_card.height() - px(46.0)) / 2.0),
                ),
                vec2(bw, px(46.0)),
            ),
            "Practice settings",
            "",
            "setup:practice",
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
    }
}

/// The playing's level (RMS, -60..0 dBFS) against the pluck threshold: the level filled (lit while over it), the
/// threshold a tick, what's under it washed (ignored), and the scale under it.
fn pluck_meter(p: &egui::Painter, groove: Rect, level: f32, onset: f32) {
    let th = t();
    let u_of = |d: f32| ((d + 60.0) / 60.0).clamp(0.0, 1.0);
    let at = |d: f32| snap(groove.min.x + groove.width() * u_of(d));
    paint::inset(p, groove, Some(groove.height() / 2.0), 3.0, Fill::Base);
    let tx = at(db(onset));
    paint::wash(
        p,
        Rect::from_min_max(groove.min, pos2(tx, groove.max.y)),
        th.track,
        0.35,
    );
    let inset = snap(groove.height() * 4.0 / 18.0);
    if level > 1e-6 {
        let fill = Rect::from_min_max(
            groove.min + vec2(px(2.0), inset),
            pos2(at(db(level)).max(groove.min.x + px(12.0)), groove.max.y - inset),
        );
        let col = if level > onset { th.hit } else { th.caption };
        p.rect_filled(fill, fill.height() / 2.0, col);
    }
    p.rect_filled(
        Rect::from_min_size(
            pos2(tx - px(1.0), groove.min.y - px(4.0)),
            vec2(px(2.0), groove.height() + px(8.0)),
        ),
        1.0,
        th.text,
    );
    for d in [-48.0f32, -36.0, -24.0, -12.0, 0.0] {
        text::draw(
            p,
            &format!("{d:.0}"),
            pos2(at(d), groove.max.y + px(6.0)),
            if d == 0.0 {
                Align2::RIGHT_TOP
            } else {
                Align2::CENTER_TOP
            },
            Style::new(11.0, 600, th.caption),
            None,
        );
    }
}

/// "No guitar input" in a step's place.
fn no_input(p: &egui::Painter, area: Rect) {
    notice(
        p,
        area,
        "No guitar input is listening",
        "Go back to the first step and pick the input your guitar is plugged into.",
    );
}

/// No sound at all (started silent): nothing to set up.
fn no_audio(p: &egui::Painter, area: Rect) {
    notice(
        p,
        area,
        "The audio engine isn't running",
        "quickRS was started without sound, so there are no devices to set up.",
    );
}

/// A card with a warning and a line on what to do, in a step's place.
fn notice(p: &egui::Painter, area: Rect, title: &str, body: &str) {
    let th = t();
    let card = Rect::from_min_size(area.min, vec2(area.width(), px(110.0)));
    paint::card(p, card, 24.0, false);
    text::draw(
        p,
        title,
        pos2(card.min.x + px(24.0), card.min.y + px(28.0)),
        Align2::LEFT_TOP,
        Style::new(18.0, 800, th.late),
        None,
    );
    text::draw(
        p,
        body,
        pos2(card.min.x + px(24.0), card.min.y + px(62.0)),
        Align2::LEFT_TOP,
        Style::new(14.0, 500, th.text_2),
        Some(card.width() - px(48.0)),
    );
}

/// A sunken list of choices, each a label with a line under it and its number key at the right (the radio rows of
/// the device lists, two lines high). Returns the list's height and the row clicked.
fn choice_rows(
    ui: &Ui,
    rect: Rect,
    rows: &[(String, String)],
    current: Option<usize>,
    row_h: f32,
) -> (f32, Option<usize>) {
    let th = t();
    let p = ui.painter().clone();
    let (pad, gap) = (px(8.0), px(4.0));
    let n = rows.len() as f32;
    let h = 2.0 * pad + n * row_h + (n - 1.0).max(0.0) * gap;
    paint::inset(
        &p,
        Rect::from_min_size(rect.min, vec2(rect.width(), h)),
        Some(18.0),
        6.0,
        Fill::Base,
    );
    let mut clicked = None;
    for (i, (label, hint)) in rows.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(rect.min.x + pad, rect.min.y + pad + i as f32 * (row_h + gap)),
            vec2(rect.width() - 2.0 * pad, row_h),
        );
        let on = current == Some(i);
        let resp = interact(ui, r, ("setup-choice", i), Sense::click());
        if on {
            paint::raised(&p, r, Some(12.0), 4.0, Fill::Base, 0.0);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
        let cy = centre_y(r);
        let c = pos2(r.min.x + px(18.0), cy);
        if on {
            paint::led(&p, c, 9.0, th.accent);
        } else {
            paint::dot(&p, c, px(4.5), th.track);
        }
        let kr = keycap(
            &p,
            &(i + 1).to_string(),
            pos2(r.max.x - px(14.0), cy),
            Align2::RIGHT_CENTER,
            11.0,
            CapOn::Material,
        );
        let tx = c.x + px(18.0);
        let tw = kr.min.x - px(16.0) - tx;
        text::draw(
            &p,
            label,
            pos2(tx, cy - px(10.0)),
            Align2::LEFT_CENTER,
            Style::new(
                15.0,
                if on { 800 } else { 700 },
                if on || resp.hovered() { th.text } else { th.text_2 },
            ),
            Some(tw),
        );
        text::draw(
            &p,
            hint,
            pos2(tx, cy + px(12.0)),
            Align2::LEFT_CENTER,
            Style::new(12.0, 500, th.caption),
            Some(tw),
        );
    }
    (h, clicked)
}
