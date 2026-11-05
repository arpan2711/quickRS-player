//! The shared room (`qrs_ui::hall`, P34.5, D88) in the transport's baked art (`transport_art`): the hall reflected
//! in the obsidian gem's facets, the shell Go stones, the thumb and the seek bar's amber resin, by where each sits in
//! the window. New in quickRS; the tables are `assets/hall/manifest.json`'s (`gem`, `go_stone`, `thumb`, `resin`), the
//! research folder's `mockups/ui-main/tiles/room/tools/surfaces.py` the meshes they mirror.
//!
//! - The gem: one fan per facet of its cut (8 kites, 16 upper-girdle triangles, 8 stars, the table), each with the
//!   facet's normal, reflectance and the colour under it; additive at gain 6, the table at 8 % so the sign stays clean.
//! - The Go stones: a polar grid (64 segments) on the radial table; the mix blend (an add can't show on white shell).
//!   The forward key mirrors it.
//! - The thumb: a polar grid (56 x 19) sampling `thumb_nm.png` (normal in RGB, reflectance in A); the mix blend on the
//!   blurred room (its dish is satin).
//! - The resin: a grid along the fill, columns every 8 points and at each beat cut, rows from the cross-section table;
//!   no reflection in the cuts or past the lead. Additive at gain 2.5.
//!
//! Every mesh is built once in its sprite's space; a frame only works out its UVs (and an additive surface's colours)
//! for where it is, and keeps them until it moves.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Color32, Context, Mesh, Painter, Pos2, Rect, Shape, pos2};
use egui::epaint::Vertex;
use qrs_ui::hall::{self, Eye, Hall};

const MANIFEST: &str = include_str!("../../../../assets/hall/manifest.json");
const THUMB_NM: &[u8] = include_bytes!("../../../../assets/hall/thumb_nm.png");

/// Gains: the gem's and the resin's room (additive); the gem's table facet's share.
const GEM_GAIN: f32 = 6.0;
const RESIN_GAIN: f32 = 2.5;
const TABLE_K: f32 = 0.08;
/// The gem's cut (its 96 point canvas): girdle radius, table and star rings as fractions of it.
const GEM_C: f32 = 48.0;
const GEM_R: f32 = 31.5;
const TABLE_AT: f32 = 0.5;
const STAR_AT: f32 = 0.76;
/// The stone's canvas centre and segments; the thumb's centre, radius, radii and segments; the nm map's scale (6x,
/// P34.7).
const STONE_C: f32 = 32.0;
const STONE_SEG: usize = 64;
const THUMB_C: f32 = 20.0;
const THUMB_R: f32 = 13.2;
const THUMB_RADII: usize = 19;
const THUMB_SEG: usize = 56;
const NM_K: f32 = 6.0;
/// The feather ring outside each round surface (m 0: egui's anti-aliasing trick).
const FEATHER: f32 = 0.5;
/// The resin's columns (points) and the half-width of the columns either side of a cut.
const RESIN_COL: f32 = 8.0;
const CUT_HALF: f32 = 1.2;

/// A surface's mesh in its sprite's space (points at 1x, y down): per vertex its place, normal (screen frame),
/// reflectance and the colour under it.
#[derive(Clone, Debug, Default)]
pub struct Surf {
    pos: Vec<[f32; 2]>,
    n: Vec<[f32; 3]>,
    m: Vec<f32>,
    base: Vec<[f32; 3]>,
    lin: Vec<[f32; 3]>,
    idx: Vec<u32>,
}

impl Surf {
    fn push(&mut self, p: [f32; 2], n: [f32; 3], m: f32, base: [f32; 3]) {
        self.pos.push(p);
        self.n.push(n);
        self.m.push(m);
        self.lin.push(base.map(hall::lin));
        self.base.push(base);
    }

    /// The same surface mirrored left to right in a canvas `w` wide (the forward key).
    fn mirrored(&self, w: f32) -> Surf {
        Surf {
            pos: self.pos.iter().map(|p| [w - p[0], p[1]]).collect(),
            n: self.n.iter().map(|n| [-n[0], n[1], n[2]]).collect(),
            ..self.clone()
        }
    }

    pub fn len(&self) -> usize {
        self.pos.len()
    }
}

/// How a surface takes the room.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Blend {
    /// Added, at this gain.
    Add(f32),
    /// Mixed in: `out = fb (1 - a) + a room`.
    Mix,
}

fn f(v: &serde_json::Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

fn rgb(v: &serde_json::Value) -> [f32; 3] {
    std::array::from_fn(|i| f(&v[i]) / 255.0)
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// The gem's facets as polygons (canvas points, y down), in the manifest's order: for j = 0..7 the kite, two upper
/// girdle triangles and the star; then the table.
pub fn gem_facets() -> Vec<Vec<[f32; 2]>> {
    let pt = |deg: f32, r: f32| {
        let a = deg.to_radians();
        [GEM_C + r * GEM_R * a.cos(), GEM_C + r * GEM_R * a.sin()]
    };
    let (g, tb, st) = (|d| pt(d, 1.0), |d| pt(d, TABLE_AT), |d| pt(d, STAR_AT));
    let mut out = Vec::with_capacity(33);
    for j in 0..8 {
        let a = 22.5 + 45.0 * j as f32;
        let mid = a + 22.5;
        out.push(vec![tb(a), st(a - 22.5), g(a), st(a + 22.5)]);
        out.push(vec![st(mid), g(mid), g(a)]);
        out.push(vec![st(mid), g(mid), g(a + 45.0)]);
        out.push(vec![tb(a), tb(a + 45.0), st(mid)]);
    }
    out.push((0..8).map(|j| tb(22.5 + 45.0 * j as f32)).collect());
    out
}

/// The gem's mesh for one sign's table of facets: a fan per facet, the facet's normal, m and colour on all of it.
fn gem_surf(rows: &[serde_json::Value]) -> Surf {
    let mut s = Surf::default();
    for (poly, row) in gem_facets().iter().zip(rows) {
        let n = unit([f(&row["n"][0]), f(&row["n"][1]), f(&row["n"][2])]);
        let k = if poly.len() == 8 { TABLE_K } else { 1.0 };
        let (m, base) = (f(&row["m"]) * k, rgb(&row["base_srgb"]));
        let c = poly.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0], a[1] + p[1]]);
        let c = [c[0] / poly.len() as f32, c[1] / poly.len() as f32];
        let i0 = s.len() as u32;
        s.push(c, n, m, base);
        for p in poly {
            s.push(*p, n, m, base);
        }
        let k = poly.len() as u32;
        for i in 0..k {
            s.idx.extend_from_slice(&[i0, i0 + 1 + i, i0 + 1 + (i + 1) % k]);
        }
    }
    s
}

/// Rings of `nseg` vertices (angles from 0, y down) at `radii` round `c`, quads between them (a degenerate ring at 0
/// closes the middle). `at(r_index, phi)` gives each vertex's normal, m and colour.
fn polar(
    c: f32,
    radii: &[f32],
    nseg: usize,
    at: impl Fn(usize, f32, [f32; 2]) -> ([f32; 3], f32, [f32; 3]),
) -> Surf {
    let mut s = Surf::default();
    for (k, &r) in radii.iter().enumerate() {
        for i in 0..nseg {
            let phi = std::f32::consts::TAU * i as f32 / nseg as f32;
            let p = [c + r * phi.cos(), c + r * phi.sin()];
            let (n, m, base) = at(k, phi, p);
            s.push(p, n, m, base);
        }
    }
    for k in 0..radii.len().saturating_sub(1) {
        let (a, b) = ((k * nseg) as u32, ((k + 1) * nseg) as u32);
        for i in 0..nseg as u32 {
            let j = (i + 1) % nseg as u32;
            s.idx
                .extend_from_slice(&[a + i, a + j, b + j, a + i, b + j, b + i]);
        }
    }
    s
}

/// The Go stone: rings at the radial table's rows (outside in), a feather ring outside them.
fn stone_surf(rows: &[serde_json::Value]) -> Surf {
    let mut rows: Vec<&serde_json::Value> = rows.iter().collect();
    rows.sort_by(|a, b| f(&b["r_px"]).total_cmp(&f(&a["r_px"])));
    let mut radii = vec![f(&rows[0]["r_px"]) + FEATHER];
    radii.extend(rows.iter().map(|r| f(&r["r_px"])));
    polar(STONE_C, &radii, STONE_SEG, |k, phi, _| {
        let row = rows[k.saturating_sub(1)];
        let (nr, nz) = (f(&row["n_r"]), f(&row["n_z"]));
        let n = unit([nr * phi.cos(), -nr * phi.sin(), nz]);
        let m = if k == 0 { 0.0 } else { f(&row["m"]) };
        (n, m, rgb(&row["base_srgb"]))
    })
}

/// The thumb: a polar grid sampling the normal + reflectance map (bilinear at texel centres).
fn thumb_surf(nm: &image::RgbaImage) -> Surf {
    let (w, h) = (nm.width() as i64, nm.height() as i64);
    let sample = |p: [f32; 2]| {
        let (x, y) = (p[0] * NM_K - 0.5, p[1] * NM_K - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |xx: i64, yy: i64| {
            let q = nm.get_pixel(xx.clamp(0, w - 1) as u32, yy.clamp(0, h - 1) as u32);
            [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32].map(|v| v / 255.0)
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        let v: [f32; 4] = std::array::from_fn(|i| {
            (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy
        });
        (unit([v[0] * 2.0 - 1.0, v[1] * 2.0 - 1.0, v[2] * 2.0 - 1.0]), v[3])
    };
    let mut radii = vec![THUMB_R + FEATHER];
    radii.extend((0..THUMB_RADII).map(|i| THUMB_R * (1.0 - i as f32 / (THUMB_RADII - 1) as f32)));
    polar(THUMB_C, &radii, THUMB_SEG, |k, _, p| {
        let (n, m) = sample(p);
        (n, if k == 0 { 0.0 } else { m }, [0.9; 3])
    })
}

/// One row of the resin's cross-section: its offset from the groove's centre line, normal, m and colour.
#[derive(Clone, Copy, Debug)]
struct Row {
    y: f32,
    n: [f32; 3],
    m: f32,
    base: [f32; 3],
    lin: [f32; 3],
}

/// The room's surfaces on the transport, built once.
pub struct Room {
    /// The gem for each sign: [lit pause, dark play].
    gem: [Surf; 2],
    /// The back key's stone and the forward key's (mirrored).
    stone: [Surf; 2],
    thumb: Surf,
    resin: Vec<Row>,
    /// Shaded meshes by place, kept until unused for a while.
    meshes: RefCell<HashMap<u64, (Vec<Arc<Mesh>>, u64)>>,
}

/// Which surface a mesh is (for the cache).
#[derive(Hash)]
enum Which {
    Gem(usize),
    Stone(usize),
    Thumb,
}

impl Room {
    pub fn load() -> Room {
        let man: serde_json::Value = serde_json::from_str(MANIFEST).expect("hall manifest");
        let rows = |v: &serde_json::Value| v.as_array().cloned().unwrap_or_default();
        let gem = [
            gem_surf(&rows(&man["gem"]["facets"]["lit_pause"])),
            gem_surf(&rows(&man["gem"]["facets"]["dark_play"])),
        ];
        let st = stone_surf(&rows(&man["go_stone"]["radial"]));
        let stone = [st.clone(), st.mirrored(2.0 * STONE_C)];
        let nm = image::load_from_memory(THUMB_NM)
            .expect("thumb_nm.png")
            .to_rgba8();
        let resin = rows(&man["resin"]["profile"])
            .iter()
            .map(|r| {
                let base = rgb(&r["base_srgb"]);
                Row {
                    y: f(&r["y_from_groove_px"]),
                    n: unit([f(&r["n"][0]), f(&r["n"][1]), f(&r["n"][2])]),
                    m: f(&r["m"]),
                    base,
                    lin: base.map(hall::lin),
                }
            })
            .collect();
        Room {
            gem,
            stone,
            thumb: thumb_surf(&nm),
            resin,
            meshes: RefCell::new(HashMap::new()),
        }
    }

    /// A surface's mesh drawn with its canvas's top-left at `at`, `k` points a canvas point.
    fn shaded(&self, ctx: &Context, which: Which, at: Pos2, k: f32) -> Vec<Arc<Mesh>> {
        let hall = Hall::get(ctx);
        let eye = Eye::of(ctx);
        let pass = ctx.cumulative_pass_nr();
        let ppp = ctx.pixels_per_point();
        let key = qrs_ui::paint::key_of((
            &which,
            at.x.to_bits(),
            at.y.to_bits(),
            k.to_bits(),
            eye.key(),
            ppp.to_bits(),
        ));
        let mut cache = self.meshes.borrow_mut();
        if let Some((m, used)) = cache.get_mut(&key) {
            *used = pass;
            return m.clone();
        }
        if cache.len() > 64 {
            cache.retain(|_, (_, used)| pass.saturating_sub(*used) < 10);
        }
        let (surf, blend, tex) = match which {
            Which::Gem(i) => (&self.gem[i], Blend::Add(GEM_GAIN), hall::Tex::Probe),
            Which::Stone(i) => (&self.stone[i], Blend::Mix, hall::Tex::Mix),
            Which::Thumb => (&self.thumb, Blend::Mix, hall::Tex::MixBlur),
        };
        let verts = shade(surf, at, k, &eye, blend, |uv| hall.radiance(uv));
        // (on the room's pyramid: a stone's rim and the gem's steep facets minify it, P34.7)
        let meshes = hall.mipped(tex, verts, &surf.idx, ppp);
        cache.insert(key, (meshes.clone(), pass));
        meshes
    }

    /// The room in the gem whose canvas is `r` (lit: its pause sign lit, else the dark play sign's table).
    pub fn gem(&self, p: &Painter, r: Rect, lit: bool, canvas: f32) {
        let m = self.shaded(p.ctx(), Which::Gem(usize::from(!lit)), r.min, r.width() / canvas);
        p.extend(m.into_iter().map(Shape::Mesh));
    }

    /// The room in a Go stone whose canvas is `r` (`flip` for the forward key).
    pub fn stone(&self, p: &Painter, r: Rect, flip: bool, canvas: f32) {
        let m = self.shaded(
            p.ctx(),
            Which::Stone(usize::from(flip)),
            r.min,
            r.width() / canvas,
        );
        p.extend(m.into_iter().map(Shape::Mesh));
    }

    /// The room in the thumb whose canvas is `r`.
    pub fn thumb(&self, p: &Painter, r: Rect, canvas: f32) {
        let m = self.shaded(p.ctx(), Which::Thumb, r.min, r.width() / canvas);
        p.extend(m.into_iter().map(Shape::Mesh));
    }

    /// The room in the resin from `x0` to the lead's end at `fill_x` on the centre line `cy`, cut at `cuts`
    /// (`k` points a canvas point).
    pub fn resin(&self, p: &Painter, x0: f32, fill_x: f32, cy: f32, cuts: &[f32], k: f32) {
        if fill_x <= x0 + 2.0 * k || self.resin.is_empty() {
            return;
        }
        let ctx = p.ctx();
        let hall = Hall::get(ctx);
        let eye = Eye::of(ctx);
        let pass = ctx.cumulative_pass_nr();
        // (the end moves a pixel at a time: the mesh is rebuilt when it reaches the next)
        let end = (fill_x * ctx.pixels_per_point()).round() / ctx.pixels_per_point();
        let key = qrs_ui::paint::key_of((
            "resin",
            x0.to_bits(),
            end.to_bits(),
            cy.to_bits(),
            cuts.iter().map(|c| c.to_bits()).collect::<Vec<_>>(),
            eye.key(),
            ctx.pixels_per_point().to_bits(),
        ));
        let meshes = {
            let mut cache = self.meshes.borrow_mut();
            if let Some((m, used)) = cache.get_mut(&key) {
                *used = pass;
                m.clone()
            } else {
                let cols = resin_columns(x0 + 2.0 * k, end, cuts, k);
                let verts = resin_vertices(&self.resin, &cols, cy, k, &eye, |uv| hall.radiance(uv));
                let idx = grid(self.resin.len(), cols.len());
                let meshes = hall.mipped(hall::Tex::Probe, verts, &idx, ctx.pixels_per_point());
                cache.insert(key, (meshes.clone(), pass));
                meshes
            }
        };
        p.extend(meshes.into_iter().map(Shape::Mesh));
    }
}

/// A surface's vertices at `at` (`k` points a canvas point) under `eye`: UVs, and the blend's colours.
fn shade(
    s: &Surf,
    at: Pos2,
    k: f32,
    eye: &Eye,
    blend: Blend,
    radiance: impl Fn([f32; 2]) -> [f32; 3],
) -> Vec<Vertex> {
    (0..s.len())
        .map(|i| {
            let pos = pos2(at.x + s.pos[i][0] * k, at.y + s.pos[i][1] * k);
            let uv = eye.uv(pos, s.n[i]);
            let color = match blend {
                Blend::Mix => hall::mix(s.m[i]),
                Blend::Add(g) if s.m[i] > 0.0 => {
                    hall::additive(s.base[i], s.lin[i], g * hall::LEVEL * s.m[i], radiance(uv))
                }
                Blend::Add(_) => Color32::TRANSPARENT,
            };
            Vertex {
                pos,
                uv: pos2(uv[0], uv[1]),
                color,
            }
        })
        .collect()
}

/// The resin's columns from `x0` to the lead's end `end`: (x, coverage), every 8 canvas points and either side of
/// each cut (none in it), the last just past the lead's round end.
fn resin_columns(x0: f32, end: f32, cuts: &[f32], k: f32) -> Vec<(f32, f32)> {
    let mut xs: Vec<(f32, f32)> = Vec::new();
    let mut x = x0;
    while x < end - k {
        xs.push((x, 1.0));
        x += RESIN_COL * k;
    }
    xs.push((end - k, 1.0));
    xs.push((end + 2.0 * k, 0.0));
    for &t in cuts.iter().filter(|&&t| t > x0 + 2.0 * k && t < end - 4.0 * k) {
        xs.extend_from_slice(&[(t - CUT_HALF * k, 1.0), (t, 0.0), (t + CUT_HALF * k, 1.0)]);
    }
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    xs.dedup_by(|a, b| (a.0 - b.0).abs() < 0.05);
    xs
}

/// The resin grid's vertices: a row per cross-section row, a column per `cols`, m times the column's coverage.
fn resin_vertices(
    rows: &[Row],
    cols: &[(f32, f32)],
    cy: f32,
    k: f32,
    eye: &Eye,
    radiance: impl Fn([f32; 2]) -> [f32; 3],
) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(rows.len() * cols.len());
    for r in rows {
        for &(x, cov) in cols {
            let pos = pos2(x, cy + r.y * k);
            let uv = eye.uv(pos, r.n);
            let gm = RESIN_GAIN * hall::LEVEL * r.m * cov;
            let color = if gm > 0.0 {
                hall::additive(r.base, r.lin, gm, radiance(uv))
            } else {
                Color32::TRANSPARENT
            };
            out.push(Vertex {
                pos,
                uv: pos2(uv[0], uv[1]),
                color,
            });
        }
    }
    out
}

/// Two triangles a cell of a `rows` x `cols` grid.
fn grid(rows: usize, cols: usize) -> Vec<u32> {
    let mut t = Vec::with_capacity(rows.saturating_sub(1) * cols.saturating_sub(1) * 6);
    for r in 0..rows.saturating_sub(1) {
        for i in 0..cols.saturating_sub(1) {
            let (a, b) = ((r * cols + i) as u32, ((r + 1) * cols + i) as u32);
            t.extend_from_slice(&[a, a + 1, b + 1, a, b + 1, b]);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_make_their_meshes() {
        let r = Room::load();
        // the gem: 33 facets of the cut (8 kites and 8 stars of 4 + 1, 16 triangles of 3 + 1, the table 8 + 1)
        assert_eq!(gem_facets().len(), 33);
        assert_eq!(r.gem[0].len(), 8 * 5 + 16 * 4 + 8 * 4 + 9);
        assert_eq!(r.gem[0].idx.len(), 3 * (8 * 4 + 16 * 3 + 8 * 3 + 8));
        // the table's reflectance is cut to 8 %
        let last = r.gem[0].len() - 1;
        assert!(r.gem[0].m[last] < 0.004 && r.gem[0].m[0] > 0.03);
        // the stone: the feather and 24 rows, 64 a ring; the feather adds nothing; the forward key mirrored
        assert_eq!(r.stone[0].len(), 25 * STONE_SEG);
        assert_eq!(r.stone[0].m[0], 0.0);
        assert!((r.stone[1].pos[5][0] - (2.0 * STONE_C - r.stone[0].pos[5][0])).abs() < 1e-4);
        assert!((r.stone[1].n[5][0] + r.stone[0].n[5][0]).abs() < 1e-6);
        // the stone's rim sees sideways, its middle the viewer
        let rim = STONE_SEG; // the first real ring, phi 0 (pointing right)
        assert!(r.stone[0].n[rim][0] > 0.8 && r.stone[0].n[r.stone[0].len() - 1][2] > 0.99);
        // the thumb: the feather and 19 radii, 56 a ring, normals from the map
        assert_eq!(r.thumb.len(), (THUMB_RADII + 1) * THUMB_SEG);
        assert!(
            r.thumb
                .n
                .iter()
                .all(|n| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-3)
        );
        // the resin: its cross-section rows, across the groove
        assert_eq!(r.resin.len(), 27);
        assert!(r.resin[0].y < 0.0 && r.resin[26].y > 0.0);
    }

    #[test]
    fn the_resin_has_no_room_in_its_cuts_or_past_its_lead() {
        let cols = resin_columns(100.0, 300.0, &[150.0, 297.0, 50.0], 1.0);
        // a cut: a column either side and none in it; a cut too near the lead (or before the start) is left out
        let at = |x: f32| cols.iter().find(|c| (c.0 - x).abs() < 1e-3).map(|c| c.1);
        assert_eq!(at(150.0), Some(0.0));
        assert_eq!(at(148.8), Some(1.0));
        assert_eq!(at(297.0), None);
        // the lead's end fades out; the columns run in order
        assert_eq!(cols.last().map(|c| c.1), Some(0.0));
        assert!(cols.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(grid(3, 4).len(), 2 * 3 * 6);
    }

    #[test]
    fn the_room_moves_with_the_surface() {
        // a stone higher up the window sees more roof; the mix alpha doesn't move with it
        let r = Room::load();
        let eye = Eye::new(1440.0, 900.0);
        let lo = shade(&r.stone[0], pos2(100.0, 820.0), 1.0, &eye, Blend::Mix, |_| {
            [0.0; 3]
        });
        let hi = shade(&r.stone[0], pos2(100.0, 120.0), 1.0, &eye, Blend::Mix, |_| {
            [0.0; 3]
        });
        let c = r.stone[0].len() - 1;
        assert!(hi[c].uv.y < lo[c].uv.y);
        assert_eq!(hi[c].color, lo[c].color);
        // the gem adds light, alpha 0
        let g = shade(
            &r.gem[0],
            pos2(100.0, 800.0),
            1.0,
            &eye,
            Blend::Add(GEM_GAIN),
            |_| [0.1; 3],
        );
        assert!(g.iter().all(|v| v.color.a() == 0) && g.iter().any(|v| v.color.r() > 0));
    }

    /// What the transport's room costs on the CPU when everything moves (`cargo test --release -p qrs-app
    /// transport_cost -- --ignored --nocapture`); a still frame reuses every mesh.
    #[test]
    #[ignore]
    fn transport_cost() {
        let r = Room::load();
        let blur = hall::blur_copy();
        let eye = Eye::new(1400.0, 875.0);
        let rad = |uv| hall::radiance_of(&blur, uv);
        let n = 200;
        let t = std::time::Instant::now();
        let mut verts = 0;
        for i in 0..n {
            let y = 820.0 + (i % 3) as f32;
            let k = 1.0;
            verts = 0;
            for (s, b) in [
                (&r.gem[0], Blend::Add(GEM_GAIN)),
                (&r.stone[0], Blend::Mix),
                (&r.stone[1], Blend::Mix),
                (&r.thumb, Blend::Mix),
            ] {
                let v = shade(s, pos2(100.0, y), k, &eye, b, rad);
                verts += v.len();
                std::hint::black_box(v);
            }
            let cols = resin_columns(330.0, 330.0 + 500.0, &[400.0, 520.0, 610.0, 700.0], k);
            let v = resin_vertices(&r.resin, &cols, y, k, &eye, rad);
            verts += v.len();
            std::hint::black_box(v);
        }
        let per = t.elapsed().as_secs_f64() / n as f64;
        println!(
            "transport room: {verts} vertices, {:.1} us a frame with everything moved",
            per * 1e6
        );
    }

    #[test]
    fn every_asset_loads_at_its_size() {
        let nm = image::load_from_memory(THUMB_NM).expect("thumb_nm.png");
        assert_eq!((nm.width(), nm.height()), (240, 240));
    }
}
