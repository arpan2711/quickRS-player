//! The shared room (P34.5, D88): one hall (a Victorian iron-and-glass station, the owner's pick of three MOSS
//! panoramas) that every glossy thing on the dark themes reflects, by where it sits in the window: the glass keys,
//! and the transport's gem, Go stones, thumb and resin bar. New in quickRS (the Python app has nothing like it); the
//! pack is the research folder's `mockups/ui-main/tiles/room/app_assets/` = `assets/hall/` (its `manifest.json`
//! carries every number, its README the method; the prototype is that folder's `tools/rt.py`).
//!
//! No shaders: each reflective surface is one egui mesh on the room's texture, a sphere map of the hall whose bytes
//! are linear light x `E_T`. Its vertices carry a normal (screen frame: x right, y up, z out of the screen), a
//! reflectance m and the colour under them, baked once; per vertex the CPU works out the view ray from an eye one
//! window-width in front of the window's centre, reflects it and looks the room up in the sphere map (`Eye::uv`).
//! Two blends:
//! - additive (glass keys, gem, resin): a grey per channel with alpha 0 (egui's premultiplied blend adds it), its
//!   strength the sRGB curve's slope at the colour under it, so the gamma-space add matches adding light
//!   (`additive`, with L the blurred room's radiance there, from the CPU copy);
//! - mix (the white Go stones and the thumb, where an add can't show): premultiplied (a, a, a, a) on an sRGB copy of
//!   the room, `out = fb (1 - a) + a room` (`mix`).
//!
//! The textures are uploaded once per context, as their bytes (not sRGB-decoded), linear filtering, clamped. A key's
//! arris sweeps its normal across about half the 1024 probe in a few pixels, so sampled as it is the roof's bright
//! lines alias into dashes that crawl as the window scrolls. The GPU can't help (egui-wgpu, our renderer, makes no
//! mipmaps whatever TextureOptions says), so the room keeps its own pyramid (each level a 2 x 2 box of the one
//! above, in the probe's linear bytes) and Hall::mipped draws each minified triangle on the two levels its texel
//! footprint falls between, weighted: for the additive surfaces that sum is exactly trilinear filtering (P34.7). The
//! flat faces magnify the probe and stay on the full-size level, their vertices shared.

use std::sync::Arc;

use egui::epaint::Vertex;
use egui::{Color32, ColorImage, Context, Id, Mesh, Pos2, TextureHandle, TextureId, TextureOptions};

/// How much of the pack's room every surface shows: the owner wants blank glass on a gloomy day (P34.5: 0.3 of the
/// pack's gains and mix strengths).
pub const LEVEL: f32 = 0.3;

/// The probe's bytes are `clamp(E_T * linear radiance)`.
pub const E_T: f32 = 2.5;
/// The mix surfaces' room: `srgb(min(1, MIX_EXPOSURE * bytes / 255 / E_T))`.
pub const MIX_EXPOSURE: f32 = 1.7;
/// The mix surfaces' alpha: `min(MIX_AMAX, MIX_K * m)`.
pub const MIX_K: f32 = 1.85;
pub const MIX_AMAX: f32 = 0.5;

const PROBE: &[u8] = include_bytes!("../../../assets/hall/room_probe.jpg");
const PROBE_BLUR: &[u8] = include_bytes!("../../../assets/hall/room_probe_blur.jpg");

/// IEC sRGB to linear.
pub fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear to IEC sRGB.
pub fn srgb(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Where the viewer is: one window-width (`d`) in front of the window's centre, all in points (the window `w` x
/// `h`, its top-left at `origin`). Moving the window on the monitor changes nothing; resizing it moves the room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eye {
    pub origin: Pos2,
    pub w: f32,
    pub h: f32,
    pub d: f32,
}

impl Eye {
    pub fn new(w: f32, h: f32) -> Eye {
        Eye {
            origin: Pos2::ZERO,
            w,
            h,
            d: w,
        }
    }

    /// The window the painter's context draws in.
    pub fn of(ctx: &Context) -> Eye {
        let r = ctx.content_rect();
        Eye {
            origin: r.min,
            ..Eye::new(r.width().max(1.0), r.height().max(1.0))
        }
    }

    /// A key for caches: the window's size (whole points are plenty).
    pub fn key(&self) -> (i32, i32) {
        (self.w.round() as i32, self.h.round() as i32)
    }

    /// The room's sphere-map UV seen in a surface at window point `p` (y down) with normal `n` (screen frame):
    /// `v = normalize(x - W/2, H/2 - y, -D)`, `R = v - 2 (v.n) n`, `m = 2 sqrt(2 (1 + Rz))`,
    /// `uv = (0.5 + Rx / m, 0.5 - Ry / m)`.
    pub fn uv(&self, p: Pos2, n: [f32; 3]) -> [f32; 2] {
        let (x, y) = (p.x - self.origin.x, p.y - self.origin.y);
        let v = [x - self.w / 2.0, self.h / 2.0 - y, -self.d];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let v = [v[0] / l, v[1] / l, v[2] / l];
        let k = 2.0 * (v[0] * n[0] + v[1] * n[1] + v[2] * n[2]);
        let r = [v[0] - k * n[0], v[1] - k * n[1], v[2] - k * n[2]];
        let m = 2.0 * (2.0 * (1.0 + r[2])).max(1e-6).sqrt();
        [0.5 + r[0] / m, 0.5 - r[1] / m]
    }
}

/// `srgb` from a table (linear 0..2 in 8192 steps, interpolated; within 2e-5 of it), for the per-vertex colours.
fn srgb_lut(c: f32) -> f32 {
    const N: usize = 8192;
    const TOP: f32 = 2.0;
    static LUT: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let lut = LUT.get_or_init(|| (0..=N).map(|i| srgb(i as f32 * TOP / N as f32)).collect());
    let x = c.max(0.0) * (N as f32 / TOP);
    if x >= N as f32 {
        return srgb(c);
    }
    let i = x as usize;
    let f = x - i as f32;
    lut[i] + (lut[i + 1] - lut[i]) * f
}

/// An additive surface's vertex colour (alpha 0): per channel `c = clamp(G m S / E_T, 0, 1)` with
/// `S = (srgb(lin(base) + x) - base) / x`, `x = max(G m L, 1e-5)`. `gm` is the gain times the reflectance,
/// `base` the colour under the vertex (sRGB 0..1) and `lin_base` its linear value, `l` the room's radiance there.
pub fn additive(base: [f32; 3], lin_base: [f32; 3], gm: f32, l: [f32; 3]) -> Color32 {
    additive_rgb(base, lin_base, [gm; 3], l)
}

/// `additive` with the gain times the reflectance per channel (a metal tints its reflection: gold reflects gold).
pub fn additive_rgb(base: [f32; 3], lin_base: [f32; 3], gm: [f32; 3], l: [f32; 3]) -> Color32 {
    if gm.iter().all(|g| *g <= 0.0) {
        return Color32::TRANSPARENT;
    }
    let ch = |i: usize| {
        if gm[i] <= 0.0 {
            return 0;
        }
        let x = (gm[i] * l[i]).max(1e-5);
        let s = (srgb_lut(lin_base[i] + x) - base[i]) / x;
        ((gm[i] * s / E_T).clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Color32::from_rgba_premultiplied(ch(0), ch(1), ch(2), 0)
}

/// A mix surface's vertex colour: premultiplied (a, a, a, a), `a = min(MIX_AMAX, MIX_K m)` at `LEVEL` of it.
pub fn mix(m: f32) -> Color32 {
    let a = ((MIX_K * m).clamp(0.0, MIX_AMAX) * LEVEL * 255.0).round() as u8;
    Color32::from_rgba_premultiplied(a, a, a, a)
}

/// Which of the room's textures a mesh samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tex {
    /// The additive surfaces' (`probe`).
    Probe,
    /// The Go stones' (`mix_tex`).
    Mix,
    /// The thumb's (`mix_blur_tex`).
    MixBlur,
    /// The blurred room as it is, for additive satin metal (the knobs' brushed caps, P34.8).
    Blur,
}

/// The pyramid stops at this size (a 16 px room is all blur).
const MIP_MIN: usize = 16;

/// The room: its textures on the GPU (each a pyramid, full size first) and the blurred copy on the CPU.
pub struct Hall {
    probe: Vec<TextureHandle>,
    mix: Vec<TextureHandle>,
    mix_blur: Vec<TextureHandle>,
    blur_tex: Vec<TextureHandle>,
    /// room_probe_blur.jpg decoded (RGB), `blur_px` square.
    blur: Vec<u8>,
    blur_px: usize,
}

/// An 8-bit RGB image decoded from the pack.
fn decode(bytes: &[u8]) -> (Vec<u8>, usize, usize) {
    let im = image::load_from_memory(bytes).expect("hall asset").to_rgb8();
    let (w, h) = (im.width() as usize, im.height() as usize);
    (im.into_raw(), w, h)
}

/// RGB bytes as an opaque egui image, taken as they are.
fn opaque(rgb: &[u8], w: usize, h: usize) -> ColorImage {
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    ColorImage::from_rgba_premultiplied([w, h], &rgba)
}

/// The mix surfaces' room: `srgb(min(1, 1.7 * bytes / 255 / E_T))`.
fn mix_bytes(rgb: &[u8]) -> Vec<u8> {
    let lut: Vec<u8> = (0..256)
        .map(|b| {
            let l = (MIX_EXPOSURE * b as f32 / 255.0 / E_T).min(1.0);
            (srgb(l) * 255.0).round() as u8
        })
        .collect();
    rgb.iter().map(|&b| lut[b as usize]).collect()
}

/// A square RGB image `n` across and its halvings down to `MIP_MIN` (2 x 2 boxes, rounded): (bytes, size) each.
fn pyramid(rgb: Vec<u8>, n: usize) -> Vec<(Vec<u8>, usize)> {
    let mut out = vec![(rgb, n)];
    while out.last().is_some_and(|l| l.1 / 2 >= MIP_MIN) {
        let (src, n) = out.last().expect("a level");
        let h = n / 2;
        let mut dst = vec![0u8; h * h * 3];
        for y in 0..h {
            for x in 0..h {
                for c in 0..3 {
                    let at = |xx: usize, yy: usize| src[(yy * n + xx) * 3 + c] as u32;
                    let s = at(2 * x, 2 * y)
                        + at(2 * x + 1, 2 * y)
                        + at(2 * x, 2 * y + 1)
                        + at(2 * x + 1, 2 * y + 1);
                    dst[(y * h + x) * 3 + c] = ((s + 2) / 4) as u8;
                }
            }
        }
        out.push((dst, h));
    }
    out
}

/// A triangle's level of detail: log2 of the most texels one screen pixel steps over (its uv's Jacobian's longer
/// column, as a GPU picks a mip), for a texture `n` texels across; screen positions in points at `ppp`.
fn lod(p: [Pos2; 3], uv: [Pos2; 3], n: f32, ppp: f32) -> f32 {
    let (e1, e2) = ((p[1] - p[0]) * ppp, (p[2] - p[0]) * ppp);
    let (t1, t2) = ((uv[1] - uv[0]) * n, (uv[2] - uv[0]) * n);
    let det = e1.x * e2.y - e1.y * e2.x;
    if det.abs() < 1e-6 {
        return 0.0;
    }
    let dx = (t1 * e2.y - t2 * e1.y) / det;
    let dy = (t2 * e1.x - t1 * e2.x) / det;
    dx.length().max(dy.length()).max(1e-6).log2().max(0.0)
}

impl Hall {
    fn load(ctx: &Context) -> Hall {
        let opts = TextureOptions::LINEAR; // linear, clamp (the pyramid is ours)
        let (p, pw, _) = decode(PROBE);
        let (b, bw, _) = decode(PROBE_BLUR);
        let upload = |name: &str, levels: &[(Vec<u8>, usize)], mix: bool| -> Vec<TextureHandle> {
            levels
                .iter()
                .enumerate()
                .map(|(i, (rgb, n))| {
                    let img = if mix {
                        opaque(&mix_bytes(rgb), *n, *n)
                    } else {
                        opaque(rgb, *n, *n)
                    };
                    ctx.load_texture(format!("{name}-{i}"), img, opts)
                })
                .collect()
        };
        let probe_levels = pyramid(p, pw);
        let blur_levels = pyramid(b.clone(), bw);
        Hall {
            probe: upload("hall-probe", &probe_levels, false),
            mix: upload("hall-mix", &probe_levels, true),
            mix_blur: upload("hall-mix-blur", &blur_levels, true),
            blur_tex: upload("hall-blur", &blur_levels, false),
            blur: b,
            blur_px: bw,
        }
    }

    fn levels(&self, t: Tex) -> &[TextureHandle] {
        match t {
            Tex::Probe => &self.probe,
            Tex::Mix => &self.mix,
            Tex::MixBlur => &self.mix_blur,
            Tex::Blur => &self.blur_tex,
        }
    }

    /// A room mesh (its vertices' UVs on `t`, colours premultiplied: an additive grey with alpha 0, or a mix's
    /// (a, a, a, a)) as meshes on the pyramid's levels for `ppp` pixels a point: each triangle that magnifies the
    /// room stays on the full-size level (vertices shared), each that minifies it goes on the two levels round its
    /// level of detail, its colours split between them by where it falls (`lod`).
    pub fn mipped(&self, t: Tex, vertices: Vec<Vertex>, indices: &[u32], ppp: f32) -> Vec<Arc<Mesh>> {
        let levels = self.levels(t);
        let n = levels[0].size()[0] as f32;
        let top = (levels.len() - 1) as f32;
        let mut out: Vec<Mesh> = levels.iter().map(|l| Mesh::with_texture(l.id())).collect();
        let mut base_idx = Vec::with_capacity(indices.len());
        for tri in indices.chunks_exact(3) {
            let v = [tri[0], tri[1], tri[2]].map(|i| vertices[i as usize]);
            let l = lod(v.map(|x| x.pos), v.map(|x| x.uv), n, ppp).min(top);
            if l <= 0.01 || v.iter().all(|x| x.color == Color32::TRANSPARENT) {
                base_idx.extend_from_slice(tri);
                continue;
            }
            let lo = l.floor();
            let f = l - lo;
            for (level, w) in [(lo as usize, 1.0 - f), (lo as usize + 1, f)] {
                if w < 0.01 || level >= out.len() {
                    continue;
                }
                let m = &mut out[level];
                let at = m.vertices.len() as u32;
                for x in v {
                    m.vertices.push(Vertex {
                        color: x.color.gamma_multiply(w),
                        ..x
                    });
                }
                m.indices.extend_from_slice(&[at, at + 1, at + 2]);
            }
        }
        // the full-size level: the shared vertices and their triangles, then any minified ones put there
        let extra = std::mem::take(&mut out[0]);
        let mut first = Mesh::with_texture(levels[0].id());
        first.vertices = vertices;
        first.indices = base_idx;
        first.append(extra);
        out[0] = first;
        // (a mesh with no triangles must not reach the GPU: wgpu can't bind an empty index buffer)
        out.into_iter()
            .filter(|m| !m.indices.is_empty())
            .map(Arc::new)
            .collect()
    }
    /// The room for this context (its textures uploaded the first time).
    pub fn get(ctx: &Context) -> Arc<Hall> {
        let id = Id::new("qrs-hall");
        if let Some(h) = ctx.data(|d| d.get_temp::<Arc<Hall>>(id)) {
            return h;
        }
        let h = Arc::new(Hall::load(ctx));
        ctx.data_mut(|d| d.insert_temp(id, h.clone()));
        h
    }

    /// The additive surfaces' texture (full size).
    pub fn probe(&self) -> TextureId {
        self.probe[0].id()
    }

    /// The Go stones' room (sRGB).
    pub fn mix_tex(&self) -> TextureId {
        self.mix[0].id()
    }

    /// The thumb's room (sRGB, blurred: its dish is satin).
    pub fn mix_blur_tex(&self) -> TextureId {
        self.mix_blur[0].id()
    }

    /// The room's local radiance at `uv` (linear, per channel): the blurred copy, bilinear at texel centres,
    /// clamped, `/ 255 / E_T`.
    pub fn radiance(&self, uv: [f32; 2]) -> [f32; 3] {
        radiance_in(&self.blur, self.blur_px, uv)
    }
}

/// Bilinear (texel centres, clamped) in a square RGB image, as linear radiance.
fn radiance_in(img: &[u8], n: usize, uv: [f32; 2]) -> [f32; 3] {
    let x = uv[0] * n as f32 - 0.5;
    let y = uv[1] * n as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let last = n as i64 - 1;
    let at = |xx: i64, yy: i64, c: usize| {
        let (xx, yy) = (xx.clamp(0, last) as usize, yy.clamp(0, last) as usize);
        img[(yy * n + xx) * 3 + c] as f32
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    std::array::from_fn(|c| {
        let a = at(x0, y0, c) * (1.0 - fx) + at(x0 + 1, y0, c) * fx;
        let b = at(x0, y0 + 1, c) * (1.0 - fx) + at(x0 + 1, y0 + 1, c) * fx;
        (a * (1.0 - fy) + b * fy) / 255.0 / E_T
    })
}

/// The blurred room on the CPU alone (tests, and code that has no context).
pub fn blur_copy() -> (Vec<u8>, usize) {
    let (b, w, _) = decode(PROBE_BLUR);
    (b, w)
}

/// `Hall::radiance` on a CPU copy from `blur_copy`.
pub fn radiance_of(copy: &(Vec<u8>, usize), uv: [f32; 2]) -> [f32; 3] {
    radiance_in(&copy.0, copy.1, uv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rooms_textures_load_at_their_sizes() {
        let (p, w, h) = decode(PROBE);
        assert_eq!((w, h, p.len()), (1024, 1024, 1024 * 1024 * 3));
        let (_, w, h) = decode(PROBE_BLUR);
        assert_eq!((w, h), (256, 256));
        // the pyramid: 1024 down to 16, each level the average of the one above
        let lv = pyramid(p, w * 4);
        assert_eq!(
            lv.iter().map(|l| l.1).collect::<Vec<_>>(),
            [1024, 512, 256, 128, 64, 32, 16]
        );
        let (a, b) = (&lv[0].0, &lv[1].0);
        let s: u32 = [0, 1, 1024, 1025].iter().map(|&i| a[i * 3] as u32).sum();
        assert_eq!(b[0] as u32, (s + 2) / 4);
    }

    #[test]
    fn a_minified_triangle_goes_on_the_levels_round_its_detail() {
        // a pixel stepping 8 texels across: level 3; the other way one texel, so the longer step counts
        let px = |x: f32, y: f32| Pos2::new(x, y);
        let n = 1024.0;
        let l = lod(
            [px(0.0, 0.0), px(1.0, 0.0), px(0.0, 1.0)],
            [px(0.0, 0.0), px(8.0 / n, 0.0), px(0.0, 1.0 / n)],
            n,
            1.0,
        );
        assert!((l - 3.0).abs() < 1e-4, "{l}");
        // at 2 pixels a point the same triangle covers twice the pixels: a level less; magnified: 0
        let l2 = lod(
            [px(0.0, 0.0), px(1.0, 0.0), px(0.0, 1.0)],
            [px(0.0, 0.0), px(8.0 / n, 0.0), px(0.0, 1.0 / n)],
            n,
            2.0,
        );
        assert!(
            (l2 - 2.0).abs() < 1e-4
                && lod(
                    [px(0.0, 0.0), px(10.0, 0.0), px(0.0, 10.0)],
                    [px(0.0, 0.0), px(1.0 / n, 0.0), px(0.0, 1.0 / n)],
                    n,
                    1.0
                ) == 0.0
        );
        // a mesh: a magnified triangle (shared vertices, level 0) and one at level 2.5 (half on 2, half on 3)
        let ctx = Context::default();
        let hall = Hall::get(&ctx);
        let grey = Color32::from_rgba_premultiplied(100, 100, 100, 0);
        let v = |x: f32, y: f32, u: f32, w: f32| Vertex {
            pos: px(x, y),
            uv: px(u, w),
            color: grey,
        };
        let s = 2f32.powf(2.5) / n;
        let verts = vec![
            v(0.0, 0.0, 0.5, 0.5),
            v(20.0, 0.0, 0.5 + 1.0 / n, 0.5),
            v(0.0, 20.0, 0.5, 0.5 + 1.0 / n),
            v(100.0, 0.0, 0.1, 0.1),
            v(101.0, 0.0, 0.1 + s, 0.1),
            v(100.0, 1.0, 0.1, 0.1 + s),
        ];
        let meshes = hall.mipped(Tex::Probe, verts, &[0, 1, 2, 3, 4, 5], 1.0);
        assert_eq!(meshes.len(), 3);
        // every triangle minified: no level-0 mesh of vertices without triangles (wgpu can't draw one)
        let only = hall.mipped(
            Tex::Probe,
            vec![
                v(100.0, 0.0, 0.1, 0.1),
                v(101.0, 0.0, 0.1 + s, 0.1),
                v(100.0, 1.0, 0.1, 0.1 + s),
            ],
            &[0, 1, 2],
            1.0,
        );
        assert!(only.iter().all(|m| !m.indices.is_empty()) && only.len() == 2);
        assert_eq!(meshes[0].texture_id, hall.probe());
        assert_eq!((meshes[0].vertices.len(), meshes[0].indices.len()), (6, 3));
        for m in &meshes[1..] {
            assert_eq!(m.indices.len(), 3);
            // the two halves add up to the triangle's light
            assert!(
                (m.vertices[0].color.r() as i32 - 50).abs() <= 1,
                "{:?}",
                m.vertices[0].color
            );
            assert_eq!(m.vertices[0].color.a(), 0);
        }
        assert_eq!(meshes[1].texture_id, hall.probe[2].id());
        assert_eq!(meshes[2].texture_id, hall.probe[3].id());
    }

    #[test]
    fn a_flat_face_in_the_middle_sees_the_centre() {
        // the window's centre, facing the viewer: R = +z, the sphere map's centre
        let e = Eye::new(1440.0, 900.0);
        let uv = e.uv(Pos2::new(720.0, 450.0), [0.0, 0.0, 1.0]);
        assert!((uv[0] - 0.5).abs() < 1e-6 && (uv[1] - 0.5).abs() < 1e-6);
        // higher on screen sees more roof (v smaller), right sees right
        let up = e.uv(Pos2::new(720.0, 100.0), [0.0, 0.0, 1.0]);
        let right = e.uv(Pos2::new(1300.0, 450.0), [0.0, 0.0, 1.0]);
        assert!(up[1] < 0.5 && right[0] > 0.5);
        // moving the window on the monitor changes nothing
        let moved = Eye {
            origin: Pos2::new(300.0, 200.0),
            ..e
        };
        let a = moved.uv(Pos2::new(1020.0, 300.0), [0.3, -0.2, 0.93]);
        let b = e.uv(Pos2::new(720.0, 100.0), [0.3, -0.2, 0.93]);
        assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
    }

    #[test]
    fn srgb_round_trips_and_the_mix_room_is_brighter() {
        for c in [0.0f32, 0.02, 0.2, 0.5, 0.9, 1.0] {
            assert!((srgb(lin(c)) - c).abs() < 1e-5, "{c}");
        }
        // the table the vertex colours use
        for i in 0..4000 {
            let l = i as f32 * 0.0006;
            assert!((srgb_lut(l) - srgb(l)).abs() < 2e-5, "{l}");
        }
        assert_eq!(srgb_lut(3.0), srgb(3.0));
        let m = mix_bytes(&[0, 64, 255]);
        assert_eq!(m[0], 0);
        // a mid byte comes up (it is dark linear light), the brightest is 1.7 / 2.5 of white's light
        assert!(m[1] > 64 && m[2] == (srgb(MIX_EXPOSURE / E_T) * 255.0).round() as u8);
    }
}
