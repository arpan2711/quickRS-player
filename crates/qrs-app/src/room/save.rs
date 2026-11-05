//! The save sheet (Room-Save): the sound's name, new or over the one it is, what it's based on, tags, a colour, a
//! favourite slot, and what gets saved. The room keeps sounding while you type.

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Rect, Sense, Stroke, Ui, pos2, vec2};
use qrs_audio::board::Board;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::draw::{self, KeyLook};
use super::rigs::{self, COLOURS, Rig, SCENE_COLOURS, TAGS};
use crate::ampbutton::rgb;
use crate::pedalfaces as faces;
use crate::state::AppState;

pub enum Act {
    None,
    Cancel,
    Save,
    /// The name box was clicked (typing goes there).
    Type,
}

pub struct Sheet {
    pub name: String,
    /// Save over `base` (the saved sound the rig is) rather than as a new one.
    pub overwrite: bool,
    pub base: Option<Rig>,
    pub based_on: String,
    pub tags: Vec<String>,
    pub color: usize,
    pub slot: u8,
    board: Board,
    taken: Vec<String>,
}

impl Sheet {
    pub fn new(title: &str, base: Option<Rig>, based_on: &str, saved: &[Rig], board: &Board) -> Sheet {
        let name = match &base {
            Some(_) => rigs::fresh_name(saved, title),
            None => title.trim_start_matches("Shuffle: ").to_string(),
        };
        let mut tags: Vec<String> = base.as_ref().map(|b| b.tags.clone()).unwrap_or_default();
        if tags.is_empty() {
            // a character from the rig, to start with
            let drive = board.blocks.iter().any(|b| {
                b.on && matches!(
                    b.def().kind,
                    qrs_audio::board::Kind::Drive | qrs_audio::board::Kind::Fuzz
                )
            });
            tags.push(if drive { "Crunch" } else { "Clean" }.into());
        }
        Sheet {
            name,
            overwrite: false,
            color: base.as_ref().map(|b| b.color).unwrap_or(2),
            slot: 0,
            based_on: base
                .as_ref()
                .map(|b| {
                    if b.based_on.is_empty() {
                        b.name.clone()
                    } else {
                        b.based_on.clone()
                    }
                })
                .unwrap_or_else(|| based_on.to_string()),
            base,
            tags,
            board: board.clone(),
            taken: saved.iter().map(|r| r.name.to_lowercase()).collect(),
        }
    }

    pub fn ui(&mut self, ui: &Ui, _st: &mut AppState, area: Rect) -> Act {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let mut act = Act::None;
        paint::dim(&p, area, Some(150));
        let w = px(750.0).min(area.width() - px(40.0));
        let h = px(712.0).min(area.height() - px(40.0));
        let panel = Rect::from_center_size(area.center(), vec2(w, h));
        // a click outside: nothing (it's a sheet, Esc or Cancel closes it)
        interact(ui, area, "room-save-back", Sense::click());
        // (opaque: the room mustn't show through a dialog)
        p.rect_filled(panel, px(24.0), t().base);
        paint::floating(&p, panel, Some(px(24.0)));
        let x0 = panel.min.x + px(30.0);
        let x1 = panel.max.x - px(30.0);
        let mut y = panel.min.y + px(30.0);
        draw::caps(
            &p,
            "SAVE SOUND",
            pos2(x0, y),
            10.5,
            th.caption,
            Align2::LEFT_CENTER,
        );
        draw::label(
            &p,
            "Save your rig",
            pos2(x0, y + px(28.0)),
            24.0,
            800,
            th.text,
            Align2::LEFT_CENTER,
        );
        let close = Rect::from_min_size(pos2(x1 - px(38.0), y - px(4.0)), vec2(px(38.0), px(38.0)));
        if draw::key(ui, close, "room-save-close", KeyLook::Dark, "\u{2715}", "", None).clicked() {
            act = Act::Cancel;
        }
        draw::label(
            &p,
            "the room keeps sounding while you type",
            pos2(close.min.x - px(12.0), y + px(16.0)),
            11.5,
            600,
            th.caption,
            Align2::RIGHT_CENTER,
        );
        // the name
        y += px(66.0);
        draw::caps(&p, "NAME", pos2(x0, y), 10.5, th.caption, Align2::LEFT_CENTER);
        let nr = Rect::from_min_size(pos2(x0, y + px(12.0)), vec2(x1 - x0, px(48.0)));
        let resp = interact(ui, nr, "room-save-name", Sense::click());
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::Text);
        }
        if resp.clicked() {
            act = Act::Type;
        }
        paint::inset(&p, nr, Some(px(14.0)), 2.0, Fill::Base);
        p.rect_stroke(
            nr,
            px(14.0),
            Stroke::new(2.0, th.accent),
            egui::StrokeKind::Inside,
        );
        let tr = draw::label_w(
            &p,
            &self.name,
            pos2(nr.min.x + px(16.0), nr.center().y),
            18.0,
            800,
            th.text,
            Align2::LEFT_CENTER,
            nr.width() - px(90.0),
        );
        draw::caret(
            &p,
            if self.name.is_empty() {
                nr.min.x + px(16.0)
            } else {
                tr.max.x
            },
            nr.center().y,
            px(22.0),
            th.accent,
        );
        draw::label(
            &p,
            &format!("{} / 40", self.name.chars().count()),
            pos2(nr.max.x - px(14.0), nr.center().y),
            11.0,
            700,
            th.caption,
            Align2::RIGHT_CENTER,
        );
        let clash = !self.overwrite
            && self.taken.contains(&self.name.trim().to_lowercase())
            && self
                .base
                .as_ref()
                .is_none_or(|b| !b.name.eq_ignore_ascii_case(self.name.trim()));
        // new or overwrite
        y = nr.max.y + px(14.0);
        let half = (x1 - x0 - px(12.0)) / 2.0;
        let cards: Vec<(bool, String, String)> = match &self.base {
            Some(b) => vec![
                (
                    false,
                    "Save as a new sound".into(),
                    format!("\u{2018}{}\u{2019} stays as it was", b.name),
                ),
                (
                    true,
                    format!("Overwrite \u{2018}{}\u{2019}", b.name),
                    "replaces it \u{b7} Undo still has the rig".into(),
                ),
            ],
            None => vec![(
                false,
                "Save as a new sound".into(),
                if clash {
                    "a sound has that name: saving replaces it".into()
                } else {
                    "it joins your sounds".into()
                },
            )],
        };
        for (i, (ow, head, sub)) in cards.iter().enumerate() {
            let cr = Rect::from_min_size(pos2(x0 + i as f32 * (half + px(12.0)), y), vec2(half, px(58.0)));
            let on = self.overwrite == *ow;
            let resp = interact(ui, cr, ("room-save-mode", i), Sense::click());
            if on {
                p.rect_filled(cr, px(14.0), th.accent.gamma_multiply(0.08));
                p.rect_stroke(
                    cr,
                    px(14.0),
                    Stroke::new(1.5, th.accent),
                    egui::StrokeKind::Inside,
                );
            } else {
                paint::inset(&p, cr, Some(px(14.0)), 1.5, Fill::Base);
            }
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            let rc = pos2(cr.min.x + px(24.0), cr.center().y);
            p.circle_stroke(
                rc,
                px(8.0),
                Stroke::new(1.5, if on { th.accent } else { th.caption }),
            );
            if on {
                p.circle_filled(rc, px(4.5), th.accent);
            }
            draw::label_w(
                &p,
                head,
                pos2(rc.x + px(22.0), cr.center().y - px(9.0)),
                14.0,
                800,
                th.text,
                Align2::LEFT_CENTER,
                cr.width() - px(60.0),
            );
            draw::label_w(
                &p,
                sub,
                pos2(rc.x + px(22.0), cr.center().y + px(10.0)),
                11.5,
                600,
                th.caption,
                Align2::LEFT_CENTER,
                cr.width() - px(60.0),
            );
            if resp.clicked() {
                self.overwrite = *ow;
                if *ow && let Some(b) = &self.base {
                    self.name = b.name.clone();
                }
            }
        }
        // based on
        y += px(72.0);
        let br = Rect::from_min_size(pos2(x0, y), vec2(x1 - x0, px(46.0)));
        paint::inset(&p, br, Some(px(14.0)), 1.5, Fill::Base);
        draw::caps(
            &p,
            "BASED ON",
            pos2(br.min.x + px(16.0), br.min.y + px(14.0)),
            9.5,
            th.caption,
            Align2::LEFT_CENTER,
        );
        draw::label_w(
            &p,
            if self.based_on.is_empty() {
                "your own rig"
            } else {
                &self.based_on
            },
            pos2(br.min.x + px(16.0), br.min.y + px(32.0)),
            13.0,
            800,
            th.text,
            Align2::LEFT_CENTER,
            br.width() - px(32.0),
        );
        // tags
        y = br.max.y + px(24.0);
        draw::caps(&p, "TAGS", pos2(x0, y), 10.5, th.caption, Align2::LEFT_CENTER);
        draw::label(
            &p,
            "help you find it again: the Sounds tab searches them",
            pos2(x0 + px(60.0), y),
            11.5,
            600,
            th.caption,
            Align2::LEFT_CENTER,
        );
        y += px(30.0);
        for (group, tags) in TAGS {
            draw::caps(
                &p,
                group,
                pos2(x0, y + px(14.0)),
                9.5,
                th.caption,
                Align2::LEFT_CENTER,
            );
            let mut x = x0 + px(104.0);
            for tag in tags {
                let on = self.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
                let label = if on {
                    format!("\u{2713} {tag}")
                } else {
                    tag.to_string()
                };
                let dot = (group == "CHARACTER").then(|| tag_colour(tag));
                let cw = draw::chip_w(&ctx, &label, dot.is_some(), 12.0);
                let cr = Rect::from_min_size(pos2(x, y), vec2(cw, px(30.0)));
                x = cr.max.x + px(7.0);
                if draw::chip(ui, cr, ("room-save-tag", *tag), &label, dot, on, 12.0).clicked() {
                    if on {
                        self.tags.retain(|t| !t.eq_ignore_ascii_case(tag));
                    } else {
                        self.tags.push(tag.to_string());
                    }
                }
            }
            y += px(40.0);
        }
        // colour and favourite slot
        y += px(4.0);
        draw::caps(
            &p,
            "COLOUR",
            pos2(x0, y + px(14.0)),
            9.5,
            th.caption,
            Align2::LEFT_CENTER,
        );
        for (i, c) in COLOURS.iter().enumerate() {
            let cc = pos2(x0 + px(116.0) + i as f32 * px(32.0), y + px(14.0));
            let hit = Rect::from_center_size(cc, vec2(px(28.0), px(28.0)));
            let resp = interact(ui, hit, ("room-save-colour", i), Sense::click());
            if self.color == i {
                p.circle_stroke(cc, px(14.0), Stroke::new(2.0, th.accent));
            }
            p.circle_filled(cc, px(11.0), rgb(*c));
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.color = i;
            }
        }
        let fx = x0 + px(400.0);
        draw::caps(
            &p,
            "FAVOURITE SLOT",
            pos2(fx, y + px(14.0)),
            9.5,
            th.caption,
            Align2::LEFT_CENTER,
        );
        for s in 1..=4u8 {
            let sr = Rect::from_min_size(
                pos2(fx + px(130.0) + (s - 1) as f32 * px(43.0), y),
                vec2(px(36.0), px(30.0)),
            );
            let on = self.slot == s;
            if draw::key(
                ui,
                sr,
                ("room-save-slot", s),
                if on { KeyLook::Lit } else { KeyLook::Dark },
                &s.to_string(),
                "",
                None,
            )
            .clicked()
            {
                self.slot = if on { 0 } else { s };
            }
        }
        // what gets saved
        y += px(48.0);
        let wr = Rect::from_min_max(pos2(x0, y), pos2(x1, y + px(124.0)));
        paint::inset(&p, wr, Some(px(16.0)), 2.0, Fill::Base);
        let n = self.board.blocks.len();
        draw::caps(
            &p,
            "WHAT GETS SAVED",
            pos2(wr.min.x + px(16.0), wr.min.y + px(18.0)),
            10.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        draw::label(
            &p,
            &format!(
                "{n} block{} with every knob \u{b7} 4 scenes \u{b7} master level {:+.1} dB",
                if n == 1 { "" } else { "s" },
                self.board.level_db
            ),
            pos2(wr.min.x + px(170.0), wr.min.y + px(18.0)),
            11.5,
            600,
            th.text_2,
            Align2::LEFT_CENTER,
        );
        // the chain as miniatures
        let mut mx = wr.min.x + px(16.0);
        let mh = px(36.0);
        for b in self.board.blocks.iter().take(12) {
            let fs = faces::board_size(b);
            let k = mh / fs.y;
            let fr = Rect::from_min_size(pos2(mx, wr.min.y + px(36.0)), vec2((fs.x * k).min(px(70.0)), mh));
            faces::miniature(&p, fr, b, 999.0);
            if !b.on {
                paint::wash(&p, fr, Color32::BLACK, 0.5);
            }
            mx = fr.max.x + px(5.0);
        }
        draw::label_w(
            &p,
            &rigs::chain_text(&self.board),
            pos2((mx + px(12.0)).min(wr.center().x), wr.min.y + px(54.0)),
            12.0,
            700,
            th.text,
            Align2::LEFT_CENTER,
            wr.max.x - mx - px(28.0),
        );
        let mut sx = wr.min.x + px(16.0);
        for i in 0..rigs::SCENES {
            let label = format!("{}", i + 1);
            let cw = draw::chip_w(&ctx, &label, false, 11.5) + px(18.0);
            let cr = Rect::from_min_size(pos2(sx, wr.max.y - px(36.0)), vec2(cw, px(26.0)));
            paint::inset(&p, cr, Some(px(9.0)), 1.5, Fill::Base);
            p.rect_filled(
                Rect::from_min_size(
                    pos2(cr.min.x + px(10.0), cr.center().y - px(1.5)),
                    vec2(px(14.0), px(3.0)),
                ),
                px(1.5),
                rgb(SCENE_COLOURS[i]),
            );
            draw::label(
                &p,
                &label,
                pos2(cr.min.x + px(30.0), cr.center().y),
                11.5,
                800,
                th.text_2,
                Align2::LEFT_CENTER,
            );
            sx = cr.max.x + px(8.0);
        }
        // the keys
        let fy = panel.max.y - px(30.0) - px(44.0);
        draw::label(
            &p,
            "kept in userdata.json with your playlists",
            pos2(x0, fy + px(22.0)),
            11.5,
            600,
            th.caption,
            Align2::LEFT_CENTER,
        );
        let save_w = draw::key_w(&ctx, "Save", "Enter", true) + px(20.0);
        let sr = Rect::from_min_size(pos2(x1 - save_w, fy), vec2(save_w, px(44.0)));
        let cancel_w = draw::key_w(&ctx, "Cancel", "Esc", false) + px(10.0);
        let cr = Rect::from_min_size(pos2(sr.min.x - px(12.0) - cancel_w, fy), vec2(cancel_w, px(44.0)));
        if draw::key(ui, cr, "room-save-cancel", KeyLook::Dark, "Cancel", "Esc", None).clicked() {
            act = Act::Cancel;
        }
        if draw::key(
            ui,
            sr,
            "room-save-save",
            KeyLook::Amber,
            "Save",
            "Enter",
            Some(&draw::icon_save),
        )
        .clicked()
        {
            act = Act::Save;
        }
        let _ = text::width(&ctx, "", Style::new(10.0, 700, th.text));
        act
    }
}

/// A character tag's colour (as the scenes').
pub fn tag_colour(tag: &str) -> Color32 {
    match tag.to_lowercase().as_str() {
        "clean" => rgb(SCENE_COLOURS[0]),
        "crunch" => rgb(SCENE_COLOURS[1]),
        "lead" | "fuzz" => rgb(SCENE_COLOURS[2]),
        "ambient" => rgb(SCENE_COLOURS[3]),
        "bass" => rgb([0xb9, 0xa6, 0x84]),
        _ => rgb([0x9a, 0xa0, 0xaa]),
    }
}
