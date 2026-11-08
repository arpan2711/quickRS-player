//! Settings (F2, or the Settings strip on the home page): every setting of the app in one panel floating over the
//! page you were on, in six pages (port of rsplay/settingsview.py, rebuilt in P35.3 on the settings review, D92;
//! the Sound page's devices are new, on `qrs_audio::devices`). Common things first, rare ones in "›" folds:
//!
//!     Sound     the guided setup's status, [the instrument on this input], the guitar input and the output (the
//!               chosen device, Change… for the list), the channel with a meter each, hearing yourself and the
//!               volumes, the input level (Check level, what kind of input it is); folds: trim by hand, the
//!               RS2014 setup, all devices
//!     Timing    [Calibrate timing…], Your hits (`input_offset`), Reduce delay (low-latency output, the buffer, the
//!               latency chip); fold: Chart sync (`av_offset`)
//!     Tone      As is / Pedalboard / Clean / ... / Custom; the custom tone's knobs (Custom only, else a fold);
//!               saved tones
//!     Display   theme and knobs, string colours, the note tile, the song screen's tiles and their order, the note
//!               widget window, notation (fold: fine-tune)
//!     Library   song folders (add, remove, RS's dlc folder or not), rescan, the song list, RS's keys
//!               (folded once found), the recordings folders
//!     About     this version and its updates, report a bug, the first-time setup again, Reset all settings…
//!
//! Old ids still open their page (audio, practice, updates: `nav::resolve`); the classic look (the Python app's,
//! for parity screenshots) keeps its five. Each page ends in "Reset <page> to defaults" (`reset.rs`).
//!
//! Everything is stored in config.json straight away; an open song picks the changes up
//! (`PlayerView::reload_settings`). The keys shown in the panel work here too (T, N, Y, 7 8 9 0, M, the sync keys,
//! F5). Esc, Done or a click outside the panel closes it; Up / Down scroll, Tab and Ctrl+Up / Down switch pages.
//!
//! The layout is the Python app's in its whole pixels (`qrs_ui::units`): `px(n)` where it has `int(n * s)`, `snap`
//! where it truncates a position or halves a length (`a // 2`), pygame's `Rect` arithmetic spelled out where it
//! matters (`centre_x`, `centre_y`, `centred`, `inflate`).

mod audio;
pub mod fastout;
mod guide;
mod library;
mod nam;
pub mod nav;
mod pages;
mod play_along;
pub mod reset;
mod setup;
mod timing;
mod timing_tool;
mod tone;
mod updates;

pub use setup::FirstSetup;
pub use timing::{NUDGE_MS, delay_label};

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Event, Key, Painter, Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use qrs_audio::Choice;
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::knobs::{self, KnobStyle};
use qrs_ui::paint::{self, FaceKind, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{self, ThemeName, t};
use qrs_ui::units::{self, pixels, px};
use qrs_ui::widgets::{self, ButtonStyle, interact, switch_size};

use crate::library::wheel_notches;
use crate::player::{PlayerView, tiles};
use crate::state::AppState;

/// The pages in the panel: (id, name, hint). The Python look's screenshots have its five, without hints.
pub fn sections() -> &'static [(&'static str, &'static str, &'static str)] {
    nav::pages(theme::classic())
}

/// The Sound page's id in this look (the guided setup and the level tool run there).
fn sound_id() -> &'static str {
    nav::sound(theme::classic())
}

/// A question asked before a reset: its words, and the action its Reset key queues.
struct Confirm {
    title: String,
    text: String,
    action: String,
}

/// `int(v)` of a position or length in points: `qrs_ui::units::snap`, with room for the f32 noise that sums of
/// `px` numbers carry (a few 1e-5 px either side; `units::snap`'s 1e-6 isn't enough: at 1440 x 900 it makes the
/// panel's `1234 // 2` 616). Use `units::snap` here once it has the room.
fn snap(v: f32) -> f32 {
    let s = units::scale();
    if s <= 0.0 {
        return v;
    }
    let x = v as f64 * s;
    ((x + 1e-3f64.copysign(x)).trunc() / s) as f32
}

/// pygame's `r.centery` (`r.y + r.h // 2`) of a rect in whole pixels.
fn centre_y(r: Rect) -> f32 {
    r.min.y + snap(r.height() / 2.0)
}

/// pygame's `r.centerx` (`r.x + r.w // 2`).
fn centre_x(r: Rect) -> f32 {
    r.min.x + snap(r.width() / 2.0)
}

/// pygame's `r.center = (w // 2, h // 2)`: a rect of `size` (whole pixels) in the middle of `area`.
fn centred(area: Rect, size: Vec2) -> Rect {
    Rect::from_min_size(
        pos2(
            area.min.x + snap(area.width() / 2.0) - snap(size.x / 2.0),
            area.min.y + snap(area.height() / 2.0) - snap(size.y / 2.0),
        ),
        size,
    )
}

/// pygame's `r.inflate(dx, dy)`: dx, dy wider and taller, the left and top moving out by half of it (C's division,
/// toward zero, so an odd shrink keeps the extra pixel at the right and bottom).
fn inflate(r: Rect, dx: f32, dy: f32) -> Rect {
    let half = |d: f32| {
        if d < 0.0 { -snap(-d / 2.0) } else { snap(d / 2.0) }
    };
    Rect::from_min_size(
        pos2(r.min.x - half(dx), r.min.y - half(dy)),
        vec2(r.width() + dx, r.height() + dy),
    )
}

/// The height of a line of text at this size (pygame's `font.get_height()`; every weight has the same).
fn line_h(ctx: &egui::Context, size: f32) -> f32 {
    text::line_height(ctx, Style::new(size, 500, Color32::WHITE))
}

struct KnobDrag {
    key: &'static str,
    y0: f32,
    v0: f32,
}

struct TileDrag {
    key: &'static str,
    grab: f32,
    y: f32,
}

pub struct SettingsView {
    pub section: &'static str,
    /// The next `switch_row`'s hint in this colour (a highlighted row: Low-latency output), then cleared.
    hint_color: Option<egui::Color32>,
    /// Set when the panel should close (the app drops it).
    pub closed: bool,
    message: Option<(String, Instant)>,
    /// The name being typed for a saved tone.
    naming: Option<String>,
    knob_drag: Option<KnobDrag>,
    /// The volume slider being dragged ("vol" / "gvol").
    slider: Option<&'static str>,
    tile_drag: Option<TileDrag>,
    scroll: f32,
    max_scroll: f32,
    /// Where each tone knob was drawn (for the wheel).
    knob_rects: Vec<(Rect, &'static str)>,
    tile_rows: Vec<(Rect, &'static str)>,
    tile_list_y: f32,
    content: Rect,
    panel: Rect,
    acts: Vec<String>,
    /// The device rows of the Audio page as drawn (the `input:i` / `output:i` acts index them).
    inputs: Vec<Choice>,
    outputs: Vec<Choice>,
    /// Calibrating the guitar input (D33, P23.10), and what the last run found and said.
    cal_run: Option<crate::calibrate::Run>,
    cal_out: Option<crate::calibrate::Outcome>,
    /// The guided guitar setup, in the Audio page's place while it runs (P23.8).
    setup: Option<guide::Guide>,
    /// How a guided setup run inside the first-time setup was left (P23.14).
    guide_exit: Option<guide::GuideExit>,
    /// How its steps were left, once it's gone (the first-time setup's summary says Skipped for a skipped step).
    guide_flow: Option<crate::guitar_setup::Flow>,
    /// The Library page's key boxes, what the last save or load said, and RS's dlc folders on this machine
    /// (looked for once).
    psarc_in: String,
    sng_in: String,
    key_note: Option<(String, bool)>,
    dlc_found: Option<Vec<std::path::PathBuf>>,
    /// Set with `closed`: open the first-time setup (P23.14), on this page if set (Tone's Suggested captures: "nam").
    pub first_setup: bool,
    pub first_setup_at: Option<String>,
    /// Closed to open the amp room (Settings › Tone's "Edit in the amp room", D138).
    pub open_room: bool,
    /// The folds open (P35.5).
    folds: nav::Folds,
    /// A reset waiting for its Reset (P35.7).
    confirm: Option<Confirm>,
    /// Sound's All devices fold: every input and output (the `input:a<i>` / `output:a<i>` acts index them).
    inputs_all: Vec<Choice>,
    outputs_all: Vec<Choice>,
    /// The Neural Amp Modeler page's state (its guided setup's step, a TONE3000 pick, the key being typed).
    nam: nam::NamPage,
}

impl SettingsView {
    pub fn new(section: &str) -> SettingsView {
        let mut v = SettingsView {
            hint_color: None,
            section: nav::resolve(section, theme::classic()).unwrap_or(sound_id()),
            closed: false,
            message: None,
            naming: None,
            knob_drag: None,
            slider: None,
            tile_drag: None,
            scroll: 0.0,
            max_scroll: 0.0,
            knob_rects: Vec::new(),
            tile_rows: Vec::new(),
            tile_list_y: 0.0,
            content: Rect::NOTHING,
            panel: Rect::NOTHING,
            acts: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            cal_run: None,
            cal_out: None,
            setup: None,
            guide_exit: None,
            guide_flow: None,
            psarc_in: String::new(),
            sng_in: String::new(),
            key_note: None,
            dlc_found: None,
            first_setup: false,
            first_setup_at: None,
            open_room: false,
            folds: nav::Folds::default(),
            confirm: None,
            inputs_all: Vec::new(),
            outputs_all: Vec::new(),
            nam: nam::NamPage::default(),
        };
        v.show(section);
        v
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), Instant::now()));
    }

    /// Whether the page showing is the guide's: Sound's, or Timing's for Calibrate timing.
    fn guide_here(&self) -> bool {
        match &self.setup {
            Some(g) if g.only_timing => matches!(self.section, "timing" | "practice"),
            Some(_) => self.section == sound_id(),
            None => false,
        }
    }

    /// Show another section.
    pub fn show(&mut self, section: &str) {
        self.setup = None;
        // the guided guitar setup on the Audio page (the first-run prompt's Set up); "setup_<step>": a screenshot
        // of that step, made up as it would be
        // (and "setup_measure": calibrating, held mid-strum; "calibrate" / "calibrate_<step>": the Audio page's own
        // Calibrate, P23.10)
        if let Some(rest) = section.strip_prefix("setup") {
            self.section = sound_id();
            match rest.strip_prefix('_') {
                Some("measure") => self.open_staged_measure(false),
                Some("timingmeasure") => self.open_staged_timing("measure"),
                Some("timingscreen") => self.open_staged_timing("screenmeasure"),
                Some("timingresult") => self.open_staged_timing("result"),
                // (the play-along, P38.4: listening, playing mid-tune, its result; "timingclicks": the click tool)
                Some(k @ ("playlisten" | "playing" | "playresult" | "timingclicks")) => {
                    self.open_staged_timing(k)
                }
                Some("settingsfold") => {
                    self.open_guide(crate::guitar_setup::Step::Settings, true);
                    self.folds.open("setup-levels");
                }
                Some(k) => match crate::guitar_setup::Step::parse(k) {
                    Some(step) => self.open_guide(step, true),
                    None => self.open_guide(crate::guitar_setup::Step::Input, false),
                },
                None => self.open_guide(crate::guitar_setup::Step::Input, false),
            }
            return;
        }
        // Settings > Timing's Calibrate timing ("timingtool"; "timingtool_<state>": a screenshot of it, P36.2)
        if let Some(rest) = section.strip_prefix("timingtool") {
            self.section = nav::resolve("timing", theme::classic()).unwrap_or("timing");
            self.open_timing_guide(match rest.strip_prefix('_') {
                Some(k) => Some(k),
                None if rest.is_empty() => None,
                None => Some(""),
            });
            return;
        }
        if let Some(rest) = section.strip_prefix("calibrate") {
            self.section = sound_id();
            match rest.strip_prefix('_') {
                Some("measure") => self.open_staged_measure(true),
                Some(k) => {
                    let step =
                        crate::guitar_setup::Step::parse(k).unwrap_or(crate::guitar_setup::Step::Calibrate);
                    self.open_cal_guide(step, true)
                }
                None => self.open_cal_guide(crate::guitar_setup::Step::Calibrate, false),
            }
            return;
        }
        if section == "audio_level" || section == "sound_level" {
            // (the screenshots: the Sound page scrolled to its input level and the folds under it, the trim open;
            // clamped as it's drawn)
            self.section = sound_id();
            self.folds.open("trim");
            self.scroll = 2000.0;
            return;
        }
        if section == "audio_lowlat" || section == "timing_lowlat" {
            // (the first-time setup's picture, P23.14: Timing's Reduce delay; the classic look's Practice has none)
            self.section = nav::resolve("timing", theme::classic()).unwrap_or(sound_id());
            self.scroll = 0.0;
            return;
        }
        if section == "tone_knobs" {
            // (the screenshots: the Tone page with the custom knobs out, P34.8)
            self.section = "tone";
            self.folds.open("knobs");
            self.scroll = 0.0;
            return;
        }
        if section == "display_end" {
            // (the screenshots: the Display page at its end, the highway notes and the tuner's face, P48.3)
            self.section = "display";
            self.scroll = 4000.0;
            return;
        }
        if section == "library_rec" {
            // (the screenshots: the Library page's recordings folders, at its end)
            self.section = "library";
            self.scroll = 4000.0;
            return;
        }
        if section == "library_keys" {
            // (the welcome's Add keys, and the screenshots: the Library page at its end, the keys open)
            self.section = "library";
            self.folds.open("keys");
            self.scroll = 2000.0;
            return;
        }
        // Neural Amp Modeler ("nam"; "nam_<state>": a screenshot of its setup's step or its management page)
        if let Some(rest) = section.strip_prefix("nam")
            && (rest.is_empty() || rest.starts_with('_'))
            && nav::resolve("nam", theme::classic()).is_some()
        {
            self.show_nam(rest.trim_start_matches('_'));
            return;
        }
        if let Some(s) = nav::resolve(section, theme::classic()) {
            self.section = s;
            self.scroll = 0.0;
        }
    }

    // -- input --------------------------------------------------------------------------------------------------
    /// Keys and the wheel (clicks and drags go through the widgets as they're drawn).
    pub fn handle(&mut self, st: &mut AppState, ev: &Event) {
        if self.naming.is_some() {
            self.handle_naming(ev);
            return;
        }
        // a reset asking first: Enter resets, Esc doesn't
        if let Some(c) = &self.confirm {
            if let Event::Key {
                key, pressed: true, ..
            } = ev
            {
                match key {
                    Key::Enter => {
                        let a = c.action.clone();
                        self.confirm = None;
                        self.acts.push(a);
                    }
                    Key::Escape => self.confirm = None,
                    _ => {}
                }
            }
            return;
        }
        // typing a key: the box has the keys (Esc leaves it)
        let typing = st.ctx.memory(|m| {
            m.focused().is_some_and(|id| {
                ["psarc", "sng", "t3k"]
                    .iter()
                    .any(|k| id == library::key_box_id(k))
            })
        });
        if typing {
            return;
        }
        if self.setup.is_some() && self.guide_here() && self.guide_key(ev) {
            return;
        }
        match ev {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                // the panel's own keys (P35.7): Esc closes, Up / Down scroll, Tab and Ctrl+Up / Down switch pages
                if let Some(nav) = nav::nav_key(*key, *modifiers) {
                    match nav {
                        nav::Nav::Close => self.closed = true,
                        nav::Nav::Scroll(d) => self.scroll = (self.scroll + d).clamp(0.0, self.max_scroll),
                        nav::Nav::Page(by) => {
                            self.section = nav::step(theme::classic(), self.section, by);
                            self.scroll = 0.0;
                        }
                    }
                    return;
                }
                let act = match key {
                    Key::Comma | Key::Period => {
                        let k = if modifiers.command {
                            "input_offset"
                        } else {
                            "av_offset"
                        };
                        // (5 ms a press, 1 ms with Shift, as everywhere; P37.6a. Ctrl+. makes the delay longer:
                        // `input_offset` down, D132)
                        let step = if modifiers.shift { 1 } else { timing::NUDGE_MS };
                        let up = (*key == Key::Period) != (k == "input_offset");
                        Some(format!("ms:{k}:{}", if up { step } else { -step }))
                    }
                    Key::Num9 => Some("vol:-0.1".into()),
                    Key::Num0 => Some("vol:0.1".into()),
                    Key::Num7 => Some("gvol:-0.1".into()),
                    Key::Num8 => Some("gvol:0.1".into()),
                    Key::M => Some("monitor".into()),
                    Key::T => Some("tonecycle".into()),
                    Key::N => Some("nota:show".into()),
                    Key::Y => Some("tile:lyrics".into()),
                    Key::F5 => Some("rescan".into()),
                    _ => None,
                };
                if let Some(a) = act {
                    self.acts.push(a);
                }
            }
            Event::MouseWheel { unit, delta, .. } => {
                let Some(pos) = st.ctx.input(|i| i.pointer.hover_pos()) else {
                    return;
                };
                let notches = wheel_notches(*unit, delta.y);
                if notches == 0.0 {
                    return;
                }
                if let Some(&(_, key)) = self.knob_rects.iter().find(|(r, _)| r.contains(pos)) {
                    let (lo, hi) = tone::range(key);
                    let v = tone::param(st, key) + (hi - lo) / 50.0 * notches.signum();
                    self.acts.push(format!("param:{key}:{v}"));
                } else if self.content.contains(pos) {
                    self.scroll = (self.scroll - notches * 60.0).clamp(0.0, self.max_scroll);
                }
            }
            _ => {}
        }
    }

    fn handle_naming(&mut self, ev: &Event) {
        let Some(name) = &mut self.naming else { return };
        match ev {
            Event::Text(s) => name.push_str(s),
            Event::Key {
                key, pressed: true, ..
            } => match key {
                Key::Backspace => {
                    name.pop();
                }
                Key::Escape => self.naming = None,
                Key::Enter => {
                    let n = name.trim().to_string();
                    self.naming = None;
                    self.acts.push(format!("savenamed:{n}"));
                }
                _ => {}
            },
            _ => {}
        }
    }

    // -- actions --------------------------------------------------------------------------------------------------
    /// After a change: save, and let an open song pick it up.
    fn changed(&self, st: &mut AppState, player: Option<&mut PlayerView>) {
        st.save_config();
        theme::set_string_colors(st.config.bool("string_colors", false));
        if let Some(p) = player {
            p.reload_settings(st);
        }
    }

    fn act(&mut self, st: &mut AppState, mut player: Option<&mut PlayerView>, name: &str) {
        let (verb, arg) = name.split_once(':').unwrap_or((name, ""));
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        match verb {
            "close" => self.closed = true,
            "saveconfig" => st.save_config(),
            "section" => self.show(arg),
            "fold" => self.folds.toggle(arg),
            "reset" => self.ask_reset(arg),
            "doreset" => self.do_reset(st, player, arg),
            "noconfirm" => self.confirm = None,
            "theme" => st.apply_theme(ThemeName::parse(arg)),
            "knobs" => {
                st.config.set("knobs", arg);
                knobs::set_style(KnobStyle::parse(arg));
                st.save_config();
            }
            "input" | "output" | "ch" | "buffer" | "exclusive" | "rs_setup" | "devrescan" => {
                self.audio_act(st, verb, arg)
            }
            "cal" | "settrim" | "setpluck" | "trimstep" | "pluckstep" | "calrec" | "calkind" | "calinst" => {
                self.cal_act(st, verb, arg)
            }
            "setup" => self.guide_act(st, arg),
            "band" => {
                self.band_act(st);
            }
            "pa" => {
                self.play_act(st, arg);
                self.changed(st, player);
            }
            "tcal" => {
                self.timing_act(st, arg);
                self.changed(st, player);
            }
            "update" => crate::update::act(st, arg),
            "nam" => self.nam_act(st, arg),
            "set" => {
                if let Some((k, v)) = arg.split_once(':') {
                    st.config.set(k, v);
                    self.changed(st, player);
                }
            }
            "toggle" => {
                let (k, dflt) = arg.split_once(':').unwrap_or((arg, ""));
                let v = !st.config.bool(k, dflt != "off");
                st.config.set(k, v);
                self.changed(st, player);
            }
            "nota" => {
                let mut m = st
                    .config
                    .get("notation")
                    .and_then(|v| v.as_object())
                    .cloned()
                    .unwrap_or_default();
                let v = !m.get(arg).and_then(|v| v.as_bool()).unwrap_or(true);
                m.insert(arg.into(), v.into());
                st.config.set("notation", serde_json::Value::Object(m));
                self.changed(st, player);
            }
            "tile" => {
                tiles::tile_action(st, name);
                self.changed(st, player);
            }
            "ms" => {
                if let Some((k, step)) = arg.split_once(':') {
                    let v = st.config.f64(k, 0.0) + step.parse::<f64>().unwrap_or(0.0) / 1000.0;
                    st.config.set(k, (v * 1000.0).round() / 1000.0);
                    self.changed(st, player);
                }
            }
            "notewidget" => {
                let on = !st.config.bool("note_widget", true);
                st.config.set("note_widget", on);
                self.changed(st, player);
            }
            "rescan" => {
                st.rescan();
                self.say("Rescanning your song folders…");
            }
            "addfolder" => {
                let from = st.dirs.first().cloned();
                if let Some(dir) = crate::dialogs::pick_folder(from.as_deref()) {
                    match st.add_folder(&dir) {
                        Ok(msg) => self.say(msg),
                        Err(e) => self.say(e),
                    }
                } else if crate::dialogs::unavailable() {
                    self.say(crate::dialogs::NO_PICKER);
                }
            }
            "rmfolder" => {
                if let Some(dir) = arg.parse::<usize>().ok().and_then(|i| st.dirs.get(i).cloned()) {
                    match st.remove_folder(&dir) {
                        Ok(msg) => self.say(msg),
                        Err(e) => self.say(e),
                    }
                }
            }
            "searchdlc" => {
                let on = !st.config.search_dlc();
                st.set_search_dlc(on);
                self.say(if on {
                    "Searching RS's dlc folder for songs"
                } else {
                    "RS's dlc folder left out: the folders you added only"
                });
            }
            "savekeys" => self.save_keys(st),
            "recdir_change" | "recdir_open" | "recdir_reset" => self.rec_folder(st, verb, arg),
            "loadkeys" => self.load_keys_file(st),
            "amproom" => {
                self.open_room = true;
                self.closed = true;
            }
            "firstsetup" => {
                self.first_setup = true;
                self.first_setup_at = (!arg.is_empty()).then(|| arg.to_string());
                self.closed = true;
            }
            "prefer" => {
                st.config.set("prefer", arg);
                st.regroup();
                self.changed(st, player);
            }
            "monitor" => {
                let (key, dflt) = st.monitor_key();
                let on = !engine
                    .as_ref()
                    .map(|e| e.monitor())
                    .unwrap_or(st.config.bool(key, dflt));
                if let Some(e) = &engine {
                    e.set_monitor(on);
                }
                st.config.set(key, on);
                self.changed(st, player);
            }
            "vol" | "gvol" | "setvol" | "setgvol" => {
                let step: f32 = arg.parse().unwrap_or(0.0);
                let set = verb.starts_with("set");
                if verb.ends_with("gvol") {
                    let now = engine
                        .as_ref()
                        .map(|e| e.guitar_gain())
                        .unwrap_or(st.config.f64("guitar_volume", 0.8) as f32);
                    let v = (((if set { step } else { now + step }).clamp(0.0, 2.0)) * 100.0).round() / 100.0;
                    if let Some(e) = &engine {
                        e.set_guitar_gain(v);
                    }
                    st.config.set("guitar_volume", v as f64);
                } else {
                    let now = match (&engine, player.as_deref()) {
                        (Some(e), _) => e.song_gain(),
                        (None, Some(p)) => p.transport.volume(),
                        _ => st.config.f64("volume", 0.8) as f32,
                    };
                    let v = (((if set { step } else { now + step }).clamp(0.0, 1.0)) * 100.0).round() / 100.0;
                    match (&engine, player.as_deref_mut()) {
                        (Some(e), _) => e.set_song_gain(v),
                        (None, Some(p)) => p.transport.set_volume(v),
                        _ => {}
                    }
                    st.config.set("volume", v as f64);
                }
                if !set {
                    self.changed(st, player);
                }
            }
            "tone" | "tonecycle" | "preset" | "delpreset" | "savepreset" | "savenamed" | "resettone"
            | "param" => {
                if verb == "tone"
                    && matches!(arg, "song" | "mine")
                    && let Some(p) = player.as_deref_mut()
                {
                    p.rack.set_follow(st, arg == "song"); // (a tone picked by hand on the Rig tile gives way)
                }
                self.tone_act(st, verb, arg);
                self.changed(st, player);
            }
            _ => {}
        }
    }

    /// Ask before a reset (`page`, or "all").
    fn ask_reset(&mut self, page: &str) {
        let name = sections()
            .iter()
            .find(|p| p.0 == page)
            .map(|p| p.1)
            .unwrap_or("this page");
        let (title, text) = match page {
            "all" => (
                "Reset all settings?".to_string(),
                "Every page but your tone goes back to its defaults: the devices (Automatic again), volumes, \
                 timing, low-latency output and the buffer, the display, the song list and updates. Kept: your song \
                 folders, RS's keys, scores, saved licks, your tone and saved tones, each input's level \
                 check and the theme.",
            ),
            "sound" | "audio" => (
                format!("Reset {name} to defaults?"),
                "Song and guitar volume, Hear guitar, the guitar input and the output (Automatic again), the \
                 channel and the RS2014 setup. Each input's level check is kept.",
            ),
            "timing" | "practice" => (
                format!("Reset {name} to defaults?"),
                "The delay and Chart sync back to 0 ms, Low-latency output off and the buffer on Auto.",
            ),
            "tone" => (
                format!("Reset {name} to defaults?"),
                "The tone back to As is and the custom knobs to where they start. Your saved tones are kept.",
            ),
            "display" => (
                format!("Reset {name} to defaults?"),
                "The knobs, string colours, the note tile, notation, the tiles and their order, the lyrics' next \
                 line and the note widget window. The theme is kept.",
            ),
            "library" => (
                format!("Reset {name} to defaults?"),
                "Expand & preview, which copy of a song in both formats plays, and the recordings folders. Your \
                 song folders and keys are kept.",
            ),
            "nam" => (
                format!("Reset {name} to defaults?"),
                "The interface reference level back to +12 dBu. Kept: your captures, your TONE3000 key and NAM for                  songs on or off.",
            ),
            _ => (
                format!("Reset {name} to defaults?"),
                "Checking for updates on start, and a version you skipped.",
            ),
        };
        self.confirm = Some(Confirm {
            title,
            text: text.to_string(),
            action: format!("doreset:{page}"),
        });
    }

    /// Reset a page (or all of them, "all") and put it into effect: the engine's devices, volumes, monitoring and
    /// tone, the knobs' look, an open song.
    fn do_reset(&mut self, st: &mut AppState, player: Option<&mut PlayerView>, page: &str) {
        if page == "all" {
            reset::reset_all(&mut st.config);
        } else {
            reset::reset(&mut st.config, page);
        }
        if let Some(a) = &st.audio {
            let prefs = reset::prefs_of(&st.config);
            if a.prefs() != prefs {
                a.set_prefs(prefs);
            }
            let (k, d) = st.monitor_key();
            a.engine.set_monitor(st.config.bool(k, d));
            a.engine.set_song_gain(st.config.f64("volume", 0.8) as f32);
            a.engine
                .set_guitar_gain(st.config.f64("guitar_volume", 0.8) as f32);
            if page == "tone" {
                a.engine.set_tone(crate::tones::current(&st.config));
            }
        }
        if matches!(page, "nam" | "all") {
            qrs_audio::board::nam::set_input_dbu(nam::input_dbu(&st.config));
        }
        knobs::set_style(KnobStyle::parse(&st.config.str("knobs", "amp")));
        self.say(if page == "all" {
            "Every setting is back to its default (your folders, keys, scores and tones kept)".to_string()
        } else {
            let name = sections()
                .iter()
                .find(|p| p.0 == page)
                .map(|p| p.1)
                .unwrap_or("The page");
            format!("{name}: back to the defaults")
        });
        self.changed(st, player);
    }

    // -- drawing --------------------------------------------------------------------------------------------------
    /// At most 1200 x 820, 32 and 24 clear of the window's edges, in the middle (`int(min(1200 * s, w - 64 * s))`).
    pub fn panel_rect(area: Rect) -> Rect {
        centred(
            area,
            vec2(
                snap((1200.0f32).min(area.width() - 64.0)),
                snap((820.0f32).min(area.height() - 48.0)),
            ),
        )
    }

    /// The panel (and, with `draw` false, only the actions queued by keys: the frame the page is captured in).
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        st: &mut AppState,
        mut player: Option<&mut PlayerView>,
        area: Rect,
        draw: bool,
    ) {
        if draw {
            self.draw(ui, st, area);
        }
        for a in std::mem::take(&mut self.acts) {
            self.act(st, player.as_deref_mut(), &a);
        }
        // the meters and lights move; the caret blinks
        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }

    /// A raised panel over the (blurred) page: Settings and Done along the top, the sections in a sunken well on
    /// the left, the section's cards on the right (they scroll when they don't fit).
    fn draw(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let th = t();
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let panel = Self::panel_rect(area);
        self.panel = panel;
        let modal = self.naming.is_some() || self.confirm.is_some();
        // modal: a click outside the panel closes it
        let outside = interact(ui, area, "settings-modal", Sense::click());
        if outside.clicked()
            && outside
                .interact_pointer_pos()
                .is_some_and(|pos| !panel.contains(pos))
            && !modal
        {
            self.closed = true;
        }
        paint::floating(&p, panel, Some(30.0));
        let pad = px(28.0);
        let head = Rect::from_min_size(
            pos2(panel.min.x + pad, panel.min.y + pad),
            vec2(panel.width() - 2.0 * pad, px(46.0)),
        );
        let nav_w = px(220.0);
        text::draw(
            &p,
            "Settings",
            pos2(head.min.x + px(4.0), centre_y(head)),
            Align2::LEFT_CENTER,
            Style::new(26.0, 800, th.text),
            None,
        );
        // (while the guitar setup runs, Esc closes the setup: Done shows no key)
        let done_key = if self.setup.is_some() && self.guide_here() {
            ""
        } else {
            "Esc"
        };
        // (P37.6a: with the guided setup open, Done would read as "the setup's done": it closes Settings, and says so)
        let guiding = self.setup.is_some() && self.guide_here();
        let done_label = if guiding { "Close Settings" } else { "Done" };
        let dw = widgets::button_width(&ctx, done_label, done_key, 15.0, false) + px(8.0);
        let done = Rect::from_min_size(pos2(head.max.x - dw, head.min.y), vec2(dw, head.height()));
        if !modal
            && widgets::button(
                ui,
                done,
                "set-done",
                done_label,
                done_key,
                ButtonStyle {
                    size: 15.0,
                    r: Some(14.0),
                    ..Default::default()
                },
            )
            .clicked()
        {
            self.acts.push("close".into());
        }
        // (the first-time setup again is About's now, P35.3: one way into each setup)
        let classic = theme::classic();
        text::draw(
            &p,
            if guiding {
                "Each step saves as you go: closing keeps what's done"
            } else if classic {
                "Changes are saved straight away"
            } else {
                "Changes save straight away"
            },
            pos2(head.min.x + nav_w + px(28.0), centre_y(head)),
            Align2::LEFT_CENTER,
            Style::new(13.0, 500, th.caption),
            Some(done.min.x - head.min.x - nav_w - px(48.0)),
        );
        // the pages, each with its hint under its name (the classic look: names only, as the Python app's)
        let n = sections().len() as f32;
        let row_h = px(if classic { 48.0 } else { 56.0 });
        let nav = Rect::from_min_size(
            pos2(head.min.x, head.max.y + px(24.0)),
            vec2(nav_w, 2.0 * px(10.0) + n * row_h + (n - 1.0) * px(6.0)),
        );
        paint::well(&p, nav, 22.0);
        let mut y = nav.min.y + px(10.0);
        for &(key, label, hint) in sections() {
            let r = Rect::from_min_size(pos2(nav.min.x + px(10.0), y), vec2(nav.width() - px(20.0), row_h));
            let on = key == self.section;
            if on {
                paint::raised(&p, r, Some(14.0), 4.0, Fill::Base, 0.0);
            }
            let resp = interact(ui, r, ("set-nav", key), Sense::click());
            let hot = resp.hovered() && !modal;
            if hot {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() && !modal {
                self.acts.push(format!("section:{key}"));
            }
            let style = Style::new(
                15.0,
                if on { 800 } else { 600 },
                if on {
                    th.accent_ink
                } else if hot {
                    th.text
                } else {
                    th.text_2
                },
            );
            if hint.is_empty() {
                text::draw(
                    &p,
                    label,
                    pos2(r.min.x + px(16.0), centre_y(r)),
                    Align2::LEFT_CENTER,
                    style,
                    None,
                );
            } else {
                text::draw(
                    &p,
                    label,
                    pos2(r.min.x + px(16.0), centre_y(r) - px(9.0)),
                    Align2::LEFT_CENTER,
                    style,
                    Some(r.width() - px(24.0)),
                );
                text::draw(
                    &p,
                    hint,
                    pos2(r.min.x + px(16.0), centre_y(r) + px(11.0)),
                    Align2::LEFT_CENTER,
                    Style::new(12.0, 500, if on { th.text_2 } else { th.caption }),
                    Some(r.width() - px(24.0)),
                );
            }
            y += row_h + px(6.0);
        }
        let content = Rect::from_min_size(
            pos2(nav.max.x + px(28.0), nav.min.y),
            vec2(head.max.x - nav.max.x - px(28.0), panel.max.y - pad - nav.min.y),
        );
        self.content = content;
        let mut clip = inflate(content, px(40.0), px(40.0)).intersect(inflate(panel, -px(10.0), -px(10.0)));
        // (pygame's `clip.top = ...` moves the rect: its height stays)
        let top = clip.min.y.max(head.max.y + px(6.0));
        clip = clip.translate(vec2(0.0, top - clip.min.y));
        self.knob_rects.clear();
        self.tile_rows.clear();
        let y0 = content.min.y - snap(self.scroll);
        let body = Rect::from_min_size(pos2(content.min.x, y0), content.size());
        let bottom = {
            let mut child = ui.new_child(UiBuilder::new().max_rect(clip));
            child.set_clip_rect(clip.intersect(ui.clip_rect()));
            if modal {
                child.disable();
            }
            let guide = self.setup.is_some();
            let bottom = match self.section {
                "timing" | "practice" if self.setup.as_ref().is_some_and(|g| g.only_timing) => {
                    self.draw_guide(&mut child, st, body)
                }
                "sound" | "audio" if guide => self.draw_guide(&mut child, st, body),
                "sound" | "audio" => self.draw_sound(&mut child, st, body),
                "timing" => self.draw_timing(&mut child, st, body),
                "practice" => self.draw_practice(&mut child, st, body),
                "tone" => self.draw_tone(&mut child, st, body),
                "display" => self.draw_display(&mut child, st, body),
                "about" => self.draw_about(&mut child, st, body),
                "nam" => self.draw_nam(&mut child, st, body),
                _ => self.draw_library(&mut child, st, body),
            };
            // each page ends in its reset (not the classic look's: the Python app had none)
            if classic || (guide && self.guide_here()) {
                bottom
            } else {
                self.reset_link(&child, st, body, bottom + px(18.0))
            }
        };
        // cards scrolled under the top or bottom edge fade there instead of being cut off by it
        paint::fade_edges(
            &p,
            clip,
            self.scroll > 0.5,
            self.scroll < self.max_scroll - 0.5,
            24.0,
            FaceKind::Base {
                rect: panel,
                sunk: false,
            },
        );
        self.max_scroll = (bottom - y0 - content.height()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        if let Some((msg, at)) = &self.message
            && at.elapsed() < Duration::from_secs(5)
        {
            widgets::toast(&p, msg, pos2(centre_x(content), panel.max.y - px(40.0)), "");
        }
        if self.naming.is_some() {
            self.draw_naming(ui, area);
        }
        if self.confirm.is_some() {
            self.draw_confirm(ui, area);
        }
        if self.knob_drag.is_some() {
            ctx.set_cursor_icon(CursorIcon::ResizeVertical);
        }
    }

    /// "Reset <page> to defaults" at the right under the page's cards (greyed out when nothing is off its
    /// default). Returns the y under it.
    fn reset_link(&mut self, ui: &Ui, st: &AppState, body: Rect, y: f32) -> f32 {
        let ctx = ui.ctx().clone();
        let Some(&(page, name, _)) = sections().iter().find(|p| p.0 == self.section) else {
            return y;
        };
        // (About's is Reset all, on the page itself)
        if page == "about" {
            return y - px(18.0);
        }
        let label = format!("Reset {name} to defaults");
        let bw = widgets::button_width(&ctx, &label, "", 13.0, false);
        let r = Rect::from_min_size(pos2(body.max.x - bw, y), vec2(bw, px(40.0)));
        if !reset::differs(&st.config, page) {
            // nothing to reset: the words only, faint
            text::draw(
                ui.painter(),
                &label,
                pos2(r.max.x - px(14.0), centre_y(r)),
                Align2::RIGHT_CENTER,
                Style::new(13.0, 600, t().tick),
                None,
            );
            return r.max.y;
        }
        self.button(
            ui,
            r,
            &label,
            "",
            &format!("reset:{page}"),
            ButtonStyle {
                size: 13.0,
                r: Some(12.0),
                plain: true,
                ..Default::default()
            },
        );
        r.max.y
    }

    /// A reset's question over the panel: what goes back and what's kept; Cancel (Esc) / Reset (Enter).
    fn draw_confirm(&mut self, ui: &Ui, area: Rect) {
        let Some(c) = &self.confirm else { return };
        let (title, text_, action) = (c.title.clone(), c.text.clone(), c.action.clone());
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        paint::dim(&p, area, None);
        let _ = interact(ui, area, "set-confirm-modal", Sense::click());
        let pad = px(24.0);
        let w = snap((540.0f32).min(area.width() - 40.0));
        let cst = Style::new(14.0, 500, th.text_2);
        let lines = text::wrap(&ctx, &text_, cst, w - 2.0 * pad);
        let lh = line_h(&ctx, 14.0) + px(3.0);
        let panel = centred(
            area,
            vec2(
                w,
                pad + px(28.0) + px(14.0) + lines.len() as f32 * lh + px(20.0) + px(48.0) + pad,
            ),
        );
        paint::floating(&p, panel, Some(26.0));
        text::draw(
            &p,
            &title,
            pos2(panel.min.x + pad, panel.min.y + pad),
            Align2::LEFT_TOP,
            Style::new(20.0, 800, th.text),
            Some(w - 2.0 * pad),
        );
        let mut y = panel.min.y + pad + px(28.0) + px(14.0);
        for line in &lines {
            text::draw(&p, line, pos2(panel.min.x + pad, y), Align2::LEFT_TOP, cst, None);
            y += lh;
        }
        let ow = widgets::button_width(&ctx, "Reset", "Enter", 14.0, false);
        let cw = widgets::button_width(&ctx, "Cancel", "Esc", 14.0, false);
        let by = panel.max.y - pad - px(48.0);
        let ok = Rect::from_min_size(pos2(panel.max.x - pad - ow, by), vec2(ow, px(48.0)));
        let cancel = Rect::from_min_size(pos2(ok.min.x - px(12.0) - cw, by), vec2(cw, px(48.0)));
        let bs = ButtonStyle {
            size: 14.0,
            r: Some(14.0),
            ..Default::default()
        };
        if widgets::button(ui, cancel, "set-confirm-cancel", "Cancel", "Esc", bs).clicked() {
            self.confirm = None;
        }
        if widgets::button(
            ui,
            ok,
            "set-confirm-ok",
            "Reset",
            "Enter",
            ButtonStyle { danger: true, ..bs },
        )
        .clicked()
        {
            self.confirm = None;
            self.acts.push(action);
        }
    }

    /// A fold (`widgets::fold_row`) at (x, y), `w` wide: returns its height and whether it's open.
    #[allow(clippy::too_many_arguments)]
    fn fold(&mut self, ui: &Ui, x: f32, y: f32, w: f32, id: &str, title: &str, hint: &str) -> (f32, bool) {
        let open = self.folds.is_open(id);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, px(widgets::FOLD_ROW_H)));
        if widgets::fold_row(ui, r, ("set-fold", id), title, hint, open).clicked() {
            self.acts.push(format!("fold:{id}"));
        }
        (r.height(), open)
    }

    /// The accent dot beside a value off its default (P35.7), centred on `at`.
    fn changed_dot(p: &Painter, at: egui::Pos2) {
        paint::dot(p, at, 3.5, t().accent);
    }

    // -- building blocks ------------------------------------------------------------------------------------------
    /// A click target for an action (the hand cursor over it).
    fn hit(&mut self, ui: &Ui, rect: Rect, action: &str) -> egui::Response {
        let resp = interact(ui, rect, ("set-hit", action), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.acts.push(action.to_string());
        }
        resp
    }

    /// How far `heading` moves down (as the Python app counts it: a 13-size line and 12 under it, a pixel more than
    /// `heading` moves).
    fn h_heading(ctx: &egui::Context) -> f32 {
        line_h(ctx, 13.0) + px(12.0)
    }

    /// The height of a `switch_row`.
    fn h_switch(size: &str, hint: bool) -> f32 {
        let sh = switch_size(size).y;
        px(sh + 8.0).max(if hint { px(40.0) } else { px(sh) })
    }

    /// A small-caps heading (with its key, and a hint at the right); returns the y under it.
    #[allow(clippy::too_many_arguments)]
    fn heading(
        &self,
        p: &Painter,
        title: &str,
        x: f32,
        y: f32,
        right: Option<f32>,
        note: &str,
        key: &str,
    ) -> f32 {
        let th = t();
        let r = text::caps(p, title, pos2(x, y), 12.0, th.caption, Align2::LEFT_TOP, None);
        if !key.is_empty() {
            keycap(
                p,
                key,
                pos2(r.max.x + px(10.0), centre_y(r)),
                Align2::LEFT_CENTER,
                11.0,
                CapOn::Material,
            );
        }
        if !note.is_empty()
            && let Some(right) = right
        {
            let room = right - r.max.x - px(if key.is_empty() { 24.0 } else { 60.0 });
            text::draw(
                p,
                note,
                pos2(right, centre_y(r)),
                Align2::RIGHT_CENTER,
                Style::new(12.0, 500, th.caption),
                Some(room.max(pixels(40.0))),
            );
        }
        r.max.y + px(12.0)
    }

    fn button(&mut self, ui: &Ui, rect: Rect, label: &str, key: &str, action: &str, st: ButtonStyle) {
        if widgets::button(ui, rect, ("set-btn", action), label, key, st).clicked() {
            self.acts.push(action.to_string());
        }
    }

    /// A label (and a hint under it) with a switch at the right; the whole row toggles. Returns its height.
    #[allow(clippy::too_many_arguments)]
    fn switch_row(
        &mut self,
        ui: &Ui,
        x: f32,
        y: f32,
        w: f32,
        label: &str,
        hint: &str,
        on: bool,
        action: &str,
        size: &str,
        key: &str,
        weight: u16,
    ) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ss = switch_size(size);
        let h = Self::h_switch(size, !hint.is_empty());
        let row = Rect::from_min_size(pos2(x, y), vec2(w, h));
        let cy = centre_y(row);
        // sr.midright = (row.right, row.centery)
        let sw = vec2(px(ss.x), px(ss.y));
        let sr = Rect::from_min_size(pos2(row.max.x - sw.x, cy - snap(sw.y / 2.0)), sw);
        widgets::draw_switch(&p, sr, egui::Id::new(("set-sw", action)), on);
        let size_l = if size == "lg" || size == "md" { 15.0 } else { 14.0 };
        let lst = Style::new(size_l, weight, th.text);
        let tr = if hint.is_empty() {
            text::draw(
                &p,
                label,
                pos2(x, cy),
                Align2::LEFT_CENTER,
                lst,
                Some(w - sr.width() - px(70.0)),
            )
        } else {
            let tr = text::draw(
                &p,
                label,
                pos2(x, cy - px(9.0)),
                Align2::LEFT_CENTER,
                lst,
                Some(w - sr.width() - px(70.0)),
            );
            text::draw(
                &p,
                hint,
                pos2(x, cy + px(11.0)),
                Align2::LEFT_CENTER,
                Style::new(12.0, 500, self.hint_color.take().unwrap_or(th.caption)),
                Some(w - sr.width() - px(20.0)),
            );
            tr
        };
        if !key.is_empty() {
            keycap(
                &p,
                key,
                pos2(tr.max.x + px(10.0), centre_y(tr)),
                Align2::LEFT_CENTER,
                11.0,
                CapOn::Material,
            );
        }
        self.hit(ui, row, action);
        h
    }

    /// A label with its keys (each its own cap) and its value, over a slider. Returns the height used.
    #[allow(clippy::too_many_arguments)]
    fn slider_row(
        &mut self,
        ui: &Ui,
        x: f32,
        y: f32,
        w: f32,
        label: &str,
        value: &str,
        u: f32,
        kind: &'static str,
        keys: &str,
    ) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let tr = text::draw(
            &p,
            label,
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(15.0, 700, th.text),
            None,
        );
        let cy = centre_y(tr);
        let mut kx = tr.max.x + px(10.0);
        for k in keys.split_whitespace() {
            kx = keycap(&p, k, pos2(kx, cy), Align2::LEFT_CENTER, 11.0, CapOn::Material)
                .max
                .x
                + px(4.0);
        }
        text::draw(
            &p,
            value,
            pos2(x + w, cy),
            Align2::RIGHT_CENTER,
            Style::new(17.0, 800, th.text),
            None,
        );
        let knob_r = px(14.0);
        let groove = Rect::from_min_size(
            pos2(x + knob_r, tr.max.y + px(16.0)),
            vec2(w - 2.0 * knob_r, px(12.0)),
        );
        let resp = interact(
            ui,
            inflate(groove, px(16.0), px(24.0)),
            ("set-slider", kind),
            Sense::click_and_drag(),
        );
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.is_pointer_button_down_on() || resp.dragged() {
            self.slider = Some(kind);
            if let Some(pos) = resp.interact_pointer_pos() {
                let u = ((pos.x - groove.min.x) / groove.width().max(1.0)).clamp(0.0, 1.0);
                let v = if kind == "gvol" { 2.0 * u } else { u };
                self.acts.push(format!("set{kind}:{v}"));
            }
        } else if self.slider == Some(kind) {
            self.slider = None;
            self.acts.push("saveconfig".into());
        }
        let hot = self.slider == Some(kind);
        widgets::slider(&p, groove, u, None, 28.0, hot, None);
        groove.max.y + knob_r - y
    }

    /// A "less" key, the value in a small screen, and a "more" key.
    fn stepper(
        &mut self,
        ui: &Ui,
        x: f32,
        y: f32,
        w: f32,
        minus: (&str, &str, &str),
        value: &str,
        plus: (&str, &str, &str),
    ) -> f32 {
        let (h, gap, vw) = (px(46.0), px(10.0), px(96.0));
        let bw = snap((w - vw - 2.0 * gap) / 2.0);
        let st = ButtonStyle {
            size: 13.0,
            r: Some(14.0),
            ..Default::default()
        };
        self.button(
            ui,
            Rect::from_min_size(pos2(x, y), vec2(bw, h)),
            minus.0,
            minus.1,
            minus.2,
            st,
        );
        widgets::button(
            ui,
            Rect::from_min_size(pos2(x + bw + gap, y), vec2(vw, h)),
            ("set-stepval", minus.2),
            value,
            "",
            ButtonStyle {
                value: true,
                size: 16.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        self.button(
            ui,
            Rect::from_min_size(pos2(x + bw + vw + 2.0 * gap, y), vec2(bw, h)),
            plus.0,
            plus.1,
            plus.2,
            st,
        );
        h
    }
}

/// Wrapped caption lines from `y`, each under the last (`ty = blit_text(...).bottom`); returns the y under them.
fn caption_lines(p: &Painter, text_: &str, x: f32, y: f32, w: f32, size: f32, max_lines: usize) -> f32 {
    let th = t();
    let st = Style::new(size, 500, th.caption);
    let mut y = y;
    for line in text::wrap(p.ctx(), text_, st, w).iter().take(max_lines) {
        y = text::draw(p, line, pos2(x, y), Align2::LEFT_TOP, st, None).max.y;
    }
    y
}
