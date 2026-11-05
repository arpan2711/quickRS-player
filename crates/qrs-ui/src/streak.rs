//! The streak on the score tile (P37.7, new in quickRS: the Python app has none): while you're on a streak the
//! accuracy ring's lit arc comes alive, a lava-lamp texture flowing along it with a glow inside the band, hotter
//! at each tier; the STREAK count heats from its own colour through orange to red-hot; when the streak breaks it
//! cools down in a moment instead of popping off. Nothing is drawn outside the lit band (`knobs::ring_band`).
//!
//! The pattern is procedural: a domain-warped, tileable value-noise field baked once per scale into two small
//! white-with-alpha textures (sharp lava blobs, a soft flowing field), wrapped (`Repeat`) and scrolled along the
//! arc by its UVs. Each frame builds a few small meshes (one per layer), tinted per theme; additive layers (glow,
//! hot core, sparks) use premultiplied colours with alpha 0. Tiers and their reasoning: docs/DECISIONS.md D113.

use std::cell::RefCell;
use std::f32::consts::TAU;

use egui::{Color32, ColorImage, Mesh, Painter, Pos2, Shape, TextureHandle, TextureId, TextureOptions, pos2};

use crate::knobs::RingBand;

/// Streak counts where each tier starts (D113): the multiplier's first two steps (x2 at 10, x3 at 20), then the
/// rhythm games' milestones (50, 100, 200, ...), topping out at a flawless dense song (1000).
pub const TIERS: [u32; 7] = [10, 20, 50, 100, 200, 500, 1000];
/// The top tier.
pub const MAX_TIER: usize = TIERS.len();
/// How long the arc takes to cool when the streak breaks (s), whatever tier it was at.
pub const COOL_S: f64 = 0.7;
/// How fast the look climbs to a new tier (tiers per second): 0 to the first in 0.4 s.
pub const RISE: f32 = 2.5;

/// The tier of a streak: 0 below the first threshold, 1..=7 from there.
pub fn tier(streak: u32) -> usize {
    TIERS.iter().filter(|&&t| streak >= t).count()
}

/// The effect's state from frame to frame: the level shown (a tier, fractional while it climbs or cools), the
/// cool-down under way, and how far the pattern has flowed (points along the arc).
#[derive(Clone, Debug, Default)]
pub struct StreakFx {
    level: f32,
    /// (when it started, the level it started from, the level it cools to)
    cool: Option<(f64, f32, f32)>,
    phase: f64,
    last: Option<f64>,
    /// The streak seen last frame (a drop to a lower tier is a break).
    streak: u32,
}

impl StreakFx {
    /// The level shown now.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Whether anything is drawn (and so whether to keep repainting).
    pub fn active(&self) -> bool {
        self.level > 1e-3
    }

    /// Straight to the streak's tier, no climb (screenshots).
    pub fn snap(&mut self, streak: u32) {
        self.level = tier(streak) as f32;
        self.cool = None;
        self.streak = streak;
    }

    /// Advance to time `now` (seconds, any monotonic clock) with the current streak; returns the level.
    pub fn update(&mut self, now: f64, streak: u32) -> f32 {
        let dt = self.last.map_or(0.0, |l| (now - l).clamp(0.0, 0.1));
        self.last = Some(now);
        let target = tier(streak) as f32;
        let broke = streak < self.streak;
        self.streak = streak;
        if target < self.level - 1e-4 || (self.cool.is_some() && target <= self.level) {
            if broke || self.cool.is_none_or(|c| c.2 != target) {
                self.cool = Some((now, self.level, target));
            }
            let (t0, from, to) = self.cool.unwrap_or((now, self.level, target));
            self.level = cooled(from, to, now - t0);
            if now - t0 >= COOL_S {
                self.cool = None;
            }
        } else {
            self.cool = None;
            self.level = (self.level + RISE * dt as f32).min(target);
        }
        self.phase += look(self.level).speed as f64 * dt;
        self.level
    }

    pub fn phase(&self) -> f64 {
        self.phase
    }
}

/// The level `secs` into a cool-down from `from` to `to`: fast first, easing out, `to` at COOL_S.
pub fn cooled(from: f32, to: f32, secs: f64) -> f32 {
    let x = (secs / COOL_S).clamp(0.0, 1.0) as f32;
    to + (from - to) * (1.0 - x) * (1.0 - x)
}

/// What a level looks like; levels between tiers mix their neighbours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Flow along the arc, points per second.
    pub speed: f32,
    /// The darker, slower undercurrent (soft field).
    pub depth: f32,
    /// The lava blobs.
    pub blobs: f32,
    /// A second, finer and faster blob layer (from 100).
    pub rich: f32,
    /// Additive glow over the band (peaked in its middle).
    pub glow: f32,
    /// How much the glow breathes.
    pub pulse: f32,
    /// The hot core: an additive bright line flowing along the band's middle (from 50).
    pub core: f32,
    /// Fast sparks in the core (500 and 1000).
    pub sparks: f32,
    /// A wash over the whole band under the lava, from 100 on, so the top tiers read apart in a still: orange at
    /// 100, red-orange at 200, red at 500, deep red at 1000 (its colour from the level, `wash_color`; P37.6d, D145).
    pub wash: f32,
    /// How far the lava has gone from its own colour to a pale yellow-white (the dark themes), so it stays bright on
    /// the redder wash.
    pub hot: f32,
}

const LOOKS: [Look; MAX_TIER + 1] = [
    Look {
        speed: 10.0,
        depth: 0.0,
        blobs: 0.0,
        rich: 0.0,
        glow: 0.0,
        pulse: 0.0,
        core: 0.0,
        sparks: 0.0,
        wash: 0.0,
        hot: 0.0,
    },
    // 10: a faint flow
    Look {
        speed: 12.0,
        depth: 0.32,
        blobs: 0.55,
        rich: 0.0,
        glow: 0.0,
        pulse: 0.0,
        core: 0.0,
        sparks: 0.0,
        wash: 0.0,
        hot: 0.0,
    },
    // 20: the lava clear, a little faster
    Look {
        speed: 17.0,
        depth: 0.42,
        blobs: 0.70,
        rich: 0.0,
        glow: 0.06,
        pulse: 0.0,
        core: 0.0,
        sparks: 0.0,
        wash: 0.0,
        hot: 0.0,
    },
    // 50: it starts to glow and breathe, a hint of a core
    Look {
        speed: 24.0,
        depth: 0.48,
        blobs: 0.78,
        rich: 0.0,
        glow: 0.14,
        pulse: 0.25,
        core: 0.12,
        sparks: 0.0,
        wash: 0.0,
        hot: 0.0,
    },
    // 100: a second, finer current
    Look {
        speed: 32.0,
        depth: 0.52,
        blobs: 0.85,
        rich: 0.50,
        glow: 0.20,
        pulse: 0.25,
        core: 0.22,
        sparks: 0.0,
        wash: 0.3,
        hot: 0.0,
    },
    // 200: a hot core
    Look {
        speed: 42.0,
        depth: 0.56,
        blobs: 0.90,
        rich: 0.62,
        glow: 0.27,
        pulse: 0.3,
        core: 0.42,
        sparks: 0.0,
        wash: 0.58,
        hot: 0.35,
    },
    // 500: brighter, sparks in the core
    Look {
        speed: 54.0,
        depth: 0.60,
        blobs: 0.94,
        rich: 0.74,
        glow: 0.34,
        pulse: 0.3,
        core: 0.62,
        sparks: 0.45,
        wash: 0.74,
        hot: 0.7,
    },
    // 1000: white-hot
    Look {
        speed: 68.0,
        depth: 0.64,
        blobs: 1.0,
        rich: 0.85,
        glow: 0.42,
        pulse: 0.35,
        core: 0.85,
        sparks: 0.9,
        wash: 0.86,
        hot: 1.0,
    },
];

/// The look at a level (0..=MAX_TIER, fractional between tiers).
pub fn look(level: f32) -> Look {
    let l = level.clamp(0.0, MAX_TIER as f32);
    let i = (l.floor() as usize).min(MAX_TIER - 1);
    let k = l - i as f32;
    let (a, b) = (LOOKS[i], LOOKS[i + 1]);
    let m = |x: f32, y: f32| x + (y - x) * k;
    Look {
        speed: m(a.speed, b.speed),
        depth: m(a.depth, b.depth),
        blobs: m(a.blobs, b.blobs),
        rich: m(a.rich, b.rich),
        glow: m(a.glow, b.glow),
        pulse: m(a.pulse, b.pulse),
        core: m(a.core, b.core),
        sparks: m(a.sparks, b.sparks),
        wash: m(a.wash, b.wash),
        hot: m(a.hot, b.hot),
    }
}

// -- the STREAK count's heat -------------------------------------------------------------------------------------

/// The heat ramp from tier 1 to the top: dark themes go amber, orange, red-hot (bright); Daylight's stay dark
/// enough to read on its light card (brown-orange to deep red).
const HEAT_DARK: [[u8; 3]; MAX_TIER] = [
    [255, 214, 130],
    [255, 190, 84],
    [255, 160, 52],
    [255, 130, 38],
    [255, 100, 32],
    [255, 72, 34],
    [255, 48, 30],
];
const HEAT_LIGHT: [[u8; 3]; MAX_TIER] = [
    [140, 72, 16],
    [178, 86, 12],
    [206, 96, 10],
    [214, 80, 10],
    [206, 58, 10],
    [196, 38, 12],
    [186, 22, 16],
];

/// The STREAK word's or number's colour at a level: `base` (its normal colour) at 0, then through the ramp.
pub fn heat_color(level: f32, base: Color32, dark: bool) -> Color32 {
    let ramp = if dark { &HEAT_DARK } else { &HEAT_LIGHT };
    let l = level.clamp(0.0, MAX_TIER as f32);
    if l <= 0.0 {
        return base;
    }
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    let (a, b, k) = if l < 1.0 {
        (base, rgb(ramp[0]), l)
    } else {
        let i = (l.floor() as usize).min(MAX_TIER - 1);
        (rgb(ramp[i - 1]), rgb(ramp[i]), l - i as f32)
    };
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

// -- the textures --------------------------------------------------------------------------------------------------

/// The texture tile in points: along the arc, across it (the band is 8 pt of the 40).
const TILE_U: f32 = 160.0;
const TILE_V: f32 = 40.0;

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xff_ffff) as f32 / 16_777_216.0
}

/// Value noise with period (px, py) cells.
fn vnoise(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let q = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (sx, sy) = (q(fx), q(fy));
    let h = |i: i32, j: i32| hash(i.rem_euclid(px), j.rem_euclid(py), seed);
    let a = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * sx;
    let b = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * sx;
    a + (b - a) * sy
}

/// Fractal value noise, period (px, py) cells at the first octave, in [0, 1].
fn fbm(x: f32, y: f32, px: i32, py: i32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut norm, mut amp, mut f) = (0.0, 0.0, 1.0, 1.0);
    for o in 0..octaves {
        sum += amp
            * vnoise(
                x * f,
                y * f,
                px * f as i32,
                py * f as i32,
                seed.wrapping_add(o * 977),
            );
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// The lava field at (u, v) in [0, 1)^2, tileable: blobs stretched along u (4 cells along, 8 across the tile, so
/// about 40 x 5 pt), domain-warped so they bend and pinch like a lava lamp's.
fn lava(u: f32, v: f32, seed: u32) -> f32 {
    let (cu, cv) = (4, 8);
    let (x, y) = (u * cu as f32, v * cv as f32);
    let wx = fbm(x, y, cu, cv, seed ^ 0x51, 3);
    let wy = fbm(x, y, cu, cv, seed ^ 0xa7, 3);
    let f = fbm(x + 1.6 * (wx - 0.5), y + 2.4 * (wy - 0.5), cu, cv, seed, 4);
    ((f - 0.5) * 2.6 + 0.5).clamp(0.0, 1.0)
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The two masks, `w` x `w/4` texels: (sharp lava blobs, a soft flowing field), white with the mask as alpha.
fn bake(w: usize) -> (ColorImage, ColorImage) {
    let h = (w / 4).max(8);
    let mut sharp = Vec::with_capacity(w * h * 4);
    let mut soft = Vec::with_capacity(w * h * 4);
    for j in 0..h {
        for i in 0..w {
            let (u, v) = (i as f32 / w as f32, j as f32 / h as f32);
            let f = lava(u, v, 7);
            // a blob's body, a brighter skin round its edge (the lamp's light catching it)
            let body = smooth(0.56, 0.66, f);
            let a = (body * 0.82 + 0.18 * smooth(0.7, 0.85, f)).min(1.0);
            sharp.extend_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
            let g = lava(u, v, 31);
            soft.extend_from_slice(&[255, 255, 255, (smooth(0.25, 0.85, g) * 255.0).round() as u8]);
        }
    }
    (
        ColorImage::from_rgba_unmultiplied([w, h], &sharp),
        ColorImage::from_rgba_unmultiplied([w, h], &soft),
    )
}

struct Baked {
    w: usize,
    sharp: TextureHandle,
    soft: TextureHandle,
}

thread_local! {
    static BAKED: RefCell<Option<Baked>> = const { RefCell::new(None) };
}

/// The tile's width in texels at a scale: about a texel a physical pixel.
fn tile_texels(ppp: f32) -> usize {
    ((TILE_U * ppp).round() as usize).clamp(128, 768) / 4 * 4
}

/// The two textures at this scale (baked the first time, and again only when the scale changes).
fn textures(ctx: &egui::Context) -> (TextureId, TextureId) {
    let w = tile_texels(ctx.pixels_per_point());
    BAKED.with(|b| {
        let mut b = b.borrow_mut();
        if b.as_ref().is_none_or(|x| x.w != w) {
            let (sharp, soft) = bake(w);
            let opt = TextureOptions {
                wrap_mode: egui::TextureWrapMode::Repeat,
                ..TextureOptions::LINEAR
            };
            *b = Some(Baked {
                w,
                sharp: ctx.load_texture("streak-lava", sharp, opt),
                soft: ctx.load_texture("streak-soft", soft, opt),
            });
        }
        let x = b.as_ref().expect("baked");
        (x.sharp.id(), x.soft.id())
    })
}

// -- the meshes ----------------------------------------------------------------------------------------------------

/// Rows across the band (fractions from the inner edge to the outer one).
const ROWS: [f32; 5] = [0.0, 0.22, 0.5, 0.78, 1.0];

/// One layer of the effect along the band.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub tex: TextureId,
    /// Premultiplied; alpha 0 adds.
    pub color: Color32,
    /// Texture tiles per point along the arc, and the offset (tiles; the flow).
    pub u_scale: f32,
    pub u_off: f32,
    /// Texture tiles per point across the band, and the offset.
    pub v_scale: f32,
    pub v_off: f32,
    /// Strength on each of ROWS.
    pub profile: [f32; 5],
}

/// The band the effect may cover: the lit band, kept a little inside its anti-aliased edges, from 12 o'clock to
/// the end of the lit arc (not over the round end caps). (centre, inner radius, outer radius, angle span).
pub fn inner_band(band: &RingBand, ppp: f32) -> (Pos2, f32, f32, f32) {
    let inset = 0.6 / ppp.max(0.1);
    (
        band.center,
        band.r_in + inset,
        band.r_out - inset,
        band.frac.clamp(0.0, 1.0) * TAU,
    )
}

/// A layer's mesh over the band; empty when the lit arc is too short to show it.
pub fn layer_mesh(band: &RingBand, ppp: f32, l: &Layer) -> Mesh {
    let mut mesh = Mesh::with_texture(l.tex);
    let (c, r0, r1, span) = inner_band(band, ppp);
    let rm = (r0 + r1) / 2.0;
    let len = span * rm;
    let bw = r1 - r0;
    if len < 3.0 || bw <= 0.0 {
        return mesh;
    }
    let segs = ((span.to_degrees() / 2.0).ceil() as usize).max(2);
    let fe = (bw * 1.6).min(len * 0.3);
    for i in 0..=segs {
        let a = span * i as f32 / segs as f32;
        let s = a * rm;
        let fade = smooth(0.0, fe, s) * smooth(0.0, fe, len - s);
        let (sn, cs) = a.sin_cos();
        for (j, &fr) in ROWS.iter().enumerate() {
            let r = r0 + (r1 - r0) * fr;
            let pos = pos2(c.x + r * sn, c.y - r * cs);
            let uv = pos2(s * l.u_scale + l.u_off, fr * bw * l.v_scale + l.v_off);
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: l.color.gamma_multiply(l.profile[j] * fade),
            });
        }
    }
    let n = ROWS.len() as u32;
    for i in 0..segs as u32 {
        for j in 0..n - 1 {
            let (a, b) = (i * n + j, (i + 1) * n + j);
            mesh.add_triangle(a, b, a + 1);
            mesh.add_triangle(b, b + 1, a + 1);
        }
    }
    mesh
}

/// The colours of the layers in a theme: (depth, blobs, rich, glow, core, sparks).
struct Palette {
    depth: [u8; 3],
    blobs: [u8; 3],
    /// The blobs at their hottest (`Look::hot`).
    blobs_hot: [u8; 3],
    rich: [u8; 3],
    glow: [u8; 3],
    core: [u8; 3],
    sparks: [u8; 3],
}

/// The wash's colour from tier 4 (100) to 7 (1000), dark themes and Daylight.
const WASH_DARK: [[u8; 3]; 4] = [[255, 146, 28], [248, 100, 22], [228, 56, 22], [190, 24, 30]];
const WASH_LIGHT: [[u8; 3]; 4] = [[226, 92, 12], [208, 60, 12], [190, 34, 14], [160, 16, 24]];

/// The wash's colour at a level (its tier-4 colour below 4, its last above 7).
pub fn wash_color(level: f32, dark: bool) -> [u8; 3] {
    let ramp = if dark { &WASH_DARK } else { &WASH_LIGHT };
    let l = (level - 4.0).clamp(0.0, 3.0);
    let i = (l.floor() as usize).min(2);
    let k = l - i as f32;
    let (a, b) = (ramp[i], ramp[i + 1]);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// Daylight (an orange arc): white lava in it, a deeper red-orange undercurrent.
const LIGHT: Palette = Palette {
    depth: [205, 62, 8],
    blobs: [255, 247, 232],
    blobs_hot: [255, 250, 238],
    rich: [255, 222, 170],
    glow: [255, 150, 70],
    core: [255, 236, 205],
    sparks: [255, 255, 240],
};
/// The dark themes (a yellow arc): orange lava in it, pale yellow-white currents and a white-hot core on top.
const DARK: Palette = Palette {
    depth: [236, 120, 22],
    blobs: [236, 92, 14],
    blobs_hot: [255, 224, 128],
    rich: [255, 238, 176],
    glow: [255, 150, 40],
    core: [255, 244, 210],
    sparks: [255, 255, 236],
};

/// Every layer for this level at time `t` (seconds) with the pattern flowed `phase` points.
pub fn layers(level: f32, t: f64, phase: f64, dark: bool, sharp: TextureId, soft: TextureId) -> Vec<Layer> {
    let lk = look(level);
    let on = level.clamp(0.0, 1.0);
    let pal = if dark { &DARK } else { &LIGHT };
    let normal =
        |c: [u8; 3], a: f32| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a * on * 255.0) as u8);
    let add = |c: [u8; 3], a: f32| {
        let k = (a * on).clamp(0.0, 1.0);
        Color32::from_rgba_premultiplied(
            (c[0] as f32 * k) as u8,
            (c[1] as f32 * k) as u8,
            (c[2] as f32 * k) as u8,
            0,
        )
    };
    // (the offsets kept within a tile, so precision holds however long the streak lasts)
    let flow = |k: f64, scale: f32| -((phase * k * scale as f64).rem_euclid(1.0)) as f32;
    let tf = (t % 3600.0) as f32;
    let flat = [0.0, 1.0, 1.0, 1.0, 0.0];
    let mut out = Vec::new();
    let mut push = |l: Layer, a: f32| {
        if a > 0.004 {
            out.push(l);
        }
    };
    let tile = (1.0 / TILE_U, 1.0 / TILE_V);
    // the wash first, a plain colour (the font atlas's white texel) under everything, fading off at the band's
    // edges so the arc's own colour rims it
    push(
        Layer {
            tex: TextureId::default(),
            color: normal(wash_color(level, dark), lk.wash),
            u_scale: 0.0,
            u_off: egui::epaint::WHITE_UV.x,
            v_scale: 0.0,
            v_off: egui::epaint::WHITE_UV.y,
            profile: [0.0, 0.85, 1.0, 0.85, 0.0],
        },
        lk.wash,
    );
    let mixc = |a: [u8; 3], b: [u8; 3], k: f32| {
        let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
        [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
    };
    let blobs = mixc(pal.blobs, pal.blobs_hot, lk.hot);
    push(
        Layer {
            tex: soft,
            color: normal(pal.depth, lk.depth),
            u_scale: tile.0 * 0.8,
            u_off: flow(0.55, tile.0 * 0.8),
            v_scale: tile.1,
            v_off: 0.1 + (tf * 0.013).fract(),
            profile: flat,
        },
        lk.depth,
    );
    push(
        Layer {
            tex: sharp,
            color: normal(blobs, lk.blobs),
            u_scale: tile.0,
            u_off: flow(1.0, tile.0),
            v_scale: tile.1,
            v_off: 0.37 + 0.04 * (tf * 0.35).sin(),
            profile: [0.0, 0.9, 1.0, 0.9, 0.0],
        },
        lk.blobs,
    );
    push(
        Layer {
            tex: sharp,
            color: normal(pal.rich, lk.rich),
            u_scale: tile.0 * 1.8,
            u_off: flow(1.4, tile.0 * 1.8),
            v_scale: tile.1 * 1.6,
            v_off: 0.71 - 0.03 * (tf * 0.5).sin(),
            profile: [0.0, 0.7, 1.0, 0.7, 0.0],
        },
        lk.rich,
    );
    let breathe = 1.0 + lk.pulse * (tf * TAU * 0.7).sin();
    push(
        Layer {
            tex: soft,
            color: add(pal.glow, lk.glow * breathe),
            u_scale: tile.0 * 0.6,
            u_off: flow(0.8, tile.0 * 0.6),
            v_scale: tile.1 * 0.5,
            v_off: 0.55,
            profile: [0.0, 0.6, 1.0, 0.6, 0.0],
        },
        lk.glow,
    );
    push(
        Layer {
            tex: soft,
            color: add(pal.core, lk.core),
            u_scale: tile.0 * 2.4,
            u_off: flow(1.8, tile.0 * 2.4),
            v_scale: tile.1 * 0.6,
            v_off: 0.2,
            profile: [0.0, 0.15, 1.0, 0.15, 0.0],
        },
        lk.core,
    );
    push(
        Layer {
            tex: sharp,
            color: add(pal.sparks, lk.sparks),
            u_scale: tile.0 * 4.0,
            u_off: flow(2.6, tile.0 * 4.0),
            v_scale: tile.1 * 1.2,
            v_off: 0.05 + (tf * 0.21).fract(),
            profile: [0.0, 0.3, 1.0, 0.3, 0.0],
        },
        lk.sparks,
    );
    out
}

/// Draw the streak over a ring gauge's lit band (`knobs::ring_band` of the same gauge) at `level`, flowed
/// `phase` points, time `t`.
pub fn draw(p: &Painter, band: &RingBand, level: f32, t: f64, phase: f64, dark: bool) {
    if level <= 1e-3 || band.frac <= 0.0 {
        return;
    }
    let (sharp, soft) = textures(p.ctx());
    let ppp = p.pixels_per_point();
    for l in layers(level, t, phase, dark, sharp, soft) {
        let m = layer_mesh(band, ppp, &l);
        if !m.vertices.is_empty() {
            p.add(Shape::mesh(m));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_start_at_their_thresholds() {
        assert_eq!(tier(0), 0);
        assert_eq!(tier(9), 0);
        assert_eq!(tier(10), 1);
        assert_eq!(tier(19), 1);
        assert_eq!(tier(20), 2);
        assert_eq!(tier(49), 2);
        assert_eq!(tier(50), 3);
        assert_eq!(tier(99), 3);
        assert_eq!(tier(100), 4);
        assert_eq!(tier(199), 4);
        assert_eq!(tier(200), 5);
        assert_eq!(tier(499), 5);
        assert_eq!(tier(500), 6);
        assert_eq!(tier(999), 6);
        assert_eq!(tier(1000), 7);
        assert_eq!(tier(50_000), 7);
    }

    #[test]
    fn each_tier_is_hotter() {
        for i in 0..MAX_TIER {
            let (a, b) = (LOOKS[i], LOOKS[i + 1]);
            assert!(b.speed > a.speed, "speed {i}");
            assert!(b.blobs >= a.blobs && b.glow >= a.glow && b.core >= a.core, "{i}");
            assert!(
                b.blobs + b.glow + b.core + b.rich + b.sparks > a.blobs + a.glow + a.core + a.rich + a.sparks
            );
        }
        // the top tiers part in a still: a wash over the band from 100, stronger and redder at each tier
        for i in 4..MAX_TIER {
            assert!(
                LOOKS[i + 1].wash > LOOKS[i].wash && LOOKS[i].wash >= 0.3,
                "wash {i}"
            );
        }
        assert!(LOOKS[..4].iter().all(|l| l.wash == 0.0));
        // redder: less green for its red at each tier, in both looks
        for dark in [true, false] {
            let red = |l: f32| {
                let c = wash_color(l, dark);
                c[0] as f32 / c[1].max(1) as f32
            };
            assert!(
                red(4.0) < red(5.0) && red(5.0) < red(6.0) && red(6.0) < red(7.0),
                "{dark}"
            );
        }
        assert_eq!(look(0.0), LOOKS[0]);
        assert_eq!(look(7.0), LOOKS[7]);
        assert_eq!(look(99.0), LOOKS[7]);
        let mid = look(3.5);
        assert!((mid.speed - (LOOKS[3].speed + LOOKS[4].speed) / 2.0).abs() < 1e-4);
    }

    #[test]
    fn the_streak_count_heats_through_orange_to_red() {
        let white = Color32::from_rgb(240, 240, 240);
        assert_eq!(heat_color(0.0, white, true), white);
        // tier 1 is the ramp's first colour, the top its last; halfway to tier 1 is halfway from the base
        assert_eq!(heat_color(1.0, white, true), Color32::from_rgb(255, 214, 130));
        assert_eq!(heat_color(7.0, white, true), Color32::from_rgb(255, 48, 30));
        assert_eq!(heat_color(12.0, white, true), Color32::from_rgb(255, 48, 30));
        let h = heat_color(0.5, white, true);
        assert_eq!((h.r(), h.g(), h.b()), (248, 227, 185));
        // warm all along, and redder at every tier
        for dark in [true, false] {
            let base = if dark {
                white
            } else {
                Color32::from_rgb(27, 29, 32)
            };
            let mut red = -1;
            for l in 1..=MAX_TIER {
                let c = heat_color(l as f32, base, dark);
                assert!(c.r() > c.g() && c.g() >= c.b(), "{dark} {l} {c:?}");
                let r = c.r() as i32 - c.g() as i32;
                assert!(r > red, "{dark} {l}");
                red = r;
            }
        }
        // Daylight's stays dark enough to read on its light card
        let top = heat_color(7.0, Color32::BLACK, false);
        assert!(top.r() < 200 && top.g() < 40);
    }

    #[test]
    fn it_climbs_to_a_tier_and_cools_when_the_streak_breaks() {
        let mut fx = StreakFx::default();
        assert_eq!(fx.update(0.0, 5), 0.0);
        assert!(!fx.active());
        // to tier 1 in 1 / RISE s (frames of 1/60 s)
        let mut t = 0.0;
        fx.update(t, 10);
        while fx.level() < 1.0 {
            t += 1.0 / 60.0;
            fx.update(t, 10);
            assert!(t < 0.5);
        }
        assert!((t - 1.0 / RISE as f64).abs() < 0.03, "{t}");
        // straight to tier 7 (a shot), then the streak breaks at t0
        fx.snap(1200);
        assert_eq!(fx.level(), 7.0);
        let t0 = 10.0;
        fx.update(t0 - 0.01, 1200);
        assert_eq!(fx.update(t0, 0), 7.0);
        let mut last = 7.0;
        let mut t = t0;
        while t < t0 + COOL_S + 0.02 {
            t += 1.0 / 60.0;
            let l = fx.update(t, 0);
            assert!(l <= last, "it only cools");
            last = l;
        }
        assert_eq!(fx.level(), 0.0);
        assert!(!fx.active());
        // quick: more than half gone in a quarter of the time (ease-out), not a pop: still there after one frame
        assert!(cooled(7.0, 0.0, COOL_S * 0.25) < 4.0);
        assert!(cooled(7.0, 0.0, 1.0 / 60.0) > 6.5);
        assert_eq!(cooled(7.0, 0.0, COOL_S), 0.0);
        assert_eq!(cooled(7.0, 2.0, COOL_S * 3.0), 2.0);
    }

    #[test]
    fn a_new_streak_during_the_cool_down_stops_it_at_its_tier() {
        let mut fx = StreakFx::default();
        fx.snap(60);
        fx.update(0.0, 60);
        fx.update(0.1, 0);
        let cooling = fx.update(0.2, 0);
        assert!(cooling < 3.0 && cooling > 0.0);
        // a streak back above 10 halfway: no longer cooling to 0, climbs back to tier 1
        let l = fx.update(0.3, 12);
        assert!(l >= cooling.min(1.0) - 1e-3);
        for k in 0..60 {
            fx.update(0.3 + k as f64 / 60.0, 12);
        }
        assert!((fx.level() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_flow_moves_faster_at_higher_tiers() {
        let mut a = StreakFx::default();
        let mut b = StreakFx::default();
        a.snap(10);
        b.snap(1000);
        for k in 0..=60 {
            a.update(k as f64 / 60.0, 10);
            b.update(k as f64 / 60.0, 1000);
        }
        assert!((a.phase() - LOOKS[1].speed as f64).abs() < 0.5);
        assert!(b.phase() > a.phase() * 4.0);
    }

    fn band(frac: f32) -> RingBand {
        RingBand {
            center: pos2(100.0, 80.0),
            r_in: 40.0,
            r_out: 48.0,
            frac,
        }
    }

    #[test]
    fn the_mesh_stays_inside_the_lit_band() {
        for ppp in [1.0, 1.5, 2.0, 3.0] {
            for frac in [0.04, 0.1, 0.33, 0.5, 0.75, 0.999, 1.0] {
                let b = band(frac);
                for level in [0.5, 1.0, 3.3, 7.0] {
                    let ls = layers(
                        level,
                        12.3,
                        456.7,
                        true,
                        TextureId::Managed(1),
                        TextureId::Managed(2),
                    );
                    assert!(!ls.is_empty());
                    for l in &ls {
                        let m = layer_mesh(&b, ppp, l);
                        assert!(!m.vertices.is_empty());
                        for v in &m.vertices {
                            let (dx, dy) = (v.pos.x - b.center.x, v.pos.y - b.center.y);
                            let r = (dx * dx + dy * dy).sqrt();
                            assert!(r >= b.r_in - 1e-3 && r <= b.r_out + 1e-3, "radius {r}");
                            // angle clockwise from 12 o'clock, in [0, 2 pi)
                            let mut a = dx.atan2(-dy);
                            if a < -1e-4 {
                                a += TAU;
                            }
                            let lim = frac * TAU + 1e-3;
                            // (the last vertex of a full ring sits back at 12 o'clock)
                            let wrapped = frac >= 1.0 && a < 1e-3;
                            assert!(a >= -1e-4 && (a <= lim || wrapped), "angle {a} > {lim}");
                        }
                        // the triangles' edges are chords of the outer circle: they never leave the disc, and an
                        // edge on the inner circle never crosses into the hole (chords lie outside the inner
                        // circle only when both ends are on it at a small angle: their middle dips in by at most
                        // the sagitta, which the inset covers)
                        let (_, r0, _, span) = inner_band(&b, ppp);
                        let segs = ((span.to_degrees() / 2.0).ceil()).max(2.0);
                        let half = span / segs / 2.0;
                        assert!(r0 * (1.0 - half.cos()) < r0 - b.r_in, "sagitta within the inset");
                    }
                }
            }
        }
        // nothing on a lit arc too short to show it, and nothing at level 0
        let tiny = band(0.005);
        let l = layers(7.0, 0.0, 0.0, true, TextureId::Managed(1), TextureId::Managed(2));
        assert!(layer_mesh(&tiny, 2.0, &l[0]).vertices.is_empty());
        assert!(layers(0.0, 0.0, 0.0, false, TextureId::Managed(1), TextureId::Managed(2)).is_empty());
    }

    #[test]
    fn the_ends_fade_into_the_plain_arc() {
        let b = band(0.5);
        let l = layers(7.0, 0.0, 0.0, false, TextureId::Managed(1), TextureId::Managed(2));
        let m = layer_mesh(&b, 2.0, &l[1]);
        let n = ROWS.len();
        let first = &m.vertices[..n];
        let last = &m.vertices[m.vertices.len() - n..];
        for v in first.iter().chain(last) {
            assert_eq!(v.color.a(), 0);
        }
        // and the band's edges too (the baked arc's anti-aliased rim stays as it was)
        for (k, v) in m.vertices.iter().enumerate() {
            if k % n == 0 || k % n == n - 1 {
                assert_eq!(v.color, Color32::TRANSPARENT);
            }
        }
    }

    #[test]
    fn the_textures_tile() {
        // the field wraps: its left and right columns, top and bottom rows match
        for k in 0..40 {
            let t = k as f32 / 40.0;
            assert!((lava(0.0, t, 7) - lava(1.0, t, 7)).abs() < 1e-4);
            assert!((lava(t, 0.0, 7) - lava(t, 1.0, 7)).abs() < 1e-4);
        }
        let (sharp, soft) = bake(128);
        assert_eq!(sharp.size, [128, 32]);
        // lava blobs cover some of it, not all, and the soft field more
        let cover = |img: &ColorImage| {
            img.pixels.iter().filter(|p| p.a() > 128).count() as f32 / img.pixels.len() as f32
        };
        let (cs, cf) = (cover(&sharp), cover(&soft));
        assert!(cs > 0.12 && cs < 0.6, "{cs}");
        assert!(cf > 0.2 && cf < 0.9, "{cf}");
        assert_eq!(tile_texels(1.0), 160);
        assert_eq!(tile_texels(3.0), 480);
        assert_eq!(tile_texels(0.5), 128);
    }
}
