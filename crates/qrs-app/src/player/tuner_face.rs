//! The tuner's faces (P48.3, new in quickRS; the owner picked all four mockups of
//! research/tuner-mockups, "Realistic Tuner Mockups"): Digital (the flat arc it always had), a Needle meter (a backlit
//! cream dial behind glass in a chrome bezel, the amp room VU's materials), a Strobe drum, a Pedal (a 7-segment note
//! over a row of LEDs in a hammertone box) and Rack arc lamps (lamps behind smoked plexiglass, the default).
//!
//! Each face's body (plate, bezel, print, unlit lamps, screws) is drawn once with the pedal faces' kit into a cached
//! texture, its glass over the live parts likewise; only the needle, bands, segments and lamps are drawn each frame.
//! Plates are metal or plexiglass (the mockups' blue plate was the console's own colour; the owner asked for a
//! material). Screws are drawn, not painted (D183): a countersunk recess, machined rings on the head, a bevel, a slot
//! with depth, wear.

use std::hash::{Hash, Hasher};

use eframe::egui::{Align2, Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use qrs_ui::paint::{self, Store};
use qrs_ui::text::{self, Style};

use crate::pedalfaces::kit::{self, C, Cv, Fit};

/// A tuner face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Face {
    /// The flat arc gauge (as before P48.3).
    Digital,
    Needle,
    Strobe,
    Pedal,
    /// Rack arc lamps: the owner's favourite, the default.
    #[default]
    Arc,
}

impl Face {
    /// In the order the dots and Settings show them.
    pub const ALL: [Face; 5] = [Face::Arc, Face::Needle, Face::Strobe, Face::Pedal, Face::Digital];

    /// Its key in config.json (`tuner_face`).
    pub fn key(self) -> &'static str {
        match self {
            Face::Digital => "digital",
            Face::Needle => "needle",
            Face::Strobe => "strobe",
            Face::Pedal => "pedal",
            Face::Arc => "arc",
        }
    }

    /// From config.json (anything else, or nothing: the default).
    pub fn parse(s: &str) -> Face {
        Face::ALL.into_iter().find(|f| f.key() == s).unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Face::Digital => "Digital",
            Face::Needle => "Needle meter",
            Face::Strobe => "Strobe",
            Face::Pedal => "Pedal LEDs",
            Face::Arc => "Rack arc lamps",
        }
    }

    /// Short, for Settings' row of keys.
    pub fn short(self) -> &'static str {
        match self {
            Face::Digital => "Digital",
            Face::Needle => "Needle",
            Face::Strobe => "Strobe",
            Face::Pedal => "Pedal",
            Face::Arc => "Lamps",
        }
    }

    /// The next in `ALL` (a click on the tuner's caption).
    pub fn next(self) -> Face {
        let i = Face::ALL.iter().position(|&f| f == self).unwrap_or(0);
        Face::ALL[(i + 1) % Face::ALL.len()]
    }

    /// The face's units (the mockups'): width x height.
    fn size(self) -> (f32, f32) {
        match self {
            Face::Digital => (348.0, 100.0),
            Face::Needle => (348.0, 178.0),
            Face::Strobe => (348.0, 136.0),
            Face::Pedal => (348.0, 204.0),
            Face::Arc => (348.0, 180.0),
        }
    }

    /// Its height for a width `w` (points).
    pub fn height(self, w: f32) -> f32 {
        let (fw, fh) = self.size();
        (w * fh / fw).round()
    }

    /// It shows the note heard itself (the tile leaves out its big note label).
    pub fn shows_note(self) -> bool {
        matches!(self, Face::Pedal | Face::Arc)
    }
}

/// What the face shows this frame.
pub struct Reading<'a> {
    /// The needle's cents (−50..50; pinned at ±50 past half a semitone), None when nothing is heard.
    pub cents: Option<f32>,
    /// On a string and within 5 cents.
    pub in_tune: bool,
    /// The note heard ("Eb3"), for the faces that show it.
    pub heard: Option<&'a str>,
    /// More than half a semitone off ("−2 semitones" instead of cents), and which way.
    pub semis: i32,
}

/// The moving parts' state between frames (the needle's swing, the lamps' afterglow, the strobe's phase).
#[derive(Clone, Debug)]
pub struct Anim {
    needle: f32,
    vel: f32,
    lamp: f32,
    phase: f32,
    lock: f32,
    led: [f32; 11],
    glow: [f32; 21],
    last: Option<f64>,
}

impl Default for Anim {
    fn default() -> Anim {
        Anim {
            needle: -57.0,
            vel: 0.0,
            lamp: 0.0,
            phase: 7.0,
            lock: 0.0,
            led: [0.0; 11],
            glow: [0.0; 21],
            last: None,
        }
    }
}

impl Anim {
    /// Step to `now` (seconds) toward the reading. Returns true while something still moves.
    fn step(&mut self, now: f64, r: &Reading) -> bool {
        // (a long gap, the first frame or a screenshot's held clock: settle at once)
        let dt = self.last.map_or(1.0, |l| (now - l) as f32);
        self.last = Some(now);
        let snap = !(0.0..=0.2).contains(&dt);
        let dt = dt.clamp(0.0, 0.05);
        let c = r.cents;
        // the needle: a damped spring onto the cents (rests pinned left with nothing heard), bouncing off the stops
        let tgt = c.map_or(-57.0, |c| c.clamp(-57.0, 57.0));
        if snap {
            self.needle = tgt;
            self.vel = 0.0;
        } else {
            for _ in 0..4 {
                let h = dt / 4.0;
                self.vel += (170.0 * (tgt - self.needle) - 17.0 * self.vel) * h;
                self.needle += self.vel * h;
                if self.needle < -58.0 {
                    self.needle = -58.0;
                    self.vel *= -0.3;
                }
                if self.needle > 58.0 {
                    self.needle = 58.0;
                    self.vel *= -0.3;
                }
            }
        }
        let ease = |v: &mut f32, t: f32, up: f32, down: f32| {
            let k = if snap {
                1.0
            } else {
                (dt / if t > *v { up } else { down }).clamp(0.0, 1.0)
            };
            *v += (t - *v) * k;
        };
        ease(&mut self.lamp, if r.in_tune { 1.0 } else { 0.0 }, 0.12, 0.12);
        // the strobe: the bands drift with the cents (far off: fast), stand still in tune
        let sp = c.map_or(0.0, |c| {
            if r.semis != 0 {
                120.0 * r.semis.signum() as f32
            } else {
                c
            }
        });
        self.phase += sp.clamp(-120.0, 120.0) * dt * 1.1;
        ease(
            &mut self.lock,
            if c.is_some_and(|c| c.abs() <= 3.0) && r.semis == 0 {
                1.0
            } else {
                0.0
            },
            0.35,
            0.35,
        );
        // the pedal's LEDs (10 cents each, the middle one ±5) and the arc's lamps (5 cents each), with afterglow
        let led_on = c.map(|c| {
            if c.abs() <= 5.0 {
                5
            } else {
                let i = ((c / 10.0).round() as i32 + 5).clamp(0, 10);
                if i == 5 {
                    if c < 0.0 { 4 } else { 6 }
                } else {
                    i as usize
                }
            }
        });
        for (i, v) in self.led.iter_mut().enumerate() {
            ease(v, if led_on == Some(i) { 1.0 } else { 0.0 }, 0.03, 0.09);
        }
        let lamp_on = c.map(|c| ((c.clamp(-52.5, 52.5) / 5.0).round() as i32 + 10).clamp(0, 20) as usize);
        for (i, v) in self.glow.iter_mut().enumerate() {
            ease(v, if lamp_on == Some(i) { 1.0 } else { 0.0 }, 0.04, 0.22);
        }
        let settled_led = self
            .led
            .iter()
            .chain(self.glow.iter())
            .all(|v| *v < 0.01 || *v > 0.99);
        self.vel.abs() > 0.5
            || (tgt - self.needle).abs() > 0.2
            || sp.abs() > 0.01
            || !settled_led
            || (self.lamp > 0.01 && self.lamp < 0.99)
    }
}

/// Draw `face` (not Digital: the tile draws that one itself) in `r` (its height from `Face::height`).
pub fn draw(p: &Painter, r: Rect, face: Face, a: &mut Anim, rd: &Reading) {
    let now = p.ctx().input(|i| i.time);
    if a.step(now, rd) {
        p.ctx().request_repaint();
    }
    let (w, h) = face.size();
    let fit = Fit::new(r, w, h, [0.0; 4]);
    let ppp = p.ctx().pixels_per_point();
    let tex = |layer: u8| {
        let mut hs = std::collections::hash_map::DefaultHasher::new();
        (
            "tuner-face",
            face,
            layer,
            (r.width() * ppp) as i32,
            (r.height() * ppp) as i32,
        )
            .hash(&mut hs);
        let (t, _) = paint::texture(p.ctx(), Store::Face, hs.finish(), || {
            let fit = Fit::new(r, w, h, [0.0; 4]);
            let mut cv = Cv::new(r, &fit, ppp).expect("tuner face canvas");
            match (face, layer) {
                (Face::Needle, 0) => needle_body(&mut cv),
                (Face::Needle, _) => needle_glass(&mut cv),
                (Face::Strobe, 0) => strobe_body(&mut cv),
                (Face::Strobe, _) => strobe_glass(&mut cv),
                (Face::Pedal, 0) => pedal_body(&mut cv),
                (Face::Pedal, _) => pedal_glass(&mut cv),
                (Face::Arc, 0) => arc_body(&mut cv),
                (Face::Arc, _) => arc_glass(&mut cv),
                (Face::Digital, _) => {}
            }
            cv.image()
        });
        t
    };
    let full = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    p.image(tex(0), r, full, Color32::WHITE);
    match face {
        Face::Needle => needle_live(p, &fit, a),
        Face::Strobe => strobe_live(p, &fit, a, rd),
        Face::Pedal => pedal_live(p, &fit, a, rd),
        Face::Arc => arc_live(p, &fit, a, rd),
        Face::Digital => {}
    }
    p.image(tex(1), r, full, Color32::WHITE);
    if face == Face::Needle {
        needle_lamp(p, &fit, a);
    }
}

// -- shared parts ------------------------------------------------------------------------------------------------------

/// A point at `deg` from straight up, `r` from (cx, cy).
fn at(cx: f32, cy: f32, r: f32, deg: f32) -> (f32, f32) {
    let t = deg.to_radians();
    (cx + r * t.sin(), cy - r * t.cos())
}

/// An arc from `d0` to `d1` degrees (from straight up), as a polyline.
fn arc(cx: f32, cy: f32, r: f32, d0: f32, d1: f32) -> Vec<(f32, f32)> {
    let n = (((d1 - d0).abs() / 2.0).ceil() as usize).max(2);
    (0..=n)
        .map(|i| at(cx, cy, r, d0 + (d1 - d0) * i as f32 / n as f32))
        .collect()
}

fn stroke_pts(cv: &mut Cv, pts: &[(f32, f32)], c: C, a: f32, w: f32) {
    if let Some(path) = qrs_ui::raster::polyline(pts) {
        let st = tiny_skia::Stroke {
            width: w,
            line_cap: tiny_skia::LineCap::Butt,
            ..Default::default()
        };
        cv.pm.stroke_path(&path, &kit::col(c, a), &st, cv.tr, None);
    }
}

/// The plates the faces sit on.
#[derive(Clone, Copy)]
enum Plate {
    /// Satin brushed aluminium.
    Aluminium,
    /// Black anodised aluminium, finely brushed.
    Anodised,
    /// Graphite hammertone (a stomp box).
    Hammer,
    /// Smoked plexiglass over a black metal back.
    Smoke,
}

fn plate(cv: &mut Cv, w: f32, h: f32, r: f32, kind: Plate) {
    let Some(path) = kit::rrect(0.5, 0.5, w - 1.0, h - 1.0, r) else {
        return;
    };
    let bx = (0.0, 0.0, w, h);
    match kind {
        Plate::Aluminium => {
            cv.fill(
                &path,
                &kit::lin(
                    0.0,
                    0.0,
                    0.0,
                    h,
                    &[(0.0, 0xd4d7db, 1.0), (0.45, 0xb3b8be, 1.0), (1.0, 0x8e949b, 1.0)],
                ),
            );
            let m = cv.mask(&path);
            kit::brushed(cv, &m, bx, 11, 1.7);
            // a soft sheen across, as satin metal catches the room's light
            cv.fill_in(
                &path,
                &kit::lin(
                    0.0,
                    0.0,
                    w,
                    0.0,
                    &[
                        (0.0, 0xffffff, 0.0),
                        (0.35, 0xffffff, 0.16),
                        (0.55, 0xffffff, 0.0),
                    ],
                ),
                &m,
            );
        }
        Plate::Anodised => {
            cv.fill(
                &path,
                &kit::lin(0.0, 0.0, 0.0, h, &[(0.0, 0x34383d, 1.0), (1.0, 0x16181b, 1.0)]),
            );
            let m = cv.mask(&path);
            kit::brushed(cv, &m, bx, 23, 1.1);
            cv.fill_in(
                &path,
                &kit::lin(
                    0.0,
                    0.0,
                    w,
                    0.0,
                    &[(0.0, 0xffffff, 0.0), (0.3, 0xffffff, 0.07), (0.5, 0xffffff, 0.0)],
                ),
                &m,
            );
        }
        Plate::Hammer => {
            cv.fill(
                &path,
                &kit::lin(0.0, 0.0, 0.0, h, &[(0.0, 0x555b63, 1.0), (1.0, 0x2c3036, 1.0)]),
            );
            let m = cv.mask(&path);
            kit::hammer(cv, &m, bx, 5);
        }
        Plate::Smoke => {
            // the black metal back, then a slab of smoked acrylic on it: lighter at its edges where the light runs
            // through, a bright polished top edge, a gloss band
            cv.fill(&path, &kit::col(0x07090b, 1.0));
            if let Some(slab) = kit::rrect(3.0, 3.0, w - 6.0, h - 6.0, (r - 2.0).max(1.0)) {
                cv.fill(
                    &slab,
                    &kit::lin(
                        0.0,
                        0.0,
                        0.0,
                        h,
                        &[(0.0, 0x39434c, 0.85), (0.5, 0x161b20, 0.8), (1.0, 0x0e1216, 0.9)],
                    ),
                );
                let m = cv.mask(&slab);
                cv.fill_in(
                    &slab,
                    &kit::lin(
                        0.0,
                        0.0,
                        w,
                        h,
                        &[
                            (0.0, 0xffffff, 0.0),
                            (0.28, 0xffffff, 0.07),
                            (0.36, 0xffffff, 0.0),
                        ],
                    ),
                    &m,
                );
                cv.stroke(
                    &slab,
                    &kit::lin(
                        0.0,
                        0.0,
                        0.0,
                        h,
                        &[
                            (0.0, 0xffffff, 0.45),
                            (0.25, 0xffffff, 0.08),
                            (1.0, 0x000000, 0.5),
                        ],
                    ),
                    1.2,
                );
            }
        }
    }
    // the plate's edge: a light lip above, dark below
    cv.stroke(
        &path,
        &kit::lin(
            0.0,
            0.0,
            0.0,
            h,
            &[
                (0.0, 0xffffff, 0.35),
                (0.3, 0xffffff, 0.06),
                (1.0, 0x000000, 0.55),
            ],
        ),
        1.0,
    );
}

/// A machined screw (D183): its countersunk recess, a domed head with lathe rings, a bevelled rim, a slot with depth
/// (shadowed wall above, lit wall below), a few scratches; `seed` turns it and wears it differently.
fn screw(cv: &mut Cv, cx: f32, cy: f32, r: f32, seed: u32, dark: bool) {
    // the recess: a soft shadow ring and the lit far edge of the countersink
    cv.circle(
        cx,
        cy,
        r * 1.32,
        &kit::rad(cx, cy, r * 1.32, r * 1.32, &[(0.7, 0, 0.55), (1.0, 0, 0.0)]),
    );
    cv.ring(
        cx,
        cy,
        r * 1.12,
        &kit::lin(
            cx,
            cy - r,
            cx,
            cy + r,
            &[(0.0, 0x000000, 0.55), (1.0, 0xffffff, 0.35)],
        ),
        r * 0.14,
    );
    // the head: steel (or blackened steel), lit from the upper left
    let (hi, mid, lo) = if dark {
        (0x8c9198, 0x3e4247, 0x1c1e21)
    } else {
        (0xffffff, 0xb8bdc3, 0x5d6268)
    };
    cv.circle(
        cx,
        cy,
        r,
        &kit::rad(
            cx - r * 0.35,
            cy - r * 0.4,
            r * 1.5,
            r * 1.5,
            &[(0.0, hi, 1.0), (0.45, mid, 1.0), (1.0, lo, 1.0)],
        ),
    );
    let Some(head) = kit::circle(cx, cy, r) else {
        return;
    };
    let m = cv.mask(&head);
    // lathe rings: fine concentric light and dark lines
    let n = (r * 3.2) as u32;
    for k in 1..n {
        let rr = r * k as f32 / n as f32;
        let h = kit::hash01(seed.wrapping_mul(97).wrapping_add(k));
        let (c, a) = if h < 0.5 {
            (0xffffff, 0.08 + 0.16 * h)
        } else {
            (0x000000, 0.06 + 0.16 * (h - 0.5))
        };
        if let Some(ring) = kit::circle(cx, cy, rr) {
            cv.stroke_in(&ring, &kit::col(c, a), r * 0.07, &m);
        }
    }
    // the anisotropic streak a turned surface throws across its centre
    let t = (kit::hash01(seed) * 180.0).to_radians();
    let (s, c) = t.sin_cos();
    if let Some(band) = kit::poly(&[
        (cx - r * c - r * 0.18 * s, cy - r * s + r * 0.18 * c),
        (cx + r * c - r * 0.18 * s, cy + r * s + r * 0.18 * c),
        (cx + r * c + r * 0.18 * s, cy + r * s - r * 0.18 * c),
        (cx - r * c + r * 0.18 * s, cy - r * s - r * 0.18 * c),
    ]) {
        cv.fill_in(&band, &kit::col(0xffffff, if dark { 0.12 } else { 0.22 }), &m);
    }
    // the bevel: light on the upper left of the rim, dark on the lower right
    cv.ring(
        cx,
        cy,
        r * 0.9,
        &kit::lin(
            cx - r,
            cy - r,
            cx + r,
            cy + r,
            &[(0.0, 0xffffff, 0.55), (0.5, 0xffffff, 0.0), (1.0, 0x000000, 0.45)],
        ),
        r * 0.16,
    );
    // the slot, at its own angle: a dark cut, its upper wall in shadow and its lower wall catching the light
    let t = ((kit::hash01(seed + 7) * 180.0) - 90.0).to_radians();
    let (s, c) = t.sin_cos();
    let (hl, hw) = (r * 0.78, r * 0.17);
    let q = |u: f32, v: f32| (cx + u * c - v * s, cy + u * s + v * c);
    if let Some(slot) = kit::poly(&[q(-hl, -hw), q(hl, -hw), q(hl, hw), q(-hl, hw)]) {
        let (x0, y0) = q(0.0, -hw);
        let (x1, y1) = q(0.0, hw);
        cv.fill(
            &slot,
            &kit::lin(
                x0,
                y0,
                x1,
                y1,
                &[(0.0, 0x050506, 1.0), (0.6, 0x1b1d20, 1.0), (1.0, 0x55595e, 1.0)],
            ),
        );
    }
    let (a0, b0) = (q(-hl, hw), q(hl, hw));
    stroke_pts(cv, &[a0, b0], 0xffffff, 0.45, r * 0.06);
    // wear: a few short scratches across the head
    for k in 0..3u32 {
        let h = |j: u32| kit::hash01(seed.wrapping_mul(31).wrapping_add(k * 11 + j));
        let (px, py) = (cx + (h(1) - 0.5) * r * 1.2, cy + (h(2) - 0.5) * r * 1.2);
        let t = (h(3) * 180.0).to_radians();
        let l = r * (0.25 + 0.4 * h(4));
        if let Some(sc) = qrs_ui::raster::polyline(&[(px, py), (px + l * t.cos(), py + l * t.sin())]) {
            cv.stroke_in(&sc, &kit::col(0xffffff, 0.18), r * 0.04, &m);
        }
    }
    cv.ring(cx, cy, r, &kit::col(0x1a1c1f, 0.9), r * 0.08);
}

fn screws(cv: &mut Cv, pts: &[(f32, f32)], r: f32, seed: u32, dark: bool) {
    for (i, &(x, y)) in pts.iter().enumerate() {
        screw(cv, x, y, r * 1.15, seed + i as u32 * 13, dark);
    }
}

/// Text in the face's units, centred.
#[allow(clippy::too_many_arguments)]
fn label(
    p: &Painter,
    fit: &Fit,
    s: &str,
    x: f32,
    y: f32,
    size: f32,
    weight: u16,
    col: Color32,
    align: Align2,
) {
    let px = fit.s(size);
    if px < 4.0 {
        return;
    }
    text::draw(p, s, fit.p(x, y), align, Style::new(px, weight, col), None);
}

/// An additive round glow: `col` (premultiplied, alpha 0 adds light) at the centre falling to nothing at `r` px.
fn glow(p: &Painter, c: Pos2, r: f32, col: Color32) {
    if r < 0.5 {
        return;
    }
    let mut m = Mesh::default();
    m.colored_vertex(c, col);
    let n = 28;
    for k in 0..=n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        m.colored_vertex(c + vec2(a.cos(), a.sin()) * r, Color32::TRANSPARENT);
        if k > 0 {
            m.add_triangle(0, k, k + 1);
        }
    }
    p.add(Shape::mesh(m));
}

fn light(c: [u8; 3], k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (c[0] as f32 * k) as u8,
        (c[1] as f32 * k) as u8,
        (c[2] as f32 * k) as u8,
        0,
    )
}

// -- 1: the needle meter -----------------------------------------------------------------------------------------------

const N_PV: (f32, f32) = (174.0, 192.0);
const N_R: f32 = 124.0;
const N_DIAL: (f32, f32, f32, f32) = (38.0, 20.0, 272.0, 138.0);

fn needle_body(cv: &mut Cv) {
    plate(cv, 348.0, 178.0, 12.0, Plate::Aluminium);
    screws(
        cv,
        &[(14.0, 16.0), (334.0, 16.0), (14.0, 162.0), (334.0, 162.0)],
        5.5,
        3,
        false,
    );
    // the bezel: its shadow, chrome, a lit rim, the inner bevel, a black lip, then the dial: warm paper, lit from below
    cv.rect(26.0, 10.5, 296.0, 162.0, 14.0, &kit::col(0, 0.45));
    cv.rect(
        26.0,
        8.0,
        296.0,
        162.0,
        14.0,
        &kit::chrome(26.0, 8.0, 296.0, 162.0),
    );
    cv.rect(
        32.0,
        14.0,
        284.0,
        150.0,
        10.0,
        &kit::lin(
            0.0,
            14.0,
            0.0,
            164.0,
            &[(0.0, 0x4a4a48, 1.0), (0.5, 0x8e8e8b, 1.0), (1.0, 0xededea, 1.0)],
        ),
    );
    cv.rect(35.5, 17.5, 277.0, 143.0, 8.0, &kit::col(0x0a0a0a, 1.0));
    let (x, y, w, h) = N_DIAL;
    cv.rect(
        x,
        y,
        w,
        h,
        6.0,
        &kit::rad(
            174.0,
            176.0,
            240.0,
            240.0,
            &[
                (0.0, 0xfff6dc, 1.0),
                (0.45, 0xf8f0dc, 1.0),
                (0.8, 0xebdfc0, 1.0),
                (1.0, 0xcfbf98, 1.0),
            ],
        ),
    );
    let (cx, cy) = N_PV;
    // zones: red past ±20, green ±5; the scale's line and ticks every 5 cents
    stroke_pts(cv, &arc(cx, cy, N_R - 3.0, -48.0, -19.2), 0xb0180e, 1.0, 3.0);
    stroke_pts(cv, &arc(cx, cy, N_R - 3.0, 19.2, 48.0), 0xb0180e, 1.0, 3.0);
    stroke_pts(cv, &arc(cx, cy, N_R - 4.0, -4.8, 4.8), 0x26803e, 1.0, 6.5);
    stroke_pts(cv, &arc(cx, cy, N_R, -48.0, 48.0), 0x1a1a1a, 1.0, 0.9);
    for c in (-50i32..=50).step_by(5) {
        let a = c as f32 / 50.0 * 48.0;
        let major = c % 10 == 0;
        let col = if c.abs() >= 20 { 0xb0180e } else { 0x1a1a1a };
        stroke_pts(
            cv,
            &[
                at(cx, cy, N_R, a),
                at(cx, cy, N_R + if major { 9.0 } else { 5.0 }, a),
            ],
            col,
            1.0,
            if major { 1.3 } else { 0.8 },
        );
    }
    // the lamp's housing
    cv.circle(
        66.0,
        136.0,
        6.6,
        &kit::rad(
            66.0,
            136.0,
            6.6,
            6.6,
            &[(0.0, 0x333333, 1.0), (1.0, 0x111111, 1.0)],
        ),
    );
}

fn needle_glass(cv: &mut Cv) {
    let (x, y, w, h) = N_DIAL;
    let Some(dial) = kit::rrect(x, y, w, h, 6.0) else {
        return;
    };
    let m = cv.mask(&dial);
    let (cx, cy) = N_PV;
    // the pivot's black cover, the lip's shadow, the sides, the glass's glare
    if let Some(cover) = kit::poly(&arc(cx, cy, 46.0, -90.0, 90.0)) {
        cv.fill_in(
            &cover,
            &kit::rad(
                cx,
                cy - 20.0,
                50.0,
                50.0,
                &[(0.0, 0x2a2a2a, 1.0), (1.0, 0x0c0c0c, 1.0)],
            ),
            &m,
        );
    }
    if let Some(b) = kit::rrect(x, y, w, 14.0, 0.0) {
        cv.fill_in(
            &b,
            &kit::lin(0.0, y, 0.0, y + 14.0, &[(0.0, 0, 0.42), (1.0, 0, 0.0)]),
            &m,
        );
    }
    if let Some(b) = kit::rrect(x, y, 14.0, h, 0.0) {
        cv.fill_in(
            &b,
            &kit::lin(x, 0.0, x + 14.0, 0.0, &[(0.0, 0, 0.25), (1.0, 0, 0.0)]),
            &m,
        );
    }
    if let Some(b) = kit::rrect(x + w - 14.0, y, 14.0, h, 0.0) {
        cv.fill_in(
            &b,
            &kit::lin(x + w, 0.0, x + w - 14.0, 0.0, &[(0.0, 0, 0.2), (1.0, 0, 0.0)]),
            &m,
        );
    }
    if let Some(g) = kit::poly(&[(x, y), (196.0, y), (120.0, y + h), (x, y + h)]) {
        cv.fill_in(
            &g,
            &kit::lin(
                x,
                y,
                150.0,
                120.0,
                &[(0.0, 0xffffff, 0.16), (0.6, 0xffffff, 0.04), (1.0, 0xffffff, 0.0)],
            ),
            &m,
        );
    }
    stroke_pts(cv, &[(44.0, 158.4), (304.0, 158.4)], 0xffffff, 0.28, 0.7);
}

fn needle_live(p: &Painter, fit: &Fit, a: &Anim) {
    let (cx, cy) = N_PV;
    let ink = Color32::from_rgb(26, 26, 26);
    for c in (-50i32..=50).step_by(10) {
        let (tx, ty) = at(cx, cy, N_R + 19.0, c as f32 / 50.0 * 48.0);
        let col = if c == 0 {
            Color32::from_rgb(30, 110, 52)
        } else if c.abs() >= 20 {
            Color32::from_rgb(160, 22, 12)
        } else {
            ink
        };
        label(
            p,
            fit,
            &c.abs().to_string(),
            tx,
            ty,
            11.0,
            700,
            col,
            Align2::CENTER_CENTER,
        );
    }
    let (fx, fy) = at(cx, cy, N_R + 12.0, -58.0);
    let (sx, sy) = at(cx, cy, N_R + 12.0, 58.0);
    label(p, fit, "b", fx, fy, 15.0, 700, ink, Align2::CENTER_CENTER);
    label(p, fit, "#", sx, sy, 15.0, 700, ink, Align2::CENTER_CENTER);
    label(p, fit, "CENTS", cx, 122.0, 11.0, 700, ink, Align2::CENTER_CENTER);
    label(
        p,
        fit,
        "A4 = 440 Hz",
        cx,
        135.0,
        8.5,
        600,
        ink.gamma_multiply(0.6),
        Align2::CENTER_CENTER,
    );
    label(
        p,
        fit,
        "IN TUNE",
        66.0,
        149.0,
        7.5,
        700,
        ink.gamma_multiply(0.75),
        Align2::CENTER_CENTER,
    );
    // the needle, clipped to the dial, its shadow on the paper first
    let (x, y, w, h) = N_DIAL;
    let clip = p.with_clip_rect(fit.rect(x, y, w, h));
    let ang = a.needle.clamp(-57.0, 57.0) / 50.0 * 48.0;
    let (tx, ty) = at(cx, cy, 154.0, ang);
    let sh = vec2(fit.s(2.2), fit.s(2.6));
    clip.line_segment(
        [fit.p(cx, cy) + sh, fit.p(tx, ty) + sh],
        Stroke::new(fit.s(2.4), Color32::from_rgba_unmultiplied(40, 24, 0, 50)),
    );
    clip.line_segment(
        [fit.p(cx, cy), fit.p(tx, ty)],
        Stroke::new(fit.s(1.6).max(1.0), Color32::from_rgb(17, 17, 17)),
    );
}

fn needle_lamp(p: &Painter, fit: &Fit, a: &Anim) {
    let c = fit.p(66.0, 136.0);
    if a.lamp > 0.05 {
        glow(p, c, fit.s(22.0), light([90, 230, 120], 0.55 * a.lamp));
    }
    let lens = |k: f32| {
        let off = [60u8, 74, 60];
        let on = [150u8, 255, 170];
        Color32::from_rgb(
            (off[0] as f32 + (on[0] as f32 - off[0] as f32) * k) as u8,
            (off[1] as f32 + (on[1] as f32 - off[1] as f32) * k) as u8,
            (off[2] as f32 + (on[2] as f32 - off[2] as f32) * k) as u8,
        )
    };
    p.circle_filled(c, fit.s(4.6), lens(a.lamp));
    p.circle_filled(
        c + vec2(-fit.s(1.4), -fit.s(1.6)),
        fit.s(1.3),
        Color32::from_white_alpha(120),
    );
}

// -- 2: the strobe -------------------------------------------------------------------------------------------------------

const S_WIN: (f32, f32, f32, f32) = (32.0, 16.0, 284.0, 86.0);

fn strobe_body(cv: &mut Cv) {
    plate(cv, 348.0, 136.0, 12.0, Plate::Anodised);
    screws(
        cv,
        &[(14.0, 15.0), (334.0, 15.0), (14.0, 121.0), (334.0, 121.0)],
        5.2,
        41,
        true,
    );
    cv.rect(26.0, 10.0, 296.0, 98.0, 8.0, &kit::col(0x04080c, 1.0));
    if let Some(edge) = kit::rrect(26.5, 10.5, 295.0, 97.0, 7.5) {
        cv.stroke(
            &edge,
            &kit::lin(0.0, 10.0, 0.0, 108.0, &[(0.0, 0, 0.6), (1.0, 0xffffff, 0.18)]),
            1.0,
        );
    }
}

fn strobe_glass(cv: &mut Cv) {
    let (x, y, w, h) = S_WIN;
    let Some(win) = kit::rrect(x, y, w, h, 1.5) else {
        return;
    };
    let m = cv.mask(&win);
    // the drum's curve: dark at both ends; the lip's shadow; a glare; the gold index marks
    cv.fill_in(
        &win,
        &kit::lin(
            x,
            0.0,
            x + w,
            0.0,
            &[
                (0.0, 0, 0.75),
                (0.2, 0, 0.12),
                (0.5, 0, 0.0),
                (0.8, 0, 0.12),
                (1.0, 0, 0.75),
            ],
        ),
        &m,
    );
    if let Some(b) = kit::rrect(x, y, w, 12.0, 0.0) {
        cv.fill_in(
            &b,
            &kit::lin(0.0, y, 0.0, y + 12.0, &[(0.0, 0, 0.5), (1.0, 0, 0.0)]),
            &m,
        );
    }
    if let Some(g) = kit::poly(&[(x, y), (x + 130.0, y), (x + 70.0, y + h), (x, y + h)]) {
        cv.fill_in(&g, &kit::col(0xffffff, 0.06), &m);
    }
    for (tip, base) in [(y + 7.0, y), (y + h - 7.0, y + h)] {
        if let Some(t) = kit::poly(&[(168.0, base), (180.0, base), (174.0, tip)]) {
            cv.fill(&t, &kit::col(0xf2b53c, 1.0));
        }
    }
}

fn strobe_live(p: &Painter, fit: &Fit, a: &Anim, rd: &Reading) {
    let (x, y, w, h) = S_WIN;
    let win = fit.rect(x, y, w, h);
    let clip = p.with_clip_rect(win);
    // the backlight: amber, green as the bands lock; dim with nothing heard
    let lit = if rd.cents.is_some() { 1.0 } else { 0.45 };
    let amber = [255.0f32, 196.0, 104.0];
    let green = [150.0f32, 236.0, 160.0];
    let mixc = |t: f32, k: f32| {
        Color32::from_rgb(
            ((amber[0] + (green[0] - amber[0]) * t) * k) as u8,
            ((amber[1] + (green[1] - amber[1]) * t) * k) as u8,
            ((amber[2] + (green[2] - amber[2]) * t) * k) as u8,
        )
    };
    let mid = mixc(a.lock, lit);
    let edge = mixc(a.lock, lit * 0.55);
    let mut m = Mesh::default();
    for (yy, c) in [(win.min.y, edge), (win.center().y, mid), (win.max.y, edge)] {
        m.colored_vertex(pos2(win.min.x, yy), c);
        m.colored_vertex(pos2(win.max.x, yy), c);
    }
    m.add_triangle(0, 1, 3);
    m.add_triangle(0, 3, 2);
    m.add_triangle(2, 3, 5);
    m.add_triangle(2, 5, 4);
    clip.add(Shape::mesh(m));
    // three bands of dark bars at 1x, 2x, 4x the spacing, wrapped round the drum
    let (cx, rc) = (174.0, 152.0);
    let proj = |u: f32| cx + rc * (u / rc).clamp(-1.45, 1.45).sin();
    let sp = a.phase;
    let speed = rd
        .cents
        .map_or(0.0, |c| if rd.semis != 0 { 120.0 } else { c.abs() });
    for (row_y, row_h, per) in [
        (24.0f32, 18.0f32, 12.0f32),
        (46.0, 18.0, 24.0),
        (68.0, 22.0, 48.0),
    ] {
        let off = (sp * per / 24.0).rem_euclid(per);
        let blur = (speed * 0.035 * per / 24.0).min(per * 0.45);
        let n = if blur > 0.8 { 4 } else { 1 };
        let ink = Color32::from_rgba_unmultiplied(22, 12, 4, if n > 1 { 87 } else { 230 });
        for k in 0..n {
            let d = if n > 1 {
                (k as f32 / (n - 1) as f32 - 0.5) * blur
            } else {
                0.0
            };
            let mut u = -220.0 + off - per;
            while u < 220.0 {
                let (l, r) = (proj(u + d), proj(u + d + per / 2.0));
                if r - l > 0.2 {
                    clip.rect_filled(
                        Rect::from_min_max(fit.p(l, row_y), fit.p(r, row_y + row_h)),
                        0.0,
                        ink,
                    );
                }
                u += per;
            }
        }
    }
    let cap = Color32::from_rgb(131, 148, 166);
    label(p, fit, "<  FLAT", 34.0, 122.0, 9.5, 700, cap, Align2::LEFT_CENTER);
    label(
        p,
        fit,
        "SHARP  >",
        314.0,
        122.0,
        9.5,
        700,
        cap,
        Align2::RIGHT_CENTER,
    );
    label(
        p,
        fit,
        "STANDS STILL IN TUNE",
        174.0,
        122.0,
        9.5,
        700,
        Color32::from_rgb(160, 175, 191),
        Align2::CENTER_CENTER,
    );
}

// -- 3: the pedal ----------------------------------------------------------------------------------------------------------

const P_BIG: (f32, f32, f32, f32, f32) = (130.0, 32.0, 44.0, 64.0, 8.5);
const P_OCT: (f32, f32, f32, f32, f32) = (186.0, 62.0, 20.0, 34.0, 4.6);
const P_LED_Y: f32 = 146.0;
const LEDC: [[u8; 3]; 11] = [
    [255, 60, 48],
    [255, 60, 48],
    [255, 170, 40],
    [255, 170, 40],
    [255, 170, 40],
    [90, 240, 110],
    [255, 170, 40],
    [255, 170, 40],
    [255, 170, 40],
    [255, 60, 48],
    [255, 60, 48],
];

fn led_x(i: usize) -> f32 {
    54.0 + i as f32 * 24.0
}

/// A 7-segment character's segments (a-g).
fn segs(ch: char) -> &'static str {
    match ch.to_ascii_uppercase() {
        'A' => "abcefg",
        'B' => "cdefg",
        'C' => "adef",
        'D' => "bcdeg",
        'E' => "adefg",
        'F' => "aefg",
        'G' => "acdef",
        '0' => "abcdef",
        '1' => "bc",
        '2' => "abdeg",
        '3' => "abcdg",
        '4' => "bcfg",
        '5' => "acdfg",
        '6' => "acdefg",
        '7' => "abc",
        '8' => "abcdefg",
        '9' => "abcdfg",
        _ => "g",
    }
}

/// Segment `k`'s hexagon in a digit box, slanted like a real display.
fn seg_poly(b: (f32, f32, f32, f32, f32), k: char) -> Vec<(f32, f32)> {
    let (x, y, w, h, t) = b;
    let g = t * 0.18;
    let (xl, xr, yt, ym, yb) = (
        x + t / 2.0,
        x + w - t / 2.0,
        y + t / 2.0,
        y + h / 2.0,
        y + h - t / 2.0,
    );
    let hs = |cx: f32, cy: f32, len: f32| {
        vec![
            (cx - len / 2.0, cy),
            (cx - len / 2.0 + t / 2.0, cy - t / 2.0),
            (cx + len / 2.0 - t / 2.0, cy - t / 2.0),
            (cx + len / 2.0, cy),
            (cx + len / 2.0 - t / 2.0, cy + t / 2.0),
            (cx - len / 2.0 + t / 2.0, cy + t / 2.0),
        ]
    };
    let vs = |cx: f32, cy: f32, len: f32| {
        vec![
            (cx, cy - len / 2.0),
            (cx + t / 2.0, cy - len / 2.0 + t / 2.0),
            (cx + t / 2.0, cy + len / 2.0 - t / 2.0),
            (cx, cy + len / 2.0),
            (cx - t / 2.0, cy + len / 2.0 - t / 2.0),
            (cx - t / 2.0, cy - len / 2.0 + t / 2.0),
        ]
    };
    let (hl, v1, v2) = (xr - xl - 2.0 * g, ym - yt - 2.0 * g, yb - ym - 2.0 * g);
    let pts = match k {
        'a' => hs((xl + xr) / 2.0, yt, hl),
        'g' => hs((xl + xr) / 2.0, ym, hl),
        'd' => hs((xl + xr) / 2.0, yb, hl),
        'f' => vs(xl, (yt + ym) / 2.0, v1),
        'b' => vs(xr, (yt + ym) / 2.0, v1),
        'e' => vs(xl, (ym + yb) / 2.0, v2),
        _ => vs(xr, (ym + yb) / 2.0, v2),
    };
    let base = y + h;
    pts.into_iter()
        .map(|(px, py)| (px - 0.09 * (py - base), py))
        .collect()
}

fn pedal_body(cv: &mut Cv) {
    plate(cv, 348.0, 204.0, 16.0, Plate::Hammer);
    screws(
        cv,
        &[(18.0, 18.0), (330.0, 18.0), (18.0, 186.0), (330.0, 186.0)],
        5.2,
        77,
        false,
    );
    // the display window: a black frame, the red filter, the unlit "ghost" segments
    cv.rect(40.0, 16.0, 268.0, 96.0, 10.0, &kit::col(0x050505, 1.0));
    if let Some(edge) = kit::rrect(40.5, 16.5, 267.0, 95.0, 9.5) {
        cv.stroke(
            &edge,
            &kit::lin(0.0, 16.0, 0.0, 112.0, &[(0.0, 0, 0.7), (1.0, 0xffffff, 0.18)]),
            1.0,
        );
    }
    cv.rect(
        45.0,
        21.0,
        258.0,
        86.0,
        4.0,
        &kit::rad(
            174.0,
            64.0,
            150.0,
            150.0,
            &[(0.0, 0x1c0807, 1.0), (1.0, 0x0b0303, 1.0)],
        ),
    );
    for b in [P_BIG, P_OCT] {
        for k in "abcdefg".chars() {
            if let Some(path) = kit::poly(&seg_poly(b, k)) {
                cv.fill(&path, &kit::col(0xff281e, 0.075));
            }
        }
    }
    for (tip, base) in [(80.0, 64.0), (268.0, 284.0)] {
        if let Some(t) = kit::poly(&[(base, 52.0), (tip, 64.0), (base, 76.0)]) {
            cv.fill(&t, &kit::col(0xff281e, 0.08));
        }
    }
    // the LEDs: knurled-looking chrome bezels and their unlit lenses
    for (i, c) in LEDC.iter().enumerate() {
        let cx = led_x(i);
        cv.circle(
            cx,
            P_LED_Y,
            8.0,
            &kit::rad(
                cx - 2.0,
                P_LED_Y - 2.0,
                9.0,
                9.0,
                &[(0.0, 0xc9ccd1, 1.0), (0.6, 0x6b6f76, 1.0), (1.0, 0x2a2d31, 1.0)],
            ),
        );
        let dim = kit::from3([
            (c[0] as f32 * 0.18 + 20.0 * 0.82) as u8,
            (c[1] as f32 * 0.18 + 14.0 * 0.82) as u8,
            (c[2] as f32 * 0.18 + 12.0 * 0.82) as u8,
        ]);
        cv.circle(cx, P_LED_Y, 5.6, &kit::col(dim, 1.0));
    }
}

fn pedal_glass(cv: &mut Cv) {
    let Some(win) = kit::rrect(45.0, 21.0, 258.0, 86.0, 4.0) else {
        return;
    };
    let m = cv.mask(&win);
    if let Some(b) = kit::rrect(45.0, 21.0, 258.0, 11.0, 0.0) {
        cv.fill_in(
            &b,
            &kit::lin(0.0, 21.0, 0.0, 32.0, &[(0.0, 0, 0.6), (1.0, 0, 0.0)]),
            &m,
        );
    }
    if let Some(g) = kit::poly(&[(45.0, 21.0), (150.0, 21.0), (110.0, 107.0), (45.0, 107.0)]) {
        cv.fill_in(&g, &kit::col(0xffffff, 0.05), &m);
    }
    for i in 0..11 {
        cv.circle(led_x(i) - 1.8, P_LED_Y - 2.0, 1.4, &kit::col(0xffffff, 0.4));
    }
}

fn pedal_live(p: &Painter, fit: &Fit, a: &Anim, rd: &Reading) {
    let red = Color32::from_rgb(255, 58, 38);
    let poly = |b: (f32, f32, f32, f32, f32), k: char, col: Color32| {
        let pts: Vec<Pos2> = seg_poly(b, k).into_iter().map(|(x, y)| fit.p(x, y)).collect();
        p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
    };
    if let Some(heard) = rd.heard {
        let mut ch = heard.chars();
        let letter = ch.next().unwrap_or('-');
        let rest: String = ch.collect();
        let acc = rest.starts_with(['#', 'b', '♯', '♭']);
        let oct = rest
            .trim_start_matches(['#', 'b', '♯', '♭'])
            .chars()
            .next()
            .unwrap_or(' ');
        // a soft red bloom behind the lit digits, then the segments
        glow(p, fit.p(152.0, 64.0), fit.s(48.0), light([255, 50, 30], 0.25));
        for k in segs(letter).chars() {
            poly(P_BIG, k, red);
        }
        if oct.is_ascii_digit() {
            for k in segs(oct).chars() {
                poly(P_OCT, k, red);
            }
        }
        if acc {
            let sharp = rest.starts_with(['#', '♯']);
            label(
                p,
                fit,
                if sharp { "#" } else { "b" },
                198.0,
                40.0,
                14.0,
                800,
                red,
                Align2::CENTER_CENTER,
            );
        }
        // the arrows: the side to tune toward, both green in tune
        let tri = |l: bool, col: Color32| {
            let pts = if l {
                [fit.p(64.0, 52.0), fit.p(80.0, 64.0), fit.p(64.0, 76.0)]
            } else {
                [fit.p(284.0, 52.0), fit.p(268.0, 64.0), fit.p(284.0, 76.0)]
            };
            p.add(Shape::convex_polygon(pts.to_vec(), col, Stroke::NONE));
        };
        let green = Color32::from_rgb(92, 255, 124);
        if rd.in_tune {
            tri(true, green);
            tri(false, green);
        } else if let Some(c) = rd.cents {
            tri(c < 0.0, red);
        }
    }
    for (i, v) in a.led.iter().enumerate() {
        if *v < 0.03 {
            continue;
        }
        let c = fit.p(led_x(i), P_LED_Y);
        glow(p, c, fit.s(20.0), light(LEDC[i], 0.55 * v));
        let core = LEDC[i];
        let col = Color32::from_rgb(
            (core[0] as f32 + (255.0 - core[0] as f32) * 0.6 * v) as u8,
            (core[1] as f32 + (255.0 - core[1] as f32) * 0.6 * v) as u8,
            (core[2] as f32 + (255.0 - core[2] as f32) * 0.6 * v) as u8,
        );
        p.circle_filled(c, fit.s(5.6), col.gamma_multiply(*v));
    }
    let print = Color32::from_rgba_unmultiplied(235, 238, 242, 190);
    for (i, s) in [(0usize, "-50"), (3, "-20"), (5, "0"), (7, "+20"), (10, "+50")] {
        label(p, fit, s, led_x(i), 165.0, 9.0, 700, print, Align2::CENTER_CENTER);
    }
    label(p, fit, "b", 34.0, 146.0, 13.0, 700, print, Align2::CENTER_CENTER);
    label(p, fit, "#", 314.0, 146.0, 13.0, 700, print, Align2::CENTER_CENTER);
    label(
        p,
        fit,
        "CHROMATIC  ·  440 Hz",
        174.0,
        187.0,
        10.0,
        800,
        print,
        Align2::CENTER_CENTER,
    );
}

// -- 4: the rack arc lamps -----------------------------------------------------------------------------------------------

const A_C: (f32, f32) = (174.0, 150.0);
const A_N: usize = 21;
const A_SPAN: f32 = 62.0;

/// Lamp `i`'s angles (degrees from straight up).
fn lamp_angles(i: usize) -> (f32, f32) {
    let step = A_SPAN * 2.0 / A_N as f32;
    let a0 = -A_SPAN + i as f32 * step;
    (a0 + 0.7, a0 + step - 0.7)
}

/// Lamp `i`'s wedge as points (outer arc, then inner arc back).
fn lamp_pts(i: usize) -> Vec<(f32, f32)> {
    let (a, b) = lamp_angles(i);
    let (cx, cy) = A_C;
    let mut v = arc(cx, cy, 118.0, a, b);
    let mut inner = arc(cx, cy, 94.0, a, b);
    inner.reverse();
    v.extend(inner);
    v
}

fn arc_body(cv: &mut Cv) {
    plate(cv, 348.0, 180.0, 12.0, Plate::Smoke);
    // chrome standoff screws holding the plexiglass
    screws(
        cv,
        &[(13.0, 15.0), (335.0, 15.0), (13.0, 165.0), (335.0, 165.0)],
        5.0,
        101,
        false,
    );
    cv.rect(26.0, 9.0, 296.0, 162.0, 13.0, &kit::col(0x020407, 1.0));
    if let Some(edge) = kit::rrect(26.5, 9.5, 295.0, 161.0, 12.5) {
        cv.stroke(
            &edge,
            &kit::lin(0.0, 9.0, 0.0, 171.0, &[(0.0, 0, 0.7), (1.0, 0xa0c8f0, 0.2)]),
            1.0,
        );
    }
    cv.rect(
        30.0,
        13.0,
        288.0,
        154.0,
        10.0,
        &kit::rad(
            174.0,
            120.0,
            190.0,
            190.0,
            &[(0.0, 0x15100a, 1.0), (1.0, 0x060505, 1.0)],
        ),
    );
    // the lamps unlit under the smoke, and the print lit from behind
    for i in 0..A_N {
        if let Some(path) = kit::poly(&lamp_pts(i)) {
            let green = (i as i32 - 10).abs() <= 1;
            cv.fill(
                &path,
                &kit::col(
                    if green { 0x5ccb86 } else { 0xf2b53c },
                    if green { 0.08 } else { 0.07 },
                ),
            );
        }
    }
    let (cx, cy) = A_C;
    stroke_pts(cv, &arc(cx, cy, 124.0, -A_SPAN, A_SPAN), 0xe8cea0, 0.35, 0.7);
    for c in (-50i32..=50).step_by(5) {
        let a = c as f32 / 50.0 * (A_SPAN - 3.0);
        let major = c % 25 == 0;
        stroke_pts(
            cv,
            &[
                at(cx, cy, 124.0, a),
                at(cx, cy, if major { 131.0 } else { 127.0 }, a),
            ],
            0xe8cea0,
            0.35,
            if major { 1.0 } else { 0.6 },
        );
    }
}

fn arc_glass(cv: &mut Cv) {
    let Some(win) = kit::rrect(30.0, 13.0, 288.0, 154.0, 10.0) else {
        return;
    };
    let m = cv.mask(&win);
    // the diffuser's fine mesh, the lip's shadow, a broad reflection
    let mut y = 13.0;
    while y < 167.0 {
        stroke_pts(cv, &[(30.0, y), (318.0, y)], 0, 0.22, 0.5);
        y += 3.0;
    }
    let mut x = 30.0;
    while x < 318.0 {
        stroke_pts(cv, &[(x, 13.0), (x, 167.0)], 0, 0.22, 0.5);
        x += 3.0;
    }
    if let Some(b) = kit::rrect(30.0, 13.0, 288.0, 15.0, 0.0) {
        cv.fill_in(
            &b,
            &kit::lin(0.0, 13.0, 0.0, 28.0, &[(0.0, 0, 0.6), (1.0, 0, 0.0)]),
            &m,
        );
    }
    cv.ellipse(120.0, 14.0, 160.0, 40.0, &kit::col(0xffffff, 0.045));
    stroke_pts(cv, &[(40.0, 167.5), (308.0, 167.5)], 0xffffff, 0.14, 0.7);
}

fn arc_live(p: &Painter, fit: &Fit, a: &Anim, rd: &Reading) {
    let (cx, cy) = A_C;
    for (i, g) in a.glow.iter().enumerate() {
        if *g < 0.02 {
            continue;
        }
        let green = (i as i32 - 10).abs() <= 1;
        let col: [u8; 3] = if green { [110, 235, 145] } else { [255, 188, 72] };
        let (a0, a1) = lamp_angles(i);
        let (hx, hy) = at(cx, cy, 106.0, (a0 + a1) / 2.0);
        glow(p, fit.p(hx, hy), fit.s(34.0), light(col, 0.45 * g));
        // the lamp's own face: a strip of quads between its two arcs
        let pts = lamp_pts(i);
        let n = pts.len() / 2;
        let face = Color32::from_rgb(
            (col[0] as f32 + (255.0 - col[0] as f32) * 0.45) as u8,
            (col[1] as f32 + (255.0 - col[1] as f32) * 0.45) as u8,
            (col[2] as f32 + (255.0 - col[2] as f32) * 0.45) as u8,
        )
        .gamma_multiply(*g);
        let mut m = Mesh::default();
        for (x, y) in &pts {
            m.colored_vertex(fit.p(*x, *y), face);
        }
        for k in 0..(n as u32).saturating_sub(1) {
            let (o0, o1) = (k, k + 1);
            let (i0, i1) = (2 * n as u32 - 1 - k, 2 * n as u32 - 2 - k);
            m.add_triangle(o0, o1, i1);
            m.add_triangle(o0, i1, i0);
        }
        p.add(Shape::mesh(m));
    }
    let print = Color32::from_rgba_unmultiplied(232, 206, 160, 150);
    for c in [-50i32, -25, 0, 25, 50] {
        let (tx, ty) = at(cx, cy, 139.0, c as f32 / 50.0 * (A_SPAN - 3.0));
        let col = if c == 0 {
            Color32::from_rgba_unmultiplied(120, 220, 150, 180)
        } else {
            print
        };
        label(
            p,
            fit,
            &c.abs().to_string(),
            tx,
            ty,
            10.0,
            700,
            col,
            Align2::CENTER_CENTER,
        );
    }
    let (fx, fy) = at(cx, cy, 116.0, -78.0);
    let (sx, sy) = at(cx, cy, 116.0, 78.0);
    label(p, fit, "b", fx, fy, 14.0, 700, print, Align2::CENTER_CENTER);
    label(p, fit, "#", sx, sy, 14.0, 700, print, Align2::CENTER_CENTER);
    label(
        p,
        fit,
        "CENTS",
        cx,
        160.0,
        8.5,
        700,
        print.gamma_multiply(0.7),
        Align2::CENTER_CENTER,
    );
    // the note glowing in the middle, the cents (or semitones) under it
    let ok = rd.in_tune;
    let (ink, halo) = if ok {
        (Color32::from_rgb(140, 240, 165), [110u8, 235, 145])
    } else {
        (Color32::from_rgb(255, 198, 92), [255u8, 180, 60])
    };
    match (rd.heard, rd.cents) {
        (Some(n), Some(c)) => {
            glow(p, fit.p(cx, 110.0), fit.s(40.0), light(halo, 0.18));
            label(p, fit, n, cx, 110.0, 36.0, 800, ink, Align2::CENTER_CENTER);
            let sub = if rd.semis != 0 {
                let k = rd.semis.unsigned_abs();
                format!(
                    "{}{k} semitone{}",
                    if rd.semis > 0 { "+" } else { "-" },
                    if k == 1 { "" } else { "s" }
                )
            } else {
                let r = c.round() as i32;
                format!(
                    "{}{} ¢",
                    if r > 0 {
                        "+"
                    } else if r < 0 {
                        "-"
                    } else {
                        "±"
                    },
                    r.abs()
                )
            };
            label(
                p,
                fit,
                &sub,
                cx,
                140.0,
                12.0,
                700,
                ink.gamma_multiply(0.85),
                Align2::CENTER_CENTER,
            );
        }
        _ => label(p, fit, "–", cx, 112.0, 30.0, 800, print, Align2::CENTER_CENTER),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_round_trip_and_cycle() {
        for f in Face::ALL {
            assert_eq!(Face::parse(f.key()), f);
        }
        assert_eq!(Face::parse("nonsense"), Face::Arc, "the owner's default");
        assert_eq!(Face::default(), Face::Arc);
        let mut f = Face::Arc;
        let mut seen = vec![f];
        for _ in 0..4 {
            f = f.next();
            seen.push(f);
        }
        assert_eq!(f.next(), Face::Arc);
        seen.sort_by_key(|f| f.key());
        seen.dedup();
        assert_eq!(seen.len(), 5, "every face once");
    }

    #[test]
    fn the_lamps_and_leds_follow_the_cents() {
        let mut a = Anim::default();
        let rd = |c: f32| Reading {
            cents: Some(c),
            in_tune: c.abs() <= 5.0,
            heard: Some("A2"),
            semis: 0,
        };
        a.step(0.0, &rd(0.0));
        assert!(
            a.glow[10] > 0.99 && a.led[5] > 0.99 && a.lamp > 0.99,
            "in tune: middle lamp, green LED, lamp lit"
        );
        a.step(10.0, &rd(-30.0));
        assert!(a.glow[4] > 0.99 && a.led[2] > 0.99, "{:?} {:?}", a.glow, a.led);
        assert!((a.needle + 30.0).abs() < 0.01, "snapped after a long gap");
        // nothing heard: the needle rests pinned left, everything dark
        a.step(
            20.0,
            &Reading {
                cents: None,
                in_tune: false,
                heard: None,
                semis: 0,
            },
        );
        assert!(a.needle < -56.0 && a.glow.iter().all(|g| *g < 0.01));
    }
}
