//! One frame of the highway, back to front in RS's layer order (research/spec/00 C3): background, floor
//! (unlit lane lines, the anchor zones as lit humps with cyan lines, beat and measure lines, zone-start bars with
//! their floor numbers), the board and inlays, then notes and chords far to near (notes.rs), then the strings over
//! them, the fret posts, the hit flashes, the neck's numbers and the words.

use eframe::egui::{Align2, Color32, Painter};
use qrs_analysis::Judgement;

use super::Cache;
use super::gfx::*;
use super::notes;
use super::skin;
use super::view::{FLOOR, V3, View, fret_w, slot, v3, wire};

const INLAYS: [i32; 10] = [3, 5, 7, 9, 12, 15, 17, 19, 21, 24];

// 05 §5 palette
const LANE_UNLIT: Color32 = rgb(45, 100, 125);
const LANE_LIT: Color32 = rgb(65, 240, 250);
const HUMP: [(f32, Color32); 4] = [
    (0.0, rgb(0, 10, 25)),
    (0.08, rgb(1, 16, 43)),
    (0.23, rgb(6, 37, 70)),
    (0.40, rgb(17, 56, 90)),
];
const BEAT: Color32 = rgb(90, 150, 190);
const BAR_TOP: Color32 = rgb(117, 200, 219);
const BAR_BODY: Color32 = rgb(71, 150, 169);
const FLOOR_NUM_IN: Color32 = rgb(142, 232, 252);
const FLOOR_NUM_NEAR: Color32 = rgb(12, 188, 190);
const FLOOR_NUM_FAR: Color32 = rgb(21, 122, 119);
const NECK_NUM_IN: Color32 = rgb(220, 168, 56);
const NECK_NUM_OUT: Color32 = rgb(27, 55, 51);
const INLAY_IN: Color32 = rgb(145, 120, 65);
const INLAY_OUT: Color32 = rgb(44, 66, 62);
const POST_IN: Color32 = rgb(81, 67, 51);
const POST_OUT: Color32 = rgb(37, 31, 19);
/// The game's near fades (00 C1, in its units: `View::units`): anchor zones and beat lines over the last 50.
const ZONE_NEAR: f32 = 50.0;

/// The skin's images and lights for this frame (skin.rs).
pub(super) struct Skin<'a> {
    pub tex: &'a skin::Tex,
    /// The amp's glow (0..2).
    pub glow: f32,
    pub clock: f64,
    /// Live, Still or Plain (the setting).
    pub backdrop: skin::Backdrop,
}

pub(super) fn draw(p: &Painter, v: &View, c: &Cache, marks: &[Option<(Judgement, f32)>], sk: &Skin) {
    // QUICKRS_HW_PLAIN: no skin at all (the game's colours, for side-by-sides); the Plain setting keeps the notes'
    // material and drops only the stage
    let bare = skin::plain();
    let backdrop = if bare { skin::Backdrop::Plain } else { sk.backdrop };
    let live = backdrop == skin::Backdrop::Live;
    let pen = Pen {
        p,
        v,
        pal: if bare { &GAME } else { c.look.pal() },
        gems: (!bare).then(|| sk.tex.gems(c.look.material)),
    };
    skin::background(p, v.well, sk.tex, &c.look, backdrop);
    // the frets whose wires show at the strings (plus one each side)
    let on = |f: i32| {
        v.pt(v3(wire(f as f32), 0.0, 0.0)).is_some_and(|q| {
            q.x > v.well.min.x - v.well.width() * 0.3 && q.x < v.well.max.x + v.well.width() * 0.3
        })
    };
    let frets: Vec<i32> = (0..=24).filter(|&f| on(f)).collect();
    let win = c.window_now(v.now);
    floor(&pen, c, &frets);
    if backdrop != skin::Backdrop::Plain {
        skin::amp(p, v, sk.tex, sk.glow, c.look.dim(), live);
    }
    board(&pen, &frets, win);
    let out = notes::draw(&pen, c, marks);
    strings(&pen, c);
    posts(&pen, &frets, win);
    notes::flashes(&pen, &out);
    neck_numbers(&pen, &frets, win);
    notes::overlays(&pen, c, &out);
    if live {
        skin::embers(p, v.well, sk.clock, (c.look.heat - 2.0).clamp(0.0, 1.0));
    }
}

/// Depth samples from z0 to z1, denser near.
fn depths(z0: f32, z1: f32, n: usize) -> Vec<f32> {
    (0..=n)
        .map(|i| {
            let u = i as f32 / n as f32;
            z0 + (z1 - z0) * u * u.sqrt()
        })
        .collect()
}

fn floor(pen: &Pen, c: &Cache, frets: &[i32]) {
    let v = pen.v;
    let far = v.far();
    let near_fade = |z: f32| (z / v.units(ZONE_NEAR)).clamp(0.0, 1.0);
    // unlit lane lines: every fret boundary, the whole depth
    let zs = depths(0.0, far, 10);
    for &f in frets {
        let x = wire(f as f32);
        let w = 0.025 * fret_w(f.max(1) as f32);
        for k in 0..zs.len() - 1 {
            let (a, b) = (zs[k], zs[k + 1]);
            pen.line(
                v3(x, FLOOR, a),
                v3(x, FLOOR, b),
                w,
                0.0,
                alpha(LANE_UNLIT, v.fade(a)),
                alpha(LANE_UNLIT, v.fade(b)),
                1.0,
            );
        }
    }
    // the anchor zones: one segment per anchor, scrolling in with the notes
    let t_far = v.now + v.look();
    for a in c.anchors.iter().filter(|a| a.1 > v.now - 1.0 && a.0 < t_far) {
        let (z0, z1) = (v.z(a.0).max(0.0), v.z(a.1).min(far));
        if z1 <= z0 {
            continue;
        }
        let zs = depths(z0, z1, ((z1 - z0) / 12.0).ceil().clamp(2.0, 16.0) as usize);
        // humps across each lane
        let mut xs: Vec<(f32, Color32)> = Vec::new();
        for f in a.2..a.2 + a.3 {
            let (l, r) = (wire((f - 1) as f32), wire(f as f32));
            let stops = HUMP
                .iter()
                .copied()
                .chain(HUMP.iter().rev().map(|&(u, c)| (1.0 - u, c)));
            for (u, col) in stops {
                if f > a.2 && u == 0.0 {
                    continue;
                }
                xs.push((l + (r - l) * u, col));
            }
        }
        let rows: Vec<Vec<V3>> = zs
            .iter()
            .map(|&z| xs.iter().map(|&(x, _)| v3(x, FLOOR, z)).collect())
            .collect();
        let cols: Vec<Vec<Color32>> = zs
            .iter()
            .map(|&z| {
                let k = 0.8 * v.fade(z) * near_fade(z);
                xs.iter().map(|&(_, col)| alpha(col, k)).collect()
            })
            .collect();
        pen.grid(&rows, &cols);
        // lit lane lines on its boundaries (additive)
        for f in (a.2 - 1)..=(a.2 - 1 + a.3) {
            let x = wire(f as f32);
            let w = 0.04 * fret_w(f.max(1) as f32);
            for k in 0..zs.len() - 1 {
                let (za, zb) = (zs[k], zs[k + 1]);
                pen.line(
                    v3(x, FLOOR, za),
                    v3(x, FLOOR, zb),
                    w,
                    w,
                    add(LANE_LIT, v.fade(za) * near_fade(za).max(0.25)),
                    add(LANE_LIT, v.fade(zb) * near_fade(zb).max(0.25)),
                    1.0,
                );
            }
        }
        // the zone-start bar and its numbers
        let zb = v.z(a.0);
        if zb > -8.0 && zb < far {
            // (they pass under the neck in the game; our board is lighter, so they fade before reaching it)
            let fa = v.fade(zb) * smooth((zb - 4.0) / 12.0);
            let (l, r) = (wire((a.2 - 1) as f32), wire((a.2 - 1 + a.3) as f32));
            let d = 0.06 * fret_w(a.2 as f32);
            if zb > 0.0 {
                pen.poly(
                    &[
                        v3(l, FLOOR + 0.12, zb),
                        v3(r, FLOOR + 0.12, zb),
                        v3(r, FLOOR, zb),
                        v3(l, FLOOR, zb),
                    ],
                    alpha(BAR_BODY, fa),
                );
                pen.poly(
                    &[
                        v3(l, FLOOR + 0.12, zb),
                        v3(r, FLOOR + 0.12, zb),
                        v3(r, FLOOR + 0.12, zb + d * 4.0),
                        v3(l, FLOOR + 0.12, zb + d * 4.0),
                    ],
                    alpha(BAR_TOP, fa),
                );
            }
            for f in INLAYS {
                if !frets.contains(&f) || (f - (a.2 + a.3 / 2)).abs() > 7 {
                    continue;
                }
                let inside = f >= a.2 && f < a.2 + a.3;
                let dist = if f < a.2 { a.2 - f } else { f - (a.2 + a.3 - 1) };
                let col = if inside {
                    FLOOR_NUM_IN
                } else if dist <= 3 {
                    FLOOR_NUM_NEAR
                } else {
                    FLOOR_NUM_FAR
                };
                pen.text(
                    &f.to_string(),
                    v3(slot(f), FLOOR + 0.1, zb.max(-8.0)),
                    1.6,
                    Align2::CENTER_BOTTOM,
                    800,
                    alpha(col, fa),
                );
            }
        }
    }
    // beat and measure lines across the zone's lanes
    let bi = c.beats.partition_point(|b| b.0 < v.now);
    for b in c.beats[bi..].iter().take_while(|b| b.0 <= t_far) {
        let Some((wf, ww)) = super::window_at(c, b.0) else {
            continue;
        };
        let z = v.z(b.0);
        let k = v.fade(z) * near_fade(z);
        let (l, r) = (wire((wf - 1) as f32), wire((wf - 1 + ww) as f32));
        let fw = fret_w(wf as f32);
        let (th, a) = if b.1 { (0.03 * fw, 0.6) } else { (0.02 * fw, 0.25) };
        let col = alpha(BEAT, a * k);
        pen.line(v3(l, FLOOR, z), v3(r, FLOOR, z), th * 3.0, 0.0, col, col, 1.0);
    }
}

/// The board behind the strings and its inlays (gold in the hand's window, teal outside).
fn board(pen: &Pen, frets: &[i32], win: Option<(i32, i32)>) {
    let (l, r) = (wire(-6.0), wire(44.0));
    pen.grad(
        [
            v3(l, 5.6, 0.0),
            v3(r, 5.6, 0.0),
            v3(r, -0.6, 0.0),
            v3(l, -0.6, 0.0),
        ],
        [
            alpha(rgb(30, 24, 20), 0.35),
            alpha(rgb(30, 24, 20), 0.35),
            alpha(rgb(30, 24, 20), 0.55),
            alpha(rgb(30, 24, 20), 0.55),
        ],
    );
    let in_win = |f: i32| win.is_some_and(|(a, w)| f >= a && f < a + w);
    for &f in frets {
        if !INLAYS.contains(&f) {
            continue;
        }
        let col = if in_win(f) { INLAY_IN } else { INLAY_OUT };
        let ys: &[f32] = if f % 12 == 0 { &[1.5, 3.5] } else { &[2.5] };
        for &y in ys {
            let at = v3(slot(f), y, 0.0);
            if let Some(q) = pen.v.pt(at) {
                let rad = 0.35 * pen.v.scale(at);
                pen.p.circle_filled(q, rad * 1.25, alpha(col, 0.25));
                pen.p.circle_filled(q, rad, alpha(col, 0.7));
            }
        }
    }
}

/// A string's state: 1 lit (it carries the next note, is sounding or just was), 0.8 active (used in the phrase),
/// 0.3 inactive (04 §1.2, §3).
fn string_levels(c: &Cache, now: f64) -> [(f32, bool); 6] {
    let mut out = [(0.3f32, false); 6];
    // used in the current phrase
    let k = c.phrase_masks.partition_point(|p| p.0 <= now + 1e-3);
    let mask = if k > 0 { c.phrase_masks[k - 1].1 } else { 0x3f };
    for (s, o) in out.iter_mut().enumerate() {
        if mask & (1 << s) != 0 {
            o.0 = 0.8;
        }
    }
    let i0 = c.notes.partition_point(|n| n.t < now - 12.0);
    let mut next: Option<f64> = None;
    for n in &c.notes[i0..] {
        let s = n.string.clamp(0, 5) as usize;
        let end = n.t + n.sus;
        if n.t <= now && now <= end + 0.28 {
            out[s] = (1.0, true);
        }
        if n.t > now - 0.03 {
            match next {
                None => next = Some(n.t),
                Some(t) if n.t > t + 3e-3 => break,
                _ => {}
            }
            out[s] = (1.0, true);
        }
    }
    out
}

fn strings(pen: &Pen, c: &Cache) {
    let v = pen.v;
    let levels = string_levels(c, v.now);
    let xs: Vec<f32> = (0..=60).map(|i| wire(-6.0 + 50.0 * i as f32 / 60.0)).collect();
    for s in 0..v.n_str as i32 {
        let k = si(s);
        let (level, lit) = levels[k];
        let core = pen.pal.str_lit[k];
        // (a touch wider than the 4K deconvolved cores and halos: the footage at 1080-1440 reads thicker)
        let (sg, tau) = (STR_SIGMA[k] * 1.5, STR_TAU[k] * 1.2);
        let y = v.sy(s);
        let pts: Vec<V3> = xs.iter().map(|&x| v3(x, y, 0.0)).collect();
        let halo = if lit { 1.0 } else { 0.0 };
        let i = |u: f32| {
            let g = (-0.5 * (u / sg).powi(2)).exp();
            g * level * (1.0 - 0.55 * halo) + halo * 0.55 * (-u / tau).exp()
        };
        let mut us = vec![0.0, sg, 2.0 * sg, 3.0 * sg];
        if lit {
            us.extend([tau, 2.0 * tau, 3.0 * tau]);
            us.sort_by(f32::total_cmp);
        }
        let stops: Vec<(f32, Color32)> = us
            .iter()
            .enumerate()
            .map(|(j, &u)| {
                let last = j == us.len() - 1;
                (
                    u,
                    if last {
                        Color32::TRANSPARENT
                    } else {
                        add(core, i(u))
                    },
                )
            })
            .collect();
        pen.band(&pts, &stops);
        if lit {
            // the bloom around a lit string (05 §4.2: bright emissives bloom ≈1-2% of the screen)
            pen.band(
                &pts,
                &[
                    (0.0, add(core, 0.14)),
                    (0.25, add(core, 0.07)),
                    (0.6, Color32::TRANSPARENT),
                ],
            );
        }
    }
}

fn posts(pen: &Pen, frets: &[i32], win: Option<(i32, i32)>) {
    for &f in frets {
        let inside = win.is_some_and(|(a, w)| f >= a - 1 && f <= a - 1 + w);
        let col = if inside { POST_IN } else { POST_OUT };
        let x = wire(f as f32);
        pen.line(v3(x, -0.45, 0.0), v3(x, 5.45, 0.0), 0.07, 0.02, col, col, 1.0);
        // a warm highlight down the posts of the hand's window, and a small pointed cap
        if inside {
            let hl = alpha(rgb(200, 170, 120), 0.7);
            pen.line(
                v3(x - 0.012, -0.4, -0.01),
                v3(x - 0.012, 5.4, -0.01),
                0.025,
                0.0,
                hl,
                hl,
                0.7,
            );
        }
        pen.poly(
            &[v3(x - 0.05, 5.45, 0.0), v3(x + 0.05, 5.45, 0.0), v3(x, 5.65, 0.0)],
            col,
        );
    }
}

fn neck_numbers(pen: &Pen, frets: &[i32], win: Option<(i32, i32)>) {
    for &f in frets {
        if f < 1 {
            continue;
        }
        let inside = win.is_some_and(|(a, w)| f >= a && f < a + w);
        if !inside && !INLAYS.contains(&f) {
            continue;
        }
        let at = v3(slot(f), -0.75, 0.0);
        if inside {
            pen.text(&f.to_string(), at, 1.35, Align2::CENTER_TOP, 800, NECK_NUM_IN);
        } else {
            pen.text(&f.to_string(), at, 1.35, Align2::CENTER_TOP, 800, NECK_NUM_OUT);
        }
    }
}
