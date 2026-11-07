//! What the player's shortcuts are doing, shown (owner request, 2026-10-25; new in the Rust port): a state that
//! takes something away glows red (the guitar muted, a take recording), one changed from normal glows amber (speed,
//! difficulty, transpose, loop, tone, sync); a one-off key flashes its keycap with what it did; and the Shortcuts
//! sheet lists every key.

use eframe::egui;
use egui::{Align2, Color32, Key, Modifiers, Painter, Pos2, Rect, pos2, vec2};
use qrs_ui::icons::Icon;
use qrs_ui::keys::{CapOn, Gem, keycap, keycap_width};
use qrs_ui::material::Mat;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::RoundKind;

/// How a state is lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lit {
    /// Changed from normal.
    Amber,
    /// Something is missing (the guitar muted, recording).
    Red,
}

impl Lit {
    pub fn color(self) -> Color32 {
        let th = t();
        match self {
            Lit::Amber => th.accent_ink,
            Lit::Red => th.miss,
        }
    }
}

fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0).round() as u8)
}

/// A glow round a knob (`d` its diameter, points) and a thin ring at its edge.
pub(super) fn knob_glow(p: &Painter, center: Pos2, d: f32, lit: Lit) {
    let c = lit.color();
    let r = Rect::from_center_size(center, vec2(d, d));
    paint::glow(p, r, c, 16.0, Some(0.75), Some(d / 2.0));
    paint::ring(p, r, Some(d / 2.0), 2.0, c, true);
}

/// A faint frame round a control that is lit red or amber (the muted GUITAR knob with its name).
pub(super) fn frame(p: &Painter, rect: Rect, lit: Lit) {
    let c = lit.color();
    p.rect_filled(rect, px(12.0), alpha(c, 0.07));
    paint::ring(p, rect, Some(12.0), 1.5, alpha(c, 0.55), false);
}

const CHIP_SIZE: f32 = 12.0;

/// The rec key's action: R's (record what you hear, pause, resume).
pub(super) const REC_ACT: &str = "rec:press";

fn chip_style(lit: Lit) -> Style {
    Style::new(CHIP_SIZE, 800, lit.color())
}

/// The width of a chip of `label`.
pub(super) fn chip_width(ctx: &egui::Context, label: &str) -> f32 {
    text::width(ctx, label, chip_style(Lit::Amber)) + px(18.0)
}

/// A small pill of `label`, tinted and glowing in its colour, its left middle at `at`. Returns its rect.
pub(super) fn chip(p: &Painter, label: &str, at: Pos2, lit: Lit) -> Rect {
    let c = lit.color();
    let w = chip_width(p.ctx(), label);
    let h = px(20.0);
    let r = Rect::from_min_size(pos2(at.x, at.y - h / 2.0), vec2(w, h));
    paint::glow(p, r, c, 8.0, Some(0.3), Some(h / 2.0));
    p.rect_filled(r, h / 2.0, alpha(c, 0.16));
    paint::ring(p, r, Some(h / 2.0), 1.0, alpha(c, 0.65), false);
    text::draw(
        p,
        label,
        pos2(r.min.x + px(9.0), r.center().y),
        Align2::LEFT_CENTER,
        chip_style(lit),
        None,
    );
    r
}

/// The keys of a press as keycap labels: modifiers first ("Ctrl", "Shift", "Alt"), then the key.
pub(super) fn key_labels(key: Key, m: Modifiers) -> Vec<String> {
    let mut out = Vec::new();
    if m.command {
        out.push("Ctrl".to_string());
    }
    if m.alt {
        out.push("Alt".to_string());
    }
    let name = key_name(key);
    // a shifted symbol names itself
    if m.shift && !matches!(key, Key::Questionmark | Key::Plus) {
        out.push("Shift".to_string());
    }
    out.push(name);
    out
}

fn key_name(key: Key) -> String {
    match key {
        Key::ArrowLeft => "←".into(),
        Key::ArrowRight => "→".into(),
        Key::ArrowUp => "↑".into(),
        Key::ArrowDown => "↓".into(),
        Key::Minus => "−".into(),
        Key::Equals => "=".into(),
        Key::Plus => "+".into(),
        Key::PageUp => "PgUp".into(),
        Key::PageDown => "PgDn".into(),
        Key::OpenBracket => "[".into(),
        Key::CloseBracket => "]".into(),
        Key::Backslash => "\\".into(),
        Key::Comma => ",".into(),
        Key::Period => ".".into(),
        Key::Questionmark => "?".into(),
        Key::Slash => "/".into(),
        Key::Escape => "Esc".into(),
        Key::Backspace => "⌫".into(),
        k => k.name().to_string(),
    }
}

/// The keycap flash: the keys pressed, then what they did (`tail` in the accent), on a raised plate centred at
/// `center`.
pub(super) fn flash(p: &Painter, keys: &[String], msg: &str, tail: &str, center: Pos2) {
    let th = t();
    let ctx = p.ctx().clone();
    let st = Style::new(15.0, 800, th.text);
    let cap = 12.0;
    let gap = px(6.0);
    let caps_w: f32 = keys.iter().map(|k| keycap_width(&ctx, k, cap) + gap).sum();
    let msg_w = text::width(&ctx, msg, st);
    let tail_w = if tail.is_empty() {
        0.0
    } else {
        text::width(&ctx, tail, st)
    };
    let w = (px(20.0) + caps_w + px(4.0) + msg_w + tail_w + px(20.0)).round();
    let h = px(46.0);
    let r = Rect::from_center_size(center, vec2(w, h));
    paint::raised(p, r, Some(14.0), 4.0, Fill::Base, 0.0);
    let mut x = r.min.x + px(20.0);
    let cy = r.center().y;
    for k in keys {
        let kr = keycap(p, k, pos2(x, cy), Align2::LEFT_CENTER, cap, CapOn::Material);
        x = kr.max.x + gap;
    }
    x += px(4.0);
    let mr = text::draw(p, msg, pos2(x, cy), Align2::LEFT_CENTER, st, None);
    if !tail.is_empty() {
        text::draw(
            p,
            tail,
            pos2(mr.max.x, cy),
            Align2::LEFT_CENTER,
            Style {
                color: th.accent_ink,
                ..st
            },
            None,
        );
    }
}

/// Every key of the player, by group, for the Shortcuts sheet: (group, [(keys, what they do)]). Keys are split on
/// spaces into caps; "/" between alternatives stays text.
pub(super) const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Playback",
        &[
            ("Space", "Play, pause"),
            ("← →", "Back, forward (held: further)"),
            ("Shift ← →", "Back, forward a second"),
            ("−", "Rewind 5 seconds"),
            ("Home", "Restart"),
            ("PgUp PgDn", "Previous, next section"),
            ("Esc", "Back to the library"),
        ],
    ),
    (
        "Practice",
        &[
            ("= / Ctrl =", "Faster, slower"),
            ("Ctrl ↑ ↓", "Harder, easier"),
            ("L", "Loop this section"),
            ("[ ]", "Loop start, loop end"),
            ("\\", "Clear the loop"),
            ("Ctrl S", "Save the loop as a lick"),
            ("E", "Review the run: spots to practise"),
            ("O", "Continuous loop: every pass scored"),
            ("W", "Note by note"),
            ("Enter", "Skip the note it waits for"),
            ("H", "Ear mode (notes hidden)"),
            ("Ctrl R / B", "Loop take (scored), listen back"),
            ("Shift C", "Count-in (3, 2, 1) on or off"),
        ],
    ),
    (
        "Sound",
        &[
            ("M", "Hear your instrument, or mute it"),
            ("G", "Synth or real guitar in the song"),
            ("T", "Next tone"),
            ("Shift T", "Your sounds (on the Rig)"),
            ("7 8", "Your instrument quieter, louder"),
            ("9 0", "Song quieter, louder"),
            ("Shift ↑ ↓", "Transpose the guitar input; backing track stays put"),
            (", .", "Sync offset"),
            ("Ctrl , .", "Your instrument's delay: shorter, longer"),
            ("R", "Record, pause, resume"),
            ("Shift R", "Stop and save the recording"),
        ],
    ),
    (
        "Parts and view",
        &[
            ("A / Shift A", "Next, previous part"),
            ("1–6", "A part"),
            ("V", "Tab or highway"),
            ("F", "Full-screen tab"),
            ("C", "Compact rows"),
            ("K", "Chord names: charted or in key"),
            ("I", "Highway: low E at the bottom"),
            ("D", "Highway look: Live, Still, Plain"),
            ("N", "Rhythm notation"),
            ("Y", "Lyrics tile: fold or open"),
            ("↑ ↓", "Zoom"),
            ("Alt ← →", "Bar width"),
            ("Alt ↑ ↓", "String spacing"),
            ("Alt 0", "Tab shape back to normal"),
        ],
    ),
    (
        "A selection (Ctrl-drag on the tab)",
        &[
            ("Space", "Loop it with the song"),
            ("P", "Hear it alone"),
            ("S", "Hear it slowly"),
            ("Esc", "Clear it (and its loop)"),
        ],
    ),
    ("This list", &[("F1 / ?", "Show or hide it")]),
];

/// The Shortcuts sheet's size for `area`: three columns, as tall as it needs.
fn sheet_rect(ctx: &egui::Context, area: Rect) -> (Rect, f32) {
    let row = text::line_height(ctx, Style::new(13.0, 600, Color32::WHITE)) + px(9.0);
    let col_h = |g: &[(&str, &[(&str, &str)])]| -> f32 {
        g.iter().map(|(_, rows)| px(34.0) + rows.len() as f32 * row).sum()
    };
    let cols = columns();
    let h = cols.iter().map(|c| col_h(c)).fold(0.0, f32::max) + px(96.0);
    let w = (area.width() - px(80.0)).min(px(1080.0));
    (
        Rect::from_center_size(area.center(), vec2(w, h.min(area.height() - px(40.0)))),
        row,
    )
}

/// The groups in three columns.
fn columns() -> [&'static [(&'static str, &'static [(&'static str, &'static str)])]; 3] {
    [&SHORTCUTS[0..2], &SHORTCUTS[2..3], &SHORTCUTS[3..]]
}

/// The Shortcuts sheet over `area` (the page dimmed under it). Returns the card's rect (a click outside closes it).
pub(super) fn draw_shortcuts(p: &Painter, area: Rect) -> Rect {
    let th = t();
    let ctx = p.ctx().clone();
    paint::dim(p, area, None);
    let (card, row) = sheet_rect(&ctx, area);
    paint::card(p, card, 24.0, true);
    let pad = px(28.0);
    text::draw(
        p,
        "Keyboard shortcuts",
        pos2(card.min.x + pad, card.min.y + pad),
        Align2::LEFT_TOP,
        Style::new(22.0, 800, th.text),
        None,
    );
    text::draw(
        p,
        "F1 or ? closes this · so does Esc or a click outside",
        pos2(card.max.x - pad, card.min.y + pad + px(6.0)),
        Align2::RIGHT_TOP,
        Style::new(12.0, 600, th.caption),
        None,
    );
    let gap = px(28.0);
    let col_w = (card.width() - 2.0 * pad - 2.0 * gap) / 3.0;
    let keys_w = px(104.0);
    let top = card.min.y + pad + px(48.0);
    let clip = p.with_clip_rect(card.shrink(px(4.0)));
    for (ci, groups) in columns().iter().enumerate() {
        let x = card.min.x + pad + ci as f32 * (col_w + gap);
        let mut y = top;
        for (name, rows) in groups.iter() {
            text::caps(
                &clip,
                name,
                pos2(x, y),
                11.0,
                th.caption,
                Align2::LEFT_TOP,
                Some(col_w),
            );
            y += px(22.0);
            for (keys, what) in rows.iter() {
                let cy = y + row / 2.0 - px(4.0);
                let mut kx = x;
                for part in keys.split(' ') {
                    if part == "/" {
                        let r = text::draw(
                            &clip,
                            "/",
                            pos2(kx, cy),
                            Align2::LEFT_CENTER,
                            Style::new(12.0, 600, th.caption),
                            None,
                        );
                        kx = r.max.x + px(5.0);
                    } else {
                        let r = keycap(
                            &clip,
                            part,
                            pos2(kx, cy),
                            Align2::LEFT_CENTER,
                            11.0,
                            CapOn::Material,
                        );
                        kx = r.max.x + px(4.0);
                    }
                }
                text::draw(
                    &clip,
                    what,
                    pos2(x + keys_w.max(kx - x + px(6.0)), cy),
                    Align2::LEFT_CENTER,
                    Style::new(13.0, 600, th.text),
                    Some(col_w - keys_w),
                );
                y += row;
            }
            y += px(12.0);
        }
    }
    card
}

impl super::PlayerView {
    /// The lamps' row over the transport (P37.11, owner request): note-for-note, loop, ear, take, count-in,
    /// synth / real, rec and review (and the recording's badge while R records), centred under the tab and kept
    /// between the footer's left and the tab's right; names dropped (the lamps and keys alone, their tips saying
    /// what they are) when the window is too narrow for them. Returns where the row ends.
    pub(super) fn draw_lamps_row(
        &mut self,
        ui: &egui::Ui,
        well: Rect,
        cradle: Rect,
        foot: Rect,
        rec: Option<crate::recording::Status>,
    ) -> f32 {
        use super::footer_keys::{Item, row_layout};
        let ctx = ui.ctx().clone();
        let y = cradle.min.y - px(9.0);
        let mut items = Item::ROW.to_vec();
        if let Some(s) = rec {
            let (short, whole) = crate::recording::badge_widths(ui, s);
            items.push(Item::Badge(whole, short));
        }
        let (slots, compact) = row_layout(
            &ctx,
            &items,
            well.center().x,
            foot.min.x,
            well.max.x.min(foot.max.x),
        );
        let mut end = foot.min.x;
        for (item, (x, w)) in items.iter().zip(slots) {
            match item {
                Item::Nbn => self.draw_nbn_lamp(ui, x, w, y, compact, foot.min.x),
                Item::Loop | Item::Ear | Item::Take | Item::Count => {
                    self.draw_lamp(ui, *item, x, w, y, compact)
                }
                Item::Synth => self.draw_synth_key(ui, x, w, y, compact),
                Item::Rec => self.draw_rec_key(ui, x, w, y, compact, rec),
                Item::Review => self.draw_review_key(ui, x, w, y, compact),
                Item::Badge(whole, _) => {
                    if let Some(s) = rec
                        && crate::recording::badge_sized(ui, pos2(x, y), s, w < *whole - 0.5).clicked()
                    {
                        self.acts.push("rec:stop".into());
                    }
                }
            }
            end = x + w;
        }
        end
    }

    /// One of the row's mode lamps (loop, ear, take, count-in) in its slot: its name, then a jewel lamp at the
    /// slot's right; lit ones glow and their names light up; a click switches it.
    fn draw_lamp(
        &mut self,
        ui: &egui::Ui,
        item: super::footer_keys::Item,
        x: f32,
        w: f32,
        y: f32,
        compact: bool,
    ) {
        use super::footer_keys::{Item, ROW_HALF};
        let th = t();
        let p = ui.painter().clone();
        let looping = self.range().is_some();
        let recording = self.take.as_ref().is_some_and(|s| s.recording());
        let (label, on, lit, act, tip, key): (&str, bool, Lit, &str, String, &str) = match item {
            Item::Loop => (
                "loop",
                looping,
                Lit::Amber,
                if looping { "clear_loop" } else { "loop_section" },
                if looping {
                    "Loop: clear it".into()
                } else {
                    "Loop this section (or Ctrl-drag across the tab)".into()
                },
                if looping { "\\" } else { "L" },
            ),
            Item::Ear => (
                "ear",
                self.ear,
                Lit::Amber,
                "ear",
                "Ear mode: notes hidden until you play them".into(),
                "H",
            ),
            Item::Take => (
                "take",
                recording,
                Lit::Red,
                "take:record",
                "Take: record the loop once, scored".into(),
                "Ctrl R",
            ),
            _ => (
                "count-in",
                self.countdown_on,
                Lit::Amber,
                "countdown",
                if self.countdown_on {
                    "Count-in on: 3, 2, 1 before the song, a loop, a review spot or a take  \u{b7}  click: off \
                     everywhere"
                        .into()
                } else {
                    "Count-in off: everything plays at once  \u{b7}  click: 3, 2, 1 everywhere again".into()
                },
                "Shift C",
            ),
        };
        let c = pos2(x + w - px(8.0), y);
        let hit = Rect::from_min_max(
            pos2(x - px(2.0), y - px(ROW_HALF)),
            pos2(x + w + px(2.0), y + px(ROW_HALF)),
        );
        let resp = qrs_ui::widgets::interact(ui, hit, ("mode-lamp", label), egui::Sense::click());
        let hot = resp.hovered();
        qrs_ui::keys::jewel_lamp(&p, c, 12.0, if on { 1.0 } else { 0.0 }, hot);
        if on {
            paint::glow(
                &p,
                Rect::from_center_size(c, vec2(px(12.0), px(12.0))),
                lit.color(),
                10.0,
                Some(0.6),
                None,
            );
        }
        if !compact {
            let col = if on {
                lit.color()
            } else if hot {
                th.text
            } else {
                th.caption
            };
            text::draw(
                &p,
                label,
                pos2(c.x - px(8.0) - px(7.0), y),
                Align2::RIGHT_CENTER,
                Style {
                    color: col,
                    weight: if on { 800 } else { 500 },
                    ..Style::new(12.0, 500, th.caption)
                },
                None,
            );
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            self.tip = Some((tip, key.into(), pos2(x, hit.min.y - px(4.0)), Align2::LEFT_BOTTOM));
        }
        if resp.clicked() {
            self.acts.push(act.into());
        }
    }

    /// The synth guitar's key (G, P23.11) in its slot: "synth / real", the one playing lit, then an abalone key
    /// with the synth's sine (on) or the song's waveform (off).
    fn draw_synth_key(&mut self, ui: &egui::Ui, x: f32, w: f32, y: f32, compact: bool) {
        use super::footer_keys::ROW_HALF;
        use super::songsynth::{How, Status};
        let th = t();
        let p = ui.painter().clone();
        let status = self.song_synth_status();
        let on = status != Status::Off;
        let name = Style::new(12.0, 500, th.caption);
        let lit = Style {
            color: Lit::Amber.color(),
            weight: 800,
            ..name
        };
        // while it's being prepared, "synth" breathes
        let synth_st = match status {
            Status::Preparing(_) => {
                let now = ui.ctx().input(|i| i.time);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(60));
                let a = 0.45 + 0.55 * (now * std::f64::consts::TAU / 1.4).sin().abs() as f32;
                Style {
                    color: alpha(lit.color, a),
                    ..lit
                }
            }
            Status::On => lit,
            Status::Off => name,
        };
        let real_st = if on {
            name
        } else {
            Style {
                color: th.text,
                weight: 800,
                ..name
            }
        };
        let ctx = ui.ctx().clone();
        let d = px(20.0);
        let key = Rect::from_center_size(pos2(x + w - d / 2.0, y), vec2(d, d));
        let hit = Rect::from_min_max(
            pos2(x - px(2.0), y - px(ROW_HALF)),
            pos2(x + w + px(2.0), y + px(ROW_HALF)),
        );
        let resp = qrs_ui::widgets::interact(ui, hit, "song-synth", egui::Sense::click());
        let hot = resp.hovered();
        if !compact {
            let w1 = text::width(&ctx, "synth", synth_st);
            let ws = text::width(&ctx, " / ", name);
            let w2 = text::width(&ctx, "real", real_st);
            let x0 = key.min.x - px(7.0) - (w1 + ws + w2);
            text::draw(&p, "synth", pos2(x0, y), Align2::LEFT_CENTER, synth_st, None);
            text::draw(&p, " / ", pos2(x0 + w1, y), Align2::LEFT_CENTER, name, None);
            text::draw(
                &p,
                "real",
                pos2(x0 + w1 + ws, y),
                Align2::LEFT_CENTER,
                real_st,
                None,
            );
        }
        if on {
            paint::glow(&p, key, Lit::Amber.color(), 10.0, Some(0.45), Some(d / 2.0));
        }
        qrs_ui::widgets::draw_round_button(
            &p,
            key,
            if on { Icon::Synth } else { Icon::Track },
            13.0,
            RoundKind::Mat(Mat::Abalone, 0, None),
            on,
            hot,
            resp.is_pointer_button_down_on(),
        );
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let part = self.part_name.clone();
            let tip = match status {
                Status::Preparing(f) => format!(
                    "Preparing the synth guitar for the {part}\u{2026} {}%",
                    (f * 100.0).round() as i64
                ),
                Status::Off => format!(
                    "Real guitar: the song as recorded  \u{b7}  click: the synth plays the {part} in its place"
                ),
                Status::On => match self.synth_how() {
                    (How::Stem, stems) => format!(
                        "Synth guitar: the {part} on the synth in place of the {} stem  \u{b7}  click: the real one",
                        stems.join(" and ")
                    ),
                    (How::Over, _) => format!(
                        "Synth guitar: the {part} on the synth over the song, turned down (its guitar can't be \
                         taken out)  \u{b7}  click: the real one"
                    ),
                },
            };
            self.tip = Some((tip, "G".into(), pos2(x, hit.min.y - px(4.0)), Align2::LEFT_BOTTOM));
        }
        if resp.clicked() {
            self.acts.push("song_synth".into());
        }
    }

    /// The rec key (P23.11) in its slot: "rec" and a garnet key that does what R does (record what you hear,
    /// pause, resume); a ruby lit while recording.
    fn draw_rec_key(
        &mut self,
        ui: &egui::Ui,
        x: f32,
        w: f32,
        y: f32,
        compact: bool,
        rec: Option<crate::recording::Status>,
    ) {
        use super::footer_keys::ROW_HALF;
        let th = t();
        let p = ui.painter().clone();
        let recording = rec.is_some_and(|s| !s.paused);
        let paused = rec.is_some_and(|s| s.paused);
        let name = Style::new(12.0, 500, th.caption);
        let st = if recording {
            Style {
                color: Lit::Red.color(),
                weight: 800,
                ..name
            }
        } else if paused {
            Style {
                color: Lit::Amber.color(),
                weight: 800,
                ..name
            }
        } else {
            name
        };
        let d = px(18.0);
        let key = Rect::from_center_size(pos2(x + w - d / 2.0, y), vec2(d, d));
        let hit = Rect::from_min_max(
            pos2(x - px(2.0), y - px(ROW_HALF)),
            pos2(x + w + px(2.0), y + px(ROW_HALF)),
        );
        let resp = qrs_ui::widgets::interact(ui, hit, "rec-key", egui::Sense::click());
        let hot = resp.hovered();
        let st = if hot && !recording && !paused {
            Style { color: th.text, ..st }
        } else {
            st
        };
        if !compact {
            text::draw(
                &p,
                "rec",
                pos2(key.min.x - px(7.0), y),
                Align2::RIGHT_CENTER,
                st,
                None,
            );
        }
        if recording {
            paint::glow(&p, key, Lit::Red.color(), 10.0, Some(0.6), Some(d / 2.0));
        }
        let down = hot && resp.is_pointer_button_down_on();
        let face = if hot && !down {
            key.translate(vec2(0.0, -1.0))
        } else {
            key
        };
        qrs_ui::keys::gem_key(&p, face, if recording { Gem::Ruby } else { Gem::Garnet }, down);
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let tip = if recording {
                "Pause the recording  \u{b7}  Shift R stops and saves it"
            } else if paused {
                "Resume the recording  \u{b7}  Shift R stops and saves it"
            } else {
                "Record what you hear (the song and you) to a WAV"
            };
            self.tip = Some((
                tip.into(),
                "R".into(),
                pos2(x, hit.min.y - px(4.0)),
                Align2::LEFT_BOTTOM,
            ));
        }
        if resp.clicked() {
            self.acts.push(REC_ACT.into());
        }
    }

    /// A keycap flash for a key whose action said nothing itself (the rest make their own toast, which flashes).
    pub(super) fn flash_for(&mut self, name: &str) {
        let (msg, tail) = match name {
            "back1" => ("Back ", "1 s".to_string()),
            "fwd1" => ("Forward ", "1 s".to_string()),
            "start" | "prev_section" => ("Restart", String::new()),
            "zoom_in" | "zoom_out" => ("Zoom ", format!("{}%", (self.tab.zoom * 100.0).round() as i64)),
            _ => return,
        };
        self.say(msg, tail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_as_caps() {
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        let ctrl = Modifiers {
            command: true,
            ..Default::default()
        };
        assert_eq!(key_labels(Key::ArrowUp, shift), ["Shift", "↑"]);
        assert_eq!(key_labels(Key::Equals, ctrl), ["Ctrl", "="]);
        assert_eq!(key_labels(Key::M, Modifiers::default()), ["M"]);
        assert_eq!(key_labels(Key::PageDown, Modifiers::default()), ["PgDn"]);
    }

    #[test]
    fn every_group_has_a_column() {
        let n: usize = columns().iter().map(|c| c.len()).sum();
        assert_eq!(n, SHORTCUTS.len());
    }
}
