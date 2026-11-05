//! The controls in place of the tiles. Was a port of the controls half of rsplay/app.py `PlayerView`; redesigned in
//! P35.1 (D90, the owner's layout A of the controls review): three tabs, Practise / Sound / View, as a segmented strip
//! at the top, each fitting the panel at 1440 x 900 (it scrolls only in a smaller window).
//! - Practise: Settings' Reduce delay card first (the FAST low-latency output, P41.4, `settings/fastout.rs`), speed
//!   (a stepper and presets 50 / 75 / 90 / 100), difficulty (a notched slider) with level-up, the loop and Save as a
//!   lick, the practice modes (note by note, continuous, ear) with a line each, the take, the review, and a "Go to"
//!   line of keys.
//! - Sound: your guitar (hear it, the tone, recorded or synth guitar in the song, your sounds), pitch, the stems one
//!   row each, and a link to the timing settings.
//! - View: tab or highway, what the tab shows, the highway's strings, two tiles and a link to the rest.
//!
//! One on/off style throughout (`widgets::switch`), no knobs (the header's SPEED knob opens Practise). The panel
//! remembers its tab (config `controls_tab`). Every control here is also a key; the keys are drawn beside them.

use eframe::egui;
use egui::{Align2, CursorIcon, Painter, Pos2, Rect, Sense, Ui, pos2, vec2};
use qrs_library::Config;
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap, keycap_width};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{fmt_time, t};
use qrs_ui::units::{px, snap};
use qrs_ui::widgets::{self, ButtonStyle, Segmented, interact};

use super::{PlayerView, SPEEDS, tiles, tone_label};
use crate::settings::fastout;
use crate::state::AppState;

/// The panel's tabs, in the strip's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrlTab {
    Practise,
    Sound,
    View,
}

impl CtrlTab {
    pub const ALL: [CtrlTab; 3] = [CtrlTab::Practise, CtrlTab::Sound, CtrlTab::View];

    pub fn key(self) -> &'static str {
        match self {
            CtrlTab::Practise => "practise",
            CtrlTab::Sound => "sound",
            CtrlTab::View => "view",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CtrlTab::Practise => "Practise",
            CtrlTab::Sound => "Sound",
            CtrlTab::View => "View",
        }
    }

    pub fn from_key(k: &str) -> Option<CtrlTab> {
        CtrlTab::ALL.into_iter().find(|t_| t_.key() == k)
    }
}

/// The tab the panel was last on (Practise at first; the old `panel_open` folds are ignored).
pub fn ctrl_tab(cfg: &Config) -> CtrlTab {
    CtrlTab::from_key(&cfg.str("controls_tab", "practise")).unwrap_or(CtrlTab::Practise)
}

pub fn set_ctrl_tab(cfg: &mut Config, tab: CtrlTab) {
    cfg.set("controls_tab", tab.key());
}

/// The speed presets beside the stepper.
pub const PRESETS: [f64; 4] = [0.5, 0.75, 0.9, 1.0];

/// The speed step nearest `v` (the presets, and anything typed into `speed_set:`, land on one of `SPEEDS`).
pub fn snap_speed(v: f64) -> f64 {
    SPEEDS
        .iter()
        .copied()
        .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
        .unwrap_or(1.0)
}

/// The difficulty step from `cur` to `target` (both 0..1; the target lands on a tenth): what `change_mastery` takes.
pub fn diff_step(cur: f64, target: f64) -> f64 {
    let to = (target.clamp(0.0, 1.0) * 10.0).round() / 10.0;
    ((to - cur) * 100.0).round() / 100.0
}

/// The panel's action for a click on the Reduce delay card ("fastout:exclusive", "fastout:buffer:128"): the player
/// hands it to `fastout::apply`, as Settings does its own.
pub fn fastout_action(a: fastout::Act) -> String {
    format!("fastout:{}", a.action())
}

/// The panel's action back to what it asks for.
pub fn fastout_act(action: &str) -> Option<fastout::Act> {
    action.strip_prefix("fastout:").and_then(fastout::Act::parse)
}

/// The difficulty slider's stops (0, 10, ... 100 %).
const DIFF_STOPS: usize = 11;

/// A card's padding, the gap between cards, a row of keys' height.
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const KEY_H: f32 = 34.0;

/// A small-caps heading at the left of a card's top line.
fn heading(p: &Painter, s: &str, x: f32, cy: f32) -> Rect {
    text::caps(p, s, pos2(x, cy), 11.0, t().caption, Align2::LEFT_CENTER, None)
}

/// Text in the caption colour.
fn note(p: &Painter, s: &str, at: Pos2, anchor: Align2, max_w: Option<f32>) -> Rect {
    text::draw(p, s, at, anchor, Style::new(11.5, 500, t().caption), max_w)
}

/// Key caps (with "/" between alternatives as text) ending at `right`, centred on `cy`. Returns their left edge.
fn keys_left_of(p: &Painter, keys: &[&str], right: f32, cy: f32) -> f32 {
    let th = t();
    let mut x = right;
    for k in keys.iter().rev() {
        if *k == "/" {
            let r = text::draw(
                p,
                "/",
                pos2(x - px(3.0), cy),
                Align2::RIGHT_CENTER,
                Style::new(11.0, 500, th.caption),
                None,
            );
            x = r.min.x - px(3.0);
        } else {
            let r = keycap(p, k, pos2(x, cy), Align2::RIGHT_CENTER, 11.0, cap_on());
            x = r.min.x - px(3.0);
        }
    }
    x
}

/// What key caps on the panel sit on: outlined in the glass ink on the dark themes (as on the glass keys), sunk
/// into the material in Daylight and the classic look.
fn cap_on() -> CapOn {
    if qrs_ui::glass::on() {
        CapOn::Ivory
    } else {
        CapOn::Material
    }
}

/// A card: a well sunk into the console.
fn card(p: &Painter, r: Rect) {
    paint::inset(p, r, Some(18.0), 5.0, Fill::Base);
}

impl PlayerView {
    /// The controls in place of the tiles: the tab strip, then the tab's page. Scrolls only when the window is too
    /// small for the page, sliding out under the console's edges.
    pub(super) fn draw_controls(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        let p = ui.painter().clone();
        let tab = ctrl_tab(&st.config);
        // the tab strip stays put; the page under it scrolls if it must
        let strip = Rect::from_min_size(area.min, vec2(area.width(), px(44.0)));
        let labels: Vec<&str> = CtrlTab::ALL.iter().map(|t_| t_.label()).collect();
        let sel = CtrlTab::ALL.iter().position(|t_| *t_ == tab);
        let seg = Segmented {
            size: 14.0,
            r: 16.0,
            inner_r: 12.0,
            pad: 5.0,
            ..Segmented::new(&labels, sel)
        };
        let (_, hit) = widgets::segmented(ui, strip, "ctrl-tabs", &seg);
        if let Some(i) = hit
            && Some(i) != sel
        {
            self.acts.push(format!("ctrl_tab:{}", CtrlTab::ALL[i].key()));
        }
        let page = Rect::from_min_max(pos2(area.min.x, strip.max.y + px(GAP)), area.max);
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(page));
        cui.set_clip_rect(page.intersect(ui.clip_rect()));
        let top = page.min.y - snap(self.ctrl_scroll);
        let (x, w) = (page.min.x, page.width());
        let bottom = match tab {
            CtrlTab::Practise => self.draw_practise_page(&cui, st, x, top, w),
            CtrlTab::Sound => self.draw_sound_page(&cui, st, x, top, w),
            CtrlTab::View => self.draw_view_page(&cui, st, x, top, w),
        };
        self.ctrl_max = (bottom - top - page.height()).max(0.0);
        self.ctrl_scroll = self.ctrl_scroll.clamp(0.0, self.ctrl_max);
        if self.ctrl_scroll > 0.5 || self.ctrl_scroll < self.ctrl_max - 0.5 {
            // what's behind the edges (the console's face) laid back over the content, fading out inwards
            let pad = px(16.0);
            let console = Rect::from_min_max(
                area.min - vec2(pad, pad),
                pos2(area.max.x + pad, area.max.y + px(12.0) + px(48.0) + pad),
            );
            paint::fade_edges(
                &p,
                page,
                self.ctrl_scroll > 0.5,
                self.ctrl_scroll < self.ctrl_max - 0.5,
                24.0,
                paint::FaceKind::Base {
                    rect: console,
                    sunk: false,
                },
            );
        }
    }

    // -- the pieces every page uses ------------------------------------------------------------------------------

    /// A card's top line: its heading at the left and, at the right, a caption and its keys. Returns the y below it.
    #[allow(clippy::too_many_arguments)]
    fn card_top(&self, p: &Painter, title: &str, lx: f32, rx: f32, y: f32, hint: &str, keys: &[&str]) -> f32 {
        let cy = y + px(9.0);
        let hr = heading(p, title, lx, cy);
        let kx = if keys.is_empty() {
            rx
        } else {
            keys_left_of(p, keys, rx, cy) - px(3.0)
        };
        if !hint.is_empty() {
            note(
                p,
                hint,
                pos2(kx, cy),
                Align2::RIGHT_CENTER,
                Some((kx - hr.max.x - px(12.0)).max(px(20.0))),
            );
        }
        y + px(18.0) + px(8.0)
    }

    /// A row with a name (its key cap after it, an explanation under it) and a switch at the right; the whole row
    /// clicks. Pushes `action` when clicked.
    #[allow(clippy::too_many_arguments)]
    fn switch_row(
        &mut self,
        ui: &Ui,
        row: Rect,
        label: &str,
        key: &str,
        explain: &str,
        on: bool,
        action: &str,
        enabled: bool,
    ) {
        let p = ui.painter();
        let th = t();
        let sw = Rect::from_min_size(
            pos2(row.max.x - px(46.0), snap(row.center().y - px(12.0))),
            vec2(px(46.0), px(24.0)),
        );
        let resp = interact(
            ui,
            row,
            ("ctrl-sw", action),
            if enabled { Sense::click() } else { Sense::hover() },
        );
        let hot = enabled && resp.hovered();
        widgets::draw_switch(p, sw, egui::Id::new(("ctrl-sw-k", action)), on);
        let ly = if explain.is_empty() {
            row.center().y
        } else {
            row.center().y - px(8.0)
        };
        let col = if enabled { th.text } else { th.caption };
        let lr = text::draw(
            p,
            label,
            pos2(row.min.x, ly),
            Align2::LEFT_CENTER,
            Style::new(13.0, 700, col),
            Some(sw.min.x - row.min.x - px(40.0)),
        );
        if !key.is_empty() {
            keycap(
                p,
                key,
                pos2(lr.max.x + px(7.0), ly),
                Align2::LEFT_CENTER,
                10.0,
                cap_on(),
            );
        }
        if !explain.is_empty() {
            note(
                p,
                explain,
                pos2(row.min.x, ly + px(17.0)),
                Align2::LEFT_CENTER,
                Some(sw.min.x - row.min.x - px(8.0)),
            );
        }
        if hot {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.acts.push(action.into());
        }
    }

    /// A line that goes somewhere (Settings, the pedalboard): caption text, an arrow, its key. Pushes `action`.
    fn link_row(&mut self, ui: &Ui, x: f32, cy: f32, max_w: f32, label: &str, key: &str, action: &str) {
        let p = ui.painter();
        let th = t();
        let ctx = ui.ctx();
        let st = Style::new(12.0, 600, th.caption);
        let key_w = if key.is_empty() {
            0.0
        } else {
            keycap_width(ctx, key, 10.0) + px(6.0)
        };
        let lw = text::width(ctx, label, st).min(max_w - px(16.0) - key_w);
        let hit = Rect::from_min_size(
            pos2(x - px(4.0), cy - px(11.0)),
            vec2(lw + px(16.0) + key_w + px(8.0), px(22.0)),
        );
        let resp = interact(ui, hit, ("ctrl-link", action), Sense::click());
        let col = if resp.hovered() { th.text } else { th.caption };
        let lr = text::draw(
            p,
            label,
            pos2(x, cy),
            Align2::LEFT_CENTER,
            Style { color: col, ..st },
            Some(lw + px(1.0)),
        );
        icons::icon(p, Icon::Right, pos2(lr.max.x + px(8.0), cy), 11.0, col);
        if !key.is_empty() {
            keycap(
                p,
                key,
                pos2(lr.max.x + px(16.0), cy),
                Align2::LEFT_CENTER,
                10.0,
                cap_on(),
            );
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.acts.push(action.into());
        }
    }

    /// A key (a glass plate on the dark themes, ivory elsewhere). Pushes `action` when clicked.
    #[allow(clippy::too_many_arguments)]
    fn key_btn(
        &mut self,
        ui: &Ui,
        r: Rect,
        label: &str,
        key: &str,
        action: &str,
        active: bool,
        enabled: bool,
    ) {
        let style = ButtonStyle {
            active,
            enabled,
            // (a stepper's − and + read as signs)
            size: if matches!(label, "−" | "+") { 17.0 } else { 12.5 },
            r: Some(12.0),
            align: if key.is_empty() {
                widgets::Align::Center
            } else {
                widgets::Align::Split
            },
            ..Default::default()
        };
        if widgets::button(ui, r, ("ctrl-key", action), label, key, style).clicked() {
            self.acts.push(action.into());
        }
    }

    /// A two-way choice beside a name (Recorded | Synth, As charted | In the key). Pushes `action` when the other
    /// option is clicked.
    fn choice(&mut self, ui: &Ui, r: Rect, id: &'static str, labels: &[&str], sel: usize, action: &str) {
        let seg = Segmented {
            size: 12.0,
            r: 13.0,
            inner_r: 10.0,
            pad: 4.0,
            ..Segmented::new(labels, Some(sel))
        };
        let (_, hit) = widgets::segmented(ui, r, id, &seg);
        if hit.is_some_and(|i| i != sel) {
            self.acts.push(action.into());
        }
    }

    /// A name with its key cap at the left of a row (the row's control is drawn by the caller). Returns its rect.
    fn row_name(&self, p: &Painter, label: &str, key: &str, x: f32, cy: f32) -> Rect {
        let th = t();
        let lr = text::draw(
            p,
            label,
            pos2(x, cy),
            Align2::LEFT_CENTER,
            Style::new(13.0, 700, th.text),
            None,
        );
        if key.is_empty() {
            return lr;
        }
        let kr = keycap(
            p,
            key,
            pos2(lr.max.x + px(7.0), cy),
            Align2::LEFT_CENTER,
            10.0,
            cap_on(),
        );
        lr.union(kr)
    }

    /// A stepper: a round − key, the value, a round + key. Returns its right edge.
    #[allow(clippy::too_many_arguments)]
    fn stepper(&mut self, ui: &Ui, x: f32, y: f32, value: &str, value_w: f32, less: &str, more: &str) -> f32 {
        let th = t();
        let d = px(KEY_H);
        let minus = Rect::from_min_size(pos2(x, y), vec2(d, d));
        self.key_btn(ui, minus, "−", "", less, false, true);
        let vx = minus.max.x + px(4.0);
        text::draw(
            ui.painter(),
            value,
            pos2(vx + value_w / 2.0, minus.center().y),
            Align2::CENTER_CENTER,
            Style::new(16.0, 800, th.accent_ink),
            None,
        );
        let plus = Rect::from_min_size(pos2(vx + value_w + px(4.0), y), vec2(d, d));
        self.key_btn(ui, plus, "+", "", more, false, true);
        plus.max.x
    }

    // -- Practise ------------------------------------------------------------------------------------------------

    /// Settings' Reduce delay card first (the FAST low-latency output, P41.4, D174), then speed and difficulty, the
    /// loop, the practice modes, "Go to". Returns the page's bottom.
    fn draw_practise_page(&mut self, ui: &Ui, st: &mut AppState, x: f32, y: f32, w: f32) -> f32 {
        let shown = fastout::Shown::of(st);
        let (r, act) = fastout::draw(ui, x, y, w, fastout::Frame::Panel, &shown, "ctrl");
        if let Some(a) = act {
            self.acts.push(fastout_action(a));
        }
        let y = r.max.y + px(GAP);
        let y = self.draw_speed_card(ui, x, y, w) + px(GAP);
        let y = self.draw_loop_card(ui, x, y, w) + px(GAP);
        let y = self.draw_modes_card(ui, x, y, w) + px(GAP);
        self.draw_goto_row(ui, x + px(PAD), y + px(10.0), w - 2.0 * px(PAD)) + px(4.0)
    }

    /// SPEED (a stepper and the presets) and DIFFICULTY (a notched slider) with level-up. Returns its bottom.
    fn draw_speed_card(&mut self, ui: &Ui, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let h = px(PAD) * 2.0 + px(26.0) + px(KEY_H) + px(16.0) + px(26.0) + px(26.0) + px(8.0) + px(28.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let mut yy = self.card_top(&p, "SPEED", lx, rx, y + px(PAD), "", &["Ctrl", "=", "/", "="]);
        let speed = self.pending_speed.unwrap_or_else(|| self.speed());
        let pct = (speed * 100.0).round() as i64;
        let right = self.stepper(ui, lx, yy, &format!("{pct}%"), px(50.0), "slower", "faster");
        // the presets: equal keys in what's left, lit when the speed is on one
        let gap = px(4.0);
        let x0 = right + px(10.0);
        let pw = ((rx - x0 - gap * 3.0) / 4.0).floor();
        for (i, v) in PRESETS.iter().enumerate() {
            let pr = Rect::from_min_size(pos2(x0 + i as f32 * (pw + gap), yy), vec2(pw, px(KEY_H)));
            // (lit by an amber label: a light wouldn't fit beside the number)
            let style = ButtonStyle {
                accent: (speed - v).abs() < 1e-6,
                size: 12.5,
                r: Some(12.0),
                ..Default::default()
            };
            let label = format!("{}", (v * 100.0).round() as i64);
            if widgets::button(ui, pr, ("ctrl-preset", i), &label, "", style).clicked() {
                self.acts.push(format!("speed_set:{v}"));
            }
        }
        yy += px(KEY_H) + px(16.0);
        // the difficulty: what the slider is set to while it's dragged
        let shown = self.diff_preview.unwrap_or(self.mastery);
        let cy = yy + px(9.0);
        let hr = heading(&p, "DIFFICULTY", lx, cy);
        text::draw(
            &p,
            &format!("{}%", (shown * 100.0).round() as i64),
            pos2(hr.max.x + px(10.0), cy),
            Align2::LEFT_CENTER,
            Style::new(14.0, 800, th.text),
            None,
        );
        keys_left_of(&p, &["Ctrl", "↓", "/", "Ctrl", "↑"], rx, cy);
        yy += px(26.0);
        let groove = Rect::from_min_size(
            pos2(lx + px(4.0), snap(yy + px(13.0) - px(4.0))),
            vec2(rx - lx - px(8.0), px(8.0)),
        );
        let hit = groove.expand2(vec2(px(10.0), px(12.0)));
        let resp = interact(ui, hit, "ctrl-diff", Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let at = |pos: Pos2| ((pos.x - groove.min.x) / groove.width()).clamp(0.0, 1.0) as f64;
        if (resp.dragged() || resp.is_pointer_button_down_on())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            self.diff_preview = Some(((at(pos) * 10.0).round() / 10.0).clamp(0.0, 1.0));
        }
        if resp.drag_stopped() || resp.clicked() {
            let target = resp
                .interact_pointer_pos()
                .map(at)
                .or(self.diff_preview)
                .unwrap_or(self.mastery);
            self.diff_preview = None;
            if diff_step(self.mastery, target).abs() > 1e-9 {
                self.acts
                    .push(format!("diff_set:{}", (target * 10.0).round() / 10.0));
            }
        } else if !resp.dragged() && !resp.is_pointer_button_down_on() {
            self.diff_preview = None;
        }
        widgets::slider_notched(
            &p,
            groove,
            shown as f32,
            DIFF_STOPS,
            18.0,
            resp.hovered() || resp.dragged(),
        );
        yy += px(26.0) + px(8.0);
        let auto = self.auto_level;
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + px(28.0))),
            "Level up after a clean loop",
            "",
            "",
            auto,
            "auto_level",
            true,
        );
        r.max.y
    }

    /// LOOP: its readout, Start / End / Section / Clear, Save as a lick. Returns its bottom.
    fn draw_loop_card(&mut self, ui: &Ui, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let h = px(PAD) * 2.0 + px(26.0) + px(40.0) + px(10.0) + px(KEY_H) + px(8.0) + px(KEY_H);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let mut yy = self.card_top(
            &p,
            "LOOP",
            lx,
            rx,
            y + px(PAD),
            "or Ctrl-drag across the tab",
            &[],
        );
        let disp = Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + px(40.0)));
        paint::display(&p, disp, Some(12.0), 3.0);
        let cy = disp.center().y;
        let tx = disp.min.x + px(14.0);
        match (self.loop_a, self.loop_b) {
            (Some(a), Some(b)) => {
                let ta = text::draw(
                    &p,
                    &format!("{}–{}", fmt_time(a), fmt_time(b)),
                    pos2(tx, cy),
                    Align2::LEFT_CENTER,
                    Style::new(15.0, 800, th.text),
                    None,
                );
                if !self.tab.measures.is_empty() {
                    let (ma, mb) = (
                        self.tab.measure_at(a).map(|m| m.number).unwrap_or(0),
                        self.tab.measure_at(b - 1e-3).map(|m| m.number).unwrap_or(0),
                    );
                    note(
                        &p,
                        &if ma == mb {
                            format!("bar {ma}")
                        } else {
                            format!("bars {ma}–{mb}")
                        },
                        pos2(ta.max.x + px(10.0), cy),
                        Align2::LEFT_CENTER,
                        None,
                    );
                }
                if self.loop_passes > 0 {
                    let s = format!(
                        "pass {}{}",
                        self.loop_passes,
                        self.loop_acc
                            .map(|a| format!("  ·  {}%", (a * 100.0).round_ties_even() as i64))
                            .unwrap_or_default()
                    );
                    text::draw(
                        &p,
                        &s,
                        pos2(disp.max.x - px(14.0), cy),
                        Align2::RIGHT_CENTER,
                        Style::new(13.0, 800, th.accent_ink),
                        None,
                    );
                }
            }
            (Some(a), None) => {
                text::draw(
                    &p,
                    &format!("from {}: set the end with ]", fmt_time(a)),
                    pos2(tx, cy),
                    Align2::LEFT_CENTER,
                    Style::new(13.0, 700, th.text_2),
                    Some(disp.max.x - px(14.0) - tx),
                );
            }
            _ => {
                text::draw(
                    &p,
                    "No loop: set one with the keys",
                    pos2(tx, cy),
                    Align2::LEFT_CENTER,
                    Style::new(13.0, 700, th.caption),
                    Some(disp.max.x - px(14.0) - tx),
                );
            }
        }
        yy = disp.max.y + px(10.0);
        // Start / End / Section in a row; Clear and Save as a lick under them
        let gap = px(6.0);
        let kw = ((rx - lx - 2.0 * gap) / 3.0).floor();
        let has_loop = self.loop_a.zip(self.loop_b).is_some();
        let keys = [
            ("Start", "[", "loop_start", self.loop_a.is_some(), true),
            ("End", "]", "loop_end", self.loop_b.is_some(), true),
            ("Section", "L", "loop_section", false, true),
            ("Clear", "\\", "clear_loop", false, true),
            ("Save as a lick", "Ctrl S", "lick_loop", false, has_loop),
        ];
        for (i, (label, key, action, on, enabled)) in keys.into_iter().enumerate() {
            let (col, row) = (i % 3, i / 3);
            let kx = lx + col as f32 * (kw + gap);
            let kr = Rect::from_min_max(
                pos2(kx, yy + row as f32 * (px(KEY_H) + px(8.0))),
                pos2(
                    if i == 4 { rx } else { kx + kw },
                    yy + row as f32 * (px(KEY_H) + px(8.0)) + px(KEY_H),
                ),
            );
            let style = ButtonStyle {
                active: on,
                enabled,
                size: 12.0,
                r: Some(12.0),
                align: widgets::Align::Split,
                ..Default::default()
            };
            if widgets::button(ui, kr, ("ctrl-loop", action), label, key, style).clicked() {
                self.acts.push(action.into());
            }
        }
        r.max.y
    }

    /// PRACTISE IT: note by note, continuous, ear mode, each with a line; a take and listening back; the review.
    /// Returns its bottom.
    fn draw_modes_card(&mut self, ui: &Ui, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let row_h = px(39.0);
        let h = px(PAD) * 2.0 + px(26.0) + 4.0 * row_h + px(8.0) + px(KEY_H) + px(10.0) + px(22.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let mut yy = self.card_top(&p, "PRACTISE IT", lx, rx, y + px(PAD), "", &[]);
        let modes = [
            (
                "Note by note",
                "W",
                "waits for each note  ·  Enter skips",
                self.nbn,
                "nbn",
            ),
            (
                "Continuous",
                "O",
                "loops without stopping, every pass scored",
                self.continuous,
                "continuous",
            ),
            (
                "Ear mode",
                "H",
                "notes hidden until you play them",
                self.ear,
                "ear",
            ),
            (
                "Count-in",
                "Shift C",
                "3, 2, 1 before playing: songs, loops, spots, takes",
                self.countdown_on,
                "countdown",
            ),
        ];
        for (label, key, explain, on, action) in modes {
            let row = Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h));
            self.switch_row(ui, row, label, key, explain, on, action, true);
            yy += row_h;
        }
        yy += px(8.0);
        let gap = px(8.0);
        let bw = ((rx - lx - gap) / 2.0).floor();
        let recording = self.take.as_ref().is_some_and(|s| s.recording());
        let can_listen = self.take.as_ref().is_some_and(|s| s.has_input());
        let listening = self.take.as_ref().is_some_and(|s| s.listening());
        self.key_btn(
            ui,
            Rect::from_min_size(pos2(lx, yy), vec2(bw, px(KEY_H))),
            "Record a take",
            "Ctrl R",
            "take:record",
            recording,
            true,
        );
        self.key_btn(
            ui,
            Rect::from_min_size(pos2(lx + bw + gap, yy), vec2(rx - lx - bw - gap, px(KEY_H))),
            if listening {
                "Stop listening"
            } else {
                "Listen back"
            },
            "B",
            "take:listen",
            listening,
            can_listen,
        );
        yy += px(KEY_H) + px(10.0);
        self.link_row(
            ui,
            lx,
            yy + px(11.0),
            rx - lx,
            "Review the run: spots to practise",
            "E",
            "review",
        );
        r.max.y
    }

    /// "Go to": the start, the previous or next section, back 5 seconds, each a key. Returns its bottom.
    fn draw_goto_row(&mut self, ui: &Ui, x: f32, cy: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let gr = note(&p, "Go to", pos2(x, cy), Align2::LEFT_CENTER, None);
        let items: [(&str, &[(&str, &str)]); 3] = [
            ("start", &[("Home", "start")]),
            ("section", &[("PgUp", "prev_section"), ("PgDn", "next_section")]),
            ("−5 s", &[("−", "rewind")]),
        ];
        let mut xx = gr.max.x + px(14.0);
        for (label, keys) in items {
            let lr = text::draw(
                &p,
                label,
                pos2(xx, cy),
                Align2::LEFT_CENTER,
                Style::new(12.0, 600, th.text_2),
                None,
            );
            let mut kx = lr.max.x + px(6.0);
            for (key, action) in keys {
                let kr = keycap(&p, key, pos2(kx, cy), Align2::LEFT_CENTER, 10.0, cap_on());
                let resp = interact(
                    ui,
                    kr.expand2(vec2(px(2.0), px(6.0))),
                    ("ctrl-goto", *action),
                    Sense::click(),
                );
                if resp.hovered() {
                    paint::ring(&p, kr.expand(px(1.0)), Some(5.0), 1.0, th.accent, false);
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    self.acts.push((*action).into());
                }
                kx = kr.max.x + px(3.0);
            }
            xx = kx + px(14.0);
            if xx > x + w {
                break;
            }
        }
        cy + px(12.0)
    }

    // -- Sound ---------------------------------------------------------------------------------------------------

    /// Your guitar, pitch, the stems, timing. Returns the page's bottom.
    fn draw_sound_page(&mut self, ui: &Ui, st: &mut AppState, x: f32, y: f32, w: f32) -> f32 {
        let y = self.draw_guitar_card(ui, st, x, y, w) + px(GAP);
        let y = self.draw_pitch_card(ui, x, y, w) + px(GAP);
        let y = if self.mixer.stems.is_some() {
            self.draw_stems_card(ui, x, y, w) + px(GAP)
        } else {
            y
        };
        self.draw_timing_card(ui, x, y, w)
    }

    /// YOUR GUITAR: hear it, the tone, recorded or synth guitar in the song, your sounds. Returns its bottom.
    fn draw_guitar_card(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let row_h = px(36.0);
        let Some(e) = self.engine().cloned() else {
            let h = px(PAD) * 2.0 + px(26.0) + px(20.0) + px(26.0);
            let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
            card(&p, r);
            let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
            let inst = st.instrument();
            let yy = self.card_top(&p, &inst.say("YOUR GUITAR"), lx, rx, y + px(PAD), "", &[]);
            note(
                &p,
                &inst.say("Hearing your guitar needs the audio engine"),
                pos2(lx, yy + px(10.0)),
                Align2::LEFT_CENTER,
                Some(rx - lx),
            );
            self.link_row(
                ui,
                lx,
                yy + px(20.0) + px(13.0),
                rx - lx,
                "Settings › Sound",
                "F2",
                "settings",
            );
            return r.max.y;
        };
        let h = px(PAD) * 2.0 + px(26.0) + 3.0 * row_h + px(6.0) + px(22.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let inst = st.instrument();
        let mut yy = self.card_top(
            &p,
            &inst.say("YOUR GUITAR"),
            lx,
            rx,
            y + px(PAD),
            &inst.say("its level: the GUITAR knob below"),
            &[],
        );
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h)),
            &inst.say("Hear my guitar"),
            "M",
            "",
            e.monitor(),
            "monitor",
            true,
        );
        yy += row_h;
        let cy = yy + row_h / 2.0;
        let nr = self.row_name(&p, "Tone", "T", lx, cy);
        let tw_ = (rx - lx) * 0.6;
        let tr = Rect::from_min_size(
            pos2(rx - tw_.max(rx - nr.max.x - px(12.0)).min(tw_), cy - px(15.0)),
            vec2(tw_.min(rx - nr.max.x - px(12.0)), px(30.0)),
        );
        let style = ButtonStyle {
            size: 12.5,
            r: Some(12.0),
            ..Default::default()
        };
        let label = tone_label(&e.tone());
        let resp = widgets::button(ui, tr, "ctrl-tone", &label, "", style);
        if resp.hovered() {
            self.tip = Some((
                "The next tone".into(),
                "T".into(),
                pos2(tr.center().x, tr.min.y - px(6.0)),
                Align2::CENTER_BOTTOM,
            ));
        }
        if resp.clicked() {
            self.acts.push("tone".into());
        }
        yy += row_h;
        let cy = yy + row_h / 2.0;
        let nr = self.row_name(&p, "In the song", "G", lx, cy);
        let synth = self.song_synth.on;
        let cw = (rx - nr.max.x - px(12.0)).min((rx - lx) * 0.6);
        self.choice(
            ui,
            Rect::from_min_size(pos2(rx - cw, cy - px(15.0)), vec2(cw, px(30.0))),
            "ctrl-synth",
            &["Recorded", "Synth"],
            usize::from(synth),
            "song_synth",
        );
        yy += row_h + px(6.0);
        let _ = st;
        self.link_row(
            ui,
            lx,
            yy + px(11.0),
            rx - lx,
            "Your sounds on the Rig",
            "Shift T",
            "sounds",
        );
        r.max.y
    }

    /// PITCH: a stepper in semitones and Original pitch. Returns its bottom.
    fn draw_pitch_card(&mut self, ui: &Ui, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let h = px(PAD) * 2.0 + px(26.0) + px(KEY_H);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let yy = self.card_top(
            &p,
            "PITCH",
            lx,
            rx,
            y + px(PAD),
            "the tab stays as written",
            &["Shift", "↑", "↓"],
        );
        let pitch = self.mixer.pitch;
        let right = self.stepper(
            ui,
            lx,
            yy,
            &super::songaudio::pitch_short(pitch),
            px(56.0),
            "pitch-",
            "pitch+",
        );
        self.key_btn(
            ui,
            Rect::from_min_max(pos2(right + px(12.0), yy), pos2(rx, yy + px(KEY_H))),
            "Original pitch",
            "",
            "pitch0",
            pitch == 0,
            true,
        );
        r.max.y
    }

    /// STEMS: one row each (level, solo, on) and "As the pack sets them". Returns its bottom.
    fn draw_stems_card(&mut self, ui: &Ui, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let Some(set) = self.mixer.stems.clone() else {
            return y;
        };
        let row_h = px(32.0);
        let n = set.stems.len().min(self.mixer.mix.len());
        let h = px(PAD) * 2.0 + px(26.0) + n as f32 * row_h + px(8.0) + px(30.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let mut yy = self.card_top(&p, "STEMS", lx, rx, y + px(PAD), "level  ·  solo  ·  on", &[]);
        let max_gain = qrs_audio::stems::MAX_GAIN;
        for i in 0..n {
            let (s, m) = (&set.stems[i], self.mixer.mix[i]);
            let cy = yy + row_h / 2.0;
            // a stem with no name shows its id ("drums"): capitalised like the named ones
            let mut c = s.info.name.chars();
            let name: String = c
                .next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default();
            text::draw(
                &p,
                &name,
                pos2(lx, cy),
                Align2::LEFT_CENTER,
                Style::new(13.0, 700, if m.on { th.text } else { th.caption }),
                Some(px(70.0)),
            );
            let sw = Rect::from_min_size(pos2(rx - px(44.0), snap(cy - px(11.0))), vec2(px(44.0), px(22.0)));
            if widgets::switch(ui, sw, ("ctrl-stem-on", i), m.on).clicked() {
                self.acts.push(format!("stem:on:{i}"));
            }
            let solo = Rect::from_min_size(
                pos2(sw.min.x - px(8.0) - px(26.0), cy - px(13.0)),
                vec2(px(26.0), px(26.0)),
            );
            let style = ButtonStyle {
                active: false,
                accent: m.solo,
                size: 11.0,
                r: Some(13.0),
                ..Default::default()
            };
            if widgets::button(ui, solo, ("ctrl-stem-solo", i), "S", "", style).clicked() {
                self.acts.push(format!("stem:solo:{i}"));
            }
            let groove = Rect::from_min_max(
                pos2(lx + px(78.0), snap(cy - px(3.0))),
                pos2(solo.min.x - px(12.0), snap(cy - px(3.0)) + px(6.0)),
            );
            let hit = groove.expand2(vec2(px(8.0), px(10.0)));
            let resp = interact(ui, hit, ("ctrl-stem-lvl", i), Sense::click_and_drag());
            if (resp.dragged() || resp.clicked())
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let u = ((pos.x - groove.min.x) / groove.width()).clamp(0.0, 1.0);
                let g = (u * max_gain * 10.0).round() / 10.0;
                if (g - m.gain).abs() > 1e-3 {
                    self.acts.push(format!("stem:set:{i}:{g}"));
                }
            }
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                self.tip = Some((
                    format!("{name} {}%", (m.gain * 100.0).round() as i64),
                    String::new(),
                    pos2(groove.center().x, groove.min.y - px(10.0)),
                    Align2::CENTER_BOTTOM,
                ));
            }
            widgets::slider(
                &p,
                groove,
                m.gain / max_gain,
                None,
                14.0,
                resp.hovered() || resp.dragged(),
                if m.on { None } else { Some(th.tick) },
            );
            yy += row_h;
        }
        yy += px(8.0);
        self.key_btn(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + px(30.0))),
            "As the pack sets them",
            "",
            "stem:reset",
            set.is_default(&self.mixer.mix),
            true,
        );
        r.max.y
    }

    /// TIMING: where its settings are. Returns its bottom.
    fn draw_timing_card(&mut self, ui: &Ui, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let h = px(PAD) * 2.0 + px(26.0) + px(20.0) + px(24.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let yy = self.card_top(&p, "TIMING", lx, rx, y + px(PAD), "", &[]);
        note(
            &p,
            "Chart sync, and when your notes count: set once",
            pos2(lx, yy + px(10.0)),
            Align2::LEFT_CENTER,
            Some(rx - lx),
        );
        self.link_row(
            ui,
            lx,
            yy + px(20.0) + px(12.0),
            rx - lx,
            "Settings › Timing",
            "",
            "settings:practice",
        );
        r.max.y
    }

    // -- View ----------------------------------------------------------------------------------------------------

    /// Tab or highway, the tab's options, the highway's, two tiles. Returns the page's bottom.
    fn draw_view_page(&mut self, ui: &Ui, st: &mut AppState, x: f32, y: f32, w: f32) -> f32 {
        let p = ui.painter().clone();
        let row_h = px(36.0);
        // SHOW THE PART AS
        let h = px(PAD) * 2.0 + px(26.0) + px(KEY_H);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let (lx, rx) = (x + px(PAD), r.max.x - px(PAD));
        let yy = self.card_top(&p, "SHOW THE PART AS", lx, rx, y + px(PAD), "", &["V"]);
        let hw = self.hw_on;
        self.choice(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + px(KEY_H))),
            "ctrl-view",
            &["Tab", "Highway"],
            usize::from(hw),
            "view",
        );
        // ON THE TAB
        let y = r.max.y + px(GAP);
        let h = px(PAD) * 2.0 + px(26.0) + 4.0 * row_h + px(18.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let mut yy = self.card_top(&p, "ON THE TAB", lx, rx, y + px(PAD), "", &[]);
        let nota = self.nota.show;
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h)),
            "Rhythm notation",
            "N",
            "",
            nota,
            "nota:show",
            true,
        );
        yy += row_h;
        let cy = yy + row_h / 2.0;
        let nr = self.row_name(&p, "Chord names", "K", lx, cy);
        let cw = (rx - nr.max.x - px(12.0)).min((rx - lx) * 0.6);
        let numerals = self.chord_numerals;
        self.choice(
            ui,
            Rect::from_min_size(pos2(rx - cw, cy - px(15.0)), vec2(cw, px(30.0))),
            "ctrl-chords",
            &["As charted", "In the key"],
            usize::from(numerals),
            "chord_numerals",
        );
        yy += row_h;
        let (compact, full) = (self.tab.compact, self.tab_full);
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h)),
            "Compact rows",
            "C",
            "",
            compact,
            "tab_compact",
            true,
        );
        yy += row_h;
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h)),
            "Full screen",
            "F",
            "",
            full,
            "tab_full",
            true,
        );
        yy += row_h;
        note(
            &p,
            "Zoom, bar width, string spacing: the tab's toolbar",
            pos2(lx, yy + px(9.0)),
            Align2::LEFT_CENTER,
            Some(rx - lx),
        );
        // ON THE HIGHWAY
        let y = r.max.y + px(GAP);
        let h = px(PAD) * 2.0 + px(26.0) + row_h + px(18.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let yy = self.card_top(&p, "ON THE HIGHWAY", lx, rx, y + px(PAD), "", &[]);
        let inv = self.highway.invert;
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h)),
            "Low E at the bottom",
            "I",
            "",
            inv,
            "hw_invert",
            true,
        );
        note(
            &p,
            "Note speed and camera: the highway's toolbar",
            pos2(lx, yy + row_h + px(9.0)),
            Align2::LEFT_CENTER,
            Some(rx - lx),
        );
        // TILES
        let y = r.max.y + px(GAP);
        let h = px(PAD) * 2.0 + px(26.0) + 2.0 * row_h + px(24.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        card(&p, r);
        let mut yy = self.card_top(&p, "TILES", lx, rx, y + px(PAD), "on: open  ·  off: folded", &[]);
        let (lyrics, note_on) = (tiles::tile_open(st, "lyrics"), tiles::tile_open(st, "note"));
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h)),
            "Lyrics",
            "Y",
            "",
            lyrics,
            "lyrics",
            true,
        );
        yy += row_h;
        self.switch_row(
            ui,
            Rect::from_min_max(pos2(lx, yy), pos2(rx, yy + row_h)),
            "Note & chord",
            "",
            "",
            note_on,
            "tile:note",
            true,
        );
        yy += row_h;
        self.link_row(
            ui,
            lx,
            yy + px(12.0),
            rx - lx,
            "All tiles, their order, the note tile: Settings › Display",
            "",
            "settings:display",
        );
        r.max.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_land_on_speeds() {
        for v in PRESETS {
            assert!(SPEEDS.iter().any(|s| (s - v).abs() < 1e-9), "{v} is a speed step");
            assert_eq!(snap_speed(v), v);
        }
        assert_eq!(snap_speed(0.77), 0.75);
        assert_eq!(snap_speed(0.1), 0.25);
        assert_eq!(snap_speed(9.0), 1.5);
        assert_eq!(snap_speed(1.36), 1.4);
    }

    #[test]
    fn difficulty_steps_to_a_tenth() {
        assert_eq!(diff_step(1.0, 0.5), -0.5);
        assert_eq!(diff_step(0.3, 0.84), 0.5);
        assert_eq!(diff_step(0.7, 0.7), 0.0);
        assert_eq!(diff_step(0.0, 1.7), 1.0);
        assert_eq!(diff_step(0.4, -1.0), -0.4);
        // (applied as change_mastery does, it lands on the target)
        let cur: f64 = 0.3;
        let new = ((cur + diff_step(cur, 0.9)).clamp(0.0, 1.0) * 100.0).round() / 100.0;
        assert_eq!(new, 0.9);
    }

    #[test]
    fn the_tab_is_remembered() {
        let mut cfg = Config::default();
        assert_eq!(ctrl_tab(&cfg), CtrlTab::Practise);
        set_ctrl_tab(&mut cfg, CtrlTab::View);
        assert_eq!(cfg.str("controls_tab", ""), "view");
        assert_eq!(ctrl_tab(&cfg), CtrlTab::View);
        set_ctrl_tab(&mut cfg, CtrlTab::Sound);
        assert_eq!(ctrl_tab(&cfg), CtrlTab::Sound);
        cfg.set("controls_tab", "nonsense");
        assert_eq!(ctrl_tab(&cfg), CtrlTab::Practise);
        // the old folds don't matter
        let mut old = Config::default();
        old.set("panel_open", serde_json::json!(["PART", "SOUND"]));
        assert_eq!(ctrl_tab(&old), CtrlTab::Practise);
    }
}
