//! Surfaces: the material's raised and sunken shapes, screens, floating panels, rings, glows, LEDs, art frames.
//! Port of the surface half of rsplay/neu.py (D15). Shapes are rasterized once per (radius, recipe, colour, scale)
//! into a small patch (crate::raster), uploaded as a texture and stretched over any size by a nine-slice mesh; the
//! face's diagonal gradient is its own anti-aliased mesh. Sizes are in points (= the Python app's scale-1 pixels);
//! everything is drawn on physical pixel boundaries.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureOptions, Vec2, pos2};

use crate::raster::{Image, Layers, Mask, pg};
use crate::theme::{self, t};

// -- caches -------------------------------------------------------------------------------------------------------

struct Entry {
    tex: TextureHandle,
    size: [usize; 2],
    bytes: usize,
    used: u64,
}

struct Cache {
    map: HashMap<u64, Entry>,
    bytes: usize,
    max_bytes: usize,
    max_count: usize,
}

impl Cache {
    fn new(max_bytes: usize, max_count: usize) -> Cache {
        Cache {
            map: HashMap::new(),
            bytes: 0,
            max_bytes,
            max_count,
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }

    fn get_or(
        &mut self,
        ctx: &egui::Context,
        key: u64,
        frame: u64,
        make: impl FnOnce() -> Image,
    ) -> (egui::TextureId, [usize; 2]) {
        if let Some(e) = self.map.get_mut(&key) {
            e.used = frame;
            return (e.tex.id(), e.size);
        }
        let img = make();
        let size = [img.w.max(1), img.h.max(1)];
        let img = if img.w == 0 || img.h == 0 {
            Image::new(1, 1)
        } else {
            img
        };
        let tex = ctx.load_texture(format!("neu-{key:016x}"), img.to_egui(), TextureOptions::LINEAR);
        let id = tex.id();
        let bytes = size[0] * size[1] * 4;
        self.bytes += bytes;
        self.map.insert(
            key,
            Entry {
                tex,
                size,
                bytes,
                used: frame,
            },
        );
        while (self.bytes > self.max_bytes || self.map.len() > self.max_count) && self.map.len() > 1 {
            let Some((&old, _)) = self
                .map
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, e)| e.used)
            else {
                break;
            };
            if let Some(e) = self.map.remove(&old) {
                self.bytes -= e.bytes;
            }
        }
        (id, size)
    }
}

struct Kit {
    patches: Cache,
    small: Cache,
    /// Art that looks the same in every theme (knob bodies, pointers).
    bodies: Cache,
    /// The edges of the material's faces, per size (neu `_surfs`).
    faces: Cache,
    theme_gen: usize,
    ppp: f32,
    frame: u64,
    last_pass: u64,
}

thread_local! {
    static KIT: RefCell<Kit> = RefCell::new(Kit {
        patches: Cache::new(128 << 20, 4000),
        small: Cache::new(256 << 20, 900),
        bodies: Cache::new(64 << 20, 200),
        faces: Cache::new(128 << 20, 4000),
        theme_gen: usize::MAX,
        ppp: 0.0,
        frame: 0,
        last_pass: u64::MAX,
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Store {
    Patch,
    Small,
    Body,
    Face,
}

/// A cached texture: built by `make` (physical pixels) the first time `key` is seen at this scale and theme.
pub fn texture(
    ctx: &egui::Context,
    store: Store,
    key: u64,
    make: impl FnOnce() -> Image,
) -> (egui::TextureId, [usize; 2]) {
    let ppp = ctx.pixels_per_point();
    let pass = ctx.cumulative_pass_nr();
    KIT.with(|k| {
        let mut k = k.borrow_mut();
        if k.last_pass != pass {
            k.last_pass = pass;
            k.frame += 1;
        }
        let tg = theme::generation();
        if k.theme_gen != tg {
            k.theme_gen = tg;
            k.patches.clear();
            k.small.clear();
            k.faces.clear();
        }
        if (k.ppp - ppp).abs() > 1e-4 {
            k.ppp = ppp;
            k.patches.clear();
            k.small.clear();
            k.bodies.clear();
            k.faces.clear();
        }
        let frame = k.frame;
        match store {
            Store::Patch => k.patches.get_or(ctx, key, frame, make),
            Store::Small => k.small.get_or(ctx, key, frame, make),
            Store::Body => k.bodies.get_or(ctx, key, frame, make),
            Store::Face => k.faces.get_or(ctx, key, frame, make),
        }
    })
}

/// Drop every cached texture (e.g. after the fonts or the scale changed outside the usual checks).
pub fn clear_caches() {
    KIT.with(|k| {
        let mut k = k.borrow_mut();
        k.patches.clear();
        k.small.clear();
        k.bodies.clear();
        k.faces.clear();
    });
    CPU_PATCHES.with(|c| c.borrow_mut().clear());
}

thread_local! {
    /// Patches kept on the CPU to build faces from (keyed with the theme and scale they were made in).
    static CPU_PATCHES: RefCell<HashMap<u64, std::sync::Arc<Image>>> = RefCell::new(HashMap::new());
}

/// A patch from the CPU cache, made by `make` the first time.
fn cpu_patch(key: u64, make: impl FnOnce() -> Image) -> std::sync::Arc<Image> {
    if let Some(img) = CPU_PATCHES.with(|c| c.borrow().get(&key).cloned()) {
        return img;
    }
    let img = std::sync::Arc::new(make());
    CPU_PATCHES.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 192 {
            c.clear();
        }
        c.insert(key, img.clone());
    });
    img
}

pub fn key_of(v: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// f32 as a hashable value.
pub(crate) fn fk(v: f32) -> u32 {
    v.to_bits()
}

pub(crate) fn ck(c: Color32) -> [u8; 4] {
    c.to_array()
}

// -- physical pixels ------------------------------------------------------------------------------------------------

/// A rect snapped to physical pixels: (x, y, w, h) in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PxRect {
    pub fn of(rect: Rect, ppp: f32) -> PxRect {
        let x0 = (rect.min.x * ppp).round() as i32;
        let y0 = (rect.min.y * ppp).round() as i32;
        let x1 = (rect.max.x * ppp).round() as i32;
        let y1 = (rect.max.y * ppp).round() as i32;
        PxRect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0),
            h: (y1 - y0).max(0),
        }
    }

    pub fn to_rect(self, ppp: f32) -> Rect {
        Rect::from_min_size(
            pos2(self.x as f32 / ppp, self.y as f32 / ppp),
            Vec2::new(self.w as f32 / ppp, self.h as f32 / ppp),
        )
    }

    pub fn moved(self, dx: i32, dy: i32) -> PxRect {
        PxRect {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// The Python app's scale for a painter at `ppp`: the f64 the app worked out (`units::scale`) when it is this
/// painter's, else `ppp` itself (e.g. the note widget's own window).
pub fn sc(ppp: f32) -> f64 {
    let s = crate::units::scale();
    if s > 0.0 && (s as f32 - ppp).abs() < 1e-4 {
        s
    } else {
        ppp as f64
    }
}

/// `neu.px(v)`: `int(round(v * s))`, halves to even as Python rounds.
pub fn pxr(v: f32, s: f64) -> i32 {
    (v as f64 * s).round_ties_even() as i32
}

/// Scale-1 pixels (points) to physical pixels, as neu.py `px`.
pub fn px(v: f32, ppp: f32) -> i32 {
    pxr(v, sc(ppp))
}

/// `int(v)` of a position in points as the Python app has it in pixels (`int(center[0])`, `pygame.Rect(float)`):
/// toward zero, float noise within 1e-3 px kept on its whole pixel.
pub fn ipx(v: f32, ppp: f32) -> i32 {
    let x = v as f64 * ppp as f64;
    let r = x.round();
    if (x - r).abs() < 1e-3 {
        r as i32
    } else {
        x.trunc() as i32
    }
}

/// A whole pixel `k` as points, nudged 0.002 px in so that code taking `int()` of it (text placement) gets `k` back
/// despite f32 rounding.
pub fn pt(k: i32, ppp: f32) -> f32 {
    ((k as f64 + 2e-3) / ppp as f64) as f32
}

/// A radius in points (the Python float, e.g. a key's `min(16.0, h / s * 0.3)`) to pixels, capped at half the
/// short side (None: fully round; neu `_radius`).
pub fn radius_px(w: i32, h: i32, r: Option<f64>, ppp: f32) -> i32 {
    let half = w.min(h) / 2;
    match r {
        None => half,
        Some(r) => half.min(pxd(r, sc(ppp))),
    }
}

/// Draw what `draw` puts on a pygame canvas (crate::raster::pg::Cover, in the screen's own pixel coordinates)
/// over the box `bx` (whole pixels) as a cached texture: pygame's anti-aliased circles and lines, which the
/// Python app draws straight onto the screen. `key` must name everything the drawing depends on.
pub(crate) fn cover(p: &Painter, key: u64, bx: PxRect, draw: impl FnOnce(&mut crate::raster::pg::Cover)) {
    if bx.w < 1 || bx.h < 1 {
        return;
    }
    let (tex, size) = texture(p.ctx(), Store::Small, key, || {
        let mut c = crate::raster::pg::Cover::at(bx.x, bx.y, bx.w, bx.h);
        draw(&mut c);
        c.image()
    });
    blit(p, tex, size, bx.x as f32, bx.y as f32, Color32::WHITE);
}

/// Draw a texture `size` pixels big with its top-left at physical (x, y).
pub fn blit(p: &Painter, tex: egui::TextureId, size: [usize; 2], x: f32, y: f32, tint: Color32) {
    let ppp = p.pixels_per_point();
    let rect = Rect::from_min_size(
        pos2(x / ppp, y / ppp),
        Vec2::new(size[0] as f32 / ppp, size[1] as f32 / ppp),
    );
    let mut mesh = Mesh::with_texture(tex);
    mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
    p.add(Shape::mesh(mesh));
}

/// How a patch maps onto its destination: corners of `c` pixels kept, the middle texel row/column stretched.
#[derive(Clone, Copy)]
struct Slice {
    cx: Option<usize>,
    cy: Option<usize>,
}

/// Stretch a patch over (x, y, w, h) physical pixels: nine-slice, three-slice or 1:1.
fn slice_mesh(
    p: &Painter,
    tex: egui::TextureId,
    patch: [usize; 2],
    s: Slice,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    tint: Color32,
) {
    let ppp = p.pixels_per_point();
    let segs = |c: Option<usize>, n: usize, total: i32| -> Vec<(f32, f32, i32, i32)> {
        match c {
            Some(c) => {
                let mid = (c as f32 + 0.5) / n as f32;
                vec![
                    (0.0, c as f32 / n as f32, 0, c as i32),
                    (mid, mid, c as i32, total - c as i32),
                    ((c + 1) as f32 / n as f32, 1.0, total - c as i32, total),
                ]
            }
            None => vec![(0.0, 1.0, 0, total)],
        }
    };
    let xs = segs(s.cx, patch[0], w);
    let ys = segs(s.cy, patch[1], h);
    let mut mesh = Mesh::with_texture(tex);
    for &(v0, v1, y0, y1) in &ys {
        if y1 <= y0 {
            continue;
        }
        for &(u0, u1, x0, x1) in &xs {
            if x1 <= x0 {
                continue;
            }
            let r = Rect::from_min_max(
                pos2((x + x0) as f32 / ppp, (y + y0) as f32 / ppp),
                pos2((x + x1) as f32 / ppp, (y + y1) as f32 / ppp),
            );
            mesh.add_rect_with_uv(r, Rect::from_min_max(pos2(u0, v0), pos2(u1, v1)), tint);
        }
    }
    p.add(Shape::mesh(mesh));
}

/// Build (or fetch) the patch for a shape `w` x `h` pixels whose corners need `c` pixels, and draw it stretched
/// over (x, y, w, h). `make(pw, ph)` rasterizes the shape at the patch size.
fn sliced(
    p: &Painter,
    key: impl Hash,
    c: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    tint: Color32,
    make: impl Fn(usize, usize) -> Image,
) {
    if w < 1 || h < 1 {
        return;
    }
    let (wu, hu) = (w as usize, h as usize);
    let (pw, ph, s) = if wu >= 2 * c && hu >= 2 * c {
        (
            2 * c + 1,
            2 * c + 1,
            Slice {
                cx: Some(c),
                cy: Some(c),
            },
        )
    } else if wu >= 2 * c {
        (
            2 * c + 1,
            hu,
            Slice {
                cx: Some(c),
                cy: None,
            },
        )
    } else if hu >= 2 * c {
        (
            wu,
            2 * c + 1,
            Slice {
                cx: None,
                cy: Some(c),
            },
        )
    } else {
        (wu, hu, Slice { cx: None, cy: None })
    };
    let k = key_of((key, pw, ph));
    let (tex, size) = texture(p.ctx(), Store::Patch, k, || make(pw, ph));
    slice_mesh(p, tex, size, s, x, y, w, h, tint);
}

// -- the face: an anti-aliased rounded rect with per-vertex colours ---------------------------------------------------

/// Fill a rounded rect (physical pixels, radius `rr`) with a colour per position, anti-aliased over one pixel like
/// the coverage masks. `uv`: map positions to texture coordinates over this rect (for art).
pub(crate) fn rrect_mesh(
    p: &Painter,
    r: PxRect,
    rr: f32,
    color: impl Fn(f32, f32) -> Color32,
    tex: Option<(egui::TextureId, PxRect)>,
) {
    if r.w < 1 || r.h < 1 {
        return;
    }
    let ppp = p.pixels_per_point();
    let (x0, y0, w, h) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
    let rr = rr.clamp(0.0, w.min(h) / 2.0);
    // corner centres, clockwise from the top left; arcs in screen angles (y down)
    let corners = [
        (x0 + rr, y0 + rr, 180.0f32),
        (x0 + w - rr, y0 + rr, 270.0),
        (x0 + w - rr, y0 + h - rr, 0.0),
        (x0 + rr, y0 + h - rr, 90.0),
    ];
    let n = ((rr * std::f32::consts::FRAC_PI_2 / 1.5).ceil() as usize).clamp(1, 48);
    let mut ring: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(4 * (n + 1)); // (cx, cy, cos, sin)
    for &(cx, cy, a0) in &corners {
        for i in 0..=n {
            let a = (a0 + 90.0 * i as f32 / n as f32).to_radians();
            ring.push((cx, cy, a.cos(), a.sin()));
        }
    }
    let mut mesh = match tex {
        Some((id, _)) => Mesh::with_texture(id),
        None => Mesh::default(),
    };
    let uv_of = |x: f32, y: f32| -> Pos2 {
        match tex {
            Some((_, u)) => pos2(
                (x - u.x as f32) / u.w.max(1) as f32,
                (y - u.y as f32) / u.h.max(1) as f32,
            ),
            None => egui::epaint::WHITE_UV,
        }
    };
    let vert = |mesh: &mut Mesh, x: f32, y: f32, c: Color32| {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: pos2(x / ppp, y / ppp),
            uv: uv_of(x, y),
            color: c,
        });
    };
    let (mx, my) = (x0 + w / 2.0, y0 + h / 2.0);
    vert(&mut mesh, mx, my, color(mx, my));
    let m = ring.len() as u32;
    // coverage is full half a pixel inside the edge and nothing half a pixel outside (as the coverage masks)
    let (ri, ro) = ((rr - 0.5).max(0.0), rr + 0.5);
    for &(cx, cy, cs, sn) in &ring {
        let (ix, iy) = (cx + ri * cs, cy + ri * sn);
        vert(&mut mesh, ix, iy, color(ix, iy));
    }
    for &(cx, cy, cs, sn) in &ring {
        vert(&mut mesh, cx + ro * cs, cy + ro * sn, Color32::TRANSPARENT);
    }
    for i in 0..m {
        let j = (i + 1) % m;
        mesh.add_triangle(0, 1 + i, 1 + j);
        mesh.add_triangle(1 + i, 1 + m + i, 1 + j);
        mesh.add_triangle(1 + m + i, 1 + m + j, 1 + j);
    }
    p.add(Shape::mesh(mesh));
}

/// The material's face colour at (x, y) in a w x h face: BASE_HI at the top left to BASE_LO at the bottom right
/// along the 145-degree diagonal (the other way round when sunk in).
pub(crate) fn face_color(r: PxRect, x: f32, y: f32, sunk: bool) -> Color32 {
    let th = t();
    let u = ((x - r.x as f32) / r.w.max(1) as f32).clamp(0.0, 1.0);
    let v = ((y - r.y as f32) / r.h.max(1) as f32).clamp(0.0, 1.0);
    let mut k = (0.57 * u + 0.43 * v).clamp(0.0, 1.0);
    if sunk {
        k = 1.0 - k;
    }
    let (a, b) = (th.base_hi, th.base_lo);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k + 0.5) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// What a surface's face is filled with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    /// The material, shaded along the diagonal.
    Base,
    Color(Color32),
}

// -- the material's faces, as the Python app builds them -------------------------------------------------------------
// neu `_get` makes a surface the size of the shape (and its margin) from a nine-slice of the patch, then lays the
// face's diagonal gradient over it (`_face_grad`: a 24 x 24 grid of what to add and what to take away, each
// smoothscaled up and blended in); `_blit` puts its opaque inside and its four edge bands. Here the patch is the
// same and so is every pixel of the edge bands (the corners, the shadows, the 1 px lights, with the gradient over
// them): they're built per size, and the inside, where the face is all there is, is a mesh with the gradient's
// colours (linear, within a level of the grid's).

/// The surfaces `_get` blits whole (neu.SMALL pixels or fewer).
const SMALL: i32 = 24000;

/// `_face_grad`'s grids in the current theme: what to add and what to take away, 24 x 24.
fn grad_grids(sunk: bool) -> (pg::Surf, pg::Surf) {
    let th = t();
    let n = 24;
    let f3 = |c: Color32| [c.r() as f32, c.g() as f32, c.b() as f32];
    let (hi, lo, mid) = (f3(th.base_hi), f3(th.base_lo), f3(th.base));
    let mut add = pg::Surf::new(n, n);
    let mut sub = pg::Surf::new(n, n);
    for y in 0..n {
        for x in 0..n {
            // np.linspace(0, 1, 24) in f32, t = clip(0.57 xs + 0.43 ys)
            let (xs, ys) = (x as f32 / (n - 1) as f32, y as f32 / (n - 1) as f32);
            let mut tt = (0.57 * xs + 0.43 * ys).clamp(0.0, 1.0);
            if sunk {
                tt = 1.0 - tt;
            }
            let d: [f32; 3] = std::array::from_fn(|i| (hi[i] - mid[i]) + (lo[i] - hi[i]) * tt);
            let a: [u8; 3] = std::array::from_fn(|i| (d[i] + 0.5).clamp(0.0, 255.0) as u8);
            let b: [u8; 3] = std::array::from_fn(|i| (-d[i] + 0.5).clamp(0.0, 255.0) as u8);
            add.px[(y * n + x) as usize] = [a[0], a[1], a[2], 255];
            sub.px[(y * n + x) as usize] = [b[0], b[1], b[2], 255];
        }
    }
    (add, sub)
}

/// The gradient over a face `w` x `h` (smoothscale's filters): across once for the whole face, down per pixel.
struct FaceGrad {
    h: i32,
    add: pg::Surf,
    sub: pg::Surf,
    /// Down still to do (taller than the grid: the bilinear expand, row by row as asked).
    down: bool,
}

impl FaceGrad {
    fn new(w: i32, h: i32, sunk: bool) -> FaceGrad {
        let (ga, gs) = grad_grids(sunk);
        if h > 24 {
            FaceGrad {
                h,
                add: ga.smoothscale(w, 24),
                sub: gs.smoothscale(w, 24),
                down: true,
            }
        } else {
            FaceGrad {
                h,
                add: ga.smoothscale(w, h),
                sub: gs.smoothscale(w, h),
                down: false,
            }
        }
    }

    /// (what to add, what to take away) at the face's pixel (x, y).
    fn at(&self, x: i32, y: i32) -> ([u8; 3], [u8; 3]) {
        let pick = |s: &pg::Surf| -> [u8; 3] {
            if !self.down {
                let p = s.at(x, y);
                return [p[0], p[1], p[2]];
            }
            // filter_expand_Y_SSE2 for one pixel
            let i = y * 23 / self.h;
            let m1 = (0x100 * ((y * 23) % self.h) / self.h) as u16;
            let m0 = 0x100 - m1;
            let (a, b) = (s.at(x, i), s.at(x, i + 1));
            std::array::from_fn(|c| (((a[c] as u16) * m0 + (b[c] as u16) * m1) >> 8) as u8)
        };
        (pick(&self.add), pick(&self.sub))
    }
}

/// `_get`'s patch for a surface `sw` x `sh` with corners `c`: its size, and whether each axis is sliced.
fn patch_plan(sw: i32, sh: i32, c: i32) -> (usize, usize, bool, bool) {
    let n = (2 * c + 1) as usize;
    if sw >= 2 * c && sh >= 2 * c {
        (n, n, true, true)
    } else if sw >= 2 * c {
        (n, sh as usize, true, false)
    } else if sh >= 2 * c {
        (sw as usize, n, false, true)
    } else {
        (sw as usize, sh as usize, false, false)
    }
}

/// `_nine` / `_three` along one axis: the patch's index for the surface's `v` of `n` (corners `c`).
fn slice_at(v: i32, n: i32, c: i32, sliced: bool) -> i32 {
    if !sliced || v < c {
        v
    } else if v >= n - c {
        v - (n - c) + c + 1
    } else {
        c
    }
}

/// A material-faced surface `sw` x `sh` with its top left at (x0, y0) (pixels): `_get` with the face's gradient
/// over `face` (in the surface), then `_blit` (corners `c`, edge bands `band` wide). `make(pw, ph)` makes the
/// patch, every layer and the face's flat colour in it.
#[allow(clippy::too_many_arguments)]
fn base_surface(
    p: &Painter,
    key: u64,
    (x0, y0): (i32, i32),
    (sw, sh): (i32, i32),
    c: i32,
    band: i32,
    face: PxRect,
    sunk: bool,
    make: &dyn Fn(usize, usize) -> Image,
) {
    if sw < 1 || sh < 1 {
        return;
    }
    let ppp = p.pixels_per_point();
    let (pw, ph, slx, sly) = patch_plan(sw, sh, c);
    let patch: std::cell::OnceCell<std::sync::Arc<Image>> = std::cell::OnceCell::new();
    let grad: std::cell::OnceCell<FaceGrad> = std::cell::OnceCell::new();
    let pkey = key_of((key, pw, ph, theme::generation(), ppp.to_bits()));
    let pixel = |x: i32, y: i32| -> [u8; 4] {
        let img = patch.get_or_init(|| cpu_patch(pkey, || make(pw, ph)));
        let (px_, py_) = (slice_at(x, sw, c, slx), slice_at(y, sh, c, sly));
        let i = (py_ as usize * img.w + px_ as usize) * 4;
        let mut o = [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]];
        let (fx, fy) = (x - face.x, y - face.y);
        if fx >= 0 && fy >= 0 && fx < face.w && fy < face.h {
            let (a, s) = grad
                .get_or_init(|| FaceGrad::new(face.w, face.h, sunk))
                .at(fx, fy);
            for ch in 0..3 {
                o[ch] = (o[ch] as i32 + a[ch] as i32)
                    .min(255)
                    .saturating_sub(s[ch] as i32)
                    .max(0) as u8;
            }
        }
        o
    };
    let whole = sw * sh <= SMALL || 2 * band >= sw.min(sh);
    let parts: Vec<PxRect> = if whole {
        vec![PxRect {
            x: 0,
            y: 0,
            w: sw,
            h: sh,
        }]
    } else {
        vec![
            PxRect {
                x: 0,
                y: 0,
                w: sw,
                h: band,
            },
            PxRect {
                x: 0,
                y: sh - band,
                w: sw,
                h: band,
            },
            PxRect {
                x: 0,
                y: band,
                w: band,
                h: sh - 2 * band,
            },
            PxRect {
                x: sw - band,
                y: band,
                w: band,
                h: sh - 2 * band,
            },
        ]
    };
    for (n, part) in parts.iter().enumerate() {
        let k = key_of((key, sw, sh, face, sunk, n, theme::generation()));
        let (tex, size) = texture(p.ctx(), Store::Face, k, || {
            let mut img = Image::new(part.w as usize, part.h as usize);
            for y in 0..part.h {
                for x in 0..part.w {
                    let i = ((y * part.w + x) * 4) as usize;
                    img.rgba[i..i + 4].copy_from_slice(&pixel(part.x + x, part.y + y));
                }
            }
            img
        });
        blit(
            p,
            tex,
            size,
            (x0 + part.x) as f32,
            (y0 + part.y) as f32,
            Color32::WHITE,
        );
    }
    if !whole {
        // the inside: the face and nothing else, its gradient's colours at the pixels' corners (the grid's value
        // for pixel x is the one at x / w)
        let fr = PxRect {
            x: x0 + face.x,
            y: y0 + face.y,
            w: face.w,
            h: face.h,
        };
        let (ax, ay, bx, by) = (x0 + band, y0 + band, x0 + sw - band, y0 + sh - band);
        let mut mesh = Mesh::default();
        for (vx, vy) in [(ax, ay), (bx, ay), (bx, by), (ax, by)] {
            let col = face_color(fr, vx as f32 - 0.5, vy as f32 - 0.5, sunk);
            mesh.colored_vertex(pos2(vx as f32 / ppp, vy as f32 / ppp), col);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        p.add(Shape::mesh(mesh));
    }
}

/// `_make_raised`: the patch with both shadows, the face's flat `fill` and the 1 px lights.
fn make_raised(
    pw: usize,
    ph: usize,
    rr: i32,
    rec: (i32, i32, i32, f64, f64),
    m: i32,
    fill: Color32,
) -> Image {
    let th = t();
    let (oh, sx, sy, sgh, sgs) = rec;
    let shape = Mask::rrect(
        (pw as i32 - 2 * m) as f32,
        (ph as i32 - 2 * m) as f32,
        rr as f32,
        m as f32,
        m as f32,
        pw,
        ph,
    );
    let mut l = Layers::new(pw, ph);
    l.over_c(th.shade, &shape.blur64(sgs).shift(sx, sy).scale(th.sh_a));
    l.over_c(th.hilite, &shape.blur64(sgh).shift(-oh, -oh).scale(th.hi_a));
    l.over_c(fill, &shape);
    l.over_c(th.rim, &shape.minus(&shape.shift(1, 1)).scale(th.rim_a));
    l.over_c(th.rim_lo, &shape.minus(&shape.shift(-1, -1)).scale(th.rimlo_a));
    l.image(None)
}

/// `_make_inset`: the patch with its flat `fill` and both inner shadows, clipped to the shape.
fn make_inset(pw: usize, ph: usize, rr: i32, rec: (i32, i32, f64, f64), fill: Color32) -> Image {
    let th = t();
    let (osh, ohi, sgs, sgh) = rec;
    let pad = (osh as f64 + 3.0 * sgs).max(ohi as f64 + 3.0 * sgh).ceil() as i32 + 1;
    let (bw, bh) = (pw + 2 * pad as usize, ph + 2 * pad as usize);
    let big = Mask::rrect(pw as f32, ph as f32, rr as f32, pad as f32, pad as f32, bw, bh);
    let p0 = pad as usize;
    let dark = big
        .blur64(sgs)
        .shift(osh, osh)
        .crop(p0, p0, pw, ph)
        .map(|v| 1.0 - v);
    let light = big
        .blur64(sgh)
        .shift(-ohi, -ohi)
        .crop(p0, p0, pw, ph)
        .map(|v| 1.0 - v);
    let mut l = Layers::new(pw, ph);
    l.over_all(crate::raster::rgbf(fill), 1.0);
    l.over_c(th.hilite, &light.scale(th.hi_a));
    l.over_c(th.shade, &dark.scale(th.sh_a));
    l.image(Some(&big.crop(p0, p0, pw, ph)))
}

/// `_make_float`: the panel's drop and contact shadows, its 1 px edge, its face and the light along its top.
fn make_float(pw: usize, ph: usize, rr: i32, rec: (i32, f64, i32, f64, i32), m: i32) -> Image {
    let th = t();
    let (cdy, csg, ddy, dsg, ins) = rec;
    let (iw, ih) = (pw as i32 - 2 * m, ph as i32 - 2 * m);
    let shape = Mask::rrect(iw as f32, ih as f32, rr as f32, m as f32, m as f32, pw, ph);
    let drop = Mask::rrect(
        (iw - 2 * ins) as f32,
        (ih - 2 * ins) as f32,
        (rr - ins).max(0) as f32,
        (m + ins) as f32,
        (m + ins) as f32,
        pw,
        ph,
    );
    let edge = Mask::rrect(
        (iw + 2) as f32,
        (ih + 2) as f32,
        (rr + 1) as f32,
        (m - 1) as f32,
        (m - 1) as f32,
        pw,
        ph,
    );
    let mut l = Layers::new(pw, ph);
    l.over_c(th.float_sh, &drop.blur64(dsg).shift(0, ddy).scale(th.f_sh2_a));
    l.over_c(th.float_sh, &shape.blur64(csg).shift(0, cdy).scale(th.f_sh1_a));
    l.over_c(th.float_edge, &edge.minus(&shape).scale(th.f_edge_a));
    l.over_c(th.base, &shape);
    l.over_c(th.float_rim, &shape.minus(&shape.shift(0, 1)).scale(th.f_rim_a));
    l.image(None)
}

// -- recipes --------------------------------------------------------------------------------------------------------

/// `int(round(v * s))` of a value Python works out in f64 (`px(0.62 * depth)`).
fn pxd(v: f64, s: f64) -> i32 {
    (v * s).round_ties_even() as i32
}

/// (light offset, dark dx, dark dy, light sigma, dark sigma) in pixels: -5/-5 blur 12 and 6/7 blur 14 at depth 8
/// (cards), -3 blur 6 and 3/4 blur 8 at depth 4 (buttons).
fn rec_raised(depth: f32, s: f64) -> (i32, i32, i32, f64, f64) {
    let d = depth as f64;
    (
        pxd(0.62 * d, s).max(1),
        pxd(0.75 * d, s).max(1),
        pxd(0.88 * d, s).max(1),
        (0.75 * d * s).max(0.6),
        (0.875 * d * s).max(0.6),
    )
}

/// (dark offset, light offset, dark sigma, light sigma): 4/4 blur 9 and -3/-3 blur 8 at depth 6 (wells), 2/2 blur 4 at
/// depth 3 (grooves). Screens blur a little more.
fn rec_inset(depth: f32, s: f64, screen: bool) -> (i32, i32, f64, f64) {
    let d = depth as f64;
    (
        pxd(0.67 * d, s).max(1),
        pxd(0.5 * d, s).max(1),
        ((if screen { 0.83 } else { 0.75 }) * d * s).max(0.6),
        ((if screen { 0.58 } else { 0.67 }) * d * s).max(0.6),
    )
}

/// A shape standing up out of the material: light shadow top left, dark shadow bottom right, a 1 px light along
/// its top-left edge, and a face (the material shaded along the diagonal, or `fill`). `lift` raises it (points).
pub fn raised(p: &Painter, rect: Rect, r: Option<f32>, depth: f32, fill: Fill, lift: f32) {
    let ppp = p.pixels_per_point();
    raised_px(
        p,
        PxRect::of(rect, ppp),
        r.map(f64::from),
        depth,
        fill,
        px(lift, ppp),
    );
}

/// `raised` on a rect in whole pixels, `lift` pixels up.
pub(crate) fn raised_px(p: &Painter, pr: PxRect, r: Option<f64>, depth: f32, fill: Fill, lift: i32) {
    let ppp = p.pixels_per_point();
    let s64 = sc(ppp);
    if pr.w < 1 || pr.h < 1 {
        return;
    }
    let rr = radius_px(pr.w, pr.h, r, ppp);
    let rec = rec_raised(depth, s64);
    let (oh, sx, sy, sgh, sgs) = rec;
    let m = ((oh as f64 + 3.0 * sgh).max(sx.max(sy) as f64 + 3.0 * sgs)).ceil() as i32 + 1;
    let pr = pr.moved(0, -lift);
    let c = rr + 2 * m + 1;
    let key = (
        "raised",
        rr,
        oh,
        sx,
        sy,
        sgh.to_bits(),
        sgs.to_bits(),
        m,
        theme::generation(),
    );
    let (x0, y0, sw, sh) = (pr.x - m, pr.y - m, pr.w + 2 * m, pr.h + 2 * m);
    match fill {
        Fill::Base => base_surface(
            p,
            key_of(key),
            (x0, y0),
            (sw, sh),
            c,
            m + rr,
            PxRect {
                x: m,
                y: m,
                w: pr.w,
                h: pr.h,
            },
            false,
            &|pw, ph| make_raised(pw, ph, rr, rec, m, t().base),
        ),
        Fill::Color(col) => sliced(
            p,
            (key, ck(col)),
            c as usize,
            x0,
            y0,
            sw,
            sh,
            Color32::WHITE,
            |pw, ph| make_raised(pw, ph, rr, rec, m, col),
        ),
    }
}

/// A shape pressed into the material: inner shadow along the top-left edges, inner light along the bottom right.
pub fn inset(p: &Painter, rect: Rect, r: Option<f32>, depth: f32, fill: Fill) {
    inset_px(
        p,
        PxRect::of(rect, p.pixels_per_point()),
        r.map(f64::from),
        depth,
        fill,
    );
}

/// `inset` on a rect in whole pixels.
pub(crate) fn inset_px(p: &Painter, pr: PxRect, r: Option<f64>, depth: f32, fill: Fill) {
    sunk(p, pr, r, depth, fill, false);
}

/// A readout's screen: sunk in, darker than the material and flat (the tab, the tiles, values).
pub fn display(p: &Painter, rect: Rect, r: Option<f32>, depth: f32) {
    display_px(p, PxRect::of(rect, p.pixels_per_point()), r.map(f64::from), depth);
}

/// `display` on a rect in whole pixels.
pub(crate) fn display_px(p: &Painter, pr: PxRect, r: Option<f64>, depth: f32) {
    sunk(p, pr, r, depth, Fill::Color(t().screen), true);
}

fn sunk(p: &Painter, pr: PxRect, r: Option<f64>, depth: f32, fill: Fill, is_screen: bool) {
    let ppp = p.pixels_per_point();
    if pr.w < 1 || pr.h < 1 {
        return;
    }
    let rr = radius_px(pr.w, pr.h, r, ppp);
    let rec = rec_inset(depth, sc(ppp), is_screen);
    let (osh, ohi, sgs, sgh) = rec;
    let reach = (osh as f64 + 3.0 * sgs).max(ohi as f64 + 3.0 * sgh).ceil() as i32;
    let c = rr + reach + 2;
    let key = (
        "inset",
        rr,
        osh,
        ohi,
        sgs.to_bits(),
        sgh.to_bits(),
        theme::generation(),
    );
    match fill {
        Fill::Base => base_surface(
            p,
            key_of(key),
            (pr.x, pr.y),
            (pr.w, pr.h),
            c,
            c,
            PxRect {
                x: 0,
                y: 0,
                w: pr.w,
                h: pr.h,
            },
            true,
            &|pw, ph| make_inset(pw, ph, rr, rec, t().base),
        ),
        Fill::Color(col) => sliced(
            p,
            (key, ck(col)),
            c as usize,
            pr.x,
            pr.y,
            pr.w,
            pr.h,
            Color32::WHITE,
            |pw, ph| make_inset(pw, ph, rr, rec, col),
        ),
    }
}

/// Cards, panels, dialogs: raised, radius 22.
pub fn card(p: &Painter, rect: Rect, r: f32, lifted: bool) {
    raised(p, rect, Some(r), if lifted { 12.0 } else { 8.0 }, Fill::Base, 0.0);
}

/// The sidebar, the controls' modules, chip groups: sunk in, radius 18-24.
pub fn well(p: &Painter, rect: Rect, r: f32) {
    inset(p, rect, Some(r), 6.0, Fill::Base);
}

/// An anti-aliased outline following a rounded shape: inside its edge, or around it (focus rings).
pub fn ring(p: &Painter, rect: Rect, r: Option<f32>, width: f32, color: Color32, outside: bool) {
    ring_px(
        p,
        PxRect::of(rect, p.pixels_per_point()),
        r.map(f64::from),
        width,
        color,
        outside,
    );
}

/// `ring` round a rect in whole pixels.
pub(crate) fn ring_px(p: &Painter, pr: PxRect, r: Option<f64>, width: f32, color: Color32, outside: bool) {
    let s = p.pixels_per_point();
    if pr.w < 2 || pr.h < 2 {
        return;
    }
    let rr = radius_px(pr.w, pr.h, r, s);
    let tk = px(width, s).max(1);
    let (x, y, w, h) = if outside {
        (pr.x - tk, pr.y - tk, pr.w + 2 * tk, pr.h + 2 * tk)
    } else {
        (pr.x, pr.y, pr.w, pr.h)
    };
    let c = (rr + 2 * tk + 2) as usize;
    let key = ("ring", rr, tk, ck(color), outside);
    sliced(p, key, c, x, y, w, h, Color32::WHITE, |pw, ph| {
        let (outer, inner) = if outside {
            (
                Mask::rrect_full(pw, ph, (rr + tk) as f32),
                Mask::rrect(
                    (pw as i32 - 2 * tk) as f32,
                    (ph as i32 - 2 * tk) as f32,
                    rr as f32,
                    tk as f32,
                    tk as f32,
                    pw,
                    ph,
                ),
            )
        } else {
            (
                Mask::rrect_full(pw, ph, rr as f32),
                Mask::rrect(
                    (pw as i32 - 2 * tk) as f32,
                    (ph as i32 - 2 * tk) as f32,
                    (rr - tk).max(0) as f32,
                    tk as f32,
                    tk as f32,
                    pw,
                    ph,
                ),
            )
        };
        let mut l = Layers::new(pw, ph);
        l.over_c(color, &outer.minus(&inner));
        l.image(None)
    });
}

/// A flat bar with round ends (across or upright), anti-aliased (`neu.pill`: pygame's rect and aacircles).
pub fn pill(p: &Painter, rect: Rect, color: Color32) {
    pill_px(p, PxRect::of(rect, p.pixels_per_point()), color);
}

/// `pill` on a rect in whole pixels.
pub(crate) fn pill_px(p: &Painter, r: PxRect, color: Color32) {
    use crate::raster::pg;
    if r.w < 1 || r.h < 1 {
        return;
    }
    let c = pg::rgb(color);
    let key = key_of(("pill", r.w, r.h, ck(color)));
    cover(
        p,
        key,
        PxRect {
            x: r.x - 2,
            y: r.y - 2,
            w: r.w + 4,
            h: r.h + 4,
        },
        |cv| {
            let rad = r.w.min(r.h) as f64 / 2.0;
            let (x, y, w, h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
            let ir = rad as i32 as f64;
            if r.w == r.h {
                let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
                pg::aacircle(cv, c, (cx as f64, cy as f64), rad, 0, None);
            } else if r.w > r.h {
                pg::rect(cv, c, (x + ir, y, w - 2.0 * ir, h), 0, 0);
                pg::aacircle(cv, c, (x + rad, y + rad), rad, 0, None);
                pg::aacircle(cv, c, (x + w - rad, y + rad), rad, 0, None);
            } else {
                pg::rect(cv, c, (x, y + ir, w, h - 2.0 * ir), 0, 0);
                pg::aacircle(cv, c, (x + rad, y + rad), rad, 0, None);
                pg::aacircle(cv, c, (x + rad, y + h - rad), rad, 0, None);
            }
        },
    );
}

/// A flat rounded rect (`pygame.draw.rect(screen, color, rect, border_radius=r)`, r in points made whole pixels
/// as `int(r * s)`).
pub fn rrect(p: &Painter, rect: Rect, r: f32, color: Color32) {
    let ppp = p.pixels_per_point();
    rrect_px(p, PxRect::of(rect, ppp), ipx(r, ppp), color);
}

/// `pygame.draw.rect(screen, color, r, border_radius=radius)` on whole pixels.
pub(crate) fn rrect_px(p: &Painter, r: PxRect, radius: i32, color: Color32) {
    use crate::raster::pg;
    if r.w < 1 || r.h < 1 {
        return;
    }
    let c = pg::rgb(color);
    let key = key_of(("rrect", r.w, r.h, radius, ck(color)));
    cover(p, key, r, |cv| {
        pg::rect(cv, c, (r.x as f64, r.y as f64, r.w as f64, r.h as f64), 0, radius);
    });
}

/// A small anti-aliased filled circle (`neu.dot`: pygame's aacircle at the rounded centre; the radius in points,
/// `int(radius * s)` pixels as the screens pass it, at least 1).
pub fn dot(p: &Painter, center: Pos2, radius: f32, color: Color32) {
    let ppp = p.pixels_per_point();
    let round = |v: f32| (v as f64 * ppp as f64).round_ties_even() as i32;
    dot_px(
        p,
        (round(center.x), round(center.y)),
        ipx(radius, ppp).max(1),
        color,
    );
}

/// `neu.dot` at a whole-pixel centre and radius.
pub(crate) fn dot_px(p: &Painter, (cx, cy): (i32, i32), r: i32, color: Color32) {
    use crate::raster::pg;
    let r = r.max(1);
    let c = pg::rgb(color);
    let key = key_of(("dot", r, ck(color)));
    let bx = PxRect {
        x: cx - r - 2,
        y: cy - r - 2,
        w: 2 * r + 5,
        h: 2 * r + 5,
    };
    cover(p, key, bx, |cv| {
        pg::aacircle(cv, c, (cx as f64, cy as f64), r as f64, 0, None);
    });
}

/// Tint rect towards color by alpha (a loop's bars, a selection): `neu.wash`, a multiply by 1 - alpha and an add of
/// the colour times alpha.
pub fn wash(p: &Painter, rect: Rect, color: Color32, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let alpha = alpha.min(1.0);
    let k = (255.0 * (1.0 - alpha) + 0.5) as u8;
    let add = |c: u8| (c as f32 * alpha + 0.5) as u8;
    let pr = PxRect::of(rect, p.pixels_per_point());
    if pr.w < 1 || pr.h < 1 {
        return;
    }
    p.rect_filled(
        pr.to_rect(p.pixels_per_point()),
        0.0,
        Color32::from_rgba_premultiplied(add(color.r()), add(color.g()), add(color.b()), 255 - k),
    );
}

/// A soft translucent disc behind a lit spot (the note to play on the neck). radius in points (`int(radius * s)`
/// pixels, at least 2), alpha 0..255; its box at `int(center) - radius`.
pub fn halo(p: &Painter, center: Pos2, radius: f32, color: Color32, alpha: u8) {
    let ppp = p.pixels_per_point();
    let rr = (radius as f64 * sc(ppp)).max(2.0) as usize;
    let key = key_of(("halo", rr, ck(color), alpha));
    let (tex, size) = texture(p.ctx(), Store::Small, key, || {
        let m = Mask::rrect_full(2 * rr, 2 * rr, rr as f32).scale(alpha as f32 / 255.0);
        let mut l = Layers::new(2 * rr, 2 * rr);
        l.over_c(color, &m);
        l.image(None)
    });
    let (x, y) = (ipx(center.x, ppp) - rr as i32, ipx(center.y, ppp) - rr as i32);
    blit(p, tex, size, x as f32, y as f32, Color32::WHITE);
}

/// A soft light around a lit thing (the cursor, a lit note, an LED): its shape blurred, behind it.
pub fn glow(p: &Painter, rect: Rect, color: Color32, spread: f32, alpha: Option<f32>, r: Option<f32>) {
    glow_px(
        p,
        PxRect::of(rect, p.pixels_per_point()),
        color,
        spread,
        alpha,
        r.map(f64::from),
    );
}

/// `glow` of a rect in whole pixels.
pub(crate) fn glow_px(
    p: &Painter,
    pr: PxRect,
    color: Color32,
    spread: f32,
    alpha: Option<f32>,
    r: Option<f64>,
) {
    let s = p.pixels_per_point();
    if pr.w < 1 || pr.h < 1 {
        return;
    }
    let a = alpha.unwrap_or(t().glow_a);
    let sg = (px(spread, s) as f64 / 2.0).max(0.8);
    let m = glow_margin(sg);
    let rr = radius_px(pr.w, pr.h, r, s);
    let key = key_of(("glow", pr.w, pr.h, rr, sg.to_bits(), ck(color), fk(a)));
    let (tex, size) = texture(p.ctx(), Store::Small, key, || {
        make_glow(pr.w, pr.h, rr, sg, a, color)
    });
    blit(p, tex, size, (pr.x - m) as f32, (pr.y - m) as f32, Color32::WHITE);
}

fn glow_margin(sg: f64) -> i32 {
    (3.0 * sg).ceil() as i32 + 1
}

/// The glow's surface: the `w` x `h` shape (radius `rr`) blurred by `sg`, in `color` at `a`, with its margin.
fn make_glow(w: i32, h: i32, rr: i32, sg: f64, a: f32, color: Color32) -> Image {
    let m = glow_margin(sg);
    let (cw, ch) = ((w + 2 * m) as usize, (h + 2 * m) as usize);
    let mask = Mask::rrect(w as f32, h as f32, rr as f32, m as f32, m as f32, cw, ch)
        .blur64(sg)
        .scale(a);
    let mut img = Image::new(cw, ch);
    for (i, v) in mask.v.iter().enumerate() {
        img.rgba[i * 4..i * 4 + 4].copy_from_slice(&[
            color.r(),
            color.g(),
            color.b(),
            (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8,
        ]);
    }
    img
}

/// A small lit indicator (a selected key's light, a module's power light): a dot with a faint glow (`neu.led`:
/// `d = max(3, px(size))` centred on `int(center)`, the glow of that square, pygame's aacircle and a darker lower
/// rim, a quarter-circle pair `d // 7` wide sitting `d / 6` lower). size in points.
pub fn led(p: &Painter, center: Pos2, size: f32, color: Color32) {
    let ppp = p.pixels_per_point();
    led_px(p, ipx(center.x, ppp), ipx(center.y, ppp), size, color);
}

/// `led` centred on whole pixels.
pub(crate) fn led_px(p: &Painter, cx: i32, cy: i32, size: f32, color: Color32) {
    use crate::raster::pg;
    let s = sc(p.pixels_per_point());
    let d = pxr(size, s).max(3);
    let r = PxRect {
        x: cx - d / 2,
        y: cy - d / 2,
        w: d,
        h: d,
    };
    glow_px(p, r, color, 5.0, None, None);
    let dark = [
        (color.r() as f32 * 0.8) as u8,
        (color.g() as f32 * 0.8) as u8,
        (color.b() as f32 * 0.8) as u8,
        255,
    ];
    let c = pg::rgb(color);
    let key = key_of(("led", d, ck(color)));
    let bx = PxRect {
        x: cx - d,
        y: cy - d,
        w: 2 * d + 1,
        h: 2 * d + 2,
    };
    cover(p, key, bx, |cv| {
        pg::aacircle(cv, c, (cx as f64, cy as f64), d as f64 / 2.0, 0, None);
        pg::aacircle(
            cv,
            dark,
            (cx as f64, cy as f64 + d as f64 / 6.0),
            d as f64 / 2.0,
            (d / 7).max(1),
            Some([false, false, true, true]),
        );
    });
}

/// Darken the page under a dialog (the theme's scrim; `alpha` 0-255 overrides its strength).
pub fn dim(p: &Painter, rect: Rect, alpha: Option<u8>) {
    let th = t();
    let a = match alpha {
        None => (th.scrim_a * 255.0) as i32,
        Some(a) => (a as f32 * th.scrim_a / 0.6) as i32,
    }
    .clamp(0, 255) as u8;
    p.rect_filled(
        rect,
        0.0,
        Color32::from_rgba_unmultiplied(th.scrim.r(), th.scrim.g(), th.scrim.b(), a),
    );
}

/// (contact dy, contact sigma, drop dy, drop sigma, drop inset) in pixels: 0 2px 4px, 0 18px 36px -12px.
fn rec_float(s: f64) -> (i32, f64, i32, f64, i32) {
    (
        pxd(2.0, s).max(1),
        (2.0 * s).max(0.8),
        pxd(18.0, s).max(2),
        (12.0 * s).max(2.0),
        pxd(12.0, s).max(0),
    )
}

/// A panel floating above the page (settings, the results card, dialogs): its face, a 1 px edge and one tight drop
/// shadow under it, with no light shadow, so the page around it stays clear.
pub fn floating(p: &Painter, rect: Rect, r: Option<f32>) {
    floating_px(p, PxRect::of(rect, p.pixels_per_point()), r.map(f64::from));
}

/// `floating` on a rect in whole pixels.
pub(crate) fn floating_px(p: &Painter, pr: PxRect, r: Option<f64>) {
    let s = p.pixels_per_point();
    if pr.w < 1 || pr.h < 1 {
        return;
    }
    let rr = radius_px(pr.w, pr.h, r, s);
    let rec = rec_float(sc(s));
    let (cdy, csg, ddy, dsg, ins) = rec;
    let m = (ddy as f64 + 3.0 * dsg).ceil() as i32 + 2;
    let key = (
        "float",
        rr,
        cdy,
        csg.to_bits(),
        ddy,
        dsg.to_bits(),
        ins,
        m,
        theme::generation(),
    );
    base_surface(
        p,
        key_of(key),
        (pr.x - m, pr.y - m),
        (pr.w + 2 * m, pr.h + 2 * m),
        rr + 2 * m + 1,
        m + rr,
        PxRect {
            x: m,
            y: m,
            w: pr.w,
            h: pr.h,
        },
        false,
        &|pw, ph| make_float(pw, ph, rr, rec, m),
    );
}

/// Album art (a texture) in a sunken frame with rounded, anti-aliased corners (just the frame while there's none).
pub fn art(p: &Painter, tex: Option<egui::TextureId>, rect: Rect, r: f32, frame: bool) {
    if frame {
        inset(p, rect, Some(r), 3.0, Fill::Base);
    }
    let Some(tex) = tex else { return };
    let s = p.pixels_per_point();
    let pr = PxRect::of(rect, s);
    let pad = if frame { px(4.0, s) } else { 0 };
    let inner = PxRect {
        x: pr.x + pad,
        y: pr.y + pad,
        w: pr.w - 2 * pad,
        h: pr.h - 2 * pad,
    };
    if inner.w < 2 || inner.h < 2 {
        return;
    }
    let rr = (radius_px(inner.w, inner.h, Some(r as f64), s) - pad / 2).max(1);
    rrect_mesh(p, inner, rr as f32, |_, _| Color32::WHITE, Some((tex, inner)));
}

/// Album art as `neu.blit_art` draws it: the sunken frame, then the picture made for its exact inner box (`get`
/// gets the texture for (width, height, corner radius) in pixels: smoothscaled and masked as the Python app makes
/// it), drawn one texel to a pixel.
pub fn art_with(
    p: &Painter,
    rect: Rect,
    r: f32,
    frame: bool,
    get: impl FnOnce(u32, u32, u32) -> Option<egui::TextureId>,
) {
    if frame {
        inset(p, rect, Some(r), 3.0, Fill::Base);
    }
    let s = p.pixels_per_point();
    let pr = PxRect::of(rect, s);
    let pad = if frame { px(4.0, s) } else { 0 };
    let (w, h) = (pr.w - 2 * pad, pr.h - 2 * pad);
    if w < 2 || h < 2 {
        return;
    }
    let rr = (radius_px(w, h, Some(r as f64), s) - pad / 2).max(1);
    if let Some(tex) = get(w as u32, h as u32, rr as u32) {
        let at = Rect::from_min_size(
            egui::pos2((pr.x + pad) as f32 / s, (pr.y + pad) as f32 / s),
            egui::vec2(w as f32 / s, h as f32 / s),
        );
        image(p, tex, at, Color32::WHITE);
    }
}

/// Band overlays for scroll areas that fade at their edges: `face` is the surface the content sits on; a band of
/// `size` points at the top and/or bottom of `area` is painted with that face's colours, solid at the edge and
/// clear inwards (smoothstep), over the content. Draw the surface's shadow layer again afterwards if it has one.
pub fn fade_edges(p: &Painter, area: Rect, top: bool, bottom: bool, size: f32, face: FaceKind) {
    let s = p.pixels_per_point();
    let ar = PxRect::of(area, s);
    let h = (px(size, s)).min(ar.h / 3).max(2);
    let fr = match face {
        FaceKind::Base { rect, .. } => PxRect::of(rect, s),
        FaceKind::Flat(_) => ar,
    };
    let color = |x: f32, y: f32| -> Color32 {
        match face {
            FaceKind::Base { sunk, .. } => face_color(fr, x, y, sunk),
            FaceKind::Flat(c) => c,
        }
    };
    let steps = 12;
    for (on, at_top) in [(top, true), (bottom, false)] {
        if !on {
            continue;
        }
        let y0 = if at_top { ar.y } else { ar.y + ar.h - h };
        let mut mesh = Mesh::default();
        for i in 0..=steps {
            let f = i as f32 / steps as f32;
            let y = y0 as f32 + f * h as f32;
            let mut k = if at_top { 1.0 - f } else { f };
            k = k * k * (3.0 - 2.0 * k);
            for x in [ar.x as f32, (ar.x + ar.w) as f32] {
                let c = color(x, y).gamma_multiply(k);
                mesh.colored_vertex(pos2(x / s, y / s), c);
            }
        }
        for i in 0..steps as u32 {
            let a = 2 * i;
            mesh.add_triangle(a, a + 1, a + 2);
            mesh.add_triangle(a + 1, a + 3, a + 2);
        }
        p.add(Shape::mesh(mesh));
    }
}

/// The surface under a fading scroll area.
#[derive(Clone, Copy, Debug)]
pub enum FaceKind {
    /// The material's face (a raised or sunken shape covering `rect`).
    Base {
        rect: Rect,
        sunk: bool,
    },
    Flat(Color32),
}

/// A full-window image (e.g. the blurred page behind a floating panel), stretched over `rect`.
pub fn image(p: &Painter, tex: egui::TextureId, rect: Rect, tint: Color32) {
    let mut mesh = Mesh::with_texture(tex);
    mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
    p.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeName;

    /// `_face_grad` as the Python app leaves a flat BASE face `w` x `h` (sunk: the other way round): the 24 x 24
    /// grid of what to add and what to take away, each smoothscaled up and blended in (BLEND_RGB_ADD / SUB).
    fn face_grad_exact(w: i32, h: i32, sunk: bool) -> Image {
        use crate::raster::pg::Surf;
        let th = t();
        let n = 24;
        let (hi, lo, mid) = (rgbf3(th.base_hi), rgbf3(th.base_lo), rgbf3(th.base));
        let mut add = Surf::new(n, n);
        let mut sub = Surf::new(n, n);
        for y in 0..n {
            for x in 0..n {
                let (xs, ys) = (x as f32 / (n - 1) as f32, y as f32 / (n - 1) as f32);
                let mut tt = (0.57 * xs + 0.43 * ys).clamp(0.0, 1.0);
                if sunk {
                    tt = 1.0 - tt;
                }
                let d: [f32; 3] = std::array::from_fn(|i| (hi[i] - mid[i]) + (lo[i] - hi[i]) * tt);
                let a: [u8; 3] = std::array::from_fn(|i| (d[i] + 0.5).clamp(0.0, 255.0) as u8);
                let b: [u8; 3] = std::array::from_fn(|i| (-d[i] + 0.5).clamp(0.0, 255.0) as u8);
                add.px[(y * n + x) as usize] = [a[0], a[1], a[2], 255];
                sub.px[(y * n + x) as usize] = [b[0], b[1], b[2], 255];
            }
        }
        let (add, sub) = (add.smoothscale(w, h), sub.smoothscale(w, h));
        let base = th.base;
        let mut img = Image::new(w as usize, h as usize);
        for i in 0..(w * h) as usize {
            for (ch, v) in [base.r(), base.g(), base.b()].iter().enumerate() {
                let up = (*v as i32 + add.px[i][ch] as i32).min(255);
                img.rgba[i * 4 + ch] = (up - sub.px[i][ch] as i32).max(0) as u8;
            }
            img.rgba[i * 4 + 3] = 255;
        }
        img
    }

    fn rgbf3(c: Color32) -> [f32; 3] {
        [c.r() as f32, c.g() as f32, c.b() as f32]
    }

    /// The patches, glows and the face's gradient against rsplay.neu's own (QRS_NEU_DUMPS, see keys.rs).
    #[test]
    fn neu_parity_surfaces() {
        let Ok(dir) = std::env::var("QRS_NEU_DUMPS") else {
            return;
        };
        let _lock = crate::raster::THEME_LOCK.lock();
        let s = 1440.0f64 / 1400.0;
        let mut bad = Vec::new();
        let mut n = 0;
        let mut grad_worst = Vec::new();
        for thn in ThemeName::ALL {
            theme::set_theme(thn);
            let th = t();
            let tk = thn.key();
            for (w, h, spread, r) in [
                (9, 9, 5.0f32, None),
                (160, 3, 4.0, None),
                (40, 22, 6.0, Some(11.0f64)),
            ] {
                let sg = (pxr(spread, s) as f64 / 2.0).max(0.8);
                let rr = match r {
                    None => w.min(h) / 2,
                    Some(r) => (w.min(h) / 2).min(pxd(r, s)),
                };
                let name = format!(
                    "{tk}_glow_{w}_{h}_{}_{}",
                    spread as i32,
                    r.map(|v| (v as i32).to_string()).unwrap_or_else(|| "None".into())
                );
                n += 1;
                let got = make_glow(w, h, rr, sg, th.glow_a, th.accent);
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &got) {
                    bad.push(e);
                }
            }
            for depth in [8.0f32, 4.0, 3.0, 12.0] {
                let (oh, sx, sy, sgh, sgs) = rec_raised(depth, s);
                let m = ((oh as f64 + 3.0 * sgh).max(sx.max(sy) as f64 + 3.0 * sgs)).ceil() as i32 + 1;
                for r in [22, 14, 8] {
                    let c = (r + 2 * m + 1) as usize;
                    let pw = 2 * c + 1;
                    let side = (pw as i32 - 2 * m) as f32;
                    let shape = Mask::rrect(side, side, r as f32, m as f32, m as f32, pw, pw);
                    let mut l = Layers::new(pw, pw);
                    l.over_c(th.shade, &shape.blur64(sgs).shift(sx, sy).scale(th.sh_a));
                    l.over_c(th.hilite, &shape.blur64(sgh).shift(-oh, -oh).scale(th.hi_a));
                    l.over_c(th.base, &shape);
                    l.over_c(th.rim, &shape.minus(&shape.shift(1, 1)).scale(th.rim_a));
                    l.over_c(th.rim_lo, &shape.minus(&shape.shift(-1, -1)).scale(th.rimlo_a));
                    n += 1;
                    let name = format!("{tk}_raised_{}_{r}", depth as i32);
                    if let Some(e) = crate::raster::tests_cmp(&dir, &name, &l.image(None)) {
                        bad.push(e);
                    }
                }
            }
            for (depth, scr) in [
                (6.0f32, false),
                (3.0, false),
                (2.0, false),
                (6.0, true),
                (3.0, true),
            ] {
                let (osh, ohi, sgs, sgh) = rec_inset(depth, s, scr);
                let reach = (osh as f64 + 3.0 * sgs).max(ohi as f64 + 3.0 * sgh).ceil() as i32;
                for r in [22, 18, 6] {
                    let c = (r + reach + 2) as usize;
                    let pw = 2 * c + 1;
                    let pad = reach + 1;
                    let bw = pw + 2 * pad as usize;
                    let big = Mask::rrect(pw as f32, pw as f32, r as f32, pad as f32, pad as f32, bw, bw);
                    let p0 = pad as usize;
                    let dark = big
                        .blur64(sgs)
                        .shift(osh, osh)
                        .crop(p0, p0, pw, pw)
                        .map(|v| 1.0 - v);
                    let light = big
                        .blur64(sgh)
                        .shift(-ohi, -ohi)
                        .crop(p0, p0, pw, pw)
                        .map(|v| 1.0 - v);
                    let mut l = Layers::new(pw, pw);
                    l.over_all(rgbf3(if scr { th.screen } else { th.base }), 1.0);
                    l.over_c(th.hilite, &light.scale(th.hi_a));
                    l.over_c(th.shade, &dark.scale(th.sh_a));
                    n += 1;
                    let name = format!("{tk}_inset_{}_{}_{r}", depth as i32, scr as u8);
                    let got = l.image(Some(&big.crop(p0, p0, pw, pw)));
                    if let Some(e) = crate::raster::tests_cmp(&dir, &name, &got) {
                        bad.push(e);
                    }
                }
            }
            let (cdy, csg, ddy, dsg, ins) = rec_float(s);
            let m = (ddy as f64 + 3.0 * dsg).ceil() as i32 + 2;
            for r in [28, 26] {
                let c = (r + 2 * m + 1) as usize;
                let pw = 2 * c + 1;
                let iw = pw as i32 - 2 * m;
                let shape = Mask::rrect(iw as f32, iw as f32, r as f32, m as f32, m as f32, pw, pw);
                let drop = Mask::rrect(
                    (iw - 2 * ins) as f32,
                    (iw - 2 * ins) as f32,
                    (r - ins).max(0) as f32,
                    (m + ins) as f32,
                    (m + ins) as f32,
                    pw,
                    pw,
                );
                let edge = Mask::rrect(
                    (iw + 2) as f32,
                    (iw + 2) as f32,
                    (r + 1) as f32,
                    (m - 1) as f32,
                    (m - 1) as f32,
                    pw,
                    pw,
                );
                let mut l = Layers::new(pw, pw);
                l.over_c(th.float_sh, &drop.blur64(dsg).shift(0, ddy).scale(th.f_sh2_a));
                l.over_c(th.float_sh, &shape.blur64(csg).shift(0, cdy).scale(th.f_sh1_a));
                l.over_c(th.float_edge, &edge.minus(&shape).scale(th.f_edge_a));
                l.over_c(th.base, &shape);
                l.over_c(th.float_rim, &shape.minus(&shape.shift(0, 1)).scale(th.f_rim_a));
                n += 1;
                let name = format!("{tk}_float_{r}");
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &l.image(None)) {
                    bad.push(e);
                }
            }
            for (w, h, sunk) in [(154, 99, false), (120, 45, true), (600, 170, false)] {
                n += 1;
                let name = format!("{tk}_grad_{w}_{h}_{}", sunk as u8);
                let exact = face_grad_exact(w, h, sunk);
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &exact) {
                    bad.push(e);
                }
                // how far the face mesh's colours (linear, rounded at its vertices) are from it
                let r = PxRect { x: 0, y: 0, w, h };
                let mut worst = 0;
                for y in 0..h {
                    for x in 0..w {
                        let c = face_color(r, x as f32 + 0.5, y as f32 + 0.5, sunk);
                        let i = ((y * w + x) * 4) as usize;
                        for (ch, v) in [c.r(), c.g(), c.b()].iter().enumerate() {
                            worst = worst.max((*v as i32 - exact.rgba[i + ch] as i32).abs());
                        }
                    }
                }
                grad_worst.push(format!("{name}: face_color within {worst}"));
            }
        }
        theme::set_theme(ThemeName::Midnight);
        eprintln!("neu surfaces parity: {n} cases; {}", grad_worst.join(", "));
        assert!(bad.is_empty(), "{} of {n} differ:\n{}", bad.len(), bad.join("\n"));
    }
}
