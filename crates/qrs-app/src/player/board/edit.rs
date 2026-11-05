//! The pedalboard's block editor (PLAN.md P21.4): the big view's page for one block, from the owner-approved
//! mockups. The board shrinks to a strip of miniatures with the previous and next block either side; the block's
//! face is drawn large, with a value bubble on the knob under the mouse, its sweet spot marked and a plain-English
//! tip beside it; A/B and undo/redo; what the block does, drawn; named starting points and your own default; an
//! About card (the unit it's after, its era, famous records, the songs in your library whose tones use it); tempo
//! sync for delay times and modulation rates; for a cab, the mic as a dot on the speaker, its distance and type.
//! New code (no Python source).

use super::*;
use crate::gearindex;
use crate::player::PlayerReq;
use crate::player::gearinfo as gi;

/// The tip column beside the face.
const GUIDE_W: f32 = 290.0;

fn num(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{:.0}", v)
    } else {
        format!("{v:.1}")
    }
}

/// A sweet spot as words: "4\u{2013}6", "60\u{2013}100 Hz", "Dynamic or Ribbon".
fn sweet_text(kd: &board::KnobDef, (lo, hi): (f32, f32)) -> String {
    match kd.unit {
        Unit::Choice(c) => (lo.round() as usize..=hi.round() as usize)
            .filter_map(|i| c.get(i).copied())
            .collect::<Vec<_>>()
            .join(" or "),
        Unit::Dial if (hi - lo).abs() < 0.05 => num(lo),
        Unit::Dial => format!("{}\u{2013}{}", num(lo), num(hi)),
        _ => {
            let (a, b) = (kd.text(lo), kd.text(hi));
            if a == b {
                return a;
            }
            match (a.rsplit_once(' '), b.rsplit_once(' ')) {
                (Some((ah, au)), Some((_, bu))) if au == bu => format!("{ah}\u{2013}{b}"),
                _ => format!("{a}\u{2013}{b}"),
            }
        }
    }
}

fn same_values(def: &board::ModelDef, a: &[f32; board::MAX_KNOBS], b: &[f32; board::MAX_KNOBS]) -> bool {
    (0..def.knobs.len()).all(|k| (a[k] - b[k]).abs() <= 1e-3 * (1.0 + a[k].abs()))
}

/// A value bubble over a control: its name and value, pointing down at `anchor`.
fn bubble(p: &Painter, anchor: Pos2, label: &str, value: &str) -> Rect {
    bubble_at(p, anchor, label, value, false)
}

/// A value bubble pointing down at `anchor`, or with `under`, hanging under it pointing up.
fn bubble_at(p: &Painter, anchor: Pos2, label: &str, value: &str, under: bool) -> Rect {
    let th = t();
    let ctx = p.ctx();
    let ls = Style::new(12.0, 700, Color32::from_rgb(232, 234, 238));
    let vs = Style::new(12.5, 800, th.accent);
    let lw = if label.is_empty() {
        0.0
    } else {
        text::width(ctx, label, ls) + px(6.0)
    };
    let w = lw + text::width(ctx, value, vs) + px(20.0);
    let h = px(26.0);
    let y0 = if under {
        anchor.y + px(6.0)
    } else {
        anchor.y - h - px(6.0)
    };
    let r = Rect::from_min_size(pos2(anchor.x - w / 2.0, y0), vec2(w, h));
    let fill = Color32::from_rgb(30, 32, 36);
    p.rect_filled(r.translate(vec2(0.0, 2.0)), 8.0, Color32::from_black_alpha(90));
    p.rect_filled(r, 8.0, fill);
    p.rect_stroke(
        r,
        8.0,
        Stroke::new(1.2, th.accent.gamma_multiply(0.75)),
        egui::StrokeKind::Inside,
    );
    p.add(egui::Shape::convex_polygon(
        if under {
            vec![
                pos2(anchor.x, r.min.y - px(6.0)),
                pos2(anchor.x + px(6.0), r.min.y + 0.5),
                pos2(anchor.x - px(6.0), r.min.y + 0.5),
            ]
        } else {
            vec![
                pos2(anchor.x - px(6.0), r.max.y - 0.5),
                pos2(anchor.x + px(6.0), r.max.y - 0.5),
                pos2(anchor.x, r.max.y + px(6.0)),
            ]
        },
        fill,
        Stroke::NONE,
    ));
    if !label.is_empty() {
        text::draw(
            p,
            label,
            pos2(r.min.x + px(10.0), r.center().y),
            Align2::LEFT_CENTER,
            ls,
            None,
        );
    }
    text::draw(
        p,
        value,
        pos2(r.max.x - px(10.0), r.center().y),
        Align2::RIGHT_CENTER,
        vs,
        None,
    );
    r
}

/// The sweet spot marked on a control: an arc round a knob, a band along a fader.
fn sweet_mark(p: &Painter, h: &HotCtl, kd: &board::KnobDef, sweet: (f32, f32)) {
    let th = t();
    let (u0, u1) = (kd.to_u(sweet.0), kd.to_u(sweet.1));
    let col = th.accent.gamma_multiply(0.85);
    match h.ctl {
        Ctl::Knob => {
            let c = h.rect.center();
            let rad = h.rect.width() / 2.0 + px(9.0);
            let at = |u: f32| {
                let a = (-135.0 + 270.0 * u).to_radians();
                pos2(c.x + rad * a.sin(), c.y - rad * a.cos())
            };
            if u1 - u0 < 0.01 {
                p.circle_filled(at(u0), px(3.0), col);
            } else {
                let pts: Vec<Pos2> = (0..=24).map(|i| at(u0 + (u1 - u0) * i as f32 / 24.0)).collect();
                p.add(egui::Shape::line(pts, Stroke::new(px(4.0), col)));
            }
        }
        Ctl::Fader { across } => {
            let tr = h.rect;
            let band = if across {
                Rect::from_min_max(
                    pos2(tr.min.x + tr.width() * u0, tr.min.y - px(5.0)),
                    pos2(tr.min.x + tr.width() * u1.max(u0 + 0.01), tr.max.y + px(5.0)),
                )
            } else {
                Rect::from_min_max(
                    pos2(tr.min.x - px(5.0), tr.max.y - tr.height() * u1.max(u0 + 0.01)),
                    pos2(tr.max.x + px(5.0), tr.max.y - tr.height() * u0),
                )
            };
            p.rect_filled(band, 3.0, th.accent.gamma_multiply(0.28));
            p.rect_stroke(band, 3.0, Stroke::new(1.2, col), egui::StrokeKind::Outside);
        }
        _ => {}
    }
}

/// A mic, drawn small: 0 dynamic, 1 ribbon, 2 condenser.
fn draw_mic(p: &Painter, r: Rect, kind: usize) {
    let c = r.center();
    let (w, h) = (r.width(), r.height());
    let dark = Color32::from_rgb(52, 55, 60);
    let silver = Color32::from_rgb(196, 200, 206);
    let line = Stroke::new(1.0, Color32::from_black_alpha(110));
    match kind {
        0 => {
            // a long thin body, a ball of grille on top
            let body = Rect::from_center_size(c + vec2(0.0, h * 0.12), vec2(w * 0.2, h * 0.62));
            p.rect_filled(body, 3.0, dark);
            let head = Rect::from_center_size(pos2(c.x, body.min.y), vec2(w * 0.3, h * 0.3));
            p.rect_filled(head, w * 0.12, silver);
            for i in 1..5 {
                let y = head.min.y + head.height() * i as f32 / 5.0;
                p.line_segment([pos2(head.min.x + 2.0, y), pos2(head.max.x - 2.0, y)], line);
            }
        }
        1 => {
            // a rounded box with slats, on a yoke
            let body = Rect::from_center_size(c - vec2(0.0, h * 0.08), vec2(w * 0.42, h * 0.64));
            p.rect_filled(body, w * 0.18, Color32::from_rgb(176, 141, 78));
            for i in 1..8 {
                let y = body.min.y + body.height() * i as f32 / 8.0;
                p.line_segment(
                    [pos2(body.min.x + 4.0, y), pos2(body.max.x - 4.0, y)],
                    Stroke::new(1.2, Color32::from_rgb(92, 70, 34)),
                );
            }
            let yoke = Stroke::new(2.0, dark);
            p.line_segment(
                [
                    pos2(body.min.x - 3.0, c.y),
                    pos2(body.min.x - 3.0, body.max.y + 6.0),
                ],
                yoke,
            );
            p.line_segment(
                [
                    pos2(body.max.x + 3.0, c.y),
                    pos2(body.max.x + 3.0, body.max.y + 6.0),
                ],
                yoke,
            );
            p.line_segment(
                [
                    pos2(body.min.x - 3.0, body.max.y + 6.0),
                    pos2(body.max.x + 3.0, body.max.y + 6.0),
                ],
                yoke,
            );
            p.line_segment([pos2(c.x, body.max.y + 6.0), pos2(c.x, r.max.y - 2.0)], yoke);
        }
        _ => {
            // a big capsule over a fat body
            let head = Rect::from_center_size(c - vec2(0.0, h * 0.2), vec2(w * 0.4, h * 0.42));
            let body = Rect::from_min_max(
                pos2(head.min.x + 2.0, head.center().y),
                pos2(head.max.x - 2.0, r.max.y - 4.0),
            );
            p.rect_filled(body, 4.0, Color32::from_rgb(214, 217, 222));
            p.rect_filled(head, w * 0.2, silver);
            for i in 1..6 {
                let x = head.min.x + head.width() * i as f32 / 6.0;
                p.line_segment([pos2(x, head.min.y + 3.0), pos2(x, head.max.y - 3.0)], line);
            }
            p.circle_filled(
                pos2(c.x, body.center().y + 4.0),
                2.5,
                Color32::from_rgb(232, 185, 49),
            );
        }
    }
}

const MICS: [(&str, &str); 3] = [
    (
        "Dynamic",
        "Punchy and mid-forward: the classic close mic on a guitar cab.",
    ),
    (
        "Ribbon",
        "Smooth and dark with soft highs: tames fizzy high gain.",
    ),
    ("Condenser", "Detailed and airy; hears more of the room."),
];

impl Rack {
    // -- edits ------------------------------------------------------------------------------------------------------

    /// Put a whole set of knob values on a block (a starting point, A/B, defaults).
    pub(crate) fn apply_values(&mut self, st: &mut AppState, uid: u32, v: [f32; board::MAX_KNOBS]) {
        self.board_edit(st, |b| {
            if let Some(i) = b.index_of(uid) {
                b.blocks[i].v = v;
            }
        });
        self.board_commit(st);
    }

    /// The block back to your default for its model, or the model's own.
    pub(crate) fn block_defaults(&mut self, st: &mut AppState, blk: Block) {
        let def = blk.def();
        let v = pb::model_default(&st.config, def.id).unwrap_or_else(|| def.defaults());
        self.apply_values(st, blk.uid, v);
    }

    pub(crate) fn undo_edit(&mut self, st: &mut AppState) {
        let Some((b, label)) = self.board.hist.pop() else {
            return;
        };
        let cur = self.board_now(st);
        self.board.redo.push((cur, label.clone()));
        self.board.kept = Some(b.clone());
        self.board_edit(st, |x| *x = b);
        self.board_commit(st);
        self.say(format!("Undid {label}"), "");
    }

    pub(crate) fn redo_edit(&mut self, st: &mut AppState) {
        let Some((b, label)) = self.board.redo.pop() else {
            return;
        };
        let cur = self.board_now(st);
        self.board.hist.push((cur, label.clone()));
        self.board.kept = Some(b.clone());
        self.board_edit(st, |x| *x = b);
        self.board_commit(st);
        self.say(format!("Redid {label}"), "");
    }

    /// A or B: the other slot's values go on, the ones playing are kept in it. The first B is a copy of A.
    fn ab_pick(&mut self, st: &mut AppState, blk: Block, want_b: bool) {
        match self.board.ab {
            Some(a) if a.uid == blk.uid => {
                if a.on_b == want_b {
                    return;
                }
                self.board.ab = Some(Ab {
                    uid: blk.uid,
                    other: blk.v,
                    on_b: want_b,
                });
                self.apply_values(st, blk.uid, a.other);
            }
            _ if want_b => {
                self.board.ab = Some(Ab {
                    uid: blk.uid,
                    other: blk.v,
                    on_b: true,
                });
                self.say(
                    "B is a copy of A",
                    "  \u{b7}  change it, then flip between A and B",
                );
            }
            _ => {}
        }
    }

    /// The song's tempo around now, from the chart's beats.
    fn song_bpm(&self) -> Option<f32> {
        let b = &self.song.btimes;
        if b.len() < 3 {
            return None;
        }
        let t = self.song.pos;
        let i = b.partition_point(|&x| x < t);
        let lo = i.saturating_sub(5);
        let hi = (i + 5).min(b.len() - 1);
        let mut d: Vec<f64> = b[lo..=hi]
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|x| *x > 0.15 && *x < 2.0)
            .collect();
        if d.is_empty() {
            return None;
        }
        d.sort_by(f64::total_cmp);
        Some((60.0 / d[d.len() / 2]) as f32)
    }

    // -- the board page's shown block ----------------------------------------------------------------------------------

    /// Under the board: the shown block's name, its keys, and the way into its editor.
    pub(super) fn block_bar(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Block) {
        let p = ui.painter().clone();
        let th = t();
        let def = blk.def();
        paint::display(&p, r, Some(18.0), 6.0);
        let x0 = r.min.x + px(18.0);
        let y1 = r.min.y + px(28.0);
        let open_w = px(190.0);
        let open = Rect::from_min_size(
            pos2(r.max.x - px(18.0) - open_w, y1 - px(17.0)),
            vec2(open_w, px(34.0)),
        );
        if widgets::button(
            ui,
            open,
            "board-open-editor",
            "Open the editor \u{203a}",
            "",
            widgets::ButtonStyle::primary(),
        )
        .clicked()
        {
            self.board.editing = true;
        }
        let kr = caps(
            &p,
            &def.kind.label().to_uppercase(),
            pos2(x0, y1),
            11.0,
            faces::kind_colour(def.kind),
            Align2::LEFT_CENTER,
        );
        let nr = text::draw(
            &p,
            &def.shown_name(),
            pos2(kr.max.x + px(10.0), y1),
            Align2::LEFT_CENTER,
            Style::new(18.0, 800, th.text),
            Some(open.min.x - kr.max.x - px(30.0)),
        );
        text::draw(
            &p,
            &Rack::file_name(&blk).unwrap_or_else(|| format!("after {}", def.after)),
            pos2(nr.max.x + px(12.0), y1 + px(1.0)),
            Align2::LEFT_CENTER,
            Style::new(12.0, 600, th.caption),
            Some((open.min.x - nr.max.x - px(28.0)).max(0.0)),
        );
        let y2 = r.min.y + px(72.0);
        let onr = Rect::from_min_size(pos2(x0, y2 - px(15.0)), vec2(px(78.0), px(30.0)));
        let resp = interact(ui, onr, "board-bar-on", Sense::click());
        lamp_key(&p, onr, if blk.on { "ON" } else { "OFF" }, blk.on, resp.hovered());
        if resp.clicked() {
            self.toggle_block(st, blk.uid);
        }
        let rr = Rect::from_min_size(pos2(onr.max.x + px(8.0), onr.min.y), vec2(px(90.0), px(30.0)));
        if widgets::button(
            ui,
            rr,
            "board-bar-remove",
            "Remove",
            "",
            widgets::ButtonStyle::default(),
        )
        .clicked()
        {
            self.remove_block(st, blk.uid);
            return;
        }
        let dr = Rect::from_min_size(pos2(rr.max.x + px(8.0), onr.min.y), vec2(px(96.0), px(30.0)));
        if widgets::button(
            ui,
            dr,
            "board-bar-defaults",
            "Defaults",
            "",
            widgets::ButtonStyle::default(),
        )
        .clicked()
        {
            self.block_defaults(st, blk);
        }
        // NAM captures (P31): which amp plays, and the file a capture or IR block runs
        let mut hx = dr.max.x;
        if def.file {
            let cr = Rect::from_min_size(pos2(hx + px(8.0), onr.min.y), vec2(px(150.0), px(30.0)));
            if widgets::button(
                ui,
                cr,
                "board-bar-choose",
                if def.kind == Kind::Cab {
                    "Choose IR\u{2026}"
                } else {
                    "Choose capture\u{2026}"
                },
                "",
                widgets::ButtonStyle::primary(),
            )
            .clicked()
            {
                self.choose_file(blk);
            }
            hx = cr.max.x;
        }
        let sw = Rect::from_min_size(pos2(hx + px(8.0), onr.min.y), vec2(px(176.0), px(30.0)));
        if self.amp_switch(ui, st, sw, blk) {
            hx = sw.max.x;
        }
        text::draw(
            &p,
            "click a block: show it \u{b7} double-click: its editor \u{b7} drag: move \u{b7} + adds one",
            pos2(hx + px(16.0), y2),
            Align2::LEFT_CENTER,
            Style::new(11.0, 600, th.caption),
            Some(r.max.x - hx - px(36.0)),
        );
        if r.height() > px(110.0) {
            let summary: Vec<String> = def
                .front_knobs()
                .iter()
                .enumerate()
                .map(|(k, kd)| format!("{} {}", kd.label, kd.text(blk.v[k])))
                .collect();
            text::draw(
                &p,
                &summary.join("  \u{b7}  "),
                pos2(x0, r.min.y + px(114.0)),
                Align2::LEFT_CENTER,
                Style::new(12.0, 700, th.text_2),
                Some(r.width() - px(36.0)),
            );
        }
    }

    // -- the editor page --------------------------------------------------------------------------------------------

    pub(crate) fn editor_view(&mut self, ui: &Ui, st: &mut AppState, inner: Rect, foot_h: f32) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let b = self.board_now(st);
        let Some(i) = self.board.sel.and_then(|u| b.index_of(u)) else {
            self.board.editing = false;
            return;
        };
        let blk = b.blocks[i];
        let def = blk.def();
        if st.audio_mode != crate::state::AudioMode::Shots {
            gearindex::start(
                st.songs.clone(),
                st.keys.clone(),
                st.data.cache().join("gear.json"),
            );
        }
        // -- the header: back, the model's name, change model, close
        let hdr = Rect::from_min_size(inner.min, vec2(inner.width(), px(52.0)));
        let back = Rect::from_min_size(pos2(hdr.min.x, hdr.min.y + px(9.0)), Vec2::splat(px(34.0)));
        if widgets::round_button(
            ui,
            back,
            "board-ed-back",
            Icon::Left,
            13.0,
            RoundKind::Ivory,
            false,
        )
        .clicked()
        {
            self.board.editing = false;
            self.board.model_menu = false;
            return;
        }
        let close = Rect::from_min_size(
            pos2(hdr.max.x - px(34.0), hdr.min.y + px(9.0)),
            Vec2::splat(px(34.0)),
        );
        if widgets::round_button(
            ui,
            close,
            "board-ed-close",
            Icon::XBold,
            13.0,
            RoundKind::Ivory,
            false,
        )
        .clicked()
        {
            self.board_commit(st);
            self.board.open = false;
            self.board.model_menu = false;
            return;
        }
        let mw = widgets::button_width(&ctx, "Change model \u{25be}", "", 13.0, false) + px(8.0);
        let model_key = Rect::from_min_size(
            pos2(close.min.x - px(10.0) - mw, hdr.min.y + px(9.0)),
            vec2(mw, px(34.0)),
        );
        if widgets::button(
            ui,
            model_key,
            "board-ed-model",
            "Change model \u{25be}",
            "",
            widgets::ButtonStyle::active(self.board.model_menu),
        )
        .clicked()
        {
            self.board.model_menu = !self.board.model_menu;
        }
        let tx = back.max.x + px(14.0);
        let room = model_key.min.x - tx - px(20.0);
        let sub = match &self.board.rig {
            Some(_) => format!(
                "RIG \u{b7} {} \u{b7} {} \u{b7} {}",
                self.song.title.to_uppercase(),
                self.song.part_name.to_uppercase(),
                self.board_title(st).to_uppercase()
            ),
            None => format!("RIG \u{b7} {}", self.board_title(st).to_uppercase()),
        };
        text::caps(
            &p,
            &sub,
            pos2(tx, hdr.min.y + px(10.0)),
            10.5,
            th.caption,
            Align2::LEFT_CENTER,
            Some(room),
        );
        let ty = hdr.min.y + px(35.0);
        let nr = text::draw(
            &p,
            def.name,
            pos2(tx, ty),
            Align2::LEFT_CENTER,
            Style::new(22.0, 800, th.text),
            Some(room * 0.5),
        );
        let mut x = nr.max.x + px(8.0);
        let brand = catalog::brand(def.id);
        if catalog::SHOW_BRANDS && !brand.is_empty() {
            let br = text::draw(
                &p,
                &format!("({brand})"),
                pos2(x, ty + px(1.0)),
                Align2::LEFT_CENTER,
                Style::new(15.0, 700, th.text_2),
                Some((model_key.min.x - x - px(120.0)).max(px(40.0))),
            );
            x = br.max.x + px(10.0);
        }
        let kc = faces::kind_colour(def.kind);
        let kl = def.kind.label().to_uppercase();
        let kw = text::width(&ctx, &kl, Style::caps(9.5, kc)) + px(16.0);
        let kr = Rect::from_min_size(pos2(x, ty - px(10.0)), vec2(kw, px(20.0)));
        p.rect_filled(kr, 6.0, kc.gamma_multiply(0.16));
        p.rect_stroke(kr, 6.0, Stroke::new(1.2, kc), egui::StrokeKind::Inside);
        caps(&p, &kl, kr.center(), 9.5, kc, Align2::CENTER_CENTER);
        if !catalog::SHOW_BRANDS {
            text::draw(
                &p,
                &format!("after {}", def.after),
                pos2(kr.max.x + px(10.0), ty),
                Align2::LEFT_CENTER,
                Style::new(12.0, 600, th.caption),
                Some((model_key.min.x - kr.max.x - px(30.0)).max(0.0)),
            );
        }
        // -- the board as a strip, the previous and next blocks either side
        let sy = hdr.max.y + px(8.0);
        let sh = (inner.height() * 0.115).clamp(px(74.0), px(96.0));
        let card_w = if inner.width() > px(1200.0) {
            px(150.0)
        } else {
            px(124.0)
        };
        let prev_r = Rect::from_min_size(pos2(inner.min.x, sy), vec2(card_w, sh));
        let next_r = Rect::from_min_size(pos2(inner.max.x - card_w, sy), vec2(card_w, sh));
        let well = Rect::from_min_max(
            pos2(prev_r.max.x + px(10.0), sy),
            pos2(next_r.min.x - px(10.0), sy + sh),
        );
        paint::inset(
            &p,
            well,
            Some(14.0),
            2.0,
            Fill::Color(Color32::from_rgb(20, 21, 24)),
        );
        self.chain_ui(ui, st, well, "edstrip", true);
        for (r, j, left) in [(prev_r, i.checked_sub(1), true), (next_r, Some(i + 1), false)] {
            let Some(other) = j.and_then(|j| b.blocks.get(j)) else {
                continue;
            };
            let resp = interact(ui, r, ("board-ed-step", left), Sense::click());
            let hot = resp.hovered();
            p.rect_filled(r, 14.0, if hot { th.screen_hi } else { th.screen });
            let kd = px(26.0);
            let kc = if left {
                pos2(r.min.x + px(12.0) + kd / 2.0, r.min.y + px(26.0))
            } else {
                pos2(r.max.x - px(12.0) - kd / 2.0, r.min.y + px(26.0))
            };
            widgets::draw_round_button(
                &p,
                Rect::from_center_size(kc, Vec2::splat(kd)),
                if left { Icon::Left } else { Icon::Right },
                10.0,
                RoundKind::Ivory,
                false,
                hot,
                false,
            );
            let (lx, anchor) = if left {
                (kc.x + kd / 2.0 + px(8.0), Align2::LEFT_CENTER)
            } else {
                (kc.x - kd / 2.0 - px(8.0), Align2::RIGHT_CENTER)
            };
            caps(
                &p,
                if left { "PREV" } else { "NEXT" },
                pos2(lx, kc.y),
                10.0,
                th.caption,
                anchor,
            );
            text::draw(
                &p,
                other.def().name,
                pos2(
                    if left {
                        r.min.x + px(12.0)
                    } else {
                        r.max.x - px(12.0)
                    },
                    r.max.y - px(20.0),
                ),
                anchor,
                Style::new(13.0, 800, th.text),
                Some(r.width() - px(20.0)),
            );
            if hot {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.board.sel = Some(other.uid);
                self.board.model_menu = false;
            }
        }
        // -- the body: the face, what it does, starting points; the About card and the tempo or the mic
        let body = Rect::from_min_max(
            pos2(inner.min.x, sy + sh + px(12.0)),
            pos2(inner.max.x, inner.max.y - foot_h - px(12.0)),
        );
        let right_w = (body.width() * 0.31).clamp(px(320.0), px(430.0));
        let left = Rect::from_min_max(body.min, pos2(body.max.x - right_w - px(14.0), body.max.y));
        let right = Rect::from_min_max(pos2(left.max.x + px(14.0), body.min.y), body.max);
        let face_h = (left.height() * 0.6)
            .max(px(230.0))
            .min(left.height() - px(150.0));
        let face_card = Rect::from_min_size(left.min, vec2(left.width(), face_h));
        let bottom = Rect::from_min_max(pos2(left.min.x, face_card.max.y + px(12.0)), left.max);
        let graph = Rect::from_min_size(bottom.min, vec2(bottom.width() * 0.55, bottom.height()));
        let starts = Rect::from_min_max(pos2(graph.max.x + px(12.0), bottom.min.y), bottom.max);
        self.face_card(ui, st, face_card, blk);
        // (the block as it is now: the face card may have changed it)
        let now = self.board_now(st);
        let Some(blk) = now.index_of(blk.uid).map(|j| now.blocks[j]) else {
            return;
        };
        if self.board.back && def.has_back() {
            // behind the panel: its controls, each with what it does, where the picture and starting points were
            self.back_card(ui, st, bottom, blk);
        } else {
            paint::display(&p, graph, Some(18.0), 6.0);
            self.picture_ui(&p, graph, &blk);
            self.starts_card(ui, st, starts, blk);
        }
        // the tempo for a delay's time or a modulation's rate, the mic for a cab
        let sync = def
            .knobs
            .iter()
            .position(|k| k.key == "time" && k.unit == Unit::Ms && def.kind == Kind::Delay)
            .map(|k| (k, None))
            .or_else(|| {
                def.knobs
                    .iter()
                    .enumerate()
                    .find_map(|(k, kd)| gi::rate_range(def.id, kd.key).map(|r| (k, Some(r))))
            });
        let extra_h = if sync.is_some() || def.face == Face::Cab {
            (right.height() * 0.4).clamp(px(170.0), px(236.0))
        } else {
            0.0
        };
        let about = if extra_h > 0.0 {
            Rect::from_min_max(right.min, pos2(right.max.x, right.max.y - extra_h - px(12.0)))
        } else {
            right
        };
        self.about_card(ui, st, about, blk);
        if extra_h > 0.0 {
            let er = Rect::from_min_max(pos2(right.min.x, right.max.y - extra_h), right.max);
            match sync {
                Some((k, rate)) => self.tempo_card(ui, st, er, blk, k, rate),
                None => self.mic_card(ui, st, er, blk),
            }
        }
        if self.board.model_menu {
            self.model_menu(ui, st, model_key, blk, inner);
        }
    }

    /// Change model: the browser, dropped down from its key.
    fn model_menu(&mut self, ui: &Ui, st: &mut AppState, key: Rect, blk: Block, inner: Rect) {
        let p = ui.painter().clone();
        let w = px(380.0);
        let r = Rect::from_min_max(
            pos2(key.max.x - w, key.max.y + px(6.0)),
            pos2(key.max.x, (key.max.y + px(480.0)).min(inner.max.y - px(10.0))),
        );
        paint::floating(&p, r, Some(16.0));
        // (the menu takes the clicks over what's under it)
        let _ = interact(ui, r, "board-model-menu", Sense::click());
        self.browser_ui(ui, st, r.shrink(px(4.0)), Some(blk));
        let now = self.board_now(st);
        let changed = now
            .index_of(blk.uid)
            .is_none_or(|j| now.blocks[j].model != blk.model);
        let away = ui.ctx().input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|q| !r.contains(q) && !key.contains(q))
        });
        if changed || away {
            self.board.model_menu = false;
        }
    }

    /// The face card: the keys (A/B, undo, on, defaults, remove), the face drawn large with its tips.
    fn face_card(&mut self, ui: &Ui, st: &mut AppState, card: Rect, blk: Block) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let def = blk.def();
        paint::display(&p, card, Some(18.0), 6.0);
        let bar = Rect::from_min_size(
            card.min + vec2(px(14.0), px(12.0)),
            vec2(card.width() - px(28.0), px(30.0)),
        );
        // Front panel / Back panel (Outside / Inside for a pedal), for models with controls behind the panel
        let x0 = if def.has_back() {
            self.back_tabs(ui, &p, bar, blk)
        } else {
            bar.min.x
        };
        let back = self.board.back && def.has_back();
        // A / B
        let abr = Rect::from_min_size(pos2(x0, bar.min.y), vec2(px(84.0), bar.height()));
        let ab = self.board.ab.filter(|a| a.uid == blk.uid);
        let on_b = ab.is_some_and(|a| a.on_b);
        let rects = pills(&p, abr, &["A".into(), "B".into()], Some(on_b as usize), 11.0);
        for (n, rr) in rects.iter().enumerate() {
            let resp = interact(ui, *rr, ("board-ab", n), Sense::click());
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                self.tip = Some((
                    "Two settings of this block to compare".into(),
                    "change one, then flip between A and B".into(),
                    pos2(rr.center().x, rr.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.clicked() {
                self.ab_pick(st, blk, n == 1);
            }
        }
        let mut x = abr.max.x + px(10.0);
        let small = Style::new(11.0, 700, th.caption);
        match ab {
            Some(a) => {
                let n = (0..def.knobs.len())
                    .filter(|&k| (a.other[k] - blk.v[k]).abs() > 1e-6)
                    .count();
                let what = match n {
                    0 => "A and B are the same".to_string(),
                    1 => "1 knob differs".to_string(),
                    n => format!("{n} knobs differ"),
                };
                let wr = text::draw(
                    &p,
                    &what,
                    pos2(x, bar.center().y),
                    Align2::LEFT_CENTER,
                    small,
                    None,
                );
                x = wr.max.x + px(8.0);
                if n > 0 {
                    let label = if on_b { "copy B to A" } else { "copy A to B" };
                    let lw = text::width(&ctx, label, small);
                    let lr = Rect::from_min_size(pos2(x, bar.min.y), vec2(lw, bar.height()));
                    let resp = interact(ui, lr, "board-ab-copy", Sense::click());
                    text::draw(
                        &p,
                        label,
                        pos2(x, bar.center().y),
                        Align2::LEFT_CENTER,
                        Style::new(11.0, 800, th.accent_ink),
                        None,
                    );
                    if resp.hovered() {
                        ctx.set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.board.ab = Some(Ab { other: blk.v, ..a });
                    }
                    x = lr.max.x + px(10.0);
                }
            }
            None => {
                let wr = text::draw(
                    &p,
                    "B: a second setting",
                    pos2(x, bar.center().y),
                    Align2::LEFT_CENTER,
                    small,
                    None,
                );
                x = wr.max.x + px(10.0);
            }
        }
        // undo, redo, and what the last change was
        for (n, label) in ["Undo", "Redo"].into_iter().enumerate() {
            let can = if n == 0 {
                !self.board.hist.is_empty()
            } else {
                !self.board.redo.is_empty()
            };
            let w = widgets::button_width(&ctx, label, "", 12.0, false);
            let r = Rect::from_min_size(pos2(x, bar.min.y), vec2(w, bar.height()));
            let st_ = widgets::ButtonStyle {
                enabled: can,
                ..Default::default()
            }
            .size(12.0);
            if widgets::button(ui, r, ("board-ed-undo", n), label, "", st_).clicked() && can {
                if n == 0 {
                    self.undo_edit(st);
                } else {
                    self.redo_edit(st);
                }
            }
            x = r.max.x + px(6.0);
        }
        // the keys at the right: remove, defaults, on
        let rm = Rect::from_min_size(
            pos2(bar.max.x - bar.height(), bar.min.y),
            Vec2::splat(bar.height()),
        );
        let rm_resp = widgets::round_button(
            ui,
            rm,
            "board-ed-remove",
            Icon::XBold,
            10.0,
            RoundKind::Gem(keys::Gem::Ruby),
            false,
        );
        if rm_resp.hovered() {
            self.tip = Some((
                format!("Take {} off the board", def.name),
                String::new(),
                pos2(rm.center().x, rm.max.y + px(4.0)),
                Align2::RIGHT_TOP,
            ));
        }
        if rm_resp.clicked() {
            self.remove_block(st, blk.uid);
            return;
        }
        let dw = widgets::button_width(&ctx, "Defaults", "", 12.0, false) + px(6.0);
        let dr = Rect::from_min_size(pos2(rm.min.x - px(8.0) - dw, bar.min.y), vec2(dw, bar.height()));
        let mine = pb::model_default(&st.config, def.id).is_some();
        let dresp = widgets::button(
            ui,
            dr,
            "board-ed-defaults",
            "Defaults",
            "",
            widgets::ButtonStyle::default().size(12.0),
        );
        if dresp.hovered() {
            self.tip = Some((
                if mine {
                    "Back to your default for this model".into()
                } else {
                    "Back to the model's defaults".into()
                },
                String::new(),
                pos2(dr.center().x, dr.max.y + px(4.0)),
                Align2::CENTER_TOP,
            ));
        }
        if dresp.clicked() {
            self.block_defaults(st, blk);
        }
        let onr = Rect::from_min_size(
            pos2(dr.min.x - px(8.0) - px(70.0), bar.min.y),
            vec2(px(70.0), bar.height()),
        );
        let resp = interact(ui, onr, "board-ed-on", Sense::click());
        lamp_key(&p, onr, if blk.on { "ON" } else { "OFF" }, blk.on, resp.hovered());
        if resp.clicked() {
            self.toggle_block(st, blk.uid);
        }
        if let Some((_, label)) = self.board.hist.last() {
            text::draw(
                &p,
                &format!("\u{21b6} {label}"),
                pos2(x + px(4.0), bar.center().y),
                Align2::LEFT_CENTER,
                small,
                Some((onr.min.x - x - px(14.0)).max(0.0)),
            );
        }
        // the face (with the cab's mic pad before it) and the tip column
        let area = Rect::from_min_max(
            pos2(card.min.x + px(18.0), bar.max.y + px(14.0)),
            pos2(card.max.x - px(18.0), card.max.y - px(28.0)),
        );
        let guide_w = px(if back { GUIDE_W - 50.0 } else { GUIDE_W }).min(area.width() * 0.4);
        let mut face_area = Rect::from_min_max(area.min, pos2(area.max.x - guide_w - px(18.0), area.max.y));
        let mut hot: Option<HotCtl> = None;
        if def.face == Face::Cab && !back {
            let (used, h) = self.cab_pad(ui, st, face_area, blk);
            hot = h;
            face_area.min.x = used + px(24.0);
        }
        // (sized from its controls, P37.13: a drive's extras beside it; a cab keeps its art's size, its pad is apart)
        let bs = if def.face == Face::Cab {
            faces::board_size(&blk)
        } else {
            faces::unit_size(&blk)
        };
        let scale = (face_area.height() / bs.y).min(face_area.width() / bs.x).min(2.4);
        let hot = if back {
            // the art behind the panel; its controls are in the card under it (last frame's hover explained here)
            self.back_art(ui, st, face_area, blk);
            self.board.back_hot.map(|k| HotCtl {
                knob: k,
                rect: Rect::NOTHING,
                ctl: Ctl::Pad,
            })
        } else {
            let face_r = Rect::from_center_size(face_area.center(), bs * scale);
            let fh = self.face_ui(ui, st, blk, face_r, Detail::Full, "editor");
            hot.or(fh)
        };
        // its bubble and sweet spot
        let now = self.board_now(st);
        let blk = now.index_of(blk.uid).map(|j| now.blocks[j]).unwrap_or(blk);
        if let Some(h) = hot
            && h.ctl != Ctl::Pad
        {
            let kd = def.knobs[h.knob];
            if let Some(tip) = gi::tip(def.id, kd.key) {
                sweet_mark(&p, &h, &kd, tip.sweet);
            }
            let lift = if h.ctl == Ctl::Knob { px(16.0) } else { px(4.0) };
            let top = pos2(h.rect.center().x, h.rect.min.y - lift);
            // (no room over it: under it)
            let under = top.y - px(38.0) < area.min.y - px(10.0);
            let at = if under {
                pos2(h.rect.center().x, h.rect.max.y + lift)
            } else {
                top
            };
            bubble_at(&p, at, kd.label, &kd.text(blk.v[h.knob]), under);
        }
        let guide = Rect::from_min_max(pos2(area.max.x - guide_w, area.min.y), area.max);
        if hot.is_none() {
            self.guide_rows(ui, st, guide, blk);
        }
        self.knob_guide(&p, guide, &blk, hot);
        // the line under it
        let cy = card.max.y - px(16.0);
        let line = if back && def.kind == Kind::Amp {
            format!(
                "the back of the head: the tubes glow with what the board plays \u{b7} after {}",
                def.after
            )
        } else if back {
            format!("inside the box: the clipping diodes \u{b7} after {}", def.after)
        } else {
            format!(
                "after {} \u{b7} drawn at {:.1}\u{d7} its board size",
                def.after, scale
            )
        };
        text::draw(
            &p,
            &line,
            pos2(card.min.x + px(18.0), cy),
            Align2::LEFT_CENTER,
            Style::new(10.5, 600, th.caption),
            Some(card.width() * 0.5),
        );
        text::draw(
            &p,
            "drag \u{b7} Shift fine \u{b7} wheel \u{b7} double-click resets",
            pos2(card.max.x - px(18.0), cy),
            Align2::RIGHT_CENTER,
            Style::new(10.5, 600, th.caption),
            Some(card.width() * 0.45),
        );
    }

    /// Beside the face: the knob under the mouse explained, with its sweet spot; else every knob at a glance.
    /// The knob list's rows (the same places `knob_guide` draws them) as controls: drag along a row's scale (or
    /// click on it) to set it, click a switch's row to step it, the wheel turns it, double-click resets (B3: the list
    /// looked like sliders but was read-only).
    fn guide_rows(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Block) {
        let def = blk.def();
        let shown: Vec<(usize, board::KnobDef)> = if self.board.back && def.has_back() {
            def.back_knobs().map(|(k, kd)| (k, *kd)).collect()
        } else {
            def.front_knobs().iter().copied().enumerate().collect()
        };
        let y0 = r.min.y + px(8.0);
        let n = shown.len().max(1);
        let rh = ((r.height() - px(24.0)) / n as f32).min(px(26.0));
        let label_w = r.width() * 0.3;
        let value_w = r.width() * 0.26;
        let (x0, x1) = (r.min.x + label_w + value_w + px(4.0), r.max.x - px(6.0));
        for (row, (k, kd)) in shown.into_iter().enumerate() {
            let y = y0 + px(22.0) + rh * (row as f32 + 0.5);
            let rr = Rect::from_min_max(pos2(r.min.x, y - rh / 2.0), pos2(r.max.x, y + rh / 2.0));
            self.board.wheel.push(super::WheelSpot {
                uid: blk.uid,
                knob: k,
                rect: rr,
            });
            let resp = interact(ui, rr, ("guide-row", blk.uid, k), Sense::click_and_drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if resp.double_clicked() {
                self.set_knob(st, blk.uid, k, kd.def);
                self.board_commit(st);
                continue;
            }
            match kd.unit {
                Unit::Choice(opts) => {
                    if resp.clicked() {
                        let v = (blk.v[k].round() + 1.0).rem_euclid(opts.len().max(1) as f32);
                        self.set_knob(st, blk.uid, k, v);
                        self.board_commit(st);
                    }
                }
                _ => {
                    if (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
                        && let Some(pos) = resp.interact_pointer_pos()
                    {
                        let u = ((pos.x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0);
                        let v = kd.from_u(u);
                        if (v - blk.v[k]).abs() > 1e-6 {
                            self.set_knob(st, blk.uid, k, v);
                        }
                    }
                    if resp.drag_stopped() || resp.clicked() {
                        self.board_commit(st);
                    }
                }
            }
        }
    }

    fn knob_guide(&self, p: &Painter, r: Rect, blk: &Block, hot: Option<HotCtl>) {
        let th = t();
        let ctx = p.ctx();
        let def = blk.def();
        let scale = |y: f32, kd: &board::KnobDef, v: f32, sweet: Option<(f32, f32)>, x0: f32, x1: f32| {
            let g = Rect::from_min_max(pos2(x0, y - px(3.5)), pos2(x1, y + px(3.5)));
            paint::inset(p, g, Some(4.0), 1.5, Fill::Color(th.track));
            if let Some((lo, hi)) = sweet {
                let (a, b) = (kd.to_u(lo), kd.to_u(hi));
                let band = Rect::from_min_max(
                    pos2(g.min.x + g.width() * a - 1.0, g.min.y),
                    pos2(g.min.x + g.width() * b + 1.0, g.max.y),
                );
                p.rect_filled(band, 3.0, th.accent.gamma_multiply(0.75));
            }
            let u = kd.to_u(v);
            let c = pos2(g.min.x + g.width() * u, g.center().y);
            p.circle_filled(c, px(5.5), Color32::from_rgb(240, 240, 236));
            p.circle_stroke(c, px(5.5), Stroke::new(1.0, Color32::from_black_alpha(120)));
        };
        match hot {
            Some(h) => {
                let kd = def.knobs[h.knob];
                let mut y = r.min.y + px(8.0);
                let lr = caps(
                    p,
                    &kd.label.to_uppercase(),
                    pos2(r.min.x, y),
                    11.0,
                    th.accent_ink,
                    Align2::LEFT_CENTER,
                );
                text::draw(
                    p,
                    &kd.text(blk.v[h.knob]),
                    pos2(lr.max.x + px(8.0), y),
                    Align2::LEFT_CENTER,
                    Style::new(13.0, 800, th.text),
                    None,
                );
                y += px(20.0);
                let Some(tip) = gi::tip(def.id, kd.key) else {
                    return;
                };
                let st = Style::new(12.5, 500, th.text_2);
                let lh = px(18.0);
                let lines = text::wrap(ctx, tip.text, st, r.width());
                let room = ((r.max.y - y - px(70.0)) / lh).floor().max(2.0) as usize;
                for line in lines.iter().take(room) {
                    text::draw(
                        p,
                        line,
                        pos2(r.min.x, y + lh / 2.0),
                        Align2::LEFT_CENTER,
                        st,
                        None,
                    );
                    y += lh;
                }
                y += px(10.0);
                match kd.unit {
                    Unit::Choice(opts) => {
                        let w = r.width() / opts.len().max(1) as f32;
                        for (n, o) in opts.iter().enumerate() {
                            let orr = Rect::from_min_size(
                                pos2(r.min.x + w * n as f32, y - px(10.0)),
                                vec2(w - px(4.0), px(22.0)),
                            );
                            let on = blk.v[h.knob].round() as usize == n;
                            let good = (n as f32) >= tip.sweet.0 - 0.01 && (n as f32) <= tip.sweet.1 + 0.01;
                            p.rect_filled(
                                orr,
                                6.0,
                                if on {
                                    th.accent.gamma_multiply(0.22)
                                } else {
                                    th.screen_hi
                                },
                            );
                            if good {
                                p.rect_stroke(
                                    orr,
                                    6.0,
                                    Stroke::new(1.2, th.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            text::draw(
                                p,
                                o,
                                orr.center(),
                                Align2::CENTER_CENTER,
                                Style::new(10.5, 800, if on { th.accent_ink } else { th.text_2 }),
                                Some(orr.width() - px(4.0)),
                            );
                        }
                        y += px(22.0);
                    }
                    _ => {
                        scale(
                            y,
                            &kd,
                            blk.v[h.knob],
                            Some(tip.sweet),
                            r.min.x + px(6.0),
                            r.max.x - px(6.0),
                        );
                        let ls = Style::new(9.5, 700, th.caption);
                        text::draw(
                            p,
                            &kd.text(kd.lo),
                            pos2(r.min.x, y + px(14.0)),
                            Align2::LEFT_CENTER,
                            ls,
                            None,
                        );
                        text::draw(
                            p,
                            &kd.text(kd.hi),
                            pos2(r.max.x, y + px(14.0)),
                            Align2::RIGHT_CENTER,
                            ls,
                            None,
                        );
                        y += px(28.0);
                    }
                }
                // the sweet spot and when it holds
                let chip = format!("Sweet spot {}", sweet_text(&kd, tip.sweet));
                let cs = Style::new(10.5, 800, th.accent_ink);
                let cw = text::width(ctx, &chip, cs) + px(16.0);
                let cr = Rect::from_min_size(pos2(r.min.x, y), vec2(cw.min(r.width()), px(22.0)));
                p.rect_stroke(cr, 6.0, Stroke::new(1.2, th.accent), egui::StrokeKind::Inside);
                text::draw(
                    p,
                    &chip,
                    cr.center(),
                    Align2::CENTER_CENTER,
                    cs,
                    Some(cr.width() - px(6.0)),
                );
                y = cr.max.y + px(4.0);
                let ns = Style::new(11.0, 600, th.caption);
                for line in text::wrap(ctx, tip.note, ns, r.width()).iter().take(2) {
                    if y + px(15.0) > r.max.y + px(14.0) {
                        break;
                    }
                    text::draw(p, line, pos2(r.min.x, y + px(7.5)), Align2::LEFT_CENTER, ns, None);
                    y += px(15.0);
                }
            }
            None => {
                let y0 = r.min.y + px(8.0);
                caps(
                    p,
                    "THE KNOBS",
                    pos2(r.min.x, y0),
                    10.5,
                    th.caption,
                    Align2::LEFT_CENTER,
                );
                text::draw(
                    p,
                    "drag a row to set it, or hover the face",
                    pos2(r.max.x, y0),
                    Align2::RIGHT_CENTER,
                    Style::new(10.5, 600, th.caption),
                    None,
                );
                // the knobs on the side shown: the face's, or the ones behind the panel
                let shown: Vec<(usize, &board::KnobDef)> = if self.board.back && def.has_back() {
                    def.back_knobs().collect()
                } else {
                    def.front_knobs().iter().enumerate().collect()
                };
                let n = shown.len().max(1);
                let rh = ((r.height() - px(24.0)) / n as f32).min(px(26.0));
                let label_w = r.width() * 0.3;
                let value_w = r.width() * 0.26;
                for (row, &(k, kd)) in shown.iter().enumerate() {
                    let y = y0 + px(22.0) + rh * (row as f32 + 0.5);
                    text::draw(
                        p,
                        kd.label,
                        pos2(r.min.x, y),
                        Align2::LEFT_CENTER,
                        Style::new(11.5, 700, th.text_2),
                        Some(label_w - px(4.0)),
                    );
                    text::draw(
                        p,
                        &crate::pedalfaces::silver::knob_text(blk, k),
                        pos2(r.min.x + label_w + value_w - px(8.0), y),
                        Align2::RIGHT_CENTER,
                        Style::new(11.5, 800, th.text),
                        Some(value_w - px(8.0)),
                    );
                    let sweet = gi::tip(def.id, kd.key).map(|t| t.sweet);
                    if let Unit::Choice(_) = kd.unit {
                        let good = sweet.is_some_and(|(a, b)| blk.v[k] >= a - 0.01 && blk.v[k] <= b + 0.01);
                        text::draw(
                            p,
                            if good { "\u{2713} sweet spot" } else { "" },
                            pos2(r.min.x + label_w + value_w + px(4.0), y),
                            Align2::LEFT_CENTER,
                            Style::new(10.0, 700, th.accent_ink),
                            None,
                        );
                    } else {
                        scale(
                            y,
                            kd,
                            blk.v[k],
                            sweet,
                            r.min.x + label_w + value_w + px(4.0),
                            r.max.x - px(6.0),
                        );
                    }
                }
            }
        }
    }

    /// The cab's mic as a dot on the speaker (how far off the centre is the Mic knob) and its distance, a slider.
    /// Returns where they end (x) and the one under the mouse.
    fn cab_pad(&mut self, ui: &Ui, st: &mut AppState, area: Rect, blk: Block) -> (f32, Option<HotCtl>) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let def = blk.def();
        let (Some(km), Some(kdist)) = (def.knob_index("mic"), def.knob_index("distance")) else {
            return (area.min.x, None);
        };
        let side = area.height().min(area.width() * 0.5);
        let pad = Rect::from_min_size(pos2(area.min.x, area.center().y - side / 2.0), Vec2::splat(side));
        let c = pad.center();
        let big_r = side / 2.0 - px(4.0);
        // the speaker: basket, surround, cone, dust cap
        p.circle_filled(c + vec2(0.0, 4.0), big_r, Color32::from_black_alpha(110));
        p.circle_filled(c, big_r, Color32::from_rgb(34, 35, 38));
        for n in 0..8 {
            let a = n as f32 / 8.0 * std::f32::consts::TAU + 0.39;
            p.circle_filled(
                c + vec2(a.cos(), a.sin()) * big_r * 0.955,
                px(2.6),
                Color32::from_rgb(150, 154, 160),
            );
        }
        p.circle_filled(c, big_r * 0.9, Color32::from_rgb(24, 24, 26));
        let steps = 14;
        for n in 0..steps {
            let f = 0.86 - 0.5 * n as f32 / steps as f32;
            let shade = 38 + (n * 2) as u8;
            p.circle_filled(c, big_r * f, Color32::from_rgb(shade, shade - 2, shade - 5));
        }
        p.circle_filled(c, big_r * 0.32, Color32::from_rgb(28, 28, 30));
        p.circle_stroke(c, big_r * 0.32, Stroke::new(1.5, Color32::from_white_alpha(22)));
        for f in [0.32, 0.62, 0.86] {
            p.circle_stroke(c, big_r * f, Stroke::new(1.0, Color32::from_white_alpha(14)));
        }
        let lab = |s: &str, at: Pos2| {
            text::caps(
                &p,
                s,
                at,
                8.5,
                Color32::from_white_alpha(120),
                Align2::CENTER_CENTER,
                None,
            );
        };
        lab("CENTRE", c + vec2(0.0, big_r * 0.12));
        lab("CAP EDGE", c + vec2(0.0, big_r * 0.4));
        lab("CONE", c + vec2(0.0, big_r * 0.7));
        lab("EDGE", c + vec2(0.0, big_r * 0.93));
        // the mic: dragged anywhere on the speaker, its distance from the centre is the knob
        let resp = interact(ui, pad, ("board-cab-pad", blk.uid), Sense::click_and_drag());
        let mut hot = None;
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
        }
        let reach = big_r * 0.95;
        if (resp.is_pointer_button_down_on() || resp.dragged())
            && let Some(q) = resp.interact_pointer_pos()
        {
            let d = q - c;
            self.board.mic_angle = d.y.atan2(d.x);
            let v = (d.length() / reach * 10.0).clamp(0.0, 10.0);
            self.board.pad_drag = Some(km);
            if (v - blk.v[km]).abs() > 1e-4 {
                self.set_knob(st, blk.uid, km, v);
            }
        } else if self.board.pad_drag == Some(km) {
            self.board.pad_drag = None;
            self.board_commit(st);
        }
        let now = self.board_now(st);
        let blk = now.index_of(blk.uid).map(|j| now.blocks[j]).unwrap_or(blk);
        let mv = blk.v[km];
        let a = self.board.mic_angle;
        let dot = c + vec2(a.cos(), a.sin()) * (mv / 10.0 * reach);
        let dashes = 10;
        for n in 0..dashes {
            if n % 2 == 1 {
                continue;
            }
            let t0 = n as f32 / dashes as f32;
            let t1 = (n + 1) as f32 / dashes as f32;
            p.line_segment(
                [c + (dot - c) * t0, c + (dot - c) * t1],
                Stroke::new(1.5, th.accent.gamma_multiply(0.8)),
            );
        }
        p.circle_filled(c, px(2.5), th.accent);
        paint::halo(&p, dot, px(22.0), th.accent, 60);
        p.circle_stroke(dot, px(13.0), Stroke::new(2.5, th.accent));
        p.circle_filled(dot, px(7.0), Color32::from_rgb(70, 70, 66));
        p.circle_stroke(dot, px(7.0), Stroke::new(1.5, Color32::from_rgb(200, 196, 180)));
        let zone = match mv {
            v if v < 1.5 => "Centre",
            v if v < 4.5 => "Cap edge",
            v if v < 7.5 => "Cone",
            _ => "Edge",
        };
        let pad_hot = resp.hovered() || resp.dragged() || self.board.shot_hover.as_deref() == Some("mic");
        if pad_hot {
            // (a 12-inch speaker: 6 inches from the centre to the edge)
            bubble(
                &p,
                dot - vec2(0.0, px(14.0)),
                zone,
                &format!("{:.1} in off centre", mv / 10.0 * 6.0),
            );
            hot = Some(HotCtl {
                knob: km,
                rect: pad,
                ctl: Ctl::Pad,
            });
        }
        // the distance: a slider standing beside it, close at the bottom
        let gx = pad.max.x + px(40.0);
        let groove = Rect::from_min_max(
            pos2(gx - px(4.0), pad.min.y + px(22.0)),
            pos2(gx + px(4.0), pad.max.y - px(10.0)),
        );
        caps(
            &p,
            "DISTANCE",
            pos2(gx, pad.min.y + px(8.0)),
            9.5,
            th.caption,
            Align2::CENTER_CENTER,
        );
        paint::inset(&p, groove, Some(4.0), 1.5, Fill::Color(th.track));
        let dv = blk.v[kdist];
        let ty = groove.max.y - groove.height() * (dv / 10.0);
        p.rect_filled(
            Rect::from_min_max(
                pos2(groove.min.x + 1.5, ty),
                pos2(groove.max.x - 1.5, groove.max.y - 1.5),
            ),
            3.0,
            th.accent,
        );
        for (f, s) in [(0.0, "close"), (1.0, "far")] {
            text::draw(
                &p,
                s,
                pos2(groove.max.x + px(8.0), groove.max.y - groove.height() * f),
                Align2::LEFT_CENTER,
                Style::new(9.5, 700, th.caption),
                None,
            );
        }
        let hit = groove.expand2(vec2(px(16.0), px(8.0)));
        let dresp = interact(ui, hit, ("board-cab-dist", blk.uid), Sense::click_and_drag());
        if dresp.hovered() || dresp.dragged() {
            ctx.set_cursor_icon(CursorIcon::ResizeVertical);
        }
        if (dresp.is_pointer_button_down_on() || dresp.dragged())
            && let Some(q) = dresp.interact_pointer_pos()
        {
            let v = ((groove.max.y - q.y) / groove.height() * 10.0).clamp(0.0, 10.0);
            self.board.pad_drag = Some(kdist);
            if (v - dv).abs() > 1e-4 {
                self.set_knob(st, blk.uid, kdist, v);
            }
        } else if self.board.pad_drag == Some(kdist) {
            self.board.pad_drag = None;
            self.board_commit(st);
        }
        keys::thumb(&p, pos2(gx, ty), px(20.0));
        if dresp.hovered() || dresp.dragged() {
            let words = match dv {
                v if v < 2.5 => "on the grille",
                v if v < 5.0 => "a hand away",
                v if v < 8.0 => "a foot away",
                _ => "arm's length",
            };
            bubble(&p, pos2(gx, ty - px(12.0)), "Distance", words);
            hot = Some(HotCtl {
                knob: kdist,
                rect: groove,
                ctl: Ctl::Pad,
            });
        }
        (groove.max.x + px(40.0), hot)
    }

    /// Named starting points for the model, your default, and the key to save one.
    fn starts_card(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Block) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let def = blk.def();
        paint::display(&p, r, Some(18.0), 6.0);
        let inner = r.shrink2(vec2(px(16.0), px(12.0)));
        caps(
            &p,
            "STARTING POINTS",
            pos2(inner.min.x, inner.min.y + px(6.0)),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        text::draw(
            &p,
            &format!("for {}", def.name),
            pos2(inner.max.x, inner.min.y + px(6.0)),
            Align2::RIGHT_CENTER,
            Style::new(11.0, 600, th.caption),
            Some(inner.width() * 0.45),
        );
        let mut chips: Vec<(&'static str, [f32; board::MAX_KNOBS])> = Vec::new();
        let mine = pb::model_default(&st.config, def.id);
        if let Some(v) = mine {
            chips.push(("Your default", v));
        }
        if let Some(a) = gi::about(def.id) {
            for s in a.starts {
                chips.push((s.name, gi::start_values(def, s)));
            }
        }
        let keys_h = px(32.0);
        let chip_h = px(28.0);
        let (mut x, mut y) = (inner.min.x, inner.min.y + px(22.0));
        let cs = Style::new(12.0, 800, th.text_2);
        for (name, v) in chips {
            let on = same_values(def, &blk.v, &v);
            let edited = !on && self.board.last_start == Some((blk.uid, name));
            let label = if edited {
                format!("{name} \u{b7} edited")
            } else {
                name.to_string()
            };
            let w = (text::width(&ctx, &label, cs) + px(24.0)).min(inner.width());
            if x + w > inner.max.x {
                x = inner.min.x;
                y += chip_h + px(7.0);
            }
            if y + chip_h > inner.max.y - keys_h - px(6.0) {
                break;
            }
            let cr = Rect::from_min_size(pos2(x, y), vec2(w, chip_h));
            let resp = interact(ui, cr, ("board-start", name), Sense::click());
            let lit = on || edited;
            p.rect_filled(
                cr,
                chip_h / 2.0,
                if lit {
                    th.accent.gamma_multiply(0.16)
                } else if resp.hovered() {
                    th.screen_hi
                } else {
                    th.screen_hi.gamma_multiply(0.7)
                },
            );
            p.rect_stroke(
                cr,
                chip_h / 2.0,
                Stroke::new(
                    if lit { 1.3 } else { 1.0 },
                    if lit { th.accent } else { th.divider },
                ),
                egui::StrokeKind::Inside,
            );
            text::draw(
                &p,
                &label,
                cr.center(),
                Align2::CENTER_CENTER,
                Style::new(12.0, 800, if lit { th.accent_ink } else { th.text_2 }),
                Some(cr.width() - px(10.0)),
            );
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                let what: Vec<String> = def
                    .knobs
                    .iter()
                    .enumerate()
                    .map(|(k, kd)| format!("{} {}", kd.label, kd.text(v[k])))
                    .collect();
                self.tip = Some((
                    name.to_string(),
                    what.join(" \u{b7} "),
                    pos2(cr.center().x, cr.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.clicked() {
                self.apply_values(st, blk.uid, v);
                self.board.last_start = Some((blk.uid, name));
            }
            x = cr.max.x + px(7.0);
        }
        // save as my default (and forget it)
        let ky = inner.max.y - keys_h;
        let label = "Save as my default";
        let w = widgets::button_width(&ctx, label, "", 12.0, false) + px(6.0);
        let sr = Rect::from_min_size(pos2(inner.min.x, ky), vec2(w, keys_h));
        let resp = widgets::button(
            ui,
            sr,
            "board-save-default",
            label,
            "",
            widgets::ButtonStyle::default().size(12.0),
        );
        if resp.hovered() {
            self.tip = Some((
                format!("New {} blocks start like this one", def.name),
                "and Defaults comes back to it".into(),
                pos2(sr.center().x, sr.max.y + px(4.0)),
                Align2::CENTER_TOP,
            ));
        }
        if resp.clicked() {
            pb::set_model_default(&mut st.config, &blk);
            st.save_config();
            self.say(
                format!("Saved as your default for {}", def.name),
                "  \u{b7}  new ones start like this",
            );
        }
        if mine.is_some() {
            let label = "Forget it";
            let w2 = widgets::button_width(&ctx, label, "", 12.0, false) + px(6.0);
            let fr = Rect::from_min_size(pos2(sr.max.x + px(8.0), ky), vec2(w2, keys_h));
            if fr.max.x <= inner.max.x
                && widgets::button(
                    ui,
                    fr,
                    "board-forget-default",
                    label,
                    "",
                    widgets::ButtonStyle::default().size(12.0),
                )
                .clicked()
            {
                pb::clear_model_default(&mut st.config, def.id);
                st.save_config();
                self.say(format!("{} starts at its own defaults again", def.name), "");
            }
        }
    }

    /// About the model: the unit it's after, its era, what it is, famous records, your songs, styles.
    fn about_card(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Block) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let def = blk.def();
        paint::display(&p, r, Some(18.0), 6.0);
        let Some(a) = gi::about(def.id) else {
            return;
        };
        let (x0, x1) = (r.min.x + px(18.0), r.max.x - px(18.0));
        let w = x1 - x0;
        let bottom = r.max.y - px(14.0);
        let mut y = r.min.y + px(18.0);
        let ar = caps(
            &p,
            "ABOUT THIS MODEL",
            pos2(x0, y),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        let info = gi::info(def.id);
        let inspired = info.map_or_else(|| format!("after {}", gi::real_unit(a)), |i| i.inspired_line());
        text::draw(
            &p,
            &inspired,
            pos2(x1, y),
            Align2::RIGHT_CENTER,
            Style::new(11.0, 600, th.caption),
            Some(x1 - ar.max.x - px(12.0)),
        );
        y += px(26.0);
        let nr = text::draw(
            &p,
            def.name,
            pos2(x0, y),
            Align2::LEFT_CENTER,
            Style::new(19.0, 800, th.text),
            Some(w * 0.6),
        );
        text::draw(
            &p,
            a.facts,
            pos2(nr.max.x + px(10.0), y + px(1.0)),
            Align2::LEFT_CENTER,
            Style::new(11.5, 600, th.caption),
            Some(x1 - nr.max.x - px(10.0)),
        );
        y += px(22.0);
        // its era on a line from 1950 to 2025
        if bottom - y > px(120.0) {
            let (y0, y1) = (1950.0, 2025.0);
            let ly = y + px(8.0);
            p.line_segment([pos2(x0, ly), pos2(x1, ly)], Stroke::new(3.0, th.track));
            let ls = Style::new(9.0, 700, th.caption);
            text::draw(&p, "1950", pos2(x0, ly - px(9.0)), Align2::LEFT_CENTER, ls, None);
            text::draw(&p, "2025", pos2(x1, ly - px(9.0)), Align2::RIGHT_CENTER, ls, None);
            let f = ((a.year as f32 - y0) / (y1 - y0)).clamp(0.0, 1.0);
            let dx = x0 + w * f;
            paint::halo(&p, pos2(dx, ly), px(10.0), th.accent, 90);
            p.circle_filled(pos2(dx, ly), px(5.5), th.accent);
            text::draw(
                &p,
                &a.year.to_string(),
                pos2(dx.clamp(x0 + px(14.0), x1 - px(14.0)), ly + px(13.0)),
                Align2::CENTER_CENTER,
                Style::new(10.0, 800, th.accent_ink),
                None,
            );
            y += px(34.0);
        }
        // what it is
        let ws = Style::new(12.5, 500, th.text_2);
        let lh = px(18.0);
        let lines = text::wrap(&ctx, info.map_or(a.what, |i| i.about), ws, w);
        let fit = ((bottom - y - px(60.0)) / lh).floor().max(2.0) as usize;
        for (n, line) in lines.iter().take(fit).enumerate() {
            let mut s = line.clone();
            if n + 1 == fit && lines.len() > fit {
                s.push('\u{2026}');
            }
            text::draw(&p, &s, pos2(x0, y + lh / 2.0), Align2::LEFT_CENTER, ws, Some(w));
            y += lh;
        }
        // its sound and who played it (P37.14)
        if let Some(i) = info {
            let ps = Style::new(11.5, 600, th.caption);
            let mut more = vec![(i.character.to_string(), Style::new(12.0, 700, th.text))];
            if !i.players.is_empty() {
                more.push((format!("Played by {}", i.players.join(", ")), ps));
            }
            more.push((format!("Era: {}", i.era), ps));
            for (s, st) in more {
                for line in text::wrap(&ctx, &s, st, w) {
                    if bottom - y < px(90.0) {
                        break;
                    }
                    text::draw(
                        &p,
                        &line,
                        pos2(x0, y + lh / 2.0),
                        Align2::LEFT_CENTER,
                        st,
                        Some(w),
                    );
                    y += lh;
                }
            }
        }
        y += px(10.0);
        // heard on
        if !a.heard.is_empty() && bottom - y > px(46.0) {
            caps(
                &p,
                "HEARD ON",
                pos2(x0, y + px(6.0)),
                10.5,
                th.caption,
                Align2::LEFT_CENTER,
            );
            y += px(18.0);
            for h in a.heard {
                if bottom - y < px(24.0) {
                    break;
                }
                let cy = y + px(11.0);
                let sr = text::draw(
                    &p,
                    h.song,
                    pos2(x0, cy),
                    Align2::LEFT_CENTER,
                    Style::new(12.5, 800, th.text),
                    Some(w * 0.55),
                );
                text::draw(
                    &p,
                    h.artist,
                    pos2(sr.max.x + px(8.0), cy),
                    Align2::LEFT_CENTER,
                    Style::new(11.5, 600, th.caption),
                    Some((x1 - sr.max.x - px(50.0)).max(0.0)),
                );
                text::draw(
                    &p,
                    &h.year.to_string(),
                    pos2(x1, cy),
                    Align2::RIGHT_CENTER,
                    Style::new(11.5, 700, th.caption),
                    None,
                );
                y += px(24.0);
                p.line_segment(
                    [pos2(x0, y - px(1.0)), pos2(x1, y - px(1.0))],
                    Stroke::new(1.0, th.divider),
                );
            }
            y += px(8.0);
        }
        // the songs in your library whose tones use it
        if bottom - y > px(50.0) {
            let mut list: Vec<(String, String, String, bool)> = Vec::new();
            let here = self.board.rig.as_ref().is_some_and(|rig| {
                rig.tones
                    .iter()
                    .any(|t| t.board.blocks.iter().any(|b| b.def().id == def.id))
            });
            if here {
                list.push((
                    self.song.sid.clone(),
                    self.song.display_title.clone(),
                    self.song.artist.clone(),
                    true,
                ));
            }
            for (id, title, artist) in gearindex::songs_with(def.id) {
                if id != self.song.sid {
                    list.push((id, title, artist, false));
                }
            }
            let lr = caps(
                &p,
                "IN YOUR LIBRARY",
                pos2(x0, y + px(6.0)),
                10.5,
                th.caption,
                Align2::LEFT_CENTER,
            );
            let status = match gearindex::reading() {
                Some((n, total)) => format!("reading your songs\u{2026} {n} of {total}"),
                None if list.is_empty() => "no song's tone uses it yet".into(),
                None if list.len() == 1 => "1 song's tone uses it".into(),
                None => format!("{} songs' tones use it", list.len()),
            };
            text::draw(
                &p,
                &status,
                pos2(lr.max.x + px(8.0), y + px(6.0)),
                Align2::LEFT_CENTER,
                Style::new(10.5, 600, th.caption),
                Some(x1 - lr.max.x - px(8.0)),
            );
            y += px(18.0);
            let ch = px(28.0);
            let rows = if bottom - y > ch * 2.0 + px(50.0) { 2 } else { 1 };
            let (mut cx, mut row) = (x0, 0);
            let ts = Style::new(11.5, 800, th.text);
            let as_ = Style::new(10.5, 600, th.caption);
            for (n, (id, title, artist, this)) in list.iter().enumerate() {
                let tail = if *this {
                    "this song".to_string()
                } else {
                    artist.clone()
                };
                let cw = (px(30.0)
                    + text::width(&ctx, title, ts)
                    + px(6.0)
                    + text::width(&ctx, &tail, as_)
                    + px(12.0))
                .min(w);
                let more_w = px(76.0);
                let last_row = row + 1 == rows;
                let left = list.len() - n;
                if cx + cw > x1 - if last_row && left > 1 { more_w } else { 0.0 } {
                    if last_row {
                        // the rest as "+N more"
                        let mr = Rect::from_min_size(pos2(cx, y), vec2(more_w, ch));
                        let resp = interact(ui, mr, "board-lib-more", Sense::hover());
                        text::draw(
                            &p,
                            &format!("+{left} more"),
                            mr.left_center() + vec2(px(6.0), 0.0),
                            Align2::LEFT_CENTER,
                            Style::new(11.5, 800, th.accent_ink),
                            None,
                        );
                        if resp.hovered() {
                            let names: Vec<&str> = list[n..].iter().take(12).map(|x| x.1.as_str()).collect();
                            self.tip = Some((
                                names.join(" \u{b7} "),
                                String::new(),
                                pos2(mr.center().x, mr.max.y + px(4.0)),
                                Align2::RIGHT_TOP,
                            ));
                        }
                        break;
                    }
                    row += 1;
                    cx = x0;
                    y += ch + px(6.0);
                }
                let cr = Rect::from_min_size(pos2(cx, y), vec2(cw, ch));
                let resp = interact(ui, cr, ("board-lib-song", n), Sense::click());
                p.rect_filled(
                    cr,
                    ch / 2.0,
                    if resp.hovered() { th.screen_hi } else { th.screen },
                );
                let dc = pos2(cr.min.x + px(14.0), cr.center().y);
                p.circle_filled(dc, px(8.5), Color32::from_rgb(240, 236, 226));
                qrs_ui::icons::icon(&p, Icon::Play, dc, px(8.0), IVORY_INK);
                let tr = text::draw(
                    &p,
                    title,
                    pos2(cr.min.x + px(28.0), cr.center().y),
                    Align2::LEFT_CENTER,
                    ts,
                    Some(cw - px(40.0)),
                );
                text::draw(
                    &p,
                    &tail,
                    pos2(tr.max.x + px(6.0), cr.center().y),
                    Align2::LEFT_CENTER,
                    as_,
                    Some((cr.max.x - tr.max.x - px(12.0)).max(0.0)),
                );
                if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                    self.tip = Some((
                        format!("{title} \u{b7} {artist}"),
                        if *this {
                            "playing now".into()
                        } else {
                            "click to play it".into()
                        },
                        pos2(cr.center().x, cr.max.y + px(4.0)),
                        Align2::CENTER_TOP,
                    ));
                }
                if resp.clicked() && !*this {
                    self.board_commit(st);
                    self.reqs.push(PlayerReq::Song { id: id.clone() });
                }
                cx = cr.max.x + px(6.0);
            }
            y += ch + px(12.0);
        }
        // styles it suits
        if !a.try_for.is_empty() && bottom - y > px(24.0) {
            let tr = caps(
                &p,
                "TRY IT FOR",
                pos2(x0, y + px(10.0)),
                10.5,
                th.caption,
                Align2::LEFT_CENTER,
            );
            let mut cx = tr.max.x + px(10.0);
            for tag in a.try_for {
                let s = tag.to_uppercase();
                let tw = text::width(&ctx, &s, Style::caps(9.5, th.text_2)) + px(16.0);
                if cx + tw > x1 {
                    break;
                }
                let r = Rect::from_min_size(pos2(cx, y), vec2(tw, px(20.0)));
                p.rect_filled(r, 6.0, th.screen_hi);
                caps(&p, &s, r.center(), 9.5, th.text_2, Align2::CENTER_CENTER);
                cx = r.max.x + px(6.0);
            }
        }
    }

    /// Tempo sync for a delay's time (`rate` None) or a modulation's rate (its LFO range): note values from the
    /// song's tempo, or a tapped one.
    fn tempo_card(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        r: Rect,
        blk: Block,
        k: usize,
        rate: Option<(f32, f32)>,
    ) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let def = blk.def();
        let kd = def.knobs[k];
        paint::display(&p, r, Some(18.0), 6.0);
        let inner = r.shrink2(vec2(px(16.0), px(12.0)));
        let song = self.song_bpm();
        let (bpm, src) = match (self.board.tap_bpm, song) {
            (Some(t), _) => (Some(t), "tapped"),
            (None, Some(s)) => (Some(s), "this song"),
            _ => (None, ""),
        };
        caps(
            &p,
            if rate.is_some() {
                "RATE TO THE BEAT"
            } else {
                "TIME TO THE BEAT"
            },
            pos2(inner.min.x, inner.min.y + px(6.0)),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        // the TAP key
        let d = (inner.height() * 0.42).clamp(px(48.0), px(64.0));
        let tc = pos2(inner.max.x - d / 2.0, inner.min.y + px(14.0) + d / 2.0);
        let tap = Rect::from_center_size(tc, Vec2::splat(d));
        let resp = interact(ui, tap, "board-tap", Sense::click());
        let cur_ms = |v: f32| match rate {
            Some(rg) => 1000.0 / gi::hz_for_dial(rg, v).max(0.01),
            None => v,
        };
        let sel = bpm.and_then(|b| gi::note_of(b, cur_ms(blk.v[k])));
        if resp.clicked() {
            let now = Instant::now();
            if self
                .board
                .taps
                .last()
                .is_some_and(|t| now.duration_since(*t) > Duration::from_millis(2000))
            {
                self.board.taps.clear();
            }
            self.board.taps.push(now);
            if self.board.taps.len() > 5 {
                self.board.taps.remove(0);
            }
            let n = self.board.taps.len();
            if n >= 2 {
                let span = self.board.taps[n - 1]
                    .duration_since(self.board.taps[0])
                    .as_secs_f32();
                let tb = (60.0 / (span / (n - 1) as f32)).clamp(40.0, 300.0);
                self.board.tap_bpm = Some(tb);
                // the note value it was on stays on, at the new tempo
                if let Some(i) = sel {
                    let ms = gi::note_ms(tb, gi::NOTES[i].1);
                    let v = match rate {
                        Some(rg) => gi::dial_for_hz(rg, 1000.0 / ms),
                        None => kd.clamp(ms),
                    };
                    self.set_knob(st, blk.uid, k, v);
                    self.board_commit(st);
                }
            }
        }
        keys::key_face(&p, tap, Some(d / 2.0), false, resp.is_pointer_button_down_on());
        if resp.hovered() {
            p.circle_stroke(
                tc,
                d / 2.0 + px(2.0),
                Stroke::new(1.2, th.accent.gamma_multiply(0.6)),
            );
        }
        text::draw(
            &p,
            "TAP",
            tc,
            Align2::CENTER_CENTER,
            Style::new(14.0, 800, keys::key_ink(false)),
            None,
        );
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        let lit = self.board.taps.len().min(4);
        for n in 0..4 {
            let c = pos2(tc.x + (n as f32 - 1.5) * px(9.0), tap.max.y + px(8.0));
            if n < lit {
                p.circle_filled(c, px(3.0), th.accent);
            } else {
                p.circle_filled(c, px(3.0), th.unlit);
            }
        }
        text::draw(
            &p,
            "tap 4 times",
            pos2(tc.x, tap.max.y + px(22.0)),
            Align2::CENTER_CENTER,
            Style::new(10.0, 600, th.caption),
            None,
        );
        // the note values
        let notes: &[usize] = if rate.is_some() {
            &[0, 1, 4, 5, 7]
        } else {
            &[1, 2, 3, 4, 5, 6, 7]
        };
        let left_max = tap.min.x - px(14.0);
        let (mut x, mut y) = (inner.min.x, inner.min.y + px(22.0));
        let ch = px(26.0);
        for &i in notes {
            let (label, q) = gi::NOTES[i];
            let ms = bpm.map(|b| gi::note_ms(b, q));
            let valid = ms.is_some_and(|ms| match rate {
                Some((lo, hi)) => (lo..=hi).contains(&(1000.0 / ms)),
                None => ms >= kd.lo && ms <= kd.hi,
            });
            let cs = Style::new(11.5, 800, th.text_2);
            let w = text::width(&ctx, label, cs) + px(20.0);
            if x + w > left_max {
                x = inner.min.x;
                y += ch + px(6.0);
            }
            let cr = Rect::from_min_size(pos2(x, y), vec2(w, ch));
            let resp = interact(ui, cr, ("board-note", i), Sense::click());
            let on = sel == Some(i);
            p.rect_filled(
                cr,
                ch / 2.0,
                if on {
                    th.accent.gamma_multiply(0.16)
                } else if resp.hovered() && valid {
                    th.screen_hi
                } else {
                    th.screen
                },
            );
            if on {
                p.rect_stroke(
                    cr,
                    ch / 2.0,
                    Stroke::new(1.3, th.accent),
                    egui::StrokeKind::Inside,
                );
            }
            text::draw(
                &p,
                label,
                cr.center(),
                Align2::CENTER_CENTER,
                Style::new(
                    11.5,
                    800,
                    if on {
                        th.accent_ink
                    } else if valid {
                        th.text_2
                    } else {
                        th.caption.gamma_multiply(0.5)
                    },
                ),
                None,
            );
            if resp.hovered() && valid {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked()
                && valid
                && let Some(ms) = ms
            {
                let v = match rate {
                    Some(rg) => gi::dial_for_hz(rg, 1000.0 / ms),
                    None => kd.clamp(ms),
                };
                self.set_knob(st, blk.uid, k, v);
                self.board_commit(st);
            }
            x = cr.max.x + px(6.0);
        }
        y += ch + px(18.0);
        // what it is now
        let now = self.board_now(st);
        let blk = now.index_of(blk.uid).map(|j| now.blocks[j]).unwrap_or(blk);
        let ms = cur_ms(blk.v[k]);
        let sel = bpm.and_then(|b| gi::note_of(b, ms));
        let value = match rate {
            Some(rg) => format!("{:.2} Hz", gi::hz_for_dial(rg, blk.v[k])),
            None => kd.text(blk.v[k]),
        };
        let big = match sel {
            Some(i) => format!("{} \u{b7} {value}", gi::NOTES[i].0),
            None => value,
        };
        text::draw(
            &p,
            &big,
            pos2(inner.min.x, y),
            Align2::LEFT_CENTER,
            Style::new(20.0, 800, th.accent_ink),
            Some(left_max - inner.min.x),
        );
        y += px(22.0);
        let line = match (bpm, sel) {
            (Some(b), Some(_)) => format!("at {b:.0} bpm ({src})"),
            (Some(b), None) => format!("not on a note value at {b:.0} bpm ({src}): pick one above"),
            (None, _) => "no beat grid here: tap the tempo".to_string(),
        };
        let lr = text::draw(
            &p,
            &line,
            pos2(inner.min.x, y),
            Align2::LEFT_CENTER,
            Style::new(11.5, 600, th.caption),
            Some(left_max - inner.min.x),
        );
        if src == "tapped" && song.is_some() {
            let label = "use the song's";
            let lw = text::width(&ctx, label, Style::new(11.5, 800, th.accent_ink));
            let rr = Rect::from_min_size(pos2(lr.max.x + px(8.0), y - px(8.0)), vec2(lw, px(16.0)));
            if rr.max.x < left_max {
                let resp = interact(ui, rr, "board-tap-reset", Sense::click());
                text::draw(
                    &p,
                    label,
                    rr.left_center(),
                    Align2::LEFT_CENTER,
                    Style::new(11.5, 800, th.accent_ink),
                    None,
                );
                if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    self.board.tap_bpm = None;
                    self.board.taps.clear();
                }
            }
        }
        y += px(14.0);
        // a delay's repeats on the beat grid
        if rate.is_none() && inner.max.y - y > px(30.0) {
            let g = Rect::from_min_max(pos2(inner.min.x, y), pos2(inner.max.x, inner.max.y));
            paint::inset(&p, g, Some(8.0), 1.5, Fill::Color(Color32::from_rgb(20, 21, 24)));
            let span = 2.0_f32;
            let beat = bpm.map(|b| 60.0 / b).unwrap_or(0.5);
            let mut tb = 0.0;
            let mut n = 0;
            while tb <= span && n < 40 {
                let bx = g.min.x + g.width() * tb / span;
                p.line_segment(
                    [pos2(bx, g.min.y + px(3.0)), pos2(bx, g.max.y - px(3.0))],
                    Stroke::new(1.0, Color32::from_white_alpha(if n % 4 == 0 { 40 } else { 16 })),
                );
                tb += beat;
                n += 1;
            }
            let fb = def
                .knob_index("repeats")
                .map(|i| 0.95 * blk.v[i] / 10.0)
                .unwrap_or(0.4);
            let dt = ms / 1000.0;
            let mut lvl = 0.85;
            let mut te = dt;
            while te <= span && lvl > 0.03 {
                let ex = g.min.x + g.width() * te / span;
                let top = g.max.y - px(3.0) - (g.height() - px(8.0)) * lvl;
                p.line_segment(
                    [pos2(ex, g.max.y - px(3.0)), pos2(ex, top)],
                    Stroke::new(px(4.0), th.accent.gamma_multiply(0.4 + 0.6 * lvl)),
                );
                lvl *= fb;
                te += dt;
            }
            p.line_segment(
                [
                    pos2(g.min.x + px(2.0), g.max.y - px(3.0)),
                    pos2(g.min.x + px(2.0), g.min.y + px(4.0)),
                ],
                Stroke::new(px(4.0), th.text_2),
            );
        }
    }

    /// A cab's mic: three cards to pick from.
    fn mic_card(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Block) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let def = blk.def();
        paint::display(&p, r, Some(18.0), 6.0);
        let Some(k) = def.knob_index("mic_type") else {
            return;
        };
        let inner = r.shrink2(vec2(px(14.0), px(12.0)));
        caps(
            &p,
            "MIC",
            pos2(inner.min.x, inner.min.y + px(6.0)),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        text::draw(
            &p,
            "click to swap",
            pos2(inner.max.x, inner.min.y + px(6.0)),
            Align2::RIGHT_CENTER,
            Style::new(11.0, 600, th.caption),
            None,
        );
        let cards = Rect::from_min_max(pos2(inner.min.x, inner.min.y + px(20.0)), inner.max);
        let gap = px(8.0);
        let cw = (cards.width() - gap * 2.0) / 3.0;
        let cur = blk.v[k].round() as usize;
        for (n, (name, what)) in MICS.iter().enumerate() {
            let cr = Rect::from_min_size(
                pos2(cards.min.x + (cw + gap) * n as f32, cards.min.y),
                vec2(cw, cards.height()),
            );
            let resp = interact(ui, cr, ("board-mic", n), Sense::click());
            let on = cur == n;
            p.rect_filled(
                cr,
                12.0,
                if on {
                    th.accent.gamma_multiply(0.12)
                } else if resp.hovered() {
                    th.screen_hi
                } else {
                    th.screen
                },
            );
            if on {
                p.rect_stroke(cr, 12.0, Stroke::new(1.6, th.accent), egui::StrokeKind::Inside);
            }
            let ih = (cr.height() * 0.38).min(px(64.0));
            draw_mic(
                &p,
                Rect::from_min_size(cr.min + vec2(0.0, px(6.0)), vec2(cw, ih)),
                n,
            );
            let mut y = cr.min.y + ih + px(18.0);
            text::draw(
                &p,
                name,
                pos2(cr.min.x + px(10.0), y),
                Align2::LEFT_CENTER,
                Style::new(13.0, 800, if on { th.accent_ink } else { th.text }),
                Some(cw - px(16.0)),
            );
            y += px(16.0);
            let ds = Style::new(10.5, 600, th.caption);
            for line in text::wrap(&ctx, what, ds, cw - px(18.0)) {
                if y + px(12.0) > cr.max.y {
                    break;
                }
                text::draw(
                    &p,
                    &line,
                    pos2(cr.min.x + px(10.0), y),
                    Align2::LEFT_CENTER,
                    ds,
                    None,
                );
                y += px(14.0);
            }
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() && !on {
                self.set_knob(st, blk.uid, k, n as f32);
                self.board_commit(st);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweet_spots_read_naturally() {
        let m = &MODELS[catalog::find("amp_crunch").unwrap()];
        assert_eq!(sweet_text(&m.knobs[0], (5.0, 7.0)), "5\u{2013}7");
        assert_eq!(sweet_text(&m.knobs[0], (4.5, 4.5)), "4.5");
        let cab = &MODELS[catalog::find("cab_4x12").unwrap()];
        assert_eq!(sweet_text(&cab.knobs[3], (60.0, 100.0)), "60\u{2013}100 Hz");
        assert_eq!(sweet_text(&cab.knobs[2], (0.0, 1.0)), "Dynamic or Ribbon");
    }

    #[test]
    fn edits_are_named_for_undo() {
        let a = Board::new(vec![Block::of("amp_crunch").unwrap()]);
        let mut b = a.clone();
        b.blocks[0].set("treble", 6.5);
        assert_eq!(change_label(&a, &b), "Treble 6.0 \u{2192} 6.5");
        let mut c = a.clone();
        c.blocks.push(Block::of("green").unwrap());
        assert_eq!(change_label(&a, &c), "Added Mean Green");
        assert_eq!(change_label(&c, &a), "Took off Mean Green");
    }
}
