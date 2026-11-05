//! The app-chrome knobs as real objects on the dark themes (P34.8, owner-approved; D100): Blender renders of
//! `knobs.rs`'s four styles (amp gold, pedal black, chrome, the vintage chicken-head) with the shared room
//! (`crate::hall`) reflected in them by where each knob sits in the window. Daylight and the classic look keep
//! `knobs.rs`'s own drawing. New in quickRS; the pack is the research folder's `mockups/ui-main/tiles/knobs/app_assets/`
//! = `assets/knobs/` (its README the method, its `manifest.json` the numbers).
//!
//! A knob `big_s` pixels across is drawn, centred on the tick ring's centre, as (after the ticks, which stay code):
//! 1. its shadow for the state (white with alpha, tinted with the theme's cast colour);
//! 2. its body (everything that doesn't turn, lit by the fixed studio), hover x1.06, pressed x0.94;
//! 3. the room: an additive polar mesh (64 segments, a ring per row of the manifest's radial table, a feather ring
//!    outside with m 0) on the hall's probe (sharp for glossy plastic, blurred for satin metal), gold reflecting gold;
//! 4. the indicator: the nearest of 19 pre-lit frames (every 15 degrees), its quad turned by the rest (at most 7.5).
//!
//! Every sprite is a 6x master (576 px for a 64 point knob's 96 point canvas) resampled once on the CPU to the pixels
//! it covers (canvas = 1.5 x the knob) and drawn 1:1, so the GPU never minifies a master (egui-wgpu makes no mips,
//! D94). The textures are cached by (style, part, pixels), least recently used out past a budget; the room mesh by
//! (style, size, place, window, state), so a still knob costs lookups and only a move or a resize redoes it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::epaint::Vertex;
use egui::{
    Color32, ColorImage, Context, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureOptions, pos2,
};

use crate::glass::{self, State};
use crate::hall::{self, Eye, Hall};
use crate::knobs::KnobStyle;
use crate::paint::key_of;
use crate::theme::t;

macro_rules! png {
    ($($part:expr),*) => {
        include_bytes!(concat!("../../../assets/knobs/", $($part),*, ".png")).as_slice()
    };
}

macro_rules! pack {
    ($s:literal, $ind:literal) => {
        Pack {
            body: png!($s, "_body"),
            shadow: [
                png!($s, "_shadow_normal"),
                png!($s, "_shadow_hover"),
                png!($s, "_shadow_pressed"),
            ],
            frames: [
                png!($s, "_", $ind, "_-135"),
                png!($s, "_", $ind, "_-120"),
                png!($s, "_", $ind, "_-105"),
                png!($s, "_", $ind, "_-090"),
                png!($s, "_", $ind, "_-075"),
                png!($s, "_", $ind, "_-060"),
                png!($s, "_", $ind, "_-045"),
                png!($s, "_", $ind, "_-030"),
                png!($s, "_", $ind, "_-015"),
                png!($s, "_", $ind, "_+000"),
                png!($s, "_", $ind, "_+015"),
                png!($s, "_", $ind, "_+030"),
                png!($s, "_", $ind, "_+045"),
                png!($s, "_", $ind, "_+060"),
                png!($s, "_", $ind, "_+075"),
                png!($s, "_", $ind, "_+090"),
                png!($s, "_", $ind, "_+105"),
                png!($s, "_", $ind, "_+120"),
                png!($s, "_", $ind, "_+135"),
            ],
        }
    };
}

/// One style's sprites (straight-alpha sRGB PNGs, 6x).
struct Pack {
    body: &'static [u8],
    /// Normal, hover, pressed.
    shadow: [&'static [u8]; 3],
    /// The indicator at -135, -120, ... +135 degrees.
    frames: [&'static [u8]; FRAMES],
}

/// In `KnobStyle::ALL` order.
static PACKS: [Pack; 4] = [
    pack!("amp", "ind"),
    pack!("pedal", "ind"),
    pack!("chrome", "ind"),
    pack!("vintage", "ptr"),
];

const MANIFEST: &str = include_str!("../../../assets/knobs/manifest.json");

/// The masters' size, and a line indicator's square in them (master px).
pub const MASTER_PX: u32 = 576;
const IND_SQUARE_PX: u32 = 278;
/// Every sprite's canvas is this many times the knob.
const CANVAS_K: f64 = 1.5;
/// The indicator frames: 19, every 15 degrees from -135.
pub const FRAMES: usize = 19;
const STEP_DEG: f64 = 15.0;
/// The room mesh's segments and its feather ring outside the last row (pixels).
const SEG: usize = 64;
const FEATHER_PX: f32 = 0.5;
/// The textures kept on the GPU (bytes, then count), least recently used out first.
const BUDGET_BYTES: usize = 24 << 20;
const BUDGET_COUNT: usize = 192;
/// Decoded masters kept on the CPU (a resize remakes a knob's few textures without decoding again).
const MASTERS_KEPT: usize = 12;

/// The baked knobs are the dark themes' (as the glass keys); Daylight and the classic look keep `knobs.rs`'s.
pub fn on() -> bool {
    glass::on()
}

fn style_i(st: KnobStyle) -> usize {
    KnobStyle::ALL.iter().position(|k| *k == st).unwrap_or(0)
}

/// The nearest frame to `deg` (clockwise from 12 o'clock, -135..135) and what's left to turn its quad by.
pub fn frame_of(deg: f64) -> (usize, f64) {
    let k = ((deg + 135.0) / STEP_DEG).round().clamp(0.0, (FRAMES - 1) as f64) as usize;
    (k, deg - frame_deg(k))
}

/// Frame `k`'s angle.
pub fn frame_deg(k: usize) -> f64 {
    -135.0 + STEP_DEG * k as f64
}

/// The whole number nearest `x` with the parity of `like` (so a sprite of that size centres where the tick ring does).
fn fit(x: f64, like: i32) -> i32 {
    let n = x.round() as i32;
    if (n - like).rem_euclid(2) == 0 {
        n
    } else if x > n as f64 {
        n + 1
    } else {
        n - 1
    }
    .max(2 + like.rem_euclid(2))
}

/// A knob `big_s` pixels across in a tick box `boxw` wide: (its canvas's pixels, its indicator's) for the style.
pub fn sprite_px(st: KnobStyle, big_s: i32, boxw: i32) -> (i32, i32) {
    let canvas = CANVAS_K * big_s as f64;
    let ind = if st == KnobStyle::Vintage {
        canvas
    } else {
        canvas * IND_SQUARE_PX as f64 / MASTER_PX as f64
    };
    (fit(canvas, boxw), fit(ind, boxw))
}

/// What a texture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Part {
    Body,
    Shadow(State),
    Frame(usize),
}

impl Part {
    fn bytes(self, st: KnobStyle) -> &'static [u8] {
        let pk = &PACKS[style_i(st)];
        match self {
            Part::Body => pk.body,
            Part::Shadow(s) => pk.shadow[s as usize],
            Part::Frame(k) => pk.frames[k],
        }
    }
}

/// A master decoded and premultiplied.
fn decode(bytes: &[u8]) -> image::RgbaImage {
    let mut im = image::load_from_memory(bytes).expect("knob sprite").to_rgba8();
    for px in im.pixels_mut() {
        let a = px[3] as u32;
        for c in 0..3 {
            px[c] = ((px[c] as u32 * a + 127) / 255) as u8;
        }
    }
    im
}

type TexKey = (KnobStyle, Part, i32);

struct Entry {
    tex: TextureHandle,
    bytes: usize,
    used: u64,
}

/// The resampled textures, and the masters they come from.
#[derive(Default)]
struct Store {
    tex: HashMap<TexKey, Entry>,
    bytes: usize,
    /// How many were made (tests and the cost probe).
    made: u64,
    masters: Vec<((KnobStyle, Part), Arc<image::RgbaImage>)>,
}

impl Store {
    fn master(&mut self, st: KnobStyle, part: Part) -> Arc<image::RgbaImage> {
        if let Some(i) = self.masters.iter().position(|(k, _)| *k == (st, part)) {
            let hit = self.masters.remove(i);
            let m = hit.1.clone();
            self.masters.push(hit);
            return m;
        }
        let m = Arc::new(decode(part.bytes(st)));
        self.masters.push(((st, part), m.clone()));
        while self.masters.len() > MASTERS_KEPT {
            self.masters.remove(0);
        }
        m
    }

    /// The texture of `part` resampled to `n` pixels square, made the first time; `pass` marks it used.
    fn get(&mut self, ctx: &Context, st: KnobStyle, part: Part, n: i32, pass: u64) -> egui::TextureId {
        let key = (st, part, n);
        if let Some(e) = self.tex.get_mut(&key) {
            e.used = pass;
            return e.tex.id();
        }
        let m = self.master(st, part);
        let n_u = n.max(1) as u32;
        let r = if m.width() == n_u {
            (*m).clone()
        } else {
            glass::resample(&m, n_u, n_u, false)
        };
        let img = ColorImage::from_rgba_premultiplied([n_u as usize, n_u as usize], r.as_raw());
        let tex = ctx.load_texture(
            format!("knob-{}-{part:?}-{n}", st.key()),
            img,
            TextureOptions::LINEAR,
        );
        let id = tex.id();
        let bytes = (n_u * n_u * 4) as usize;
        self.bytes += bytes;
        self.made += 1;
        self.tex.insert(
            key,
            Entry {
                tex,
                bytes,
                used: pass,
            },
        );
        // out with the least recently used, never one this pass draws
        while self.bytes > BUDGET_BYTES || self.tex.len() > BUDGET_COUNT {
            let Some((&old, _)) = self
                .tex
                .iter()
                .filter(|(_, e)| e.used != pass)
                .min_by_key(|(_, e)| e.used)
            else {
                break;
            };
            if let Some(e) = self.tex.remove(&old) {
                self.bytes -= e.bytes;
            }
        }
        id
    }
}

/// One row of a style's radial table: r (of the knob's radius) -> the mean normal, reflectance per channel and the
/// colour under it.
#[derive(Clone, Copy, Debug)]
struct Row {
    r: f32,
    n_r: f32,
    n_z: f32,
    m: [f32; 3],
    base: [f32; 3],
    lin: [f32; 3],
}

/// The manifest's rows per style (`KnobStyle::ALL` order), outside in.
fn rows() -> &'static [Vec<Row>; 4] {
    static ROWS: OnceLock<[Vec<Row>; 4]> = OnceLock::new();
    ROWS.get_or_init(|| {
        let man: serde_json::Value = serde_json::from_str(MANIFEST).expect("knob manifest");
        let f = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
        KnobStyle::ALL.map(|st| {
            let mut rows: Vec<Row> = man[st.key()]["radial"]
                .as_array()
                .map(|a| a.as_slice())
                .unwrap_or_default()
                .iter()
                .map(|r| {
                    let base: [f32; 3] = std::array::from_fn(|i| f(&r["base_srgb"][i]) / 255.0);
                    Row {
                        r: f(&r["r"]),
                        n_r: f(&r["n_r"]),
                        n_z: f(&r["n_z"]),
                        m: std::array::from_fn(|i| f(&r["m_rgb"][i])),
                        base,
                        lin: base.map(hall::lin),
                    }
                })
                .collect();
            rows.sort_by(|a, b| b.r.total_cmp(&a.r));
            rows
        })
    })
}

/// The room's gain and probe per style: satin metal on the blurred room, glossy plastic on the sharp one at more gain.
fn room_of(st: KnobStyle) -> (f32, hall::Tex) {
    match st {
        KnobStyle::Amp | KnobStyle::Chrome => (1.0, hall::Tex::Blur),
        KnobStyle::Pedal | KnobStyle::Vintage => (2.6, hall::Tex::Probe),
    }
}

fn state_gain(s: State) -> f32 {
    match s {
        State::Normal => 1.0,
        State::Hover => 1.18,
        State::Pressed => 0.6,
    }
}

/// The room mesh's vertices for a knob of radius `r_pt` (points) centred at `c`: a feather ring, then a ring per
/// row, outside in; each vertex's UV for where it is under `eye`, its colour the additive light.
fn room_vertices(
    st: KnobStyle,
    c: Pos2,
    r_pt: f32,
    feather_pt: f32,
    eye: &Eye,
    state: State,
    radiance: impl Fn([f32; 2]) -> [f32; 3],
) -> Vec<Vertex> {
    let rows = &rows()[style_i(st)];
    let (gain, _) = room_of(st);
    let g = gain * state_gain(state) * hall::LEVEL;
    let mut out = Vec::with_capacity((rows.len() + 1) * SEG);
    let rings = std::iter::once((rows[0].r * r_pt + feather_pt, rows[0], true))
        .chain(rows.iter().map(|row| (row.r * r_pt, *row, false)));
    for (rad, row, feather) in rings {
        for i in 0..SEG {
            let phi = std::f32::consts::TAU * i as f32 / SEG as f32;
            let (cs, sn) = (phi.cos(), phi.sin());
            let pos = pos2(c.x + rad * cs, c.y + rad * sn);
            // (y down on screen, up in the normal's frame)
            let n = [row.n_r * cs, -row.n_r * sn, row.n_z];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
            let n = [n[0] / l, n[1] / l, n[2] / l];
            let uv = eye.uv(pos, n);
            let color = if feather {
                Color32::TRANSPARENT
            } else {
                hall::additive_rgb(row.base, row.lin, row.m.map(|m| g * m), radiance(uv))
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

/// Quads between the rings (`rings` of `SEG`).
fn ring_indices(rings: usize) -> Vec<u32> {
    let mut t = Vec::with_capacity(rings.saturating_sub(1) * SEG * 6);
    for k in 0..rings.saturating_sub(1) {
        let (a, b) = ((k * SEG) as u32, ((k + 1) * SEG) as u32);
        for i in 0..SEG as u32 {
            let j = (i + 1) % SEG as u32;
            t.extend_from_slice(&[a + i, a + j, b + j, a + i, b + j, b + i]);
        }
    }
    t
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
    /// Room meshes by place, kept until unused for a while.
    static MESHES: RefCell<HashMap<u64, (Vec<Arc<Mesh>>, u64)>> = RefCell::new(HashMap::new());
}

/// The room in a knob (cached by everything that changes it).
fn room(p: &Painter, st: KnobStyle, c: Pos2, big_s: i32, state: State) -> Vec<Arc<Mesh>> {
    let ctx = p.ctx();
    let ppp = p.pixels_per_point();
    let eye = Eye::of(ctx);
    let pass = ctx.cumulative_pass_nr();
    let key = key_of((
        st,
        big_s,
        c.x.to_bits(),
        c.y.to_bits(),
        eye.key(),
        ppp.to_bits(),
        state,
    ));
    MESHES.with(|m| {
        let mut cache = m.borrow_mut();
        if let Some((meshes, used)) = cache.get_mut(&key) {
            *used = pass;
            return meshes.clone();
        }
        if cache.len() > 96 {
            cache.retain(|_, (_, used)| pass.saturating_sub(*used) < 10);
        }
        let hall = Hall::get(ctx);
        let r_pt = big_s as f32 / 2.0 / ppp;
        let verts = room_vertices(st, c, r_pt, FEATHER_PX / ppp, &eye, state, |uv| hall.radiance(uv));
        let idx = ring_indices(rows()[style_i(st)].len() + 1);
        let meshes = hall.mipped(room_of(st).1, verts, &idx, ppp);
        cache.insert(key, (meshes.clone(), pass));
        meshes
    })
}

/// A square texture `n` pixels across with its top-left at pixel (x, y), tinted (premultiplied).
fn sprite(p: &Painter, id: egui::TextureId, x: i32, y: i32, n: i32, tint: Color32) {
    let ppp = p.pixels_per_point();
    let r = Rect::from_min_size(
        pos2(x as f32 / ppp, y as f32 / ppp),
        egui::vec2(n as f32 / ppp, n as f32 / ppp),
    );
    let mut mesh = Mesh::with_texture(id);
    mesh.add_rect_with_uv(r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
    p.add(Shape::mesh(mesh));
}

/// The four corners of a square `half` points from `c`, turned `deg` clockwise about it (y down), with UVs 0..1.
fn turned_quad(id: egui::TextureId, c: Pos2, half: f32, deg: f64) -> Mesh {
    let (sn, cs) = (deg.to_radians().sin() as f32, deg.to_radians().cos() as f32);
    let mut mesh = Mesh::with_texture(id);
    for (dx, dy, u, v) in [
        (-half, -half, 0.0, 0.0),
        (half, -half, 1.0, 0.0),
        (half, half, 1.0, 1.0),
        (-half, half, 0.0, 1.0),
    ] {
        mesh.vertices.push(Vertex {
            pos: pos2(c.x + dx * cs - dy * sn, c.y + dx * sn + dy * cs),
            uv: pos2(u, v),
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    mesh
}

/// The knob `big_s` pixels across in the tick box `boxw` wide whose top-left is pixel `(bx, by)`, turned to `deg`,
/// in a state (the ticks are the caller's).
pub(crate) fn draw(
    p: &Painter,
    st: KnobStyle,
    big_s: i32,
    boxw: i32,
    (bx, by): (i32, i32),
    deg: f64,
    state: State,
) {
    let ctx = p.ctx().clone();
    let ppp = p.pixels_per_point();
    let pass = ctx.cumulative_pass_nr();
    let (n, ni) = sprite_px(st, big_s, boxw);
    let (k, rest) = frame_of(deg);
    let (shadow, body, ind) = STORE.with(|s| {
        let mut s = s.borrow_mut();
        (
            s.get(&ctx, st, Part::Shadow(state), n, pass),
            s.get(&ctx, st, Part::Body, n, pass),
            s.get(&ctx, st, Part::Frame(k), ni, pass),
        )
    });
    let (x, y) = (bx + (boxw - n) / 2, by + (boxw - n) / 2);
    let cast = t().cast;
    sprite(
        p,
        shadow,
        x,
        y,
        n,
        Color32::from_rgb(cast.r(), cast.g(), cast.b()),
    );
    match state {
        State::Pressed => sprite(
            p,
            body,
            x,
            y,
            n,
            Color32::from_rgba_premultiplied(240, 240, 240, 255),
        ),
        _ => sprite(p, body, x, y, n, Color32::WHITE),
    }
    if state == State::Hover {
        // x1.06: the body's own colour again at 6 %, added (alpha 0)
        sprite(p, body, x, y, n, Color32::from_rgba_premultiplied(15, 15, 15, 0));
    }
    let c = pos2(
        (bx as f32 + boxw as f32 / 2.0) / ppp,
        (by as f32 + boxw as f32 / 2.0) / ppp,
    );
    p.extend(room(p, st, c, big_s, state).into_iter().map(Shape::Mesh));
    if rest.abs() < 1e-6 {
        let (xi, yi) = (bx + (boxw - ni) / 2, by + (boxw - ni) / 2);
        sprite(p, ind, xi, yi, ni, Color32::WHITE);
    } else {
        p.add(Shape::mesh(turned_quad(ind, c, ni as f32 / 2.0 / ppp, rest)));
    }
}

/// How many textures the knobs have made, and how many are kept with their bytes (tests, the GPU estimate).
pub fn stats() -> (u64, usize, usize) {
    STORE.with(|s| {
        let s = s.borrow();
        (s.made, s.tex.len(), s.bytes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_frame_and_the_rest_cover_the_range() {
        assert_eq!(frame_of(-135.0), (0, 0.0));
        assert_eq!(frame_of(135.0), (18, 0.0));
        assert_eq!(frame_of(0.0), (9, 0.0));
        let (k, r) = frame_of(-127.0);
        assert_eq!(k, 1);
        assert!((r - -7.0).abs() < 1e-9);
        // past the ends (never asked for) the end frames, turned on
        assert_eq!(frame_of(-140.0).0, 0);
        assert_eq!(frame_of(150.0).0, 18);
        // everywhere in the range: the frame's angle plus the rest is the angle, the rest at most half a step
        for i in 0..=2700 {
            let deg = -135.0 + i as f64 * 0.1;
            let (k, r) = frame_of(deg);
            assert!(k < FRAMES && r.abs() <= 7.5 + 1e-9, "{deg}: {k} {r}");
            assert!((frame_deg(k) + r - deg).abs() < 1e-9);
        }
        // the value scale: u 0 and 1 are the end frames exactly
        assert_eq!(frame_of(-135.0 + 270.0 * 0.5), (9, 0.0));
    }

    #[test]
    fn the_sprites_scale_with_the_knob_and_centre_on_the_ticks() {
        // a 34 point knob at 1x (big_s 35 at 1440 wide), its tick box 59: canvas 1.5 x, odd like the box
        let (n, ni) = sprite_px(KnobStyle::Amp, 35, 59);
        assert_eq!((n % 2, ni % 2), (1, 1));
        assert!((n as f64 - 52.5).abs() <= 1.0 && (ni as f64 - 52.5 * 278.0 / 576.0).abs() <= 1.0);
        // even box, the vintage pointer on the full canvas
        let (n, ni) = sprite_px(KnobStyle::Vintage, 75, 124);
        assert_eq!((n, ni), (112, 112));
        assert_eq!(fit(10.4, 1), 11);
        assert_eq!(fit(9.6, 1), 9);
        assert_eq!(fit(10.0, 0), 10);
    }

    #[test]
    fn every_sprite_loads_at_its_size_and_the_tables_parse() {
        for st in KnobStyle::ALL {
            let pk = &PACKS[style_i(st)];
            let size = |b: &[u8]| {
                let im = image::load_from_memory(b).expect("sprite");
                (im.width(), im.height())
            };
            assert_eq!(size(pk.body), (MASTER_PX, MASTER_PX), "{st:?}");
            for s in pk.shadow {
                assert_eq!(size(s), (MASTER_PX, MASTER_PX));
            }
            let ind = if st == KnobStyle::Vintage {
                MASTER_PX
            } else {
                IND_SQUARE_PX
            };
            for f in pk.frames {
                assert_eq!(size(f), (ind, ind), "{st:?}");
            }
            let rows = &rows()[style_i(st)];
            assert_eq!(rows.len(), 50);
            // outside in; the middle faces the viewer
            assert!(rows[0].r > 1.0 && rows[49].r == 0.0 && rows[49].n_z > 0.99);
        }
        // gold reflects gold, chrome nearly grey
        let amp = &rows()[0][30];
        assert!(amp.m[0] > amp.m[2] * 3.0);
    }

    #[test]
    fn the_room_mesh_moves_with_the_knob_and_adds_light() {
        let eye = Eye::new(1440.0, 900.0);
        let blur = hall::blur_copy();
        let rad = |uv| hall::radiance_of(&blur, uv);
        let at = |y: f32, s: State| room_vertices(KnobStyle::Pedal, pos2(700.0, y), 17.0, 0.5, &eye, s, rad);
        let (hi, lo) = (at(60.0, State::Normal), at(840.0, State::Normal));
        assert_eq!(hi.len(), 51 * SEG);
        assert_eq!(ring_indices(51).len(), 50 * SEG * 6);
        // the feather adds nothing; the rest is light (alpha 0)
        assert!(hi[..SEG].iter().all(|v| v.color == Color32::TRANSPARENT));
        assert!(hi.iter().all(|v| v.color.a() == 0) && hi.iter().any(|v| v.color.r() > 0));
        // higher in the window the middle sees more roof
        let mid = hi.len() - 1;
        assert!(hi[mid].uv.y < lo[mid].uv.y);
        // hover brighter, pressed dimmer
        let sum = |v: &[Vertex]| {
            v.iter()
                .map(|x| x.color.r() as u32 + x.color.g() as u32)
                .sum::<u32>()
        };
        let (h, pr) = (at(60.0, State::Hover), at(60.0, State::Pressed));
        assert!(sum(&h) > sum(&hi) && sum(&pr) < sum(&hi));
        // gold tints its light
        let g = room_vertices(
            KnobStyle::Amp,
            pos2(700.0, 60.0),
            17.0,
            0.5,
            &eye,
            State::Normal,
            |_| [0.2; 3],
        );
        let v = g[SEG * 20];
        assert!(v.color.r() > v.color.b());
    }

    #[test]
    fn textures_are_made_once_per_size_and_old_ones_go() {
        let ctx = Context::default();
        let mut s = Store::default();
        let a = s.get(&ctx, KnobStyle::Amp, Part::Body, 75, 1);
        assert_eq!(s.made, 1);
        // the same size again: the same texture, nothing made
        assert_eq!(s.get(&ctx, KnobStyle::Amp, Part::Body, 75, 2), a);
        assert_eq!(s.made, 1);
        // its master decoded once for every size
        s.get(&ctx, KnobStyle::Amp, Part::Body, 52, 2);
        assert_eq!((s.made, s.masters.len()), (2, 1));
        assert_eq!(s.bytes, (75 * 75 + 52 * 52) * 4);
        // past the count budget the least recently used go, never one drawn this pass
        for i in 0..BUDGET_COUNT as i32 + 5 {
            s.get(&ctx, KnobStyle::Pedal, Part::Frame(9), 20 + i, 3 + i as u64);
        }
        assert!(s.tex.len() <= BUDGET_COUNT);
        assert!(!s.tex.contains_key(&(KnobStyle::Amp, Part::Body, 75)));
        let last = BUDGET_COUNT as i32 + 4;
        assert!(s.tex.contains_key(&(KnobStyle::Pedal, Part::Frame(9), 20 + last)));
        assert_eq!(s.bytes, s.tex.values().map(|e| e.bytes).sum::<usize>());
        // the masters kept on the CPU are capped
        for k in 0..FRAMES {
            s.master(KnobStyle::Chrome, Part::Frame(k));
        }
        assert_eq!(s.masters.len(), MASTERS_KEPT);
    }

    #[test]
    fn a_resampled_sprite_keeps_its_colour_within_its_alpha() {
        let m = decode(PACKS[0].frames[3]);
        let r = glass::resample(&m, 40, 40, false);
        assert!(r.pixels().all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
        assert!(r.pixels().any(|p| p[3] > 200));
    }
}
