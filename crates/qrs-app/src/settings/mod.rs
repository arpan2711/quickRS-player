//! Settings (F2, or the Settings strip on the home page): every setting of the app in one panel floating over the
//! page you were on, in sections (port of rsplay/settingsview.py; the Audio page is new, on `qrs_audio::devices`).
//!
//!     Audio         the guitar input and the output (automatic, or a device you pick), the guitar's channel with
//!                   a meter each, hearing the guitar, latency, volumes, the Rocksmith 2014 setup
//!     Guitar tone   As is / Clean / Crunch / Lead / Bass / Acoustic / Custom; the custom tone as amp knobs;
//!                   saved tones
//!     Display       theme and knobs, string colours, note-tile instrument, notation, the tiles and their order
//!     Practice      level-up in loops, audio sync and guitar timing
//!     Library       song folders, rescan, psarc or feedpak, expand & preview in the list
//!
//! Everything is stored in config.json straight away; an open song picks the changes up
//! (`PlayerView::reload_settings`). The keys shown in the panel work here too (T, N, Y, 7 8 9 0, M, the sync keys,
//! F5). Esc, Done or a click outside the panel closes it.
//!
//! The layout is the Python app's in its whole pixels (`qrs_ui::units`): `px(n)` where it has `int(n * s)`, `snap`
//! where it truncates a position or halves a length (`a // 2`), pygame's `Rect` arithmetic spelled out where it
//! matters (`centre_x`, `centre_y`, `centred`, `inflate`).

mod audio;
mod pages;
mod tone;

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

pub const SECTIONS: [(&str, &str); 5] = crate::home::SECTIONS;

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
}

impl SettingsView {
    pub fn new(section: &str) -> SettingsView {
        SettingsView {
            section: SECTIONS
                .iter()
                .find(|s| s.0 == section)
                .map(|s| s.0)
                .unwrap_or("audio"),
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
        }
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), Instant::now()));
    }

    /// Show another section.
    pub fn show(&mut self, section: &str) {
        if let Some(s) = SECTIONS.iter().find(|s| s.0 == section) {
            self.section = s.0;
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
        match ev {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let act = match key {
                    Key::Escape | Key::Backspace => {
                        self.closed = true;
                        None
                    }
                    Key::ArrowUp | Key::ArrowDown | Key::Tab => {
                        let n = SECTIONS.len();
                        let i = SECTIONS.iter().position(|s| s.0 == self.section).unwrap_or(0);
                        let back = *key == Key::ArrowUp || modifiers.shift;
                        self.section = SECTIONS[(i + if back { n - 1 } else { 1 }) % n].0;
                        self.scroll = 0.0;
                        None
                    }
                    Key::Comma | Key::Period => {
                        let k = if modifiers.command {
                            "input_offset"
                        } else {
                            "av_offset"
                        };
                        Some(format!("ms:{k}:{}", if *key == Key::Period { 10 } else { -10 }))
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
            "theme" => st.apply_theme(ThemeName::parse(arg)),
            "knobs" => {
                st.config.set("knobs", arg);
                knobs::set_style(KnobStyle::parse(arg));
                st.save_config();
            }
            "input" | "output" | "ch" | "buffer" | "exclusive" | "rocksmith" | "devrescan" => {
                self.audio_act(st, verb, arg)
            }
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
                self.tone_act(st, verb, arg);
                self.changed(st, player);
            }
            _ => {}
        }
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
        // modal: a click outside the panel closes it
        let outside = interact(ui, area, "settings-modal", Sense::click());
        if outside.clicked()
            && outside
                .interact_pointer_pos()
                .is_some_and(|pos| !panel.contains(pos))
            && self.naming.is_none()
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
        let dw = widgets::button_width(&ctx, "Done", "Esc", 15.0, false) + px(8.0);
        let done = Rect::from_min_size(pos2(head.max.x - dw, head.min.y), vec2(dw, head.height()));
        if self.naming.is_none()
            && widgets::button(
                ui,
                done,
                "set-done",
                "Done",
                "Esc",
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
        text::draw(
            &p,
            "Changes are saved straight away",
            pos2(head.min.x + nav_w + px(28.0), centre_y(head)),
            Align2::LEFT_CENTER,
            Style::new(13.0, 500, th.caption),
            Some(done.min.x - head.min.x - nav_w - px(48.0)),
        );
        let n = SECTIONS.len() as f32;
        let nav = Rect::from_min_size(
            pos2(head.min.x, head.max.y + px(24.0)),
            vec2(nav_w, 2.0 * px(10.0) + n * px(48.0) + (n - 1.0) * px(6.0)),
        );
        paint::well(&p, nav, 22.0);
        let mut y = nav.min.y + px(10.0);
        for (key, label) in SECTIONS {
            let r = Rect::from_min_size(
                pos2(nav.min.x + px(10.0), y),
                vec2(nav.width() - px(20.0), px(48.0)),
            );
            let on = key == self.section;
            if on {
                paint::raised(&p, r, Some(14.0), 4.0, Fill::Base, 0.0);
            }
            let resp = interact(ui, r, ("set-nav", key), Sense::click());
            let hot = resp.hovered() && self.naming.is_none();
            if hot {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() && self.naming.is_none() {
                self.acts.push(format!("section:{key}"));
            }
            text::draw(
                &p,
                label,
                pos2(r.min.x + px(16.0), centre_y(r)),
                Align2::LEFT_CENTER,
                Style::new(
                    15.0,
                    if on { 800 } else { 600 },
                    if on {
                        th.accent_ink
                    } else if hot {
                        th.text
                    } else {
                        th.text_2
                    },
                ),
                None,
            );
            y += px(54.0);
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
            if self.naming.is_some() {
                child.disable();
            }
            match self.section {
                "audio" => self.draw_audio(&mut child, st, body),
                "tone" => self.draw_tone(&mut child, st, body),
                "display" => self.draw_display(&mut child, st, body),
                "practice" => self.draw_practice(&mut child, st, body),
                _ => self.draw_library(&mut child, st, body),
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
        if self.knob_drag.is_some() {
            ctx.set_cursor_icon(CursorIcon::ResizeVertical);
        }
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
                Style::new(12.0, 500, th.caption),
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
