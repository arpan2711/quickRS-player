//! Settings › Neural Amp Modeler (P37.15, D146-D147; new in quickRS, owner request): the one home of everything NAM,
//! the last page (NAM builds only). Until it's set up, the page is a short guided setup of its own (what NAM is, NAM
//! for songs or not, the TONE3000 key, the captures suggested for your songs, where captures live, done); after it,
//! the management page: NAM for songs, the TONE3000 key, your captures (the folder, add, read again, details,
//! remove), the suggestions, the interface reference level the captures are driven at, level matching, and the
//! NAM setup again.
//!
//! TONE3000 (D80, D96): quickRS never downloads or ships a capture by itself; every one comes through your own
//! account, picked by you on TONE3000's own page (`captures::tone3000`, PKCE: no secret in the app). The key it asks
//! for is the publishable one (`t3k_pub_...`, a public id); no sign-in or token is kept.
//!
//! config.json: `nam_setup` / `nam_songs` (the first-time setup's, `crate::setup::set_nam`), `nam_page` ("done" once
//! this page's setup was finished), `nam_input_dbu` (the reference level, +12 dBu unless set), `tone3000_client_id`.

use std::time::Duration;

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_library::Config;
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle};
use serde_json::Value;

use super::library::text_box;
use super::{Confirm, SettingsView, caption_lines, centre_y};
use crate::captures::{self, Kind, tone3000};
use crate::state::AppState;

/// "done" once the page's own setup was finished.
pub const KEY_DONE: &str = "nam_page";
/// The interface reference level (dBu at 0 dBFS).
pub const KEY_DBU: &str = "nam_input_dbu";
/// The TONE3000 publishable key.
pub const KEY_T3K: &str = "tone3000_client_id";

/// The reference levels offered: (label, what it is, dBu at 0 dBFS). -10 dBV is -7.78 dBu.
pub const LEVELS: [(&str, &str, f32); 3] = [
    ("+12 dBu", "most interfaces, NAM's own default", 12.0),
    ("+4 dBu", "pro line level", 4.0),
    ("\u{2212}10 dBV", "consumer line level", -7.78),
];

/// The guided setup's steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    About,
    Songs,
    Account,
    Suggest,
    Files,
    Done,
}

impl Step {
    pub const ALL: [Step; 6] = [
        Step::About,
        Step::Songs,
        Step::Account,
        Step::Suggest,
        Step::Files,
        Step::Done,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Step::About => "about",
            Step::Songs => "songs",
            Step::Account => "account",
            Step::Suggest => "suggest",
            Step::Files => "files",
            Step::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Step> {
        Step::ALL.into_iter().find(|p| p.key() == s)
    }

    pub fn index(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// Its name in the step line.
    pub fn short(self) -> &'static str {
        match self {
            Step::About => "What it is",
            Step::Songs => "Songs",
            Step::Account => "TONE3000",
            Step::Suggest => "Suggested",
            Step::Files => "Your files",
            Step::Done => "Done",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::About => "Neural Amp Modeler: real gear, captured",
            Step::Songs => "Use NAM for songs?",
            Step::Account => "Your TONE3000 account",
            Step::Suggest => "Captures for your songs",
            Step::Files => "Where your captures live",
            Step::Done => "NAM is set up",
        }
    }

    /// The step after this one: with NAM off for songs, the account and the captures are skipped (nothing to fetch).
    pub fn next(self, use_nam: Option<bool>) -> Step {
        match self {
            Step::Songs if use_nam == Some(false) => Step::Done,
            s => Step::ALL[(s.index() + 1).min(Step::ALL.len() - 1)],
        }
    }

    pub fn back(self, use_nam: Option<bool>) -> Step {
        match self {
            Step::Done if use_nam == Some(false) => Step::Songs,
            s => Step::ALL[s.index().saturating_sub(1)],
        }
    }
}

/// Whether NAM has been set up: songs use it (the first-time setup's yes or this page's), or this page's setup was
/// finished (with a no, too). Not set up: the page opens on its guided setup.
pub fn is_set_up(cfg: &Config) -> bool {
    crate::setup::nam_choice(cfg) == Some(true) || cfg.get(KEY_DONE).and_then(Value::as_str) == Some("done")
}

/// The reference level config.json has (the default when it has none).
pub fn input_dbu(cfg: &Config) -> f32 {
    cfg.get(KEY_DBU)
        .and_then(Value::as_f64)
        .map(|v| qrs_audio::board::nam::clamp_dbu(v as f32))
        .unwrap_or(qrs_audio::board::nam::INPUT_DBU)
}

/// Which of `LEVELS` a level is (None: custom).
pub fn level_preset(dbu: f32) -> Option<usize> {
    LEVELS.iter().position(|l| (l.2 - dbu).abs() < 0.05)
}

/// A level as people say it: "+12 dBu", "-10 dBV" for the consumer one.
pub fn level_label(dbu: f32) -> String {
    match level_preset(dbu) {
        Some(i) => LEVELS[i].0.to_string(),
        None => format!("{dbu:+.0} dBu").replace('-', "\u{2212}"),
    }
}

/// A capture's architecture as NAM's people call it: SlimmableContainer is A2, WaveNet and LSTM are A1.
pub fn arch_label(arch: &str) -> String {
    match arch {
        "SlimmableContainer" => "A2".into(),
        "WaveNet" => "A1 (WaveNet)".into(),
        "LSTM" => "A1 (LSTM)".into(),
        "" => "unknown".into(),
        other => other.to_string(),
    }
}

/// A saved key shown without all of it: "t3k_pub_...a1b2".
pub fn key_hint(key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("t3k_pub_\u{2026}{tail}")
}

/// The page's state that lives while Settings is open.
#[derive(Default)]
pub struct NamPage {
    pub step: Option<Step>,
    /// Run the NAM setup again was pressed: the guided setup shows although it's set up.
    pub rerun: bool,
    /// Screenshots: the guided setup (true) or the management page (false), whatever the config says.
    pub staged: Option<bool>,
    /// The answer to Use NAM for songs? while staged (nothing is saved in screenshots).
    pub staged_songs: Option<bool>,
    /// The download this page started last (its end opens the capture's details).
    pick: Option<u64>,
    note: Option<String>,
    key_in: String,
    /// The capture whose details are open.
    open: Option<u32>,
    measuring: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The guided setup card's height last frame.
    card_h: f32,
}

impl SettingsView {
    fn nam_guided(&self, st: &AppState) -> bool {
        match self.nam.staged {
            Some(g) => g,
            None => self.nam.rerun || !is_set_up(&st.config),
        }
    }

    fn nam_use(&self, st: &AppState) -> Option<bool> {
        if self.nam.staged.is_some() {
            return self.nam.staged_songs;
        }
        crate::setup::nam_choice(&st.config)
    }

    /// Screenshots: "nam" (as the config says), "nam_<step>" (the guided setup on that step), "nam_songs_yes"
    /// (the answer picked), "nam_manage" (the management page), "nam_manage_open" (a capture's details open).
    pub(super) fn show_nam(&mut self, what: &str) {
        self.section = "nam";
        self.scroll = 0.0;
        self.nam = NamPage::default();
        match what {
            "" => {}
            "manage" => self.nam.staged = Some(false),
            "manage_open" => {
                self.nam.staged = Some(false);
                self.nam.open = captures::with(|l| l.items.last().map(|i| i.id));
            }
            "songs_yes" => {
                self.nam.staged = Some(true);
                self.nam.step = Some(Step::Songs);
                self.nam.staged_songs = Some(true);
            }
            "manage_low" => {
                // (scrolled to its end: clamped as it's drawn)
                self.nam.staged = Some(false);
                self.scroll = 4000.0;
            }
            k => {
                self.nam.staged = Some(true);
                self.nam.step = Step::parse(k);
                // (Use NAM for songs? shown before it's answered; the later steps after a yes)
                self.nam.staged_songs = (self.nam.step != Some(Step::Songs)).then_some(true);
            }
        }
    }

    // -- actions ------------------------------------------------------------------------------------------------
    pub(super) fn nam_act(&mut self, st: &mut AppState, arg: &str) {
        let (verb, arg) = arg.split_once(':').unwrap_or((arg, ""));
        let staged = self.nam.staged.is_some();
        let step = self.nam.step.unwrap_or(Step::About);
        match verb {
            "next" => self.nam.step = Some(step.next(self.nam_use(st))),
            "back" => self.nam.step = Some(step.back(self.nam_use(st))),
            "songs" => {
                let on = arg == "yes";
                if staged {
                    self.nam.staged_songs = Some(on);
                } else {
                    crate::setup::set_nam(&mut st.config, on);
                    st.save_config();
                }
            }
            "toggle" => {
                if staged {
                    return;
                }
                let on = !captures::songs_use_nam();
                crate::setup::set_nam(&mut st.config, on);
                // (answered here: the page stays the management page with NAM off)
                st.config.set(KEY_DONE, "done");
                st.save_config();
                self.say(if on {
                    "Songs play on your captures where you have them"
                } else {
                    "Songs play on quickRS's own models"
                });
            }
            "finish" => {
                if !staged {
                    st.config.set(KEY_DONE, "done");
                    st.save_config();
                }
                self.nam.rerun = false;
                self.nam.step = None;
                self.scroll = 0.0;
            }
            "rerun" => {
                self.nam.rerun = true;
                self.nam.step = Some(Step::About);
                self.scroll = 0.0;
            }
            "site" => crate::update::open_url(&format!("{}/settings", tone3000::SITE)),
            "savekey" => {
                let k = self.nam.key_in.trim().to_string();
                if !k.starts_with("t3k_pub_") {
                    self.say("That isn't a publishable key: it starts with t3k_pub_");
                    return;
                }
                st.config.set(KEY_T3K, k);
                st.save_config();
                self.nam.key_in.clear();
                self.say("Key saved: Find on TONE3000 is ready");
            }
            "forgetkey" => {
                st.config.remove(KEY_T3K);
                st.save_config();
                self.say("Key forgotten here (sign out of tone3000.com itself in your browser)");
            }
            "find" => self.nam_find(st, arg.parse().unwrap_or(0)),
            "dlcancel" => crate::downloads::cancel(arg.parse().unwrap_or(0)),
            "dlretry" => crate::downloads::retry(arg.parse().unwrap_or(0)),
            "folder" => {
                if let Some(d) = captures_dir(st) {
                    crate::recording::open_dir(&d);
                }
            }
            "add" => {
                if staged {
                    return;
                }
                if let Some(path) = crate::dialogs::pick_capture_file() {
                    match captures::with(|l| l.add_path_as(&path, None)) {
                        Ok(it) => {
                            self.nam.note = Some(format!("Added {}", it.name));
                            self.nam.open = Some(it.id);
                            // (its level trim, measured as at start)
                            std::thread::spawn(|| crate::namcheck::calibrate(false));
                        }
                        Err(e) => self.nam.note = Some(format!("Not added: {e}")),
                    }
                } else if crate::dialogs::unavailable() {
                    self.say(crate::dialogs::NO_PICKER);
                }
            }
            "rescan" => {
                if staged {
                    return;
                }
                captures::reload();
                self.say(format!(
                    "Your captures read again: {}",
                    captures::with(|l| l.items.len())
                ));
            }
            "open" => {
                let id: u32 = arg.parse().unwrap_or(0);
                self.nam.open = if self.nam.open == Some(id) { None } else { Some(id) };
            }
            "askremove" => {
                let id: u32 = arg.parse().unwrap_or(0);
                let name = captures::name_of(id).unwrap_or_else(|| "this capture".into());
                self.confirm = Some(Confirm {
                    title: format!("Remove {name}?"),
                    text: "Its file is deleted from your captures folder. Rigs and songs that used it play quickRS's \
                           own model in its place; you can add it again from TONE3000 or the file."
                        .into(),
                    action: format!("nam:remove:{id}"),
                });
            }
            "remove" => {
                if staged {
                    return;
                }
                let id: u32 = arg.parse().unwrap_or(0);
                let name = captures::name_of(id).unwrap_or_default();
                captures::with(|l| l.remove(id));
                if self.nam.open == Some(id) {
                    self.nam.open = None;
                }
                self.say(format!("Removed {name}"));
            }
            "level" => {
                let v: f32 = arg.parse().unwrap_or(qrs_audio::board::nam::INPUT_DBU);
                self.nam_set_level(st, v);
            }
            "levelstep" => {
                let d: f32 = arg.parse().unwrap_or(0.0);
                let v = (input_dbu(&st.config) + d).round();
                self.nam_set_level(st, v);
            }
            "measure" => {
                if staged || self.nam.measuring.is_some() {
                    return;
                }
                let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let f = flag.clone();
                std::thread::spawn(move || {
                    let n = crate::namcheck::calibrate(true);
                    crate::applog::write(&format!("captures: {n} level trims measured again"));
                    f.store(true, std::sync::atomic::Ordering::Release);
                });
                self.nam.measuring = Some(flag);
            }
            _ => {}
        }
    }

    fn nam_set_level(&mut self, st: &mut AppState, v: f32) {
        let v = qrs_audio::board::nam::clamp_dbu(v);
        if self.nam.staged.is_none() {
            if (v - qrs_audio::board::nam::INPUT_DBU).abs() < 1e-4 {
                st.config.remove(KEY_DBU);
            } else {
                st.config.set(KEY_DBU, ((v as f64) * 100.0).round() / 100.0);
            }
            st.save_config();
            qrs_audio::board::nam::set_input_dbu(v);
        }
    }

    /// Find on TONE3000: the search words to the clipboard and TONE3000's own picker in the browser (your account).
    fn nam_find(&mut self, st: &AppState, i: usize) {
        if self.nam.staged.is_some() {
            return;
        }
        let (list, _, _) = super::setup::nam_suggestions(false, 8, st.instrument());
        let Some(sg) = list.get(i) else { return };
        let Some(key) = tone3000::client_id(&st.config) else {
            self.nam.note = Some("TONE3000 needs your key first: see TONE3000 on this page.".into());
            return;
        };
        st.ctx.copy_text(sg.unit.query.to_string());
        self.nam.pick = Some(crate::downloads::start(key, sg.unit.kind, Some(sg.unit)));
        self.nam.note = Some(format!(
            "TONE3000 is open in your browser: search \"{}\" (copied) and pick a capture; it downloads here.",
            sg.unit.query
        ));
    }

    /// The download started here, and a measuring run: what they say now (the card over the page shows the
    /// download itself, `crate::downloads`).
    fn nam_poll(&mut self, ctx: &egui::Context) {
        use tone3000::Phase;
        if let Some(id) = self.nam.pick {
            match crate::downloads::view(id).map(|v| v.phase) {
                Some(Phase::Done(it)) => {
                    self.nam.note = Some(format!("Added {} to your captures", it.name));
                    self.nam.open = Some(it.id);
                    self.nam.pick = None;
                }
                Some(Phase::Failed(_, e)) => {
                    self.nam.note = Some(format!("Didn't download: {e}"));
                    self.nam.pick = None;
                }
                Some(Phase::Cancelled { .. }) | None => {
                    self.nam.note = None;
                    self.nam.pick = None;
                }
                Some(_) => {}
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(f) = &self.nam.measuring
            && f.load(std::sync::atomic::Ordering::Acquire)
        {
            self.nam.measuring = None;
            self.say("Every capture's level measured again");
        }
    }

    // -- drawing ------------------------------------------------------------------------------------------------
    pub(super) fn draw_nam(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        self.nam_poll(ui.ctx());
        if self.nam_guided(st) {
            self.draw_nam_setup(ui, st, body)
        } else {
            self.draw_nam_manage(ui, st, body)
        }
    }

    /// The guided setup: the steps along the top, the step's card, Back / Next under it.
    fn draw_nam_setup(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let step = self.nam.step.unwrap_or(Step::About);
        let use_nam = self.nam_use(st);
        // the steps (the ones skipped with NAM off greyed)
        let mut x = body.min.x + px(4.0);
        let sy = body.min.y + px(12.0);
        for s in Step::ALL {
            let skipped = use_nam == Some(false) && matches!(s, Step::Account | Step::Suggest | Step::Files);
            let (w, c) = if s == step {
                (800, th.accent_ink)
            } else if s.index() < step.index() && !skipped {
                (700, th.text_2)
            } else {
                (600, th.caption)
            };
            let r = text::draw(
                &p,
                &format!("{}  {}", s.index() + 1, s.short()),
                pos2(x, sy),
                Align2::LEFT_CENTER,
                Style::new(13.0, w, c),
                None,
            );
            x = r.max.x + px(22.0);
        }
        let pad = px(26.0);
        let top = body.min.y + px(34.0);
        let x = body.min.x + pad;
        let w = body.width() - 2.0 * pad;
        // the card, as tall as its content was last frame (it follows the content a frame later)
        let card_h = self.nam.card_h.max(px(60.0));
        paint::card(
            &p,
            Rect::from_min_size(pos2(body.min.x, top), vec2(body.width(), card_h)),
            24.0,
            false,
        );
        let mut y = top + pad;
        y = text::caps(
            &p,
            &format!("STEP {} OF {}", step.index() + 1, Step::ALL.len()),
            pos2(x, y),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        )
        .max
        .y + px(10.0);
        y = text::draw(
            &p,
            step.title(),
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(22.0, 800, th.text),
            Some(w),
        )
        .max
        .y + px(12.0);
        for para in step_words(step, use_nam) {
            let n = text::wrap(&ctx, para, Style::new(14.0, 500, th.caption), w.min(px(860.0))).len();
            y = caption_lines(&p, para, x, y, w.min(px(860.0)), 14.0, n) + px(8.0);
        }
        y += px(8.0);
        y = match step {
            Step::Songs => self.nam_songs_choice(ui, st, x, y, w),
            Step::Account => self.nam_account(ui, st, x, y, w, false),
            Step::Suggest => self.nam_suggest_rows(ui, st, x, y, w, 8),
            Step::Files => self.nam_files_row(ui, st, x, y, w),
            Step::Done => self.nam_done_rows(st, &p, x, y, w),
            Step::About => y,
        };
        // Back / Skip / Next (Finish on the last)
        let bh = px(46.0);
        let by = y + px(14.0);
        let last = step == Step::Done;
        let next = if last { "Finish" } else { "Next" };
        let nw = widgets::button_width(&ctx, next, "", 15.0, false).max(px(130.0));
        let enabled = step != Step::Songs || use_nam.is_some();
        self.button(
            ui,
            Rect::from_min_size(pos2(x + w - nw, by), vec2(nw, bh)),
            next,
            "",
            if last { "nam:finish" } else { "nam:next" },
            ButtonStyle {
                size: 15.0,
                r: Some(14.0),
                primary: true,
                enabled,
                ..Default::default()
            },
        );
        if step != Step::About {
            let bw = widgets::button_width(&ctx, "Back", "", 15.0, false).max(px(110.0));
            self.button(
                ui,
                Rect::from_min_size(pos2(x + w - nw - px(12.0) - bw, by), vec2(bw, bh)),
                "Back",
                "",
                "nam:back",
                ButtonStyle {
                    size: 15.0,
                    r: Some(14.0),
                    plain: true,
                    ..Default::default()
                },
            );
        }
        if let Some(n) = &self.nam.note
            && matches!(step, Step::Suggest | Step::Files | Step::Account)
        {
            text::draw(
                &p,
                n,
                pos2(x, by + bh / 2.0),
                Align2::LEFT_CENTER,
                Style::new(13.0, 600, th.text_2),
                Some(w * 0.6),
            );
        }
        let bottom = by + bh + pad;
        if (bottom - top - self.nam.card_h).abs() > 0.5 {
            self.nam.card_h = bottom - top;
            ctx.request_repaint();
        }
        bottom.max(top + card_h)
    }

    /// Use NAM for songs?: No / Yes, the two keys.
    fn nam_songs_choice(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32) -> f32 {
        let labels = ["No: quickRS's own amps", "Yes: captures where I have them"];
        let seg = Rect::from_min_size(pos2(x, y), vec2(px(620.0).min(w), px(46.0)));
        let mut so = widgets::Segmented::new(&labels, self.nam_use(st).map(|on| on as usize));
        so.size = 14.0;
        so.pad = 4.0;
        so.r = 14.0;
        so.inner_r = 10.0;
        let (_, clicked) = widgets::segmented(ui, seg, "set-nam-songs", &so);
        if let Some(i) = clicked {
            self.acts
                .push(if i == 1 { "nam:songs:yes" } else { "nam:songs:no" }.into());
        }
        let words = match self.nam_use(st) {
            Some(true) => {
                "Songs play on your captures where you have one for their gear, and on quickRS's models for \
                           the rest. Next: your TONE3000 account."
            }
            Some(false) => {
                "Every song plays on quickRS's own amps, cabs and pedals. You can turn NAM on here any time; \
                            the amp room has MODELLED or NAM on each unit either way."
            }
            None => "Not sure? No is fine: quickRS's models play every song, and nothing is lost.",
        };
        caption_lines(ui.painter(), words, x + px(4.0), seg.max.y + px(14.0), w, 13.0, 2) + px(4.0)
    }

    /// The TONE3000 key: saved (Forget key) or a box to paste it in (Save key), and Open tone3000.com.
    fn nam_account(&mut self, ui: &mut Ui, st: &AppState, x: f32, y: f32, w: f32, compact: bool) -> f32 {
        let th = t();
        let ctx = ui.ctx().clone();
        let bh = px(42.0);
        let style = ButtonStyle {
            size: 13.0,
            r: Some(12.0),
            ..Default::default()
        };
        let key = if self.nam.staged.is_some() {
            compact.then(|| "t3k_pub_0000000000a1b2".to_string())
        } else {
            tone3000::client_id(&st.config)
        };
        let mut y = y;
        match key {
            Some(k) => {
                let r = text::draw(
                    ui.painter(),
                    &format!("Key saved: {}", key_hint(&k)),
                    pos2(x, y + bh / 2.0),
                    Align2::LEFT_CENTER,
                    Style::new(15.0, 700, th.text),
                    Some(w * 0.5),
                );
                let fw = widgets::button_width(&ctx, "Forget key", "", 13.0, false);
                self.button(
                    ui,
                    Rect::from_min_size(pos2(r.max.x + px(18.0), y), vec2(fw, bh)),
                    "Forget key",
                    "",
                    "nam:forgetkey",
                    style,
                );
                y += bh + px(8.0);
                y = caption_lines(
                    ui.painter(),
                    "You sign in on TONE3000's own page when you pick a capture; the site remembers you. quickRS \
                     keeps no password or sign-in, only this public key. To sign out, sign out on tone3000.com.",
                    x,
                    y,
                    w,
                    13.0,
                    3,
                );
            }
            None => {
                let ow = widgets::button_width(&ctx, "Open tone3000.com", "", 13.0, false);
                let sw = widgets::button_width(&ctx, "Save key", "", 13.0, false);
                let fw = (w - ow - sw - px(24.0)).clamp(px(200.0), px(420.0));
                let fr = Rect::from_min_size(pos2(x, y), vec2(fw, bh));
                text_box(ui, fr, "t3k", &mut self.nam.key_in, "t3k_pub_\u{2026}");
                let ok = self.nam.key_in.trim().starts_with("t3k_pub_");
                self.button(
                    ui,
                    Rect::from_min_size(pos2(fr.max.x + px(12.0), y), vec2(sw, bh)),
                    "Save key",
                    "",
                    "nam:savekey",
                    ButtonStyle { enabled: ok, ..style },
                );
                self.button(
                    ui,
                    Rect::from_min_size(pos2(fr.max.x + sw + px(24.0), y), vec2(ow, bh)),
                    "Open tone3000.com",
                    "",
                    "nam:site",
                    ButtonStyle { plain: true, ..style },
                );
                y += bh + px(10.0);
            }
        }
        // the free tier's credit
        text::draw(
            ui.painter(),
            "Powered by TONE3000",
            pos2(x, y + px(4.0)),
            Align2::LEFT_TOP,
            Style::new(12.0, 700, th.caption),
            None,
        )
        .max
        .y + px(6.0)
    }

    /// The suggested captures (the gear your songs use most), each with Find on TONE3000.
    fn nam_suggest_rows(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32, limit: usize) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let staged = self.nam.staged.is_some();
        let (list, covered, total) = super::setup::nam_suggestions(staged, limit, st.instrument());
        let mut y = y;
        if list.is_empty() {
            let words = if crate::gearindex::reading().is_some() {
                "Reading your songs' gear\u{2026}"
            } else {
                "No songs with RS tones yet: add your songs in Settings › Library, then look here again."
            };
            return text::draw(
                &p,
                words,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(14.0, 600, th.text_2),
                Some(w),
            )
            .max
            .y + px(8.0);
        }
        text::draw(
            &p,
            &format!(
                "These cover {covered} of your {total} song{}",
                if total == 1 { "" } else { "s" }
            ),
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(13.0, 700, th.text_2),
            None,
        );
        y += px(26.0);
        let row_h = px(42.0);
        for (i, sg) in list.iter().enumerate() {
            let r = Rect::from_min_size(pos2(x, y), vec2(w, row_h));
            if i > 0 {
                p.hline(r.min.x..=r.max.x, r.min.y, (1.0, th.divider));
            }
            let cy = centre_y(r);
            let kind = match sg.unit.kind {
                Kind::Nam => "amp",
                Kind::Pedal => "pedal",
                Kind::Ir => "cab (IR)",
            };
            let nr = text::draw(
                &p,
                sg.unit.name,
                pos2(r.min.x + px(4.0), cy),
                Align2::LEFT_CENTER,
                Style::new(15.0, 700, th.text),
                Some(r.width() * 0.45),
            );
            text::draw(
                &p,
                &format!("{kind}{}", if sg.unit.bass { ", bass" } else { "" }),
                pos2(nr.max.x + px(12.0), cy),
                Align2::LEFT_CENTER,
                Style::new(12.0, 600, th.caption),
                None,
            );
            let bw = px(150.0);
            let br = Rect::from_min_size(pos2(r.max.x - bw, cy - px(15.0)), vec2(bw, px(30.0)));
            match crate::downloads::suggestion_tail(ui, ("nam-sg", i), br, sg.unit, sg.have, sg.songs) {
                Some(crate::downloads::RowAct::Find) => self.acts.push(format!("nam:find:{i}")),
                Some(crate::downloads::RowAct::Cancel(id)) => self.acts.push(format!("nam:dlcancel:{id}")),
                Some(crate::downloads::RowAct::Retry(id)) => self.acts.push(format!("nam:dlretry:{id}")),
                None => {}
            }
            y += row_h;
        }
        y + px(4.0)
    }

    /// The captures folder, Open it and Add a .nam or IR file….
    fn nam_files_row(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let ctx = ui.ctx().clone();
        let n = captures::with(|l| l.items.len());
        let dir = captures_dir(st)
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "(not open yet)".into());
        let r = text::draw(
            ui.painter(),
            &format!("{n} capture{} in {dir}", if n == 1 { "" } else { "s" }),
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(14.0, 700, th.text),
            Some(w),
        );
        let y = r.max.y + px(14.0);
        let bh = px(42.0);
        let mut bx = x;
        for (label, action) in [
            ("Add a .nam or IR file\u{2026}", "nam:add"),
            ("Open the folder", "nam:folder"),
        ] {
            let bw = widgets::button_width(&ctx, label, "", 13.0, false);
            self.button(
                ui,
                Rect::from_min_size(pos2(bx, y), vec2(bw, bh)),
                label,
                "",
                action,
                ButtonStyle {
                    size: 13.0,
                    r: Some(12.0),
                    ..Default::default()
                },
            );
            bx += bw + px(12.0);
        }
        y + bh + px(4.0)
    }

    /// Done: what's set.
    fn nam_done_rows(&self, st: &AppState, p: &egui::Painter, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let use_nam = self.nam_use(st);
        let key = self.nam.staged.is_some() || tone3000::client_id(&st.config).is_some();
        let n = captures::with(|l| l.items.len());
        let rows: Vec<(&str, String)> = vec![
            (
                "Songs",
                match use_nam {
                    Some(true) => "your captures where you have them, quickRS's models for the rest".into(),
                    _ => "quickRS's own models".into(),
                },
            ),
            (
                "TONE3000",
                if key {
                    "your key is saved".into()
                } else {
                    "no key yet (this page, TONE3000)".into()
                },
            ),
            ("Captures", format!("{n} in your library")),
            (
                "Input level",
                level_label(if self.nam.staged.is_some() {
                    qrs_audio::board::nam::INPUT_DBU
                } else {
                    input_dbu(&st.config)
                }),
            ),
        ];
        let mut y = y;
        for (k, v) in rows {
            text::draw(
                p,
                k,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(14.0, 700, th.text_2),
                None,
            );
            y = text::draw(
                p,
                &v,
                pos2(x + px(150.0), y),
                Align2::LEFT_TOP,
                Style::new(14.0, 600, th.text),
                Some(w - px(150.0)),
            )
            .max
            .y + px(8.0);
        }
        y
    }

    /// The management page: songs, TONE3000, your captures, the suggestions, the input level, the setup again.
    fn draw_nam_manage(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (gap, pad) = (px(24.0), px(22.0));
        let colw = super::snap((body.width() - gap) / 2.0);
        let wd = colw - 2.0 * pad;
        let full = body.width() - 2.0 * pad;
        let hh = Self::h_heading(&ctx);
        let staged = self.nam.staged.is_some();
        // songs on NAM, and the TONE3000 key: side by side
        let h1 = 2.0 * pad + hh + Self::h_switch("md", true) + px(14.0) + px(19.0) * 2.0;
        let h2 = 2.0 * pad + hh + px(42.0) + px(8.0) + px(19.0) * 3.0 + px(30.0);
        let h = h1.max(h2);
        let c1 = Rect::from_min_size(body.min, vec2(colw, h));
        let c2 = Rect::from_min_size(pos2(body.min.x + colw + gap, body.min.y), vec2(colw, h));
        paint::card(&p, c1, 24.0, false);
        paint::card(&p, c2, 24.0, false);
        let x = c1.min.x + pad;
        let mut y = self.heading(&p, "NEURAL AMP MODELER", x, c1.min.y + pad, None, "", "");
        let on = if staged { true } else { captures::songs_use_nam() };
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "Use NAM for songs",
            "songs play on your captures where you have them",
            on,
            "nam:toggle",
            "md",
            "",
            700,
        ) + px(14.0);
        caption_lines(
            &p,
            "Off: every song plays on quickRS's own models. The amp room has MODELLED or NAM on each unit either way.",
            x,
            y,
            wd,
            13.0,
            2,
        );
        let x2 = c2.min.x + pad;
        let y2 = self.heading(&p, "TONE3000", x2, c2.min.y + pad, None, "your own account", "");
        self.nam_account(ui, st, x2, y2, wd, true);
        // your captures
        let top = c1.max.y.max(c2.max.y) + gap;
        let y = self.nam_library_card(ui, st, body, top, pad, full);
        // the suggestions
        let top = y + gap;
        let list_h = {
            let (list, _, _) = super::setup::nam_suggestions(staged, 6, st.instrument());
            px(26.0) + list.len().max(1) as f32 * px(42.0) + px(8.0)
        };
        let card = Rect::from_min_size(
            pos2(body.min.x, top),
            vec2(body.width(), 2.0 * pad + hh + list_h + px(26.0)),
        );
        paint::card(&p, card, 24.0, false);
        let x = card.min.x + pad;
        let yy = self.heading(
            &p,
            "SUGGESTED FOR YOUR SONGS",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            "the gear your songs use most",
            "",
        );
        let yy = self.nam_suggest_rows(ui, st, x, yy, full, 6);
        let note = self.nam.note.clone().unwrap_or_else(|| {
            "Each opens TONE3000 in your browser: pick a capture there and it downloads into your captures.".into()
        });
        caption_lines(&p, &note, x, yy + px(2.0), full, 13.0, 1);
        // the input level, and level matching / the setup again
        let top = card.max.y + gap;
        let dbu = if staged {
            qrs_audio::board::nam::INPUT_DBU
        } else {
            input_dbu(&st.config)
        };
        let lvl_words = "A capture is trained at the level its gear was fed. quickRS takes 0 dBFS on your input as this \
                         level to drive it as hard as the real amp would be. Most interfaces: +12 dBu. Set it with \
                         your interface's gain knob at minimum: that's where the figure is true. Captures too \
                         clean: raise it; too dirty: lower it.";
        let lw = text::wrap(&ctx, lvl_words, Style::new(13.0, 500, th.caption), wd).len();
        let h = 2.0 * pad + hh + lw as f32 * px(19.0) + px(14.0) + px(46.0) + px(12.0) + px(46.0) + px(24.0);
        let c1 = Rect::from_min_size(pos2(body.min.x, top), vec2(colw, h));
        let c2 = Rect::from_min_size(pos2(body.min.x + colw + gap, top), vec2(colw, h));
        paint::card(&p, c1, 24.0, false);
        paint::card(&p, c2, 24.0, false);
        let x = c1.min.x + pad;
        let y = self.heading(
            &p,
            "INTERFACE REFERENCE LEVEL",
            x,
            c1.min.y + pad,
            Some(c1.max.x - pad),
            "set with the interface gain at minimum",
            "",
        );
        let y = caption_lines(&p, lvl_words, x, y, wd, 13.0, lw) + px(14.0);
        let mut labels: Vec<&str> = LEVELS.iter().map(|l| l.0).collect();
        labels.push("Custom");
        let sel = level_preset(dbu).unwrap_or(LEVELS.len());
        let seg = Rect::from_min_size(pos2(x, y), vec2(wd, px(46.0)));
        let mut so = widgets::Segmented::new(&labels, Some(sel));
        so.size = 13.0;
        so.pad = 4.0;
        so.r = 14.0;
        so.inner_r = 10.0;
        let (_, clicked) = widgets::segmented(ui, seg, "set-nam-level", &so);
        if let Some(i) = clicked {
            let v = LEVELS.get(i).map(|l| l.2).unwrap_or((dbu + 1.0).round());
            self.acts.push(format!("nam:level:{v}"));
        }
        let y = seg.max.y + px(12.0);
        self.stepper(
            ui,
            x,
            y,
            wd,
            ("\u{2212} 1 dB", "", "nam:levelstep:-1"),
            &level_label(dbu),
            ("+ 1 dB", "", "nam:levelstep:1"),
        );
        caption_lines(
            &p,
            match level_preset(dbu) {
                Some(i) => LEVELS[i].1,
                None => "custom: what your interface's manual gives as its maximum input level",
            },
            x,
            y + px(46.0) + px(6.0),
            wd,
            12.0,
            1,
        );
        // level matching and the setup again
        let x2 = c2.min.x + pad;
        let (n, trimmed) =
            captures::with(|l| (l.items.len(), l.items.iter().filter(|i| i.trim.is_some()).count()));
        let y2 = self.heading(&p, "LEVEL MATCHING", x2, c2.min.y + pad, None, "", "");
        let y2 = caption_lines(
            &p,
            &format!(
                "Each capture is measured once against the quickRS model it stands in for, so switching MODELLED and \
                 NAM keeps the level, and a gain ladder's steps sit together. {trimmed} of {n} measured."
            ),
            x2,
            y2,
            wd,
            13.0,
            3,
        ) + px(12.0);
        let measuring = self.nam.measuring.is_some();
        let mw = widgets::button_width(&ctx, "Measure every capture again", "", 13.0, false);
        self.button(
            ui,
            Rect::from_min_size(pos2(x2, y2), vec2(mw, px(42.0))),
            if measuring {
                "Measuring\u{2026}"
            } else {
                "Measure every capture again"
            },
            "",
            "nam:measure",
            ButtonStyle {
                size: 13.0,
                r: Some(12.0),
                enabled: !measuring && n > 0,
                ..Default::default()
            },
        );
        let y2 = y2 + px(42.0) + px(22.0);
        let y2 = self.heading(&p, "THE NAM SETUP", x2, y2, None, "", "");
        let rw = widgets::button_width(&ctx, "Run the NAM setup again", "", 13.0, false);
        self.button(
            ui,
            Rect::from_min_size(pos2(x2, y2), vec2(rw, px(42.0))),
            "Run the NAM setup again",
            "",
            "nam:rerun",
            ButtonStyle {
                size: 13.0,
                r: Some(12.0),
                plain: true,
                ..Default::default()
            },
        );
        c1.max.y.max(c2.max.y)
    }

    /// Your captures: how many and where, Add / Open the folder / Read again, the newest (or all) with their details.
    fn nam_library_card(
        &mut self,
        ui: &mut Ui,
        st: &AppState,
        body: Rect,
        top: f32,
        pad: f32,
        full: f32,
    ) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let all = self.folds.is_open("nam_all");
        let mut items: Vec<captures::Item> = captures::with(|l| l.items.clone());
        items.sort_by(|a, b| b.added.total_cmp(&a.added));
        let n = items.len();
        let shown = if all { n } else { n.min(8) };
        let row_h = px(40.0);
        let detail_h = px(19.0) * 4.0 + px(12.0);
        let open = self
            .nam
            .open
            .filter(|id| items.iter().take(shown).any(|i| i.id == *id));
        let list_h = shown.max(1) as f32 * row_h + if open.is_some() { detail_h } else { 0.0 };
        let more_h = if n > 8 { px(44.0) } else { 0.0 };
        let card = Rect::from_min_size(
            pos2(body.min.x, top),
            vec2(
                body.width(),
                2.0 * pad + Self::h_heading(&ctx) + px(26.0) + px(42.0) + px(14.0) + list_h + more_h,
            ),
        );
        paint::card(&p, card, 24.0, false);
        let x = card.min.x + pad;
        let y = self.heading(
            &p,
            &format!("YOUR CAPTURES ({n})"),
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            "amps, pedals and cab IRs",
            "",
        );
        let dir = captures_dir(st)
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "(not open yet)".into());
        text::draw(
            &p,
            &format!("In {dir}"),
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(13.0, 600, th.text_2),
            Some(full),
        );
        let y = y + px(26.0);
        let mut bx = x;
        for (label, action) in [
            ("Add a .nam or IR file\u{2026}", "nam:add"),
            ("Open the folder", "nam:folder"),
            ("Read again", "nam:rescan"),
        ] {
            let bw = widgets::button_width(&ctx, label, "", 13.0, false);
            self.button(
                ui,
                Rect::from_min_size(pos2(bx, y), vec2(bw, px(42.0))),
                label,
                "",
                action,
                ButtonStyle {
                    size: 13.0,
                    r: Some(12.0),
                    ..Default::default()
                },
            );
            bx += bw + px(12.0);
        }
        let mut y = y + px(42.0) + px(14.0);
        if n == 0 {
            caption_lines(
                &p,
                "None yet: Find on TONE3000 below, or add a .nam (an amp or pedal) or a .wav (a cab IR) you have.",
                x,
                y + px(8.0),
                full,
                13.0,
                1,
            );
        }
        for it in items.iter().take(shown) {
            let r = Rect::from_min_size(pos2(x, y), vec2(full, row_h));
            p.hline(r.min.x..=r.max.x, r.min.y, (1.0, th.divider));
            let cy = centre_y(r);
            let kind = match it.kind {
                Kind::Nam => "amp",
                Kind::Pedal => "pedal",
                Kind::Ir => "cab IR",
            };
            let nr = text::draw(
                &p,
                &it.name,
                pos2(r.min.x + px(4.0), cy),
                Align2::LEFT_CENTER,
                Style::new(14.0, 700, th.text),
                Some(full * 0.5),
            );
            text::draw(
                &p,
                &format!("{kind}{}", if it.bass { ", bass" } else { "" }),
                pos2(nr.max.x + px(12.0), cy),
                Align2::LEFT_CENTER,
                Style::new(12.0, 600, th.caption),
                None,
            );
            let rw = widgets::button_width(&ctx, "Remove", "", 12.0, false);
            let dw = widgets::button_width(&ctx, "Details", "", 12.0, false) + px(18.0);
            let style = ButtonStyle {
                size: 12.0,
                r: Some(11.0),
                plain: true,
                ..Default::default()
            };
            self.button(
                ui,
                Rect::from_min_size(pos2(r.max.x - rw, cy - px(15.0)), vec2(rw, px(30.0))),
                "Remove",
                "",
                &format!("nam:askremove:{}", it.id),
                style,
            );
            self.button(
                ui,
                Rect::from_min_size(
                    pos2(r.max.x - rw - px(8.0) - dw, cy - px(15.0)),
                    vec2(dw, px(30.0)),
                ),
                "Details",
                "",
                &format!("nam:open:{}", it.id),
                ButtonStyle {
                    active: open == Some(it.id),
                    ..style
                },
            );
            y += row_h;
            if open == Some(it.id) {
                let lines = capture_details(it);
                let mut ly = y + px(2.0);
                for l in lines.iter().take(4) {
                    ly = text::draw(
                        &p,
                        l,
                        pos2(x + px(20.0), ly),
                        Align2::LEFT_TOP,
                        Style::new(13.0, 600, th.text_2),
                        Some(full - px(24.0)),
                    )
                    .max
                    .y + px(3.0);
                }
                y += detail_h;
            }
        }
        if n > 8 {
            let label = if all {
                "Show the newest 8".to_string()
            } else {
                format!("Show all {n}")
            };
            let bw = widgets::button_width(&ctx, &label, "", 13.0, false);
            self.button(
                ui,
                Rect::from_min_size(pos2(x, y + px(8.0)), vec2(bw, px(36.0))),
                &label,
                "",
                "fold:nam_all",
                ButtonStyle {
                    size: 13.0,
                    r: Some(12.0),
                    plain: true,
                    ..Default::default()
                },
            );
        }
        card.max.y
    }
}

/// The guided setup's words for a step, a paragraph each.
fn step_words(step: Step, use_nam: Option<bool>) -> &'static [&'static str] {
    match step {
        Step::About => &[
            "Neural Amp Modeler (NAM) is a free, open way to capture real gear. Someone plays test sounds through an \
             amp or a pedal, records what comes out, and trains a small neural network on it. The file it makes (a \
             .nam) plays like that amp did, knobs where they were set.",
            "quickRS can play those files in place of its own models: for songs (the gear their RS tones use) and in \
             the amp room. People share captures on TONE3000, a free site; you pick them through your own free \
             account there. quickRS never downloads or ships any for you.",
            "Each capture takes about a fifth of a CPU core. quickRS's own amps, cabs and pedals play every song \
             without any.",
        ],
        Step::Songs => &[
            "When a song starts, its tone is built from the gear it names. Captures can stand in for that \
                          gear where you have one.",
        ],
        Step::Account => &[
            "quickRS asks TONE3000 for captures with a key from your own account. On tone3000.com: sign in (free), \
             open Settings › API Keys, create one, and copy its publishable key (it starts t3k_pub_). It's a public \
             id, not a password.",
            "When you pick a capture, TONE3000's own page opens in your browser and you sign in there. quickRS never \
             sees your password and keeps no sign-in.",
        ],
        Step::Suggest => &[
            "The gear your songs use most, ranked by how many songs it covers. Find on TONE3000 opens \
                            the search for it (the words are copied): pick a capture you like and it downloads into \
                            your captures. Songs use the ones you have; the rest stay on quickRS's models.",
        ],
        Step::Files => &[
            "Captures live in a folder of quickRS's own, with captures.json listing them. You can also \
                          add a .nam you have (an amp or pedal), or a .wav impulse response for a cab. They show in \
                          the amp room's Catalogue and stand in for songs' gear.",
        ],
        Step::Done if use_nam == Some(false) => &[
            "Songs play on quickRS's own models. This page keeps everything NAM: \
                                                   turn it on, add captures or change the input level any time.",
        ],
        Step::Done => &[
            "Everything here stays on this page to change later: NAM for songs, your TONE3000 key, your \
                         captures and their details, and the input level they're driven at.",
        ],
    }
}

/// A capture's details, a line each: what it is (architecture, rate, size), what it was captured from, what it
/// sounds like of ours, its level.
pub fn capture_details(it: &captures::Item) -> Vec<String> {
    let info = qrs_audio::board::nam::capture_info(it.id);
    let mut out = Vec::new();
    let mut file: Vec<String> = Vec::new();
    match (&info, it.kind) {
        (Some(i), _) => {
            file.push(format!("Architecture {}", arch_label(&i.architecture)));
            if i.rate > 0.0 {
                file.push(format!("{} kHz", (i.rate / 100.0).round() / 10.0));
            }
        }
        (None, Kind::Ir) => file.push("Impulse response (a cab)".into()),
        (None, _) => file.push("Not loaded (the file is missing or can't run here)".into()),
    }
    if let Some(s) = &it.size {
        file.push(format!("{s} size"));
    }
    if it.kind != Kind::Ir {
        match it.cab.or(info.as_ref().and_then(|i| i.includes_cab)) {
            Some(true) => file.push("cab included".into()),
            Some(false) => file.push("no cab: plays into a cab".into()),
            None => {}
        }
    }
    out.push(file.join(" \u{b7} "));
    let mut from: Vec<String> = Vec::new();
    if let Some(g) = it.gear.clone().or(info.as_ref().and_then(|i| i.gear.clone())) {
        from.push(format!("Captured from {g}"));
    }
    if let Some(c) = it
        .creator
        .clone()
        .or(info.as_ref().and_then(|i| i.modeled_by.clone()))
    {
        from.push(format!("by {c}"));
    }
    if it.source == "tone3000" {
        from.push(match &it.licence {
            Some(l) => format!("TONE3000 \u{b7} {}", captures::licence_name(l)),
            None => "TONE3000".into(),
        });
    }
    if from.is_empty() {
        from.push("Added from a file".into());
    }
    out.push(from.join(" \u{b7} "));
    let like = it
        .skin
        .as_deref()
        .and_then(qrs_audio::board::catalog::find)
        .map(|s| format!("Sounds like our {}", qrs_audio::board::MODELS[s].name))
        .unwrap_or_else(|| "Sounds like: no model of ours matched".into());
    out.push(like);
    out.push(match it.trim {
        Some(tr) => format!("Level matched: {tr:+.1} dB against our model"),
        None => "Level not measured yet".into(),
    });
    out
}

/// The captures folder (None until the library has a place).
fn captures_dir(st: &AppState) -> Option<std::path::PathBuf> {
    Some(st.data.root.join("captures"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(name: &str, json: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("qrs-nampage-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("config.json");
        std::fs::write(&p, json).unwrap();
        Config::load(&p)
    }

    #[test]
    fn set_up_or_not() {
        // never asked: the page's own setup
        assert!(!is_set_up(&cfg("new", "{}")));
        // the first-time setup's no: not set up (the page offers the setup)
        assert!(!is_set_up(&cfg(
            "no",
            r#"{"nam_setup": "no", "nam_songs": false}"#
        )));
        // its yes: set up
        assert!(is_set_up(&cfg("yes", r#"{"nam_setup": "yes"}"#)));
        // this page's setup finished, with a no too: the management page
        assert!(is_set_up(&cfg(
            "done",
            r#"{"nam_setup": "no", "nam_page": "done"}"#
        )));
    }

    #[test]
    fn the_setups_steps() {
        assert_eq!(Step::About.next(None), Step::Songs);
        assert_eq!(Step::Songs.next(Some(true)), Step::Account);
        assert_eq!(
            Step::Songs.next(Some(false)),
            Step::Done,
            "no NAM: nothing to fetch"
        );
        assert_eq!(Step::Done.back(Some(false)), Step::Songs);
        assert_eq!(Step::Done.back(Some(true)), Step::Files);
        assert_eq!(Step::Done.next(Some(true)), Step::Done);
        assert_eq!(Step::About.back(None), Step::About);
        for s in Step::ALL {
            assert_eq!(Step::parse(s.key()), Some(s));
            assert!(!step_words(s, Some(true)).is_empty());
        }
    }

    #[test]
    fn the_reference_level() {
        assert_eq!(
            input_dbu(&cfg("dbu0", "{}")),
            12.0,
            "today's behaviour by default"
        );
        assert_eq!(input_dbu(&cfg("dbu4", r#"{"nam_input_dbu": 4}"#)), 4.0);
        assert_eq!(input_dbu(&cfg("dbu99", r#"{"nam_input_dbu": 99}"#)), 30.0);
        assert_eq!(level_preset(12.0), Some(0));
        assert_eq!(level_preset(-7.78), Some(2));
        assert_eq!(level_preset(9.0), None);
        assert_eq!(level_label(-7.78), "\u{2212}10 dBV");
        assert_eq!(level_label(9.0), "+9 dBu");
        assert_eq!(level_label(-3.0), "\u{2212}3 dBu");
        // the setting's effect on a capture trained at +12 dBu: the consumer level drives it ~19.8 dB softer
        use qrs_audio::board::nam::input_gain;
        let g = |dbu: f32| 20.0 * input_gain(dbu, Some(12.0)).log10();
        assert!(g(input_dbu(&cfg("dbu12", "{}"))).abs() < 1e-4);
        assert!((g(LEVELS[1].2) + 8.0).abs() < 1e-4);
        assert!((g(LEVELS[2].2) + 19.78).abs() < 1e-3);
    }

    #[test]
    fn words_for_people() {
        assert_eq!(arch_label("SlimmableContainer"), "A2");
        assert_eq!(arch_label("WaveNet"), "A1 (WaveNet)");
        assert_eq!(key_hint("t3k_pub_abcdef1234"), "t3k_pub_\u{2026}1234");
        // RS, not the full name, in what the page says (D138)
        for s in Step::ALL {
            for w in step_words(s, Some(true)).iter().chain(step_words(s, Some(false))) {
                assert!(!w.contains("Rocksmith"), "{w}");
            }
        }
    }
}
