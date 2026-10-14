//! The components screens are built from: push buttons (ivory keys with labels, icons and key caps), round keys
//! (ivory, gems, materials), segmented controls, switches, sliders, scrollbars, radio lists and toasts. Port of
//! the component half of rsplay/neu.py (`button`, `round_button`, `segmented`, `switch`, `slider`,
//! `scroll_groove`, `radio_rows`, `toast`). Layout is absolute (rects in points, as the Python app's in its scale-1
//! pixels), worked out inside in whole pixels exactly as neu.py does (`px()`, `//`, `int()`); each interactive
//! component takes the `Ui` for its hover and click, and returns egui's `Response`.

use egui::{Align2, Color32, CursorIcon, Id, Painter, Pos2, Rect, Response, Sense, Ui, Vec2};

use crate::anim;
use crate::icons::{self, Icon};
use crate::keys::{self, CapOn, Gem, keycap, keycap_width};
use crate::material::Mat;
use crate::paint::{self, Fill, PxRect, ipx, pt, pxr, sc};
use crate::raster::{mix, pg};
use crate::text::{self, Style};
use crate::theme::{IVORY_INK, t};

/// Hover and click on a rect. Never keyboard-focusable: Tab, Enter and Space are the app's own keys.
pub fn interact(ui: &Ui, rect: Rect, id: impl egui::AsId, sense: Sense) -> Response {
    let mut sense = sense;
    sense.remove(Sense::FOCUSABLE);
    ui.interact(rect, Id::new(id), sense)
}

/// A text's width in whole pixels (pygame's).
fn tw(ctx: &egui::Context, label: &str, st: Style) -> i32 {
    if label.is_empty() {
        return 0;
    }
    (text::width(ctx, label, st) * ctx.pixels_per_point()).round() as i32
}

/// Whole pixels (x, y) as a point for text placed by `int()` of it.
fn ptp(x: i32, y: i32, ppp: f32) -> Pos2 {
    Pos2::new(pt(x, ppp), pt(y, ppp))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Center,
    Left,
    /// Label at the left, key cap at the right edge (the controls' grid).
    Split,
}

/// How a push button looks.
#[derive(Clone, Copy, Debug)]
pub struct ButtonStyle {
    /// A toggle that's on: a small accent light before the label.
    pub active: bool,
    /// The main action: an accent key.
    pub primary: bool,
    /// An accent-coloured label.
    pub accent: bool,
    /// A red label.
    pub danger: bool,
    /// A read-only value, sunk in a small screen in the accent.
    pub value: bool,
    pub enabled: bool,
    pub icon: Option<Icon>,
    pub size: f32,
    pub align: Align,
    pub r: Option<f32>,
    pub color: Option<Color32>,
    /// A key cut from a material instead of ivory (seed: which part of it).
    pub mat: Option<Mat>,
    pub seed: u32,
    pub icon_size: f32,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        ButtonStyle {
            active: false,
            primary: false,
            accent: false,
            danger: false,
            value: false,
            enabled: true,
            icon: None,
            size: 13.0,
            align: Align::Center,
            r: None,
            color: None,
            mat: None,
            seed: 0,
            icon_size: 16.0,
        }
    }
}

impl ButtonStyle {
    pub fn primary() -> ButtonStyle {
        ButtonStyle {
            primary: true,
            ..Default::default()
        }
    }
    pub fn active(on: bool) -> ButtonStyle {
        ButtonStyle {
            active: on,
            ..Default::default()
        }
    }
    pub fn value() -> ButtonStyle {
        ButtonStyle {
            value: true,
            ..Default::default()
        }
    }
    pub fn icon(mut self, i: Icon) -> ButtonStyle {
        self.icon = Some(i);
        self
    }
    pub fn size(mut self, s: f32) -> ButtonStyle {
        self.size = s;
        self
    }
    pub fn split(mut self) -> ButtonStyle {
        self.align = Align::Split;
        self
    }
}

/// The width a button needs for its label, key cap and icon (`neu.button_width`: whole pixels).
pub fn button_width(ctx: &egui::Context, label: &str, key: &str, size: f32, icon: bool) -> f32 {
    let ppp = ctx.pixels_per_point();
    let s = sc(ppp);
    let mut w = tw(ctx, label, Style::new(size, 700, Color32::WHITE)) + pxr(28.0, s);
    if !key.is_empty() {
        w += (keycap_width(ctx, key, 11.0) * ppp).round() as i32 + pxr(8.0, s);
    }
    if icon {
        w += pxr(22.0, s);
    }
    w as f32 / ppp
}

/// A push button: a solid ivory key with its label, an optional icon before it and key cap after it.
pub fn button(ui: &Ui, rect: Rect, id: impl egui::AsId, label: &str, key: &str, st: ButtonStyle) -> Response {
    let resp = interact(
        ui,
        rect,
        id,
        if st.enabled && !st.value {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    draw_button(
        ui.painter(),
        rect,
        label,
        key,
        st,
        resp.hovered(),
        resp.is_pointer_button_down_on(),
    );
    if st.enabled && !st.value && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

/// Draw a button in a given hover / press state (no interaction).
pub fn draw_button(
    p: &Painter,
    rect: Rect,
    label: &str,
    key: &str,
    st: ButtonStyle,
    hovered: bool,
    pressed: bool,
) {
    let th = t();
    let ctx = p.ctx();
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let r = PxRect::of(rect, ppp);
    let rad: f64 =
        st.r.map(f64::from)
            .unwrap_or_else(|| (r.h as f64 / s * 0.3).min(16.0));
    let hot = st.enabled && !st.value && hovered;
    let down = hot && pressed;
    let one = pxr(1.0, s).max(1);
    let mut lift = 0;
    let mut on_key = CapOn::Material;
    let mut shadow = 0.0;
    let (col, weight);
    if st.value {
        paint::display_px(p, r, Some(rad), 3.0);
        col = st.color.unwrap_or(th.accent_ink);
        weight = 800;
    } else if !st.enabled {
        paint::inset_px(p, r, Some(rad), 2.0, Fill::Base);
        col = th.caption;
        weight = 700;
    } else {
        lift = if hot && !down { one } else { 0 };
        let face = r.moved(0, -lift + if down { one } else { 0 });
        if let Some(mat) = st.mat {
            let m = mat.get();
            keys::mat_key_px(p, face, mat, Some(rad), down, st.seed);
            on_key = CapOn::for_mat(mat);
            shadow = m.shadow;
            col = if st.accent || st.primary {
                m.play.unwrap_or(th.accent)
            } else {
                m.ink
            };
        } else {
            keys::key_face_px(p, face, Some(rad), st.primary, down);
            on_key = if st.primary { CapOn::Accent } else { CapOn::Ivory };
            col = if st.primary {
                th.on_accent
            } else {
                keys::ink_on_ivory(st.danger, st.accent)
            };
        }
        weight = if st.primary || st.active || st.accent {
            800
        } else {
            700
        };
        if down {
            lift = -one;
        }
    }
    let cy = r.y + r.h / 2 - lift;
    let lst = Style::new(st.size, weight, col);
    let label_w = tw(ctx, label, lst);
    let icon_w = if st.icon.is_some() {
        pxr(st.icon_size, s)
    } else {
        0
    };
    let gap_i = if st.icon.is_some() && !label.is_empty() {
        pxr(8.0, s)
    } else {
        0
    };
    let key_w = (keycap_width(ctx, key, 11.0) * ppp).round() as i32;
    let gap_k = if !key.is_empty() && (!label.is_empty() || st.icon.is_some()) {
        pxr(8.0, s)
    } else {
        0
    };
    let led_w = if st.active && st.enabled && !st.value {
        pxr(7.0, s) + pxr(8.0, s)
    } else {
        0
    };
    let right = r.x + r.w;
    if st.align == Align::Split {
        let mut x = r.x + pxr(11.0, s);
        if led_w > 0 {
            paint::led_px(p, x + pxr(3.0, s), cy, 7.0, th.accent);
            x += led_w;
        }
        if let Some(ic) = st.icon {
            icons::icon(p, ic, ptp(x + icon_w / 2, cy, ppp), st.icon_size, col);
            x += icon_w + gap_i;
        }
        let room = right - pxr(9.0, s) - if key.is_empty() { 0 } else { key_w + pxr(6.0, s) } - x;
        if !label.is_empty() {
            text::draw(
                p,
                label,
                ptp(x, cy, ppp),
                Align2::LEFT_CENTER,
                lst,
                Some(room.max(8) as f32 / ppp),
            );
        }
        if !key.is_empty() {
            keycap(
                p,
                key,
                ptp(right - pxr(9.0, s), cy, ppp),
                Align2::RIGHT_CENTER,
                11.0,
                on_key,
            );
        }
        return;
    }
    let total = led_w + icon_w + gap_i + label_w + gap_k + key_w;
    let mut x = if st.align == Align::Center {
        r.x + r.w / 2 - total.div_euclid(2)
    } else {
        r.x + pxr(14.0, s)
    };
    x = x.max(r.x + pxr(8.0, s));
    if led_w > 0 {
        paint::led_px(p, x + pxr(3.0, s), cy, 7.0, th.accent);
        x += led_w;
    }
    if let Some(ic) = st.icon {
        icons::ink_icon(p, ic, ptp(x + icon_w / 2, cy, ppp), st.icon_size, col, shadow);
        x += icon_w + gap_i;
    }
    if !label.is_empty() {
        let room = right - pxr(10.0, s) - x - (key_w + gap_k);
        let tr = keys::label(
            p,
            label,
            ptp(x, cy, ppp),
            Align2::LEFT_CENTER,
            lst,
            Some(room.max(8) as f32 / ppp),
            shadow,
        );
        x = (tr.max.x * ppp).round() as i32 + gap_k;
    }
    if !key.is_empty() {
        keycap(p, key, ptp(x, cy, ppp), Align2::LEFT_CENTER, 11.0, on_key);
    }
}

/// What a round key is made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoundKind {
    Ivory,
    Gem(Gem),
    /// A material, with the icon in `ink` (else the material's own).
    Mat(Mat, u32, Option<Color32>),
}

/// A round key holding an icon: ivory, a cut gem (the play key, a tile's close key) or a material.
pub fn round_button(
    ui: &Ui,
    rect: Rect,
    id: impl egui::AsId,
    icon: Icon,
    icon_size: f32,
    kind: RoundKind,
    active: bool,
) -> Response {
    let resp = interact(ui, rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    draw_round_button(
        ui.painter(),
        rect,
        icon,
        icon_size,
        kind,
        active,
        resp.hovered(),
        resp.is_pointer_button_down_on(),
    );
    resp
}

pub fn draw_round_button(
    p: &Painter,
    rect: Rect,
    icon: Icon,
    icon_size: f32,
    kind: RoundKind,
    active: bool,
    hot: bool,
    pressed: bool,
) {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let r = PxRect::of(rect, ppp);
    let one = pxr(1.0, s).max(1);
    let down = hot && pressed;
    let lift = if hot && !down {
        one
    } else if down {
        -one
    } else {
        0
    };
    let face = r.moved(0, -lift);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2 - lift);
    match kind {
        RoundKind::Mat(mat, seed, ink) => {
            let m = mat.get();
            keys::mat_key_px(p, face, mat, None, down, seed);
            icons::ink_icon(
                p,
                icon,
                ptp(cx, cy, ppp),
                icon_size,
                ink.unwrap_or(m.icon),
                m.shadow,
            );
        }
        RoundKind::Gem(g) => {
            keys::gem_key_px(p, face, g, down);
            let spec = g.spec();
            icons::icon(
                p,
                icon,
                ptp(cx, cy + one, ppp),
                icon_size,
                mix(spec.shade, Color32::BLACK, 0.4),
            );
            icons::icon(p, icon, ptp(cx, cy, ppp), icon_size, spec.ink);
        }
        RoundKind::Ivory => {
            keys::key_face_px(p, face, None, false, down || active);
            icons::icon(p, icon, ptp(cx, cy, ppp), icon_size, IVORY_INK);
        }
    }
}

/// Options for a segmented control.
#[derive(Clone, Debug)]
pub struct Segmented<'a> {
    pub labels: &'a [&'a str],
    pub sel: Option<usize>,
    pub size: f32,
    /// Each option's width (default: equal).
    pub widths: Option<Vec<f32>>,
    pub pad: f32,
    pub gap: f32,
    pub r: f32,
    pub inner_r: f32,
    pub keys: Option<&'a [&'a str]>,
    /// The chosen key is cut from this material instead of ivory.
    pub mat: Option<Mat>,
    /// Leave room under the labels for something drawn inside each option (e.g. a level meter).
    pub extra: bool,
}

impl<'a> Segmented<'a> {
    pub fn new(labels: &'a [&'a str], sel: Option<usize>) -> Segmented<'a> {
        Segmented {
            labels,
            sel,
            size: 15.0,
            widths: None,
            pad: 6.0,
            gap: 4.0,
            r: 18.0,
            inner_r: 13.0,
            keys: None,
            mat: None,
            extra: false,
        }
    }
}

/// The widths options need for their labels (`neu.segment_widths`: whole pixels).
pub fn segment_widths(ctx: &egui::Context, labels: &[&str], size: f32, pad: f32) -> Vec<f32> {
    let ppp = ctx.pixels_per_point();
    let s = sc(ppp);
    labels
        .iter()
        .map(|l| {
            (tw(ctx, l, Style::new(size, 800, Color32::WHITE)) + 2 * pxr(pad, s) + pxr(15.0, s)) as f32 / ppp
        })
        .collect()
}

/// A sunken track of options; the chosen one is an ivory key with a small accent light (it glides across when the
/// choice changes), the others are flat. Returns the options' rects and the one clicked.
pub fn segmented(
    ui: &Ui,
    rect: Rect,
    id: impl egui::AsId + Copy,
    o: &Segmented,
) -> (Vec<Rect>, Option<usize>) {
    let p = ui.painter();
    let ctx = ui.ctx();
    let th = t();
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let r = PxRect::of(rect, ppp);
    paint::inset_px(p, r, Some(o.r as f64), 6.0, Fill::Base);
    let (pd, g) = (pxr(o.pad, s), pxr(o.gap, s));
    let inner = PxRect {
        x: r.x + pd,
        y: r.y + pd,
        w: r.w - 2 * pd,
        h: r.h - 2 * pd,
    };
    let n = o.labels.len().max(1);
    let widths: Vec<f64> = match &o.widths {
        // (widths from segment_widths are whole pixels: kept whole through the f32 points)
        Some(w) => w
            .iter()
            .map(|v| {
                let x = *v as f64 * ppp as f64;
                if (x - x.round()).abs() < 1e-3 {
                    x.round()
                } else {
                    x
                }
            })
            .collect(),
        None => vec![(inner.w - g * (n as i32 - 1)) as f64 / n as f64; n],
    };
    let mut rects = Vec::with_capacity(n);
    let mut x = inner.x as f64;
    for wd in &widths {
        let x0 = x as i32;
        rects.push(PxRect {
            x: x0,
            y: inner.y,
            w: (x + wd) as i32 - x0,
            h: inner.h,
        });
        x += wd + g as f64;
    }
    if let Some(sel) = o.sel
        && sel < rects.len()
    {
        let tgt = rects[sel];
        let sx = anim::smooth(ctx, (Id::new(id), "x"), (tgt.x - r.x) as f32, 0.05, None);
        let sw = anim::smooth(ctx, (Id::new(id), "w"), tgt.w as f32, 0.05, None);
        let kr = PxRect {
            x: r.x + sx as i32,
            y: tgt.y,
            w: sw as i32,
            h: tgt.h,
        };
        match o.mat {
            Some(m) => keys::mat_key_px(p, kr, m, Some(o.inner_r as f64), false, 2),
            None => keys::key_face_px(p, kr, Some(o.inner_r as f64), false, false),
        }
    }
    let m = o.mat.map(|m| m.get());
    let mut clicked = None;
    let mut out = Vec::with_capacity(n);
    for (i, (label, rr)) in o.labels.iter().zip(&rects).enumerate() {
        let rr_pt = rr.to_rect(ppp);
        out.push(rr_pt);
        let resp = interact(ui, rr_pt, (Id::new(id), i), Sense::click());
        let chosen = o.sel == Some(i);
        let hot = resp.hovered() && !chosen;
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
        let col = if chosen {
            m.map(|m| m.ink).unwrap_or(IVORY_INK)
        } else if hot {
            th.text
        } else {
            th.text_3
        };
        let weight = if chosen { 800 } else { 600 };
        let st = Style::new(o.size, weight, col);
        // (with something drawn under the labels, they sit at 0.38 of the option's height: int() where used)
        let cy = if o.extra {
            (rr.y as f64 + rr.h as f64 * 0.38) as i32
        } else {
            rr.y + rr.h / 2
        };
        let k = o.keys.and_then(|k| k.get(i).copied()).unwrap_or("");
        let lw = tw(ctx, label, st);
        let led_w = if chosen && rr.w > lw + pxr(30.0, s) {
            pxr(7.0, s) + pxr(7.0, s)
        } else {
            0
        };
        let twd =
            lw + if k.is_empty() {
                0
            } else {
                pxr(8.0, s) + (keycap_width(ctx, k, 11.0) * ppp).round() as i32
            } + led_w;
        let mut x0 = rr.x + rr.w / 2 - twd.div_euclid(2);
        if led_w > 0 {
            paint::led_px(p, x0 + pxr(3.0, s), cy, 7.0, th.accent);
            x0 += led_w;
        }
        let shadow = if chosen {
            m.map(|m| m.shadow).unwrap_or(0.0)
        } else {
            0.0
        };
        if !k.is_empty() {
            let tr = keys::label(p, label, ptp(x0, cy, ppp), Align2::LEFT_CENTER, st, None, shadow);
            let on = if chosen {
                match o.mat {
                    Some(mt) => CapOn::for_mat(mt),
                    None => CapOn::Ivory,
                }
            } else {
                CapOn::Material
            };
            let kx = (tr.max.x * ppp).round() as i32 + pxr(8.0, s);
            keycap(p, k, ptp(kx, cy, ppp), Align2::LEFT_CENTER, 11.0, on);
        } else {
            keys::label(
                p,
                label,
                ptp(x0, cy, ppp),
                Align2::LEFT_CENTER,
                st,
                Some((rr.x + rr.w - pxr(4.0, s) - x0) as f32 / ppp),
                shadow,
            );
        }
    }
    (out, clicked)
}

/// (width, height) of the switch sizes.
pub fn switch_size(name: &str) -> Vec2 {
    match name {
        "lg" => Vec2::new(60.0, 32.0),
        "sm" => Vec2::new(48.0, 26.0),
        "xs" => Vec2::new(44.0, 24.0),
        _ => Vec2::new(56.0, 30.0),
    }
}

/// An on/off switch: a sunken track (a flat accent fill when on) and an ivory thumb that slides.
pub fn switch(ui: &Ui, rect: Rect, id: impl egui::AsId + Copy, on: bool) -> Response {
    let resp = interact(ui, rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    draw_switch(ui.painter(), rect, Id::new(id), on);
    resp
}

/// A pygame `draw.line` on the screen (whole pixels, `width` px), in `color`.
fn screen_line(p: &Painter, color: Color32, a: (i32, i32), b: (i32, i32), width: i32) {
    let c = pg::rgb(color);
    let (x0, y0) = (a.0.min(b.0) - width - 1, a.1.min(b.1) - width - 1);
    let bx = PxRect {
        x: x0,
        y: y0,
        w: (a.0 - b.0).abs() + 2 * width + 3,
        h: (a.1 - b.1).abs() + 2 * width + 3,
    };
    let key = paint::key_of((
        "line",
        a.0 - x0,
        a.1 - y0,
        b.0 - x0,
        b.1 - y0,
        width,
        paint::ck(color),
    ));
    paint::cover(p, key, bx, |cv| pg::line(cv, c, a, b, width));
}

pub fn draw_switch(p: &Painter, rect: Rect, id: Id, on: bool) {
    let th = t();
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let r = PxRect::of(rect, ppp);
    if on {
        paint::pill_px(p, r, th.accent);
        screen_line(
            p,
            th.accent_lo,
            (r.x + r.h / 2, r.y + 1),
            (r.x + r.w - r.h / 2, r.y + 1),
            pxr(1.0, s).max(1),
        );
    } else {
        paint::inset_px(p, r, None, 3.0, Fill::Base);
    }
    let m = pxr(3.0, s).max(2);
    let k = r.h - 2 * m;
    let tt = anim::smooth(p.ctx(), (id, "switch"), if on { 1.0 } else { 0.0 }, 0.05, None) as f64;
    let x = r.x as f64 + m as f64 + tt * (r.w - 2 * m - k) as f64;
    keys::thumb_px(p, (x + k as f64 / 2.0) as i32, r.y + r.h / 2, k);
}

/// A sunken groove with an accent fill from the left (or from `zero`, for values either side of a centre) and an
/// ivory thumb `knob` points across. Drawing only; returns the thumb's rect.
pub fn slider(
    p: &Painter,
    groove: Rect,
    u: f32,
    zero: Option<f32>,
    knob: f32,
    hot: bool,
    color: Option<Color32>,
) -> Rect {
    let th = t();
    let color = color.unwrap_or(th.accent);
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let gr = PxRect::of(groove, ppp);
    paint::inset_px(p, gr, None, 3.0, Fill::Base);
    let u = u.clamp(0.0, 1.0) as f64;
    let x = gr.x as f64 + gr.w as f64 * u;
    let m = if gr.h >= pxr(8.0, s) {
        pxr(2.0, s).max(1)
    } else {
        0
    };
    let inner = PxRect {
        x: gr.x + m,
        y: gr.y + m,
        w: gr.w - 2 * m,
        h: gr.h - 2 * m,
    };
    match zero {
        None => {
            if x - inner.x as f64 >= 1.0 {
                paint::pill_px(
                    p,
                    PxRect {
                        w: ((x - inner.x as f64) as i32).max(inner.h),
                        ..inner
                    },
                    color,
                );
            }
        }
        Some(z) => {
            let zx = gr.x as f64 + gr.w as f64 * z as f64;
            let (a, b) = (x.min(zx), x.max(zx));
            if b - a >= 1.0 {
                paint::rrect_px(
                    p,
                    PxRect {
                        x: a as i32,
                        y: inner.y,
                        w: ((b - a) as i32).max(1),
                        h: inner.h,
                    },
                    inner.h / 2,
                    color,
                );
            }
            let zi = zx as i32;
            screen_line(
                p,
                th.tick,
                (zi, gr.y - pxr(4.0, s)),
                (zi, gr.y + gr.h + pxr(4.0, s) - 1),
                pxr(2.0, s).max(1),
            );
        }
    }
    keys::thumb_px(
        p,
        x as i32,
        gr.y + gr.h / 2,
        pxr(knob + if hot { 2.0 } else { 0.0 }, s),
    )
    .to_rect(ppp)
}

/// A sunken scrollbar groove with a raised thumb (wider than the groove; or cut from a material) and an accent
/// mark where the playback cursor is.
pub fn scroll_groove(
    p: &Painter,
    track: Rect,
    thumb: Rect,
    cursor_y: Option<f32>,
    hot: bool,
    mat: Option<Mat>,
) {
    let th = t();
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let tr = PxRect::of(track, ppp);
    paint::inset_px(p, tr, None, 3.0, Fill::Base);
    let th_r = PxRect::of(thumb, ppp);
    let grow = pxr(8.0, s);
    let tb = PxRect {
        x: th_r.x - grow / 2,
        w: th_r.w + grow,
        ..th_r
    };
    match mat {
        Some(m) => keys::mat_key_px(p, tb, m, Some(8.0), false, 5),
        None => paint::raised_px(p, tb, Some(8.0), 4.0, Fill::Base, 0),
    }
    if hot {
        paint::ring_px(p, tb, Some(8.0), 1.0, th.tick, false);
    }
    if let Some(y) = cursor_y {
        let mark = PxRect {
            x: tr.x - pxr(6.0, s),
            y: ipx(y, ppp) - pxr(1.0, s),
            w: tr.w + pxr(12.0, s),
            h: pxr(3.0, s).max(2),
        };
        paint::glow_px(p, mark, th.accent, 4.0, None, None);
        paint::pill_px(p, mark, th.accent);
    }
}

/// A sunken list of choices (audio devices): the current one raised with a lit dot, the others with a dark socket;
/// `notes` (optional, one per row) are small captions at the right end. The well is drawn to fit the rows; returns
/// the rows' rects and the one clicked.
#[allow(clippy::too_many_arguments)]
pub fn radio_rows(
    ui: &Ui,
    rect: Rect,
    id: impl egui::AsId + Copy,
    labels: &[String],
    notes: &[String],
    current: Option<usize>,
    row_h: f32,
    size: f32,
) -> (Vec<Rect>, Option<usize>) {
    let p = ui.painter();
    let th = t();
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let r = PxRect::of(rect, ppp);
    let (pad, gap, rh) = (pxr(8.0, s), pxr(4.0, s), pxr(row_h, s));
    let n = labels.len() as i32;
    let h = 2 * pad + n * rh + (n - 1).max(0) * gap;
    paint::inset_px(p, PxRect { h, ..r }, Some(18.0), 6.0, Fill::Base);
    let mut rows = Vec::with_capacity(labels.len());
    let mut clicked = None;
    let mut y = r.y + pad;
    for (i, name) in labels.iter().enumerate() {
        let row = PxRect {
            x: r.x + pad,
            y,
            w: r.w - 2 * pad,
            h: rh,
        };
        let row_pt = row.to_rect(ppp);
        let on = current == Some(i);
        let resp = interact(ui, row_pt, (Id::new(id), i), Sense::click());
        if on {
            paint::raised_px(p, row, Some(12.0), 4.0, Fill::Base, 0);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
        let cx = row.x + pxr(18.0, s);
        let cy = row.y + row.h / 2;
        if on {
            paint::led_px(p, cx, cy, 9.0, th.accent);
        } else {
            paint::dot_px(p, (cx, cy), pxr(4.5, s), th.track);
        }
        let hot = resp.hovered() && !on;
        let col = if on || hot { th.text } else { th.text_2 };
        let note = notes.get(i).map(|n| n.as_str()).unwrap_or("");
        let note_w = if note.is_empty() {
            0
        } else {
            let nr = text::draw(
                p,
                note,
                ptp(row.x + row.w - pxr(14.0, s), cy, ppp),
                Align2::RIGHT_CENTER,
                Style::new(12.0, 600, th.caption),
                Some(row.w as f32 * 0.4 / ppp),
            );
            (nr.width() * ppp).round() as i32 + pxr(12.0, s)
        };
        text::draw(
            p,
            name,
            ptp(cx + pxr(16.0, s), cy, ppp),
            Align2::LEFT_CENTER,
            Style::new(size, if on { 800 } else { 600 }, col),
            Some((row.x + row.w - cx - pxr(28.0, s) - note_w) as f32 / ppp),
        );
        rows.push(row_pt);
        y += rh + gap;
    }
    (rows, clicked)
}

/// A raised pill with a short message (the part in `tail` in the accent): `neu.toast`, the text's box plus
/// `px(44)` by `px(20)`, centred on `int(center)`.
pub fn toast(p: &Painter, msg: &str, center: Pos2, tail: &str) {
    let th = t();
    let ctx = p.ctx();
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let st = Style::new(16.0, 700, th.text);
    let w = tw(ctx, msg, st) + tw(ctx, tail, st);
    let h = (text::line_height(ctx, st) * ppp).round() as i32;
    let (rw, rh) = (w + pxr(44.0, s), h + pxr(20.0, s));
    let (cx, cy) = (ipx(center.x, ppp), ipx(center.y, ppp));
    let r = PxRect {
        x: cx - rw / 2,
        y: cy - rh / 2,
        w: rw,
        h: rh,
    };
    paint::raised_px(p, r, Some(16.0), 4.0, Fill::Base, 0);
    let mid = r.y + r.h / 2;
    let tr = text::draw(
        p,
        msg,
        ptp(r.x + pxr(22.0, s), mid, ppp),
        Align2::LEFT_CENTER,
        st,
        None,
    );
    if !tail.is_empty() {
        text::draw(
            p,
            tail,
            ptp((tr.max.x * ppp).round() as i32, mid, ppp),
            Align2::LEFT_CENTER,
            Style {
                color: th.accent_ink,
                ..st
            },
            None,
        );
    }
}
