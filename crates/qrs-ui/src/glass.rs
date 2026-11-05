//! Glass keys (P34.5, D88; the owner's pick A2 Crown of the research folder's `mockups/ui-main/tiles/glasskey/`
//! round 2): on the dark themes a key is a 5 px glass plate laid on the panel's rubber, its label engraved in the
//! rubber under it (a dark wall up-left, a lit one down-right, the ink in the cut, a faint inner reflection), and the
//! shared room (`crate::hall`) reflected in its face and polished arris by where the key sits in the window. Daylight
//! and the classic look keep the ivory keys (`keys::key_face`). New in quickRS; the pack is `assets/hall/` (its
//! `manifest.json`: `glass_r16`, `glass_disc`).
//!
//! Two looks, one sprite per state each (6x, straight alpha over the panel, a soft drop shadow baked in):
//! - `RECT`: a 232 x 46 plate with 16 point corners at (12, 12) of a 256 x 70 canvas, nine-sliced onto any key with
//!   30 point insets (a key under 36 points shrinks the whole recipe);
//! - `DISC`: a 48 point disc at (12, 12) of a 72 point canvas, scaled to the key (round keys, 32-64 points).
//!
//! The sprites are resampled once per scale so their edges land on the screen's pixels. The room is a mesh of rings
//! over the plate (`geom`): a feather ring outside (m 0), ten across the arris, each tilted outward by its angle in
//! the manifest's table, then flat face rings to the mid-line (a disc's to a fan at its centre). The mesh is cached
//! per plate size, its UVs and colours per position, so a still key costs a lookup a frame and only a scroll or a
//! resize redoes them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::epaint::Vertex;
use egui::{
    Align2, Color32, ColorImage, Context, Mesh, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
    TextureHandle, TextureId, TextureOptions, pos2, vec2,
};

use crate::hall::{self, Eye, Hall};
use crate::icons::{self, Icon};
use crate::paint::key_of;
use crate::text::{self, Style};
use crate::theme::{self, t};

macro_rules! png {
    ($name:literal) => {
        include_bytes!(concat!("../../../assets/hall/", $name, ".png")).as_slice()
    };
}

/// The panel colour the sprites were made over, and how far above a theme's base it sits (Midnight's base is
/// (17, 31, 43)).
#[cfg(test)]
const PANEL: [u8; 3] = [23, 38, 52];
const PANEL_LIFT: [i16; 3] = [6, 7, 9];

/// The room's gain on a key (at the owner's `hall::LEVEL` of it), and per state (hover's x1.18, pressed's x0.6).
const GAIN: f32 = 2.6;
const STATE_GAIN: [f32; 3] = [1.0, 1.18, 0.6];
/// The plate's lift in its sprites (points): hover 1 up, pressed 1.1 down; the room follows it.
const STATE_LIFT: [f32; 3] = [0.0, -1.0, 1.1];
const SIDE: f32 = 24.0;

/// One glass look: its sprites (in `State` order), canvas and the plate in it, corner radius, nine-slice inset (0:
/// the whole sprite scaled), the arris rings (inset, tilt in degrees, reflectance m; the first the feather), the
/// face's m and rings, corner segments (a disc's: a quarter of its segments); `k` its sprites' pixels a point.
pub(crate) struct Look {
    pub sprites: [&'static [u8]; 3],
    pub k: f32,
    pub canvas: [f32; 2],
    pub margin: f32,
    pub radius: f32,
    pub inset: f32,
    pub rings: &'static [(f32, f32, f32)],
    pub face_m: f32,
    pub face: &'static [f32],
    pub per_corner: usize,
    pub disc: bool,
}

impl Look {
    /// The plate size (points) from which it's drawn at full size.
    fn full(&self) -> f32 {
        if self.disc {
            self.canvas[1] - 2.0 * self.margin
        } else {
            2.0 * (self.inset - self.margin)
        }
    }
}

pub(crate) const RECT: Look = Look {
    sprites: [
        png!("glass_r16_normal"),
        png!("glass_r16_hover"),
        png!("glass_r16_pressed"),
    ],
    k: 6.0,
    canvas: [256.0, 70.0],
    margin: 12.0,
    radius: 16.0,
    inset: 30.0,
    rings: &[
        (-0.5, 0.0, 0.0),
        (0.0, 39.93, 0.03415),
        (0.2, 72.65, 0.059),
        (0.45, 64.34, 0.04557),
        (0.8, 55.23, 0.04868),
        (1.25, 45.97, 0.09347),
        (1.8, 36.38, 0.08587),
        (2.5, 25.72, 0.08261),
        (3.4, 13.23, 0.0815),
        (4.4, 2.53, 0.08156),
        (5.6, 0.0, 0.08172),
    ],
    face_m: 0.08165,
    face: &[7.0, 11.0, 16.0],
    per_corner: 10,
    disc: false,
};

pub(crate) const DISC: Look = Look {
    sprites: [
        png!("glass_disc_normal"),
        png!("glass_disc_hover"),
        png!("glass_disc_pressed"),
    ],
    k: 6.0,
    canvas: [72.0, 72.0],
    margin: 12.0,
    radius: 24.0,
    inset: 0.0,
    rings: &[
        (-0.5, 0.0, 0.0),
        (0.0, 47.99, 0.04142),
        (0.2, 69.79, 0.05468),
        (0.45, 59.89, 0.04278),
        (0.8, 49.55, 0.08596),
        (1.25, 38.94, 0.08702),
        (1.8, 27.82, 0.08302),
        (2.5, 15.11, 0.08189),
        (3.4, 2.38, 0.08178),
        (4.4, 0.0, 0.08162),
        (5.6, 0.0, 0.08159),
    ],
    face_m: 0.08139,
    face: &[8.0, 14.0, 19.0],
    per_corner: 16,
    disc: true,
};

/// The first look (radius 7, `glass_key` in the manifest): kept for the pack's reference vertices.
#[cfg(test)]
pub(crate) const R7: Look = Look {
    sprites: [png!("glass_normal"), png!("glass_normal"), png!("glass_normal")],
    k: 3.0,
    canvas: [256.0, 70.0],
    margin: 12.0,
    radius: 7.0,
    inset: 22.0,
    rings: &[
        (-0.5, 0.0, 0.0),
        (0.0, 35.95, 0.02868),
        (0.2, 64.57, 0.05117),
        (0.45, 60.04, 0.06236),
        (0.8, 50.29, 0.08234),
        (1.25, 39.54, 0.08631),
        (1.8, 28.26, 0.08343),
        (2.5, 15.52, 0.08192),
        (3.4, 3.01, 0.08167),
        (4.6, 0.0, 0.08158),
    ],
    face_m: 0.08178,
    face: &[7.0, 11.0, 16.0],
    per_corner: 6,
    disc: false,
};

/// The engraving (points): the dark wall up-left, the lit wall down-right, the inks; the ghost (the ink's inner
/// reflection, offset by the plate's thickness); the label under the glass at 94 %, pressed 8 % toward the panel.
const DARK: Color32 = Color32::from_rgb(8, 13, 18);
const LIGHT: Color32 = Color32::from_rgb(51, 80, 106);
const PASS: f32 = 0.6;
pub const INK_WHITE: Color32 = Color32::from_rgb(214, 220, 226);
pub const INK_AMBER: Color32 = Color32::from_rgb(246, 190, 84);
const GHOST_A: f32 = 0.05;
const GHOST_OFF: [f32; 2] = [1.1, 1.7];
const UNDER: f32 = 0.94;
const PRESSED_TO_PANEL: f32 = 0.08;

/// A key's state, as its sprites are made.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum State {
    Normal,
    Hover,
    Pressed,
}

impl State {
    pub fn of(hot: bool, down: bool) -> State {
        if down {
            State::Pressed
        } else if hot {
            State::Hover
        } else {
            State::Normal
        }
    }

    fn i(self) -> usize {
        self as usize
    }
}

/// Glass keys are the dark themes' (round 3 look); Daylight and the classic look keep ivory.
pub fn on() -> bool {
    !theme::classic() && t().dark
}

/// The panel colour under the keys in this theme.
fn panel() -> [u8; 3] {
    let b = t().base;
    let lift = |c: u8, i: usize| (c as i16 + PANEL_LIFT[i]).clamp(0, 255) as u8;
    [lift(b.r(), 0), lift(b.g(), 1), lift(b.b(), 2)]
}

/// The label's ink: white, or amber on the main action.
pub fn ink(amber: bool) -> Color32 {
    if amber { INK_AMBER } else { INK_WHITE }
}

/// A plate's look: a square one is a disc (round keys), the rest rounded rects.
fn look_of(plate: Rect) -> &'static Look {
    if (plate.width() - plate.height()).abs() < 0.5 {
        &DISC
    } else {
        &RECT
    }
}

/// How much of the recipe a plate this size gets (a disc: its size over the sprite's; a rect: 1 from 36 points up).
fn unit(look: &Look, plate: Rect) -> f32 {
    let s = plate.width().min(plate.height()) / look.full();
    if look.disc { s.max(0.2) } else { s.clamp(0.3, 1.0) }
}

// -- the plate ---------------------------------------------------------------------------------------------------

fn decode(bytes: &[u8]) -> image::RgbaImage {
    image::load_from_memory(bytes).expect("glass sprite").to_rgba8()
}

/// The sprites' textures, resampled for the scale: (look is a disc, ppp) -> the three states and their size.
#[derive(Default)]
struct Sprites {
    tex: HashMap<(bool, u32), (Vec<TextureHandle>, [u32; 2])>,
}

thread_local! {
    static SPRITE: RefCell<Sprites> = RefCell::new(Sprites::default());
}

/// A premultiplied image resampled to `w` x `h` (Catmull-Rom, its support scaled to the shrink so it filters like an
/// area average, the arris kept crisp), each colour kept within its alpha (the filter's small overshoot would
/// otherwise add light at the edges). dditive: the texels are light to add (alpha 0), kept as they come.
pub fn resample(pm: &image::RgbaImage, w: u32, h: u32, additive: bool) -> image::RgbaImage {
    let mut r = image::imageops::resize(pm, w, h, image::imageops::FilterType::CatmullRom);
    if additive {
        return r;
    }
    for px in r.pixels_mut() {
        let a = px[3];
        for c in 0..3 {
            px[c] = px[c].min(a);
        }
    }
    r
}

/// A look's textures at `ppp` pixels a point (premultiplied, then resampled down from the 6x sprite so the arris
/// lines stay whole; a disc at the size of a 64 point key).
fn textures(ctx: &Context, look: &Look) -> ([TextureId; 3], [u32; 2]) {
    let ppp = ctx.pixels_per_point();
    SPRITE.with(|s| {
        let mut s = s.borrow_mut();
        let (tex, size) = s.tex.entry((look.disc, ppp.to_bits())).or_insert_with(|| {
            let k = if look.disc { ppp * 64.0 / 48.0 } else { ppp };
            let (w, h) = (
                ((look.canvas[0] * k).round() as u32).max(8),
                ((look.canvas[1] * k).round() as u32).max(8),
            );
            let tex = look
                .sprites
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let mut pm = decode(b);
                    for px in pm.pixels_mut() {
                        let a = px[3] as u32;
                        for c in 0..3 {
                            px[c] = ((px[c] as u32 * a + 127) / 255) as u8;
                        }
                    }
                    let r = resample(&pm, w, h, false);
                    let img = ColorImage::from_rgba_premultiplied([w as usize, h as usize], r.as_raw());
                    ctx.load_texture(format!("glass-{}-{i}", look.disc), img, TextureOptions::LINEAR)
                })
                .collect();
            (tex, [w, h])
        });
        ([tex[0].id(), tex[1].id(), tex[2].id()], *size)
    })
}

/// The nine-slice: (destination, uv) quads for a canvas `dst` with `inset` points, from a sprite whose insets are
/// `uv_in` of its width and height. Empty pieces are left out.
pub(crate) fn nine(dst: Rect, inset: f32, uv_in: [f32; 2]) -> Vec<(Rect, Rect)> {
    let i = inset.min(dst.width() / 2.0).min(dst.height() / 2.0);
    let xs = [dst.min.x, dst.min.x + i, dst.max.x - i, dst.max.x];
    let ys = [dst.min.y, dst.min.y + i, dst.max.y - i, dst.max.y];
    let us = [0.0, uv_in[0], 1.0 - uv_in[0], 1.0];
    let vs = [0.0, uv_in[1], 1.0 - uv_in[1], 1.0];
    let mut out = Vec::with_capacity(9);
    for r in 0..3 {
        for c in 0..3 {
            let d = Rect::from_min_max(pos2(xs[c], ys[r]), pos2(xs[c + 1], ys[r + 1]));
            if d.width() > 1e-4 && d.height() > 1e-4 {
                out.push((
                    d,
                    Rect::from_min_max(pos2(us[c], vs[r]), pos2(us[c + 1], vs[r + 1])),
                ));
            }
        }
    }
    out
}

/// The glass plate over `plate` (the key's rect, points) in a state, at `alpha` of its strength (a faint plate
/// under a text key's hover). A square plate is a disc.
pub fn plate(p: &Painter, plate: Rect, state: State, alpha: f32) {
    if plate.width() < 2.0 || plate.height() < 2.0 || alpha <= 0.0 {
        return;
    }
    let look = look_of(plate);
    let (tex, size) = textures(p.ctx(), look);
    let g = unit(look, plate);
    let canvas = plate.expand(look.margin * g);
    let v = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    let tint = Color32::from_rgba_premultiplied(v, v, v, v);
    let mut mesh = Mesh::with_texture(tex[state.i()]);
    if look.disc {
        mesh.add_rect_with_uv(canvas, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
    } else {
        // the sprite's insets as fractions of its resampled size
        let uv_in = [
            (look.inset / look.canvas[0] * size[0] as f32).round() / size[0] as f32,
            (look.inset / look.canvas[1] * size[1] as f32).round() / size[1] as f32,
        ];
        for (d, uv) in nine(canvas, look.inset * g, uv_in) {
            mesh.add_rect_with_uv(d, uv, tint);
        }
    }
    p.add(Shape::mesh(mesh));
}

// -- the room in it ----------------------------------------------------------------------------------------------

/// A plate's reflection mesh in plate space (points, y down, the plate's top-left at 0): per vertex its position,
/// normal (screen frame), reflectance and the colour under it (sRGB 0..1 and linear); the triangles.
#[derive(Debug)]
pub(crate) struct Geom {
    pub pos: Vec<[f32; 2]>,
    pub n: Vec<[f32; 3]>,
    pub m: Vec<f32>,
    pub base: Vec<[f32; 3]>,
    pub lin_base: Vec<[f32; 3]>,
    pub idx: Vec<u32>,
}

/// Rounded-rect rings at `insets` round a `w` x `h` plate at (`x0`, `y0`) with corner radius `r`, the same number
/// of vertices each, each ring's vertex i on ring 0's normal line; with each vertex's outward direction. The
/// outline runs from the bottom-right corner's start (y down: angle 0 is +x) through the bottom, left, top and
/// right; `per_corner` segments a corner, the long sides cut every `SIDE` points (rt.py's `rrect_rings`). A disc
/// (`w = h = 2r`) has no sides.
pub(crate) fn rings(
    (x0, y0, w, h): (f32, f32, f32, f32),
    r: f32,
    insets: &[f32],
    per_corner: usize,
    side: f32,
) -> (Vec<Vec<[f32; 2]>>, Vec<[f32; 2]>) {
    let disc = (w - h).abs() < 1e-3 && (w / 2.0 - r).abs() < 1e-3;
    let per_side = if disc {
        [1, 1]
    } else {
        [((w / side) as usize).max(2), ((h / side) as usize).max(1)]
    };
    let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
    let (hw, hh) = (w / 2.0 - r, h / 2.0 - r);
    let corners = [
        (hw, hh, 0.0f32),
        (-hw, hh, 90.0),
        (-hw, -hh, 180.0),
        (hw, -hh, 270.0),
    ];
    // the core: a point on the inner rect and the outward direction there
    let mut core: Vec<([f32; 2], [f32; 2])> = Vec::new();
    for (ci, &(ax, ay, a0)) in corners.iter().enumerate() {
        // (a disc's corners meet: each ends where the next begins)
        let last = if disc { per_corner - 1 } else { per_corner };
        for i in 0..=last {
            let a = (a0 + 90.0 * i as f32 / per_corner as f32).to_radians();
            core.push(([cx + ax, cy + ay], [a.cos(), a.sin()]));
        }
        let (bx, by, _) = corners[(ci + 1) % 4];
        let nside = per_side[ci % 2];
        let a = (a0 + 90.0).to_radians();
        for j in 1..nside {
            let t = j as f32 / nside as f32;
            core.push((
                [cx + ax + (bx - ax) * t, cy + ay + (by - ay) * t],
                [a.cos(), a.sin()],
            ));
        }
    }
    let out = insets
        .iter()
        .map(|&d| {
            let rr = (r - d).max(0.0);
            let (sx, sy) = if d > r {
                let f = |half: f32| {
                    if half > 1e-6 {
                        (half - (d - r)).max(0.0) / half
                    } else {
                        0.0
                    }
                };
                (f(hw), f(hh))
            } else {
                (1.0, 1.0)
            };
            core.iter()
                .map(|(c, o)| {
                    let (x, y) = (c[0] + o[0] * rr, c[1] + o[1] * rr);
                    [cx + (x - cx) * sx, cy + (y - cy) * sy]
                })
                .collect()
        })
        .collect();
    (out, core.iter().map(|(_, o)| *o).collect())
}

/// Quads between consecutive rings (n vertices each), and a fan from the last ring to vertex `centre`.
fn ring_tris(nrings: usize, n: usize, centre: Option<u32>) -> Vec<u32> {
    let mut t = Vec::with_capacity(nrings * n * 6);
    for k in 0..nrings.saturating_sub(1) {
        let (a, b) = ((k * n) as u32, ((k + 1) * n) as u32);
        for i in 0..n as u32 {
            let j = (i + 1) % n as u32;
            t.extend_from_slice(&[a + i, a + j, b + j, a + i, b + j, b + i]);
        }
    }
    if let Some(c) = centre {
        let a = ((nrings - 1) * n) as u32;
        for i in 0..n as u32 {
            t.extend_from_slice(&[a + i, a + (i + 1) % n as u32, c]);
        }
    }
    t
}

/// The CPU sprite (straight alpha, `k` pixels a point) over the panel at its 1x canvas point (sx, sy), bilinear at
/// texel centres.
fn sprite_under(im: &image::RgbaImage, k: f32, sx: f32, sy: f32, panel: [u8; 3]) -> [f32; 3] {
    let (w, h) = (im.width() as i64, im.height() as i64);
    let (x, y) = (sx * k - 0.5, sy * k - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |xx: i64, yy: i64| {
        let p = im.get_pixel(xx.clamp(0, w - 1) as u32, yy.clamp(0, h - 1) as u32);
        let a = p[3] as f32 / 255.0;
        std::array::from_fn::<f32, 3, _>(|c| (p[c] as f32 * a + panel[c] as f32 * (1.0 - a)) / 255.0)
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    std::array::from_fn(|i| {
        (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy
    })
}

/// A canvas coordinate (points, along a side `len` long with destination inset `inset`) to the sprite's 1x
/// coordinate (along `src_len`, its inset `src_in`): the nine-slice's own mapping.
pub(crate) fn slice_src(x: f32, len: f32, inset: f32, src_in: f32, src_len: f32) -> f32 {
    let i = inset.min(len / 2.0);
    if i <= 0.0 {
        return x / len.max(1e-6) * src_len;
    }
    if x <= i {
        x * src_in / i
    } else if x >= len - i {
        src_len - (len - x) * src_in / i
    } else {
        src_in + (x - i) / (len - 2.0 * i) * (src_len - 2.0 * src_in)
    }
}

/// The reflection mesh of a `w` x `h` plate of `look` at unit `g` over `panel`, with the colour under each vertex
/// sampled from the normal sprite (through the nine-slice, or the disc's scale).
pub(crate) fn geom(look: &Look, w: f32, h: f32, g: f32, panel: [u8; 3], sprite: &image::RgbaImage) -> Geom {
    let r = if look.disc { w / 2.0 } else { look.radius * g };
    let mut insets: Vec<f32> = look.rings.iter().map(|x| x.0 * g).collect();
    let mut tilt: Vec<f32> = look.rings.iter().map(|x| x.1).collect();
    let mut ms: Vec<f32> = look.rings.iter().map(|x| x.2).collect();
    for d in look.face.iter().map(|d| d * g).filter(|&d| d < h / 2.0 - 2.0) {
        insets.push(d);
        tilt.push(0.0);
        ms.push(look.face_m);
    }
    if !look.disc {
        insets.push(h / 2.0);
        tilt.push(0.0);
        ms.push(look.face_m);
    }
    let (rs, dirs) = rings((0.0, 0.0, w, h), r, &insets, look.per_corner, SIDE);
    let n = dirs.len();
    let cm = look.margin * g;
    let (cw, ch) = (w + 2.0 * cm, h + 2.0 * cm);
    let src = |v: [f32; 2]| {
        if look.disc {
            [(v[0] + cm) / g, (v[1] + cm) / g]
        } else {
            [
                slice_src(v[0] + cm, cw, look.inset * g, look.inset, look.canvas[0]),
                slice_src(v[1] + cm, ch, look.inset * g, look.inset, look.canvas[1]),
            ]
        }
    };
    let total = n * rs.len() + usize::from(look.disc);
    let centre = look.disc.then_some((n * rs.len()) as u32);
    let mut out = Geom {
        pos: Vec::with_capacity(total),
        n: Vec::with_capacity(total),
        m: Vec::with_capacity(total),
        base: Vec::with_capacity(total),
        lin_base: Vec::with_capacity(total),
        idx: ring_tris(rs.len(), n, centre),
    };
    let mut push = |v: [f32; 2], nrm: [f32; 3], m: f32| {
        let s = src(v);
        let b = sprite_under(sprite, look.k, s[0], s[1], panel);
        out.pos.push(v);
        out.n.push(nrm);
        out.m.push(m);
        out.lin_base.push(b.map(hall::lin));
        out.base.push(b);
    };
    for (k, ring) in rs.iter().enumerate() {
        let (s, c) = tilt[k].to_radians().sin_cos();
        for (v, o) in ring.iter().zip(&dirs) {
            // sprite y is down, the screen frame's up
            push(*v, [o[0] * s, -o[1] * s, c], ms[k]);
        }
    }
    if look.disc {
        push([w / 2.0, h / 2.0], [0.0, 0.0, 1.0], look.face_m);
    }
    out
}

/// The vertex colours and UVs of a plate's mesh at `at` (its top-left, points) under `eye`, at gain `gain`.
pub(crate) fn shade(
    gm: &Geom,
    at: Pos2,
    eye: &Eye,
    gain: f32,
    radiance: impl Fn([f32; 2]) -> [f32; 3],
) -> Vec<Vertex> {
    gm.pos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let pos = pos2(at.x + p[0], at.y + p[1]);
            let uv = eye.uv(pos, gm.n[i]);
            let k = gain * gm.m[i];
            let color = if k > 0.0 {
                hall::additive(gm.base[i], gm.lin_base[i], k, radiance(uv))
            } else {
                Color32::TRANSPARENT
            };
            Vertex {
                pos,
                uv: pos2(uv[0], uv[1]),
                color,
            }
        })
        .collect()
}

/// Meshes kept from frame to frame: plate geometry by size, and shaded meshes by place (a scroll or a resize makes
/// new ones; the old ones go after a while unused).
#[derive(Default)]
struct Cache {
    geoms: HashMap<u64, Arc<Geom>>,
    meshes: HashMap<u64, (Vec<Arc<Mesh>>, u64)>,
    /// The looks' normal sprites (rect, disc) on the CPU.
    normal: [Option<image::RgbaImage>; 2],
    pass: u64,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The room reflected in a glass plate over `plate` in a state (draw it over the plate and its label).
pub fn room(p: &Painter, plate: Rect, state: State) {
    if plate.width() < 2.0 || plate.height() < 2.0 {
        return;
    }
    let ctx = p.ctx();
    let hall = Hall::get(ctx);
    let eye = Eye::of(ctx);
    let look = look_of(plate);
    let g = unit(look, plate);
    let panel = panel();
    let at = plate.min + vec2(0.0, STATE_LIFT[state.i()] * g.min(1.0));
    let size = (plate.width().to_bits(), plate.height().to_bits());
    let gkey = key_of((size, g.to_bits(), panel, look.disc));
    let mkey = key_of((
        gkey,
        at.x.to_bits(),
        at.y.to_bits(),
        eye.key(),
        eye.origin.x.to_bits(),
        state,
        ctx.pixels_per_point().to_bits(),
    ));
    let pass = ctx.cumulative_pass_nr();
    let meshes = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.pass != pass {
            c.pass = pass;
            if c.meshes.len() > 400 {
                c.meshes.retain(|_, (_, used)| pass.saturating_sub(*used) < 30);
            }
            if c.geoms.len() > 300 {
                c.geoms.clear();
            }
        }
        if let Some((m, used)) = c.meshes.get_mut(&mkey) {
            *used = pass;
            return m.clone();
        }
        if !c.geoms.contains_key(&gkey) {
            let normal = c.normal[usize::from(look.disc)].get_or_insert_with(|| decode(look.sprites[0]));
            let gm = geom(look, plate.width(), plate.height(), g, panel, normal);
            c.geoms.insert(gkey, Arc::new(gm));
        }
        let gm = c.geoms[&gkey].clone();
        let gain = GAIN * hall::LEVEL * STATE_GAIN[state.i()];
        let verts = shade(&gm, at, &eye, gain, |uv| hall.radiance(uv));
        // (on the room's pyramid: the arris minifies it, P34.7)
        let meshes = hall.mipped(hall::Tex::Probe, verts, &gm.idx, ctx.pixels_per_point());
        c.meshes.insert(mkey, (meshes.clone(), pass));
        meshes
    });
    p.extend(meshes.into_iter().map(Shape::Mesh));
}

/// A whole glass key's body: the plate, then (no label) the room.
pub fn key(p: &Painter, plate_r: Rect, state: State) {
    plate(p, plate_r, state, 1.0);
    room(p, plate_r, state);
}

// -- the engraving -----------------------------------------------------------------------------------------------

/// An engraving pass's offset in whole pixels and the strength it's drawn at (a 0.6 point step is a pixel at 0.6).
fn step(off: f32, ppp: f32) -> (f32, f32) {
    let px = off * ppp;
    if px.abs() < 1e-3 {
        return (0.0, 1.0);
    }
    let whole = px.round().abs().max(1.0).copysign(px);
    (whole / ppp, (px / whole).clamp(0.0, 1.0))
}

/// The ink under the glass in a state.
fn under(ink: Color32, state: State) -> Color32 {
    let c = if state == State::Pressed {
        let pn = panel();
        let k = PRESSED_TO_PANEL;
        let f = |a: u8, b: u8| (a as f32 * (1.0 - k) + b as f32 * k).round() as u8;
        Color32::from_rgb(f(ink.r(), pn[0]), f(ink.g(), pn[1]), f(ink.b(), pn[2]))
    } else {
        ink
    };
    c.gamma_multiply(UNDER)
}

/// The passes of an engraving at `pos`: (offset, colour) for the ghost, the dark wall, the lit wall and the ink.
fn passes(ppp: f32, ink: Color32, state: State) -> [(egui::Vec2, Color32); 4] {
    let (dg, ag) = (step(GHOST_OFF[0], ppp), step(GHOST_OFF[1], ppp));
    let (d, ad) = step(-PASS, ppp);
    let (l, al) = step(PASS, ppp);
    [
        (vec2(dg.0, ag.0), ink.gamma_multiply(GHOST_A)),
        (vec2(d, d), DARK.gamma_multiply(ad)),
        (vec2(l, l), LIGHT.gamma_multiply(al)),
        (egui::Vec2::ZERO, under(ink, state)),
    ]
}

/// A label engraved under the glass (its anchor at `pos`, cut to `max_w`); `st.color` is the ink. Returns the ink's
/// box.
pub fn label(
    p: &Painter,
    label: &str,
    pos: Pos2,
    anchor: Align2,
    st: Style,
    max_w: Option<f32>,
    state: State,
) -> Rect {
    let txt = match max_w {
        Some(m) => text::fit(p.ctx(), label, st, m),
        None => label.to_string(),
    };
    let mut r = Rect::NOTHING;
    for (off, c) in passes(p.pixels_per_point(), st.color, state) {
        r = text::draw(p, &txt, pos + off, anchor, Style { color: c, ..st }, None);
    }
    r
}

/// An icon engraved under the glass.
pub fn icon(p: &Painter, ic: Icon, center: Pos2, size: f32, ink: Color32, state: State) -> Rect {
    let mut r = Rect::NOTHING;
    for (off, c) in passes(p.pixels_per_point(), ink, state) {
        r = icons::icon(p, ic, center + off, size, c);
    }
    r
}

/// A key cap on glass: a thin outline in the ink, the key's name in it (`r` the cap's box).
pub fn cap(p: &Painter, r: Rect, ink: Color32) {
    let rad = (r.height() / 2.0).min(6.0);
    p.rect_stroke(
        r.shrink(0.5),
        rad,
        Stroke::new(1.0, ink.gamma_multiply(0.55)),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERTS: &str = include_str!("../../../assets/hall/ref/ref_key_verts.json");
    const MANIFEST: &str = include_str!("../../../assets/hall/manifest.json");

    fn f(v: &serde_json::Value) -> f32 {
        v.as_f64().expect("number") as f32
    }

    #[test]
    fn the_tables_are_the_manifests() {
        let m: serde_json::Value = serde_json::from_str(MANIFEST).expect("manifest");
        for (name, look) in [("glass_key", &R7), ("glass_r16", &RECT), ("glass_disc", &DISC)] {
            let gk = &m[name];
            let rings = gk["reflection_mesh"]["rings"].as_array().expect("rings");
            assert_eq!(rings.len(), look.rings.len(), "{name}");
            for (r, &(d, t, mm)) in rings.iter().zip(look.rings) {
                assert_eq!(
                    (f(&r["inset_px"]), f(&r["tilt_deg"]), f(&r["m"])),
                    (d, t, mm),
                    "{name}"
                );
            }
            assert_eq!(f(&gk["reflection_mesh"]["face_m"]), look.face_m, "{name}");
            let face: Vec<f32> = gk["reflection_mesh"]["face_rings_inset_px"]
                .as_array()
                .expect("face rings")
                .iter()
                .filter_map(|v| v.as_f64().map(|x| x as f32))
                .collect();
            assert_eq!(face, look.face, "{name}");
            assert_eq!(f(&gk["plate_at"][0]), look.margin, "{name}");
            assert_eq!(
                [f(&gk["canvas_app_px"][0]), f(&gk["canvas_app_px"][1])],
                look.canvas,
                "{name}"
            );
            if !look.disc {
                assert_eq!(f(&gk["corner_radius"]), look.radius, "{name}");
                assert_eq!(f(&gk["nine_slice_insets_app_px"]["l"]), look.inset, "{name}");
            } else {
                assert!((f(&gk["corner_radius"]) - look.radius).abs() < 0.05);
                assert_eq!(
                    gk["reflection_mesh"]["segments"].as_u64(),
                    Some(4 * look.per_corner as u64)
                );
            }
        }
        assert_eq!(
            m["glass_r16"]["reflection_mesh"]["per_corner_segments"].as_u64(),
            Some(10)
        );
        assert_eq!(f(&m["gains"]["glass_key"]), GAIN);
        assert_eq!(f(&m["gains"]["glass_key_state_factor"]["hover"]), STATE_GAIN[1]);
        assert_eq!(f(&m["gains"]["glass_key_state_factor"]["pressed"]), STATE_GAIN[2]);
        assert_eq!(f(&m["room"]["probe"]["E_T"]), hall::E_T);
        let ink = &m["glass_key"]["label"]["passes"][2]["rgb_amber_main_action"];
        assert_eq!([f(&ink[0]), f(&ink[1]), f(&ink[2])], [246.0, 190.0, 84.0]);
    }

    #[test]
    fn every_sprite_loads_at_its_size() {
        for look in [&RECT, &DISC, &R7] {
            for b in look.sprites {
                let im = decode(b);
                let want = (look.canvas[0] * look.k) as u32;
                assert_eq!(
                    (im.width(), im.height()),
                    (want, (look.canvas[1] * look.k) as u32)
                );
            }
        }
    }

    /// The pack's reference key: a blank 232 x 46 plate (radius 7) on a 256 x 70 canvas centred at two points of a
    /// 1440 x 900 window. Our mesh has its vertices, normals, reflectances, colours under them, triangles, UVs and
    /// colours.
    #[test]
    fn the_reflection_matches_the_reference_key() {
        let refs: serde_json::Value = serde_json::from_str(VERTS).expect("ref_key_verts.json");
        let sprite = decode(R7.sprites[0]);
        let gm = geom(&R7, 232.0, 46.0, 1.0, PANEL, &sprite);
        let blur = hall::blur_copy();
        let eye = Eye::new(1440.0, 900.0);
        for key in refs.as_array().expect("keys") {
            let verts = key["verts"].as_array().expect("verts");
            assert_eq!(verts.len(), gm.pos.len());
            let tris: Vec<u32> = key["tris"]
                .as_array()
                .expect("tris")
                .iter()
                .flat_map(|t| {
                    t.as_array()
                        .expect("tri")
                        .iter()
                        .map(|i| i.as_u64().expect("i") as u32)
                })
                .collect();
            assert_eq!(tris, gm.idx);
            let tl = &key["sprite_top_left"];
            // the plate's top-left on screen: the canvas's plus the margin
            let at = pos2(f(&tl[0]) + R7.margin, f(&tl[1]) + R7.margin);
            let shaded = shade(&gm, at, &eye, GAIN, |uv| hall::radiance_of(&blur, uv));
            let (mut worst_c, mut worst_uv, mut worst_b) = (0i32, 0f32, 0f32);
            for (i, v) in verts.iter().enumerate() {
                let (x, y) = (f(&v["x"]) - R7.margin, f(&v["y"]) - R7.margin);
                assert!(
                    (gm.pos[i][0] - x).abs() < 2e-3 && (gm.pos[i][1] - y).abs() < 2e-3,
                    "pos {i}"
                );
                for c in 0..3 {
                    assert!((gm.n[i][c] - f(&v["n"][c])).abs() < 2e-4, "n {i}");
                    worst_b = worst_b.max((gm.base[i][c] - f(&v["base"][c])).abs());
                }
                assert!((gm.m[i] - f(&v["m"])).abs() < 1e-6, "m {i}");
                let uv = shaded[i].uv;
                worst_uv = worst_uv
                    .max((uv.x - f(&v["u"])).abs())
                    .max((uv.y - f(&v["v"])).abs());
                let col = shaded[i].color;
                assert_eq!(col.a(), 0, "additive");
                for (c, got) in [col.r(), col.g(), col.b()].into_iter().enumerate() {
                    let want = v["col"][c].as_i64().expect("col") as i32;
                    worst_c = worst_c.max((got as i32 - want).abs());
                }
            }
            println!(
                "reference key at {}: uv within {worst_uv:.1e}, base {worst_b:.4}, colour {worst_c} levels",
                key["screen_centre"]
            );
            // (the JPEG decoders differ by a level or two on the blurred room, and the reference was made on the
            // first pack's blurred room, q92 4:2:0; P34.7's is q95 4:4:4: a level more)
            assert!(worst_uv < 1e-4, "uv off by {worst_uv}");
            assert!(worst_b < 2.0 / 255.0, "base off by {worst_b}");
            assert!(worst_c <= 3, "colour off by {worst_c} levels");
        }
    }

    #[test]
    fn the_colour_formula_on_its_own() {
        // no reflectance adds nothing; more light under a darker base adds more; never past white
        let base = [0.09f32, 0.15, 0.2];
        let lb = base.map(hall::lin);
        assert_eq!(hall::additive(base, lb, 0.0, [0.1; 3]), Color32::TRANSPARENT);
        let a = hall::additive(base, lb, 0.2, [0.05; 3]);
        let b = hall::additive(base, lb, 0.2, [0.2; 3]);
        assert!(a.r() > 0 && a.a() == 0);
        // the slope: the same light adds less (per unit) where it's brighter
        assert!((b.r() as f32) < 4.0 * a.r() as f32 + 2.0);
        let w = hall::additive([1.0; 3], [1.0; 3], 6.0, [1.0; 3]);
        assert_eq!(w.a(), 0);
        // the mix alpha is capped at half, at the owner's 0.3 of the pack's strength
        assert_eq!(hall::mix(1.0).a(), 38);
        assert_eq!(hall::mix(0.1), Color32::from_rgba_premultiplied(14, 14, 14, 14));
    }

    #[test]
    fn the_nine_slice_keeps_its_corners() {
        let d = Rect::from_min_size(pos2(10.0, 20.0), vec2(300.0, 76.0));
        let q = nine(d, 22.0, [22.0 / 256.0, 22.0 / 70.0]);
        assert_eq!(q.len(), 9);
        // the corners keep their size, the middle stretches, the pieces tile the canvas
        assert_eq!(q[0].0, Rect::from_min_size(pos2(10.0, 20.0), vec2(22.0, 22.0)));
        assert_eq!(q[8].0.max, d.max);
        assert!((q[4].0.width() - 256.0).abs() < 1e-4 && (q[4].0.height() - 32.0).abs() < 1e-4);
        let area: f32 = q.iter().map(|(r, _)| r.area()).sum();
        assert!((area - d.area()).abs() < 1e-2);
        assert_eq!(q[4].1.min, pos2(22.0 / 256.0, 22.0 / 70.0));
        // a canvas just twice the inset: no middle row
        let thin = nine(
            Rect::from_min_size(Pos2::ZERO, vec2(100.0, 44.0)),
            22.0,
            [0.1, 0.3],
        );
        assert_eq!(thin.len(), 6);
        // the mapping back: identity at the sprite's own size, the ends kept on a stretched key
        assert!((slice_src(100.0, 256.0, 30.0, 30.0, 256.0) - 100.0).abs() < 1e-4);
        assert!((slice_src(5.0, 400.0, 30.0, 30.0, 256.0) - 5.0).abs() < 1e-4);
        assert!((slice_src(395.0, 400.0, 30.0, 30.0, 256.0) - 251.0).abs() < 1e-4);
        assert!((slice_src(200.0, 400.0, 30.0, 30.0, 256.0) - 128.0).abs() < 1e-3);
        // a small key's inset shrinks with it, mapping onto the sprite's whole corner
        assert!((slice_src(15.0, 100.0, 15.0, 30.0, 256.0) - 30.0).abs() < 1e-4);
    }

    /// What a key's room costs on the CPU (`cargo test --release -p qrs-ui glass_cost -- --ignored --nocapture`):
    /// its mesh once per size, its UVs and colours once per place.
    #[test]
    #[ignore]
    fn glass_cost() {
        let sprite = decode(RECT.sprites[0]);
        let blur = hall::blur_copy();
        let eye = Eye::new(1400.0, 875.0);
        let n = 200;
        let t = std::time::Instant::now();
        let mut verts = 0;
        for i in 0..n {
            verts = geom(&RECT, 180.0 + i as f32 * 0.01, 48.0, 1.0, PANEL, &sprite)
                .pos
                .len();
        }
        let per_geom = t.elapsed().as_secs_f64() / n as f64;
        let gm = geom(&RECT, 180.0, 48.0, 1.0, PANEL, &sprite);
        let t = std::time::Instant::now();
        for i in 0..n {
            let v = shade(&gm, pos2(300.0, 100.0 + i as f32), &eye, GAIN, |uv| {
                hall::radiance_of(&blur, uv)
            });
            std::hint::black_box(v);
        }
        let per_shade = t.elapsed().as_secs_f64() / n as f64;
        println!(
            "glass key 180x48: {verts} vertices; mesh {:.1} us once per size, shading {:.1} us per move ({:.3} us a vertex)",
            per_geom * 1e6,
            per_shade * 1e6,
            per_shade * 1e6 / verts as f64
        );
    }

    #[test]
    fn small_keys_keep_their_rings() {
        // a 24 point pill: the face rings that would cross the mid-line are left out, every ring the same size
        let sprite = decode(RECT.sprites[0]);
        let g = unit(&RECT, Rect::from_min_size(Pos2::ZERO, vec2(80.0, 24.0)));
        assert!((g - 24.0 / 36.0).abs() < 1e-5);
        let gm = geom(&RECT, 80.0, 24.0, g, PANEL, &sprite);
        let n = gm.pos.len();
        // 11 arris rings, the 7 point face ring scaled, the mid-line; 4 x 11 corner vertices, 2 more on each long side
        let per_ring = 4 * 11 + 2 * 2;
        assert_eq!(n % per_ring, 0, "{n}");
        assert_eq!(gm.idx.len() % 6, 0);
        assert!(gm.idx.iter().all(|&i| (i as usize) < n));
    }

    #[test]
    fn a_round_key_is_a_disc() {
        let r = Rect::from_min_size(Pos2::ZERO, vec2(44.0, 44.0));
        assert!(look_of(r).disc && !look_of(r.expand2(vec2(10.0, 0.0))).disc);
        let sprite = decode(DISC.sprites[0]);
        let g = unit(&DISC, r);
        let gm = geom(&DISC, 44.0, 44.0, g, PANEL, &sprite);
        // 11 arris rings and 3 face rings of 64, the centre
        assert_eq!(gm.pos.len(), 14 * 64 + 1);
        assert_eq!(gm.idx.len(), (13 * 64 * 2 + 64) * 3);
        // the outer real ring on the circle, its normal tilted outward; the centre faces the viewer
        let c = 22.0;
        let p = gm.pos[64];
        assert!(((p[0] - c).hypot(p[1] - c) - 22.0).abs() < 1e-3);
        assert!(gm.n[64][0] > 0.4 && gm.n[gm.pos.len() - 1] == [0.0, 0.0, 1.0]);
        // under the face the sprite is the plate (not the panel's rubber beyond it)
        let mid = gm.pos.len() - 1;
        assert!(gm.base[mid].iter().all(|&v| v > 0.0 && v < 0.6));
    }
}
