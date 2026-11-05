//! Solid keys: ivory (or the accent, for the main action) with a glossy face, a visible lower edge and a tight
//! contact shadow; keys cut from a material; key caps engraved into a key; ivory thumbs; cut-gem round keys;
//! the jewel lamp (new in quickRS: note by note's switch). In the round 3 look the ivory and accent keys are flat
//! tiles (`make_tile`) and the transport's back and forward keys are Go stones (`make_stone`); both new, the owner's.
//! Port of rsplay/neu.py (`_make_key`, `_make_mat_key`, `_make_engraved`, `keycap`, `_make_thumb`, `thumb`,
//! `_make_gem`, `gem_key`, `_label`). The numpy recipes are run the same way (f32 masks); what the Python app
//! draws with pygame (the gems' facets, shadows and glow, the thumbs' smoothscale) goes through
//! crate::raster::pg, pygame's own routines.

use egui::{Align, Align2, Color32, Painter, Pos2, Rect};

use crate::material::{Mat, sheet64};
use crate::paint::{PxRect, Store, blit, ck, inset_px, ipx, key_of, pt, pxr, radius_px, sc, texture};
use crate::raster::pg::{self, Surf};
use crate::raster::{Image, Layers, Mask, rgbf};
use crate::text::{self, Style};
use crate::theme::{
    self, DANGER_ON_IVORY, IVORY_BOT, IVORY_EDGE, IVORY_INK, IVORY_KBD, IVORY_MID, IVORY_TOP, t,
};

struct KeyPalette {
    top: Color32,
    mid: Color32,
    bot: Color32,
    edge: Color32,
    gloss: f32,
    top_line: f32,
    shade: [f32; 3],
}

fn key_palette(accent: bool) -> KeyPalette {
    let th = t();
    if accent {
        KeyPalette {
            top: th.accent_hi,
            mid: th.accent,
            bot: th.accent_lo,
            edge: th.accent_edge,
            gloss: 0.45,
            top_line: 0.7,
            shade: [90.0, 50.0, 0.0],
        }
    } else {
        KeyPalette {
            top: IVORY_TOP,
            mid: IVORY_MID,
            bot: IVORY_BOT,
            edge: IVORY_EDGE,
            gloss: 0.8,
            top_line: 0.95,
            shade: [120.0, 95.0, 55.0],
        }
    }
}

/// The key's two shadows (or the one of a pressed key).
fn key_shadows(l: &mut Layers, shape: &Mask, pressed: bool, s: f64) {
    let th = t();
    let a = th.keysh_a;
    if pressed {
        l.over_c(
            th.key_sh,
            &shape
                .blur((pxr(1.0, s) as f32).max(0.6))
                .shift(0, pxr(1.0, s).max(1))
                .scale(a),
        );
    } else {
        l.over_c(
            th.key_sh,
            &shape
                .blur((pxr(4.0, s) as f32).max(0.8))
                .shift(0, pxr(4.0, s))
                .scale(a * 0.75),
        );
        l.over_c(
            th.key_sh,
            &shape
                .blur((pxr(0.6, s) as f32).max(0.5))
                .shift(0, pxr(1.0, s).max(1))
                .scale(a),
        );
    }
}

/// The margin round a key's surface: `px(10)`.
fn key_margin(s: f64) -> i32 {
    pxr(10.0, s)
}

fn make_key(w: usize, h: usize, rr: i32, accent: bool, pressed: bool, s: f64) -> Image {
    let k = key_palette(accent);
    let m = key_margin(s);
    let (cw, ch) = (w + 2 * m as usize, h + 2 * m as usize);
    let shape = Mask::rrect(w as f32, h as f32, rr as f32, m as f32, m as f32, cw, ch);
    let mut l = Layers::new(cw, ch);
    key_shadows(&mut l, &shape, pressed, s);
    let (top, mid, bot) = (rgbf(k.top), rgbf(k.mid), rgbf(k.bot));
    let mut face = Vec::with_capacity(cw * ch);
    let (gx, gy) = ((1.2 * w as f64).max(1.0) as f32, (0.7 * h as f64).max(1.0) as f32);
    let cx = (m as f64 + w as f64 / 2.0) as f32;
    for y in 0..ch {
        let ys = (y as f32 - m as f32) / (h.max(1)) as f32;
        let tt = ys.clamp(0.0, 1.0);
        let row: [f32; 3] = if tt < 0.55 {
            std::array::from_fn(|i| top[i] + (mid[i] - top[i]) * (tt / 0.55))
        } else {
            std::array::from_fn(|i| mid[i] + (bot[i] - mid[i]) * ((tt - 0.55) / 0.45))
        };
        let yr = (y as f32 - m as f32) / gy;
        let low = ((ys - 0.5) / 0.5).clamp(0.0, 1.0) * 0.16;
        for x in 0..cw {
            let xs = (x as f32 - cx) / gx;
            let gl = (1.0 - xs.hypot(yr) / 0.55).clamp(0.0, 1.0) * k.gloss;
            let mut c: [f32; 3] = std::array::from_fn(|i| row[i] * (1.0 - gl) + 255.0 * gl);
            for i in 0..3 {
                c[i] = c[i] * (1.0 - low) + k.shade[i] * low;
            }
            face.push(c);
        }
    }
    l.over_img(&face, &shape);
    l.over([255.0; 3], &shape.minus(&shape.shift(0, 1)).scale(k.top_line));
    let ed = pxr(if pressed { 1.0 } else { 3.0 }, s).max(1);
    l.over_c(k.edge, &shape.minus(&shape.shift(0, -ed)));
    l.image(None)
}

/// Corner radius (points) of a flat key at most, in the round 3 look.
const TILE_R: f32 = 7.0;

/// Smooth value noise in one dimension, -1..1.
fn vnoise(t: f32) -> f32 {
    let hash = |i: f32| -> f32 {
        let x = ((i as f64) * 12.9898 + 78.233).sin() * 43758.5453;
        (x - x.floor()) as f32 * 2.0 - 1.0
    };
    let (i, f) = (t.floor(), t - t.floor());
    let k = f * f * (3.0 - 2.0 * f);
    hash(i) + (hash(i + 1.0) - hash(i)) * k
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A key as a flat tile (round 3, the owner's): tight corners, a face lit a little from the upper left with a broad
/// satin sheen across its top (no gloss spot), a polish line along the top and a thin edge along the bottom, a tight
/// contact shadow. Ivory has its grain: fine lines along the key, wavering, a few darker fibres, faint yellowing.
fn make_tile(w: usize, h: usize, rr: i32, accent: bool, pressed: bool, s: f64) -> Image {
    let k = key_palette(accent);
    let th = t();
    let m = key_margin(s);
    let (cw, ch) = (w + 2 * m as usize, h + 2 * m as usize);
    let shape = Mask::rrect(w as f32, h as f32, rr as f32, m as f32, m as f32, cw, ch);
    let mut l = Layers::new(cw, ch);
    let a = th.keysh_a;
    if !pressed {
        l.over_c(
            th.key_sh,
            &shape
                .blur((pxr(1.8, s) as f32).max(0.7))
                .shift(0, pxr(2.0, s).max(1))
                .scale(a * 0.45),
        );
    }
    l.over_c(
        th.key_sh,
        &shape.blur(0.6).shift(0, pxr(1.0, s).max(1)).scale(a * 0.9),
    );
    let (top, mid, bot) = (rgbf(k.top), rgbf(k.mid), rgbf(k.bot));
    let yellow = [236.0f32, 222.0, 186.0];
    let unit = (s as f32).max(0.5);
    let mut face = Vec::with_capacity(cw * ch);
    for y in 0..ch {
        let ys = ((y as f32 - m as f32) / h.max(1) as f32).clamp(0.0, 1.0);
        for x in 0..cw {
            let xs = ((x as f32 - m as f32) / w.max(1) as f32).clamp(0.0, 1.0);
            // lit a little from the upper left: lighter there, warmer toward the lower right
            let tt = (0.62 * ys + 0.38 * xs).clamp(0.0, 1.0);
            let base: [f32; 3] = if tt < 0.6 {
                std::array::from_fn(|i| top[i] + (mid[i] - top[i]) * (0.25 + tt / 0.6 * 0.75))
            } else {
                std::array::from_fn(|i| mid[i] + (bot[i] - mid[i]) * ((tt - 0.6) / 0.4) * 0.45)
            };
            let mut c = base;
            if !accent {
                // the grain, in screen pixels (so it's as fine on a big key as a small one)
                let (gx, gy) = (x as f32 / unit, y as f32 / unit);
                let wy = gy + 0.9 * (gx * 0.019 + 1.3).sin() + 0.5 * (gx * 0.006 + gy * 0.09).sin();
                let line = 0.65 * vnoise(wy * 1.1) + 0.35 * vnoise(wy * 0.37 + 11.0);
                let fibre = smoothstep(0.55, 0.9, vnoise(wy * 0.21 + 5.0))
                    * (0.5 + 0.5 * vnoise(gx * 0.04 + wy * 0.3));
                let g = 1.0 + 0.016 * line - 0.03 * fibre;
                let age = 0.06 * (0.5 + 0.5 * vnoise(gx * 0.012 + gy * 0.05 + 3.0));
                for i in 0..3 {
                    c[i] = (c[i] * (1.0 - age) + yellow[i] * age) * g;
                }
            }
            // a broad satin sheen across the top third, strongest toward the light
            let sheen = (-((ys - 0.24) / 0.2).powi(2)).exp() * (0.75 + 0.25 * (1.0 - xs));
            let sk = if accent { 0.16 } else { 0.11 } * sheen;
            face.push(std::array::from_fn(|i| {
                (c[i] * (1.0 - sk) + 255.0 * sk).min(255.0)
            }));
        }
    }
    l.over_img(&face, &shape);
    l.over(
        [255.0; 3],
        &shape.minus(&shape.shift(0, 1)).scale(k.top_line * 0.6),
    );
    l.over([255.0; 3], &shape.minus(&shape.shift(1, 0)).scale(0.22));
    let ed = pxr(if pressed { 1.0 } else { 1.6 }, s).max(1);
    l.over_c(k.edge, &shape.minus(&shape.shift(0, -ed)).scale(0.85));
    l.over_c(k.edge, &shape.minus(&shape.shift(-1, 0)).scale(0.25));
    l.image(None)
}

/// A white clamshell Go stone (hamaguri) `d` px across, seen from above on the panel, lit from the upper left as everything else is: a
/// lens-shaped body (shaded by its curve), a soft broad sheen and a smaller brighter one where the light reflects,
/// light glowing through the rim on the far side, the shell's fine growth lines, and its
/// shadow on the panel (a soft one and a tight contact one; only the contact one when pressed). Drawn 3x and
/// shrunk. Returns (surface, margin).
fn make_stone(d: i32, pressed: bool) -> (Image, i32) {
    let th = t();
    let m = ((d as f64 * 0.22).ceil() as i32).max(5);
    let n = (d + 2 * m) as usize;
    let ss = 3usize;
    let nn = n * ss;
    let c = nn as f32 / 2.0;
    let rad = (d as f32 / 2.0 - 0.25) * ss as f32;
    let norm = |x: f32, y: f32, z: f32| {
        let l = (x * x + y * y + z * z).sqrt();
        (x / l, y / l, z / l)
    };
    let lt = norm(-0.42, -0.58, 0.70);
    let hv = norm(lt.0, lt.1, lt.2 + 1.0);
    // the body a soft white, so the glint can show brighter than it
    let body = [245.0f32, 244.0, 239.0];
    let (amb, dif) = (0.68f32, 0.30f32);
    // a broad satin sheen and a crisp glint (strength, exponent)
    let ((s1, e1), (s2, e2)) = ((0.22f32, 28.0f32), (0.85f32, 900.0f32));
    // light through the rim, and its tint
    let (sss, sss_tint) = (0.10f32, [255.0f32, 252.0, 240.0]);
    // the shell's growth lines are arcs about a point far off the stone
    let fx = c + 2.8 * rad * (-150f32).to_radians().cos();
    let fy = c + 2.8 * rad * (-150f32).to_radians().sin();
    let mut col = vec![[0f32; 3]; n * n];
    let mut cov = vec![0f32; n * n];
    for y in 0..nn {
        for x in 0..nn {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let rr = (dx * dx + dy * dy).sqrt();
            let a = (rad - rr + 0.5).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let (u, v) = (dx / rad, dy / rad);
            let r = (u * u + v * v).sqrt().min(1.0);
            // a lens, not a ball: nearly flat across the middle, curving away fast toward the rim
            let tilt = 1.19 * r.powf(1.7);
            let (su, sv) = if r > 1e-6 { (u / r, v / r) } else { (0.0, 0.0) };
            let nv = (tilt.sin() * su, tilt.sin() * sv, tilt.cos());
            let ndl = (nv.0 * lt.0 + nv.1 * lt.1 + nv.2 * lt.2).max(0.0);
            let ndh = (nv.0 * hv.0 + nv.1 * hv.1 + nv.2 * hv.2).max(0.0);
            // the growth lines: thin, varying in spacing, clearer in some bands than others, a faint warmth in a few
            let mut cc = body;
            let fd = ((x as f32 - fx).powi(2) + (y as f32 - fy).powi(2)).sqrt() / rad;
            let period = 0.055 * (1.0 + 0.25 * (fd * 2.3).sin());
            let ph = fd / period + 0.35 * vnoise(fd * 7.0);
            let line = smoothstep(0.6, 1.0, 0.5 + 0.5 * (ph * std::f32::consts::TAU).sin());
            let band = 0.3 + 0.7 * (0.5 + 0.5 * (fd * 5.1 + 1.7).sin());
            let warm = 0.5 + 0.5 * (fd * 3.3 + 0.4).sin();
            for (i, ci) in cc.iter_mut().enumerate() {
                let tint = [1.0, 0.99, 0.965][i];
                *ci *= (1.0 - 0.045 * line * band) * (1.0 - (1.0 - tint) * 0.6 * warm);
            }
            // shaded by its curve; the lower rim also sees the dark panel
            let shade = amb + dif * ndl - 0.07 * smoothstep(0.35, 1.0, r) * v.max(0.0);
            // light through the rim on the side away from the light
            let rim = smoothstep(0.78, 0.96, r) * (1.0 - smoothstep(0.96, 1.0, r));
            let back = rim * (0.6 * u + 0.8 * v).max(0.0) * sss;
            let spec = s1 * ndh.powf(e1) + s2 * ndh.powf(e2);
            let edge = 1.0 - 0.16 * smoothstep(0.93, 1.0, r);
            let i = (y / ss) * n + x / ss;
            for ch in 0..3 {
                let v = (cc[ch] * shade + sss_tint[ch] * back) * edge + 255.0 * spec;
                col[i][ch] += v.min(255.0) * a;
            }
            cov[i] += a;
        }
    }
    let k2 = (ss * ss) as f32;
    let face: Vec<[f32; 3]> = col
        .iter()
        .zip(&cov)
        .map(|(cl, &cv)| {
            if cv > 0.0 {
                std::array::from_fn(|i| cl[i] / cv)
            } else {
                [0.0; 3]
            }
        })
        .collect();
    let alpha = Mask {
        w: n,
        h: n,
        v: cov.iter().map(|&cv| cv / k2).collect(),
    };
    let disc = Mask::disc(n, n, n as f32 / 2.0, n as f32 / 2.0, d as f32 / 2.0);
    let df = d as f32;
    let mut l = Layers::new(n, n);
    let a = th.keysh_a;
    if !pressed {
        l.over_c(
            th.key_sh,
            &disc
                .blur((0.1 * df).max(1.0))
                .shift((0.04 * df) as i32, (0.12 * df).round() as i32)
                .scale(a * 0.6),
        );
    }
    l.over_c(
        th.key_sh,
        &disc
            .blur((0.03 * df).max(0.6))
            .shift(0, ((0.035 * df).round() as i32).max(1))
            .scale(a * 0.95),
    );
    l.over_img(&face, &alpha);
    (l.image(None), m)
}

/// A Go stone round key's body at rect (no icon): `min(w, h)` across, centred.
pub(crate) fn stone_px(p: &Painter, pr: PxRect, pressed: bool) {
    let d = pr.w.min(pr.h);
    if d < 6 {
        return;
    }
    let key = key_of(("stone", d, pressed, theme::generation()));
    let m = ((d as f64 * 0.22).ceil() as i32).max(5);
    let (tex, size) = texture(p.ctx(), Store::Small, key, || make_stone(d, pressed).0);
    let (cx, cy) = (pr.x + pr.w / 2, pr.y + pr.h / 2);
    blit(
        p,
        tex,
        size,
        (cx - d / 2 - m) as f32,
        (cy - d / 2 - m) as f32,
        Color32::WHITE,
    );
}

/// A key's body (no label) at rect: ivory, or the accent when it's the main action.
pub fn key_face(p: &Painter, rect: Rect, r: Option<f32>, accent: bool, pressed: bool) {
    key_face_px(
        p,
        PxRect::of(rect, p.pixels_per_point()),
        r.map(f64::from),
        accent,
        pressed,
    );
}

/// `key_face` on a rect in whole pixels.
pub(crate) fn key_face_px(p: &Painter, pr: PxRect, r: Option<f64>, accent: bool, pressed: bool) {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    if pr.w < 2 || pr.h < 2 {
        return;
    }
    // the dark themes: the main action (accent) a glass plate with the room in it (P34.5; a square key is a disc),
    // any other key the quiet well (P37.3, D110)
    if crate::quiet::on() && !accent {
        crate::quiet::key_px(p, pr, r, crate::quiet::State::of(pressed, pressed));
        return;
    }
    if crate::glass::on() {
        crate::glass::key(p, pr.to_rect(ppp), crate::glass::State::of(false, pressed));
        return;
    }
    let rr = radius_px(pr.w, pr.h, r, ppp);
    // round 3: a flat tile, its corners tighter unless it's a round key
    let tile = !theme::classic();
    let rr = if tile && (r.is_some() || pr.w != pr.h) {
        rr.min(pxr(TILE_R, s))
    } else {
        rr
    };
    let key = key_of((
        tile,
        "key",
        pr.w,
        pr.h,
        rr,
        accent,
        pressed,
        theme::generation(),
        ck(t().accent),
    ));
    let m = key_margin(s);
    let (tex, size) = texture(p.ctx(), Store::Small, key, || {
        if tile {
            make_tile(pr.w as usize, pr.h as usize, rr, accent, pressed, s)
        } else {
            make_key(pr.w as usize, pr.h as usize, rr, accent, pressed, s)
        }
    });
    blit(p, tex, size, (pr.x - m) as f32, (pr.y - m) as f32, Color32::WHITE);
}

fn make_mat_key(w: usize, h: usize, rr: i32, mat: Mat, pressed: bool, seed: u32, s: f64) -> Image {
    let mt = mat.get();
    let m = key_margin(s);
    let (cw, ch) = (w + 2 * m as usize, h + 2 * m as usize);
    let shape = Mask::rrect(w as f32, h as f32, rr as f32, m as f32, m as f32, cw, ch);
    let mut l = Layers::new(cw, ch);
    key_shadows(&mut l, &shape, pressed, s);
    let sh = sheet64(mat, s);
    // each key cut from its own part of the sheet
    let ox = (((seed * 97) % 220) as f64 * s) as i64;
    let oy = (((seed * 53) % 90) as f64 * s) as i64;
    let mut face = Vec::with_capacity(cw * ch);
    let over = |img: [f32; 3], color: [f32; 3], alpha: f32| -> [f32; 3] {
        let a = alpha.clamp(0.0, 1.0);
        std::array::from_fn(|i| img[i] * (1.0 - a) + color[i] * a)
    };
    let f3 = |c: [u8; 3]| [c[0] as f32, c[1] as f32, c[2] as f32];
    for y in 0..ch {
        let ys = (y as f32 - m as f32) / h.max(1) as f32;
        let yc = ys.clamp(0.0, 1.0);
        for x in 0..cw {
            let xs = (x as f32 - m as f32) / w.max(1) as f32;
            let mut c = sh.at(x as i64 - m as i64 + ox, y as i64 - m as i64 + oy);
            if let Some((c0, c1, ta)) = mt.tint {
                // aluminium: a little lighter at the top, cooler at the bottom
                let (c0, c1) = (f3(c0), f3(c1));
                c = std::array::from_fn(|i| c[i] * (1.0 - ta) + (c0[i] + (c1[i] - c0[i]) * yc) * ta);
            }
            if let Some((vc, v0, v1)) = mt.veil {
                c = over(c, f3(vc), v0 + (v1 - v0) * yc); // darker toward the bottom, so the labels read
            }
            if let Some((x0, xp, x1, sa)) = mt.sheen {
                let v = if xs < xp {
                    (xs - x0) / (xp - x0)
                } else {
                    (x1 - xs) / (x1 - xp)
                };
                c = over(c, [255.0; 3], v.clamp(0.0, 1.0) * sa);
            }
            let (gc, ga, (gw, gh), gend) = mt.gloss;
            c = over(
                c,
                f3(gc),
                (1.0 - ((xs - 0.5) / gw).hypot(ys / gh) / gend).clamp(0.0, 1.0) * ga,
            );
            face.push(c);
        }
    }
    l.over_img(&face, &shape);
    let (ic, ia) = mt.inner;
    let rising = shape
        .shift(0, -pxr(5.0, s))
        .map(|v| 1.0 - v)
        .blur((pxr(3.5, s) as f32).max(0.6));
    l.over(f3(ic), &rising.map(|v| v.clamp(0.0, 1.0)).times(&shape).scale(ia));
    let ed = pxr(if pressed { 1.0 } else { 3.0 }, s).max(1);
    l.over(f3(mt.edge), &shape.minus(&shape.shift(0, -ed)));
    let (rc, ra) = mt.rim;
    l.over(f3(rc), &shape.minus(&shape.shift(0, 1)).scale(ra));
    l.image(None)
}

/// `mat_key` on a rect in whole pixels.
pub(crate) fn mat_key_px(p: &Painter, pr: PxRect, mat: Mat, r: Option<f64>, pressed: bool, seed: u32) {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    if pr.w < 2 || pr.h < 2 {
        return;
    }
    let rr = radius_px(pr.w, pr.h, r, ppp);
    let key = key_of((
        "mat",
        mat,
        pr.w,
        pr.h,
        rr,
        pressed,
        seed,
        theme::generation(),
        s.to_bits(),
    ));
    let m = key_margin(s);
    let (tex, size) = texture(p.ctx(), Store::Small, key, || {
        make_mat_key(pr.w as usize, pr.h as usize, rr, mat, pressed, seed, s)
    });
    blit(p, tex, size, (pr.x - m) as f32, (pr.y - m) as f32, Color32::WHITE);
}

/// How a key cap sits: sunk into the material, or engraved into a key's face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CapOn {
    Material,
    Ivory,
    Accent,
    /// engraved as on the accent, into a fill that isn't a key (stays so on the glass look: the RS Highway pill)
    Fill,
    /// tortoiseshell or abalone
    Dark,
    /// aluminium
    Metal,
}

impl CapOn {
    pub fn for_mat(mat: Mat) -> CapOn {
        if mat.get().light {
            CapOn::Metal
        } else {
            CapOn::Dark
        }
    }

    fn engrave(self) -> Option<([f32; 3], f32, f32, f32)> {
        match self {
            CapOn::Material => None,
            CapOn::Ivory => Some(([80.0, 60.0, 30.0], 0.07, 0.3, 0.7)),
            CapOn::Accent | CapOn::Fill => Some(([90.0, 50.0, 0.0], 0.1, 0.38, 0.4)),
            CapOn::Dark => Some(([0.0, 0.0, 0.0], 0.28, 0.4, 0.12)),
            CapOn::Metal => Some(([40.0, 40.0, 60.0], 0.07, 0.28, 0.7)),
        }
    }

    fn ink(self) -> Color32 {
        match self {
            CapOn::Material => t().text_2,
            CapOn::Ivory => IVORY_KBD,
            CapOn::Accent | CapOn::Fill => Color32::from_rgb(90, 52, 0),
            CapOn::Dark => Color32::from_rgb(236, 228, 218),
            CapOn::Metal => Color32::from_rgb(61, 70, 82),
        }
    }
}

fn make_engraved(w: usize, h: usize, rr: i32, kind: CapOn) -> Image {
    let m = Mask::rrect_full(w, h, rr as f32);
    let (tint, a_fill, a_dark, a_light) = kind.engrave().unwrap_or(([0.0; 3], 0.0, 0.0, 0.0));
    let mut l = Layers::new(w, h);
    l.over(tint, &m.scale(a_fill));
    l.over(tint, &m.minus(&m.shift(1, 1)).scale(a_dark));
    l.over([255.0; 3], &m.minus(&m.shift(-1, -1)).scale(a_light));
    l.image(None)
}

/// A text's width and height in whole pixels, as pygame's surface for it has them.
fn text_px(ctx: &egui::Context, label: &str, st: Style) -> (i32, i32) {
    let ppp = ctx.pixels_per_point();
    (
        (text::width(ctx, label, st) * ppp).round() as i32,
        (text::line_height(ctx, st) * ppp).round() as i32,
    )
}

/// A rect `w` x `h` (pixels) placed by pygame's `setattr(rect, anchor, (x, y))` (x, y whole pixels).
pub(crate) fn anchored(anchor: Align2, x: i32, y: i32, w: i32, h: i32) -> PxRect {
    let rx = match anchor.x() {
        Align::Min => x,
        Align::Center => x - w / 2,
        Align::Max => x - w,
    };
    let ry = match anchor.y() {
        Align::Min => y,
        Align::Center => y - h / 2,
        Align::Max => y - h,
    };
    PxRect { x: rx, y: ry, w, h }
}

/// A shortcut hint: the key's name on a small cap (`neu.keycap`: the text's box plus `px(12)` by `px(2)`, placed
/// by its anchor at `int(pos)`, the label centred on it). Returns its rect.
pub fn keycap(p: &Painter, label: &str, pos: Pos2, anchor: Align2, size: f32, on: CapOn) -> Rect {
    // on a glass key (the dark themes): a thin outline in the key's ink, the name in it (P34.5)
    let glass = crate::glass::on() && matches!(on, CapOn::Ivory | CapOn::Accent);
    let ink = if glass {
        crate::glass::ink(on == CapOn::Accent).gamma_multiply(0.85)
    } else {
        on.ink()
    };
    let st = Style::new(size, 700, ink);
    let ctx = p.ctx();
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let (tw, th) = text_px(ctx, label, st);
    let r = anchored(
        anchor,
        ipx(pos.x, ppp),
        ipx(pos.y, ppp),
        tw + pxr(12.0, s),
        th + pxr(2.0, s),
    );
    if glass {
        crate::glass::cap(p, r.to_rect(ppp), ink);
    } else if on == CapOn::Material {
        inset_px(p, r, Some(6.0), 2.0, crate::paint::Fill::Base);
    } else {
        let rr = pxr(6.0, s).min(r.h / 2);
        let key = key_of(("engraved", r.w, r.h, rr, on));
        let (tex, sz) = texture(ctx, Store::Small, key, || {
            make_engraved(r.w.max(1) as usize, r.h.max(1) as usize, rr, on)
        });
        blit(p, tex, sz, r.x as f32, r.y as f32, Color32::WHITE);
    }
    let c = Pos2::new(pt(r.x + r.w / 2, ppp), pt(r.y + r.h / 2, ppp));
    text::draw(p, label, c, Align2::CENTER_CENTER, st, None);
    r.to_rect(ppp)
}

/// `keycap_width`: the text's width plus `px(12)` (0 for no key).
pub fn keycap_width(ctx: &egui::Context, label: &str, size: f32) -> f32 {
    if label.is_empty() {
        return 0.0;
    }
    let ppp = ctx.pixels_per_point();
    let (tw, _) = text_px(ctx, label, Style::new(size, 700, Color32::WHITE));
    (tw + pxr(12.0, sc(ppp))) as f32 / ppp
}

fn make_thumb(d: usize, s: f64) -> Image {
    let th = t();
    make_thumb_with(
        d,
        s,
        (rgbf(th.thumb_top), rgbf(th.thumb_bot)),
        ([255.0; 3], 0.95),
        ([110.0, 90.0, 50.0], 0.25),
    )
}

/// A thumb's sprite: its face from `top` to `bot`, a gloss of `gloss` (colour, strength) up-left, a rim of `rim`
/// along its lower edge.
fn make_thumb_with(
    d: usize,
    s: f64,
    (top, bot): ([f32; 3], [f32; 3]),
    gloss: ([f32; 3], f32),
    rim: ([f32; 3], f32),
) -> Image {
    let ss = 3usize;
    let m = pxr(6.0, s);
    let n = (d + 2 * m as usize) * ss;
    let c = n as f32 / 2.0;
    let r = (d * ss) as f32 / 2.0;
    let th = t();
    let disc = Mask::from_fn(n, n, |x, y| {
        let dist = (x as f32 + 0.5 - c).hypot(y as f32 + 0.5 - c);
        (r - dist + 0.5 * ss as f32).clamp(0.0, ss as f32) / ss as f32
    });
    let mut l = Layers::new(n, n);
    let a = th.keysh_a;
    l.over_c(
        th.key_sh,
        &disc
            .blur((2.5 * ss as f64 * s) as f32)
            .shift(0, (2.0 * ss as f64 * s) as i32)
            .scale(a * 0.8),
    );
    l.over_c(
        th.key_sh,
        &disc
            .blur(0.6 * ss as f32)
            .shift(0, (ss as f64 * s) as i32)
            .scale(a),
    );
    let mut face = Vec::with_capacity(n * n);
    let mut rimlo = Mask::new(n, n);
    for y in 0..n {
        let yy = y as f32 + 0.5;
        let v = ((yy - (c - r)) / (2.0 * r)).clamp(0.0, 1.0);
        for x in 0..n {
            let xx = x as f32 + 0.5;
            let mut col: [f32; 3] = std::array::from_fn(|i| top[i] + (bot[i] - top[i]) * v);
            let gl = (1.0 - (xx - (c - 0.2 * r)).hypot(yy - (c - 0.44 * r)) / (0.96 * r)).clamp(0.0, 1.0);
            for (ch, g) in col.iter_mut().zip(gloss.0) {
                *ch = *ch * (1.0 - gloss.1 * gl) + g * gloss.1 * gl;
            }
            face.push(col);
            let dist = (xx - c).hypot(yy - c);
            let i = y * n + x;
            rimlo.v[i] = ((dist - (r - 1.5 * ss as f32)) / (1.5 * ss as f32)).clamp(0.0, 1.0)
                * ((yy - c) / r).clamp(0.0, 1.0)
                * disc.v[i];
        }
    }
    l.over_img(&face, &disc);
    l.over(rim.0, &rimlo.scale(rim.1));
    let out = (d + 2 * m as usize) as i32;
    Surf::from_image(&l.image(None)).smoothscale(out, out).image()
}

/// A thumb (a switch's or slider's; ivory, graphite on the dark themes) `d` points across (`int(d * s)` pixels, at least 6) centred on
/// `int(center)`. Returns its rect.
pub fn thumb(p: &Painter, center: Pos2, d: f32) -> Rect {
    let ppp = p.pixels_per_point();
    if crate::quiet::on() {
        // the dark themes: graphite, not ivory (P37.3, D111)
        return quiet_thumb_px(p, ipx(center.x, ppp), ipx(center.y, ppp), ipx(d, ppp)).to_rect(ppp);
    }
    thumb_px(p, ipx(center.x, ppp), ipx(center.y, ppp), ipx(d, ppp)).to_rect(ppp)
}

/// `neu.thumb` with the centre and diameter in whole pixels.
pub(crate) fn thumb_px(p: &Painter, cx: i32, cy: i32, d: i32) -> PxRect {
    let s = sc(p.pixels_per_point());
    let d = d.max(6);
    let m = pxr(6.0, s);
    let key = key_of(("thumb", d, theme::generation()));
    let (tex, size) = texture(p.ctx(), Store::Small, key, || make_thumb(d as usize, s));
    let r = PxRect {
        x: cx - d / 2,
        y: cy - d / 2,
        w: d,
        h: d,
    };
    blit(p, tex, size, (r.x - m) as f32, (r.y - m) as f32, Color32::WHITE);
    r
}

/// The quiet look's thumb (P37.3, D111; the dark themes' switches and sliders): a graphite disc `d` pixels across,
/// a faint sheen up-left, no white. Cached per size and theme like the ivory one.
pub(crate) fn quiet_thumb_px(p: &Painter, cx: i32, cy: i32, d: i32) -> PxRect {
    let s = sc(p.pixels_per_point());
    let d = d.max(6);
    let m = pxr(6.0, s);
    let key = key_of(("thumb-q", d, theme::generation()));
    let (tex, size) = texture(p.ctx(), Store::Small, key, || {
        let (top, bot, sheen) = crate::quiet::thumb_colours();
        make_thumb_with(
            d as usize,
            s,
            (rgbf(top), rgbf(bot)),
            (rgbf(sheen), 0.45),
            ([0.0; 3], 0.35),
        )
    });
    let r = PxRect {
        x: cx - d / 2,
        y: cy - d / 2,
        w: d,
        h: d,
    };
    blit(p, tex, size, (r.x - m) as f32, (r.y - m) as f32, Color32::WHITE);
    r
}

// -- gems ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gem {
    Ruby,
    Emerald,
    /// A ruby unlit (the jewel lamp off; new in quickRS).
    Garnet,
}

pub struct GemSpec {
    pub h: f64,
    pub s: f64,
    pub l: f64,
    pub k: f64,
    pub alt: f64,
    pub ink: Color32,
    pub glow: Color32,
    pub shade: Color32,
}

impl Gem {
    pub fn spec(self) -> GemSpec {
        match self {
            Gem::Ruby => GemSpec {
                h: 349.0,
                s: 76.0,
                l: 31.0,
                k: 15.0,
                alt: 5.0,
                ink: Color32::from_rgb(255, 243, 245),
                glow: Color32::from_rgb(255, 70, 110),
                shade: Color32::from_rgb(40, 0, 8),
            },
            Gem::Emerald => GemSpec {
                h: 154.0,
                s: 74.0,
                l: 24.0,
                k: 15.0,
                alt: 5.0,
                ink: Color32::from_rgb(242, 255, 249),
                glow: Color32::from_rgb(56, 224, 150),
                shade: Color32::from_rgb(0, 30, 18),
            },
            Gem::Garnet => GemSpec {
                h: 347.0,
                s: 55.0,
                l: 15.0,
                k: 11.0,
                alt: 4.0,
                ink: Color32::from_rgb(255, 243, 245),
                glow: Color32::from_rgb(120, 30, 50),
                shade: Color32::from_rgb(20, 0, 4),
            },
        }
    }
}

/// colorsys.hls_to_rgb
pub fn hls_to_rgb(h: f32, l: f32, s: f32) -> (f32, f32, f32) {
    let (r, g, b) = hls_to_rgb64(h as f64, l as f64, s as f64);
    (r as f32, g as f32, b as f32)
}

/// colorsys.hls_to_rgb in Python's f64.
fn hls_to_rgb64(h: f64, l: f64, s: f64) -> (f64, f64, f64) {
    if s == 0.0 {
        return (l, l, l);
    }
    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let m1 = 2.0 * l - m2;
    let v = |hue: f64| {
        let hue = hue.rem_euclid(1.0);
        if hue < 1.0 / 6.0 {
            m1 + (m2 - m1) * hue * 6.0
        } else if hue < 0.5 {
            m2
        } else if hue < 2.0 / 3.0 {
            m1 + (m2 - m1) * (2.0 / 3.0 - hue) * 6.0
        } else {
            m1
        }
    };
    (v(h + 1.0 / 3.0), v(h), v(h - 1.0 / 3.0))
}

/// A facet's colour at lightness `lum`: bright facets lose saturation, toward white (`_gem_tone`).
fn gem_tone(g: &GemSpec, lum: f64) -> pg::Rgba {
    let lum = lum.clamp(4.0, 97.0);
    let sat = g.s * (1.0 - ((lum - g.l) / 45.0).clamp(0.0, 0.55));
    let (r, gg, b) = hls_to_rgb64(g.h / 360.0, lum / 100.0, sat / 100.0);
    let q = |v: f64| (v * 255.0).round_ties_even().clamp(0.0, 255.0) as u8;
    [q(r), q(gg), q(b), 255]
}

/// `_make_gem`: a round brilliant `d` px across, with its shadow and the coloured light it throws on the panel
/// under it. Returns (surface, margin).
fn make_gem(d: i32, gem: Gem, pressed: bool) -> (Image, i32) {
    let g = gem.spec();
    let th = t();
    let ss = 3i32;
    let m = ((d as f64 * 0.24).ceil() as i32).max(4);
    let n1 = d + 2 * m;
    let n = n1 * ss;
    let c = n as f64 / 2.0;
    let rr = (d as f64 / 2.0 - 0.5) * ss as f64;
    let (rt, rs) = (rr * 0.5, rr * 0.76);
    let pt = |deg: f64, r: f64| {
        let a = deg.to_radians();
        (c + r * a.cos(), c + r * a.sin())
    };
    let light = (-115.0f64).to_radians();
    let lit = |deg: f64| (deg.to_radians() - light).cos();
    let jitter = |i: f64| {
        let x = (i * 12.9898 + g.h).sin() * 43758.5453;
        (x - x.floor()) * 2.0 - 1.0
    };
    let girdle: Vec<(f64, f64)> = (0..16).map(|i| pt(i as f64 * 22.5, rr)).collect();
    // under it: a tight shadow, a soft one and (unless pressed in) the stone's own light on the panel
    let mut under = Surf::new(n1, n1);
    let a_sh = th.keysh_a as f64;
    let k = d as f64 / 56.0;
    let passes: &[(f64, f64, f64)] = if pressed {
        &[(1.0, 1.0, 1.0)]
    } else {
        &[(1.0, 1.0, 1.1), (4.0, 3.0, 0.8)]
    };
    let key_sh = pg::rgb(th.key_sh);
    for &(dy, blur, alpha) in passes {
        let mut layer = Surf::new(n1, n1);
        let pts: Vec<(f64, f64)> = girdle
            .iter()
            .map(|&(x, y)| (x / ss as f64, y / ss as f64 + dy * k + 1.0))
            .collect();
        pg::polygon(&mut layer, key_sh, &pts, 0);
        let mut layer = layer.gaussian_blur(((blur * k).round_ties_even() as i32).max(1));
        layer.fill_mult([255, 255, 255, (255.0 * (a_sh * alpha).min(1.0)) as u8]);
        under.blit(&layer, 0, 0);
    }
    if !pressed {
        let mut glow = Surf::new(n1, n1);
        let (gw, gh) = (d as f64 * 0.72, 12.0 * d as f64 / 64.0);
        let (c1, r1) = (n1 as f64 / 2.0, d as f64 / 2.0 - 0.5);
        pg::ellipse(
            &mut glow,
            pg::rgb(g.glow),
            (c1 - gw / 2.0, c1 + r1 + 7.0 * d as f64 / 64.0 - gh * 0.9, gw, gh),
        );
        let mut glow = glow.gaussian_blur(((5.0 * d as f64 / 64.0).round_ties_even() as i32).max(1));
        glow.fill_mult([255, 255, 255, (255.0 * 0.48) as u8]);
        under.blit(&glow, 0, 0);
    }
    let mut face = Surf::new(n, n);
    pg::polygon(&mut face, gem_tone(&g, g.l - 6.0), &girdle, 0);
    let mut edges: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut facet = |face: &mut Surf, pts: Vec<(f64, f64)>, normal: f64, tilt: f64, i: f64| {
        let tone = gem_tone(&g, g.l + g.k * tilt * lit(normal) + g.alt * 1.3 * jitter(i));
        pg::polygon(face, tone, &pts, 0);
        edges.push(pts);
    };
    for j in 0..8 {
        let a = 22.5 + 45.0 * j as f64;
        let mid = a + 22.5;
        let jf = j as f64;
        facet(
            &mut face,
            vec![pt(a, rt), pt(a - 22.5, rs), pt(a, rr), pt(a + 22.5, rs)],
            a,
            0.9,
            jf,
        );
        facet(
            &mut face,
            vec![pt(mid, rs), pt(mid, rr), pt(a, rr)],
            mid - 11.25,
            1.1,
            8.0 + 2.0 * jf,
        );
        facet(
            &mut face,
            vec![pt(mid, rs), pt(mid, rr), pt(a + 45.0, rr)],
            mid + 11.25,
            1.1,
            9.0 + 2.0 * jf,
        );
        facet(
            &mut face,
            vec![pt(a, rt), pt(a + 45.0, rt), pt(mid, rs)],
            mid,
            0.5,
            24.0 + jf,
        );
    }
    let table: Vec<(f64, f64)> = (0..8).map(|j| pt(22.5 + 45.0 * j as f64, rt)).collect();
    pg::polygon(&mut face, gem_tone(&g, g.l), &table, 0);
    edges.push(table);
    // the facets' edges catch the light (drawn onto their own surface: pygame sets pixels, so shared edges
    // don't add up)
    let mut lines = Surf::new(n, n);
    let lw = ((0.55 * ss as f64) as i32).max(1);
    for pts in &edges {
        pg::polygon(&mut lines, [255, 255, 255, 41], pts, lw);
    }
    pg::polygon(&mut lines, [0, 0, 0, 115], &girdle, ss);
    if d >= 30 {
        // a glint on the upper left
        let (gx, gy) = pt(-135.0, rr * 0.62);
        let sz = (d as f64 * 0.09).min(4.5) * ss as f64;
        let star = [
            (gx, gy - sz),
            (gx + sz * 0.22, gy - sz * 0.22),
            (gx + sz, gy),
            (gx + sz * 0.22, gy + sz * 0.22),
            (gx, gy + sz),
            (gx - sz * 0.22, gy + sz * 0.22),
            (gx - sz, gy),
            (gx - sz * 0.22, gy - sz * 0.22),
        ];
        pg::polygon(&mut lines, [255, 255, 255, 230], &star, 0);
    }
    face.blit(&lines, 0, 0);
    under.blit(&face.smoothscale(n1, n1), 0, 0);
    (under.image(), m)
}

/// A round cut-gem key's body at rect (no icon): ruby or emerald.
pub fn gem_key(p: &Painter, rect: Rect, gem: Gem, pressed: bool) {
    gem_key_px(p, PxRect::of(rect, p.pixels_per_point()), gem, pressed);
}

/// `gem_key` on a rect in whole pixels: the stone `min(w, h)` across, centred as pygame centres it.
pub(crate) fn gem_key_px(p: &Painter, pr: PxRect, gem: Gem, pressed: bool) {
    let d = pr.w.min(pr.h);
    if d < 6 {
        return;
    }
    let key = key_of(("gem", d, gem, pressed, theme::generation()));
    let m = ((d as f64 * 0.24).ceil() as i32).max(4);
    let (tex, size) = texture(p.ctx(), Store::Small, key, || make_gem(d, gem, pressed).0);
    let (cx, cy) = (pr.x + pr.w / 2, pr.y + pr.h / 2);
    blit(
        p,
        tex,
        size,
        (cx - d / 2 - m) as f32,
        (cy - d / 2 - m) as f32,
        Color32::WHITE,
    );
}

/// The jewel lamp: a small cut ruby set in a sunken socket, like an amp's jewel pilot light. `lit` 0 is off (a
/// dark garnet, no light thrown), 1 on (the ruby, its light on the panel round it); in between fades from one to
/// the other, and above 1 (to 2) a halo swells round it (note by note waiting for you). `d` is the stone's
/// diameter (points); the socket is a little wider. Returns the socket's rect.
pub fn jewel_lamp(p: &Painter, center: Pos2, d: f32, lit: f32, hot: bool) -> Rect {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let dp = pxr(d, s).max(6);
    let rim = pxr(2.0, s).max(1);
    let (cx, cy) = (ipx(center.x, ppp), ipx(center.y, ppp));
    let sock = PxRect {
        x: cx - dp / 2 - rim,
        y: cy - dp / 2 - rim,
        w: dp + 2 * rim,
        h: dp + 2 * rim,
    };
    let lit = lit.clamp(0.0, 2.0);
    let on = lit.min(1.0);
    // the halo (in steps, so a handful of textures serve every frame of the swell)
    let swell = ((lit - 1.0).max(0.0) * 8.0).round() / 8.0;
    if on > 0.0 {
        let g = Gem::Ruby.spec().glow;
        let a = ((0.35 + 0.45 * swell) * on * 8.0).round() / 8.0;
        crate::paint::glow_px(
            p,
            PxRect {
                x: sock.x - (swell * rim as f32 * 2.0) as i32,
                y: sock.y - (swell * rim as f32 * 2.0) as i32,
                w: sock.w + 2 * (swell * rim as f32 * 2.0) as i32,
                h: sock.h + 2 * (swell * rim as f32 * 2.0) as i32,
            },
            g,
            7.0 + 5.0 * swell,
            Some(a),
            None,
        );
    }
    inset_px(p, sock, Some((sock.w / 2) as f64), 1.5, crate::paint::Fill::Base);
    let m = ((dp as f64 * 0.24).ceil() as i32).max(4);
    let (x, y) = ((cx - dp / 2 - m) as f32, (cy - dp / 2 - m) as f32);
    let stone = |gem: Gem, pressed: bool, alpha: f32| {
        if alpha <= 0.0 {
            return;
        }
        let key = key_of(("gem", dp, gem, pressed, theme::generation()));
        let (tex, size) = texture(p.ctx(), Store::Small, key, || make_gem(dp, gem, pressed).0);
        blit(
            p,
            tex,
            size,
            x,
            y,
            Color32::from_white_alpha((alpha * 255.0).round() as u8),
        );
    };
    stone(Gem::Garnet, true, 1.0 - on);
    stone(Gem::Ruby, false, on);
    let ctr = pt(cx, ppp);
    let cty = pt(cy, ppp);
    let r = d / 2.0;
    if on > 0.0 {
        // lit from behind: a warm core showing through the stone
        let core = Pos2::new(ctr, cty + r * 0.1);
        let warm = |a: f32| Color32::from_rgba_unmultiplied(255, 110, 140, (a * on) as u8);
        p.circle_filled(core, r * 0.6, warm(55.0));
        p.circle_filled(
            core,
            r * 0.32,
            Color32::from_rgba_unmultiplied(255, 205, 215, (80.0 * on) as u8),
        );
    }
    // a glint on the upper left (the big stones draw theirs into the cut)
    p.circle_filled(
        Pos2::new(ctr - r * 0.32, cty - r * 0.36),
        (r * 0.2).max(0.8),
        Color32::from_white_alpha((90.0 + 110.0 * on) as u8),
    );
    let rect = Rect::from_min_size(
        Pos2::new(pt(sock.x, ppp), pt(sock.y, ppp)),
        egui::vec2(pt(sock.w, ppp), pt(sock.h, ppp)),
    );
    if hot {
        p.circle_stroke(
            rect.center(),
            rect.width() / 2.0 + 1.5,
            egui::Stroke::new(1.0, t().accent.gamma_multiply(0.7)),
        );
    }
    rect
}

/// The ink of a label on a key from `key_face`: ivory's dark ink (or text on the accent), or on the dark themes'
/// the quiet key's text colour (or the glass key's amber on the main action).
pub fn key_ink(accent: bool) -> Color32 {
    match (crate::glass::on(), accent) {
        (true, false) => t().text,
        (true, true) => crate::glass::ink(true),
        (false, true) => t().on_accent,
        (false, false) => IVORY_INK,
    }
}

/// Text colours on keys.
pub fn ink_on_ivory(danger: bool, accent: bool) -> Color32 {
    if danger {
        DANGER_ON_IVORY
    } else if accent {
        t().accent_on_ivory
    } else {
        IVORY_INK
    }
}

/// A label on a key; on a dark material with a soft shadow under it (`neu._label`: the black text at `shadow`
/// alpha, `max(1, px(1))` lower).
pub fn label(
    p: &Painter,
    label: &str,
    pos: Pos2,
    anchor: Align2,
    st: Style,
    max_w: Option<f32>,
    shadow: f32,
) -> Rect {
    if shadow > 0.0 {
        let ppp = p.pixels_per_point();
        let txt = match max_w {
            Some(m) => text::fit(p.ctx(), label, st, m),
            None => label.to_string(),
        };
        let a = (shadow.clamp(0.0, 1.0) * 255.0) as u8;
        let (x, y) = (ipx(pos.x, ppp), ipx(pos.y, ppp) + pxr(1.0, sc(ppp)).max(1));
        text::draw(
            p,
            &txt,
            Pos2::new(pt(x, ppp), pt(y, ppp)),
            anchor,
            Style {
                color: Color32::from_black_alpha(a),
                ..st
            },
            None,
        );
        return text::draw(p, &txt, pos, anchor, st, None);
    }
    text::draw(p, label, pos, anchor, st, max_w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parts pygame draws are compared with the Python app's own surfaces: QRS_NEU_DUMPS=<dir> of raw dumps
    /// (u32 w, u32 h, RGBA) of rsplay.neu's `_make_gem` and `_make_thumb` at scale 36/35 (skipped without it).
    #[test]
    fn neu_parity_keys() {
        let Ok(dir) = std::env::var("QRS_NEU_DUMPS") else {
            return;
        };
        let _lock = crate::raster::THEME_LOCK.lock();
        let s = 1440.0f64 / 1400.0;
        let mut bad = Vec::new();
        let mut n = 0;
        for th in theme::ThemeName::ALL {
            theme::set_theme(th);
            for (gem, gname) in [(Gem::Ruby, "ruby"), (Gem::Emerald, "emerald")] {
                for d in [29, 30, 31, 57, 58, 70] {
                    for pressed in [false, true] {
                        let name = format!("{}_gem_{gname}_{d}_{}", th.key(), pressed as u8);
                        n += 1;
                        let got = make_gem(d, gem, pressed).0;
                        if let Some(e) = crate::raster::tests_cmp(&dir, &name, &got) {
                            bad.push(e);
                        }
                    }
                }
            }
            for d in [20, 24, 26, 27, 28] {
                let name = format!("{}_thumb_{d}", th.key());
                n += 1;
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &make_thumb(d, s)) {
                    bad.push(e);
                }
            }
            for (w, h, rr, mat, mname, pressed, seed) in [
                (122, 45, 14, Mat::Tortoise, "tortoise", false, 0),
                (144, 45, 14, Mat::Abalone, "abalone", false, 3),
                (134, 45, 14, Mat::Aluminium, "aluminium", false, 1),
                (58, 58, 29, Mat::Tortoise, "tortoise", true, 5),
                (196, 41, 13, Mat::Tortoise, "tortoise", false, 2),
                (16, 60, 8, Mat::Aluminium, "aluminium", false, 5),
            ] {
                let name = format!("{}_mat_{mname}_{w}_{h}_{rr}_{}_{seed}", th.key(), pressed as u8);
                n += 1;
                let got = make_mat_key(w, h, rr, mat, pressed, seed, s);
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &got) {
                    bad.push(e);
                }
            }
            for (w, h, rr, kind, kname) in [
                (30, 18, 6, CapOn::Ivory, "ivory"),
                (41, 18, 6, CapOn::Accent, "accent"),
                (24, 16, 6, CapOn::Dark, "dark"),
                (24, 16, 6, CapOn::Metal, "metal"),
            ] {
                let name = format!("{}_engr_{kname}_{w}_{h}", th.key());
                n += 1;
                if let Some(e) = crate::raster::tests_cmp(&dir, &name, &make_engraved(w, h, rr, kind)) {
                    bad.push(e);
                }
            }
            for (w, h, rr, accent, pressed) in [
                (122, 45, 14, false, false),
                (144, 45, 14, true, false),
                (58, 58, 29, false, true),
                (60, 33, 10, false, false),
            ] {
                let name = format!("{}_key_{w}_{h}_{rr}_{}_{}", th.key(), accent as u8, pressed as u8);
                n += 1;
                if let Some(e) =
                    crate::raster::tests_cmp(&dir, &name, &make_key(w, h, rr, accent, pressed, s))
                {
                    bad.push(e);
                }
            }
        }
        theme::set_theme(theme::ThemeName::Midnight);
        eprintln!("neu keys parity: {n} cases");
        assert!(bad.is_empty(), "{} of {n} differ:\n{}", bad.len(), bad.join("\n"));
    }
}
