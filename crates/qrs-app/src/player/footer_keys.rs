//! Two small keys in the player's footer (new in quickRS, P37.8, owner request; D115, D116): a restart key at the
//! top right of the » stone (what Home does), and a Review key at the end of the lamps' row, beside "rec" (what E
//! does; its hover lists the part's last three runs to jump into). Neither is another white stone or garnet: the
//! restart key is a dark socket with the arrow engraved in it, lit amber on hover; the review key an amber glass lens
//! (smoky and unlit until there's a review), a tick engraved in it.

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Painter, Pos2, Rect, Response, Sense, Ui, pos2, vec2};
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::hud::Lit;

/// The restart key's socket across (points).
pub(super) const RESTART_D: f32 = 22.0;
/// The review lens across, and its socket's rim.
const LENS_D: f32 = 16.0;
const LENS_RIM: f32 = 2.0;
/// Runs the review key's menu lists.
pub(super) const RECENT: usize = 3;
/// The lamps' row (P37.11): the gap between its items, a name's gap to its lamp or key, half an item's hit height.
pub(super) const ROW_GAP: f32 = 18.0;
const NAME_GAP: f32 = 7.0;
pub(super) const ROW_HALF: f32 = 9.0;

/// An item of the lamps' row, left to right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Item {
    Nbn,
    Loop,
    Ear,
    Take,
    Count,
    Synth,
    Rec,
    Review,
    /// The recording's badge while R records (its width whole, and short: lamp, word and time, without the keys).
    Badge(f32, f32),
}

impl Item {
    /// The row as it always is (the badge joins it while recording).
    pub const ROW: [Item; 8] = [
        Item::Nbn,
        Item::Loop,
        Item::Ear,
        Item::Take,
        Item::Count,
        Item::Synth,
        Item::Rec,
        Item::Review,
    ];

    /// Its name (drawn left of its lamp or key).
    pub fn name(self) -> &'static str {
        match self {
            Item::Nbn => "note by note",
            Item::Loop => "loop",
            Item::Ear => "ear",
            Item::Take => "take",
            Item::Count => "count-in",
            Item::Synth => "synth / real",
            Item::Rec => "rec",
            Item::Review => "review",
            Item::Badge(..) => "",
        }
    }

    /// Its lamp's or key's width (points).
    fn key_w(self) -> f32 {
        match self {
            Item::Synth => 20.0,
            Item::Rec => 18.0,
            Item::Review => LENS_D + 2.0 * LENS_RIM,
            _ => 16.0,
        }
    }

    /// Its width on the row (pixels): its name (as wide as it gets, lit) and its key; compact, the key alone.
    pub fn width(self, ctx: &egui::Context, compact: bool) -> f32 {
        if let Item::Badge(w, short) = self {
            return if compact { short } else { w };
        }
        let key = px(self.key_w());
        if compact {
            return key;
        }
        let name = text::width(ctx, self.name(), Style::new(12.0, 800, t().caption));
        name + px(NAME_GAP) + key
    }
}

/// Lay out items `widths` wide `gap` apart, centred on `centre`, kept within [lo, hi]: each one's left. None when
/// they don't fit.
pub(super) fn row_xs(widths: &[f32], gap: f32, centre: f32, lo: f32, hi: f32) -> Option<Vec<f32>> {
    let total: f32 = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    if total > hi - lo {
        return None;
    }
    let mut x = (centre - total / 2.0).clamp(lo, hi - total);
    Some(
        widths
            .iter()
            .map(|w| {
                let at = x;
                x += w + gap;
                at
            })
            .collect(),
    )
}

/// The lamps' row: each item's slot (left, width), centred on `centre` (the tab's middle) within [lo, hi], with
/// their names if they fit (else compact: the lamps and keys alone). Too narrow even so, from `lo`.
pub(super) fn row_layout(
    ctx: &egui::Context,
    items: &[Item],
    centre: f32,
    lo: f32,
    hi: f32,
) -> (Vec<(f32, f32)>, bool) {
    // names and the whole badge; names and the short badge; keys alone (the badge with its keys ran past the tab's
    // right edge, and at scale 2.2 under the console: P37.4b)
    for (compact, short) in [(false, false), (false, true), (true, true)] {
        let widths: Vec<f32> = items
            .iter()
            .map(|i| match i {
                Item::Badge(..) => i.width(ctx, short),
                _ => i.width(ctx, compact),
            })
            .collect();
        let gap = px(if compact { ROW_GAP - 4.0 } else { ROW_GAP });
        if let Some(xs) = row_xs(&widths, gap, centre, lo, hi) {
            return (xs.into_iter().zip(widths).collect(), compact);
        }
    }
    let widths: Vec<f32> = items.iter().map(|i| i.width(ctx, true)).collect();
    let mut x = lo;
    let slots = widths
        .into_iter()
        .map(|w| {
            let at = x;
            x += w + px(ROW_GAP - 8.0);
            (at, w)
        })
        .collect();
    (slots, true)
}

/// Where the restart key sits: just off the cradle's top right corner, on the » stone's shoulder, below the lamps'
/// row and left of the seek bar's line (the owner's mark).
pub(super) fn restart_rect(cradle: Rect) -> Rect {
    let d = px(RESTART_D);
    Rect::from_min_size(pos2(cradle.max.x - px(1.0), cradle.min.y + px(0.5)), vec2(d, d))
}

/// The review key's menu for the key's hit rect: the card above it, and its rows (up to `RECENT` of `n`).
pub(super) fn menu_rects(key: Rect, n: usize) -> (Rect, Vec<Rect>) {
    let rows = n.min(RECENT);
    let (head, row, pad, w) = (px(30.0), px(30.0), px(6.0), px(250.0));
    let h = head + rows as f32 * row + pad;
    let card = Rect::from_min_size(pos2(key.min.x - px(8.0), key.min.y - px(8.0) - h), vec2(w, h));
    let list = (0..rows)
        .map(|k| {
            Rect::from_min_size(
                pos2(card.min.x + pad, card.min.y + head + k as f32 * row),
                vec2(w - 2.0 * pad, row - px(2.0)),
            )
        })
        .collect();
    (card, list)
}

/// The act a click on the review key's menu row `k` (or on the key, `None`) sends.
pub(super) fn review_act_for(row: Option<usize>) -> String {
    match row {
        Some(k) => format!("review:recent:{k}"),
        None => "review".into(),
    }
}

/// `review:recent:<k>` → k.
pub(super) fn parse_recent(name: &str) -> Option<usize> {
    name.strip_prefix("review:recent:")?.parse().ok()
}

/// The restart key at `rect`: a round socket sunk in the panel, the restart arrow engraved in it (a light lip under
/// dim ink); on hover the arrow lights amber and throws a little light; held, it sits a hair lower.
pub(super) fn restart_key(ui: &Ui, rect: Rect) -> Response {
    let resp = interact(ui, rect, "t-restart", Sense::click());
    let hot = resp.hovered();
    let down = hot && resp.is_pointer_button_down_on();
    let p = ui.painter();
    let th = t();
    let r = rect.width() / 2.0;
    paint::inset(p, rect, Some(r), if down { 2.5 } else { 1.5 }, Fill::Base);
    if th.dark {
        // obsidian: the socket's floor a shade darker than the panel
        p.circle_filled(rect.center(), r - px(2.0), Color32::from_black_alpha(70));
    }
    let c = rect.center() + vec2(0.0, if down { px(0.6) } else { 0.0 });
    if hot {
        paint::glow(
            p,
            Rect::from_center_size(c, vec2(px(10.0), px(10.0))),
            Lit::Amber.color(),
            8.0,
            Some(0.35),
            None,
        );
    }
    let (lip, ink) = if hot {
        (Color32::TRANSPARENT, Lit::Amber.color())
    } else if th.dark {
        (Color32::from_black_alpha(160), Color32::from_rgb(152, 158, 168))
    } else {
        (Color32::from_white_alpha(170), th.text_2)
    };
    if lip.a() > 0 {
        // engraved: dark themes a shadow above the cut, Daylight a light lip under it
        let off = if th.dark { -0.8 } else { 0.8 };
        icons::icon(p, Icon::Restart, c + vec2(0.0, px(off)), 13.0, lip);
    }
    icons::icon(p, Icon::Restart, c, 13.0, ink);
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

/// The review lens (`d` across, at `c`) in its socket: amber glass, smoky and unlit with nothing to review, warm
/// with a review waiting, bright on hover; a tick engraved in it.
pub(super) fn review_lens(p: &Painter, c: Pos2, has: bool, hot: bool, down: bool) {
    let th = t();
    let d = px(LENS_D);
    let sock = Rect::from_center_size(c, vec2(d + 2.0 * px(LENS_RIM), d + 2.0 * px(LENS_RIM)));
    let lit: f32 = match (has, hot) {
        (true, true) => 1.0,
        (true, false) => 0.45,
        (false, true) => 0.12,
        (false, false) => 0.0,
    };
    if lit > 0.3 {
        paint::glow(
            p,
            sock,
            Color32::from_rgb(255, 170, 60),
            8.0,
            Some(0.5 * lit),
            Some(sock.width() / 2.0),
        );
    }
    paint::inset(p, sock, Some(sock.width() / 2.0), 1.5, Fill::Base);
    let c = c + vec2(0.0, if down { px(0.5) } else { 0.0 });
    let r = d / 2.0;
    // the glass: from its dark rim in to a core lit from behind, in a few rings
    let mix = |a: [f32; 3], b: [f32; 3], k: f32| {
        Color32::from_rgb(
            (a[0] + (b[0] - a[0]) * k) as u8,
            (a[1] + (b[1] - a[1]) * k) as u8,
            (a[2] + (b[2] - a[2]) * k) as u8,
        )
    };
    let (rim_off, core_off) = ([34.0, 27.0, 22.0], [70.0, 54.0, 38.0]);
    let (rim_on, core_on) = ([96.0, 52.0, 8.0], [255.0, 184.0, 70.0]);
    let steps = 6;
    for i in 0..steps {
        let k = i as f32 / (steps - 1) as f32; // 0 rim → 1 core
        let rim = mix(rim_off, rim_on, lit);
        let core = mix(core_off, core_on, lit);
        let col = mix(
            [rim.r() as f32, rim.g() as f32, rim.b() as f32],
            [core.r() as f32, core.g() as f32, core.b() as f32],
            k * k,
        );
        p.circle_filled(c + vec2(0.0, r * 0.12 * k), r * (1.0 - 0.72 * k), col);
    }
    // the tick, cut into the glass
    let ink = if lit > 0.3 {
        Color32::from_rgba_unmultiplied(58, 30, 4, 230)
    } else {
        Color32::from_rgba_unmultiplied(150, 140, 128, 150)
    };
    icons::icon(p, Icon::Check, c + vec2(0.0, px(0.3)), 10.0, ink);
    // a glint up on the left, and a thin dark edge so it reads as a dome
    p.circle_filled(
        c + vec2(-r * 0.36, -r * 0.4),
        (r * 0.2).max(0.8),
        Color32::from_white_alpha(if lit > 0.3 { 170 } else { 90 }),
    );
    p.circle_stroke(
        c,
        r,
        egui::Stroke::new(px(0.8).max(0.5), Color32::from_black_alpha(110)),
    );
    if hot && !th.dark {
        p.circle_stroke(
            c,
            sock.width() / 2.0 + 1.5,
            egui::Stroke::new(1.0, th.accent.gamma_multiply(0.7)),
        );
    }
}

impl super::PlayerView {
    /// The runs saved for the part being played, read once per part (the review key's menu lists them).
    pub(super) fn ensure_runs(&mut self) {
        let key = self.history_key();
        if self.runs_for != key {
            self.runs = super::history::load(&self.reviews_path, &key);
            self.runs_for = key;
        }
    }

    /// The saved run `k` (newest first) as the review, its page up (the review key's menu).
    pub(super) fn open_recent(&mut self, k: usize) {
        if !self.review_may_open() {
            return;
        }
        if self.review.as_ref().is_some_and(|r| r.part_key != self.part_key) {
            self.review = None;
        }
        self.ensure_runs();
        if k >= self.runs.len() {
            self.show_review();
            return;
        }
        self.pick_run(k);
        if self.review_open() {
            self.review_opening();
            self.hold();
        }
    }

    /// The restart key over the cradle's top right; a click goes back to the start (as Home).
    pub(super) fn draw_restart_key(&mut self, ui: &Ui, cradle: Rect) {
        let rect = restart_rect(cradle);
        self.restart_rect = rect;
        let resp = restart_key(ui, rect);
        if resp.hovered() {
            let what = if self.range().is_some() {
                "Restart the loop  \u{b7}  again: the song"
            } else {
                "Restart the song"
            };
            self.tip = Some((
                what.into(),
                "Home".into(),
                pos2(rect.min.x, rect.min.y - px(4.0)),
                Align2::LEFT_BOTTOM,
            ));
        }
        if resp.clicked() {
            self.acts.push("start".into());
        }
    }

    /// The review key in its slot on the lamps' row (`x`, `w` wide, at `y`): "review" and its lens. Its menu is
    /// drawn later, over everything (`draw_review_menu`).
    pub(super) fn draw_review_key(&mut self, ui: &Ui, x: f32, w: f32, y: f32, compact: bool) {
        self.ensure_runs();
        let th = t();
        let p = ui.painter().clone();
        let has = !self.runs.is_empty() || self.review.is_some();
        let name = Style::new(12.0, 500, th.caption);
        let d = px(LENS_D + 2.0 * LENS_RIM);
        let key = Rect::from_center_size(pos2(x + w - d / 2.0, y), vec2(d, d));
        let hit = Rect::from_min_max(
            pos2(x - px(2.0), y - px(ROW_HALF)),
            pos2(x + w + px(2.0), y + px(ROW_HALF)),
        );
        self.review_rect = hit;
        let resp = interact(ui, hit, "review-key", Sense::click());
        let hot = resp.hovered();
        let st = if hot && has {
            Style {
                color: th.text,
                ..name
            }
        } else if has {
            name
        } else {
            Style {
                color: th.caption.gamma_multiply(0.6),
                ..name
            }
        };
        if !compact {
            text::draw(
                &p,
                "review",
                pos2(key.min.x - px(NAME_GAP), y),
                Align2::RIGHT_CENTER,
                st,
                None,
            );
        }
        review_lens(
            &p,
            key.center(),
            has,
            hot,
            hot && resp.is_pointer_button_down_on(),
        );
        if hot {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            if !has {
                self.tip = Some((
                    "Finish a run to get a review".into(),
                    "E".into(),
                    pos2(x, hit.min.y - px(4.0)),
                    Align2::LEFT_BOTTOM,
                ));
            } else if self.runs.is_empty() {
                self.tip = Some((
                    "Review this run: the spots to practise".into(),
                    "E".into(),
                    pos2(x, hit.min.y - px(4.0)),
                    Align2::LEFT_BOTTOM,
                ));
            }
        }
        if resp.clicked() {
            self.acts.push(review_act_for(None));
        }
    }

    /// The review key's menu, while the pointer is on the key or the menu: the part's last runs (newest first), a
    /// click opens that one's review.
    pub(super) fn draw_review_menu(&mut self, ui: &Ui) {
        let key = self.review_rect;
        if key == Rect::NOTHING || self.runs.is_empty() || self.review_open() {
            self.review_menu = false;
            return;
        }
        let (card, rows) = menu_rects(key, self.runs.len());
        let at = ui.ctx().input(|i| i.pointer.hover_pos());
        let bridge = Rect::from_min_max(
            pos2(key.min.x, card.max.y - px(2.0)),
            pos2(key.max.x, key.min.y + px(2.0)),
        );
        let on_key = at.is_some_and(|a| key.contains(a));
        let on_menu = at.is_some_and(|a| card.expand(px(6.0)).contains(a) || bridge.contains(a));
        self.review_menu = on_key || (self.review_menu && on_menu);
        if !self.review_menu {
            return;
        }
        let th = t();
        let p = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("review-key-menu"),
        ));
        // (a catch over the card, so what's under it doesn't take the hover)
        let _ = interact(ui, card, "review-menu-card", Sense::hover());
        paint::floating_opaque(&p, card, Some(14.0));
        let lx = card.min.x + px(14.0);
        let hy = card.min.y + px(16.0);
        text::caps(
            &p,
            "LAST RUNS",
            pos2(lx, hy),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
            None,
        );
        let cap = keycap(
            &p,
            "E",
            pos2(card.max.x - px(14.0), hy),
            Align2::RIGHT_CENTER,
            11.0,
            CapOn::Material,
        );
        text::draw(
            &p,
            "latest",
            pos2(cap.min.x - px(6.0), hy),
            Align2::RIGHT_CENTER,
            Style::new(11.0, 500, th.caption),
            None,
        );
        for (k, row) in rows.iter().enumerate() {
            let run = &self.runs[k];
            let resp = interact(ui, *row, ("review-recent", k), Sense::click());
            if resp.hovered() {
                paint::raised(&p, *row, Some(10.0), 2.0, Fill::Base, 0.0);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let when = format!(
                "{}{}",
                if k == 0 { "Latest  \u{b7}  " } else { "" },
                crate::home::fmt_when(Some(run.at), false)
            );
            let pc = format!("{}%", (run.acc * 100.0).round() as i64);
            let pr = text::draw(
                &p,
                &pc,
                pos2(row.max.x - px(10.0), row.center().y),
                Align2::RIGHT_CENTER,
                Style::new(13.0, 800, acc_color(run.acc)),
                None,
            );
            text::draw(
                &p,
                &when,
                pos2(row.min.x + px(10.0), row.center().y),
                Align2::LEFT_CENTER,
                Style::new(13.0, 700, th.text),
                Some(pr.min.x - px(10.0) - row.min.x - px(10.0)),
            );
            if resp.clicked() {
                self.review_menu = false;
                self.acts.push(review_act_for(Some(k)));
            }
        }
    }
}

/// A run's accuracy in the review's colours.
fn acc_color(a: f64) -> Color32 {
    let th = t();
    if a >= 0.9 {
        th.hit
    } else if a >= 0.6 {
        th.late
    } else {
        th.miss
    }
}

/// The practice bar's continuous key (P38.1, owner request, D151): the one practice mode the bar offers, so it is
/// made of something the other keys aren't, the review lens's amber glass, as a small square plate in its socket.
/// Not on yet: warm glass that breathes, slowly, a little brighter and back (an invitation, not a blink); hovered:
/// brighter; on: lit right through, steady, with a light thrown round it. Daylight: paler glass, darker ink.
/// `t` is the clock (seconds) the breath follows. Returns the response.
pub(super) fn continuous_key(ui: &Ui, rect: Rect, id: impl egui::AsId, on: bool, t_now: f64) -> Response {
    let resp = interact(ui, rect, id, Sense::click());
    let hot = resp.hovered();
    let down = hot && resp.is_pointer_button_down_on();
    let p = ui.painter();
    let th = t();
    // (the breath: 0..1 over 2.8 s, eased; in steps, so the glow's cached textures stay few)
    let breath = (((t_now * std::f64::consts::TAU / 2.8).sin() * 0.5 + 0.5) as f32 * 8.0).round() / 8.0;
    let lit: f32 = match (on, hot) {
        (true, _) => 1.0,
        (false, true) => 0.85,
        (false, false) => 0.42 + 0.22 * breath,
    };
    let rad = px(11.0);
    let glow_a = if on { 0.6 } else { 0.18 + 0.22 * lit };
    paint::glow(
        p,
        rect.shrink(px(2.0)),
        Color32::from_rgb(255, 170, 60),
        if on { 12.0 } else { 9.0 },
        Some((glow_a * 10.0).round() / 10.0),
        Some(rad),
    );
    paint::inset(p, rect, Some(rad), if down { 2.5 } else { 1.5 }, Fill::Base);
    let glass = rect
        .shrink(px(3.0))
        .translate(vec2(0.0, if down { px(0.5) } else { 0.0 }));
    let mix = |a: [f32; 3], b: [f32; 3], k: f32| {
        Color32::from_rgb(
            (a[0] + (b[0] - a[0]) * k) as u8,
            (a[1] + (b[1] - a[1]) * k) as u8,
            (a[2] + (b[2] - a[2]) * k) as u8,
        )
    };
    // rim and core, unlit and lit (Daylight: a paler glass, never muddy on the light panel)
    let (rim_off, core_off, rim_on, core_on) = if th.dark {
        (
            [70.0, 40.0, 10.0],
            [150.0, 98.0, 36.0],
            [120.0, 62.0, 8.0],
            [255.0, 190.0, 80.0],
        )
    } else {
        (
            [196.0, 128.0, 44.0],
            [244.0, 196.0, 120.0],
            [206.0, 120.0, 20.0],
            [255.0, 214.0, 120.0],
        )
    };
    let rim = mix(rim_off, rim_on, lit);
    let core = mix(core_off, core_on, lit);
    let steps = 7;
    for i in 0..steps {
        let k = i as f32 / (steps - 1) as f32;
        let col = mix(
            [rim.r() as f32, rim.g() as f32, rim.b() as f32],
            [core.r() as f32, core.g() as f32, core.b() as f32],
            k * k,
        );
        let inset = glass.width().min(glass.height()) * 0.36 * k;
        let r = glass
            .shrink(inset)
            .translate(vec2(0.0, glass.height() * 0.06 * k));
        p.rect_filled(r, (rad - px(3.0) - inset * 0.5).max(px(2.0)), col);
    }
    // a glint along the top, a thin dark edge so it reads as glass in its socket
    let glint = Rect::from_min_size(
        glass.min + vec2(glass.width() * 0.2, px(2.0)),
        vec2(glass.width() * 0.6, px(2.5)),
    );
    p.rect_filled(
        glint,
        px(1.25),
        Color32::from_white_alpha(if lit > 0.6 { 120 } else { 70 }),
    );
    p.rect_stroke(
        glass,
        rad - px(3.0),
        egui::Stroke::new(
            px(0.8).max(0.5),
            Color32::from_black_alpha(if th.dark { 120 } else { 70 }),
        ),
        egui::StrokeKind::Inside,
    );
    // the arrow, cut into the glass
    let ink = if th.dark {
        Color32::from_rgba_unmultiplied(58, 30, 4, 235)
    } else {
        Color32::from_rgba_unmultiplied(92, 50, 6, 235)
    };
    icons::icon(p, Icon::Restart, glass.center() + vec2(0.0, px(0.3)), 13.0, ink);
    if on {
        // lit: a warm ring round the socket and a small bright bead at its foot say it is running
        p.rect_stroke(
            rect.expand(px(1.0)),
            rad + px(1.0),
            egui::Stroke::new(px(1.5), Color32::from_rgb(255, 184, 70)),
            egui::StrokeKind::Outside,
        );
        p.circle_filled(
            pos2(glass.center().x, glass.max.y - px(4.0)),
            px(1.6),
            Color32::from_rgb(255, 244, 214),
        );
    } else {
        // (the breath needs frames: a slow repaint while it shows)
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(90));
    }
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        if !th.dark {
            p.rect_stroke(
                rect.expand(1.5),
                rad + 1.5,
                egui::Stroke::new(1.0, th.accent.gamma_multiply(0.7)),
                egui::StrokeKind::Outside,
            );
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Event;

    /// The footer's cradle at 1440 x 900 (scale 1) and what's round it, as draw_footer lays them out.
    fn cradle() -> Rect {
        let (pad, gap, small, big) = (6.0, 12.0, 48.0, 64.0);
        let cy = 833.0;
        Rect::from_min_size(
            pos2(28.0, cy - big / 2.0 - pad),
            vec2(2.0 * pad + 2.0 * small + big + 2.0 * gap, big + 2.0 * pad),
        )
    }

    fn circle_hits_rect(c: Pos2, r: f32, rect: Rect) -> bool {
        let q = pos2(
            c.x.clamp(rect.min.x, rect.max.x),
            c.y.clamp(rect.min.y, rect.max.y),
        );
        q.distance(c) < r
    }

    #[test]
    fn the_restart_key_clears_the_stone_the_lamps_and_the_seek_bar() {
        let cr = cradle();
        let k = restart_rect(cr);
        let cy = cr.center().y;
        // the » stone: 48 across, its centre 6 + 24 in from the cradle's right
        let fwd_c = pos2(cr.max.x - 6.0 - 24.0, cy);
        assert!(!circle_hits_rect(fwd_c, 24.0 + 3.0, k), "{k:?}");
        // nor the play gem (64 across)
        assert!(!circle_hits_rect(pos2(cr.center().x, cy), 32.0 + 3.0, k));
        // note by note's lamp and name, right of centre over the cradle's right end
        let nbn = Rect::from_center_size(pos2(cr.max.x - 17.0, cr.min.y - 9.0), vec2(26.0, 18.0));
        assert!(!nbn.intersects(k), "{nbn:?} {k:?}");
        // the first mode lamp's hit (from cradle.max.x + 16, 9 either side of the row)
        let row_y = cr.min.y - 9.0;
        let loop_hit = Rect::from_min_max(
            pos2(cr.max.x + 14.0, row_y - 9.0),
            pos2(cr.max.x + 80.0, row_y + 9.0),
        );
        assert!(!loop_hit.intersects(k));
        // the seek bar's grab (the groove inflated 16 x 20) and its line of text (from the groove's left) for any
        // 13 pt line height a font gives
        for lab_h in [15.0f32, 16.0, 18.0, 20.0, 22.0] {
            let top = cy - (lab_h + 24.0) / 2.0;
            let bar = Rect::from_min_size(pos2(cr.max.x + 24.0, top + lab_h + 10.0), vec2(600.0, 14.0));
            assert!(!bar.expand2(vec2(16.0, 20.0)).intersects(k), "lab_h {lab_h}");
            let line = Rect::from_min_size(pos2(bar.min.x, top), vec2(300.0, lab_h));
            assert!(!line.intersects(k), "lab_h {lab_h}");
        }
    }

    /// The lamps' row for a window `w` wide (900 tall, scale 1) as draw_footer lays it out: its items' hit rects,
    /// whether compact, and the tab well and the footer.
    fn lamps_row(w: f32, recording: bool) -> (Vec<Rect>, bool, Rect, Rect) {
        let ctx = egui::Context::default();
        qrs_ui::text::install(&ctx);
        let mut out = None;
        for _ in 0..2 {
            let mut o = ctx.run_ui(egui::RawInput::default(), |ui| {
                // PlayerView::layout: 28 margins, the console 26% (330-380) and 26 from the main column
                let aside_w = 330f32.max(380f32.min(w * 0.26)).trunc();
                let main =
                    Rect::from_min_size(pos2(28.0, 28.0), vec2(w - 28.0 - aside_w - 26.0 - 28.0, 844.0));
                let foot = Rect::from_min_size(pos2(main.min.x, main.max.y - 76.0), vec2(main.width(), 76.0));
                let well = Rect::from_min_max(
                    pos2(main.min.x, 120.0),
                    pos2(main.max.x - 24.0, foot.min.y - 18.0),
                );
                let mut items = Item::ROW.to_vec();
                if recording {
                    items.push(Item::Badge(240.0, 110.0));
                }
                let (slots, compact) = row_layout(ui.ctx(), &items, well.center().x, foot.min.x, well.max.x);
                let y = cradle().min.y - 9.0;
                let hits = slots
                    .iter()
                    .map(|&(x, w)| {
                        Rect::from_min_max(pos2(x - 2.0, y - ROW_HALF), pos2(x + w + 2.0, y + ROW_HALF))
                    })
                    .collect();
                out = Some((hits, compact, well, foot));
            });
            o.textures_delta.clear();
        }
        out.expect("laid out")
    }

    #[test]
    fn the_lamps_row_is_centred_under_the_tab_and_nothing_overlaps() {
        for (w, recording) in [
            (1440.0, false),
            (1920.0, false),
            (1440.0, true),
            (1180.0, false),
            (980.0, false),
            (980.0, true),
        ] {
            let (hits, compact, well, foot) = lamps_row(w, recording);
            let restart = restart_rect(cradle());
            for (k, h) in hits.iter().enumerate() {
                assert!(
                    !h.intersects(restart),
                    "{w}: item {k} {h:?} on the restart key {restart:?}"
                );
                assert!(
                    h.min.x >= foot.min.x - 2.0 && h.max.x <= well.max.x + 2.0,
                    "{w}: item {k} {h:?}"
                );
                // above the transport and the seek bar's line (the cradle's top), in the gap under the tab
                assert!(
                    h.max.y <= cradle().min.y && h.min.y >= well.max.y - 1.0,
                    "{w}: {h:?}"
                );
            }
            for p in hits.windows(2) {
                assert!(!p[0].intersects(p[1]), "{w}: {:?} {:?}", p[0], p[1]);
            }
            let (l, r) = (hits[0].min.x + 2.0, hits.last().unwrap().max.x - 2.0);
            if !recording && w >= 1180.0 {
                assert!(!compact, "{w}: the names fit");
                assert!(
                    ((l + r) / 2.0 - well.center().x).abs() < 1.0,
                    "{w}: centred under the tab"
                );
            }
        }
        // the narrowest window still has every lamp, without names
        let (hits, compact, ..) = lamps_row(980.0, false);
        assert_eq!(hits.len(), Item::ROW.len());
        let _ = compact;
    }

    #[test]
    fn row_xs_centres_and_clamps() {
        assert_eq!(
            row_xs(&[10.0, 10.0], 10.0, 50.0, 0.0, 100.0),
            Some(vec![35.0, 55.0])
        );
        assert_eq!(
            row_xs(&[10.0, 10.0], 10.0, 5.0, 0.0, 100.0),
            Some(vec![0.0, 20.0])
        );
        assert_eq!(
            row_xs(&[10.0, 10.0], 10.0, 99.0, 0.0, 100.0),
            Some(vec![70.0, 90.0])
        );
        assert_eq!(row_xs(&[60.0, 60.0], 10.0, 50.0, 0.0, 100.0), None);
    }

    #[test]
    fn the_menu_sits_over_the_key_with_three_rows_at_most() {
        let key = Rect::from_min_size(pos2(700.0, 776.0), vec2(70.0, 20.0));
        let (card, rows) = menu_rects(key, 4);
        assert_eq!(rows.len(), 3);
        assert!(card.max.y < key.min.y);
        for w in rows.windows(2) {
            assert!(!w[0].intersects(w[1]));
        }
        assert!(rows.iter().all(|r| card.contains_rect(*r)));
        assert_eq!(menu_rects(key, 1).1.len(), 1);
    }

    #[test]
    fn clicks_send_restart_and_review_acts() {
        assert_eq!(review_act_for(None), "review");
        assert_eq!(review_act_for(Some(2)), "review:recent:2");
        assert_eq!(parse_recent(&review_act_for(Some(1))), Some(1));
        assert_eq!(parse_recent("review:run:1"), None);
        // the restart key takes a click inside it, not one beside it
        let ctx = egui::Context::default();
        qrs_ui::text::install(&ctx);
        let rect = restart_rect(cradle());
        let mut clicked = Vec::new();
        let mut frame = 0;
        for at in [rect.center(), rect.center() + vec2(rect.width(), 0.0)] {
            let mut got = false;
            for evs in [
                vec![Event::PointerMoved(at)],
                vec![Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                }],
                vec![Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            ] {
                frame += 1;
                let input = egui::RawInput {
                    events: evs,
                    time: Some(frame as f64 * 0.05),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    got |= restart_key(ui, rect).clicked();
                });
                out.textures_delta.clear();
            }
            clicked.push(got);
        }
        assert_eq!(clicked, [true, false]);
    }
}
