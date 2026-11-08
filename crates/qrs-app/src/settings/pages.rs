//! The Display page (port of settingsview.py `draw_display`, `tile_row`; rebuilt in P35.3, D92: look, notes, notation
//! with its fine-tuning folded, the song screen's tiles and the note widget window), and the classic look's Practice
//! page (`draw_practice`, the Python app's, for the parity shots: the new look's Timing replaces it). Library: library.rs.

use eframe::egui;
use egui::{Align2, CursorIcon, Rect, Sense, Ui, pos2, vec2};
use qrs_ui::anim;
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::knobs;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{self, t};
use qrs_ui::units::px;
use qrs_ui::widgets::{self, Segmented, interact};

use super::{SettingsView, TileDrag, centre_x, centre_y, line_h, snap};
use crate::player::highway_size::{SIZES, size_from, size_label, size_pct};
use crate::player::tiles::{self, TILES};
use crate::player::{Backdrop, GEM_SIZES, NoteColours, gem_from};
use crate::state::AppState;

const THEMES: [(&str, &str); 3] = [
    ("midnight", "Midnight"),
    ("graphite", "Graphite"),
    ("daylight", "Daylight"),
];
const KNOB_STYLES: [(&str, &str); 4] = [
    ("amp", "Amp gold"),
    ("pedal", "Pedal black"),
    ("chrome", "Chrome"),
    ("vintage", "Vintage"),
];
const NOTATIONS: [(&str, &str); 5] = [
    ("show", "Notation under the tab"),
    ("snap", "Snap to beats"),
    ("rests", "Rests"),
    ("beams", "Beams & ties"),
    ("triplets", "Triplets"),
];

pub(super) fn seg_opts<'a>(labels: &'a [&'a str], sel: usize) -> Segmented<'a> {
    let mut so = Segmented::new(labels, Some(sel));
    so.size = 14.0;
    so.r = 16.0;
    so.inner_r = 12.0;
    so.gap = 6.0;
    so
}

impl SettingsView {
    pub(super) fn draw_display(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (gap, pad) = (px(24.0), px(22.0));
        let colw = snap((body.width() - gap) / 2.0);
        let seg_h = px(44.0) + 2.0 * px(6.0);
        let hh = Self::h_heading(&ctx);
        // the look: theme and knobs (the pick at the top of the sidebar steps through all three too)
        let look = Rect::from_min_size(body.min, vec2(body.width(), 2.0 * pad + hh + px(24.0) + seg_h));
        paint::card(&p, look, 24.0, false);
        let y = self.heading(
            &p,
            "LOOK",
            look.min.x + pad,
            look.min.y + pad,
            Some(look.max.x - pad),
            "the pick at the top of the sidebar switches dark and light",
            "",
        );
        let theme_now = theme::theme().key();
        let knobs_now = knobs::style().key();
        for (i, (title, options, current, verb)) in [
            ("Theme", &THEMES[..], theme_now, "theme"),
            ("Knobs", &KNOB_STYLES[..], knobs_now, "knobs"),
        ]
        .into_iter()
        .enumerate()
        {
            let x = look.min.x + pad + i as f32 * (colw + snap(gap / 2.0) - snap(pad / 2.0));
            let wd = colw - pad - snap(gap / 2.0);
            text::draw(
                &p,
                title,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(14.0, 700, th.text_2),
                None,
            );
            let seg = Rect::from_min_size(pos2(x, y + px(24.0)), vec2(wd, seg_h));
            let labels: Vec<&str> = options.iter().map(|o| o.1).collect();
            let sel = options.iter().position(|o| o.0 == current).unwrap_or(0);
            let (_, clicked) = widgets::segmented(ui, seg, ("set-look", verb), &seg_opts(&labels, sel));
            if let Some(k) = clicked {
                self.acts.push(format!("{verb}:{}", options[k].0));
            }
        }
        let top = look.max.y + gap;
        // notes: string colours and what the note tile shows
        let nota = st.config.get("notation").and_then(|v| v.as_object()).cloned();
        let nota_on = |k: &str| {
            nota.as_ref()
                .and_then(|m| m.get(k))
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
        };
        let card = Rect::from_min_size(
            pos2(body.min.x, top),
            vec2(
                colw,
                2.0 * pad + hh + Self::h_switch("md", true) + px(16.0) + seg_h,
            ),
        );
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        paint::card(&p, card, 24.0, false);
        let mut y = self.heading(&p, "NOTES", x, card.min.y + pad, None, "", "");
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "String colours",
            "RS's string colours on the tab, tuner and neck",
            st.config.bool("string_colors", false),
            "toggle:string_colors:off",
            "md",
            "",
            700,
        ) + px(16.0);
        let lr = text::draw(
            &p,
            "Note tile shows",
            pos2(x, y + snap(seg_h / 2.0)),
            Align2::LEFT_CENTER,
            Style::new(15.0, 700, th.text),
            None,
        );
        let sx = lr.max.x + px(18.0);
        let seg = Rect::from_min_size(pos2(sx, y), vec2(x + wd - sx, seg_h));
        let cur = usize::from(st.config.str("note_view", "piano") == "guitar");
        let (_, clicked) =
            widgets::segmented(ui, seg, "set-noteview", &seg_opts(&["Piano", "Guitar neck"], cur));
        if let Some(i) = clicked {
            self.acts
                .push(format!("set:note_view:{}", ["piano", "guitar"][i]));
        }
        // notation: rhythm under the tab (N), its fine-tuning folded
        let fine_open = self.folds.is_open("notation");
        let fine_n = NOTATIONS.len() - 1;
        let card_n = Rect::from_min_size(
            pos2(body.min.x, card.max.y + gap),
            vec2(
                colw,
                2.0 * pad
                    + hh
                    + Self::h_switch("md", true)
                    + px(12.0)
                    + px(widgets::FOLD_ROW_H)
                    + if fine_open {
                        px(12.0) + fine_n as f32 * (Self::h_switch("sm", false) + px(8.0)) - px(8.0)
                    } else {
                        0.0
                    }
                    + px(16.0)
                    + line_h(&ctx, 12.0),
            ),
        );
        paint::card(&p, card_n, 24.0, false);
        let mut y = self.heading(&p, "NOTATION", x, card_n.min.y + pad, None, "", "");
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            NOTATIONS[0].1,
            "note lengths, rests and beams",
            nota_on(NOTATIONS[0].0),
            &format!("nota:{}", NOTATIONS[0].0),
            "md",
            "N",
            700,
        ) + px(12.0);
        let (h, open) = self.fold(
            ui,
            x - px(10.0),
            y,
            wd + px(20.0),
            "notation",
            "Fine-tune notation",
            "snap to beats \u{b7} rests \u{b7} beams & ties \u{b7} triplets",
        );
        y += h;
        if open {
            y += px(12.0);
            for (k, label) in &NOTATIONS[1..] {
                y += self.switch_row(
                    ui,
                    x + px(32.0),
                    y,
                    wd - px(32.0),
                    label,
                    "",
                    nota_on(k),
                    &format!("nota:{k}"),
                    "sm",
                    "",
                    600,
                ) + px(8.0);
            }
            y -= px(8.0);
        }
        text::draw(
            &p,
            "Notation on or off (N) is also in a song's Controls.",
            pos2(x, y + px(16.0)),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, th.caption),
            Some(wd),
        );
        // the song screen: the tiles in their order (drag a row by its grip to move the tile), the lyrics' next line
        // and the note widget window
        let order = tiles::tile_order(st);
        let (row_h, rgap) = (px(44.0), px(4.0));
        let n = order.len() as f32;
        let list_h = 2.0 * px(6.0) + n * row_h + (n - 1.0) * rgap;
        let card2 = Rect::from_min_size(
            pos2(body.min.x + colw + gap, top),
            vec2(
                colw,
                2.0 * pad
                    + hh
                    + list_h
                    + px(18.0)
                    + Self::h_switch("sm", false)
                    + px(12.0)
                    + Self::h_switch("md", true)
                    + px(18.0)
                    + hh
                    + seg_h
                    + px(18.0)
                    + hh
                    + seg_h,
            ),
        );
        paint::card(&p, card2, 24.0, false);
        let (x2, wd2) = (card2.min.x + pad, card2.width() - 2.0 * pad);
        let y2 = self.heading(
            &p,
            "SONG SCREEN",
            x2,
            card2.min.y + pad,
            Some(card2.max.x - pad),
            "off: folded  ·  drag to reorder",
            "",
        );

        let lst = Rect::from_min_size(pos2(x2, y2), vec2(wd2, list_h));
        paint::well(&p, lst, 16.0);
        self.tile_list_y = lst.min.y;
        let drag_key = self.tile_drag.as_ref().map(|d| d.key);
        // (a switch is open or folded: a tile is never hidden, P35.2; a device's tile is on or off)
        for (i, (k, _)) in order.iter().enumerate() {
            if drag_key == Some(*k) {
                continue; // drawn last, on top
            }
            let on = tiles::tile_open(st, k);
            let slot = px(6.0) + i as f32 * (row_h + rgap);
            let ry = anim::smooth(&ctx, ("srow", *k), slot, 0.05, None);
            self.tile_row(
                ui,
                st,
                Rect::from_min_size(
                    pos2(lst.min.x + px(6.0), lst.min.y + snap(ry)),
                    vec2(lst.width() - px(12.0), row_h),
                ),
                k,
                on,
                false,
            );
        }
        if let Some(d) = &self.tile_drag
            && let Some((k, _)) = order.iter().find(|o| o.0 == d.key).copied()
        {
            let on = tiles::tile_open(st, k);
            let r = Rect::from_min_size(
                pos2(lst.min.x + px(6.0), snap(d.y)),
                vec2(lst.width() - px(12.0), row_h),
            );
            self.tile_row(ui, st, r, k, on, true);
        }
        let mut y2 = lst.max.y + px(18.0);
        y2 += self.switch_row(
            ui,
            x2,
            y2,
            wd2,
            "Lyrics: show the next line",
            "",
            st.config.bool("lyrics_next", true),
            "toggle:lyrics_next",
            "sm",
            "",
            600,
        ) + px(12.0);
        // the note widget window (on the Audio page before P35.3)
        y2 += self.switch_row(
            ui,
            x2,
            y2,
            wd2,
            "Note widget window",
            "the always-on-top note and key window",
            st.config.bool("note_widget", true),
            "notewidget",
            "md",
            "",
            700,
        ) + px(18.0);
        // the highway's background (P30.10): the living stage, one still wallpaper, or plain dark (also its toolbar's eye)
        let y2 = self.heading(&p, "HIGHWAY BACKGROUND", x2, y2, None, "", "D");
        let seg = Rect::from_min_size(pos2(x2, y2), vec2(wd2, seg_h));
        let cur = Backdrop::parse(&st.config.str("hw_backdrop", "live"));
        let labels = Backdrop::ALL.map(Backdrop::label);
        let sel = Backdrop::ALL.iter().position(|&b| b == cur).unwrap_or(0);
        let (_, clicked) = widgets::segmented(ui, seg, "set-hwbackdrop", &seg_opts(&labels, sel));
        if let Some(i) = clicked {
            self.acts
                .push(format!("set:hw_backdrop:{}", Backdrop::ALL[i].key()));
        }
        // the highway's note size (P37.6d, D143): the camera's distance, 100% = RS's (also its toolbar's Size)
        let y2 = self.heading(
            &p,
            "HIGHWAY NOTE SIZE",
            x2,
            seg.max.y + px(18.0),
            Some(x2 + wd2),
            "bigger: the camera closer",
            "",
        );
        let seg = Rect::from_min_size(pos2(x2, y2), vec2(wd2, seg_h));
        let labels: Vec<String> = (0..SIZES.len()).map(size_label).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let sel = size_from(&st.config);
        let (_, clicked) = widgets::segmented(ui, seg, "set-hwsize", &seg_opts(&labels, sel));
        if let Some(i) = clicked {
            self.acts.push(format!("set:hw_size:{}", size_pct(i)));
        }
        // the highway's notes (P43.2/P43.3), a card under NOTATION: their colours (Vivid, the game's own, by default)
        // and their boxes' size
        let card_h = Rect::from_min_size(
            pos2(body.min.x, card_n.max.y + gap),
            vec2(
                colw,
                2.0 * pad + hh + 2.0 * (line_h(&ctx, 15.0) + px(10.0) + seg_h) + px(16.0),
            ),
        );
        paint::card(&p, card_h, 24.0, false);
        let mut y = self.heading(
            &p,
            "HIGHWAY NOTES",
            x,
            card_h.min.y + pad,
            Some(x + wd),
            "boxes, sustains and bends",
            "",
        );
        let rows: [(&str, Vec<String>, usize, &str); 2] = [
            (
                "Colours",
                NoteColours::ALL.iter().map(|c| c.label().to_string()).collect(),
                {
                    let cur = NoteColours::parse(&st.config.str("hw_colours", "vivid"));
                    NoteColours::ALL.iter().position(|&c| c == cur).unwrap_or(0)
                },
                "set-hwcolours",
            ),
            (
                "Box size",
                GEM_SIZES.iter().map(|g| format!("{:.0}%", g * 100.0)).collect(),
                gem_from(&st.config),
                "set-hwgem",
            ),
        ];
        for (k, (label, opts, sel, id)) in rows.iter().enumerate() {
            text::draw(
                &p,
                label,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(15.0, 700, th.text),
                None,
            );
            y += line_h(&ctx, 15.0) + px(10.0);
            let seg = Rect::from_min_size(pos2(x, y), vec2(wd, seg_h));
            let opts: Vec<&str> = opts.iter().map(String::as_str).collect();
            let (_, clicked) = widgets::segmented(ui, seg, *id, &seg_opts(&opts, *sel));
            if let Some(i) = clicked {
                self.acts.push(if k == 0 {
                    format!("set:hw_colours:{}", NoteColours::ALL[i].key())
                } else {
                    format!("set:hw_gem:{:.0}", GEM_SIZES[i] * 100.0)
                });
            }
            y = seg.max.y + px(16.0);
        }
        // the tuner's face (P48.3): the same choice as the dots beside TUNER on the tile
        use crate::player::tuner_face::Face;
        let card_t = Rect::from_min_size(
            pos2(body.min.x, card_h.max.y + gap),
            vec2(colw, 2.0 * pad + hh + seg_h + px(8.0)),
        );
        paint::card(&p, card_t, 24.0, false);
        let y = self.heading(
            &p,
            "TUNER",
            x,
            card_t.min.y + pad,
            Some(x + wd),
            "its face (also the dots beside TUNER on the tile)",
            "",
        );
        let cur = Face::parse(&st.config.str("tuner_face", ""));
        let opts: Vec<&str> = Face::ALL.iter().map(|f| f.short()).collect();
        let sel = Face::ALL.iter().position(|&f| f == cur).unwrap_or(0);
        let seg = Rect::from_min_size(pos2(x, y), vec2(wd, seg_h));
        let (_, clicked) = widgets::segmented(ui, seg, "set-tunerface", &seg_opts(&opts, sel));
        if let Some(i) = clicked {
            self.acts.push(format!("set:tuner_face:{}", Face::ALL[i].key()));
        }
        card_t.max.y.max(card2.max.y)
    }

    /// One tile in the TILES list: a grip to drag it by, its name (with its key) and a switch.
    fn tile_row(&mut self, ui: &Ui, st: &mut AppState, row: Rect, key: &'static str, on: bool, lifted: bool) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let mouse = ctx.input(|i| i.pointer.hover_pos());
        if lifted || mouse.is_some_and(|m| row.contains(m) && ui.clip_rect().contains(m)) {
            paint::raised(
                &p,
                row,
                Some(10.0),
                if lifted { 8.0 } else { 3.0 },
                Fill::Base,
                0.0,
            );
        }
        let grip = Rect::from_min_size(row.min, vec2(px(34.0), row.height()));
        let cy = centre_y(row);
        icons::icon(&p, Icon::Grip, pos2(centre_x(grip), cy), 16.0, th.tick);
        let gresp = interact(ui, grip, ("set-grip", key), Sense::drag());
        if gresp.hovered() || lifted {
            ctx.set_cursor_icon(CursorIcon::Move);
        }
        if gresp.drag_started()
            && let Some(pos) = gresp.interact_pointer_pos()
        {
            self.tile_drag = Some(TileDrag {
                key,
                grab: pos.y - row.min.y,
                y: row.min.y,
            });
        }
        if lifted && let Some(d) = &mut self.tile_drag {
            if gresp.dragged()
                && let Some(pos) = gresp.interact_pointer_pos()
            {
                d.y = pos.y - d.grab;
                // reorder while dragging: the row goes where its middle is among the others
                let centre = d.y + row.height() / 2.0;
                let order = tiles::tile_order(st);
                let rows = self.tile_rows.clone();
                let others: Vec<(&'static str, bool)> =
                    order.iter().filter(|o| o.0 != key).copied().collect();
                let idx = others
                    .iter()
                    .filter(|o| rows.iter().any(|(r, k)| *k == o.0 && centre_y(*r) < centre))
                    .count();
                let mine = order.iter().find(|o| o.0 == key).copied().unwrap_or((key, on));
                let mut new = others.clone();
                new.insert(idx.min(new.len()), mine);
                if new != order {
                    tiles::set_tiles(st, &new);
                }
            }
            if gresp.drag_stopped() || !gresp.dragged() && !gresp.is_pointer_button_down_on() {
                // settle from where it was dropped
                let dy = d.y - self.tile_list_y;
                anim::forget(("srow", key));
                let _ = anim::smooth(&ctx, ("srow", key), dy, 0.05, Some(dy));
                self.tile_drag = None;
                self.acts.push("saveconfig".into());
            }
        }
        let name = TILES.iter().find(|t_| t_.0 == key).map(|t_| t_.1).unwrap_or(key);
        let tr = text::draw(
            &p,
            name,
            pos2(row.min.x + px(38.0), cy),
            Align2::LEFT_CENTER,
            Style::new(14.0, 600, if on { th.text } else { th.caption }),
            None,
        );
        if key == "lyrics" {
            keycap(
                &p,
                "Y",
                pos2(tr.max.x + px(10.0), cy),
                Align2::LEFT_CENTER,
                11.0,
                CapOn::Material,
            );
        }
        // sw_r.midright = (row.right - 10, row.centery)
        let ss = vec2(px(44.0), px(24.0));
        let sw = Rect::from_min_size(pos2(row.max.x - px(10.0) - ss.x, cy - snap(ss.y / 2.0)), ss);
        widgets::draw_switch(&p, sw, egui::Id::new(("set-tile-sw", key)), on);
        if !lifted {
            self.tile_rows.push((row, key));
        }
        self.hit(
            ui,
            Rect::from_min_max(pos2(row.min.x + px(34.0), row.min.y), row.max),
            &format!("tile:{key}"),
        );
    }

    pub(super) fn draw_practice(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (gap, pad) = (px(24.0), px(22.0));
        let colw = snap((body.width() - gap) / 2.0);
        let wd = colw - 2.0 * pad;
        let hh = Self::h_heading(&ctx);
        let rule = text::wrap(
            &ctx,
            "A high score is only saved for a whole part played start to finish at 100% difficulty and 100% speed.",
            Style::new(13.0, 500, th.caption),
            wd,
        );
        let card = Rect::from_min_size(
            body.min,
            vec2(
                colw,
                2.0 * pad
                    + hh
                    + 2.0 * Self::h_switch("md", true)
                    + px(12.0)
                    + px(20.0)
                    + rule.len() as f32 * px(19.0),
            ),
        );
        paint::card(&p, card, 24.0, false);
        let x = card.min.x + pad;
        let mut y = self.heading(&p, "LOOPS", x, card.min.y + pad, None, "", "");
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "Level up after a clean loop",
            "90%+ on a loop pass raises difficulty 10%",
            st.config.bool("auto_level", true),
            "toggle:auto_level",
            "md",
            "",
            700,
        ) + px(12.0);
        // the count-in (P37.11, D121): one switch for every count-in there is
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "Count in: 3, 2, 1",
            "before the song, a loop, a review spot or a take starts (Shift C in the player)",
            st.config.bool("countdown", true),
            "toggle:countdown",
            "md",
            "",
            700,
        ) + px(20.0);
        for line in &rule {
            text::draw(
                &p,
                line,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(13.0, 500, th.caption),
                None,
            );
            y += px(19.0);
        }
        let sync = st.config.f64("av_offset", 0.0) * 1000.0;
        let gt = st.config.f64("input_offset", 0.0) * 1000.0;
        let card2 = Rect::from_min_size(
            pos2(body.min.x + colw + gap, body.min.y),
            vec2(colw, 2.0 * pad + 2.0 * (hh + px(18.0) + px(46.0)) + px(24.0)),
        );
        paint::card(&p, card2, 24.0, false);
        let (x2, wd2) = (card2.min.x + pad, card2.width() - 2.0 * pad);
        let mut y2 = card2.min.y + pad;
        for (title, hint, minus, plus, value) in [
            (
                "AUDIO / NOTES SYNC",
                "if the notes run ahead of or behind the song",
                ("Notes earlier", ",", "ms:av_offset:-5"),
                ("Notes later", ".", "ms:av_offset:5"),
                format!("{sync:+.0} ms"),
            ),
            (
                "GUITAR TIMING",
                "if your hits all show as late or early",
                ("Count later", "Ctrl+,", "ms:input_offset:5"),
                ("Count earlier", "Ctrl+.", "ms:input_offset:-5"),
                format!("{gt:+.0} ms"),
            ),
        ] {
            y2 = self.heading(&p, title, x2, y2, None, "", "");
            text::draw(
                &p,
                hint,
                pos2(x2, y2 - px(4.0)),
                Align2::LEFT_TOP,
                Style::new(12.0, 500, th.caption),
                Some(wd2),
            );
            y2 += px(18.0);
            y2 += self.stepper(ui, x2, y2, wd2, minus, &value.replace('-', "\u{2212}"), plus) + px(24.0);
        }
        card.max.y.max(card2.max.y)
    }
}
