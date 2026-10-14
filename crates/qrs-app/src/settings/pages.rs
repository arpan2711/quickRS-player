//! The Display, Practice and Library pages (port of settingsview.py `draw_display`, `tile_row`, `draw_practice`,
//! `draw_library`; the library page adds which copy plays when a song is there as both a psarc and a feedpak).

use eframe::egui;
use egui::{Align2, CursorIcon, Rect, Sense, Ui, pos2, vec2};
use qrs_library::Prefer;
use qrs_ui::anim;
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::knobs;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{self, t};
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle, Segmented, interact};

use super::{SettingsView, TileDrag, caption_lines, centre_x, centre_y, line_h, snap};
use crate::home::group_digits;
use crate::player::tiles::{self, TILES};
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

fn seg_opts<'a>(labels: &'a [&'a str], sel: usize) -> Segmented<'a> {
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
        // the look: theme and knobs (the pick at the top of the sidebar switches dark and light too)
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
        // notes and the tab
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
                2.0 * pad
                    + 3.0 * hh
                    + Self::h_switch("md", true)
                    + px(18.0)
                    + seg_h
                    + px(22.0)
                    + NOTATIONS.len() as f32 * (Self::h_switch("sm", false) + px(8.0))
                    - px(8.0),
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
            "Rocksmith's string colours on the tab, tuner and neck",
            st.config.bool("string_colors", false),
            "toggle:string_colors:off",
            "md",
            "",
            700,
        ) + px(18.0);
        y = self.heading(&p, "NOTE TILE SHOWS", x, y, None, "", "");
        let seg = Rect::from_min_size(pos2(x, y), vec2(wd, seg_h));
        let cur = usize::from(st.config.str("note_view", "piano") == "guitar");
        let (_, clicked) =
            widgets::segmented(ui, seg, "set-noteview", &seg_opts(&["Piano", "Guitar neck"], cur));
        if let Some(i) = clicked {
            self.acts
                .push(format!("set:note_view:{}", ["piano", "guitar"][i]));
        }
        y = seg.max.y + px(22.0);
        y = self.heading(&p, "RHYTHM NOTATION", x, y, None, "", "N");
        for (k, label) in NOTATIONS {
            y += self.switch_row(
                ui,
                x,
                y,
                wd,
                label,
                "",
                nota_on(k),
                &format!("nota:{k}"),
                "sm",
                "",
                600,
            ) + px(8.0);
        }
        // the tiles in their order: drag a row by its grip to move the tile
        let order = tiles::tile_order(st);
        let (row_h, rgap) = (px(44.0), px(4.0));
        let n = order.len() as f32;
        let list_h = 2.0 * px(6.0) + n * row_h + (n - 1.0) * rgap;
        let card2 = Rect::from_min_size(
            pos2(body.min.x + colw + gap, top),
            vec2(
                colw,
                2.0 * pad + hh + list_h + px(22.0) + Self::h_switch("sm", false),
            ),
        );
        paint::card(&p, card2, 24.0, false);
        let (x2, wd2) = (card2.min.x + pad, card2.width() - 2.0 * pad);
        let y2 = self.heading(
            &p,
            "TILES",
            x2,
            card2.min.y + pad,
            Some(card2.max.x - pad),
            "drag to reorder",
            "",
        );
        let lst = Rect::from_min_size(pos2(x2, y2), vec2(wd2, list_h));
        paint::well(&p, lst, 16.0);
        self.tile_list_y = lst.min.y;
        let drag_key = self.tile_drag.as_ref().map(|d| d.key);
        for (i, (k, on)) in order.iter().enumerate() {
            if drag_key == Some(*k) {
                continue; // drawn last, on top
            }
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
                *on,
                false,
            );
        }
        if let Some(d) = &self.tile_drag
            && let Some((k, on)) = order.iter().find(|o| o.0 == d.key).copied()
        {
            let r = Rect::from_min_size(
                pos2(lst.min.x + px(6.0), snap(d.y)),
                vec2(lst.width() - px(12.0), row_h),
            );
            self.tile_row(ui, st, r, k, on, true);
        }
        self.switch_row(
            ui,
            x2,
            lst.max.y + px(22.0),
            wd2,
            "Lyrics tile: next line",
            "",
            st.config.bool("lyrics_next", true),
            "toggle:lyrics_next",
            "sm",
            "",
            600,
        );
        card.max.y.max(card2.max.y)
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
                2.0 * pad + hh + Self::h_switch("md", true) + px(20.0) + rule.len() as f32 * px(19.0),
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
                ("Notes earlier", ",", "ms:av_offset:-10"),
                ("Notes later", ".", "ms:av_offset:10"),
                format!("{sync:+.0} ms"),
            ),
            (
                "GUITAR TIMING",
                "if your hits all show as late or early",
                ("Count earlier", "Ctrl+,", "ms:input_offset:-10"),
                ("Count later", "Ctrl+.", "ms:input_offset:10"),
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

    pub(super) fn draw_library(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let dirs: Vec<String> = if st.dirs.is_empty() {
            vec!["(none yet: drop a folder of songs on the window to add it)".into()]
        } else {
            st.dirs.iter().map(|d| d.display().to_string()).collect()
        };
        let wd_all = snap(body.width().min(760.0));
        let box_h = 2.0 * px(10.0) + dirs.len() as f32 * px(22.0);
        let both = st
            .groups
            .iter()
            .any(|g| g.has_psarc(&st.songs) && g.has_feedpak(&st.songs));
        // (quickRS's own: which copy plays, under the song list's switch, laid out as the Display page's choices)
        let seg_h = px(44.0) + 2.0 * px(6.0);
        let prefer_h = if both {
            px(26.0) + hh + seg_h + px(10.0) + line_h(&ctx, 13.0)
        } else {
            0.0
        };
        let card = Rect::from_min_size(
            body.min,
            vec2(
                wd_all,
                pad + hh
                    + box_h
                    + px(16.0)
                    + px(46.0)
                    + px(26.0)
                    + hh
                    + Self::h_switch("md", true)
                    + prefer_h
                    + pad,
            ),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let found = format!("{} songs found", group_digits(st.groups.len()));
        let mut y = self.heading(
            &p,
            "SONG FOLDERS",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &found,
            "",
        );
        let bx = Rect::from_min_size(pos2(x, y), vec2(wd, box_h));
        paint::display(&p, bx, Some(14.0), 4.0);
        for (i, d) in dirs.iter().enumerate() {
            text::draw(
                &p,
                d,
                pos2(bx.min.x + px(14.0), bx.min.y + px(10.0) + i as f32 * px(22.0)),
                Align2::LEFT_TOP,
                Style::new(13.0, 500, if st.dirs.is_empty() { th.caption } else { th.text }),
                Some(bx.width() - px(28.0)),
            );
        }
        y = bx.max.y + px(16.0);
        let bw = widgets::button_width(&ctx, "Rescan folders", "F5", 14.0, false);
        let rescan = Rect::from_min_size(pos2(x, y), vec2(bw, px(46.0)));
        self.button(
            ui,
            rescan,
            "Rescan folders",
            "F5",
            "rescan",
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        if st.scanning() {
            let done = st.library.done.load(std::sync::atomic::Ordering::Relaxed);
            let total = st.library.total.load(std::sync::atomic::Ordering::Relaxed);
            text::draw(
                &p,
                &format!("Reading songs… {done} of {total}"),
                pos2(x + bw + px(16.0), centre_y(rescan)),
                Align2::LEFT_CENTER,
                Style::new(13.0, 500, th.caption),
                Some(wd - bw - px(16.0)),
            );
        }
        y += px(46.0) + px(26.0);
        y = self.heading(&p, "SONG LIST", x, y, None, "", "");
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "Expand & preview the selected song",
            "bigger cover and the song's preview clip",
            st.config.bool("select_preview", true),
            "toggle:select_preview",
            "md",
            "",
            700,
        );
        if both {
            y += px(26.0);
            y = self.heading(
                &p,
                "SONGS IN BOTH FORMATS",
                x,
                y,
                Some(card.max.x - pad),
                "which copy plays",
                "",
            );
            let seg = Rect::from_min_size(pos2(x, y), vec2(wd.min(px(420.0)), seg_h));
            let sel = usize::from(st.prefer() == Prefer::Feedpak);
            let (_, clicked) = widgets::segmented(
                ui,
                seg,
                "set-prefer",
                &seg_opts(&["The psarc", "The feedpak"], sel),
            );
            if let Some(i) = clicked {
                self.acts.push(format!("prefer:{}", ["psarc", "feedpak"][i]));
            }
            caption_lines(
                &p,
                "Your scores are shared by both copies; the part picker can still open the other one.",
                x,
                seg.max.y + px(10.0),
                wd,
                13.0,
                1,
            );
        }
        card.max.y
    }
}
