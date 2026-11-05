//! The first-time setup's panel (P23.14, owner request; new code, no Python source): a modal panel over the app,
//! the size of Settings', with the steps along its top (Look, Low latency, Your instrument (Your bass once Bass is
//! picked), Use NAM?, Note widget, Folders, Done), the page's words and controls on the left, a picture of what it's about on the right, and Close / Back /
//! Skip / Next at the bottom (Esc, Backspace, S, Enter). `setup.rs` has the pages and their logic.
//!
//! It runs on Settings' own pieces, held in a `SettingsView` of its own: the guitar page runs the guided guitar
//! setup (`guide.rs`) in the panel, the folders page the Directories page's actions and key boxes, the switches
//! the Audio page's actions, so everything is saved exactly as Settings saves it.

use std::time::Duration;

use eframe::egui;
use egui::{Align2, CursorIcon, Event, Key, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{self, ThemeName, t};
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle, interact};

use super::guide::GuideExit;
use super::library::{key_box_id, text_box};
use super::{SettingsView, centre_y, line_h, snap};
use crate::calibrate;
use crate::guitar_setup::{Mark, Step};
use crate::keyfile::{self, KeyState};
use crate::player::PlayerView;
use crate::setup::{self as fs, Flow, Page, Pic};
use crate::state::AppState;

/// The first-time setup under way.
pub struct FirstSetup {
    pub flow: Flow,
    /// Settings' pieces: the guided guitar setup, the key boxes, the folder and device actions, their messages.
    view: SettingsView,
    /// Set when the panel should close (the app drops it).
    pub closed: bool,
    /// Set with `closed`: Done's "Go to the amp room".
    pub room: bool,
    /// Screenshots: a page made up as it would look (nothing is saved).
    staged: bool,
    acts: Vec<String>,
    /// Use NAM? (P36.7): the answer while staged (screenshots save nothing), the TONE3000 pick under way (the
    /// suggestion's row) and what it last said.
    nam_staged: Option<bool>,
    /// The download this page started last (`crate::downloads`).
    nam_pick: Option<u64>,
    nam_note: Option<String>,
}

/// The themes' words on the look page.
fn theme_words(t: ThemeName) -> &'static str {
    match t {
        ThemeName::Midnight => "Dark, deep blue. The default.",
        ThemeName::Graphite => "Dark grey: a softer dark.",
        ThemeName::Daylight => "Light, for a bright room.",
    }
}

/// A page's words under its heading, a paragraph each, for what you play (`inst`) and on an output that's ASIO
/// already (`asio`: the low-latency page has nothing to set then, P37.6a).
fn intro(page: Page, inst: qrs_analysis::instrument::Instrument, asio: bool) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let n = inst.noun();
    match page {
        Page::Look => v(&["Three looks, the same app. Pick one and the whole app changes as you pick."]),
        Page::Guitar => vec![
            format!(
                "quickRS listens to your {n}: it scores your playing and plays it through its amps and pedals. A \
                 few short steps set it up: the input and its channel, where you hear it, a level check, what's \
                 plugged in, the level it sets for you, the open strings and your timing."
            ),
            format!("About two minutes. Have your {n} plugged in and tuned."),
        ],
        Page::Latency if asio => v(&[
            "Your sound goes out through ASIO, which already talks to your sound device directly: there's nothing \
             to turn on here.",
            "Press Next: quickRS stays on ASIO.",
        ]),
        Page::Latency => v(&[
            "Low-latency output sends the sound straight to your sound device instead of through Windows' sound \
             mixer, so what you play comes back sooner.",
            "While it's on and quickRS is playing, other programs can't use that device: they go quiet. You can \
             turn it off any time.",
            "Without it quickRS is still good. Turn it on for focused practice or for recording.",
        ]),
        Page::Widget => v(&[
            "A small window that stays on top of everything: the note you're playing, the key you seem to be in, \
             and that key's scale over the whole neck.",
            "Great with two screens, or if you don't mind it floating over your one screen. Drag it where you \
             like; right-click it for its menu.",
        ]),
        Page::Folders => v(&[
            "Where your songs are, RS's keys if you have psarc files, and where recordings go. Songs are \
             read as soon as a folder is added.",
        ]),
        Page::Nam => v(&[
            "quickRS's own amps, cabs and pedals are all built in. You can also play captures of real gear made with \
             Neural Amp Modeler (NAM), from TONE3000, with your own free account there.",
            "Yes lists the gear your songs use most; each opens on TONE3000 for you to pick and download. No: every \
             song plays on quickRS's own models, and nothing is lost.",
        ]),
        Page::Done => v(&["That's it. Here's what's set; all of it is in Settings."]),
    }
}

/// Whether the output plays through ASIO now (the low-latency page has nothing to set then).
fn asio_out(st: &AppState) -> bool {
    st.audio
        .as_ref()
        .and_then(|a| a.engine.status().output)
        .is_some_and(|o| o.mode == "ASIO")
}

/// What the picture beside a page shows, under it.
fn pic_caption(pic: Pic) -> &'static str {
    match pic {
        Pic::Guitar => "Calibration's settings, in Settings > Sound",
        Pic::Latency => "Where it lives: Settings > Timing > Reduce delay",
        Pic::Widget => "The note widget, over whatever else is on your screen",
        Pic::Folders => "Settings > Library: your song folders",
        Pic::Room => "The amp room: your rig of amps and pedals, no song needed",
        Pic::Theme(_) => "",
    }
}

impl FirstSetup {
    /// The setup from its first page (a first run, Settings' button or Home's Getting started), on the current
    /// settings: nothing changes until you change it.
    pub fn new() -> FirstSetup {
        FirstSetup {
            flow: Flow::default(),
            view: SettingsView::new("sound"),
            closed: false,
            room: false,
            staged: false,
            acts: Vec::new(),
            nam_staged: None,
            nam_pick: None,
            nam_note: None,
        }
    }

    /// The setup opened on a page (Settings > Tone's Suggested captures opens it on Use NAM?).
    pub fn at(page: &str) -> FirstSetup {
        let mut s = FirstSetup::new();
        if let Some(p) = Page::parse(page) {
            s.flow.go(p);
        }
        s
    }

    /// Screenshots: a page ("look", "guitar", "guitar_<step>" for a step of the guitar setup in it, "latency",
    /// "widget", "folders", "done") made up as it would look.
    pub fn staged(page: &str) -> FirstSetup {
        let mut s = FirstSetup::new();
        s.staged = true;
        let (p, step) = match page.split_once('_') {
            Some((p, step)) => (p, Step::parse(step)),
            None => (page, None),
        };
        let p = Page::parse(p).unwrap_or(Page::Look);
        // (the pages before it shown as done)
        let k = s.flow.pages().iter().position(|q| *q == p).unwrap_or(0);
        for _ in 0..k {
            s.flow.next();
        }
        if p == Page::Nam {
            // ("nam_yes": the suggestions shown)
            s.nam_staged = (page == "nam_yes").then_some(true);
        } else if let Some(step) = step {
            s.view.open_guide_embedded(step, true);
        }
        s
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.view.say(msg);
    }

    /// The page showing (the note widget shows itself on its own page).
    pub fn page(&self) -> Page {
        self.flow.page
    }

    // -- input --------------------------------------------------------------------------------------------------
    pub fn handle(&mut self, st: &mut AppState, ev: &Event) {
        let typing = st.ctx.memory(|m| {
            m.focused()
                .is_some_and(|id| id == key_box_id("psarc") || id == key_box_id("sng"))
        });
        if typing {
            return;
        }
        if self.view.setup.is_some() {
            self.view.guide_key(ev);
            return;
        }
        let Event::Key {
            key, pressed: true, ..
        } = ev
        else {
            return;
        };
        let page = self.flow.page;
        let act = match key {
            Key::Escape => Some("close".to_string()),
            Key::Enter | Key::ArrowRight => Some(if self.flow.is_last() {
                "finish".into()
            } else {
                "next".into()
            }),
            Key::Backspace | Key::ArrowLeft => Some("back".into()),
            Key::S if !self.flow.is_last() => Some("skip".into()),
            Key::Space if page == Page::Latency => Some("exclusive".into()),
            Key::Space if page == Page::Widget => Some("notewidget".into()),
            Key::Num1 if page == Page::Nam => Some("nam:no".into()),
            Key::Num2 if page == Page::Nam => Some("nam:yes".into()),
            Key::Num1 | Key::Num2 | Key::Num3 if page == Page::Look => {
                let i = match key {
                    Key::Num1 => 0,
                    Key::Num2 => 1,
                    _ => 2,
                };
                Some(format!("theme:{}", ThemeName::ALL[i].key()))
            }
            _ => None,
        };
        if let Some(a) = act {
            self.acts.push(a);
        }
    }

    // -- actions ------------------------------------------------------------------------------------------------
    fn act(&mut self, st: &mut AppState, player: Option<&mut PlayerView>, name: &str) {
        let (verb, arg) = name.split_once(':').unwrap_or((name, ""));
        match verb {
            "next" if self.flow.page == Page::Guitar && !guitar_ready(st) => self.start_guitar(),
            "next" => self.flow.next(),
            "skip" => self.flow.skip(),
            "back" => {
                self.flow.back();
            }
            "go" => {
                if let Some(p) = Page::parse(arg) {
                    self.flow.go(p);
                }
            }
            "guitar" => self.start_guitar(),
            "instrument" => set_instrument(st, qrs_analysis::instrument::Instrument::parse(arg)),
            "nam" => {
                let on = arg == "yes";
                if self.staged {
                    self.nam_staged = Some(on);
                } else {
                    fs::set_nam(&mut st.config, on);
                    st.save_config();
                }
            }
            "namfind" => self.nam_find(st, arg.parse().unwrap_or(0)),
            "namcancel" => crate::downloads::cancel(arg.parse().unwrap_or(0)),
            "namretry" => crate::downloads::retry(arg.parse().unwrap_or(0)),
            "close" => self.close(st),
            "finish" => self.finish(st),
            "room" => {
                self.finish(st);
                self.room = true;
            }
            "theme" => {
                if !self.staged {
                    st.apply_theme(ThemeName::parse(arg));
                }
            }
            "songsdefault" => {
                let dir = fs::default_songs_dir(&st.data.root);
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    self.view.say(format!("Couldn't make {}: {e}", dir.display()));
                    return;
                }
                match st.add_folder(&dir) {
                    Ok(_) => self.view.say(format!(
                        "Made {}: put your feedpaks or psarc files in it",
                        dir.display()
                    )),
                    Err(e) => self.view.say(e),
                }
            }
            // the switches: through the engine as Settings > Audio does (with no engine, saved for the next start)
            "exclusive" if st.audio.is_none() => {
                let on = !fs::low_latency(&st.config);
                fs::set_low_latency(&mut st.config, on);
                st.save_config();
            }
            "notewidget" => {
                let on = !fs::note_widget(&st.config);
                fs::set_note_widget(&mut st.config, on);
                self.view.changed(st, player);
            }
            "openfolder" => {
                if let Some(d) = st.dirs.first() {
                    crate::recording::open_dir(d);
                }
            }
            // Settings' own: the switches, the folders, the keys
            _ => self.view.act(st, player, name),
        }
    }

    fn start_guitar(&mut self) {
        self.view.open_guide_embedded(Step::Input, self.staged);
    }

    /// Esc or Close: it doesn't open by itself again (Settings has it).
    fn close(&mut self, st: &mut AppState) {
        self.closed = true;
        if self.staged {
            return;
        }
        fs::record(&mut st.config, "skipped");
        st.save_config();
        st.say("Setup closed: you can run it again from Settings › About");
    }

    fn finish(&mut self, st: &mut AppState) {
        self.closed = true;
        if self.staged {
            return;
        }
        fs::record(&mut st.config, "done");
        st.save_config();
    }

    // -- drawing --------------------------------------------------------------------------------------------------
    /// The panel (with `draw` false, only the actions queued by keys: the frame the page under it is captured in).
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
        // the guided setup's and the Directories page's own buttons
        for a in std::mem::take(&mut self.view.acts) {
            self.act(st, player.as_deref_mut(), &a);
        }
        match self.view.guide_exit.take() {
            Some(GuideExit::Close) => self.close(st),
            Some(GuideExit::Back) => {}
            Some(GuideExit::Finish) => self.flow.next(),
            None => {}
        }
        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }

    fn draw(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let panel = SettingsView::panel_rect(area);
        // modal: clicks outside the panel do nothing (a stray click doesn't end a first run's setup)
        interact(ui, area, "first-setup-modal", Sense::click());
        // (a solid face under the glass: the page's words and covers would show through it)
        paint::rrect(&p, panel, px(30.0), th.base);
        paint::floating(&p, panel, Some(30.0));
        let pad = px(28.0);
        let head = Rect::from_min_size(
            pos2(panel.min.x + pad, panel.min.y + pad),
            vec2(panel.width() - 2.0 * pad, px(46.0)),
        );
        let tr = text::draw(
            &p,
            "Set up quickRS",
            pos2(head.min.x + px(4.0), centre_y(head)),
            Align2::LEFT_CENTER,
            Style::new(26.0, 800, th.text),
            None,
        );
        self.draw_steps(
            ui,
            st.instrument(),
            Rect::from_min_max(pos2(tr.max.x + px(28.0), head.min.y), head.max),
        );
        let body = Rect::from_min_max(
            pos2(head.min.x, head.max.y + px(24.0)),
            pos2(head.max.x, panel.max.y - pad),
        );
        let mut child = ui.new_child(UiBuilder::new().max_rect(panel));
        child.set_clip_rect(panel.intersect(ui.clip_rect()));
        if self.view.setup.is_some() {
            // the guided guitar setup, in the panel (its own heading, footer and keys)
            self.view.draw_guide(&mut child, st, body);
        } else {
            self.draw_page(&mut child, st, body);
        }
        if let Some((msg, at)) = &self.view.message
            && at.elapsed() < Duration::from_secs(5)
        {
            widgets::toast(&p, msg, pos2(panel.center().x, panel.max.y - px(110.0)), "");
        }
    }

    /// The steps along the top: a well with a key each (the page on it raised), a light for each page left done
    /// (green) or skipped (amber); a click goes to it.
    fn draw_steps(&mut self, ui: &Ui, inst: qrs_analysis::instrument::Instrument, room: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let pages = self.flow.pages();
        let n = pages.len() as f32;
        let gap = px(4.0);
        let inner = px(5.0);
        let w = (px(150.0) * n + gap * (n - 1.0) + 2.0 * inner).min(room.width());
        let well = Rect::from_min_size(pos2(room.max.x - w, room.min.y), vec2(w, room.height()));
        paint::well(&p, well, 18.0);
        let pw = snap((w - 2.0 * inner - gap * (n - 1.0)) / n);
        for (i, pg) in pages.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(well.min.x + inner + i as f32 * (pw + gap), well.min.y + inner),
                vec2(pw, well.height() - 2.0 * inner),
            );
            let on = *pg == self.flow.page;
            if on {
                paint::raised(&p, r, Some(12.0), 3.0, Fill::Base, 0.0);
            }
            let resp = interact(ui, r, ("first-setup-step", i), Sense::click());
            let guide = self.view.setup.is_some();
            if resp.hovered() && !guide {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() && !guide {
                self.acts.push(format!("go:{}", pg.key()));
            }
            let c = pos2(r.min.x + px(16.0), centre_y(r));
            match (on, self.flow.mark(*pg)) {
                (true, _) => paint::led(&p, c, 9.0, th.accent),
                (_, Mark::Done) => paint::led(&p, c, 8.0, th.hit),
                (_, Mark::Skipped) => paint::led(&p, c, 8.0, th.late),
                (_, Mark::Todo) => paint::dot(&p, c, px(4.0), th.track),
            }
            text::draw(
                &p,
                &format!("{}  {}", i + 1, pg.short_for(inst)),
                pos2(c.x + px(14.0), centre_y(r)),
                Align2::LEFT_CENTER,
                Style::new(
                    13.0,
                    if on { 800 } else { 600 },
                    if on {
                        th.accent_ink
                    } else if resp.hovered() {
                        th.text
                    } else {
                        th.text_2
                    },
                ),
                Some(r.max.x - c.x - px(18.0)),
            );
        }
    }

    /// A page: the footer, the words and controls at the left, the picture at the right (the look page: its three
    /// themes across).
    fn draw_page(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let page = self.flow.page;
        let (i, n) = self.flow.place();
        let last = self.flow.is_last();
        // the footer: Close at the left; Back, Skip and Next (Finish on the last page) at the right
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
        if widgets::button(
            ui,
            Rect::from_min_size(pos2(body.min.x, fy), vec2(cw, bh)),
            "first-setup-close",
            "Close",
            "Esc",
            bst(false, true),
        )
        .clicked()
        {
            self.acts.push("close".into());
        }
        let next_label = if last {
            "Finish"
        } else if page == Page::Guitar && !guitar_ready(st) {
            if st.instrument().is_bass() {
                "Set up my bass"
            } else {
                "Set up my guitar"
            }
        } else {
            "Next"
        };
        let mut buttons = vec![(
            next_label,
            "Enter",
            if last { "finish" } else { "next" },
            true,
            true,
        )];
        if !last {
            buttons.push(("Skip", "S", "skip", false, true));
        }
        buttons.push(("Back", "", "back", false, i > 0));
        let mut right = body.max.x;
        for (label, key, act, primary, enabled) in buttons {
            let bw = widgets::button_width(&ctx, label, key, 14.0, false).max(px(110.0));
            let r = Rect::from_min_size(pos2(right - bw, fy), vec2(bw, bh));
            if widgets::button(ui, r, ("first-setup-btn", act), label, key, bst(primary, enabled)).clicked() {
                self.acts.push(act.to_string());
            }
            right = r.min.x - px(10.0);
        }
        let lx = body.min.x + cw + px(18.0);
        text::draw(
            &p,
            if self.flow.page == Page::Done {
                "Settings has all of this, and this setup again at its top."
            } else {
                "Close any time: you can run this again from Settings › About."
            },
            pos2(lx, fy + bh / 2.0),
            Align2::LEFT_CENTER,
            Style::new(13.0, 500, th.caption),
            Some((right - px(8.0) - lx).max(px(40.0))),
        );
        let content = Rect::from_min_max(body.min, pos2(body.max.x, fy - px(24.0)));
        // the left column: the step, the heading, the words, the page's controls, and where it is in Settings
        let full = page == Page::Look;
        let gap = px(36.0);
        let lw = if full || page == Page::Nam {
            content.width()
        } else if page == Page::Folders {
            snap(content.width() * 0.6)
        } else {
            snap(content.width() * 0.5)
        };
        let (x, w) = (content.min.x, lw);
        let cr = text::caps(
            &p,
            &format!("STEP {} OF {n}", i + 1),
            pos2(x, content.min.y),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        let asio = page == Page::Latency && asio_out(st);
        let tr = text::draw(
            &p,
            if asio {
                "Low latency: already on"
            } else {
                page.title_for(st.instrument())
            },
            pos2(x, cr.max.y + px(8.0)),
            Align2::LEFT_TOP,
            Style::new(30.0, 800, th.text),
            Some(w),
        );
        let mut y = tr.max.y + px(10.0);
        let ist = Style::new(15.0, 500, th.text_2);
        let paras = intro(page, st.instrument(), asio);
        // (the folders page is tight: its words in one line less)
        for (k, para) in paras.iter().enumerate() {
            if k > 0 {
                y += px(8.0);
            }
            for line in text::wrap(&ctx, para, ist, w) {
                y = text::draw(&p, &line, pos2(x, y), Align2::LEFT_TOP, ist, None)
                    .max
                    .y;
            }
        }
        y += px(20.0);
        let later_h = line_h(&ctx, 13.0);
        let ctl = Rect::from_min_max(pos2(x, y), pos2(x + w, content.max.y - later_h - px(14.0)));
        match page {
            Page::Look => self.page_look(ui, ctl),
            Page::Guitar => self.page_guitar(ui, st, ctl),
            Page::Latency if asio => {}
            Page::Latency => self.page_switch(ui, st, ctl, true),
            Page::Widget => self.page_switch(ui, st, ctl, false),
            Page::Folders => self.page_folders(ui, st, ctl),
            Page::Nam => self.page_nam(ui, st, ctl),
            Page::Done => self.page_done(ui, st, ctl),
        }
        // where it is later
        let ly = content.max.y - later_h;
        paint::dot(&p, pos2(x + px(4.0), ly + later_h / 2.0), px(3.5), th.accent);
        text::draw(
            &p,
            &page.later(),
            pos2(x + px(16.0), ly),
            Align2::LEFT_TOP,
            Style::new(13.0, 700, th.text_2),
            Some(w - px(16.0)),
        );
        // the picture, in a framed screen, with what it shows under it
        if let Some(pic) = Pic::of(page) {
            let col = Rect::from_min_max(pos2(x + lw + gap, content.min.y), content.max);
            picture(ui, pic, col, pic_caption(pic));
        }
    }

    /// The three themes across: each a card with its picture, name and words; the one in use ringed.
    fn page_look(&mut self, ui: &Ui, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let gap = px(24.0);
        let cw = snap((area.width() - 2.0 * gap) / 3.0);
        let now = theme::theme();
        // under the cards: the sidebar's pick (the logo by "quickRS") steps through the themes any time, with the
        // pick itself drawn in the line
        let pick_h = px(30.0);
        // the cards as tall as their picture and words (the pictures all one shape)
        let ipad = px(16.0);
        let text_h = line_h(&ctx, 18.0) + px(6.0) + line_h(&ctx, 13.0) + px(4.0);
        let aspect = fs::picture(&ctx, Pic::Theme(now))
            .map(|(_, [iw, ih])| iw as f32 / ih as f32)
            .unwrap_or(1.5);
        let card_h = (2.0 * ipad + (cw - 2.0 * ipad) / aspect + px(12.0) + text_h)
            .min(area.height() - pick_h - px(22.0));
        let ly = area.min.y + card_h + px(22.0);
        let lst = Style::new(14.0, 600, th.text_2);
        let a = text::draw(
            &p,
            "Click the",
            pos2(area.min.x + px(4.0), ly + pick_h / 2.0),
            Align2::LEFT_CENTER,
            lst,
            None,
        );
        let pr = Rect::from_min_size(pos2(a.max.x + px(6.0), ly), vec2(pick_h, pick_h));
        if let Some(tex) = crate::picks::pick(
            &ctx,
            if theme::is_dark() {
                "pick-gold"
            } else {
                "pick-graphite"
            },
        ) {
            paint::image(&p, tex, pr, egui::Color32::WHITE);
        }
        text::draw(
            &p,
            "pick at the top left, beside \"quickRS\" in the sidebar, to switch themes any time: Midnight, Graphite, \
             Daylight in turn.",
            pos2(pr.max.x + px(6.0), ly + pick_h / 2.0),
            Align2::LEFT_CENTER,
            lst,
            Some(area.max.x - pr.max.x - px(10.0)),
        );
        for (i, name) in ThemeName::ALL.into_iter().enumerate() {
            let card = Rect::from_min_size(
                pos2(area.min.x + i as f32 * (cw + gap), area.min.y),
                vec2(cw, card_h),
            );
            let on = name == now;
            let resp = interact(ui, card, ("first-setup-theme", i), Sense::click());
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.acts.push(format!("theme:{}", name.key()));
            }
            paint::card(&p, card, 24.0, resp.hovered() && !on);
            if on {
                paint::ring(&p, card, Some(24.0), 3.0, th.accent, false);
            }
            let pr = Rect::from_min_max(
                card.min + vec2(ipad, ipad),
                pos2(card.max.x - ipad, card.max.y - ipad - text_h - px(12.0)),
            );
            if let Some((tex, [iw, ih])) = fs::picture(&ctx, Pic::Theme(name)) {
                let r = fit(pr, iw as f32 / ih as f32, true);
                paint::art(&p, Some(tex), r, 16.0, true);
            }
            let ty = card.max.y - ipad - text_h;
            let nr = text::draw(
                &p,
                name.label(),
                pos2(card.min.x + ipad + px(4.0), ty),
                Align2::LEFT_TOP,
                Style::new(18.0, 800, if on { th.accent_ink } else { th.text }),
                None,
            );
            keycap(
                &p,
                &(i + 1).to_string(),
                pos2(card.max.x - ipad - px(4.0), centre_y(nr)),
                Align2::RIGHT_CENTER,
                11.0,
                CapOn::Material,
            );
            if on {
                text::caps(
                    &p,
                    "IN USE",
                    pos2(nr.max.x + px(12.0), centre_y(nr)),
                    11.0,
                    th.hit,
                    Align2::LEFT_CENTER,
                    None,
                );
            }
            text::draw(
                &p,
                theme_words(name),
                pos2(card.min.x + ipad + px(4.0), nr.max.y + px(6.0)),
                Align2::LEFT_TOP,
                Style::new(13.0, 500, th.caption),
                Some(cw - 2.0 * ipad),
            );
        }
    }

    /// Your guitar: how it stands, the button that starts the steps, and what works without songs.
    fn page_guitar(&mut self, ui: &Ui, st: &AppState, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (x, w) = (area.min.x, area.width());
        let mut y = area.min.y;
        let ready = guitar_ready(st);
        let (status, ok) = guitar_status(st);
        let sr = text::draw(
            &p,
            &status,
            pos2(x + px(22.0), y),
            Align2::LEFT_TOP,
            Style::new(14.0, 700, if ok { th.hit } else { th.text_2 }),
            Some(w - px(22.0)),
        );
        let c = pos2(x + px(7.0), centre_y(sr));
        if ok {
            paint::led(&p, c, 8.0, th.hit);
        } else {
            paint::dot(&p, c, px(4.0), th.track);
        }
        y = sr.max.y + px(18.0);
        // what you play (P32.1): a guitar unless you say bass; Settings > Audio has it per input
        {
            use qrs_analysis::instrument::Instrument;
            let labels = ["Guitar", "Bass", "5-string bass"];
            let now = st.instrument();
            let sel = Instrument::ALL.iter().position(|i| *i == now);
            let seg = Rect::from_min_size(pos2(x, y), vec2(px(360.0).min(w), px(40.0)));
            let mut so = widgets::Segmented::new(&labels, sel);
            so.size = 13.0;
            so.pad = 4.0;
            so.r = 14.0;
            so.inner_r = 10.0;
            let (_, clicked) = widgets::segmented(ui, seg, "first-setup-instrument", &so);
            if let Some(i) = clicked {
                self.acts.push(format!("instrument:{}", Instrument::ALL[i].key()));
            }
            let cr = text::draw(
                &p,
                match now {
                    Instrument::Bass5 => {
                        "A 5-string bass: its notes are read down to the low B, and songs open on their bass part."
                    }
                    Instrument::Bass => {
                        "A bass: its notes are read down to the low E, and songs open on their bass part."
                    }
                    Instrument::Guitar => "What you play. A guitar can play bass parts too.",
                },
                pos2(x, seg.max.y + px(8.0)),
                Align2::LEFT_TOP,
                Style::new(13.0, 500, th.caption),
                Some(w),
            );
            y = cr.max.y + px(18.0);
        }
        let label = match (ready, st.instrument().is_bass()) {
            (true, _) => "Set it up again",
            (false, true) => "Set up my bass",
            (false, false) => "Set up my guitar",
        };
        let bw = widgets::button_width(&ctx, label, "", 15.0, false).max(px(220.0));
        if widgets::button(
            ui,
            Rect::from_min_size(pos2(x, y), vec2(bw, px(50.0))),
            "first-setup-guitar",
            label,
            "",
            ButtonStyle {
                primary: !ready,
                size: 15.0,
                r: Some(15.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            self.acts.push("guitar".into());
        }
        y += px(50.0) + px(26.0);
        // the amp room works with no songs and no keys
        let nst = Style::new(14.0, 500, th.text_2);
        let words = format!(
            "No songs or keys yet? You can still play through the amp room and record yourself: it needs only \
             your {}.",
            st.instrument().noun()
        );
        let lines = text::wrap(&ctx, &words, nst, w - px(40.0));
        let lh = text::line_height(&ctx, nst);
        let box_h = px(20.0) + line_h(&ctx, 12.0) + px(6.0) + lines.len() as f32 * lh + px(18.0);
        let bx = Rect::from_min_size(pos2(x, y), vec2(w, box_h));
        if bx.max.y <= area.max.y + px(4.0) {
            paint::well(&p, bx, 18.0);
            let cr = text::caps(
                &p,
                "THE AMP ROOM",
                pos2(x + px(20.0), y + px(18.0)),
                12.0,
                th.accent_ink,
                Align2::LEFT_TOP,
                None,
            );
            let mut ly = cr.max.y + px(6.0);
            for l in &lines {
                ly = text::draw(&p, l, pos2(x + px(20.0), ly), Align2::LEFT_TOP, nst, None)
                    .max
                    .y;
            }
        }
    }

    /// Low latency or the note widget: a switch to choose now, and what it is.
    fn page_switch(&mut self, ui: &Ui, st: &AppState, area: Rect, latency: bool) {
        let th = t();
        let p = ui.painter().clone();
        let (x, w) = (area.min.x, area.width());
        let pad = px(20.0);
        let h = SettingsView::h_switch("lg", true) + 2.0 * pad;
        let card = Rect::from_min_size(area.min, vec2(w, h));
        paint::card(&p, card, 22.0, false);
        let (on, label, hint, act) = if latency {
            let on = st
                .audio
                .as_ref()
                .map(|a| a.prefs().exclusive)
                .unwrap_or_else(|| fs::low_latency(&st.config));
            (
                on,
                "Low-latency output",
                if on {
                    "on: other programs go quiet while quickRS plays"
                } else {
                    "off: other programs can play too (the usual way)"
                },
                "exclusive",
            )
        } else {
            let on = fs::note_widget(&st.config);
            (
                on,
                "Note widget",
                if on {
                    "on: it floats over your screen"
                } else {
                    "off: no window of its own"
                },
                "notewidget",
            )
        };
        self.view.switch_row(
            ui,
            x + pad,
            card.min.y + pad,
            w - 2.0 * pad,
            label,
            hint,
            on,
            act,
            "lg",
            "Space",
            700,
        );
        let mut y = card.max.y + px(14.0);
        let note = if latency {
            if st.audio.is_none() {
                "The audio engine isn't running, so it's saved for the next start."
            } else {
                "It takes effect straight away; if your device won't do it, quickRS stays on the shared output."
            }
        } else {
            "It listens to the input you set up, songs or not."
        };
        y = super::caption_lines(&p, note, x + px(4.0), y, w - px(8.0), 13.0, 2);
        let _ = (y, th);
    }

    /// The folders: songs, RS's keys, and recordings.
    fn page_folders(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (x, w) = (area.min.x, area.width());
        let mut y = area.min.y;
        let bs = ButtonStyle {
            size: 13.0,
            r: Some(12.0),
            ..Default::default()
        };
        let small_h = px(36.0);
        // SONGS
        let note = if st.scanning() {
            let done = st.library.done.load(std::sync::atomic::Ordering::Relaxed);
            let total = st.library.total.load(std::sync::atomic::Ordering::Relaxed);
            format!("reading songs… {done} of {total}")
        } else {
            format!("{} found", crate::home::songs(st.groups.len()))
        };
        y = self.view.heading(&p, "SONGS", x, y, Some(x + w), &note, "");
        let dirs = st.dirs.clone();
        let rows = dirs.len().clamp(1, 2);
        let row_h = px(22.0);
        let bx = Rect::from_min_size(pos2(x, y), vec2(w, px(12.0) + rows as f32 * row_h));
        paint::display(&p, bx, Some(12.0), 3.0);
        if dirs.is_empty() {
            text::draw(
                &p,
                "No song folder yet",
                pos2(bx.min.x + px(14.0), centre_y(bx)),
                Align2::LEFT_CENTER,
                Style::new(13.0, 500, th.caption),
                Some(w - px(28.0)),
            );
        }
        for (k, d) in dirs.iter().take(rows).enumerate() {
            let more = if k + 1 == rows && dirs.len() > rows {
                format!("  (and {} more)", dirs.len() - rows)
            } else {
                String::new()
            };
            text::draw(
                &p,
                &format!("{}{more}", d.display()),
                pos2(
                    bx.min.x + px(14.0),
                    bx.min.y + px(6.0) + k as f32 * row_h + row_h / 2.0,
                ),
                Align2::LEFT_CENTER,
                Style::new(13.0, 600, th.text),
                Some(w - px(28.0)),
            );
        }
        y = bx.max.y + px(10.0);
        let mut bxr = x;
        if dirs.is_empty() {
            let dflt = fs::default_songs_dir(&st.data.root);
            let label = format!("Make {}", folder_name(&dflt));
            let bw = widgets::button_width(&ctx, &label, "", 13.0, false);
            let r = Rect::from_min_size(pos2(bxr, y), vec2(bw, small_h));
            if widgets::button(
                ui,
                r,
                "first-setup-songsdefault",
                &label,
                "",
                ButtonStyle { primary: true, ..bs },
            )
            .on_hover_text(dflt.display().to_string())
            .clicked()
            {
                self.acts.push("songsdefault".into());
            }
            bxr = r.max.x + px(10.0);
        }
        let label = if dirs.is_empty() {
            "Pick a folder…"
        } else {
            "Add another folder…"
        };
        let bw = widgets::button_width(&ctx, label, "", 13.0, false);
        let r = Rect::from_min_size(pos2(bxr, y), vec2(bw, small_h));
        if widgets::button(ui, r, "first-setup-addfolder", label, "", bs).clicked() {
            self.acts.push("addfolder".into());
        }
        let cx = r.max.x + px(14.0);
        text::draw(
            &p,
            if st.config.search_dlc() && !qrs_library::config::rs_dlc_dirs().is_empty() {
                "RS's dlc folder is read too"
            } else {
                "Feedpaks play straight away"
            },
            pos2(cx, centre_y(r)),
            Align2::LEFT_CENTER,
            Style::new(12.0, 500, th.caption),
            Some((x + w - cx).max(px(20.0))),
        );
        y = r.max.y + px(22.0);
        // RS KEYS
        y = self.view.heading(
            &p,
            "RS KEYS",
            x,
            y,
            Some(x + w),
            "only for .psarc files: feedpaks don't need keys",
            "",
        );
        let (status, col) = match &st.key_state {
            KeyState::Found { .. } => ("Keys found: psarc songs play".to_string(), th.hit),
            KeyState::Partial { .. } => ("Your keys file has only one of the two keys".into(), th.miss),
            KeyState::Invalid { .. } => (
                "Your keys file doesn't read: paste the keys again".into(),
                th.miss,
            ),
            KeyState::Missing => (
                "No keys: paste them, or load your keys file".to_string(),
                th.caption,
            ),
        };
        let sr = text::draw(
            &p,
            &status,
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(13.0, 700, col),
            Some(w),
        );
        y = sr.max.y + px(8.0);
        let kgap = px(12.0);
        let kw = snap((w - kgap) / 2.0);
        let box_h = px(38.0);
        let mut pasted = None;
        for (k, (which, hint)) in [("psarc", "PSARC key (64 hex characters)"), ("sng", "SNG key")]
            .into_iter()
            .enumerate()
        {
            let r = Rect::from_min_size(pos2(x + k as f32 * (kw + kgap), y), vec2(kw, box_h));
            let value = if which == "psarc" {
                &mut self.view.psarc_in
            } else {
                &mut self.view.sng_in
            };
            let resp = text_box(ui, r, which, value, hint);
            if resp.changed()
                && let Some(both) = keyfile::split_pasted(value)
            {
                pasted = Some(both);
            }
            if let Err(e) = keyfile::check_key(value) {
                text::draw(
                    &p,
                    &e,
                    pos2(r.min.x + px(4.0), r.max.y + px(3.0)),
                    Align2::LEFT_TOP,
                    Style::new(11.0, 600, th.miss),
                    Some(kw),
                );
            }
        }
        if let Some((a, b)) = pasted {
            self.view.psarc_in = a;
            self.view.sng_in = b;
        }
        y += box_h + px(20.0);
        let ready = matches!(keyfile::check_key(&self.view.psarc_in), Ok(Some(_)))
            && matches!(keyfile::check_key(&self.view.sng_in), Ok(Some(_)));
        let sw = widgets::button_width(&ctx, "Save keys", "", 13.0, false);
        let save = Rect::from_min_size(pos2(x, y), vec2(sw, small_h));
        if widgets::button(
            ui,
            save,
            "first-setup-savekeys",
            "Save keys",
            "",
            ButtonStyle {
                primary: ready,
                enabled: ready,
                ..bs
            },
        )
        .clicked()
        {
            self.acts.push("savekeys".into());
        }
        let lw = widgets::button_width(&ctx, "Load keys file…", "", 13.0, false);
        let load = Rect::from_min_size(pos2(save.max.x + px(10.0), y), vec2(lw, small_h));
        if widgets::button(ui, load, "first-setup-loadkeys", "Load keys file…", "", bs).clicked() {
            self.acts.push("loadkeys".into());
        }
        let nx = load.max.x + px(14.0);
        let (msg, ok) = self
            .view
            .key_note
            .clone()
            .unwrap_or_else(|| ("A whole keys.json pasted into either box fills both".into(), true));
        text::draw(
            &p,
            &msg,
            pos2(nx, centre_y(load)),
            Align2::LEFT_CENTER,
            Style::new(
                12.0,
                if self.view.key_note.is_some() { 700 } else { 500 },
                if self.view.key_note.is_none() {
                    th.caption
                } else if ok {
                    th.hit
                } else {
                    th.miss
                },
            ),
            Some((x + w - nx).max(px(20.0))),
        );
        y = load.max.y + px(22.0);
        // RECORDINGS
        use qrs_library::config::{RECORDINGS_DIR, TAKES_DIR};
        y = self.view.heading(
            &p,
            "RECORDINGS",
            x,
            y,
            Some(x + w),
            "named by the date and time",
            "",
        );
        let root = st.data.root.clone();
        for (key, title, cap, dir) in [
            (
                RECORDINGS_DIR,
                "What you hear",
                "R",
                st.config.recordings_dir(&root),
            ),
            (TAKES_DIR, "Song takes", "Ctrl R", st.config.takes_dir(&root)),
        ] {
            if y + small_h > area.max.y + px(4.0) {
                break;
            }
            let row = Rect::from_min_size(pos2(x, y), vec2(w, small_h));
            let cw = widgets::button_width(&ctx, "Change…", "", 12.0, false);
            let ch = Rect::from_min_size(pos2(row.max.x - cw, row.min.y + px(3.0)), vec2(cw, px(30.0)));
            if widgets::button(
                ui,
                ch,
                ("first-setup-recdir", key),
                "Change…",
                "",
                ButtonStyle {
                    size: 12.0,
                    r: Some(10.0),
                    ..Default::default()
                },
            )
            .clicked()
            {
                self.acts.push(format!("recdir_change:{key}"));
            }
            let tr = text::draw(
                &p,
                title,
                pos2(x, centre_y(row)),
                Align2::LEFT_CENTER,
                Style::new(13.0, 800, th.text),
                None,
            );
            let kr = keycap(
                &p,
                cap,
                pos2(tr.max.x + px(8.0), centre_y(row)),
                Align2::LEFT_CENTER,
                10.0,
                CapOn::Material,
            );
            let px0 = (x + px(150.0)).max(kr.max.x + px(12.0));
            text::draw(
                &p,
                &dir.display().to_string(),
                pos2(px0, centre_y(row)),
                Align2::LEFT_CENTER,
                Style::new(12.0, 600, th.text_2),
                Some((ch.min.x - px(12.0) - px0).max(px(20.0))),
            );
            y = row.max.y + px(6.0);
        }
    }

    /// Find on TONE3000 (P36.7): the search words to the clipboard, and TONE3000's own select flow in the browser
    /// (your account); the capture you pick downloads into your library.
    fn nam_find(&mut self, st: &AppState, i: usize) {
        use crate::captures::tone3000;
        if self.staged {
            return;
        }
        let (list, _, _) = nam_suggestions(false, 8, st.instrument());
        let Some(sg) = list.get(i) else { return };
        let Some(key) = tone3000::client_id(&st.config) else {
            self.nam_note = Some(
                "TONE3000 needs your key first: Settings > Neural Amp Modeler says where to get it.".into(),
            );
            return;
        };
        st.ctx.copy_text(sg.unit.query.to_string());
        self.nam_pick = Some(crate::downloads::start(key, sg.unit.kind, Some(sg.unit)));
        self.nam_note = Some(format!(
            "TONE3000 is open in your browser: search \"{}\" (copied) and pick a capture; it downloads here.",
            sg.unit.query
        ));
    }

    /// Use NAM?: No / Yes, and with Yes the suggested captures (the gear your songs use most).
    fn page_nam(&mut self, ui: &Ui, st: &AppState, area: Rect) {
        use crate::captures::tone3000::Phase;
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (x, w) = (area.min.x, area.width());
        let choice = if self.staged {
            self.nam_staged
        } else {
            fs::nam_choice(&st.config)
        };
        // the answer: two keys (1 / 2)
        let labels = ["1  No: quickRS's own amps", "2  Yes: NAM captures of real gear"];
        let seg = Rect::from_min_size(pos2(x, area.min.y), vec2(px(620.0).min(w), px(46.0)));
        let mut so = widgets::Segmented::new(&labels, choice.map(|on| on as usize));
        so.size = 14.0;
        so.pad = 4.0;
        so.r = 14.0;
        so.inner_r = 10.0;
        let (_, clicked) = widgets::segmented(ui, seg, "first-setup-nam", &so);
        if let Some(i) = clicked {
            self.acts.push(if i == 1 { "nam:yes" } else { "nam:no" }.into());
        }
        let mut y = seg.max.y + px(20.0);
        // the pick under way
        if let Some(id) = self.nam_pick {
            match crate::downloads::view(id).map(|v| v.phase) {
                Some(Phase::Done(it)) => {
                    self.nam_note = Some(format!("Added {} to your captures", it.name));
                    self.nam_pick = None;
                }
                Some(Phase::Failed(_, e)) => {
                    self.nam_note = Some(format!("Didn't download: {e}"));
                    self.nam_pick = None;
                }
                Some(Phase::Cancelled { .. }) | None => {
                    self.nam_note = None;
                    self.nam_pick = None;
                }
                Some(_) => {}
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let pad = px(20.0);
        if choice != Some(true) {
            let words = if choice == Some(false) {
                "Every song plays on quickRS's own amps, cabs and pedals. You can change your mind any time: \
                 Settings > Neural Amp Modeler has everything NAM, and the amp room has MODELLED or NAM on each unit."
            } else {
                "Not sure? No is fine: quickRS's models play every song. NAM takes more of the computer (each capture \
                 about a fifth of a core) and a free TONE3000 account."
            };
            super::caption_lines(&p, words, x + px(4.0), y, w.min(px(900.0)), 14.0, 3);
            return;
        }
        let (list, covered, total) = nam_suggestions(self.staged, 8, st.instrument());
        let hh = SettingsView::h_heading(&ctx);
        let row_h = px(44.0);
        let rows = list.len().max(1);
        // (the card leaves room for its caption under it: two lines)
        let cap_room = px(12.0) + 2.0 * line_h(&ctx, 13.0);
        let card_h = (2.0 * pad + hh + rows as f32 * row_h + px(8.0)).min(area.max.y - y - cap_room);
        let card = Rect::from_min_size(pos2(x, y), vec2(w, card_h));
        paint::card(&p, card, 22.0, false);
        let note = if list.is_empty() {
            String::new()
        } else {
            format!(
                "these cover {covered} of your {total} song{}{}",
                if total == 1 { "" } else { "s" },
                if st.instrument().is_bass() {
                    " with a bass part"
                } else {
                    ""
                }
            )
        };
        y = self.view.heading(
            &p,
            if st.instrument().is_bass() {
                "SUGGESTED FOR YOUR BASS PARTS"
            } else {
                "SUGGESTED FOR YOUR SONGS"
            },
            x + pad,
            card.min.y + pad,
            Some(card.max.x - pad),
            &note,
            "",
        );
        if list.is_empty() {
            text::draw(
                &p,
                if crate::gearindex::reading().is_some() {
                    "Reading your songs' gear\u{2026}"
                } else {
                    "No songs with RS tones yet: add your songs (next pages), then see Settings > Neural Amp Modeler."
                },
                pos2(x + pad, y + px(10.0)),
                Align2::LEFT_TOP,
                Style::new(14.0, 600, th.text_2),
                Some(w - 2.0 * pad),
            );
        }
        for (i, sg) in list.iter().enumerate() {
            let r = Rect::from_min_size(pos2(x + pad, y + i as f32 * row_h), vec2(w - 2.0 * pad, row_h));
            if r.max.y > card.max.y - px(4.0) {
                break;
            }
            if i > 0 {
                p.hline(r.min.x..=r.max.x, r.min.y, (1.0, th.divider));
            }
            let cy = centre_y(r);
            let kind = match sg.unit.kind {
                crate::captures::Kind::Nam => "amp",
                crate::captures::Kind::Pedal => "pedal",
                crate::captures::Kind::Ir => "cab (IR)",
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
            let br = Rect::from_min_size(pos2(r.max.x - bw, cy - px(16.0)), vec2(bw, px(32.0)));
            match crate::downloads::suggestion_tail(ui, ("setup-sg", i), br, sg.unit, sg.have, sg.songs) {
                Some(crate::downloads::RowAct::Find) => self.acts.push(format!("namfind:{i}")),
                Some(crate::downloads::RowAct::Cancel(id)) => self.acts.push(format!("namcancel:{id}")),
                Some(crate::downloads::RowAct::Retry(id)) => self.acts.push(format!("namretry:{id}")),
                None => {}
            }
        }
        let ny = card.max.y + px(12.0);
        let words = self.nam_note.clone().unwrap_or_else(|| {
            "Each opens TONE3000 in your browser: sign in with your own account, pick a capture, and it downloads \
             into your captures. Songs use the ones you have; the rest stay on quickRS's models."
                .into()
        });
        super::caption_lines(&p, &words, x + px(4.0), ny, w, 13.0, 2);
    }

    /// Done: what's set, and where to go with no songs (the amp room, feedpaks, the converter).
    fn page_done(&mut self, ui: &Ui, st: &AppState, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (x, w) = (area.min.x, area.width());
        let mut rows: Vec<(&str, String, bool)> = vec![("Look", theme::theme().label().to_string(), true)];
        // (a step you skipped says so: not "not calibrated yet" right after you chose to skip it, P37.6a)
        let inner = self.view.guide_flow.as_ref();
        let page_skipped = self.flow.mark(Page::Guitar) == Mark::Skipped;
        let skipped = |s: Step| page_skipped || inner.is_some_and(|f| f.mark(s) == Mark::Skipped);
        let (gs, gok) = guitar_status(st);
        let gs = if !gok && skipped(Step::Calibrate) {
            "Skipped: Settings > Sound sets it up any time".to_string()
        } else {
            gs.trim_start_matches("Now: ").to_string()
        };
        rows.push((Page::Guitar.short_for(st.instrument()), gs, gok));
        let timing = super::timing_tool::summary(st);
        rows.push((
            "Timing",
            match &timing {
                Some(t) => t.clone(),
                None if skipped(Step::Timing) => {
                    "Skipped: Settings > Timing > Calibrate timing any time".into()
                }
                None => "not measured yet (Settings > Timing > Calibrate timing)".into(),
            },
            timing.is_some(),
        ));
        if self.flow.pages().contains(&Page::Latency) {
            let on = st
                .audio
                .as_ref()
                .map(|a| a.prefs().exclusive)
                .unwrap_or_else(|| fs::low_latency(&st.config));
            let what = if asio_out(st) {
                "ASIO: already direct"
            } else if on {
                "On"
            } else {
                "Off"
            };
            rows.push(("Low latency", what.into(), true));
        }
        if self.flow.pages().contains(&Page::Nam) {
            let nam = self.nam_staged.or(fs::nam_choice(&st.config));
            rows.push((
                "Tones",
                match nam {
                    Some(true) => "NAM captures where you have them, quickRS's models for the rest".into(),
                    Some(false) => "quickRS's own models".into(),
                    None => "not asked (Settings > Neural Amp Modeler)".into(),
                },
                nam.is_some(),
            ));
        }
        let nw = fs::note_widget(&st.config);
        rows.push(("Note widget", if nw { "On" } else { "Off" }.into(), true));
        let songs = st.groups.len();
        rows.push((
            "Songs",
            if st.scanning() {
                "reading your folders…".into()
            } else if songs > 0 {
                format!(
                    "{} in {} folder{}",
                    crate::home::songs(songs),
                    st.dirs.len(),
                    if st.dirs.len() == 1 { "" } else { "s" }
                )
            } else if st.dirs.is_empty() {
                "no song folder yet".into()
            } else {
                "no songs in your folders yet".into()
            },
            songs > 0,
        ));
        let keys = matches!(st.key_state, KeyState::Found { .. });
        rows.push((
            "RS keys",
            if keys {
                "found".into()
            } else {
                "none (feedpaks don't need them)".into()
            },
            keys,
        ));
        rows.push((
            "Recordings",
            st.config.recordings_dir(&st.data.root).display().to_string(),
            true,
        ));
        let pad = px(18.0);
        let row_h = px(32.0);
        let card = Rect::from_min_size(area.min, vec2(w, 2.0 * pad + rows.len() as f32 * row_h));
        paint::card(&p, card, 22.0, false);
        let lw = px(150.0);
        for (k, (name, what, ok)) in rows.iter().enumerate() {
            let cy = card.min.y + pad + k as f32 * row_h + snap(row_h / 2.0);
            if k > 0 {
                p.hline(
                    x + pad..=card.max.x - pad,
                    cy - snap(row_h / 2.0),
                    (1.0, th.divider),
                );
            }
            let c = pos2(x + pad + px(6.0), cy);
            if *ok {
                paint::led(&p, c, 8.0, th.hit);
            } else {
                paint::dot(&p, c, px(4.0), th.track);
            }
            text::draw(
                &p,
                name,
                pos2(x + pad + px(22.0), cy),
                Align2::LEFT_CENTER,
                Style::new(14.0, 700, th.text),
                Some(lw - px(26.0)),
            );
            text::draw(
                &p,
                what,
                pos2(x + pad + lw, cy),
                Align2::LEFT_CENTER,
                Style::new(14.0, 500, th.text_2),
                Some(w - 2.0 * pad - lw),
            );
        }
        let mut y = card.max.y + px(20.0);
        if songs == 0 || !keys {
            let words = if songs == 0 {
                "No songs yet? Feedpaks play straight away: drop a folder of them on the window, or make them from \
                 your own RS songs with quickRS Converter. Meanwhile the amp room needs no songs: play \
                 through its amps and pedals, and record yourself."
            } else {
                "Your psarc songs wait for RS's keys (Settings > Library); feedpaks play now. The amp \
                 room needs no songs: play through its amps and pedals, and record yourself."
            };
            let nst = Style::new(13.0, 500, th.text_2);
            let bw = widgets::button_width(&ctx, "Go to the amp room", "", 14.0, false).max(px(200.0));
            for line in text::wrap(&ctx, words, nst, w) {
                y = text::draw(&p, &line, pos2(x, y), Align2::LEFT_TOP, nst, None)
                    .max
                    .y;
            }
            y += px(14.0);
            if y + px(46.0) <= area.max.y + px(4.0)
                && widgets::button(
                    ui,
                    Rect::from_min_size(pos2(x, y), vec2(bw, px(46.0))),
                    "first-setup-room",
                    "Go to the amp room",
                    "",
                    ButtonStyle {
                        primary: songs == 0,
                        size: 14.0,
                        r: Some(14.0),
                        ..Default::default()
                    },
                )
                .clicked()
            {
                self.acts.push("room".into());
            }
        }
    }
}

impl Default for FirstSetup {
    fn default() -> Self {
        FirstSetup::new()
    }
}

/// The suggestions shown: the library's (`captures::suggested_captures`), or, in screenshots, a made-up library's.
/// The made-up library's cabs and amps vary song to song (P37.6a: one cab on every song read as "a Princeton 1x10 for
/// 40 of 40 songs", which no real library does); a bass's are bass parts' gear.
pub(super) fn nam_suggestions(
    staged: bool,
    limit: usize,
    inst: qrs_analysis::instrument::Instrument,
) -> (Vec<crate::captures::suggest::Suggestion>, usize, usize) {
    if staged {
        let bass = inst.is_bass();
        let songs: Vec<Vec<String>> = (0..40)
            .map(|i| {
                let (amp, cab) = if bass {
                    match i % 7 {
                        0..=2 => ("Bass_Amp_HT300B", "Bass_Cab_AT810BC_57_Cone"),
                        3 | 4 => ("Bass_Amp_LT25B", "Bass_Cab_AT1150BC_Tube_Cone"),
                        5 => ("DI_Amp_BassDriver", "Bass_Cab_BT410BC_Tube_Cone"),
                        _ => ("Bass_Amp_CH300B", "Bass_Cab_BT410BC_Tube_Cone"),
                    }
                } else {
                    match i % 7 {
                        0..=2 => ("Amp_AT20", "Cab_EN212C_57_Cone"),
                        3 | 4 => ("Amp_TW26", "Cab_TW110C_57_Cone"),
                        5 => ("Pedal_TubeScreamer", "Cab_CA412C_57_Cone"),
                        _ => ("Amp_MarshallPlexi", "Cab_GB412CMKIII_57_Cone"),
                    }
                };
                vec![amp.to_string(), cab.to_string()]
            })
            .collect();
        let have = |u: &crate::captures::suggest::Unit| {
            u.rs.contains(&"Amp_TW26") || u.rs.contains(&"Bass_Amp_LT25B")
        };
        let (l, c) = crate::captures::suggest::rank(&songs, have, limit, bass);
        return (l, c, songs.len());
    }
    crate::captures::suggested_captures(limit, inst)
}

/// Whether the guitar input listening now has been calibrated (the guitar page's Next moves on then): a run that
/// asked for another go doesn't count, unless it was kept anyway (P36.3).
fn guitar_ready(st: &AppState) -> bool {
    calibrate::current_input(st).is_some_and(|(id, _)| calibrate::load(&st.config, &id).calibrated())
}

/// How the guitar stands, in a line, and whether it's set up.
/// What you play, from the setup's pick (P32.1): the default for inputs never set, and the input listening now.
fn set_instrument(st: &mut AppState, inst: qrs_analysis::instrument::Instrument) {
    st.config.set("instrument", serde_json::json!(inst.key()));
    if let Some((id, _)) = calibrate::current_input(st) {
        let mut c = calibrate::load(&st.config, &id);
        c.instrument = inst;
        calibrate::save(&mut st.config, &id, &c);
    }
    st.save_config();
}

fn guitar_status(st: &AppState) -> (String, bool) {
    match calibrate::current_input(st) {
        Some((id, name)) if calibrate::load(&st.config, &id).calibrated() => {
            match calibrate::load(&st.config, &id).caveat() {
                Some(c) => (format!("Now: {name}, calibrated ({c})"), true),
                None => (format!("Now: {name}, calibrated"), true),
            }
        }
        Some((id, name)) if calibrate::load(&st.config, &id).peak_db.is_some() => (
            format!("Now: {name}: the last calibration asked for another go, calibrate again"),
            false,
        ),
        Some((_, name)) => (format!("Now: {name}, its level not set yet"), false),
        None if st.audio.is_none() => ("The audio engine isn't running".into(), false),
        None => (
            format!("No input found yet: plug your {} in", st.instrument().noun()),
            false,
        ),
    }
}

/// A folder's last part ("quickRS Songs").
fn folder_name(d: &std::path::Path) -> String {
    d.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| d.display().to_string())
}

/// The largest rect of `aspect` (w / h) in `r`: at its top (`top`) or its middle, centred across.
fn fit(r: Rect, aspect: f32, top: bool) -> Rect {
    let (mut w, mut h) = (r.width(), r.width() / aspect);
    if h > r.height() {
        h = r.height();
        w = h * aspect;
    }
    let x = r.min.x + (r.width() - w) / 2.0;
    let y = if top {
        r.min.y
    } else {
        r.min.y + (r.height() - h) / 2.0
    };
    Rect::from_min_size(pos2(x, y), vec2(w, h))
}

/// A page's picture in `col`: framed like a screen, at the column's top, at most its own size (it isn't blown up
/// past the screenshot's pixels), with its caption under it.
fn picture(ui: &Ui, pic: Pic, col: Rect, caption: &str) {
    let th = t();
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let Some((tex, [iw, ih])) = fs::picture(&ctx, pic) else {
        return;
    };
    let cap_h = line_h(&ctx, 12.0) + px(10.0);
    let ppp = ctx.pixels_per_point();
    let natural = vec2(iw as f32 / ppp, ih as f32 / ppp) + vec2(px(8.0), px(8.0));
    let room = Rect::from_min_size(
        col.min,
        vec2(
            col.width().min(natural.x * 1.25),
            (col.height() - cap_h).min(natural.y * 1.25),
        ),
    );
    let mut r = fit(room, iw as f32 / ih as f32, true);
    r = r.translate(vec2((col.width() - r.width()) / 2.0 - (r.min.x - col.min.x), 0.0));
    paint::art(&p, Some(tex), r, 18.0, true);
    text::draw(
        &p,
        caption,
        pos2(r.center().x, r.max.y + px(10.0)),
        Align2::CENTER_TOP,
        Style::new(12.0, 600, th.caption),
        Some(col.width()),
    );
}
