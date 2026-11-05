//! The AMP ROOM button as an amp head (the owner's mockups A6 then A7, 2026-11-03; D73): a flat panel, zoomed to fill
//! the button (no tolex round it),
//! in the theme's material with the VU meter in its chrome bezel and the lettering printed on it, and over the whole
//! face a clear, glossy acrylic sheet; the meter's dial backlit warm white. Drawn as the pedal faces are: the body,
//! the glass over the needle and the plastic over everything with tiny-skia once per size and cached; the needle, the
//! peak lamp (it glows with how hard you play) and the lettering live. The mockup's units: the button 232 x 56, scaled
//! by its height, the panel stretched to its width (what's right of the middle keeps its distance from the right edge).

use eframe::egui;
use egui::{Align2, Color32, Painter, Rect, Stroke as EStroke, pos2, vec2};
use qrs_ui::paint::{self, Store};
use qrs_ui::raster::{self, Image};
use tiny_skia::{Color, FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::listen::Listen;
use crate::pedalfaces::{Lettering, lettering, linear, radial, rrect, shade, sk, solid};

const UW: f32 = 232.0;
/// The meter's bezel's top left (units), its face inside the bezel, and the needle's pivot (under the face).
const MX: f32 = 16.0;
const MY: f32 = 8.0;
const FACE: (f32, f32, f32, f32) = (4.0, 4.0, 52.0, 32.0);
const PIVOT: (f32, f32) = (30.0, 42.0);
/// The peak lamp, inside the window at the top right.
const LAMP: (f32, f32) = (51.0, 9.0);

/// Units across for a button `w` x `h` (its height is 56 units).
fn units_wide(w: f32, h: f32) -> f32 {
    w / h * VH + 2.0 * VX
}

/// What the button shows of the mockup: its panel (from (VX, VY), VH tall), zoomed to fill it, the tolex round it
/// left out (owner request, 2026-11-03).
const VX: f32 = 5.0;
const VY: f32 = 4.5;
const VH: f32 = 47.0;

/// Mockup units to the pixmap's pixels for a button `h` pixels tall: the scale and the transform.
fn view(h: u32) -> (f32, Transform) {
    let s = h as f32 / VH;
    (s, Transform::from_scale(s, s).pre_translate(-VX, -VY))
}

/// A unit's x on a button `wu` units wide: left of the middle as drawn, right of it kept off the right edge.
fn ux(x: f32, wu: f32) -> f32 {
    if x <= UW / 2.0 { x } else { wu - (UW - x) }
}

fn stroke(w: f32) -> Stroke {
    Stroke {
        width: w,
        ..Default::default()
    }
}

fn line(a: (f32, f32), b: (f32, f32)) -> Option<Path> {
    let mut p = PathBuilder::new();
    p.move_to(a.0, a.1);
    p.line_to(b.0, b.1);
    p.finish()
}

/// An arc round the pivot at radius `r` from `a0` to `a1` degrees (0 straight up, clockwise).
fn arc(r: f32, a0: f32, a1: f32) -> Option<Path> {
    let mut p = PathBuilder::new();
    let n = ((a1 - a0).abs() / 1.5).ceil().max(1.0) as usize;
    for i in 0..=n {
        let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
        let (x, y) = (PIVOT.0 + r * a.sin(), PIVOT.1 - r * a.cos());
        if i == 0 { p.move_to(x, y) } else { p.line_to(x, y) }
    }
    p.finish()
}

fn fill(pm: &mut Pixmap, path: Option<Path>, paint: &Paint, t: Transform, mask: Option<&Mask>) {
    if let Some(path) = path {
        pm.fill_path(&path, paint, FillRule::Winding, t, mask);
    }
}

fn outline(pm: &mut Pixmap, path: Option<Path>, paint: &Paint, w: f32, t: Transform) {
    if let Some(path) = path {
        pm.stroke_path(&path, paint, &stroke(w), t, None);
    }
}

fn mask_of(w: u32, h: u32, path: Option<Path>, t: Transform) -> Option<Mask> {
    let mut m = Mask::new(w, h)?;
    m.fill_path(&path?, FillRule::Winding, true, t);
    Some(m)
}

fn rgb(c: [u8; 3]) -> Color {
    sk(c, 1.0)
}

/// The plate's finish: the theme's own material as a sheet of metal (Graphite's graphite, Midnight's blue, Daylight's
/// light with an orange tint; owner request, 2026-11-03), its grooves and edges from that colour.
#[derive(Clone, Copy, Hash)]
struct Finish {
    mid: [u8; 3],
    dark: bool,
}

impl Finish {
    fn of_theme() -> Finish {
        let th = qrs_ui::theme::t();
        let c = [th.base.r(), th.base.g(), th.base.b()];
        let mid = if th.dark {
            shade(c, 0.10)
        } else {
            blend(c, [240, 168, 96], 0.22)
        };
        Finish { mid, dark: th.dark }
    }
}

fn blend(a: [u8; 3], b: [u8; 3], k: f32) -> [u8; 3] {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

/// The body: tolex, piping, the plate and its screws, the meter's fitting, bezel and dial with its scale. `w` x `h`
/// pixels, the plate in `f`.
fn body(w: u32, h: u32, f: Finish) -> Image {
    let Some(mut pm) = Pixmap::new(w, h) else {
        return Image::new(1, 1);
    };
    let (s, t) = view(h);
    let wu = w as f32 / s + 2.0 * VX;
    // the panel under the plastic (mockup A7): flat, in the theme's material, shaded where the plastic's frame
    // meets it
    let (px, py, pw, ph) = (5.0, 4.5, wu - 10.0, 47.0);
    let panel = || rrect(px, py, pw, ph, 4.0);
    fill(&mut pm, panel(), &solid(rgb(f.mid)), t, None);
    if let Some(p) = linear(
        (0.0, py),
        (0.0, py + ph),
        vec![
            (0.0, sk([0, 0, 0], 0.18)),
            (0.12, sk([0, 0, 0], 0.0)),
            (0.9, sk([0, 0, 0], 0.0)),
            (1.0, sk([0, 0, 0], 0.12)),
        ],
    ) {
        fill(&mut pm, panel(), &p, t, None);
    }
    outline(&mut pm, panel(), &solid(sk([0, 0, 0], 0.45)), 0.6, t);
    // the meter's bezel sits on it, its shadow under it
    let mt = t.pre_translate(MX, MY);
    fill(
        &mut pm,
        rrect(0.7, 1.1, 60.0, 40.0, 6.0),
        &solid(sk([0, 0, 0], 0.3)),
        mt,
        None,
    );
    fill(
        &mut pm,
        rrect(0.35, 0.7, 60.0, 40.0, 6.0),
        &solid(sk([0, 0, 0], 0.2)),
        mt,
        None,
    );
    // the bezel: chrome, a lit rim, an inner bevel down to the black lip
    if let Some(p) = linear(
        (0.0, 0.0),
        (0.0, 40.0),
        vec![
            (0.0, rgb([244, 244, 242])),
            (0.35, rgb([154, 154, 152])),
            (0.7, rgb([92, 92, 90])),
            (1.0, rgb([216, 216, 212])),
        ],
    ) {
        fill(&mut pm, rrect(0.0, 0.0, 60.0, 40.0, 6.0), &p, mt, None);
    }
    if let Some(p) = linear(
        (0.0, 0.0),
        (60.0, 40.0),
        vec![
            (0.0, sk([255, 255, 255], 0.9)),
            (0.5, sk([255, 255, 255], 0.15)),
            (1.0, sk([0, 0, 0], 0.35)),
        ],
    ) {
        outline(&mut pm, rrect(0.35, 0.35, 59.3, 39.3, 5.7), &p, 0.7, mt);
    }
    outline(
        &mut pm,
        rrect(1.3, 1.3, 57.4, 37.4, 5.0),
        &solid(sk([43, 43, 42], 0.6)),
        0.35,
        mt,
    );
    if let Some(p) = linear(
        (0.0, 1.6),
        (0.0, 38.4),
        vec![
            (0.0, rgb([74, 74, 72])),
            (0.5, rgb([142, 142, 139])),
            (1.0, rgb([237, 237, 234])),
        ],
    ) {
        fill(&mut pm, rrect(1.6, 1.6, 56.8, 36.8, 4.7), &p, mt, None);
    }
    fill(
        &mut pm,
        rrect(2.6, 2.6, 54.8, 34.8, 3.8),
        &solid(rgb([10, 10, 10])),
        mt,
        None,
    );
    // the dial: warm white paper, the backlight's warmth low down
    let (fx, fy, fw, fh) = FACE;
    if let Some(p) = radial(
        (30.0, 37.6),
        40.0,
        vec![
            (0.0, rgb([255, 246, 220])),
            (0.45, rgb([248, 240, 220])),
            (0.8, rgb([235, 223, 192])),
            (1.0, rgb([207, 191, 152])),
        ],
    ) {
        fill(&mut pm, rrect(fx, fy, fw, fh, 2.6), &p, mt, None);
    }
    // the scale: -20 to 0 in black, 0 to +3 a thick red band, the ticks
    outline(
        &mut pm,
        arc(26.0, -38.7, 18.9),
        &solid(rgb([26, 26, 26])),
        0.45,
        mt,
    );
    outline(
        &mut pm,
        arc(26.0, 18.9, 45.0),
        &solid(rgb([176, 24, 14])),
        1.6,
        mt,
    );
    for (a, major, red) in [
        (-38.7, true, false),
        (-25.2, false, false),
        (-16.2, false, false),
        (-9.0, true, false),
        (0.0, false, false),
        (5.4, false, false),
        (11.7, false, false),
        (18.9, true, true),
        (26.1, false, true),
        (35.1, false, true),
        (45.0, true, true),
    ] {
        let (r0, r1) = match (major, red) {
            (true, true) => (22.9, 26.4),
            (true, false) => (23.0, 26.0),
            (false, true) => (23.2, 26.2),
            (false, false) => (23.4, 25.4),
        };
        let ar = (a as f32).to_radians();
        let at = |r: f32| (PIVOT.0 + r * ar.sin(), PIVOT.1 - r * ar.cos());
        let c = if red {
            rgb([176, 24, 14])
        } else {
            rgb([26, 26, 26])
        };
        outline(
            &mut pm,
            line(at(r0), at(r1)),
            &solid(c),
            if red { 0.65 } else { 0.55 },
            mt,
        );
    }
    outline(
        &mut pm,
        line((9.0, 33.4), (13.0, 33.4)),
        &solid(sk([107, 74, 28], 0.6)),
        0.4,
        mt,
    );
    raster::from_pixmap(&pm)
}

/// The glass over the needle: the pivot's cover, the bezel lip's shadow on the dial, one glare, the lip's lit
/// bottom edge. Transparent elsewhere; the same size as `body`.
fn glass(w: u32, h: u32) -> Image {
    let Some(mut pm) = Pixmap::new(w, h) else {
        return Image::new(1, 1);
    };
    let (_, t) = view(h);
    let mt = t.pre_translate(MX, MY);
    let (fx, fy, fw, fh) = FACE;
    let m = mask_of(w, h, rrect(fx, fy, fw, fh, 2.6), mt);
    let m = m.as_ref();
    let mut cover = PathBuilder::new();
    cover.move_to(20.0, 36.0);
    cover.cubic_to(20.0, 32.36, 24.48, 29.4, 30.0, 29.4);
    cover.cubic_to(35.52, 29.4, 40.0, 32.36, 40.0, 36.0);
    cover.close();
    fill(&mut pm, cover.finish(), &solid(rgb([14, 14, 14])), mt, m);
    let mut hl = PathBuilder::new();
    hl.move_to(21.0, 35.6);
    hl.cubic_to(21.0, 32.4, 25.0, 29.8, 30.0, 29.8);
    hl.cubic_to(35.0, 29.8, 39.0, 32.4, 39.0, 35.6);
    if let Some(path) = hl.finish() {
        pm.stroke_path(&path, &solid(sk([58, 58, 58], 1.0)), &stroke(0.4), mt, m);
    }
    if let Some(p) = linear(
        (0.0, fy),
        (0.0, fy + 5.0),
        vec![(0.0, sk([0, 0, 0], 0.45)), (1.0, sk([0, 0, 0], 0.0))],
    ) {
        fill(&mut pm, rrect(fx, fy, fw, 5.0, 0.0), &p, mt, m);
    }
    if let Some(p) = linear(
        (fx, 0.0),
        (fx + 2.2, 0.0),
        vec![(0.0, sk([0, 0, 0], 0.3)), (1.0, sk([0, 0, 0], 0.0))],
    ) {
        fill(&mut pm, rrect(fx, fy, 2.2, fh, 0.0), &p, mt, m);
    }
    if let Some(p) = linear(
        (fx + fw, 0.0),
        (fx + fw - 2.2, 0.0),
        vec![(0.0, sk([0, 0, 0], 0.3)), (1.0, sk([0, 0, 0], 0.0))],
    ) {
        fill(&mut pm, rrect(fx + fw - 2.2, fy, 2.2, fh, 0.0), &p, mt, m);
    }
    let mut glare = PathBuilder::new();
    glare.move_to(4.0, 4.0);
    glare.line_to(38.0, 4.0);
    glare.quad_to(26.0, 9.0, 14.0, 18.0);
    glare.line_to(4.0, 19.0);
    glare.close();
    if let Some(p) = linear(
        (0.0, 4.0),
        (0.0, 19.0),
        vec![(0.0, sk([255, 255, 255], 0.16)), (1.0, sk([255, 255, 255], 0.02))],
    ) {
        fill(&mut pm, glare.finish(), &p, mt, m);
    }
    fill(
        &mut pm,
        rrect(51.6, 7.0, 0.9, 22.0, 0.0),
        &solid(sk([255, 255, 255], 0.06)),
        mt,
        m,
    );
    outline(
        &mut pm,
        line((4.6, 36.4), (55.4, 36.4)),
        &solid(sk([255, 255, 255], 0.28)),
        0.35,
        mt,
    );
    raster::from_pixmap(&pm)
}

/// The clear acrylic sheet over the whole face (mockup A7): a faint reflection fading down, a broad diagonal gloss, a
/// highlight streak near its top, its shadow along the bottom, polished edges catching the light (bright top and
/// left, a line glowing through its thickness along the bottom). Transparent elsewhere; the same size as `body`.
fn plexi(w: u32, h: u32) -> Image {
    let Some(mut pm) = Pixmap::new(w, h) else {
        return Image::new(1, 1);
    };
    let (s, t) = view(h);
    let wu = w as f32 / s + 2.0 * VX;
    let (x, y, sw, sh) = (VX, VY, wu - 2.0 * VX, VH);
    let sheet = || rrect(x, y, sw, sh, 4.0);
    let m = mask_of(w, h, sheet(), t);
    let m = m.as_ref();
    if let Some(p) = linear(
        (0.0, y),
        (0.0, y + sh),
        vec![
            (0.0, sk([255, 255, 255], 0.12)),
            (0.45, sk([255, 255, 255], 0.02)),
            (1.0, sk([255, 255, 255], 0.0)),
        ],
    ) {
        fill(&mut pm, sheet(), &p, t, m);
    }
    let mut gloss = PathBuilder::new();
    gloss.move_to(x, y);
    gloss.line_to(x + 114.0, y);
    gloss.line_to(x + 62.0, y + sh);
    gloss.line_to(x, y + sh);
    gloss.close();
    if let Some(p) = linear(
        (x, y),
        (x + sw, y + 0.55 * sw),
        vec![
            (0.0, sk([255, 255, 255], 0.20)),
            (0.3, sk([255, 255, 255], 0.10)),
            (0.31, sk([255, 255, 255], 0.03)),
            (0.6, sk([255, 255, 255], 0.0)),
            (1.0, sk([255, 255, 255], 0.04)),
        ],
    ) {
        fill(&mut pm, gloss.finish(), &p, t, m);
    }
    let streak_w = (sw * 0.67).min(150.0);
    if let Some(p) = linear(
        (18.0, 0.0),
        (18.0 + streak_w, 0.0),
        vec![
            (0.0, sk([255, 255, 255], 0.0)),
            (0.15, sk([255, 255, 255], 0.55)),
            (0.7, sk([255, 255, 255], 0.25)),
            (1.0, sk([255, 255, 255], 0.0)),
        ],
    ) {
        fill(&mut pm, rrect(18.0, y + 1.3, streak_w, 1.3, 0.65), &p, t, m);
    }
    for (dy, a) in [(-0.5, 0.06), (0.0, 0.12), (0.5, 0.06)] {
        outline(
            &mut pm,
            line((x + 1.0, y + sh - 1.4 + dy), (x + sw - 1.0, y + sh - 1.4 + dy)),
            &solid(sk([0, 0, 0], a)),
            0.6,
            t,
        );
    }
    if let Some(p) = linear(
        (x, y),
        (x + sh, y + sh),
        vec![
            (0.0, sk([255, 255, 255], 0.85)),
            (0.5, sk([255, 255, 255], 0.25)),
            (1.0, sk([255, 255, 255], 0.45)),
        ],
    ) {
        outline(&mut pm, sheet(), &p, 0.7, t);
    }
    outline(
        &mut pm,
        rrect(x + 1.0, y + 1.0, sw - 2.0, sh - 2.0, 3.2),
        &solid(sk([255, 255, 255], 0.14)),
        0.5,
        t,
    );
    outline(
        &mut pm,
        line((x + 3.0, y + sh - 0.6), (x + sw - 3.0, y + sh - 0.6)),
        &solid(sk([255, 255, 255], 0.4)),
        0.45,
        t,
    );
    raster::from_pixmap(&pm)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// How brightly the peak lamp glows (0 to 1): with the needle (so with the VU's ballistics: it swells and fades
/// with your playing rather than flicker), from about -10 VU; full while it's too hot.
pub fn lamp_glow(listen: &Listen) -> f32 {
    if listen.lamp() {
        1.0
    } else {
        ((listen.needle - 0.3) / 0.6).clamp(0.0, 1.0)
    }
}

/// The lamp's colour at a glow: dark red, red, bright red, orange-white.
fn lamp_colour(i: f32) -> Color32 {
    let stops = [
        (0.0, Color32::from_rgb(0x3a, 0x08, 0x06)),
        (0.35, Color32::from_rgb(0xa0, 0x18, 0x10)),
        (0.7, Color32::from_rgb(0xff, 0x3a, 0x1c)),
        (1.0, Color32::from_rgb(0xff, 0xf0, 0xd0)),
    ];
    for w in stops.windows(2) {
        if i <= w[1].0 {
            return mix(w[0].1, w[1].1, (i - w[0].0) / (w[1].0 - w[0].0));
        }
    }
    stops[3].1
}

/// The button in `r` (points): the head, the needle at `needle` (0..1), the lamp at `glow`, and engraved on the
/// plate "Amp Room" and the rig (or, too hot, what's wrong), centred between the meter and the right-hand screws;
/// `plexi_on`: the acrylic sheet over it all (left off under a glass key, which takes its place: P34.7).
pub fn draw(p: &Painter, r: Rect, needle: f32, glow: f32, sub: &str, hot: bool, plexi_on: bool) {
    let ppp = p.pixels_per_point();
    let (x0, y0) = ((r.min.x * ppp).round(), (r.min.y * ppp).round());
    let (w, h) = (
        (r.width() * ppp).round().max(2.0) as u32,
        (r.height() * ppp).round().max(2.0) as u32,
    );
    let ctx = p.ctx();
    // (the plate in the theme's material: drawn again when the theme changes)
    let f = Finish::of_theme();
    let (tex, size) = paint::texture(ctx, Store::Face, paint::key_of(("amphead-body", w, h, f)), || {
        body(w, h, f)
    });
    paint::blit(p, tex, size, x0, y0, Color32::WHITE);
    // (in points from here, on the texture's whole pixels)
    let r = Rect::from_min_size(pos2(x0 / ppp, y0 / ppp), r.size());
    let u = r.height() / VH;
    let wu = units_wide(r.width(), r.height());
    let at = |x: f32, y: f32| pos2(r.min.x + (ux(x, wu) - VX) * u, r.min.y + (y - VY) * u);
    let m = |x: f32, y: f32| pos2(r.min.x + (MX + x - VX) * u, r.min.y + (MY + y - VY) * u);
    let face = Rect::from_min_max(m(FACE.0, FACE.1), m(FACE.0 + FACE.2, FACE.1 + FACE.3));
    let fp = p.with_clip_rect(face.intersect(p.clip_rect()));
    // the needle and its shadow on the dial
    let a = (-46.0 + 91.0 * needle.clamp(0.0, 1.0)).to_radians();
    let pivot = m(PIVOT.0, PIVOT.1);
    let tip = pivot + vec2(a.sin(), -a.cos()) * 29.5 * u;
    let sh = vec2(0.8, 0.9) * u;
    fp.line_segment(
        [pivot + sh, tip + sh],
        EStroke::new(0.9 * u, Color32::from_black_alpha(56)),
    );
    fp.line_segment(
        [pivot, tip],
        EStroke::new(0.7 * u, Color32::from_rgb(0x11, 0x11, 0x11)),
    );
    // the peak lamp: its glow over the dial, then the lamp
    let lc = m(LAMP.0, LAMP.1);
    let i = glow.clamp(0.0, 1.0);
    if i > 0.6 {
        let k = (i - 0.6) / 0.4;
        paint::halo(
            &fp,
            lc,
            16.0 * u,
            Color32::from_rgb(0xff, 0x6a, 0x2a),
            (70.0 * k) as u8,
        );
    }
    if i > 0.05 {
        let c = mix(
            Color32::from_rgb(0xd0, 0x30, 0x1c),
            Color32::from_rgb(0xff, 0x8a, 0x3a),
            i,
        );
        paint::halo(&fp, lc, (3.0 + 5.0 * i) * u, c, (60.0 + 140.0 * i) as u8);
    }
    let (tex, size) = paint::texture(ctx, Store::Face, paint::key_of(("amphead-glass", w, h)), || {
        glass(w, h)
    });
    paint::blit(p, tex, size, x0, y0, Color32::WHITE);
    if i > 0.75 {
        p.circle_filled(lc, 1.9 * u, Color32::from_rgb(0xff, 0x7a, 0x2a));
    }
    p.circle_filled(lc, if i > 0.75 { 1.15 } else { 1.6 } * u, lamp_colour(i));
    if i < 0.6 {
        p.circle_filled(
            lc + vec2(-0.5, -0.6) * u,
            0.45 * u,
            Color32::from_white_alpha(115),
        );
    }
    // the dial's print: a few numbers and VU
    let ink = Color32::from_rgb(0x1a, 0x1a, 0x1a);
    let red = Color32::from_rgb(0xb0, 0x18, 0x0e);
    for (s, x, y, c) in [
        ("20", 11.8, 18.4, ink),
        ("10", 17.6, 14.6, ink),
        ("5", 25.4, 12.2, ink),
        ("0", 39.6, 13.2, red),
        ("+3", 50.6, 19.6, red),
    ] {
        lettering(
            p,
            s,
            m(x, y),
            Align2::CENTER_CENTER,
            3.4 * u,
            c,
            8.0 * u,
            Lettering::Condensed,
            false,
        );
    }
    lettering(
        p,
        "VU",
        m(30.0, 25.2),
        Align2::CENTER_CENTER,
        5.6 * u,
        ink,
        12.0 * u,
        Lettering::Condensed,
        false,
    );
    // the lettering, centred between the meter's border and the right-hand screws: Amp Room, a rule, the rig (or
    // what's wrong)
    let (l, rt) = (at(82.0, 0.0).x, at(UW - 15.0, 0.0).x);
    let (mid, room) = ((l + rt) / 2.0, (rt - l).max(10.0 * u));
    let (ink, ink2) = if f.dark {
        (
            Color32::from_rgb(0xdc, 0xd9, 0xd2),
            Color32::from_rgb(0xc9, 0xc6, 0xbf),
        )
    } else {
        (
            Color32::from_rgb(0x1f, 0x22, 0x26),
            Color32::from_rgb(0x2a, 0x2e, 0x33),
        )
    };
    // printed on the panel under the plastic
    lettering(
        p,
        "Amp Room",
        pos2(mid, at(0.0, 20.5).y),
        Align2::CENTER_CENTER,
        21.0 * u,
        ink,
        room,
        Lettering::Script,
        false,
    );
    let half = (room * 0.36).min(36.0 * u);
    let ry = at(0.0, 31.8).y;
    p.line_segment(
        [pos2(mid - half, ry), pos2(mid + half, ry)],
        EStroke::new(0.4 * u, ink2.gamma_multiply(0.6)),
    );
    let sub_c = if hot {
        if f.dark {
            Color32::from_rgb(0xff, 0x6a, 0x5a)
        } else {
            Color32::from_rgb(0x8a, 0x16, 0x0c)
        }
    } else {
        ink2
    };
    lettering(
        p,
        sub,
        pos2(mid, at(0.0, 40.0).y),
        Align2::CENTER_CENTER,
        9.0 * u,
        sub_c,
        room,
        Lettering::Condensed,
        true,
    );
    // the plastic over everything
    if !plexi_on {
        return;
    }
    let (tex, size) = paint::texture(ctx, Store::Face, paint::key_of(("amphead-plexi", w, h)), || {
        plexi(w, h)
    });
    paint::blit(p, tex, size, x0, y0, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_right_side_keeps_its_distance_from_the_right_edge() {
        assert_eq!(ux(9.3, 260.0), 9.3);
        assert!((ux(222.7, 260.0) - 250.7).abs() < 1e-4);
        assert!((units_wide(222.0, 47.0) - UW).abs() < 1e-4);
    }

    #[test]
    fn the_lamp_goes_from_dark_red_to_orange_white() {
        assert_eq!(lamp_colour(0.0), Color32::from_rgb(0x3a, 0x08, 0x06));
        assert_eq!(lamp_colour(1.0), Color32::from_rgb(0xff, 0xf0, 0xd0));
        let mid = lamp_colour(0.7);
        assert!(mid.r() == 0xff && mid.g() < 0x60);
    }

    #[test]
    fn the_body_and_glass_draw_at_any_size() {
        for (w, h) in [(232, 56), (464, 120), (300, 60)] {
            let b = body(
                w,
                h,
                Finish {
                    mid: [200, 200, 200],
                    dark: false,
                },
            );
            let g = glass(w, h);
            assert_eq!((b.w, b.h), (w as usize, h as usize));
            assert_eq!((g.w, g.h), (w as usize, h as usize));
        }
    }
}
