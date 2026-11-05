//! The highway's skin (owner's picks, 2026-11-03; research/spec/11): stage plates (MOSS-Image), note materials and an
//! amp stack (Blender renders of our own models), and a look that rotates by itself, with no setting: the song picks
//! its note material and home stage, the stage may swap once (at the solo, or mid-song), a solo goes dark and wide,
//! "noguitar" sections dim, and the streak heats the stage (amber, red, on fire). Deterministic: hashes of the song.

use eframe::egui::epaint::Vertex;
use eframe::egui::{
    self, Color32, ColorImage, Context, Mesh, Painter, Rect, Shape, Stroke, TextureHandle, TextureId,
    TextureOptions, pos2, vec2,
};
use qrs_formats::Arrangement;

use super::gfx::{BAKELITE, Pal, TUBE};
use super::view::View;

const PLATE_BYTES: [&[u8]; 6] = [
    include_bytes!("../../../../../assets/highway/plate_blue.jpg"),
    include_bytes!("../../../../../assets/highway/plate_white.jpg"),
    include_bytes!("../../../../../assets/highway/plate_amber.jpg"),
    include_bytes!("../../../../../assets/highway/plate_red.jpg"),
    include_bytes!("../../../../../assets/highway/plate_fire.jpg"),
    include_bytes!("../../../../../assets/highway/plate_solo.jpg"),
];
const GEM_BYTES: [&[u8]; 3] = [
    include_bytes!("../../../../../assets/highway/gems_bakelite.png"),
    include_bytes!("../../../../../assets/highway/gems_copper.png"),
    include_bytes!("../../../../../assets/highway/gems_worn.png"),
];
const AMP_BYTES: &[u8] = include_bytes!("../../../../../assets/highway/amp.png");
const AMP_GLOW_BYTES: &[u8] = include_bytes!("../../../../../assets/highway/amp_glow.png");

/// Plates, in `PLATE_BYTES` order.
const BLUE: usize = 0;
const WHITE: usize = 1;
const HEAT: [usize; 3] = [2, 3, 4];
const SOLO: usize = 5;
/// The amp's height and where its foot stands, as shares of the highway's height.
const AMP_H: f32 = 0.36;
const AMP_FOOT: f32 = 0.76;
/// The plates' brightness behind the highway, and in "noguitar" sections.
const PLATE_V: f32 = 0.55;
const QUIET_V: f32 = 0.45;
/// Streak (judged notes in a row) for each heat step: amber, red, on fire.
const STREAK: [usize; 3] = [25, 50, 90];

pub struct Tex {
    plates: Vec<TextureHandle>,
    gems: Vec<TextureHandle>,
    amp: TextureHandle,
    amp_glow: TextureHandle,
}

fn load(ctx: &Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let im = image::load_from_memory(bytes)
        .expect("highway skin asset")
        .to_rgba8();
    let size = [im.width() as usize, im.height() as usize];
    let opts = TextureOptions {
        mipmap_mode: Some(egui::TextureFilter::Linear),
        ..TextureOptions::LINEAR
    };
    ctx.load_texture(name, ColorImage::from_rgba_unmultiplied(size, im.as_raw()), opts)
}

impl Tex {
    pub fn load(ctx: &Context) -> Tex {
        Tex {
            plates: PLATE_BYTES
                .iter()
                .enumerate()
                .map(|(i, b)| load(ctx, &format!("hw-plate{i}"), b))
                .collect(),
            gems: GEM_BYTES
                .iter()
                .enumerate()
                .map(|(i, b)| load(ctx, &format!("hw-gems{i}"), b))
                .collect(),
            amp: load(ctx, "hw-amp", AMP_BYTES),
            amp_glow: load(ctx, "hw-amp-glow", AMP_GLOW_BYTES),
        }
    }
}

/// `QUICKRS_HW_PLAIN`: the highway without its skin, in RS's measured colours (the research folder's
/// side-by-sides with the game).
pub fn plain() -> bool {
    static PLAIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PLAIN.get_or_init(|| std::env::var_os("QUICKRS_HW_PLAIN").is_some())
}

/// The highway's background (owner request, P30.10, D93; config "hw_backdrop"): the living stage, one still
/// wallpaper, or the plain dark backdrop the highway had before its skin. The notes keep their material in all three.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Backdrop {
    /// Plates crossfading with the song, sections and streak heat; the amp pulsing; embers and haze on fire.
    #[default]
    Live,
    /// The blue stage plate, fixed: no crossfades, heat, embers or haze; the amp stands still, unlit by hits.
    Still,
    /// A dark backdrop, no plates and no amp.
    Plain,
}

impl Backdrop {
    pub const ALL: [Backdrop; 3] = [Backdrop::Live, Backdrop::Still, Backdrop::Plain];

    pub fn key(self) -> &'static str {
        match self {
            Backdrop::Live => "live",
            Backdrop::Still => "still",
            Backdrop::Plain => "plain",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Backdrop::Live => "Live",
            Backdrop::Still => "Still",
            Backdrop::Plain => "Plain",
        }
    }

    /// From the config's value (anything else: Live).
    pub fn parse(s: &str) -> Backdrop {
        Backdrop::ALL
            .into_iter()
            .find(|b| b.key() == s)
            .unwrap_or_default()
    }

    /// Live, Still, Plain, Live...
    pub fn next(self) -> Backdrop {
        match self {
            Backdrop::Live => Backdrop::Still,
            Backdrop::Still => Backdrop::Plain,
            Backdrop::Plain => Backdrop::Live,
        }
    }
}

/// Plain's backdrop (and `QUICKRS_HW_PLAIN`'s): the night blue the highway drew before its skin.
pub const PLAIN_BG: Color32 = Color32::from_rgb(4, 9, 18);

/// FNV-1a: stable across builds and machines (std's hasher isn't promised to be).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The look over one song: chosen once from the song, then smoothed frame to frame.
pub struct Look {
    /// 0 Bakelite, 1 patina copper, 2 worn paint.
    pub material: usize,
    home: usize,
    other: usize,
    swap_at: Option<f64>,
    solos: Vec<(f64, f64)>,
    quiet: Vec<(f64, f64)>,
    /// Each plate's share of the background (crossfades), the brightness, and the heat (0 cold .. 3 on fire).
    w: [f32; 6],
    v: f32,
    pub heat: f32,
}

impl Look {
    pub fn new(arr: &Arrangement) -> Look {
        let seed = fnv(&format!("{}|{}|{}", arr.artist, arr.title, arr.name));
        let material = [0, 0, 1, 2][(seed % 4) as usize];
        let home = if (seed >> 8).is_multiple_of(2) {
            BLUE
        } else {
            WHITE
        };
        let kind = |n: &str| {
            n.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ')
                .to_lowercase()
        };
        let span = |k: &str| {
            arr.sections
                .iter()
                .filter(|s| kind(&s.name) == k)
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>()
        };
        let solos = span("solo");
        let mid = arr
            .sections
            .first()
            .zip(arr.sections.last())
            .map_or(arr.song_length / 2.0, |(a, b)| (a.start + b.end) / 2.0);
        // two songs in three swap the stage once: at the first solo, or mid-song
        let swap_at = (!(seed >> 16).is_multiple_of(3)).then(|| solos.first().map_or(mid, |s| s.0));
        let mut w = [0.0; 6];
        w[home] = 1.0;
        Look {
            material,
            home,
            other: if home == BLUE { WHITE } else { BLUE },
            swap_at,
            solos,
            quiet: span("noguitar"),
            w,
            v: PLATE_V,
            heat: 0.0,
        }
    }

    pub fn pal(&self) -> &'static Pal {
        if self.material == 0 { &BAKELITE } else { &TUBE }
    }

    /// Move toward the look for song time `now` and a streak, over `dt` seconds of the clock. Still and Plain hold the
    /// blue stage, cold and at full brightness (Plain draws no plate, but keeps the look ready for a switch back).
    pub fn step(&mut self, now: f64, streak: usize, dt: f32, backdrop: Backdrop) {
        if backdrop != Backdrop::Live {
            self.w = [0.0; 6];
            self.w[BLUE] = 1.0;
            self.v = PLATE_V;
            self.heat = 0.0;
            return;
        }
        // screenshots (the research folder's side-by-sides) can force a heat step: QUICKRS_HW_HEAT=0..3
        static FORCED: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
        let forced =
            *FORCED.get_or_init(|| std::env::var("QUICKRS_HW_HEAT").ok().and_then(|v| v.parse().ok()));
        let tier = forced
            .unwrap_or_else(|| STREAK.iter().filter(|&&s| streak >= s).count())
            .min(3);
        let inside = |v: &[(f64, f64)]| v.iter().any(|&(a, b)| now >= a && now < b);
        let plate = if tier > 0 {
            HEAT[tier - 1]
        } else if inside(&self.solos) {
            SOLO
        } else if self.swap_at.is_some_and(|t| now >= t) {
            self.other
        } else {
            self.home
        };
        let quiet = tier == 0 && inside(&self.quiet);
        // 2 s crossfades (a heat step comes in faster)
        let k = 1.0 - (-dt / if tier > 0 { 0.5 } else { 0.7 }).exp();
        for (i, w) in self.w.iter_mut().enumerate() {
            *w += ((i == plate) as u8 as f32 - *w) * k;
        }
        self.v += (if quiet { QUIET_V * PLATE_V } else { PLATE_V } - self.v) * k;
        self.heat += (tier as f32 - self.heat) * k;
    }

    /// How much the scene dims (1 = normal), for the amp and the highway's own lights.
    pub fn dim(&self) -> f32 {
        self.v / PLATE_V
    }
}

/// Premultiplied grey `g` at opacity `a`.
fn grey(g: f32, a: f32) -> Color32 {
    let c = (g.clamp(0.0, 1.0) * a * 255.0) as u8;
    Color32::from_rgba_premultiplied(c, c, c, (a * 255.0) as u8)
}

fn image(p: &Painter, tex: TextureId, r: Rect, uv: Rect, tint: Color32) {
    let mut m = Mesh::with_texture(tex);
    m.add_rect_with_uv(r, uv, tint);
    p.add(Shape::mesh(m));
}

/// The plates drawn this frame (weight over 0.004), heaviest first.
fn shown(look: &Look) -> Vec<usize> {
    let mut order: Vec<usize> = (0..6).filter(|&i| look.w[i] > 0.004).collect();
    order.sort_by(|a, b| look.w[*b].total_cmp(&look.w[*a]));
    order
}

/// The stage behind the highway: the plates in their shares, cover-fitted to the well; Plain: the dark backdrop.
pub fn background(p: &Painter, well: Rect, tex: &Tex, look: &Look, backdrop: Backdrop) {
    if backdrop == Backdrop::Plain {
        p.rect_filled(well, 14.0, PLAIN_BG);
        return;
    }
    p.rect_filled(well, 14.0, Color32::BLACK);
    let order = shown(look);
    let mut sum = 0.0;
    for (n, &i) in order.iter().enumerate() {
        let t = &tex.plates[i];
        let s = t.size_vec2();
        let (ta, wa) = (s.x / s.y, well.width() / well.height().max(1.0));
        // cover: crop the plate's sides or top/bottom (keeping the lower part, where the stage is)
        let uv = if wa > ta {
            let h = ta / wa;
            Rect::from_min_max(pos2(0.0, (1.0 - h) * 0.6), pos2(1.0, (1.0 - h) * 0.6 + h))
        } else {
            let w = wa / ta;
            Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
        };
        sum += look.w[i];
        let a = if n == 0 { 1.0 } else { look.w[i] / sum };
        image(p, t.id(), well, uv, grey(look.v, a));
    }
}

/// The amp stack at the stage's left edge, like the game's (screen-anchored, so it never stands behind the notes),
/// drifting a little as the camera pans (unless `drift` is off: Still); `glow` lights its cones and tubes (added light).
pub fn amp(p: &Painter, v: &View, tex: &Tex, glow: f32, dim: f32, drift: bool) {
    let well = v.well;
    let s = tex.amp.size_vec2();
    let h = AMP_H * well.height();
    let w = h * s.x / s.y;
    let drift = if drift {
        (v.eye().x - 10.0) * well.width() * 0.004
    } else {
        0.0
    };
    let r = Rect::from_min_size(
        pos2(
            well.min.x - 0.05 * well.width() - drift,
            well.min.y + AMP_FOOT * well.height() - h,
        ),
        vec2(w, h),
    );
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    image(p, tex.amp.id(), r, uv, grey(0.85 * dim, 1.0));
    let g = (glow.clamp(0.0, 2.0) * 255.0 * 0.5) as u8;
    image(
        p,
        tex.amp_glow.id(),
        r,
        uv,
        Color32::from_rgba_premultiplied(g, g, g, 0),
    );
}

/// Embers rising over the stage and a warm haze from below, at heat `k` (0..1, the on-fire step).
pub fn embers(p: &Painter, well: Rect, now: f64, k: f32) {
    if k <= 0.01 {
        return;
    }
    let (w, h) = (well.width(), well.height());
    let haze = Color32::from_rgba_premultiplied((70.0 * k) as u8, (26.0 * k) as u8, (6.0 * k) as u8, 0);
    let mut m = Mesh::default();
    for (pt, c) in [
        (pos2(well.min.x, well.min.y + h * 0.45), Color32::TRANSPARENT),
        (pos2(well.max.x, well.min.y + h * 0.45), Color32::TRANSPARENT),
        (pos2(well.max.x, well.max.y), haze),
        (pos2(well.min.x, well.max.y), haze),
    ] {
        m.vertices.push(Vertex {
            pos: pt,
            uv: egui::epaint::WHITE_UV,
            color: c,
        });
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(Shape::mesh(m));
    let n = (220.0 * k) as u32;
    for i in 0..n {
        let r = |j: u32| (fnv(&format!("{i}:{j}")) % 10_000) as f32 / 10_000.0;
        let speed = 0.08 + 0.12 * r(1);
        let u = ((now as f32 * speed + r(2)) % 1.0 + 1.0) % 1.0;
        let x = well.min.x + r(0) * w + (u * 9.0 + r(3) * 6.0).sin() * 14.0;
        let y = well.max.y - u * h * 1.05;
        let len = (2.0 + 6.0 * r(4)) * (h / 1000.0);
        let a = k * (1.0 - u).powf(0.7) * (0.5 + 0.5 * r(5));
        let c = Color32::from_rgba_premultiplied(
            (255.0 * a) as u8,
            ((120.0 + 100.0 * r(6)) * a) as u8,
            (50.0 * a) as u8,
            0,
        );
        // a short hot spark in a soft glow
        let sz = (1.5 + 2.0 * r(7)) * (h / 1000.0);
        let soft = Color32::from_rgba_premultiplied(c.r() / 5, c.g() / 6, c.b() / 6, 0);
        p.circle_filled(pos2(x, y), sz * 3.0, soft);
        p.line_segment([pos2(x, y), pos2(x - len * 0.25, y + len)], Stroke::new(sz, c));
    }
}

/// A gem's cell in its material's atlas (low E first).
pub fn gem_uv(s: usize) -> Rect {
    Rect::from_min_max(pos2(s as f32 / 6.0, 0.0), pos2((s + 1) as f32 / 6.0, 1.0))
}

impl Tex {
    pub fn gems(&self, material: usize) -> TextureId {
        self.gems[material.min(2)].id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_stable() {
        // FNV-1a's published test vector: the look must not change between builds or machines
        assert_eq!(fnv("a"), 0xaf63_dc4c_8601_ec8c);
    }

    /// A song with a solo, a "noguitar" break and a stage swap.
    fn song() -> Arrangement {
        let mut arr = Arrangement {
            artist: "Test".into(),
            title: "Heat".into(),
            name: "Lead".into(),
            song_length: 200.0,
            ..Default::default()
        };
        for (number, (name, start, end)) in [
            ("verse", 0.0, 60.0),
            ("solo", 60.0, 90.0),
            ("noguitar", 90.0, 110.0),
            ("chorus", 110.0, 200.0),
        ]
        .into_iter()
        .enumerate()
        {
            arr.sections.push(qrs_formats::model::Section {
                name: name.into(),
                number: number as i32,
                start,
                end,
            });
        }
        arr
    }

    /// Steps over the song at 60 fps, the streak climbing past every heat step from the solo on; `see` each frame.
    fn play(look: &mut Look, backdrop: Backdrop, mut see: impl FnMut(&Look)) {
        let dt = 1.0 / 60.0;
        for f in 0..(200 * 60) {
            let now = f as f64 * dt as f64;
            let streak = if now >= 60.0 {
                ((now - 60.0) * 2.0) as usize
            } else {
                0
            };
            look.step(now, streak, dt, backdrop);
            see(look);
        }
    }

    #[test]
    fn live_changes_plate_and_heat() {
        let mut look = Look::new(&song());
        let (mut plates, mut hot) = (Vec::new(), 0.0f32);
        play(&mut look, Backdrop::Live, |l| {
            let top = shown(l)[0];
            if !plates.contains(&top) {
                plates.push(top);
            }
            hot = hot.max(l.heat);
        });
        assert!(plates.len() >= 3, "live moves through stages: {plates:?}");
        assert!(hot > 2.5, "live heats up: {hot}");
    }

    #[test]
    fn still_never_changes_plate_or_heat() {
        let mut look = Look::new(&song());
        // even from a hot, mid-crossfade Live look
        look.step(100.0, 120, 0.3, Backdrop::Live);
        let material = look.material;
        play(&mut look, Backdrop::Still, |l| {
            assert_eq!(shown(l), vec![BLUE], "one plate, the blue");
            assert_eq!(l.w[BLUE], 1.0);
            assert_eq!(l.heat, 0.0, "no heat");
            assert_eq!(l.dim(), 1.0, "no dimming");
            assert_eq!(l.material, material, "the notes keep their material");
        });
    }

    /// The plate images one `background` call draws, the look caught mid-crossfade into the solo.
    fn plates_drawn(backdrop: Backdrop) -> usize {
        let ctx = Context::default();
        ctx.begin_pass(Default::default());
        let tex = Tex::load(&ctx);
        let ids: Vec<TextureId> = tex.plates.iter().map(|t| t.id()).collect();
        let mut look = Look::new(&song());
        look.step(60.5, 0, 0.5, backdrop);
        let p = ctx.layer_painter(egui::LayerId::background());
        let well = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 450.0));
        background(&p, well, &tex, &look, backdrop);
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        out.shapes
            .iter()
            .filter(|s| matches!(&s.shape, Shape::Mesh(m) if ids.contains(&m.texture_id)))
            .count()
    }

    #[test]
    fn plain_draws_no_plate_and_still_one() {
        assert_eq!(plates_drawn(Backdrop::Live), 2, "live crossfades two plates");
        assert_eq!(plates_drawn(Backdrop::Still), 1);
        assert_eq!(plates_drawn(Backdrop::Plain), 0);
    }

    #[test]
    fn backdrop_keys_round_trip_and_cycle() {
        for b in Backdrop::ALL {
            assert_eq!(Backdrop::parse(b.key()), b);
        }
        assert_eq!(Backdrop::parse("nonsense"), Backdrop::Live);
        assert_eq!(Backdrop::Live.next(), Backdrop::Still);
        assert_eq!(Backdrop::Still.next(), Backdrop::Plain);
        assert_eq!(Backdrop::Plain.next(), Backdrop::Live);
    }
}
