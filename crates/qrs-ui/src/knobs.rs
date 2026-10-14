//! Real knobs (a knurled skirt, a cap and an indicator line; Vintage: a chicken-head pointer) with their scale
//! printed round them, and the gauges: the accuracy ring, the tuner's arc and the level meters. Port of
//! rsplay/neu.py (`knob`, `_make_knob`, `_make_cast`, `_make_pointer`, `_tick_ring`, `ring_gauge`, `_arc_band`,
//! `arc_gauge`, `meter`, `led_meter`): the numpy recipes run the same way, and what it draws with pygame (the
//! ticks, the arcs, the pointer and its rotation, the indicator line, the smoothscale of everything drawn at 3-4x)
//! goes through crate::raster::pg, pygame's own routines.

use std::sync::atomic::{AtomicU8, Ordering};

use egui::{Color32, Painter, Pos2, Rect};

use crate::paint::{
    Fill, PxRect, Store, blit, ck, cover, inset_px, ipx, key_of, pill_px, pxr, raised_px, rrect_px, sc,
    texture,
};
use crate::raster::pg::{self, Surf};
use crate::raster::{Image, Layers, Mask, mix};
use crate::theme::{self, t};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnobStyle {
    Amp,
    Pedal,
    Chrome,
    Vintage,
}

impl KnobStyle {
    pub const ALL: [KnobStyle; 4] = [
        KnobStyle::Amp,
        KnobStyle::Pedal,
        KnobStyle::Chrome,
        KnobStyle::Vintage,
    ];

    pub fn key(self) -> &'static str {
        match self {
            KnobStyle::Amp => "amp",
            KnobStyle::Pedal => "pedal",
            KnobStyle::Chrome => "chrome",
            KnobStyle::Vintage => "vintage",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KnobStyle::Amp => "Amp gold",
            KnobStyle::Pedal => "Pedal black",
            KnobStyle::Chrome => "Chrome",
            KnobStyle::Vintage => "Vintage",
        }
    }

    pub fn parse(s: &str) -> KnobStyle {
        match s {
            "pedal" => KnobStyle::Pedal,
            "chrome" => KnobStyle::Chrome,
            "vintage" => KnobStyle::Vintage,
            _ => KnobStyle::Amp,
        }
    }
}

static STYLE: AtomicU8 = AtomicU8::new(0);

/// The knob style chosen in Settings > Display.
pub fn set_style(s: KnobStyle) {
    STYLE.store(
        KnobStyle::ALL.iter().position(|k| *k == s).unwrap_or(0) as u8,
        Ordering::Relaxed,
    );
}

pub fn style() -> KnobStyle {
    KnobStyle::ALL[STYLE.load(Ordering::Relaxed) as usize % 4]
}

const GOLD: [(f32, [f32; 3]); 11] = [
    (0.0, [232.0, 197.0, 110.0]),
    (0.09, [168.0, 123.0, 37.0]),
    (0.18, [251.0, 226.0, 163.0]),
    (0.28, [180.0, 138.0, 51.0]),
    (0.38, [243.0, 213.0, 140.0]),
    (0.48, [164.0, 119.0, 31.0]),
    (0.58, [251.0, 230.0, 173.0]),
    (0.68, [178.0, 135.0, 47.0]),
    (0.78, [240.0, 208.0, 131.0]),
    (0.88, [168.0, 123.0, 37.0]),
    (1.0, [232.0, 197.0, 110.0]),
];
const SILVER: [(f32, [f32; 3]); 11] = [
    (0.0, [230.0, 232.0, 235.0]),
    (0.09, [152.0, 158.0, 166.0]),
    (0.18, [248.0, 249.0, 250.0]),
    (0.28, [162.0, 167.0, 175.0]),
    (0.38, [238.0, 240.0, 242.0]),
    (0.48, [149.0, 155.0, 163.0]),
    (0.58, [249.0, 250.0, 251.0]),
    (0.68, [160.0, 165.0, 173.0]),
    (0.78, [236.0, 238.0, 240.0]),
    (0.88, [152.0, 158.0, 166.0]),
    (1.0, [230.0, 232.0, 235.0]),
];

/// Colours from conic-gradient stops (fractions of a turn) at `a` in [0, 1): np.interp (in f64) of the f32 stops.
fn conic(a: f32, stops: &[(f32, [f32; 3])]) -> [f32; 3] {
    let x = a as f64;
    if x <= stops[0].0 as f64 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        let ((p0, c0), (p1, c1)) = (w[0], w[1]);
        let (p0, p1) = (p0 as f64, p1 as f64);
        if x < p1 {
            return std::array::from_fn(|i| {
                let slope = (c1[i] as f64 - c0[i] as f64) / (p1 - p0);
                (slope * (x - p0) + c0[i] as f64) as f32
            });
        }
    }
    stops[stops.len() - 1].1
}

/// `_knob_margin`: `max(px(5), int(size * 0.24))`.
fn knob_margin(size: i32, s: f64) -> i32 {
    pxr(5.0, s).max((size as f64 * 0.24) as i32)
}

fn make_cast(size: i32, st: KnobStyle, s: f64) -> Image {
    let m = knob_margin(size, s);
    let n = (size + 2 * m) as usize;
    let br = size as f64 / 2.0 * if st == KnobStyle::Vintage { 0.84 } else { 1.0 };
    let half = n as f64 / 2.0;
    let disc = Mask::rrect(
        (2.0 * br) as f32,
        (2.0 * br) as f32,
        br as f32,
        (half - br) as f32,
        (half - br) as f32,
        n,
        n,
    );
    let th = t();
    let mut l = Layers::new(n, n);
    l.over_c(
        th.cast,
        &disc
            .blur64((size as f64 * 0.055).max(0.8))
            .shift(0, ((size as f64 * 0.065) as i32).max(1))
            .scale(th.cast_a),
    );
    l.over_c(th.cast, &disc.blur64(0.7).shift(0, 1).scale(th.cast_a * 0.9));
    l.image(None)
}

/// A knob's body (everything but the indicator and its shadow), the same in every theme: drawn at 3x and
/// smoothscaled down.
fn make_knob(size: i32, st: KnobStyle, s: f64) -> Image {
    let ss = 3usize;
    let m = knob_margin(size, s);
    let n = (size as usize + 2 * m as usize) * ss;
    let c = n as f32 / 2.0;
    let r = (size as usize * ss) as f32 / 2.0;
    let ssf = ss as f32;
    let body_r = r * if st == KnobStyle::Vintage { 0.84 } else { 1.0 };
    let cap_r = r * (1.0 - 2.0 * if st == KnobStyle::Pedal { 0.12 } else { 0.15 });
    let step = if size >= pxr(120.0, s) { 2.0 } else { 3.0 };
    let mut l = Layers::new(n, n);
    let layer = |l: &mut Layers, f: &dyn Fn(f32, f32, f32, f32, f32) -> Option<([f32; 3], f32)>| {
        let mut cols = vec![[0.0f32; 3]; n * n];
        let mut a = Mask::new(n, n);
        for y in 0..n {
            for x in 0..n {
                let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
                let dist = dx.hypot(dy);
                let ang = (dx.atan2(-dy).to_degrees() + 360.0) % 360.0;
                let yy = y as f32 + 0.5;
                if let Some((col, al)) = f(dx, dy, dist, ang, yy) {
                    cols[y * n + x] = col;
                    a.v[y * n + x] = al;
                }
            }
        }
        l.over_img(&cols, &a);
    };
    let disc = |dist: f32| ((body_r - dist) / ssf + 0.5).clamp(0.0, 1.0);
    if st == KnobStyle::Vintage {
        layer(&mut l, &|dx, dy, dist, _, _| {
            let rr = (dx + 0.16 * body_r).hypot(dy + 0.36 * body_r) / body_r;
            let k = (rr / 1.4).clamp(0.0, 1.0);
            let base = [
                60.0 + (28.0 - 60.0) * k,
                63.0 + (29.0 - 63.0) * k,
                69.0 + (32.0 - 69.0) * k,
            ];
            Some((base, disc(dist)))
        });
    } else {
        let chrome = st == KnobStyle::Chrome;
        let (dark, light) = if chrome {
            ([141.0, 146.0, 154.0], [227.0, 230.0, 234.0])
        } else {
            ([18.0, 19.0, 21.0], [58.0, 61.0, 67.0])
        };
        let (hi_a, lo_a) = if chrome { (0.35, 0.45) } else { (0.16, 0.55) };
        layer(&mut l, &|_, _, dist, ang, yy| {
            let v = (yy - (c - body_r)) / (2.0 * body_r);
            let stripe = (ang / step).floor() % 2.0;
            let wa = (1.0 - v / 0.42).clamp(0.0, 1.0) * hi_a;
            let ba = ((v - 0.42) / 0.58).clamp(0.0, 1.0) * lo_a;
            let sk: [f32; 3] = std::array::from_fn(|i| {
                let base = dark[i] + (light[i] - dark[i]) * stripe;
                (base * (1.0 - wa) + 255.0 * wa) * (1.0 - ba)
            });
            Some((sk, disc(dist)))
        });
        let cap = |dist: f32| ((cap_r - dist) / ssf + 0.5).clamp(0.0, 1.0);
        if st == KnobStyle::Pedal {
            layer(&mut l, &|dx, dy, dist, _, _| {
                let rr = (dx + 0.16 * cap_r).hypot(dy + 0.36 * cap_r) / cap_r;
                let col: [f32; 3] = if rr < 0.55 {
                    let k = rr / 0.55;
                    [
                        74.0 + (38.0 - 74.0) * k,
                        77.0 + (40.0 - 77.0) * k,
                        84.0 + (44.0 - 84.0) * k,
                    ]
                } else {
                    let k = ((rr - 0.55) / 0.85).clamp(0.0, 1.0);
                    [
                        38.0 + (23.0 - 38.0) * k,
                        40.0 + (24.0 - 40.0) * k,
                        44.0 + (27.0 - 44.0) * k,
                    ]
                };
                Some((col, cap(dist)))
            });
            layer(&mut l, &|_, dy, dist, _, _| {
                let edge = (1.0 - (dist - (cap_r - ssf)).abs() / ssf).clamp(0.0, 1.0) * cap(dist);
                Some(([255.0; 3], edge * (-dy / cap_r).clamp(0.0, 1.0) * 0.18))
            });
            layer(&mut l, &|_, _, dist, _, _| {
                Some((
                    [0.0; 3],
                    (1.0 - (dist - cap_r).abs() / (0.8 * ssf)).clamp(0.0, 1.0) * 0.6,
                ))
            });
        } else {
            let amp = st == KnobStyle::Amp;
            let stops: &[(f32, [f32; 3])] = if amp { &GOLD } else { &SILVER };
            layer(&mut l, &|_, _, dist, ang, _| {
                let col = conic((ang - 10.0).rem_euclid(360.0) / 360.0, stops);
                let vig = ((dist / cap_r - 0.62) / 0.38).clamp(0.0, 1.0) * if amp { 0.22 } else { 0.18 };
                Some((std::array::from_fn(|i| col[i] * (1.0 - vig)), cap(dist)))
            });
            let edge =
                |dist: f32| (1.0 - (dist - (cap_r - 1.2 * ssf)).abs() / ssf).clamp(0.0, 1.0) * cap(dist);
            let hi = if amp { [255.0, 248.0, 220.0] } else { [255.0; 3] };
            layer(&mut l, &|_, dy, dist, _, _| {
                Some((hi, edge(dist) * (-dy / cap_r).clamp(0.0, 1.0) * 0.85))
            });
            let lo = if amp { [60.0, 35.0, 0.0] } else { [0.0; 3] };
            layer(&mut l, &|_, dy, dist, _, _| {
                Some((lo, edge(dist) * (dy / cap_r).clamp(0.0, 1.0) * 0.45))
            });
            let ring_a = if amp { 0.45 } else { 0.3 };
            layer(&mut l, &|_, _, dist, _, _| {
                Some((
                    [0.0; 3],
                    (1.0 - (dist - cap_r).abs() / (0.8 * ssf)).clamp(0.0, 1.0) * ring_a,
                ))
            });
        }
    }
    let out = size + 2 * m;
    Surf::from_image(&l.image(None)).smoothscale(out, out).image()
}

/// The cream chicken-head pointer (Vintage), pointing up, `size` pixels square: drawn at 4x with pygame and
/// smoothscaled.
fn make_pointer(size: i32) -> Surf {
    let ss = 4;
    let n = size.max(1) * ss;
    let nf = n as f64;
    let mut s = Surf::new(n, n);
    let p = |x: f64, y: f64| (x / 100.0 * nf, y / 100.0 * nf);
    let mut pts = Vec::new();
    for i in 0..25 {
        let tt = i as f64 / 24.0; // the beak, tip to hub
        pts.push(p(50.0 + 9.0 * tt.powf(1.4) + 9.0 * tt.powi(6), 1.0 + 45.0 * tt));
    }
    for i in 1..40 {
        let a = (-50.0 + 280.0 * i as f64 / 40.0).to_radians(); // round the hub
        pts.push(p(50.0 + 26.0 * a.cos(), 61.0 + 26.0 * a.sin() - 0.0));
    }
    for i in (0..=24).rev() {
        let tt = i as f64 / 24.0;
        pts.push(p(50.0 - 9.0 * tt.powf(1.4) - 9.0 * tt.powi(6), 1.0 + 45.0 * tt));
    }
    pg::polygon(&mut s, [231, 220, 194, 255], &pts, 0);
    pg::polygon(&mut s, [120, 105, 80, 255], &pts, ss.max(1));
    let (ex, ey) = p(32.0, 52.0);
    pg::ellipse(
        &mut s,
        [255, 255, 255, 90],
        (ex, ey, 22.0 / 100.0 * nf, 13.0 / 100.0 * nf),
    );
    pg::line_f(
        &mut s,
        [90, 80, 64, 255],
        p(50.0, 6.0),
        p(50.0, 40.0),
        ((2.2 * ss as f64) as i32).max(2),
    );
    s.smoothscale(size, size)
}

/// `_tick_ring`: the scale printed around a knob: n ticks, those in `lit` (a range of indices) in the accent with a
/// faint glow; `home` (a tick index) drawn longer. Drawn at 3x and smoothed.
fn tick_ring(
    boxw: i32,
    r0: f64,
    r1: f64,
    n: i32,
    lit: (i32, i32),
    width: f64,
    home: Option<i32>,
    s: f64,
) -> Image {
    let th = t();
    let ss = 3;
    let nn = boxw * ss;
    let mut surf = Surf::new(nn, nn);
    let mut lit_s = Surf::new(nn, nn);
    let c = nn as f64 / 2.0;
    let ssf = ss as f64;
    for i in 0..n {
        let a = (-135.0 + 270.0 * i as f64 / (n - 1).max(1) as f64).to_radians();
        let on = lit.0 <= i && i <= lit.1;
        let (mut ra, mut rb) = (r0, r1);
        if home == Some(i) {
            ra = r0 - (r1 - r0) * 0.35;
            rb = r1 + (r1 - r0) * 0.25;
        }
        let p0 = (c + ra * ssf * a.sin(), c - ra * ssf * a.cos());
        let p1 = (c + rb * ssf * a.sin(), c - rb * ssf * a.cos());
        let w = (((width + if on { 0.4 } else { 0.0 }) * ssf) as i32).max(2);
        let col = pg::rgb(if on {
            th.accent
        } else if home == Some(i) {
            th.text_2
        } else {
            th.knob_tick
        });
        for s_ in if on {
            vec![&mut surf, &mut lit_s]
        } else {
            vec![&mut surf]
        } {
            pg::line_f(s_, col, p0, p1, w);
            pg::circle(s_, col, p0, w as f64 / 2.0, 0);
            pg::circle(s_, col, p1, w as f64 / 2.0, 0);
        }
    }
    let out = surf.smoothscale(boxw, boxw);
    if lit.1 < lit.0 {
        return out.image();
    }
    let mut g = lit_s.smoothscale(boxw, boxw).gaussian_blur(pxr(2.0, s).max(1));
    g.fill_mult([255, 255, 255, (255.0 * th.glow_a as f64 * 1.6) as u8]);
    g.blit(&out, 0, 0);
    g.image()
}

/// What a knob shows round it.
#[derive(Clone, Copy, Debug)]
pub struct Scale {
    pub n: i32,
    /// For values either side of a centre (EQ gains): the lit ticks run from here, and its tick is drawn longer.
    pub zero: Option<f32>,
    /// A value whose tick is drawn longer (100% speed).
    pub home: Option<f32>,
    pub ticks: bool,
}

impl Default for Scale {
    fn default() -> Self {
        Scale {
            n: 21,
            zero: None,
            home: None,
            ticks: true,
        }
    }
}

/// A real knob `size` points across at `center` (`int(center)`), turned to `u` (0..1 over 270 degrees, 7 to 5
/// o'clock), with its scale printed round it. Returns the knob's rect.
pub fn knob(p: &Painter, center: Pos2, size: f32, u: f32, sc_: Scale, st: Option<KnobStyle>) -> Rect {
    let ppp = p.pixels_per_point();
    knob_px(p, (ipx(center.x, ppp), ipx(center.y, ppp)), size, u, sc_, st).to_rect(ppp)
}

/// `knob` centred on whole pixels.
pub(crate) fn knob_px(
    p: &Painter,
    (cx, cy): (i32, i32),
    size: f32,
    u: f32,
    sc_: Scale,
    st: Option<KnobStyle>,
) -> PxRect {
    let st = st.unwrap_or_else(style);
    let s = sc(p.pixels_per_point());
    let big_s = pxr(size, s).max(12);
    let u = u.clamp(0.0, 1.0) as f64;
    let deg = -135.0 + 270.0 * u;
    if sc_.ticks {
        let small = size < 50.0;
        let r0 = big_s as f64 / 2.0 + pxr(if small { 5.0 } else { 8.0 }, s) as f64;
        let r1 = big_s as f64 / 2.0 + pxr(if small { 9.0 } else { 14.0 }, s) as f64;
        let boxw = (2.0 * r1 + pxr(8.0, s) as f64) as i32;
        let steps = (sc_.n - 1) as f64;
        let idx = (u * steps).round_ties_even() as i32;
        let z = sc_.zero.map(|z| (z as f64 * steps).round_ties_even() as i32);
        let lit = match z {
            None => (0, idx),
            Some(z) => (idx.min(z), idx.max(z)),
        };
        let home = sc_
            .home
            .or(sc_.zero)
            .map(|h| (h as f64 * steps).round_ties_even() as i32);
        let width = if small { 1.6 } else { 2.0 };
        let key = key_of((
            "ticks",
            boxw,
            r0.to_bits(),
            r1.to_bits(),
            sc_.n,
            lit,
            small,
            home,
            theme::generation(),
            ck(t().accent),
        ));
        let (tex, sz) = texture(p.ctx(), Store::Small, key, || {
            tick_ring(boxw, r0, r1, sc_.n, lit, width, home, s)
        });
        blit(
            p,
            tex,
            sz,
            (cx - boxw / 2) as f32,
            (cy - boxw / 2) as f32,
            Color32::WHITE,
        );
    }
    let m = knob_margin(big_s, s);
    let at = ((cx - big_s / 2 - m) as f32, (cy - big_s / 2 - m) as f32);
    let ckey = key_of(("cast", big_s, st, theme::generation(), s.to_bits()));
    let (tex, sz) = texture(p.ctx(), Store::Small, ckey, || make_cast(big_s, st, s));
    blit(p, tex, sz, at.0, at.1, Color32::WHITE);
    let bkey = key_of(("knob", big_s, st, s.to_bits()));
    let (tex, sz) = texture(p.ctx(), Store::Body, bkey, || make_knob(big_s, st, s));
    blit(p, tex, sz, at.0, at.1, Color32::WHITE);
    let a = deg.to_radians();
    if st == KnobStyle::Vintage {
        // the pointer turned to the nearest degree (rotozoom), and a copy of it in black at 90/255 a little lower
        let d = deg.round_ties_even();
        for (shadow, dy) in [(true, pxr(3.0, s)), (false, 0)] {
            let pkey = key_of(("ptr", big_s, d as i32, shadow));
            let (tex, sz) = texture(p.ctx(), Store::Body, pkey, || {
                let mut ptr = make_pointer(big_s).rotozoom(-d as f32);
                if shadow {
                    ptr.fill_mult([0, 0, 0, 90]);
                }
                ptr.image()
            });
            let (w, h) = (sz[0] as i32, sz[1] as i32);
            blit(
                p,
                tex,
                sz,
                (cx - w / 2) as f32,
                (cy + dy - h / 2) as f32,
                Color32::WHITE,
            );
        }
    } else {
        let col = match st {
            KnobStyle::Pedal => Color32::from_rgb(242, 242, 239),
            KnobStyle::Amp => Color32::from_rgb(20, 20, 20),
            _ => Color32::from_rgb(26, 27, 29),
        };
        let w = pxr(
            if size >= 120.0 {
                5.0
            } else if size >= 40.0 {
                3.0
            } else {
                2.0
            },
            s,
        )
        .max(2);
        let bs = big_s as f64;
        let (r_in, r_out) = (bs * if size < 120.0 { 0.06 } else { 0.13 }, bs * 0.33);
        let (cxf, cyf) = (cx as f64, cy as f64);
        let p0 = ((cxf + r_in * a.sin()) as f32, (cyf - r_in * a.cos()) as f32);
        let p1 = ((cxf + r_out * a.sin()) as f32, (cyf - r_out * a.cos()) as f32);
        aaline(p, col, p0, p1, w);
    }
    PxRect {
        x: cx - big_s / 2,
        y: cy - big_s / 2,
        w: big_s,
        h: big_s,
    }
}

/// `pygame.draw.aaline(screen, col, p0, p1, w)` (points in the screen's pixels, as f32).
fn aaline(p: &Painter, col: Color32, p0: (f32, f32), p1: (f32, f32), w: i32) {
    let (x0, x1) = (p0.0.min(p1.0).floor() as i32, p0.0.max(p1.0).ceil() as i32);
    let (y0, y1) = (p0.1.min(p1.1).floor() as i32, p0.1.max(p1.1).ceil() as i32);
    let pad = w + 3;
    let bx = PxRect {
        x: x0 - pad,
        y: y0 - pad,
        w: x1 - x0 + 2 * pad + 1,
        h: y1 - y0 + 2 * pad + 1,
    };
    // the shape at a position whole pixels away is the same (to f32 rounding)
    let fr = |v: f32, o: i32| ((v - o as f32) * 64.0).round() as i32;
    let key = key_of((
        "aaline",
        fr(p0.0, bx.x),
        fr(p0.1, bx.y),
        fr(p1.0, bx.x),
        fr(p1.1, bx.y),
        w,
        ck(col),
    ));
    let c = pg::rgb(col);
    cover(p, key, bx, |cv| pg::aaline(cv, c, p0, p1, w));
}

/// The ring gauge's surface: radius `r`, width `w` (pixels), `q` of 400 lit.
fn make_ring_gauge(r: i32, w: i32, q: i32, color: Color32, s: f64) -> Image {
    let th = t();
    let ss = 3;
    let boxw = 2 * r + 2 * w + pxr(10.0, s);
    let n = boxw * ss;
    let mut track = Surf::new(n, n);
    let c = n as f64 / 2.0;
    let (rf, wf, ssf) = (r as f64, w as f64, ss as f64);
    pg::circle(
        &mut track,
        pg::rgb(th.unlit),
        (c, c),
        (rf + wf / 2.0) * ssf,
        w * ss,
    );
    let mut lit = Surf::new(n, n);
    let col = pg::rgb(color);
    if q > 0 {
        let steps = q.max(2);
        let mut pts_o = Vec::with_capacity(steps as usize + 1);
        let mut pts_i = Vec::with_capacity(steps as usize + 1);
        for i in 0..=steps {
            let a = (360.0 * (q as f64 / 400.0) * i as f64 / steps as f64).to_radians();
            pts_o.push((c + (rf + wf) * ssf * a.sin(), c - (rf + wf) * ssf * a.cos()));
            pts_i.push((c + rf * ssf * a.sin(), c - rf * ssf * a.cos()));
        }
        pts_o.extend(pts_i.into_iter().rev());
        pg::polygon(&mut lit, col, &pts_o, 0);
        for a_ in [0.0, (360.0 * q as f64 / 400.0).to_radians()] {
            let mid = (rf + wf / 2.0) * ssf;
            pg::circle(
                &mut lit,
                col,
                (c + mid * a_.sin(), c - mid * a_.cos()),
                wf * ssf / 2.0,
                0,
            );
        }
    }
    let mut out = track.smoothscale(boxw, boxw);
    let g = lit.smoothscale(boxw, boxw);
    let mut glow = g.gaussian_blur(pxr(3.0, s).max(1));
    glow.fill_mult([255, 255, 255, (255.0 * th.glow_a as f64 * 1.4) as u8]);
    out.blit(&glow, 0, 0);
    out.blit(&g, 0, 0);
    out.image()
}

/// A ring gauge (accuracy): a dark track all round and an arc of the accent from 12 o'clock, round ends.
pub fn ring_gauge(p: &Painter, center: Pos2, radius: f32, width: f32, u: f32, color: Option<Color32>) {
    let th = t();
    let color = color.unwrap_or(th.accent);
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let (r, w) = (pxr(radius, s), pxr(width, s).max(2));
    let q = (u.clamp(0.0, 1.0) as f64 * 400.0).round_ties_even() as i32;
    let key = key_of(("ringg", r, w, q, ck(color), ck(th.unlit), s.to_bits()));
    let (tex, sz) = texture(p.ctx(), Store::Small, key, || make_ring_gauge(r, w, q, color, s));
    let (cx, cy) = (ipx(center.x, ppp), ipx(center.y, ppp));
    let (bw, bh) = (sz[0] as i32, sz[1] as i32);
    blit(
        p,
        tex,
        sz,
        (cx - bw / 2) as f32,
        (cy - bh / 2) as f32,
        Color32::WHITE,
    );
}

/// `_arc_band`: the band between radii r0 and r1 from angle a0 to a1 (degrees, clockwise from 12 o'clock), with round
/// ends when `caps`.
fn arc_band(s: &mut Surf, c: (f64, f64), r0: f64, r1: f64, a0: f64, a1: f64, col: pg::Rgba, caps: bool) {
    let steps = ((((a1 - a0).abs() / 2.0) as i32) + 1).max(2);
    let mut outer = Vec::with_capacity(steps as usize + 1);
    let mut inner = Vec::with_capacity(steps as usize + 1);
    for i in 0..=steps {
        let a = (a0 + (a1 - a0) * i as f64 / steps as f64).to_radians();
        outer.push((c.0 + r1 * a.sin(), c.1 - r1 * a.cos()));
        inner.push((c.0 + r0 * a.sin(), c.1 - r0 * a.cos()));
    }
    outer.extend(inner.into_iter().rev());
    pg::polygon(s, col, &outer, 0);
    if caps {
        let (rm, w) = ((r0 + r1) / 2.0, (r1 - r0) / 2.0);
        for a in [a0.to_radians(), a1.to_radians()] {
            pg::circle(s, col, (c.0 + rm * a.sin(), c.1 - rm * a.cos()), w, 0);
        }
    }
}

/// The tuner dial's surface, `w` x `h` pixels, its centre `cy` down, the arc's radius `rr` (pixels).
fn make_arc_gauge(w: i32, h: i32, cy: i32, rr: f64) -> Image {
    let th = t();
    let k = rr / 74.0;
    let ss = 3;
    let ssf = ss as f64;
    let (bw, bh) = (w.max(1), h.max(1));
    let mut big = Surf::new(bw * ss, bh * ss);
    let c = (bw as f64 * ssf / 2.0, cy as f64 * ssf);
    let (r0, r1) = ((rr - 4.0 * k) * ssf, (rr + 4.0 * k) * ssf);
    arc_band(&mut big, c, r0, r1, -90.0, 90.0, pg::rgb(th.unlit), true);
    let mut zone = Surf::new(bw * ss, bh * ss);
    arc_band(&mut zone, c, r0, r1, -9.0, 9.0, pg::rgb(th.hit), false);
    zone.fill_mult([255, 255, 255, 150]);
    big.blit(&zone, 0, 0);
    for i in 0..11 {
        let a = (-90.0 + 18.0 * i as f64).to_radians();
        let r1 = if i == 5 { 92.0 } else { 88.0 } * k;
        let p0 = (c.0 + 82.0 * k * ssf * a.sin(), c.1 - 82.0 * k * ssf * a.cos());
        let p1 = (c.0 + r1 * ssf * a.sin(), c.1 - r1 * ssf * a.cos());
        let col = pg::rgb(if i == 5 { th.text_2 } else { th.bar_line });
        pg::line_f(&mut big, col, p0, p1, ((1.6 * k * ssf) as i32).max(2));
    }
    big.smoothscale(bw, bh).image()
}

/// The tuner's dial in rect (about 300 x 100): a dark arc from -50 to +50 cents with the in-tune zone and a tick
/// every 10 cents, and a needle at `cents` (none when nothing is heard) on a small hub.
pub fn arc_gauge(p: &Painter, rect: Rect, cents: Option<f32>, color: Color32) {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let pr = PxRect::of(rect, ppp);
    let th = t();
    let rr = (74.0 * s).min(pr.w as f64 * 0.42).min(pr.h as f64 * 0.74);
    let k = rr / 74.0;
    let (cx, cy) = (pr.x + pr.w / 2, pr.y + (pr.h as f64 * 0.94) as i32);
    let key = key_of((
        "arcg",
        pr.w,
        pr.h,
        (rr * 10.0).round_ties_even() as i64,
        theme::generation(),
    ));
    let (tex, sz) = texture(p.ctx(), Store::Small, key, || {
        make_arc_gauge(pr.w, pr.h, cy - pr.y, rr)
    });
    blit(p, tex, sz, pr.x as f32, pr.y as f32, Color32::WHITE);
    if let Some(cents) = cents {
        let a = ((cents as f64 / 50.0).clamp(-1.0, 1.0) * 90.0).to_radians();
        let (cxf, cyf) = (cx as f64, cy as f64);
        let p0 = (
            (cxf + 12.0 * k * a.sin()) as f32,
            (cyf - 12.0 * k * a.cos()) as f32,
        );
        let p1 = (
            (cxf + 70.0 * k * a.sin()) as f32,
            (cyf - 70.0 * k * a.cos()) as f32,
        );
        let halo = mix(color, th.screen, 0.72);
        aaline(p, halo, p0, p1, ((7.0 * k) as i32).max(3));
        aaline(p, color, p0, p1, ((3.0 * k) as i32).max(2));
    }
    let hub = ((18.0 * k) as i32).max(8);
    raised_px(
        p,
        PxRect {
            x: cx - hub / 2,
            y: cy - hub / 2,
            w: hub,
            h: hub,
        },
        None,
        3.0,
        Fill::Base,
        0,
    );
}

fn level(peak: f32) -> f64 {
    if peak <= 1e-5 {
        0.0
    } else {
        ((20.0 * (peak as f64).log10() + 60.0) / 60.0).clamp(0.0, 1.0)
    }
}

/// Input level, -60..0 dBFS, in a sunken groove: green, orange near the top, red at clipping.
pub fn meter(p: &Painter, peak: f32, rect: Rect) {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let r = PxRect::of(rect, ppp);
    inset_px(p, r, None, if r.h < pxr(10.0, s) { 2.0 } else { 3.0 }, Fill::Base);
    let lvl = level(peak);
    if lvl > 0.0 {
        let th = t();
        let col = if lvl < 0.85 {
            th.hit
        } else if lvl < 0.97 {
            th.late
        } else {
            th.miss
        };
        let w = ((r.w as f64 * lvl) as i32).max(r.h);
        pill_px(p, PxRect { w, ..r }, col);
    }
}

/// Input level as a row of LED segments (green, orange near the top, red at clipping): `n` segments of a whole
/// pixel width `max(2, (w - gap (n - 1)) // n)`, `max(1, px(2))` apart, corners `max(1, px(1))`.
pub fn led_meter(p: &Painter, peak: f32, rect: Rect, n: usize) {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let th = t();
    let r = PxRect::of(rect, ppp);
    let lvl = level(peak);
    let n = n.max(1) as i32;
    let gap = pxr(2.0, s).max(1);
    let w = ((r.w - gap * (n - 1)).div_euclid(n)).max(2);
    let on = (lvl * n as f64).round_ties_even() as i32;
    let rad = pxr(1.0, s).max(1);
    for i in 0..n {
        let seg = PxRect {
            x: r.x + i * (w + gap),
            y: r.y,
            w,
            h: r.h,
        };
        let col = if i < on {
            let frac = (i + 1) as f64 / n as f64;
            if frac < 0.8 {
                th.hit
            } else if frac < 0.95 {
                th.late
            } else {
                th.miss
            }
        } else {
            th.unlit
        };
        rrect_px(p, seg, rad, col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The knobs' parts against rsplay.neu's own surfaces (QRS_NEU_DUMPS, see keys.rs).
    #[test]
    fn neu_parity_knobs() {
        let Ok(dir) = std::env::var("QRS_NEU_DUMPS") else {
            return;
        };
        let _lock = crate::raster::THEME_LOCK.lock();
        let s = 1440.0f64 / 1400.0;
        let mut bad = Vec::new();
        let mut n = 0;
        for st in KnobStyle::ALL {
            for size in [37, 66, 123] {
                n += 1;
                let name = format!("knob_{}_{size}", st.key());
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &make_knob(size, st, s)) {
                    bad.push(e);
                }
            }
        }
        for size in [37, 66] {
            n += 1;
            let name = format!("ptr_{size}");
            if let Some(e) = crate::raster::tests_cmp(&dir, &name, &make_pointer(size).image()) {
                bad.push(e);
            }
            for deg in [-135, -60, -1, 0, 1, 45, 135] {
                n += 1;
                let name = format!("ptrrot_{size}_{deg}");
                let got = make_pointer(size).rotozoom(-deg as f32).image();
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &got) {
                    bad.push(e);
                }
            }
        }
        for th in theme::ThemeName::ALL {
            theme::set_theme(th);
            for st in KnobStyle::ALL {
                for size in [37, 66] {
                    n += 1;
                    let name = format!("{}_cast_{}_{size}", th.key(), st.key());
                    if let Some(e) = crate::raster::tests_cmp(&dir, &name, &make_cast(size, st, s)) {
                        bad.push(e);
                    }
                }
            }
            // a 64 knob (S = 66) and a 36 one (S = 37), at a few values
            for (size, sq, small) in [(64.0f64, 66, false), (36.0, 37, true)] {
                let r0 = sq as f64 / 2.0 + pxr(if small { 5.0 } else { 8.0 }, s) as f64;
                let r1 = sq as f64 / 2.0 + pxr(if small { 9.0 } else { 14.0 }, s) as f64;
                let boxw = (2.0 * r1 + pxr(8.0, s) as f64) as i32;
                for (lit, home) in [((0, 14), None), ((6, 10), Some(10)), ((0, 0), Some(10))] {
                    n += 1;
                    let name = format!("{}_ticks_{size}_{}_{}_{:?}", th.key(), lit.0, lit.1, home);
                    let got = tick_ring(boxw, r0, r1, 21, lit, if small { 1.6 } else { 2.0 }, home, s);
                    if let Some(e) = crate::raster::tests_cmp(&dir, &name, &got) {
                        bad.push(e);
                    }
                }
            }
            for (radius, width, u) in [
                (46.0f32, 9.0f32, 0.72f64),
                (38.0, 7.0, 0.15),
                (46.0, 9.0, 0.0),
                (46.0, 9.0, 1.0),
            ] {
                let (r, w) = (pxr(radius, s), pxr(width, s).max(2));
                let q = (u * 400.0).round_ties_even() as i32;
                n += 1;
                let name = format!("{}_ringg_{r}_{w}_{q}", th.key());
                if let Some(e) =
                    crate::raster::tests_cmp(&dir, &name, &make_ring_gauge(r, w, q, t().accent, s))
                {
                    bad.push(e);
                }
            }
            for (w, h) in [(267, 103), (300, 100), (226, 90)] {
                let rr = (74.0 * s).min(w as f64 * 0.42).min(h as f64 * 0.74);
                n += 1;
                let name = format!("{}_arcg_{w}_{h}", th.key());
                let got = make_arc_gauge(w, h, (h as f64 * 0.94) as i32, rr);
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &got) {
                    bad.push(e);
                }
            }
        }
        theme::set_theme(theme::ThemeName::Midnight);
        eprintln!("neu knobs parity: {n} cases");
        assert!(bad.is_empty(), "{} of {n} differ:\n{}", bad.len(), bad.join("\n"));
    }
}
