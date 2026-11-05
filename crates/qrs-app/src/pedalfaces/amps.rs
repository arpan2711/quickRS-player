//! The amps' faces (PLAN.md P21.2): each drawn as the real unit it models, from the owner-approved mockups. The
//! Gemini Reverb is a blackface combo (black panel, skirted numbered knobs, silver-turquoise grille), the Tweedle Dee a
//! tweed combo (chrome top plate, brown pointers), the Dartford 30 a copper-topped combo (cream chicken-heads, brown
//! diamond grille), the Perspex 100 and Bletchley 800 Marshall-style heads (gold panel along the top, gold-capped
//! knobs), the Petaluma Plate a chrome diamond-plate head, the Blue Line 300 a silver-panelled bass head. P41.2: the
//! Lagunitas C+ a black head with a cane grille and its five graphic-EQ sliders on the panel, the Snakeskin 100 a
//! chrome-panelled head in snakeskin, the Tangerine 50 an orange head with a cream panel and a basket-weave grille,
//! the Surrey 103 a black head with a grey panel and the name on a plate on the grille.
//!
//! `Detail::Board` is the whole front at the unit's own proportions; `Detail::Full` is the same front with the panel
//! enlarged to half the height so the controls are big enough to use. Everything static (covering, cloth, panel,
//! jacks, plates) is painted into the cached body; knobs, lamps, switches and print are drawn live.
//!
//! This file also holds the drawing kit the cabs share (`Cv`, the textures, the small parts).

use std::cell::RefCell;

use eframe::egui::{self, Align2, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use qrs_audio::board::{Block, KnobDef, Unit};
use qrs_ui::raster::{self, Image};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use tiny_skia::{FillRule, Pixmap, Transform};

use super::{Detail, Hot, Layout, Lettering};

// -- colours ----------------------------------------------------------------------------------------------------------

pub(super) fn hx(h: u32) -> [u8; 3] {
    [(h >> 16) as u8, (h >> 8) as u8, h as u8]
}

pub(super) fn c32(h: u32) -> Color32 {
    let c = hx(h);
    Color32::from_rgb(c[0], c[1], c[2])
}

pub(super) fn ca(h: u32, a: f32) -> Color32 {
    let c = hx(h);
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub(super) fn rgbf(h: u32) -> [f32; 3] {
    let c = hx(h);
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// `c` laid over `d` at `a`.
pub(super) fn ov(d: [f32; 3], c: u32, a: f32) -> [f32; 3] {
    let s = rgbf(c);
    [
        d[0] + (s[0] - d[0]) * a,
        d[1] + (s[1] - d[1]) * a,
        d[2] + (s[2] - d[2]) * a,
    ]
}

fn to_u(c: [f32; 3]) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u32;
    (b(c[0]) << 16) | (b(c[1]) << 8) | b(c[2])
}

fn mixu(a: u32, b: u32, k: f32) -> u32 {
    to_u(ov(rgbf(a), b, k))
}

/// The colour at `t` along gradient stops.
fn sample(stops: &[(f32, u32)], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let mut prev = stops[0];
    for &s in stops {
        if t <= s.0 {
            let span = (s.0 - prev.0).max(1e-4);
            return ov(rgbf(prev.1), s.1, ((t - prev.0) / span).clamp(0.0, 1.0));
        }
        prev = s;
    }
    rgbf(prev.1)
}

fn col(c: [f32; 3]) -> Color32 {
    c32(to_u(c))
}

/// Smooth value noise in 0..1 (tolex grain).
pub(super) fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let h = |i: f32, j: f32| {
        super::hash01(
            (i as i32 as u32).wrapping_mul(73_856_093) ^ (j as i32 as u32).wrapping_mul(19_349_663) ^ seed,
        )
    };
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let a = h(xi, yi) + (h(xi + 1.0, yi) - h(xi, yi)) * sx;
    let b = h(xi, yi + 1.0) + (h(xi + 1.0, yi + 1.0) - h(xi, yi + 1.0)) * sx;
    a + (b - a) * sy
}

// -- the body canvas --------------------------------------------------------------------------------------------------

/// A tiny-skia canvas over the face's rect, addressed in points (absolute, as the layout's).
pub(super) struct Cv {
    pub pm: Pixmap,
    pub s: f32,
    pub o: Pos2,
}

impl Cv {
    pub(super) fn new(r: Rect, ppp: f32) -> Option<Cv> {
        let w = (r.width() * ppp).round().max(2.0) as u32;
        let h = (r.height() * ppp).round().max(2.0) as u32;
        Some(Cv {
            pm: Pixmap::new(w, h)?,
            s: ppp,
            o: r.min,
        })
    }

    pub(super) fn image(&self) -> Image {
        raster::from_pixmap(&self.pm)
    }

    fn pt(&self, p: Pos2) -> (f32, f32) {
        ((p.x - self.o.x) * self.s, (p.y - self.o.y) * self.s)
    }

    fn rpath(&self, q: Rect, rad: f32) -> Option<tiny_skia::Path> {
        let (x, y) = self.pt(q.min);
        super::rrect(x, y, q.width() * self.s, q.height() * self.s, rad * self.s)
    }

    pub(super) fn rr(&mut self, q: Rect, rad: f32, c: u32, a: f32) {
        if let Some(p) = self.rpath(q, rad) {
            self.pm.fill_path(
                &p,
                &super::solid(super::sk(hx(c), a)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// A rounded rect in a gradient from `a` to `b` (points), stops (offset, colour, alpha).
    pub(super) fn rr_grad(&mut self, q: Rect, rad: f32, a: Pos2, b: Pos2, stops: &[(f32, u32, f32)]) {
        let (pa, pb) = (self.pt(a), self.pt(b));
        if let (Some(p), Some(paint)) = (
            self.rpath(q, rad),
            super::linear(
                pa,
                pb,
                stops
                    .iter()
                    .map(|&(o, c, al)| (o, super::sk(hx(c), al)))
                    .collect(),
            ),
        ) {
            self.pm
                .fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    /// Top-to-bottom gradient.
    pub(super) fn rr_v(&mut self, q: Rect, rad: f32, stops: &[(f32, u32, f32)]) {
        self.rr_grad(q, rad, q.center_top(), q.center_bottom(), stops);
    }

    pub(super) fn rr_stroke(&mut self, q: Rect, rad: f32, w: f32, c: u32, a: f32) {
        if let Some(p) = self.rpath(q, rad) {
            self.pm.stroke_path(
                &p,
                &super::solid(super::sk(hx(c), a)),
                &raster::stroke(w * self.s),
                Transform::identity(),
                None,
            );
        }
    }

    pub(super) fn disc(&mut self, c: Pos2, r: f32, cl: u32, a: f32) {
        let (x, y) = self.pt(c);
        if let Some(p) = raster::circle(x, y, r * self.s) {
            self.pm.fill_path(
                &p,
                &super::solid(super::sk(hx(cl), a)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    pub(super) fn ring(&mut self, c: Pos2, r: f32, w: f32, cl: u32, a: f32) {
        let (x, y) = self.pt(c);
        if let Some(p) = raster::circle(x, y, r * self.s) {
            self.pm.stroke_path(
                &p,
                &super::solid(super::sk(hx(cl), a)),
                &raster::stroke(w * self.s),
                Transform::identity(),
                None,
            );
        }
    }

    /// A lit ball: a radial gradient with its highlight up and left.
    pub(super) fn ball(&mut self, c: Pos2, r: f32, stops: &[(f32, u32)]) {
        let (x, y) = self.pt(c);
        let rs = r * self.s;
        if let (Some(p), Some(paint)) = (
            raster::circle(x, y, rs),
            super::radial(
                (x - rs * 0.3, y - rs * 0.35),
                rs * 1.35,
                stops.iter().map(|&(o, cl)| (o, super::sk(hx(cl), 1.0))).collect(),
            ),
        ) {
            self.pm
                .fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    pub(super) fn seg(&mut self, a: Pos2, b: Pos2, w: f32, c: u32, al: f32) {
        let (pa, pb) = (self.pt(a), self.pt(b));
        if let Some(p) = raster::polyline(&[pa, pb]) {
            self.pm.stroke_path(
                &p,
                &super::solid(super::sk(hx(c), al)),
                &raster::stroke(w * self.s),
                Transform::identity(),
                None,
            );
        }
    }

    pub(super) fn poly(&mut self, pts: &[Pos2], c: u32, a: f32) {
        let v: Vec<(f32, f32)> = pts.iter().map(|&p| self.pt(p)).collect();
        if let Some(p) = raster::polygon(&v) {
            self.pm.fill_path(
                &p,
                &super::solid(super::sk(hx(c), a)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    pub(super) fn poly_stroke(&mut self, pts: &[Pos2], w: f32, c: u32, a: f32) {
        let v: Vec<(f32, f32)> = pts.iter().map(|&p| self.pt(p)).collect();
        if let Some(p) = raster::polygon(&v) {
            self.pm.stroke_path(
                &p,
                &super::solid(super::sk(hx(c), a)),
                &raster::stroke(w * self.s),
                Transform::identity(),
                None,
            );
        }
    }

    /// A soft shadow under a rounded rect (layers growing outwards).
    pub(super) fn shadow(&mut self, q: Rect, rad: f32, spread: f32, a: f32) {
        let n = 6;
        for i in (0..n).rev() {
            let k = (i as f32 + 1.0) / n as f32;
            self.rr(
                q.expand(spread * k),
                rad + spread * k,
                0x000000,
                a / n as f32 * 1.6,
            );
        }
    }

    /// Fill a rounded rect pixel by pixel: `f(x, y)` (points from `q`'s corner) gives the colour and alpha.
    pub(super) fn pattern(&mut self, q: Rect, rad: f32, f: impl Fn(f32, f32) -> [f32; 4]) {
        let s = self.s;
        let (w, h) = (self.pm.width() as i32, self.pm.height() as i32);
        let (ox, oy) = (q.min.x - self.o.x, q.min.y - self.o.y);
        let x0 = (ox * s).floor().max(0.0) as i32;
        let y0 = (oy * s).floor().max(0.0) as i32;
        let x1 = (((ox + q.width()) * s).ceil() as i32).min(w);
        let y1 = (((oy + q.height()) * s).ceil() as i32).min(h);
        let (hw, hh) = (q.width() * 0.5, q.height() * 0.5);
        let rad = rad.min(hw).min(hh).max(0.0);
        let stride = w as usize * 4;
        let data = self.pm.data_mut();
        for py in y0..y1 {
            for px in x0..x1 {
                let lx = (px as f32 + 0.5) / s - ox;
                let ly = (py as f32 + 0.5) / s - oy;
                let dx = (lx - hw).abs() - (hw - rad);
                let dy = (ly - hh).abs() - (hh - rad);
                let out = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - rad;
                let cov = (0.5 - out * s).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let [r, g, b, a] = f(lx, ly);
                let a = (a * cov).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let i = py as usize * stride + px as usize * 4;
                let src = [
                    r.clamp(0.0, 1.0) * 255.0 * a,
                    g.clamp(0.0, 1.0) * 255.0 * a,
                    b.clamp(0.0, 1.0) * 255.0 * a,
                    255.0 * a,
                ];
                for k in 0..4 {
                    let d = data[i + k] as f32;
                    data[i + k] = (src[k] + d * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    /// A chrome jack socket.
    pub(super) fn jack(&mut self, c: Pos2, r: f32) {
        self.disc(c + vec2(0.0, r * 0.15), r * 1.08, 0x000000, 0.45);
        self.ball(
            c,
            r,
            &[
                (0.0, 0xffffff),
                (0.45, 0xc3c8cc),
                (0.85, 0x6e747a),
                (1.0, 0x9da2a7),
            ],
        );
        self.ring(c, r * 0.72, r * 0.08, 0x000000, 0.28);
        self.disc(c, r * 0.46, 0x050505, 1.0);
    }

    pub(super) fn screw(&mut self, c: Pos2, r: f32) {
        self.disc(c + vec2(0.0, r * 0.2), r * 1.1, 0x000000, 0.4);
        self.ball(c, r, &[(0.0, 0xffffff), (0.5, 0xb9bec2), (1.0, 0x6a7076)]);
        self.seg(
            c + vec2(-r * 0.7, r * 0.4),
            c + vec2(r * 0.7, -r * 0.4),
            r * 0.28,
            0x3c4045,
            1.0,
        );
    }

    /// A chrome strip (handle ends, feet, side handles).
    pub(super) fn chrome(&mut self, q: Rect, rad: f32) {
        self.rr(q.translate(vec2(0.0, 1.0)), rad, 0x000000, 0.45);
        self.rr_v(
            q,
            rad,
            &[
                (0.0, 0xf6f7f8, 1.0),
                (0.42, 0xc3c8cc, 1.0),
                (0.55, 0x80868c, 1.0),
                (0.75, 0xb9bec2, 1.0),
                (1.0, 0xeceef0, 1.0),
            ],
        );
    }

    /// A carry handle across the top: a black strap with chrome ends, `leather` brown.
    pub(super) fn strap(&mut self, cx: f32, y: f32, w: f32, h: f32, leather: bool) {
        let e = h * 1.1;
        self.chrome(
            Rect::from_min_size(pos2(cx - w / 2.0 - e * 0.6, y + h * 0.2), vec2(e, h * 0.85)),
            2.0,
        );
        self.chrome(
            Rect::from_min_size(pos2(cx + w / 2.0 - e * 0.4, y + h * 0.2), vec2(e, h * 0.85)),
            2.0,
        );
        let q = Rect::from_min_size(pos2(cx - w / 2.0, y), vec2(w, h));
        self.rr(q.translate(vec2(0.0, 1.5)), h * 0.5, 0x000000, 0.5);
        if leather {
            self.rr_v(
                q,
                h * 0.5,
                &[(0.0, 0x9a6a3f, 1.0), (0.6, 0x6b4424, 1.0), (1.0, 0x4a2d16, 1.0)],
            );
        } else {
            self.rr_v(
                q,
                h * 0.5,
                &[(0.0, 0x3d3d3d, 1.0), (0.55, 0x151515, 1.0), (1.0, 0x050505, 1.0)],
            );
        }
    }
}

// -- textures ---------------------------------------------------------------------------------------------------------

/// Black tolex: grain from noise, a sheen along the top, shade along the bottom. `cell` sets the grain size
/// (fine, levant, bronco).
pub(super) fn tolex(base: u32, cell: f32, h: f32) -> impl Fn(f32, f32) -> [f32; 4] {
    let b = rgbf(base);
    move |x, y| {
        let n = vnoise(x / cell, y / cell, 7) * 0.6 + vnoise(x / (cell * 2.3), y / (cell * 2.3), 11) * 0.4;
        let pit = vnoise(x / (cell * 0.6), y / (cell * 0.6), 3);
        let g = (n - 0.5) * 0.13 + if pit > 0.8 { 0.045 } else { 0.0 } - if pit < 0.16 { 0.03 } else { 0.0 };
        let yy = y / h.max(1.0);
        let shade = 0.05 * (1.0 - yy * 4.0).max(0.0) - 0.07 * ((yy - 0.7) * 3.3).max(0.0);
        let k = |v: f32| (v + g + shade).clamp(0.0, 1.0);
        [k(b[0]), k(b[1]), k(b[2]), 1.0]
    }
}

/// Grille cloths and coverings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Cloth {
    /// Blackface silver with turquoise threads.
    SilverTurq,
    /// Tweed amp's brown-and-gold stripe.
    TweedGrille,
    /// The Vox brown diamond.
    Diamond,
    /// Vox black basketweave (covering).
    Basket,
    /// Marshall salt-and-pepper basketweave.
    SaltPepper,
    /// 80s black weave.
    BlackWeave,
    /// Black sparkle (Ampeg).
    Sparkle,
    /// Silver sparkle (flip-top).
    SilverSparkle,
    /// Lacquered tweed (covering).
    Tweed,
    /// Blue check (flip-top covering).
    BlueCheck,
    /// The Mark combo's cane (wicker) grille; the Orange guitar head's beige basket weave (P41.2).
    Cane,
    BasketTan,
    /// Chrome diamond plate.
    Tread,
}

#[allow(clippy::too_many_arguments)]
fn basket(x: f32, y: f32, base: u32, light: u32, dark: u32, size: f32, la: f32, da: f32) -> [f32; 3] {
    let h = size / 2.0;
    let (tx, ty) = (x.rem_euclid(size), y.rem_euclid(size));
    let horiz = (tx < h) == (ty < h);
    let o = if horiz { ty.rem_euclid(h) } else { tx.rem_euclid(h) } / h * 4.0;
    let d = rgbf(base);
    if (0.35..1.45).contains(&o) {
        ov(d, light, la)
    } else if (1.45..2.05).contains(&o) {
        ov(d, dark, da)
    } else if (2.35..3.45).contains(&o) {
        ov(d, dark, la)
    } else if (3.45..4.0).contains(&o) {
        ov(d, light, da)
    } else {
        d
    }
}

/// A cloth's colour at (x, y) points; `k` scales the weave (1 = as the mockups at board size).
pub(super) fn cloth(c: Cloth, x: f32, y: f32, k: f32) -> [f32; 3] {
    let (x, y) = (x / k, y / k);
    match c {
        Cloth::SilverTurq => {
            let (tx, ty) = (x.rem_euclid(3.4) / 0.85, y.rem_euclid(3.4) / 0.85);
            let mut d = rgbf(0xb7bbba);
            if ty < 1.0 {
                d = ov(d, 0x5e9ea6, 0.62);
            }
            if (2.0..3.0).contains(&ty) {
                d = ov(d, 0xe6e9e8, 0.85);
            }
            if tx >= 3.0 {
                d = ov(d, 0x2b2e2f, 0.5);
            }
            if (1.0..2.0).contains(&tx) && (1.0..2.0).contains(&ty) {
                d = ov(d, 0x2b2e2f, 0.3);
            }
            d
        }
        Cloth::TweedGrille => {
            let (tx, ty) = (x.rem_euclid(4.0), y.rem_euclid(3.2) / 0.8);
            let mut d = rgbf(0x4a3424);
            if ty < 1.2 {
                d = ov(d, 0x9a7646, 0.9);
            } else if (2.4..3.0).contains(&ty) {
                d = ov(d, 0x6b4e30, 0.8);
            }
            if tx < 0.5 {
                d = ov(d, 0x241810, 0.35);
            }
            d
        }
        Cloth::Diamond => {
            let (w, h) = (14.0, 17.5);
            let (tx, ty) = (x.rem_euclid(w), y.rem_euclid(h));
            let mut d = rgbf(0x3b231a);
            if ty.rem_euclid(1.2) < 0.45 {
                d = ov(d, 0x5c3a28, 0.45);
            }
            let q = (tx - w / 2.0).abs() / (w / 2.0) + (ty - h / 2.0).abs() / (h / 2.0);
            let dist = (q - 1.0).abs() * w * 0.39;
            if dist < 1.3 {
                d = ov(d, 0x1e100a, 0.6);
            }
            if dist < 0.65 {
                d = ov(d, 0x9b7a55, 1.0);
            }
            if dist < 0.22 {
                d = ov(d, 0xd2b68a, 0.7);
            }
            d
        }
        Cloth::Basket => basket(x, y, 0x1a1918, 0x3a3631, 0x080706, 5.0, 0.85, 0.9),
        Cloth::Cane => basket(x, y, 0x9c7a45, 0xd8bb82, 0x3d2a14, 7.0, 0.8, 0.9),
        Cloth::BasketTan => basket(x, y, 0xb9a986, 0xe6dcc2, 0x5e533e, 4.5, 0.8, 0.85),
        Cloth::SaltPepper => {
            let mut d = basket(x, y, 0x77705f, 0xddd8cb, 0x26241f, 5.0, 0.75, 0.85);
            let (tx, ty) = (x.rem_euclid(5.0), y.rem_euclid(5.0));
            if (0.8..1.5).contains(&tx) && (3.3..4.0).contains(&ty) {
                d = ov(d, 0xf2efe6, 0.7);
            }
            if (3.3..4.0).contains(&tx) && (0.8..1.5).contains(&ty) {
                d = ov(d, 0x151411, 0.8);
            }
            d
        }
        Cloth::BlackWeave => {
            let (tx, ty) = (x.rem_euclid(2.6), y.rem_euclid(2.6));
            let mut d = rgbf(0x1e1f21);
            if tx < 0.9 {
                d = ov(d, 0x4a4c50, 0.38);
            }
            if ty < 0.9 {
                d = ov(d, 0x4a4c50, 0.26);
            }
            d
        }
        Cloth::Sparkle => {
            let (tx, ty) = (x.rem_euclid(13.0), y.rem_euclid(11.0));
            let mut d = rgbf(0x1a1b1d);
            if tx < 0.6 || ty < 0.6 {
                d = ov(d, 0x2a2c2f, 0.6);
            }
            for (cx, cy, r, o) in [
                (2.0, 3.0, 0.7, 0.9),
                (8.0, 1.5, 0.5, 0.6),
                (11.0, 7.0, 0.65, 0.8),
                (5.0, 8.5, 0.45, 0.55),
                (9.5, 10.0, 0.4, 0.5),
            ] {
                if ((tx - cx).powi(2) + (ty - cy).powi(2)).sqrt() < r {
                    d = ov(d, 0x9a9ea4, o);
                }
            }
            d
        }
        Cloth::SilverSparkle => {
            let (tx, ty) = (x.rem_euclid(5.0), y.rem_euclid(5.0));
            let mut d = rgbf(0x9ea3a8);
            if ty < 0.5 {
                d = rgbf(0x8b9096);
            }
            if tx < 0.5 {
                d = rgbf(0xb3b8bd);
            }
            for (cx, cy, r, c) in [
                (1.6, 2.4, 0.45, 0xdfe3e7),
                (3.9, 4.0, 0.35, 0x7f858b),
                (4.1, 1.3, 0.3, 0xeef1f4),
            ] {
                if ((tx - cx).powi(2) + (ty - cy).powi(2)).sqrt() < r {
                    d = rgbf(c);
                }
            }
            d
        }
        Cloth::Tweed => {
            let a = ((x + y) * std::f32::consts::FRAC_1_SQRT_2).rem_euclid(3.5);
            let b = ((x - y) * std::f32::consts::FRAC_1_SQRT_2).rem_euclid(4.0);
            let mut d = rgbf(0xcaa45c);
            if a < 1.5 {
                d = ov(d, 0x7a5a2a, 0.5);
            }
            if b < 1.0 {
                d = ov(d, 0xfff0c8, 0.16);
            }
            d
        }
        Cloth::BlueCheck => {
            let (tx, ty) = (x.rem_euclid(5.0), y.rem_euclid(5.0));
            let same = (tx < 2.5) == (ty < 2.5);
            let mut d = rgbf(if same { 0x2b3c66 } else { 0x1f2c4d });
            if same && (tx.rem_euclid(2.5) - 1.2).abs() < 0.45 && (ty.rem_euclid(2.5) - 1.2).abs() < 0.45 {
                d = rgbf(0x3d5687);
            }
            d
        }
        Cloth::Tread => {
            let sz = 15.0;
            let (tx, ty) = (x.rem_euclid(sz), y.rem_euclid(sz));
            let mut d = rgbf(0xa9aeb3);
            let q = sz / 4.0;
            for (bx, by, rot) in [
                (q, q, 45.0f32),
                (3.0 * q, q, -45.0),
                (q, 3.0 * q, -45.0),
                (3.0 * q, 3.0 * q, 45.0),
            ] {
                let (sn, cs) = rot.to_radians().sin_cos();
                let lozenge = |ox: f32, oy: f32| {
                    let (px, py) = (tx - bx - ox, ty - by - oy);
                    let u = px * cs + py * sn;
                    let v = -px * sn + py * cs;
                    (((u.abs() - 2.9).max(0.0)).powi(2) + v * v).sqrt() - 1.25
                };
                if lozenge(0.6, 0.7) < 0.0 {
                    d = ov(d, 0x5f6469, 0.8);
                }
                if lozenge(0.0, 0.0) < 0.0 {
                    d = rgbf(0xc6cace);
                    let (px, py) = (tx - bx, ty - by);
                    if -px * sn + py * cs < -0.4 {
                        d = ov(d, 0xf3f5f6, 0.85);
                    }
                }
            }
            d
        }
    }
}

/// A cloth over a rect with its inner shading (darker round the edges, a bit lit at the top).
pub(super) fn grille(cv: &mut Cv, q: Rect, rad: f32, c: Cloth, k: f32) {
    let (w, h) = (q.width(), q.height());
    cv.pattern(q, rad, |x, y| {
        let d = cloth(c, x, y, k);
        let e = (x.min(w - x).min(y).min(h - y) / 9.0).clamp(0.0, 1.0);
        let lit = 0.06 * (1.0 - y / h * 3.0).max(0.0);
        let dark = (1.0 - e) * 0.38 + 0.18 * ((y / h - 0.6) * 2.5).max(0.0);
        let f = |v: f32| (v * (1.0 - dark) + lit).clamp(0.0, 1.0);
        [f(d[0]), f(d[1]), f(d[2]), 1.0]
    });
    cv.rr_stroke(q, rad, 1.4, 0x000000, 0.55);
}

/// A faint speaker behind the cloth.
pub(super) fn ghost(cv: &mut Cv, c: Pos2, r: f32, dark: bool, chrome_cap: bool) {
    let a = if dark { 1.45 } else { 1.3 };
    cv.disc(c, r, 0x000000, 0.16 * a);
    cv.ring(c, r * 0.9, r * 0.2, 0x000000, 0.1 * a);
    cv.ring(c, r * 0.97, r * 0.05, 0x000000, 0.22 * a);
    cv.ring(c, r * 0.8, r * 0.02, 0x000000, 0.12 * a);
    cv.disc(c, r * 0.24, 0x000000, 0.2 * a);
    if dark {
        // on black cloth the cones show by their rims catching the light
        cv.ring(c, r * 0.99, r * 0.03, 0xffffff, 0.1);
        cv.ring(c, r * 0.62, r * 0.02, 0xffffff, 0.06);
        cv.ring(c, r * 0.26, r * 0.03, 0xffffff, 0.08);
    }
    if chrome_cap {
        cv.disc(c, r * 0.19, 0xe4e8ec, 0.3);
    }
}

// -- live parts (egui) ------------------------------------------------------------------------------------------------

/// A filled disc with a radial gradient faked by concentric circles drifting up-left (the light's side).
pub(super) fn rgrad(p: &Painter, c: Pos2, r: f32, stops: &[(f32, u32)]) {
    let n = (r * 0.9).clamp(5.0, 14.0) as i32;
    for i in 0..n {
        let k = 1.0 - i as f32 / n as f32;
        let cc = c + vec2(-0.22, -0.28) * r * (1.0 - k);
        p.circle_filled(cc, r * k, col(sample(stops, k)));
    }
}

pub(super) fn at(c: Pos2, r: f32, deg: f32) -> Pos2 {
    let (s, co) = deg.to_radians().sin_cos();
    pos2(c.x + r * s, c.y - r * co)
}

fn rot(c: Pos2, deg: f32, v: Vec2) -> Pos2 {
    let (s, co) = deg.to_radians().sin_cos();
    pos2(c.x + v.x * co - v.y * s, c.y + v.x * s + v.y * co)
}

fn shadow(p: &Painter, c: Pos2, r: f32) {
    p.circle_filled(
        c + vec2(r * 0.08, r * 0.2),
        r * 1.04,
        Color32::from_black_alpha(70),
    );
    p.circle_filled(
        c + vec2(r * 0.04, r * 0.1),
        r * 1.02,
        Color32::from_black_alpha(90),
    );
}

/// The kinds of knob on the amps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Knob {
    /// Blackface: a black skirt with 1-10 turning with it, an aluminium cap.
    Skirted,
    /// Tweed: a brown bakelite pointer.
    Pointer,
    /// Vox: a cream chicken-head.
    Cream,
    /// A black chicken-head (a selector).
    BlackChicken,
    /// Marshall: a black knurled skirt and a gold cap.
    Gold,
    /// Mesa: a chrome knurled dome.
    Dome,
    /// Ampeg: a black skirt with a silver cap.
    Silver,
}

fn knurl(p: &Painter, c: Pos2, r: f32, color: Color32) {
    let n = ((r * 2.2) as i32).clamp(14, 40);
    for i in 0..n {
        let a = 360.0 * i as f32 / n as f32;
        p.line_segment(
            [at(c, r * 0.84, a), at(c, r * 0.98, a)],
            Stroke::new((r * 0.08).max(0.6), color),
        );
    }
}

/// A knob `d` across at `c`, turned to `u` (0..1).
pub(super) fn knob(p: &Painter, c: Pos2, d: f32, u: f32, kind: Knob) {
    let r = d * 0.5;
    let ang = -150.0 + 300.0 * u.clamp(0.0, 1.0);
    match kind {
        Knob::Skirted => {
            shadow(p, c, r);
            rgrad(p, c, r, &[(0.0, 0x3a3a3a), (0.6, 0x161616), (1.0, 0x040404)]);
            p.circle_stroke(c, r - 1.3, Stroke::new(0.8, Color32::from_white_alpha(30)));
            let v = 1.0 + 9.0 * u;
            for num in 1..=10 {
                let a = (num as f32 - v) * (300.0 / 9.0);
                if a.abs() > 150.0 {
                    continue;
                }
                if d >= 30.0 {
                    text::draw(
                        p,
                        &num.to_string(),
                        at(c, r * 0.76, a),
                        Align2::CENTER_CENTER,
                        Style::new(d * 0.13, 700, c32(0xededed)),
                        None,
                    );
                } else {
                    p.line_segment(
                        [at(c, r * 0.68, a), at(c, r * 0.9, a)],
                        Stroke::new(0.9, ca(0xededed, 0.85)),
                    );
                }
            }
            rgrad(
                p,
                c,
                d * 0.25,
                &[
                    (0.0, 0xffffff),
                    (0.35, 0xe8eaec),
                    (0.75, 0xa3a8ad),
                    (1.0, 0x8a8f94),
                ],
            );
            p.circle_stroke(c, d * 0.25, Stroke::new(0.6, ca(0x3c4044, 0.6)));
        }
        Knob::Pointer => {
            let r0 = d * 0.33;
            let pts = vec![
                rot(c, ang, vec2(-r0 * 0.62, 0.0)),
                rot(c, ang, vec2(-r * 0.08, -r * 0.94)),
                rot(c, ang, vec2(0.0, -r * 1.02)),
                rot(c, ang, vec2(r * 0.08, -r * 0.94)),
                rot(c, ang, vec2(r0 * 0.62, 0.0)),
            ];
            let sh: Vec<Pos2> = pts.iter().map(|&q| q + vec2(1.0, 2.0)).collect();
            p.add(Shape::convex_polygon(
                sh,
                Color32::from_black_alpha(90),
                Stroke::NONE,
            ));
            shadow(p, c, r0);
            p.add(Shape::convex_polygon(
                pts,
                c32(0x3a281c),
                Stroke::new(0.7, c32(0x0e0805)),
            ));
            rgrad(p, c, r0, &[(0.0, 0x6a4a34), (0.55, 0x2b1e16), (1.0, 0x120b07)]);
            p.circle_stroke(c, r0, Stroke::new(0.7, c32(0x0e0805)));
            p.line_segment(
                [
                    rot(c, ang, vec2(0.0, -r0 * 0.25)),
                    rot(c, ang, vec2(0.0, -r * 0.9)),
                ],
                Stroke::new((d * 0.03).max(1.0), c32(0xe8dcc0)),
            );
        }
        Knob::Cream | Knob::BlackChicken => {
            let black = kind == Knob::BlackChicken;
            let (b0, b1, b2) = if black {
                (0x3a3a3a, 0x151515, 0x050505)
            } else {
                (0xf3ead3, 0xd4c6a3, 0xa99a76)
            };
            shadow(p, c, d * 0.36);
            rgrad(p, c, d * 0.36, &[(0.0, b0), (0.6, b1), (1.0, b2)]);
            p.circle_stroke(c, d * 0.36, Stroke::new(0.7, Color32::from_black_alpha(110)));
            let shape: [(f32, f32); 12] = [
                (0.0, -0.5),
                (0.11, -0.41),
                (0.18, -0.22),
                (0.2, 0.04),
                (0.17, 0.26),
                (0.09, 0.37),
                (0.0, 0.41),
                (-0.09, 0.37),
                (-0.17, 0.26),
                (-0.2, 0.04),
                (-0.18, -0.22),
                (-0.11, -0.41),
            ];
            let pts: Vec<Pos2> = shape.iter().map(|&(x, y)| rot(c, ang, vec2(x, y) * d)).collect();
            let sh: Vec<Pos2> = pts.iter().map(|&q| q + vec2(1.2, 2.2)).collect();
            p.add(Shape::convex_polygon(
                sh,
                Color32::from_black_alpha(100),
                Stroke::NONE,
            ));
            let (fill, edge, line, ridge) = if black {
                (0x232323, 0x000000, 0xe9e9e9, ca(0xffffff, 0.22))
            } else {
                (0xe3d7b8, 0x6d5f43, 0x5e4e33, ca(0xffffff, 0.75))
            };
            p.add(Shape::convex_polygon(pts, c32(fill), Stroke::new(0.7, c32(edge))));
            // the lit flank of the head
            let flank: Vec<Pos2> = [
                (-0.02, -0.46),
                (-0.15, -0.22),
                (-0.17, 0.04),
                (-0.14, 0.24),
                (-0.02, 0.34),
            ]
            .iter()
            .map(|&(x, y)| rot(c, ang, vec2(x, y) * d))
            .collect();
            p.add(Shape::convex_polygon(
                flank,
                if black {
                    ca(0x4a4a4a, 0.6)
                } else {
                    ca(0xfffaf0, 0.6)
                },
                Stroke::NONE,
            ));
            p.line_segment(
                [rot(c, ang, vec2(0.0, -0.4 * d)), rot(c, ang, vec2(0.0, 0.2 * d))],
                Stroke::new(0.8, ridge),
            );
            p.line_segment(
                [
                    rot(c, ang, vec2(0.0, -0.45 * d)),
                    rot(c, ang, vec2(0.0, -0.2 * d)),
                ],
                Stroke::new((d * 0.025).max(1.0), c32(line)),
            );
        }
        Knob::Gold | Knob::Silver => {
            shadow(p, c, r);
            rgrad(p, c, r, &[(0.0, 0x383838), (0.7, 0x121212), (1.0, 0x030303)]);
            knurl(p, c, r, c32(0x464646));
            p.circle_stroke(c, r, Stroke::new(0.7, Color32::BLACK));
            if kind == Knob::Gold {
                p.line_segment(
                    [at(c, r * 0.64, ang), at(c, r * 0.92, ang)],
                    Stroke::new((d * 0.04).max(1.0), c32(0xf2efe6)),
                );
                rgrad(
                    p,
                    c,
                    r * 0.6,
                    &[
                        (0.0, 0xfff3c8),
                        (0.35, 0xe3c97f),
                        (0.75, 0xb8973f),
                        (1.0, 0x8a6c28),
                    ],
                );
                p.circle_stroke(c, r * 0.6, Stroke::new(0.7, c32(0x5e4716)));
                p.circle_stroke(c, r * 0.42, Stroke::new(0.6, ca(0x5a4214, 0.35)));
                p.line_segment(
                    [at(c, r * 0.12, ang), at(c, r * 0.54, ang)],
                    Stroke::new((d * 0.032).max(0.9), c32(0x1a1a1a)),
                );
            } else {
                rgrad(
                    p,
                    c,
                    r * 0.58,
                    &[
                        (0.0, 0xffffff),
                        (0.4, 0xd8dbde),
                        (0.85, 0x9aa0a5),
                        (1.0, 0x7d8389),
                    ],
                );
                p.circle_stroke(c, r * 0.58, Stroke::new(0.7, c32(0x4f5459)));
                for k in [0.2, 0.32, 0.44] {
                    p.circle_stroke(c, r * k, Stroke::new(0.4, ca(0xffffff, 0.3)));
                }
                p.line_segment(
                    [at(c, r * 0.62, ang), at(c, r * 0.95, ang)],
                    Stroke::new((d * 0.045).max(1.0), Color32::WHITE),
                );
            }
        }
        Knob::Dome => {
            shadow(p, c, r);
            rgrad(p, c, r, &[(0.0, 0xd8dbde), (0.7, 0x8a9096), (1.0, 0x5f656b)]);
            knurl(p, c, r, c32(0x3e4348));
            p.circle_stroke(c, r, Stroke::new(0.7, c32(0x2b2e31)));
            rgrad(
                p,
                c,
                r * 0.76,
                &[
                    (0.0, 0xffffff),
                    (0.22, 0xeef0f2),
                    (0.55, 0xb6bbc0),
                    (0.85, 0x7a8086),
                    (1.0, 0xaeb3b8),
                ],
            );
            p.circle_filled(at(c, r * 0.56, ang), (d * 0.04).max(1.2), c32(0x26292c));
        }
    }
}

/// A jewel pilot lamp `d` across: chrome bezel, faceted glass, a glow round it when on.
pub(super) fn jewel(p: &Painter, c: Pos2, d: f32, color: u32, on: bool) {
    let r = d * 0.5;
    if on {
        for (k, a) in [(2.5, 0.06), (1.9, 0.1), (1.45, 0.16), (1.15, 0.22)] {
            p.circle_filled(c, r * k, ca(color, a));
        }
    }
    shadow(p, c, r);
    rgrad(
        p,
        c,
        r,
        &[
            (0.0, 0xffffff),
            (0.5, 0xc9cdd1),
            (0.85, 0x6f747a),
            (1.0, 0xa7acb1),
        ],
    );
    p.circle_filled(c, r * 0.84, c32(0x141414));
    let (hi, mid, dk) = if on {
        (
            mixu(color, 0xffffff, 0.6),
            mixu(color, 0xff3030, 0.15),
            mixu(color, 0x000000, 0.65),
        )
    } else {
        (
            mixu(color, 0x000000, 0.35),
            mixu(color, 0x000000, 0.62),
            mixu(color, 0x000000, 0.85),
        )
    };
    rgrad(p, c, r * 0.76, &[(0.0, hi), (0.45, mid), (1.0, dk)]);
    let oct: Vec<Pos2> = (0..8).map(|i| at(c, r * 0.4, 22.5 + 45.0 * i as f32)).collect();
    let fa = if on { 0.38 } else { 0.14 };
    for i in 0..8 {
        p.line_segment([oct[i], oct[(i + 1) % 8]], Stroke::new(0.7, ca(0xffffff, fa)));
        p.line_segment(
            [oct[i], at(c, r * 0.74, 22.5 + 45.0 * i as f32)],
            Stroke::new(0.6, ca(0xffffff, fa * 0.6)),
        );
    }
    if on {
        p.circle_filled(c, r * 0.24, ca(hi, 0.55));
    }
    p.circle_filled(
        c + vec2(-r * 0.26, -r * 0.32),
        r * 0.13,
        Color32::from_white_alpha(if on { 210 } else { 110 }),
    );
}

/// A bat toggle drawn live (`s` its size), lever up or down.
pub(super) fn bat(p: &Painter, c: Pos2, s: f32, up: bool) {
    let nut: Vec<Pos2> = (0..6).map(|i| at(c, s * 0.44, 30.0 + 60.0 * i as f32)).collect();
    p.add(Shape::convex_polygon(
        nut,
        c32(0xb4b9be),
        Stroke::new(0.7, c32(0x3a3e42)),
    ));
    p.circle_filled(c, s * 0.24, c32(0x8e9399));
    let sg = if up { -1.0 } else { 1.0 };
    let lever = vec![
        c + vec2(-s * 0.09, 0.0),
        c + vec2(-s * 0.17, sg * s * 0.84),
        c + vec2(0.0, sg * s * 0.98),
        c + vec2(s * 0.17, sg * s * 0.84),
        c + vec2(s * 0.09, 0.0),
    ];
    let sh: Vec<Pos2> = lever.iter().map(|&q| q + vec2(s * 0.1, s * 0.07)).collect();
    p.add(Shape::convex_polygon(
        sh,
        Color32::from_black_alpha(90),
        Stroke::NONE,
    ));
    p.add(Shape::convex_polygon(
        lever,
        c32(0xe4e7ea),
        Stroke::new(0.6, c32(0x4a4f54)),
    ));
    p.line_segment(
        [
            c + vec2(-s * 0.05, sg * s * 0.1),
            c + vec2(-s * 0.08, sg * s * 0.8),
        ],
        Stroke::new((s * 0.06).max(0.6), Color32::WHITE),
    );
}

/// A rocker switch: `lit` glows in its colour (`None` a black one).
pub(super) fn rocker(p: &Painter, c: Pos2, w: f32, h: f32, color: Option<u32>, lit: bool) {
    let q = Rect::from_center_size(c, vec2(w, h));
    if let (Some(cl), true) = (color, lit) {
        for (k, a) in [(1.6, 0.08), (1.25, 0.14)] {
            p.rect_filled(q.expand(w * (k - 1.0)), w * 0.4, ca(cl, a));
        }
    }
    p.rect_filled(
        q.translate(vec2(0.0, h * 0.05)),
        w * 0.14,
        Color32::from_black_alpha(120),
    );
    p.rect_filled(q, w * 0.14, c32(0x111111));
    let f = q.shrink(w * 0.12);
    let (top, bot) = match color {
        Some(cl) if lit => (mixu(cl, 0xffffff, 0.4), mixu(cl, 0x000000, 0.25)),
        Some(cl) => (mixu(cl, 0x000000, 0.35), mixu(cl, 0x000000, 0.6)),
        None => (0x444444, 0x101010),
    };
    let (a, b) = f.split_top_bottom_at_fraction(0.5);
    p.rect_filled(a, w * 0.08, c32(top));
    p.rect_filled(b, w * 0.08, c32(bot));
    let ink = if color.is_some() && lit {
        Color32::from_white_alpha(230)
    } else {
        Color32::from_white_alpha(150)
    };
    p.line_segment(
        [a.center() - vec2(0.0, h * 0.09), a.center() + vec2(0.0, h * 0.09)],
        Stroke::new((w * 0.06).max(0.8), ink),
    );
    p.circle_stroke(b.center(), h * 0.08, Stroke::new((w * 0.05).max(0.7), ink));
}

/// A label in a lettering (the Print style is the app's caps).
#[allow(clippy::too_many_arguments)]
pub(super) fn lab(
    p: &Painter,
    s: &str,
    pos: Pos2,
    anchor: Align2,
    size: f32,
    color: Color32,
    max_w: f32,
    style: Lettering,
) {
    if style == Lettering::Print {
        // (a label a little too wide for its knob prints smaller before it's cut: "TREB…", P37.6c)
        let up = s.to_uppercase();
        let w = text::width(p.ctx(), &up, text::Style::caps(size, color));
        let size = if w > max_w && w > 0.0 {
            (size * max_w / w * 0.95).max(size * 0.5)
        } else {
            size
        };
        text::caps(p, &up, pos, size, color, anchor, Some(max_w.max(4.0)));
    } else {
        // (the display faces run small at a label's size: all capitals, little x-height)
        let k = if style == Lettering::Condensed { 1.3 } else { 1.0 };
        super::lettering(p, s, pos, anchor, size * k, color, max_w.max(4.0), style, true);
    }
}

/// A logo with a drop shadow under it.
#[allow(clippy::too_many_arguments)]
pub(super) fn logo(
    p: &Painter,
    s: &str,
    pos: Pos2,
    anchor: Align2,
    size: f32,
    color: Color32,
    max_w: f32,
    style: Lettering,
    caps: bool,
) {
    let off = (size * 0.06).max(0.8);
    super::lettering(
        p,
        s,
        pos + vec2(off * 0.5, off),
        anchor,
        size,
        Color32::from_black_alpha(170),
        max_w,
        style,
        caps,
    );
    super::lettering(p, s, pos, anchor, size, color, max_w, style, caps);
}

// -- the amps ---------------------------------------------------------------------------------------------------------

/// Which amp is drawn: one per model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Amp {
    Twin,
    Tweed,
    Ac30,
    Plexi,
    Jcm,
    Mesa,
    Svt,
    /// The bass heads in the Blue Line 300's build (P37.6c): the flip-top's head, the solid-state head, the
    /// orange all-tube head.
    B15,
    Gk,
    Orange,
    /// The guitar heads of P41.2 (D167): the Mark, the SLO, the Orange-style guitar head, the Hiwatt-style head.
    Mark,
    Slo,
    Tang,
    Hiwatt,
}

impl Amp {
    /// A head drawn in the SVT's build (a panel over a grille).
    fn head(self) -> bool {
        matches!(
            self,
            Amp::Svt | Amp::B15 | Amp::Gk | Amp::Orange | Amp::Mark | Amp::Slo | Amp::Tang | Amp::Hiwatt
        )
    }
}

/// The amps with a designed face (P37.2: the new bass heads use the generic one until theirs are drawn).
pub(super) fn designed(id: &str) -> bool {
    matches!(
        id,
        "amp_clean"
            | "amp_tweed"
            | "amp_chime"
            | "amp_crunch"
            | "amp_lead"
            | "amp_modern"
            | "amp_bass"
            | "amp_b15"
            | "amp_bass_ss"
            | "amp_bass_tube"
            | "amp_cali"
            | "amp_snake"
            | "amp_tangerine"
            | "amp_surrey"
    )
}

pub(super) fn amp_of(id: &str) -> Amp {
    match id {
        "amp_clean" => Amp::Twin,
        "amp_tweed" => Amp::Tweed,
        "amp_chime" => Amp::Ac30,
        "amp_crunch" => Amp::Plexi,
        "amp_lead" => Amp::Jcm,
        "amp_modern" => Amp::Mesa,
        "amp_b15" => Amp::B15,
        "amp_bass_ss" => Amp::Gk,
        "amp_bass_tube" => Amp::Orange,
        "amp_cali" => Amp::Mark,
        "amp_snake" => Amp::Slo,
        "amp_tangerine" => Amp::Tang,
        "amp_surrey" => Amp::Hiwatt,
        _ => Amp::Svt,
    }
}

/// Something on an amp's control panel, left to right.
#[derive(Clone, Copy, Debug, PartialEq)]
enum It {
    /// A knob (its index in the model's knobs).
    K(usize),
    /// A two-way choice as a switch.
    Tog(usize),
    /// Input jacks (1, 2 side by side, 4 as two by two).
    Jacks(u8),
    /// The pilot lamp (the 800's MAINS rocker).
    Jewel,
    /// The standby switch: the face's on/off.
    Standby,
    /// The power switch, always on.
    Power,
    /// The name on the panel (only where there's room).
    Logo(f32),
    Fuse,
    /// The Rectifier's mode switch (P41.2: a real three-way switch on the face, its knob index).
    Mode(usize),
    /// The Mark's five-band graphic EQ: five sliders from this knob index (P41.2).
    Geq(usize),
    Gap(f32),
}

impl It {
    fn weight(self) -> f32 {
        match self {
            It::K(_) => 1.0,
            It::Tog(_) => 0.55,
            It::Jacks(1) => 0.5,
            It::Jacks(_) => 0.85,
            It::Jewel => 0.62,
            It::Standby | It::Power => 0.55,
            It::Logo(w) => w,
            It::Fuse => 0.5,
            It::Mode(_) => 0.85,
            It::Geq(_) => 3.0,
            It::Gap(g) => g,
        }
    }

    fn decor(self) -> bool {
        matches!(self, It::Jacks(_) | It::Logo(_) | It::Fuse | It::Power)
    }
}

fn items(a: Amp) -> &'static [It] {
    use It::*;
    match a {
        // (P37.6c, D140: a control the real amp doesn't have isn't on its panel: Master on the Twin, the tweed, the
        // Plexi and the B-15, Depth on the Rectifier sit on the "added" plate beside the face)
        // (P41.2, D169: as the real panel runs, Bright then Volume, Treble, Middle, Bass, Reverb, Speed, Intensity)
        Amp::Twin => &[
            Jacks(2),
            Tog(0),
            K(1),
            K(2),
            K(3),
            K(4),
            K(5),
            K(6),
            K(7),
            Logo(1.6),
            Standby,
            Power,
            Jewel,
        ],
        Amp::Tweed => &[Logo(1.9), Jacks(4), K(0), K(1), Fuse, Jewel, Standby, Power],
        Amp::Ac30 => &[
            Jacks(4),
            K(0),
            K(1),
            K(2),
            Gap(0.25),
            K(3),
            K(4),
            Logo(1.9),
            Standby,
            Jewel,
        ],
        Amp::Plexi => &[
            Power,
            Standby,
            Jewel,
            Logo(1.15),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            Jacks(4),
        ],
        Amp::Jcm => &[
            Jewel,
            Standby,
            Logo(1.8),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            K(5),
            Jacks(2),
        ],
        Amp::Mesa => &[
            Power,
            Jewel,
            Standby,
            Logo(1.5),
            Mode(7),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            K(6),
            Jacks(1),
        ],
        Amp::Svt => &[
            Jacks(2),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            K(5),
            Tog(6),
            Gap(0.4),
            Tog(7),
            Power,
            Standby,
            Jewel,
            Logo(1.1),
        ],
        Amp::B15 => &[
            Jacks(2),
            Tog(3),
            K(0),
            K(1),
            K(2),
            Gap(0.3),
            Power,
            Standby,
            Jewel,
            Logo(1.3),
        ],
        Amp::Gk => &[
            Jacks(1),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            Tog(5),
            K(6),
            Power,
            Standby,
            Jewel,
            Logo(1.1),
        ],
        Amp::Orange => &[
            Jacks(1),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            Gap(0.3),
            Power,
            Standby,
            Jewel,
            Logo(1.3),
        ],
        // the Mark IIC+: Volume (our Drive), Treble, Bass, Middle, Master, Presence, the graphic EQ at the right
        Amp::Mark => &[
            Jacks(1),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            K(5),
            Geq(6),
            Standby,
            Power,
            Jewel,
        ],
        // the SLO-100: Gain, Bass, Middle, Treble, Master, Presence
        Amp::Slo => &[
            Jacks(1),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            K(5),
            Logo(1.3),
            Standby,
            Power,
            Jewel,
        ],
        // the Orange dirty channel: Volume, Treble, Middle, Bass, Gain
        Amp::Tang => &[
            Logo(1.4),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            Gap(0.3),
            Jacks(1),
            Standby,
            Power,
            Jewel,
        ],
        // the DR103: four inputs, Normal, Brilliant, Bass, Treble, Middle, Presence, Master
        Amp::Hiwatt => &[
            Jacks(4),
            K(0),
            K(1),
            K(2),
            K(3),
            K(4),
            K(5),
            K(6),
            Standby,
            Power,
            Jewel,
        ],
    }
}

/// How an amp is dressed.
struct Dress {
    knob: Knob,
    ink: u32,
    letters: Lettering,
    /// Labels over the knobs (Marshall, Mesa, Ampeg) or under them.
    above: bool,
    jewel: u32,
}

fn dress(a: Amp) -> Dress {
    let (knob, ink, letters, above, jewel) = match a {
        Amp::Twin => (Knob::Skirted, 0xf2f0ea, Lettering::Print, false, 0xc8102e),
        Amp::Tweed => (Knob::Pointer, 0x1b1b1b, Lettering::Condensed, false, 0xb0101e),
        Amp::Ac30 => (Knob::Cream, 0x2a160a, Lettering::Print, false, 0xc21e1e),
        Amp::Plexi | Amp::Jcm => (Knob::Gold, 0x161616, Lettering::Condensed, true, 0xb5121b),
        Amp::Mesa => (Knob::Dome, 0xededed, Lettering::Condensed, true, 0xb3141c),
        Amp::Svt | Amp::B15 => (Knob::Silver, 0x161616, Lettering::Print, true, 0xc8102e),
        Amp::Gk => (Knob::Dome, 0xeeeeee, Lettering::Condensed, true, 0xd8401e),
        Amp::Orange | Amp::Tang => (Knob::BlackChicken, 0x161616, Lettering::Heavy, true, 0xe8761e),
        Amp::Mark => (Knob::Silver, 0xe9e6dc, Lettering::Condensed, false, 0xd02a1e),
        Amp::Slo => (Knob::Silver, 0x141414, Lettering::Condensed, true, 0xd8261c),
        Amp::Hiwatt => (Knob::Silver, 0xe6e6e2, Lettering::Print, false, 0xc8102e),
    };
    Dress {
        knob,
        ink,
        letters,
        above,
        jewel,
    }
}

/// Where the parts of an amp's front are.
struct Geo {
    /// The cabinet (the front's outline).
    cab: Rect,
    /// The control panel.
    panel: Rect,
    /// The grille cloth (empty on the Mesa).
    grille: Rect,
    /// The Mesa's diamond plate; the tweed and AC30's top (where the panel lies).
    band: Rect,
    /// Corner radius.
    rad: f32,
    /// A weave scale for the cloths.
    k: f32,
}

fn geo(a: Amp, r: Rect, detail: Detail) -> Geo {
    let full = detail == Detail::Full;
    let (w, h) = (r.width(), r.height());
    let k = (h / 200.0).clamp(0.7, 1.4);
    // on the board a handle stands above the cabinet
    let handle = if full { 0.0 } else { (h * 0.05).clamp(4.0, 10.0) };
    let cab = Rect::from_min_max(pos2(r.min.x, r.min.y + handle), r.max);
    let ch = cab.height();
    let b = (w.min(ch) * 0.035).clamp(3.0, 10.0);
    match a {
        Amp::Twin
        | Amp::Plexi
        | Amp::Jcm
        | Amp::Svt
        | Amp::B15
        | Amp::Gk
        | Amp::Orange
        | Amp::Mark
        | Amp::Slo
        | Amp::Tang
        | Amp::Hiwatt => {
            let f = match (a, full) {
                (_, true) => 0.56,
                (Amp::Twin, false) => 0.3,
                (a, false) if a.head() => 0.42,
                _ => 0.47,
            };
            let side = if a == Amp::Svt && !full { w * 0.025 } else { 0.0 };
            let cab = Rect::from_min_max(
                pos2(cab.min.x + side, cab.min.y),
                pos2(cab.max.x - side, cab.max.y),
            );
            let panel = Rect::from_min_size(
                pos2(cab.min.x + b, cab.min.y + b),
                vec2(cab.width() - 2.0 * b, ch * f - b),
            );
            let foot = if a == Amp::Twin { b * 1.6 } else { b * 1.2 };
            let grille = Rect::from_min_max(
                pos2(cab.min.x + b * 1.2, panel.max.y + b * 0.9),
                pos2(cab.max.x - b * 1.2, cab.max.y - foot),
            );
            Geo {
                cab,
                panel,
                grille,
                band: Rect::NOTHING,
                rad: if a == Amp::Twin { b * 1.3 } else { b },
                k,
            }
        }
        Amp::Mesa => {
            let tread = if full { 0.38 } else { 0.42 };
            let band = Rect::from_min_size(
                pos2(cab.min.x + b, cab.min.y + b),
                vec2(cab.width() - 2.0 * b, ch * tread),
            );
            let panel = Rect::from_min_max(
                pos2(cab.min.x + b, band.max.y + b * 0.8),
                pos2(cab.max.x - b, cab.max.y - b),
            );
            Geo {
                cab,
                panel,
                grille: Rect::NOTHING,
                band,
                rad: b,
                k,
            }
        }
        Amp::Tweed | Amp::Ac30 => {
            // the panel lies on the top, seen from a little above: a strip over the front
            let f = if full { 0.56 } else { 0.27 };
            let top = Rect::from_min_size(cab.min, vec2(cab.width(), ch * f));
            let inset = if a == Amp::Tweed { w * 0.08 } else { w * 0.06 };
            let panel = Rect::from_min_max(
                pos2(top.min.x + inset, top.min.y + top.height() * 0.1),
                pos2(top.max.x - inset, top.max.y - top.height() * 0.08),
            );
            let front = Rect::from_min_max(pos2(cab.min.x, top.max.y), cab.max);
            let fb = (front.width().min(front.height()) * 0.08).clamp(4.0, 22.0);
            let grille = if a == Amp::Tweed {
                Rect::from_min_max(
                    pos2(front.min.x + fb, front.min.y + fb * 0.9),
                    pos2(front.max.x - fb, front.max.y - fb * 1.05),
                )
            } else {
                let band = front.height() * 0.26;
                Rect::from_min_max(
                    pos2(front.min.x + b * 1.3, front.min.y + band),
                    pos2(front.max.x - b * 1.3, front.max.y - b * 1.3),
                )
            };
            Geo {
                cab,
                panel,
                grille,
                band: top,
                rad: if a == Amp::Tweed { (w * 0.05).min(16.0) } else { b },
                k,
            }
        }
    }
}

/// Where each item on the panel goes: (item, centre), and the knobs' size. Decorations are left out where they'd
/// crowd the knobs.
fn place(a: Amp, g: &Geo, detail: Detail) -> (Vec<(It, Pos2)>, f32) {
    let all = items(a);
    let p = g.panel;
    let dr = dress(a);
    let pad = p.height() * 0.1;
    let (x0, x1) = (p.min.x + pad, p.max.x - pad);
    let sum = |v: &[It]| v.iter().map(|i| i.weight()).sum::<f32>();
    let kh = if dr.above { 0.44 } else { 0.5 };
    let full_unit = (x1 - x0) / sum(all);
    let keep: Vec<It> = if detail == Detail::Full && full_unit >= p.height() * 0.62 {
        all.to_vec()
    } else {
        all.iter().copied().filter(|i| !i.decor()).collect()
    };
    let unit = (x1 - x0) / sum(&keep);
    let d = (unit * 0.74).min(p.height() * kh).max(8.0);
    let y = if dr.above {
        p.center().y + p.height() * 0.12
    } else {
        p.center().y - p.height() * 0.09
    };
    let mut x = x0;
    let mut out = Vec::new();
    for it in keep {
        let w = it.weight() * unit;
        out.push((it, pos2(x + w * 0.5, y)));
        x += w;
    }
    (out, d)
}

/// The Rectifier's mode switch: the lever and its three names (the click spot).
fn mode_rect(c: Pos2, d: f32) -> Rect {
    Rect::from_min_max(c + vec2(-d * 0.42, -d * 0.62), c + vec2(d * 0.62, d * 0.62))
}

/// The Mark's graphic EQ sliders' tracks, in an item `wt` knobs wide centred on `c`.
fn geq_tracks(c: Pos2, d: f32, wt: f32) -> Vec<Rect> {
    let span = d / 0.74 * wt * 0.86;
    let step = span / 5.0;
    let h = d * 1.15;
    (0..5)
        .map(|k| {
            let x = c.x - span * 0.5 + step * (k as f32 + 0.5);
            Rect::from_center_size(pos2(x, c.y - d * 0.05), vec2((step * 0.42).clamp(5.0, 12.0), h))
        })
        .collect()
}

/// A three-way mini toggle: the lever up (-1), centre (0) or down (1).
fn toggle3(p: &Painter, c: Pos2, s: f32, pos: i32) {
    let nut: Vec<Pos2> = (0..6).map(|i| at(c, s * 0.44, 30.0 + 60.0 * i as f32)).collect();
    p.add(Shape::convex_polygon(
        nut,
        c32(0xb4b9be),
        Stroke::new(0.7, c32(0x3a3e42)),
    ));
    p.circle_filled(c, s * 0.24, c32(0x8e9399));
    let end = c + vec2(0.0, pos as f32 * s * 0.9);
    p.line_segment([c, end], Stroke::new((s * 0.22).max(1.5), c32(0xdfe2e5)));
    p.circle_filled(end, (s * 0.17).max(1.2), c32(0xf2f4f6));
}

fn label_size(d: f32) -> f32 {
    (d * 0.21).clamp(5.5, 10.5)
}

thread_local! {
    /// The last amp drawn on the board this frame: (pass, its right edge, which). The 2x12 after it takes its look.
    static SEEN: RefCell<(u64, f32, Option<Amp>)> = const { RefCell::new((u64::MAX, 0.0, None)) };
}

/// The amp drawn on the board just before `x` this pass, if any (for the 2x12's look).
pub(super) fn amp_before(pass: u64, x: f32) -> Option<Amp> {
    SEEN.with(|s| {
        let s = s.borrow();
        if s.0 == pass && s.1 <= x + 1.0 { s.2 } else { None }
    })
}

pub(super) fn layout(b: &Block, r: Rect, detail: Detail) -> Layout {
    let a = amp_of(b.def().id);
    let g = geo(a, r, detail);
    let (its, d) = place(a, &g, detail);
    let mut l = Layout {
        radius: g.rad,
        panel: g.panel,
        ..Default::default()
    };
    for (it, c) in its {
        match it {
            It::K(i) => {
                let dd = if a == Amp::Tweed { d * 0.86 } else { d };
                l.knobs.push((i, c, dd));
            }
            It::Tog(i) => l
                .choices
                .push((i, Rect::from_center_size(c, vec2(d * 0.42, d * 0.8)))),
            It::Standby => l.switch = Rect::from_center_size(c, vec2(d * 0.55, d * 0.85)),
            It::Jewel => l.led = c,
            It::Mode(i) => l.choices.push((i, mode_rect(c, d))),
            It::Geq(i) => {
                for (k, tr) in geq_tracks(c, d, It::Geq(i).weight()).into_iter().enumerate() {
                    l.faders.push((i + k, tr));
                }
            }
            _ => {}
        }
    }
    l.name = if g.grille.is_positive() { g.grille } else { g.band };
    l
}

/// The body (cached per size): everything that doesn't move.
pub(super) fn body(b: &Block, r: Rect, l: &Layout, ppp: f32) -> Image {
    let a = amp_of(b.def().id);
    let detail = if geo(a, r, Detail::Full).panel == l.panel {
        Detail::Full
    } else {
        Detail::Board
    };
    let g = geo(a, r, detail);
    let Some(mut cv) = Cv::new(r, ppp) else {
        return Image::new(1, 1);
    };
    let full = detail == Detail::Full;
    let c = g.cab;
    let (w, h) = (c.width(), c.height());
    let b0 = (w.min(h) * 0.035).clamp(3.0, 10.0);
    // the handle on top (on the board)
    if !full {
        let hh = (c.min.y - r.min.y).max(4.0);
        match a {
            Amp::Tweed => cv.strap(c.center().x, r.min.y, w * 0.26, hh * 0.9, true),
            Amp::Ac30 => cv.strap(c.center().x, r.min.y, w * 0.38, hh * 0.9, false),
            a if a.head() => {}
            Amp::Mesa => cv.strap(c.center().x, r.min.y, w * 0.3, hh * 1.1, false),
            _ => cv.strap(c.center().x, r.min.y, w * 0.25, hh * 0.95, false),
        }
    }
    if a == Amp::Svt && !full {
        // chrome side handles
        for x in [r.min.x, c.max.x - 1.0] {
            cv.chrome(
                Rect::from_min_size(
                    pos2(x, c.min.y + h * 0.35),
                    vec2(c.min.x - r.min.x + 1.0, h * 0.28),
                ),
                2.0,
            );
        }
    }
    // the covering
    match a {
        Amp::Tweed => {
            cv.pattern(c, g.rad, |x, y| {
                let d = cloth(Cloth::Tweed, x, y, 1.0);
                let e = (x.min(w - x).min(y).min(h - y) / (w.min(h) * 0.12)).clamp(0.0, 1.0);
                let burn = (1.0 - e).powf(1.5) * 0.55;
                let lit = 0.08 * (1.0 - y / h * 3.0).max(0.0);
                let f = |v: f32, k: f32| (v * (1.0 - burn * k) + lit).clamp(0.0, 1.0);
                [f(d[0], 0.9), f(d[1], 1.05), f(d[2], 1.25), 1.0]
            });
        }
        Amp::Ac30 => {
            let k = g.k;
            cv.pattern(c, g.rad, move |x, y| {
                let d = cloth(Cloth::Basket, x, y, k);
                let s = 0.05 * (1.0 - y / h * 4.0).max(0.0) - 0.1 * ((y / h - 0.7) * 3.0).max(0.0);
                [d[0] + s, d[1] + s, d[2] + s, 1.0]
            });
        }
        Amp::Twin => cv.pattern(c, g.rad, tolex(0x151515, 0.9, h)),
        Amp::Mesa => cv.pattern(c, g.rad, tolex(0x101010, 1.8, h)),
        // (the flip-top's blue check vinyl, the orange head's orange tolex)
        Amp::B15 => cv.pattern(c, g.rad, |x, y| {
            let d = cloth(Cloth::BlueCheck, x, y, 1.0);
            let s = 0.05 * (1.0 - y / h * 4.0).max(0.0) - 0.12 * ((y / h - 0.7) * 3.0).max(0.0);
            [d[0] + s, d[1] + s, d[2] + s, 1.0]
        }),
        Amp::Orange | Amp::Tang => cv.pattern(c, g.rad, tolex(0xd8661a, 1.1, h)),
        // (the SLO's snakeskin: dark scales over the black)
        Amp::Slo => cv.pattern(c, g.rad, move |x, y| {
            let (sx, sy) = (x / 7.0, y / 5.0 + 0.5 * (x / 7.0).floor());
            let (fx, fy) = (sx.fract() - 0.5, sy.fract() - 0.5);
            let e = (fx * fx * 1.6 + fy * fy).sqrt();
            let base = 0.09 + 0.05 * vnoise(sx.floor(), sy.floor(), 11);
            let v = base * (1.25 - e) + if e > 0.46 { -0.04 } else { 0.0 };
            let s = 0.05 * (1.0 - y / h * 4.0).max(0.0);
            [v + s, v * 0.95 + s, v * 0.85 + s, 1.0]
        }),
        _ => cv.pattern(c, g.rad, tolex(0x121212, 1.3, h)),
    }
    // a bevel: light along the top, dark along the bottom
    cv.rr_v(
        c,
        g.rad,
        &[
            (0.0, 0xffffff, 0.1),
            (0.04, 0xffffff, 0.0),
            (0.94, 0x000000, 0.0),
            (1.0, 0x000000, 0.45),
        ],
    );
    cv.rr_stroke(c, g.rad, 1.0, 0x000000, 0.7);
    // the top of the tweed and AC30, where the panel lies
    if matches!(a, Amp::Tweed | Amp::Ac30) {
        let tp = g.band;
        cv.rr_v(
            tp,
            g.rad,
            &[(0.0, 0xffffff, 0.12), (0.85, 0x000000, 0.0), (1.0, 0x000000, 0.4)],
        );
        cv.seg(
            pos2(tp.min.x + 2.0, tp.max.y),
            pos2(tp.max.x - 2.0, tp.max.y),
            1.4,
            0x000000,
            0.55,
        );
    }
    // the panel
    let p = g.panel;
    let pr = if a == Amp::Tweed { 5.0 } else { 2.5 };
    cv.shadow(p.translate(vec2(0.0, 1.5)), pr, 2.0, 0.5);
    let ph = p.height();
    match a {
        Amp::Twin => {
            cv.rr_v(
                p,
                pr,
                &[(0.0, 0x1b1b1b, 1.0), (0.3, 0x0e0e0e, 1.0), (1.0, 0x0a0a0a, 1.0)],
            );
            cv.rr_stroke(p.shrink(ph * 0.05), 2.5, 1.2, 0x9ea2a6, 1.0);
        }
        Amp::Tweed => {
            cv.pattern(p, pr, |x, y| {
                let d = sample(
                    &[
                        (0.0, 0xf1f3f4),
                        (0.16, 0xccd0d3),
                        (0.44, 0xa9aeb2),
                        (0.7, 0xd6d9dc),
                        (1.0, 0x9da2a6),
                    ],
                    y / ph,
                );
                let hl = if x.rem_euclid(3.0) < 1.0 { 0.04 } else { -0.01 };
                [d[0] + hl, d[1] + hl, d[2] + hl, 1.0]
            });
            let sr = (ph * 0.045).clamp(1.5, 4.5);
            for (x, y) in [
                (p.min.x, p.min.y),
                (p.max.x, p.min.y),
                (p.min.x, p.max.y),
                (p.max.x, p.max.y),
            ] {
                let cc = pos2(
                    x + (p.center().x - x).signum() * sr * 2.2,
                    y + (p.center().y - y).signum() * sr * 2.2,
                );
                cv.screw(cc, sr);
            }
        }
        Amp::Ac30 => {
            cv.pattern(p, pr, |_, y| {
                let d = sample(
                    &[
                        (0.0, 0xdc9a5f),
                        (0.38, 0xbd7b42),
                        (0.72, 0xa5642f),
                        (1.0, 0x8a4f24),
                    ],
                    y / ph,
                );
                let m = y.rem_euclid(2.5);
                let hl = if m < 0.5 {
                    0.06
                } else if m < 1.2 {
                    -0.04
                } else {
                    0.0
                };
                [d[0] + hl, d[1] + hl, d[2] + hl, 1.0]
            });
            cv.rr_stroke(p.shrink(1.5), pr, 0.8, 0x3c1905, 0.55);
        }
        Amp::Plexi => {
            let pw = p.width();
            cv.pattern(p, pr, |x, y| {
                let d = sample(
                    &[
                        (0.0, 0xf4dc9a),
                        (0.3, 0xdcb85d),
                        (0.66, 0xcda650),
                        (1.0, 0xa9852f),
                    ],
                    y / ph,
                );
                // glossy acrylic: two diagonal streaks of light
                let tt = (x / pw) + (y / ph) * 0.12;
                let gl = 0.35 * (1.0 - ((tt - 0.33) / 0.06).abs()).max(0.0)
                    + 0.18 * (1.0 - ((tt - 0.7) / 0.04).abs()).max(0.0);
                [d[0] + gl * 0.3, d[1] + gl * 0.28, d[2] + gl * 0.2, 1.0]
            });
            cv.rr_stroke(p.shrink(ph * 0.05), 2.0, 1.0, 0x1a1a1a, 0.9);
        }
        Amp::Jcm => {
            cv.pattern(p, pr, |_, y| {
                let d = sample(&[(0.0, 0xd9bc70), (0.45, 0xc9a653), (1.0, 0xb8953f)], y / ph);
                let m = y.rem_euclid(2.5);
                let hl = if m < 0.5 {
                    0.1
                } else if m < 1.0 {
                    -0.05
                } else {
                    0.0
                };
                [d[0] + hl, d[1] + hl, d[2] + hl, 1.0]
            });
            cv.rr_stroke(p.shrink(ph * 0.05), 1.5, 1.0, 0x1a1a1a, 0.9);
        }
        Amp::Mesa => {
            cv.rr_v(
                p,
                pr,
                &[(0.0, 0x1a1a1c, 1.0), (0.4, 0x0f0f10, 1.0), (1.0, 0x0a0a0b, 1.0)],
            );
            cv.rr_stroke(p.shrink(2.0), 2.0, 0.8, 0xffffff, 0.1);
        }
        Amp::Svt => {
            cv.pattern(p, pr, |_, y| {
                let d = sample(&[(0.0, 0xe2e5e7), (0.45, 0xc7cbcf), (1.0, 0xb3b8bd)], y / ph);
                let m = y.rem_euclid(2.0);
                let hl = if m < 0.5 {
                    0.12
                } else if m < 1.0 {
                    -0.04
                } else {
                    0.0
                };
                [d[0] + hl, d[1] + hl, d[2] + hl, 1.0]
            });
            let ly = p.min.y + ph * 0.12;
            cv.rr_v(
                Rect::from_min_max(
                    pos2(p.min.x + ph * 0.08, ly),
                    pos2(p.max.x - ph * 0.08, ly + (ph * 0.03).max(1.5)),
                ),
                0.5,
                &[(0.0, 0x5d86d6, 1.0), (0.5, 0x2f5bb0, 1.0), (1.0, 0x1f3f80, 1.0)],
            );
            cv.rr_stroke(p.shrink(1.5), 2.0, 0.8, 0x464b50, 0.5);
        }
        Amp::B15 => {
            // brushed steel, darker than the SVT's, with no stripe
            cv.pattern(p, pr, |_, y| {
                let d = sample(&[(0.0, 0xd3d7db), (0.5, 0xb6bbc0), (1.0, 0x9ea4aa)], y / ph);
                let hl = if y.rem_euclid(2.0) < 0.5 { 0.08 } else { 0.0 };
                [d[0] + hl, d[1] + hl, d[2] + hl, 1.0]
            });
            cv.rr_stroke(p.shrink(1.5), 2.0, 0.8, 0x2b3c66, 0.7);
        }
        Amp::Gk => {
            // black anodised, a red-orange stripe along the top
            cv.rr_v(
                p,
                pr,
                &[(0.0, 0x222225, 1.0), (0.4, 0x161618, 1.0), (1.0, 0x0e0e10, 1.0)],
            );
            let ly = p.min.y + ph * 0.1;
            cv.rr(
                Rect::from_min_max(
                    pos2(p.min.x + ph * 0.06, ly),
                    pos2(p.max.x - ph * 0.06, ly + (ph * 0.035).max(1.5)),
                ),
                0.5,
                0xd8401e,
                1.0,
            );
            cv.rr_stroke(p.shrink(1.5), 2.0, 0.8, 0xffffff, 0.1);
        }
        Amp::Orange => {
            // a cream panel with a black border
            cv.rr_v(
                p,
                pr,
                &[(0.0, 0xf6f1e4, 1.0), (0.6, 0xe9e2d0, 1.0), (1.0, 0xd9d1bc, 1.0)],
            );
            cv.rr_stroke(p.shrink(2.0), 2.0, 1.4, 0x161616, 0.85);
        }
        Amp::Tang => {
            // the cream panel, its black border and a thin orange line inside
            cv.rr_v(
                p,
                pr,
                &[(0.0, 0xf6f1e4, 1.0), (0.6, 0xe9e2d0, 1.0), (1.0, 0xd9d1bc, 1.0)],
            );
            cv.rr_stroke(p.shrink(2.0), 2.0, 1.4, 0x161616, 0.85);
            cv.rr_stroke(p.shrink(ph * 0.09), 1.5, 0.8, 0xd8661a, 0.8);
        }
        Amp::Mark => {
            // a black panel, a thin silver frame round the EQ
            cv.rr_v(
                p,
                pr,
                &[(0.0, 0x1c1c1c, 1.0), (0.45, 0x111111, 1.0), (1.0, 0x0a0a0a, 1.0)],
            );
            cv.rr_stroke(p.shrink(1.5), 2.0, 0.8, 0xffffff, 0.12);
        }
        Amp::Slo => {
            // a polished chrome panel
            cv.chrome(p, pr);
            cv.rr_stroke(p.shrink(1.0), 2.0, 0.8, 0x2e3236, 0.7);
        }
        Amp::Hiwatt => {
            // the grey panel, white print, a white rule along the top
            cv.rr_v(
                p,
                pr,
                &[(0.0, 0x3a3b3d, 1.0), (0.5, 0x2c2d2f, 1.0), (1.0, 0x222325, 1.0)],
            );
            let ly = p.min.y + ph * 0.12;
            cv.rr(
                Rect::from_min_max(
                    pos2(p.min.x + ph * 0.08, ly),
                    pos2(p.max.x - ph * 0.08, ly + (ph * 0.025).max(1.0)),
                ),
                0.5,
                0xe6e6e2,
                0.8,
            );
            cv.rr_stroke(p.shrink(1.5), 2.0, 0.8, 0x000000, 0.6);
        }
    }
    // the static parts on the panel
    let (its, d) = place(a, &g, detail);
    if a == Amp::Ac30 {
        // the boxed groups round the knobs
        for grp in [[0usize, 2], [3, 4]] {
            let xs: Vec<f32> = its
                .iter()
                .filter_map(|&(it, c)| match it {
                    It::K(i) if i >= grp[0] && i <= grp[1] => Some(c.x),
                    _ => None,
                })
                .collect();
            if let (Some(lo), Some(hi)) = (xs.first(), xs.last()) {
                let q = Rect::from_min_max(
                    pos2(lo - d * 0.72, p.min.y + ph * 0.16),
                    pos2(hi + d * 0.72, p.max.y - ph * 0.07),
                );
                cv.rr_stroke(q, ph * 0.08, 1.1, 0x2a160a, 0.75);
            }
        }
    }
    for &(it, c) in &its {
        match it {
            It::Jacks(n) => {
                let jr = (d * 0.19).clamp(2.5, 9.0);
                let pts: Vec<Pos2> = match n {
                    1 => vec![c],
                    2 => vec![c - vec2(jr * 1.4, 0.0), c + vec2(jr * 1.4, 0.0)],
                    _ => vec![
                        c + vec2(-jr * 1.4, -jr * 1.4),
                        c + vec2(jr * 1.4, -jr * 1.4),
                        c + vec2(-jr * 1.4, jr * 1.4),
                        c + vec2(jr * 1.4, jr * 1.4),
                    ],
                };
                for q in pts {
                    cv.jack(q, jr);
                }
            }
            It::Fuse => {
                let fr = d * 0.2;
                cv.ball(c, fr, &[(0.0, 0xffffff), (0.5, 0xd0d3d6), (1.0, 0x8f9499)]);
                cv.rr(
                    Rect::from_center_size(c, vec2(fr * 1.2, fr * 0.3)),
                    0.5,
                    0x4a4f54,
                    1.0,
                );
            }
            It::Geq(_) => {
                // the sliders' slots
                for tr in geq_tracks(c, d, it.weight()) {
                    cv.rr(tr.shrink2(vec2(tr.width() * 0.32, 0.0)), 1.0, 0x000000, 0.9);
                }
            }
            It::Logo(_) => match a {
                Amp::Tweed => {
                    let q = Rect::from_center_size(c, vec2(d * 1.75, d * 0.68));
                    cv.shadow(q.translate(vec2(0.0, 1.5)), d * 0.15, 2.0, 0.5);
                    cv.rr_v(
                        q,
                        d * 0.15,
                        &[(0.0, 0x6e2229, 1.0), (0.6, 0x4c1519, 1.0), (1.0, 0x3c1013, 1.0)],
                    );
                    cv.rr_stroke(q.shrink(1.5), d * 0.13, 1.0, 0xc9a96a, 1.0);
                }
                Amp::Jcm => {
                    let q = Rect::from_center_size(c, vec2(d * 1.6, d * 0.75));
                    cv.shadow(q.translate(vec2(0.0, 1.0)), 2.0, 1.5, 0.5);
                    cv.rr_v(
                        q,
                        2.0,
                        &[(0.0, 0x2a2a2a, 1.0), (0.4, 0x0b0b0b, 1.0), (1.0, 0x050505, 1.0)],
                    );
                }
                Amp::Mesa => oval_badge(&mut cv, Rect::from_center_size(c, vec2(d * 1.35, d * 0.66))),
                Amp::Svt => blue_plate(&mut cv, Rect::from_center_size(c, vec2(d * 1.05, d * 0.44))),
                Amp::B15 => blue_plate(&mut cv, Rect::from_center_size(c, vec2(d * 1.25, d * 0.44))),
                _ => {}
            },
            _ => {}
        }
    }
    // the front below the panel
    let gq = g.grille;
    match a {
        Amp::Twin => {
            grille(&mut cv, gq, 2.5, Cloth::SilverTurq, g.k);
            if !full {
                for x in [c.min.x + w * 0.01, c.max.x - w * 0.075] {
                    cv.chrome(
                        Rect::from_min_size(pos2(x, c.max.y - b0 * 0.9), vec2(w * 0.065, b0 * 0.9)),
                        1.5,
                    );
                }
            }
        }
        Amp::Tweed => {
            grille(
                &mut cv,
                gq,
                (gq.height() * 0.08).min(10.0),
                Cloth::TweedGrille,
                g.k,
            );
            // the oxblood name plate over the grille's top
            let q = tweed_plate(gq);
            cv.shadow(q.translate(vec2(0.0, 1.5)), q.height() * 0.3, 2.0, 0.5);
            cv.rr_v(
                q,
                q.height() * 0.28,
                &[(0.0, 0x6e2229, 1.0), (0.6, 0x4c1519, 1.0), (1.0, 0x3c1013, 1.0)],
            );
            cv.rr_stroke(q.shrink(1.2), q.height() * 0.24, 0.9, 0xc9a96a, 1.0);
        }
        Amp::Ac30 => {
            let band_y = gq.min.y - (gq.min.y - g.band.max.y) * 0.16;
            cv.rr_v(
                Rect::from_min_max(
                    pos2(c.min.x + 3.0, band_y - 1.6),
                    pos2(c.max.x - 3.0, band_y + 1.6),
                ),
                1.0,
                &[(0.0, 0xf0d898, 1.0), (0.5, 0xc8a867, 1.0), (1.0, 0x7d6634, 1.0)],
            );
            cv.rr_stroke(gq.expand(1.5), 2.5, 2.0, 0xc8a867, 1.0);
            grille(&mut cv, gq, 2.0, Cloth::Diamond, g.k);
            // chrome corners on the top
            let s = (w * 0.06).clamp(6.0, 20.0);
            let tk = (s * 0.28).max(2.0);
            for (x, dir) in [(c.min.x, 1.0f32), (c.max.x, -1.0)] {
                let y = c.min.y;
                cv.poly(
                    &[
                        pos2(x, y),
                        pos2(x + dir * s, y),
                        pos2(x + dir * s, y + tk),
                        pos2(x + dir * tk, y + tk),
                        pos2(x + dir * tk, y + s),
                        pos2(x, y + s),
                    ],
                    0xd7dadd,
                    1.0,
                );
            }
        }
        Amp::Plexi | Amp::Jcm => {
            let pip = if a == Amp::Plexi { 0xb8973f } else { 0xedede8 };
            cv.rr_stroke(gq.expand(2.0), 2.5, 2.0, pip, 1.0);
            grille(
                &mut cv,
                gq,
                2.0,
                if a == Amp::Plexi {
                    Cloth::SaltPepper
                } else {
                    Cloth::BlackWeave
                },
                g.k,
            );
            if a == Amp::Jcm {
                corners_plastic(&mut cv, c, (w.min(h) * 0.1).clamp(6.0, 18.0));
            }
        }
        Amp::Mesa => {
            let tq = g.band;
            let k = g.k * 0.8;
            cv.pattern(tq, 2.0, move |x, y| {
                let d = cloth(Cloth::Tread, x, y, k);
                let sheen = 0.3 * (1.0 - (((x + y * 0.5) / tq.width() - 0.4) / 0.18).abs()).max(0.0);
                let s = 0.1 * (1.0 - y / tq.height()) - 0.18 * (y / tq.height());
                [d[0] + sheen + s, d[1] + sheen + s, d[2] + sheen + s, 1.0]
            });
            cv.rr_stroke(tq, 2.0, 1.0, 0x3c4045, 1.0);
            // the chrome vent across the middle and the badge on it
            let v = mesa_vent(tq);
            cv.chrome(v, 2.0);
            let inner = v.shrink(v.height() * 0.12);
            let ih = inner.height();
            cv.pattern(inner, 0.0, |x, y| {
                let m = x.rem_euclid(3.5);
                let d = rgbf(if m < 2.0 {
                    0x141517
                } else if m < 2.8 {
                    0xd5d9dc
                } else {
                    0x7d8288
                });
                let s = 0.35 * (1.0 - y / ih * 3.0).max(0.0);
                [d[0] * (1.0 - s), d[1] * (1.0 - s), d[2] * (1.0 - s), 1.0]
            });
            oval_badge(&mut cv, mesa_badge(tq));
            corners_metal(&mut cv, c, (w.min(h) * 0.09).clamp(6.0, 16.0), false);
        }
        Amp::Svt => {
            cv.rr_stroke(gq.expand(2.0), 2.5, 2.0, 0x8d9297, 1.0);
            grille(&mut cv, gq, 2.0, Cloth::Sparkle, g.k);
            blue_plate(&mut cv, svt_plate(gq));
            corners_metal(&mut cv, c, (w.min(h) * 0.085).clamp(6.0, 16.0), true);
        }
        Amp::B15 => {
            cv.rr_stroke(gq.expand(2.0), 2.5, 2.0, 0xc3c8cc, 1.0);
            grille(&mut cv, gq, 2.0, Cloth::SilverSparkle, g.k);
            blue_plate(&mut cv, svt_plate(gq));
        }
        Amp::Gk => {
            grille(&mut cv, gq, 2.0, Cloth::BlackWeave, g.k);
            corners_metal(&mut cv, c, (w.min(h) * 0.085).clamp(6.0, 16.0), false);
        }
        Amp::Orange => {
            cv.rr_stroke(gq.expand(1.5), 2.5, 1.6, 0x161616, 0.9);
            grille(&mut cv, gq, 2.0, Cloth::SaltPepper, g.k);
            corners_plastic(&mut cv, c, (w.min(h) * 0.085).clamp(6.0, 16.0));
        }
        Amp::Tang => {
            cv.rr_stroke(gq.expand(1.5), 2.5, 1.6, 0x161616, 0.9);
            grille(&mut cv, gq, 2.0, Cloth::BasketTan, g.k);
            corners_plastic(&mut cv, c, (w.min(h) * 0.085).clamp(6.0, 16.0));
        }
        Amp::Mark => {
            cv.rr_stroke(gq.expand(2.0), 2.5, 2.0, 0x0a0a0a, 1.0);
            grille(&mut cv, gq, 2.0, Cloth::Cane, g.k);
            corners_metal(&mut cv, c, (w.min(h) * 0.085).clamp(6.0, 16.0), false);
        }
        Amp::Slo => {
            cv.rr_stroke(gq.expand(2.0), 2.5, 2.0, 0xc9cdd1, 1.0);
            grille(&mut cv, gq, 2.0, Cloth::BlackWeave, g.k);
            corners_metal(&mut cv, c, (w.min(h) * 0.085).clamp(6.0, 16.0), true);
        }
        Amp::Hiwatt => {
            cv.rr_stroke(gq.expand(1.5), 2.5, 1.4, 0x8d9297, 0.9);
            grille(&mut cv, gq, 2.0, Cloth::BlackWeave, g.k);
            // the name plate across the grille's middle
            let q = hiwatt_plate(gq);
            cv.shadow(q.translate(vec2(0.0, 1.5)), 2.0, 2.0, 0.5);
            cv.rr_v(q, 2.0, &[(0.0, 0x26272a, 1.0), (1.0, 0x0b0b0c, 1.0)]);
            cv.rr_stroke(q, 2.0, 1.0, 0xe6e6e2, 0.9);
        }
    }
    cv.image()
}

fn tweed_plate(gq: Rect) -> Rect {
    Rect::from_center_size(
        pos2(gq.center().x, gq.min.y + gq.height() * 0.07),
        vec2(gq.width() * 0.5, (gq.height() * 0.2).clamp(10.0, 34.0)),
    )
}

fn hiwatt_plate(gq: Rect) -> Rect {
    Rect::from_center_size(
        gq.center(),
        vec2(gq.width() * 0.46, (gq.height() * 0.3).clamp(10.0, 40.0)),
    )
}

fn mesa_vent(t: Rect) -> Rect {
    Rect::from_center_size(t.center(), vec2(t.width() * 0.52, t.height() * 0.44))
}

fn mesa_badge(t: Rect) -> Rect {
    let v = mesa_vent(t);
    Rect::from_center_size(v.center(), vec2(v.width() * 0.66, v.height() * 0.95))
}

fn svt_plate(gq: Rect) -> Rect {
    Rect::from_min_size(
        gq.min + vec2(gq.width() * 0.05, gq.height() * 0.14),
        vec2(gq.width() * 0.34, (gq.height() * 0.26).clamp(10.0, 40.0)),
    )
}

fn oval_badge(cv: &mut Cv, q: Rect) {
    let (cx, cy) = (q.center().x, q.center().y);
    let (rx, ry) = (q.width() * 0.5, q.height() * 0.5);
    let ell = |k: f32| -> Vec<Pos2> {
        (0..48)
            .map(|i| {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                pos2(cx + (rx - k) * a.cos(), cy + (ry - k) * a.sin())
            })
            .collect()
    };
    let sh: Vec<Pos2> = ell(0.0).iter().map(|&p| p + vec2(0.0, 1.5)).collect();
    cv.poly(&sh, 0x000000, 0.55);
    cv.poly(&ell(0.0), 0x2b2e31, 1.0);
    cv.poly(&ell(0.8), 0x9aa0a6, 1.0);
    let top: Vec<Pos2> = ell(0.8).into_iter().filter(|p| p.y <= cy + ry * 0.1).collect();
    cv.poly(&top, 0xe9ecee, 1.0);
    cv.poly_stroke(&ell(ry * 0.16), 1.3, 0x111111, 1.0);
}

fn blue_plate(cv: &mut Cv, q: Rect) {
    cv.shadow(q.translate(vec2(0.0, 1.5)), 3.0, 2.0, 0.5);
    cv.rr(q.expand(2.0), 4.0, 0x6d7378, 1.0);
    cv.rr(q.expand(1.2), 3.5, 0xcfd3d6, 1.0);
    cv.rr_v(q, 3.0, &[(0.0, 0x1d1e20, 1.0), (1.0, 0x050505, 1.0)]);
}

pub(super) fn corners_plastic(cv: &mut Cv, c: Rect, s: f32) {
    for (x, y) in [
        (c.min.x, c.min.y),
        (c.max.x - s, c.min.y),
        (c.min.x, c.max.y - s),
        (c.max.x - s, c.max.y - s),
    ] {
        let q = Rect::from_min_size(pos2(x, y), vec2(s, s));
        cv.rr(q.translate(vec2(0.0, 1.0)), s * 0.35, 0x000000, 0.6);
        cv.rr_grad(
            q,
            s * 0.35,
            q.min,
            q.max,
            &[(0.0, 0x3a3a3a, 1.0), (0.45, 0x141414, 1.0), (1.0, 0x020202, 1.0)],
        );
        cv.rr(
            Rect::from_min_size(q.min + vec2(s * 0.12, s * 0.12), vec2(s * 0.76, s * 0.22)),
            s * 0.1,
            0xffffff,
            0.08,
        );
    }
}

pub(super) fn corners_metal(cv: &mut Cv, c: Rect, s: f32, chrome: bool) {
    for (x, y) in [
        (c.min.x, c.min.y),
        (c.max.x - s, c.min.y),
        (c.min.x, c.max.y - s),
        (c.max.x - s, c.max.y - s),
    ] {
        let q = Rect::from_min_size(pos2(x, y), vec2(s, s));
        cv.rr(q.translate(vec2(0.0, 1.0)), s * 0.25, 0x000000, 0.6);
        if chrome {
            cv.rr_grad(
                q,
                s * 0.25,
                q.min,
                q.max,
                &[
                    (0.0, 0xfafbfc, 1.0),
                    (0.45, 0xaeb3b9, 1.0),
                    (0.55, 0xeef0f2, 1.0),
                    (1.0, 0x5b6066, 1.0),
                ],
            );
        } else {
            cv.rr_grad(
                q,
                s * 0.25,
                q.min,
                q.max,
                &[(0.0, 0x4a4d51, 1.0), (0.55, 0x1a1b1d, 1.0), (1.0, 0x0a0a0b, 1.0)],
            );
        }
        let sc = pos2(
            if x < c.center().x {
                x + s * 0.35
            } else {
                x + s * 0.65
            },
            if y < c.center().y {
                y + s * 0.35
            } else {
                y + s * 0.65
            },
        );
        cv.ball(sc, (s * 0.12).max(1.0), &[(0.0, 0xffffff), (1.0, 0x777777)]);
    }
}

/// A knob's printed scale on the panel: numbers where the knob is big enough, else ticks or dots.
fn scale(p: &Painter, a: Amp, c: Pos2, d: f32, kd: &KnobDef, ink: Color32, room: f32) {
    let r = d * 0.5;
    let big = d >= 28.0 && room >= d * 1.75;
    let fs = (d * 0.16).clamp(5.0, 10.0);
    let num = |s: &str, pos: Pos2| {
        text::draw(p, s, pos, Align2::CENTER_CENTER, Style::new(fs, 700, ink), None);
    };
    match a {
        Amp::Twin => {
            // the panel's fixed marker over the knob
            let tip = c - vec2(0.0, r + 2.0);
            let s = (d * 0.06).max(1.8);
            p.add(Shape::convex_polygon(
                vec![tip - vec2(s, s * 1.4), tip + vec2(s, -s * 1.4), tip],
                ink,
                Stroke::NONE,
            ));
        }
        Amp::Tweed => {
            for i in 0..12 {
                let ang = -150.0 + 300.0 * i as f32 / 11.0;
                p.line_segment([at(c, r * 1.1, ang), at(c, r * 1.24, ang)], Stroke::new(1.0, ink));
                if big {
                    num(&(i + 1).to_string(), at(c, r * 1.48, ang));
                }
            }
        }
        Amp::Ac30 | Amp::Mesa | Amp::Mark => {
            for i in 0..11 {
                let ang = -150.0 + 300.0 * i as f32 / 10.0;
                p.circle_filled(at(c, r * 1.22, ang), (d * 0.028).clamp(0.8, 1.7), ink);
            }
        }
        Amp::Plexi | Amp::Jcm => {
            for i in 0..11 {
                let ang = -150.0 + 300.0 * i as f32 / 10.0;
                p.line_segment([at(c, r * 1.1, ang), at(c, r * 1.24, ang)], Stroke::new(0.9, ink));
                if big && i % 2 == 0 {
                    num(&i.to_string(), at(c, r * 1.48, ang));
                }
            }
        }
        Amp::Svt | Amp::B15 | Amp::Gk | Amp::Orange | Amp::Tang | Amp::Slo | Amp::Hiwatt => {
            if a == Amp::Svt && matches!(kd.unit, Unit::Hz) {
                // the frequency selector's stops
                for (i, s) in ["220", "450", "800", "1.6k", "3k"].iter().enumerate() {
                    let ang = -120.0 + 60.0 * i as f32;
                    p.circle_filled(at(c, r * 1.12, ang), (d * 0.035).max(1.0), ink);
                    if big {
                        num(s, at(c, r * 1.52, ang));
                    }
                }
            } else {
                for i in 0..10 {
                    let ang = -150.0 + 300.0 * i as f32 / 9.0;
                    p.line_segment([at(c, r * 1.1, ang), at(c, r * 1.24, ang)], Stroke::new(0.9, ink));
                    if big && (i == 0 || i == 9 || i % 2 == 1) {
                        num(&(i + 1).to_string(), at(c, r * 1.48, ang));
                    }
                }
            }
        }
    }
}

/// What's drawn live over the body: knobs, switches, the jewel, labels, the logos.
pub(super) fn controls(p: &Painter, r: Rect, b: &Block, l: &Layout, detail: Detail, hot: Hot) {
    let def = b.def();
    let a = amp_of(def.id);
    let g = geo(a, r, detail);
    let dr = dress(a);
    let th = t();
    let ink = c32(dr.ink);
    let (its, d) = place(a, &g, detail);
    let ls = label_size(d);
    // (the switches' names only where the panel shows everything)
    let full = detail == Detail::Full && its.len() == items(a).len();
    let ly = |c: Pos2, dd: f32| {
        if dr.above {
            c.y - dd * 0.5 - if dd >= 30.0 { ls * 1.9 } else { ls * 1.0 }
        } else {
            c.y + dd * 0.5 + if dd >= 30.0 { ls * 1.3 } else { ls * 0.9 }
        }
    };
    let room = if its.len() > 1 {
        let n = its.len();
        (its[n - 1].1.x - its[0].1.x) / (n - 1) as f32
    } else {
        d * 1.4
    };
    for &(it, c) in &its {
        match it {
            It::K(i) => {
                let Some(&(_, kc, kd_)) = l.knobs.iter().find(|k| k.0 == i) else {
                    continue;
                };
                let kd = &def.knobs[i];
                let u = kd.to_u(b.v[i]);
                scale(p, a, kc, kd_, kd, ink, room);
                let kind = if a == Amp::Svt && matches!(kd.unit, Unit::Hz) {
                    Knob::BlackChicken
                } else {
                    dr.knob
                };
                knob(p, kc, kd_, u, kind);
                let pos = pos2(kc.x, ly(kc, kd_));
                if hot.active == Some(i) || hot.hover == Some(i) {
                    p.circle_stroke(kc, kd_ * 0.5 + 3.0, Stroke::new(1.6, th.accent));
                    text::draw(
                        p,
                        &kd.text(b.v[i]),
                        pos,
                        Align2::CENTER_CENTER,
                        Style::new(ls + 1.5, 800, ink),
                        None,
                    );
                } else {
                    lab(p, kd.label, pos, Align2::CENTER_CENTER, ls, ink, room, dr.letters);
                }
            }
            It::Tog(i) => {
                let Some(&(_, q)) = l.choices.iter().find(|k| k.0 == i) else {
                    continue;
                };
                let on = b.v[i] >= 0.5;
                // a slide switch (the blackface's bright)
                let plate = Rect::from_center_size(q.center(), vec2(q.width() * 0.7, q.height() * 0.9));
                p.rect_filled(plate, 2.0, c32(0x050505));
                p.rect_stroke(
                    plate,
                    2.0,
                    Stroke::new(0.8, c32(0x2c2c2c)),
                    egui::StrokeKind::Outside,
                );
                let nub = Rect::from_min_size(
                    pos2(
                        plate.min.x + 1.5,
                        if on {
                            plate.min.y + 1.5
                        } else {
                            plate.center().y + 0.5
                        },
                    ),
                    vec2(plate.width() - 3.0, plate.height() * 0.5 - 2.0),
                );
                p.rect_filled(nub, 1.5, c32(0x3a3a3a));
                p.line_segment(
                    [nub.left_top() + vec2(1.0, 1.0), nub.right_top() + vec2(-1.0, 1.0)],
                    Stroke::new(0.8, ca(0xffffff, 0.25)),
                );
                lab(
                    p,
                    "ON",
                    pos2(q.center().x, plate.min.y - ls * 0.8),
                    Align2::CENTER_CENTER,
                    ls * 0.8,
                    ink,
                    room,
                    dr.letters,
                );
                // (its name under it when the names are over the controls: "ON" is over it)
                let name_y = if dr.above {
                    plate.max.y + ls * 0.9
                } else {
                    ly(c, d)
                };
                lab(
                    p,
                    def.knobs[i].label,
                    pos2(q.center().x, name_y),
                    Align2::CENTER_CENTER,
                    ls,
                    ink,
                    room * 1.2,
                    dr.letters,
                );
                if hot.hover == Some(i) {
                    p.rect_stroke(
                        plate.expand(2.5),
                        3.0,
                        Stroke::new(1.5, th.accent),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            It::Jewel => match a {
                Amp::Jcm => {
                    rocker(p, c, d * 0.5, d * 0.72, Some(0xc0191f), b.on);
                    if full {
                        lab(
                            p,
                            "MAINS",
                            pos2(c.x, ly(c, d)),
                            Align2::CENTER_CENTER,
                            ls * 0.8,
                            ink,
                            room,
                            dr.letters,
                        );
                    }
                }
                _ => jewel(p, c, d * if a == Amp::Twin { 0.72 } else { 0.62 }, dr.jewel, b.on),
            },
            It::Standby => {
                let q = l.switch;
                match a {
                    a if a == Amp::Jcm || a.head() => {
                        rocker(p, q.center(), d * 0.5, d * 0.72, None, false);
                        if !b.on {
                            // a black rocker shows its state by which half is pressed: darken the top when off
                            let f =
                                Rect::from_center_size(q.center(), vec2(d * 0.5, d * 0.72)).shrink(d * 0.06);
                            p.rect_filled(
                                f.split_top_bottom_at_fraction(0.5).0,
                                1.5,
                                Color32::from_black_alpha(150),
                            );
                        }
                    }
                    _ => bat(p, q.center() + vec2(0.0, d * 0.08), d * 0.42, b.on),
                }
                if full {
                    lab(
                        p,
                        "STANDBY",
                        pos2(q.center().x, ly(c, d)),
                        Align2::CENTER_CENTER,
                        ls * 0.8,
                        ink,
                        room,
                        dr.letters,
                    );
                }
                if hot.switch {
                    p.rect_stroke(
                        q.expand(2.0),
                        3.0,
                        Stroke::new(1.5, th.accent),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            It::Power => {
                match a {
                    a if a.head() => rocker(p, c, d * 0.5, d * 0.72, Some(0xc8231d), b.on),
                    _ => bat(p, c + vec2(0.0, d * 0.08), d * 0.42, true),
                }
                lab(
                    p,
                    "POWER",
                    pos2(c.x, ly(c, d)),
                    Align2::CENTER_CENTER,
                    ls * 0.8,
                    ink,
                    room,
                    dr.letters,
                );
            }
            It::Jacks(_) => lab(
                p,
                "INPUT",
                pos2(c.x, ly(c, d)),
                Align2::CENTER_CENTER,
                ls * 0.8,
                ink,
                room * 1.2,
                dr.letters,
            ),
            It::Fuse => lab(
                p,
                "FUSE",
                pos2(c.x, ly(c, d)),
                Align2::CENTER_CENTER,
                ls * 0.8,
                ink,
                room,
                dr.letters,
            ),
            It::Mode(i) => {
                // the Rectifier's three-way mode switch (P41.2): the lever where the knob is, its position lit
                let pos = b.v[i].round().clamp(0.0, 2.0) as i32;
                let lever = c + vec2(-d * 0.2, 0.0);
                toggle3(p, lever, d * 0.42, pos - 1);
                let x = c.x + d * 0.04;
                for (k, s) in ["RAW", "VINTAGE", "MODERN"].iter().enumerate() {
                    lab(
                        p,
                        s,
                        pos2(x, c.y + (k as f32 - 1.0) * ls * 1.15),
                        Align2::LEFT_CENTER,
                        ls * 0.72,
                        if k as i32 == pos {
                            Color32::WHITE
                        } else {
                            c32(0x8a8a8a)
                        },
                        d,
                        dr.letters,
                    );
                }
                if full {
                    lab(
                        p,
                        def.knobs[i].label,
                        pos2(c.x, ly(c, d)),
                        Align2::CENTER_CENTER,
                        ls,
                        ink,
                        room,
                        dr.letters,
                    );
                }
                if hot.hover == Some(i) {
                    p.rect_stroke(
                        mode_rect(c, d),
                        3.0,
                        Stroke::new(1.5, th.accent),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            It::Geq(i0) => {
                // the Mark's graphic EQ: a cap on each slot, the frequency under it, the centre line
                for (k, tr) in geq_tracks(c, d, it.weight()).into_iter().enumerate() {
                    let i = i0 + k;
                    let Some(kd) = def.knobs.get(i) else {
                        continue;
                    };
                    let u = kd.to_u(b.v[i]);
                    p.line_segment(
                        [
                            pos2(tr.min.x - 1.5, tr.center().y),
                            pos2(tr.max.x + 1.5, tr.center().y),
                        ],
                        Stroke::new(0.8, ca(dr.ink, 0.6)),
                    );
                    let cy = tr.max.y - u * tr.height();
                    let cap = Rect::from_center_size(
                        pos2(tr.center().x, cy),
                        vec2(tr.width(), (d * 0.16).max(4.0)),
                    );
                    p.rect_filled(cap.translate(vec2(0.6, 1.0)), 1.0, Color32::from_black_alpha(120));
                    p.rect_filled(cap, 1.0, c32(0xc9cdd1));
                    p.line_segment(
                        [pos2(cap.min.x + 1.0, cy), pos2(cap.max.x - 1.0, cy)],
                        Stroke::new(1.0, c32(0x2a2a2a)),
                    );
                    let hot_here = hot.active == Some(i) || hot.hover == Some(i);
                    let pos = pos2(tr.center().x, tr.max.y + ls * 0.95);
                    if hot_here {
                        p.rect_stroke(
                            tr.expand(2.5),
                            2.0,
                            Stroke::new(1.4, th.accent),
                            egui::StrokeKind::Outside,
                        );
                        text::draw(
                            p,
                            &kd.text(b.v[i]),
                            pos,
                            Align2::CENTER_CENTER,
                            Style::new(ls, 800, ink),
                            None,
                        );
                    } else {
                        lab(
                            p,
                            kd.label,
                            pos,
                            Align2::CENTER_CENTER,
                            ls * 0.8,
                            ink,
                            tr.width() * 3.0,
                            dr.letters,
                        );
                    }
                }
            }
            It::Logo(wt) => {
                let mw = wt * room * 0.95;
                match a {
                    Amp::Twin => logo(
                        p,
                        def.name,
                        c,
                        Align2::CENTER_CENTER,
                        d * 0.55,
                        c32(0xf2f0ea),
                        mw,
                        Lettering::Script,
                        false,
                    ),
                    Amp::Tweed => logo(
                        p,
                        def.name,
                        c,
                        Align2::CENTER_CENTER,
                        d * 0.42,
                        c32(0xf0e6cc),
                        d * 1.6,
                        Lettering::Script,
                        false,
                    ),
                    Amp::Ac30 => {
                        lab(
                            p,
                            def.name,
                            c - vec2(0.0, d * 0.12),
                            Align2::CENTER_CENTER,
                            d * 0.34,
                            ink,
                            mw,
                            Lettering::Engraved,
                        );
                        lab(
                            p,
                            "THIRTY WATT  CLASS A",
                            c + vec2(0.0, d * 0.22),
                            Align2::CENTER_CENTER,
                            ls * 0.75,
                            ink,
                            mw,
                            Lettering::Print,
                        );
                    }
                    Amp::Plexi => {
                        lab(
                            p,
                            "PERSPEX",
                            c - vec2(0.0, d * 0.16),
                            Align2::CENTER_CENTER,
                            d * 0.3,
                            ink,
                            mw,
                            Lettering::Heavy,
                        );
                        p.line_segment(
                            [c + vec2(-mw * 0.35, 0.0), c + vec2(mw * 0.35, 0.0)],
                            Stroke::new(1.2, ink),
                        );
                        lab(
                            p,
                            "100 WATT",
                            c + vec2(0.0, d * 0.14),
                            Align2::CENTER_CENTER,
                            ls * 0.8,
                            ink,
                            mw,
                            Lettering::Condensed,
                        );
                    }
                    Amp::Jcm => {
                        lab(
                            p,
                            def.name,
                            c - vec2(0.0, d * 0.1),
                            Align2::CENTER_CENTER,
                            d * 0.32,
                            c32(0xf4f2ec),
                            d * 1.45,
                            Lettering::Condensed,
                        );
                        lab(
                            p,
                            "MASTER VOLUME 100 W",
                            c + vec2(0.0, d * 0.2),
                            Align2::CENTER_CENTER,
                            ls * 0.62,
                            c32(0xd0d0cc),
                            d * 1.4,
                            Lettering::Condensed,
                        );
                    }
                    Amp::Mesa => {
                        lab(
                            p,
                            "PETALUMA",
                            c - vec2(0.0, d * 0.05),
                            Align2::CENTER_CENTER,
                            d * 0.22,
                            c32(0x111111),
                            d * 1.0,
                            Lettering::Condensed,
                        );
                        lab(
                            p,
                            "PLATE",
                            c + vec2(0.0, d * 0.16),
                            Align2::CENTER_CENTER,
                            d * 0.11,
                            c32(0x111111),
                            d,
                            Lettering::Condensed,
                        );
                    }
                    Amp::Svt => logo(
                        p,
                        "blue line 300",
                        c,
                        Align2::CENTER_CENTER,
                        d * 0.22,
                        c32(0xe8ebee),
                        d * 0.92,
                        Lettering::Rounded,
                        false,
                    ),
                    Amp::B15 => logo(
                        p,
                        "portable 15",
                        c,
                        Align2::CENTER_CENTER,
                        d * 0.24,
                        c32(0xe8ebee),
                        d * 1.1,
                        Lettering::Rounded,
                        false,
                    ),
                    Amp::Gk => logo(
                        p,
                        "SILICON 800",
                        c,
                        Align2::CENTER_CENTER,
                        d * 0.26,
                        c32(0xeeeeee),
                        d * 1.0,
                        Lettering::Heavy,
                        false,
                    ),
                    Amp::Orange => logo(
                        p,
                        "CITRUS",
                        c,
                        Align2::CENTER_CENTER,
                        d * 0.34,
                        c32(0xd8661a),
                        d * 1.2,
                        Lettering::Heavy,
                        false,
                    ),
                    Amp::Tang => logo(
                        p,
                        "TANGERINE",
                        c,
                        Align2::CENTER_CENTER,
                        d * 0.3,
                        c32(0xd8661a),
                        mw,
                        Lettering::Heavy,
                        false,
                    ),
                    Amp::Slo => {
                        lab(
                            p,
                            "SNAKESKIN",
                            c - vec2(0.0, d * 0.1),
                            Align2::CENTER_CENTER,
                            d * 0.26,
                            c32(0x141414),
                            mw,
                            Lettering::Heavy,
                        );
                        lab(
                            p,
                            "SUPER LEAD OVERDRIVE 100",
                            c + vec2(0.0, d * 0.18),
                            Align2::CENTER_CENTER,
                            ls * 0.6,
                            c32(0x141414),
                            mw,
                            Lettering::Condensed,
                        );
                    }
                    Amp::Mark | Amp::Hiwatt => {}
                }
            }
            It::Gap(_) => {}
        }
    }
    if a == Amp::Ac30 && detail == Detail::Full {
        // the groups' titles, on the copper over their boxes
        for (grp, name) in [([0usize, 2], "BRILLIANT"), ([3, 4], "MASTER")] {
            let xs: Vec<f32> = its
                .iter()
                .filter_map(|&(it, c)| match it {
                    It::K(i) if i >= grp[0] && i <= grp[1] => Some(c.x),
                    _ => None,
                })
                .collect();
            if let (Some(lo), Some(hi)) = (xs.first(), xs.last()) {
                let pos = pos2((lo + hi) * 0.5, g.panel.min.y + g.panel.height() * 0.16);
                let fs = ls * 0.8;
                let tw = text::width(p.ctx(), name, Style::caps(fs, ink)) + 6.0;
                p.rect_filled(
                    Rect::from_center_size(pos, vec2(tw, fs * 1.3)),
                    0.0,
                    c32(0xc5844a),
                );
                lab(p, name, pos, Align2::CENTER_CENTER, fs, ink, tw, Lettering::Print);
            }
        }
    }
    // the name on the front
    let gq = g.grille;
    match a {
        Amp::Twin => {
            let sz = (gq.height() * 0.27).clamp(10.0, 52.0);
            logo(
                p,
                "Gemini",
                gq.min + vec2(gq.width() * 0.05, gq.height() * 0.04),
                Align2::LEFT_TOP,
                sz,
                c32(0xe9ecef),
                gq.width() * 0.55,
                Lettering::Script,
                false,
            );
        }
        Amp::Tweed => {
            let q = tweed_plate(gq);
            logo(
                p,
                def.name,
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.8,
                c32(0xf0e6cc),
                q.width() * 0.88,
                Lettering::Script,
                false,
            );
        }
        Amp::Ac30 => {
            let top = g.band.max.y;
            let y = (top + gq.min.y) * 0.5 - 1.0;
            let sz = ((gq.min.y - top) * 0.64).clamp(8.0, 44.0);
            logo(
                p,
                def.name,
                pos2(gq.center().x, y),
                Align2::CENTER_CENTER,
                sz,
                c32(0xd9b866),
                gq.width() * 0.9,
                Lettering::Engraved,
                true,
            );
        }
        Amp::Plexi | Amp::Jcm => {
            let sz = (gq.height() * 0.44).clamp(8.0, 44.0);
            let pos = gq.center();
            for (off, cl) in [(2.0, 0x111111), (1.4, 0x8d8a83), (0.7, 0xbdbab2)] {
                lab(
                    p,
                    def.name,
                    pos + vec2(0.0, off * sz / 20.0),
                    Align2::CENTER_CENTER,
                    sz,
                    c32(cl),
                    gq.width() * 0.85,
                    Lettering::Heavy,
                );
            }
            lab(
                p,
                def.name,
                pos,
                Align2::CENTER_CENTER,
                sz,
                c32(0xf4f2ec),
                gq.width() * 0.85,
                Lettering::Heavy,
            );
        }
        Amp::Mesa => {
            let v = mesa_badge(g.band);
            lab(
                p,
                def.name,
                v.center(),
                Align2::CENTER_CENTER,
                v.height() * 0.48,
                c32(0x111111),
                v.width() * 0.76,
                Lettering::Condensed,
            );
        }
        Amp::B15 => {
            let q = svt_plate(gq);
            logo(
                p,
                "portable 15",
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.62,
                c32(0xe8ebee),
                q.width() * 0.9,
                Lettering::Rounded,
                false,
            );
        }
        Amp::Gk => {
            logo(
                p,
                "SILICON 800",
                gq.min + vec2(gq.width() * 0.05, gq.height() * 0.12),
                Align2::LEFT_TOP,
                (gq.height() * 0.2).clamp(7.0, 30.0),
                c32(0xeeeeee),
                gq.width() * 0.5,
                Lettering::Heavy,
                true,
            );
        }
        Amp::Orange => {
            logo(
                p,
                "CITRUS 200",
                gq.center(),
                Align2::CENTER_CENTER,
                (gq.height() * 0.3).clamp(8.0, 40.0),
                c32(0xd8661a),
                gq.width() * 0.7,
                Lettering::Heavy,
                true,
            );
        }
        Amp::Tang => {
            logo(
                p,
                "TANGERINE",
                gq.center(),
                Align2::CENTER_CENTER,
                (gq.height() * 0.3).clamp(8.0, 40.0),
                c32(0xd8661a),
                gq.width() * 0.7,
                Lettering::Heavy,
                true,
            );
        }
        Amp::Mark => {
            // the script logo on the cane, at the left
            logo(
                p,
                "Lagunitas",
                gq.min + vec2(gq.width() * 0.05, gq.height() * 0.1),
                Align2::LEFT_TOP,
                (gq.height() * 0.32).clamp(9.0, 44.0),
                c32(0x161616),
                gq.width() * 0.55,
                Lettering::Script,
                false,
            );
        }
        Amp::Slo => {
            logo(
                p,
                def.name,
                gq.center(),
                Align2::CENTER_CENTER,
                (gq.height() * 0.28).clamp(8.0, 36.0),
                c32(0xd6dadd),
                gq.width() * 0.75,
                Lettering::Condensed,
                true,
            );
        }
        Amp::Hiwatt => {
            let q = hiwatt_plate(gq);
            logo(
                p,
                "Surrey",
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.7,
                c32(0xf2f2ee),
                q.width() * 0.9,
                Lettering::Script,
                false,
            );
        }
        Amp::Svt => {
            let q = svt_plate(gq);
            logo(
                p,
                "blue line 300",
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.62,
                c32(0xe8ebee),
                q.width() * 0.9,
                Lettering::Rounded,
                false,
            );
        }
    }
    if detail == Detail::Board {
        let pass = p.ctx().cumulative_pass_nr();
        SEEN.with(|s| *s.borrow_mut() = (pass, r.max.x, Some(a)));
    }
}

pub(super) fn board_size(b: &Block) -> Vec2 {
    match amp_of(b.def().id) {
        Amp::Twin => vec2(352.0, 200.0),
        Amp::Tweed => vec2(214.0, 190.0),
        Amp::Ac30 => vec2(312.0, 206.0),
        Amp::Plexi | Amp::Jcm => vec2(330.0, 128.0),
        Amp::Mesa => vec2(326.0, 132.0),
        Amp::Svt | Amp::B15 | Amp::Gk | Amp::Orange => vec2(300.0, 160.0),
        Amp::Slo | Amp::Tang | Amp::Hiwatt => vec2(320.0, 160.0),
        Amp::Mark => vec2(360.0, 170.0),
    }
}

#[cfg(test)]
mod tests {
    use qrs_audio::board::{Face, MODELS};

    use super::*;

    /// Every amp and cab control has a spot on the face the mouse can find (the editor's, the tile's and, for
    /// amps, the board's), inside the face.
    #[test]
    fn every_control_has_a_spot() {
        for (m, def) in MODELS.iter().enumerate() {
            if !matches!(def.face, Face::Amp | Face::Cab) {
                continue;
            }
            let b = Block::new(m);
            for (detail, size) in [
                (Detail::Full, vec2(390.0, 195.0)),
                (Detail::Full, vec2(310.0, 170.0)),
                (Detail::Board, super::super::board_size(&b)),
            ] {
                if def.face == Face::Cab && detail == Detail::Board {
                    continue;
                }
                let r = Rect::from_min_size(pos2(10.0, 20.0), size);
                let l = super::super::layout(&b, r, detail);
                // (on the board an added control or a late one may sit only on the open unit's ADDED plate beside
                // the face, P37.6c: every one has a spot in Full)
                for i in 0..def.front_len() {
                    let found = l.knobs.iter().any(|k| k.0 == i)
                        || l.choices.iter().any(|k| k.0 == i)
                        || l.faders.iter().any(|k| k.0 == i)
                        || l.extra.iter().any(|k| k.0 == i);
                    assert!(
                        found || detail == Detail::Board,
                        "{} knob {i} has no spot ({detail:?})",
                        def.id
                    );
                }
                // the back panel stays off the face (the editor's Back panel shows it)
                for (i, _) in def.back_knobs() {
                    let found = l.knobs.iter().any(|k| k.0 == i)
                        || l.choices.iter().any(|k| k.0 == i)
                        || l.faders.iter().any(|k| k.0 == i);
                    assert!(!found, "{} back knob {i} on the face ({detail:?})", def.id);
                }
                for &(_, c, d) in &l.knobs {
                    assert!(r.contains(c) && d >= 8.0, "{}: a knob outside or tiny", def.id);
                }
                if def.face == Face::Amp {
                    assert!(r.contains_rect(l.switch), "{}: standby off the face", def.id);
                }
            }
        }
    }
}
