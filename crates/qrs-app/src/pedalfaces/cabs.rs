//! The cabs' faces (PLAN.md P21.2): each drawn as the real cabinet, from the owner-approved mockups: a blackface 1x10,
//! a tweed 1x12, an open 2x12 (blackface, or the AC30's brown diamond behind a Dartford 30), a Marshall-style slant
//! 4x12, the Ampeg fridge, a blue flip-top 1x15, and (P37.6c) the bass cabs at their real speaker counts: a 4x10 and a
//! ported 2x10 behind perforated metal with a horn, a tall vintage 4x15 in black cloth; (P41.2) a tweed 4x10 combo. The
//! speakers show faintly through the cloth; the mic stands in front
//! of one, slid from the cone's edge to its cap by Mic, pulled back by Distance, its silhouette set by Mic type.
//!
//! The cabinet is cached as its own texture (keyed by its look, which for the 2x12 follows the amp before it on the
//! board); the mic and the controls are drawn live.

use std::cell::RefCell;
use std::collections::HashMap;

use eframe::egui::{Align2, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use qrs_audio::board::{Block, Face};
use qrs_ui::knobs::{self, KnobStyle, Scale};
use qrs_ui::paint::{self, Store};
use qrs_ui::raster::Image;
use qrs_ui::theme::t;

use super::amps::{self, Amp, Cloth, Cv, c32, ca, cloth, ghost, grille, logo, tolex};
use super::{Detail, Hot, Layout, Lettering};

/// How the 2x12 is dressed (the others have one look each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Look {
    Blackface,
    Dartford,
}

thread_local! {
    /// Each 2x12's look by block, as last seen on the board.
    static LOOKS: RefCell<HashMap<u32, Look>> = RefCell::new(HashMap::new());
}

fn look_of(b: &Block) -> Look {
    LOOKS.with(|m| m.borrow().get(&b.uid).copied().unwrap_or(Look::Blackface))
}

/// The cabinet's proportions (width / height) and its size on the board.
fn size(id: &str) -> Vec2 {
    match id {
        "cab_1x10" => vec2(150.0, 132.0),
        "cab_1x12" => vec2(162.0, 142.0),
        "cab_2x12" => vec2(236.0, 186.0),
        "cab_4x12" => vec2(212.0, 210.0),
        "cab_8x10" => vec2(112.0, 212.0),
        "cab_4x10" => vec2(196.0, 204.0),
        "cab_2x10" => vec2(196.0, 140.0),
        "cab_4x15" => vec2(176.0, 236.0),
        "cab_4x10_open" => vec2(200.0, 206.0),
        _ => vec2(150.0, 178.0),
    }
}

/// The engineer's strip beside the cab in Full (P37.13): its width at a face 200 points high (it scales with the face).
const STRIP_W: f32 = 236.0;
const UNIT_H: f32 = 200.0;

/// The open unit's size: the cabinet at its proportions, 200 points high, and the strip beside it.
pub(super) fn unit_size(b: &Block) -> Vec2 {
    let s = size(b.def().id);
    vec2(UNIT_H * s.x / s.y + 6.0 + STRIP_W, UNIT_H)
}

/// Where the cabinet art goes in the face (the whole face on the board; at the left, at its proportions, in Full,
/// leaving the strip its width).
fn art_rect(id: &str, r: Rect, detail: Detail) -> Rect {
    if detail == Detail::Board {
        return r;
    }
    let s = size(id);
    let k = r.height() / UNIT_H;
    let h = r.height();
    let w = (h * s.x / s.y).min(r.width() - STRIP_W * k - 6.0).max(20.0);
    let h = w * s.y / s.x;
    Rect::from_min_size(pos2(r.min.x + 6.0, r.center().y - h * 0.5), vec2(w - 6.0, h))
}

/// The cabinet in the art rect (handle room above, feet or casters below).
fn cab_rect(id: &str, a: Rect) -> Rect {
    let h = a.height();
    let top = if id == "cab_8x10" { 0.02 } else { 0.05 } * h;
    let bot = match id {
        "cab_4x12" | "cab_8x10" | "cab_4x10" | "cab_4x15" => 0.05,
        _ => 0.03,
    } * h;
    Rect::from_min_max(
        pos2(a.min.x, a.min.y + top.clamp(2.0, 10.0)),
        pos2(a.max.x, a.max.y - bot.clamp(2.0, 10.0)),
    )
}

/// The grille(s) and speakers: (grilles, speakers (centre, radius), the one the mic is on).
fn parts(id: &str, look: Look, c: Rect) -> (Vec<Rect>, Vec<(Pos2, f32)>, usize) {
    let (w, h) = (c.width(), c.height());
    let at = |fx: f32, fy: f32, fw: f32, fh: f32| {
        Rect::from_min_size(pos2(c.min.x + w * fx, c.min.y + h * fy), vec2(w * fw, h * fh))
    };
    match id {
        "cab_1x10" => {
            let g = at(0.065, 0.075, 0.87, 0.85);
            (vec![g], vec![(g.center() + vec2(0.0, h * 0.02), w * 0.25)], 0)
        }
        "cab_1x12" => {
            let g = at(0.09, 0.19, 0.82, 0.7);
            (vec![g], vec![(g.center(), w * 0.255)], 0)
        }
        "cab_2x12" => {
            let g = match look {
                Look::Blackface => at(0.05, 0.065, 0.9, 0.87),
                Look::Dartford => at(0.045, 0.25, 0.91, 0.69),
            };
            let r = w * 0.2;
            (
                vec![g],
                vec![
                    (pos2(g.min.x + g.width() * 0.27, g.center().y), r),
                    (pos2(g.min.x + g.width() * 0.73, g.center().y), r),
                ],
                0,
            )
        }
        "cab_4x12" => {
            let g = at(0.06, 0.06, 0.88, 0.88);
            let r = w * 0.19;
            let s = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                .iter()
                .map(|&(a, b)| (pos2(g.min.x + g.width() * a, g.min.y + g.height() * b), r))
                .collect();
            (vec![g], s, 3)
        }
        "cab_8x10" => {
            let b = 0.14;
            let gap = 0.03;
            let half = (1.0 - 2.0 * b * w / h - gap) / 2.0;
            let g1 = at(b, b * w / h, 1.0 - 2.0 * b, half);
            let g2 = at(b, b * w / h + half + gap, 1.0 - 2.0 * b, half);
            let r = w * 0.19;
            let mut s = Vec::new();
            for g in [g1, g2] {
                for (a, bb) in [(0.27, 0.25), (0.73, 0.25), (0.27, 0.75), (0.73, 0.75)] {
                    s.push((pos2(g.min.x + g.width() * a, g.min.y + g.height() * bb), r));
                }
            }
            (vec![g1, g2], s, 4)
        }
        "cab_4x10" | "cab_4x15" => {
            let g = if id == "cab_4x10" {
                at(0.07, 0.07, 0.86, 0.86)
            } else {
                at(0.08, 0.13, 0.84, 0.81)
            };
            let r = w * if id == "cab_4x10" { 0.175 } else { 0.195 };
            let s = [(0.27, 0.25), (0.73, 0.25), (0.27, 0.75), (0.73, 0.75)]
                .iter()
                .map(|&(a, b)| (pos2(g.min.x + g.width() * a, g.min.y + g.height() * b), r))
                .collect();
            (vec![g], s, 3)
        }
        // (P41.2: the tweed 4x10 combo, four tens under its name plate)
        "cab_4x10_open" => {
            let g = at(0.07, 0.18, 0.86, 0.76);
            let r = w * 0.17;
            let s = [(0.27, 0.25), (0.73, 0.25), (0.27, 0.75), (0.73, 0.75)]
                .iter()
                .map(|&(a, b)| (pos2(g.min.x + g.width() * a, g.min.y + g.height() * b), r))
                .collect();
            (vec![g], s, 3)
        }
        "cab_2x10" => {
            let g = at(0.07, 0.08, 0.86, 0.68);
            let r = w * 0.165;
            (
                vec![g],
                vec![
                    (pos2(g.min.x + g.width() * 0.25, g.center().y), r),
                    (pos2(g.min.x + g.width() * 0.75, g.center().y), r),
                ],
                0,
            )
        }
        _ => {
            let g = at(0.13, 0.31, 0.74, 0.6);
            (vec![g], vec![(g.center(), w * 0.31)], 0)
        }
    }
}

/// The cabinet's art (cached by look and size).
fn art(id: &str, look: Look, a: Rect, ppp: f32) -> Image {
    let Some(mut cv) = Cv::new(a, ppp) else {
        return Image::new(1, 1);
    };
    let c = cab_rect(id, a);
    let (w, h) = (c.width(), c.height());
    let (gs, sps, _) = parts(id, look, c);
    let k = (w / 220.0).clamp(0.6, 1.3);
    let top_room = c.min.y - a.min.y;
    let rad = match id {
        "cab_1x12" | "cab_4x10_open" => w * 0.075,
        "cab_1x15" => w * 0.045,
        _ => w * 0.035,
    };
    // the handle and the feet
    match id {
        "cab_1x12" | "cab_4x10_open" => cv.strap(c.center().x, a.min.y, w * 0.3, top_room * 0.95, true),
        "cab_8x10" => cv.rr(
            Rect::from_center_size(pos2(c.center().x, c.min.y), vec2(w * 0.3, top_room * 1.6)),
            2.0,
            0x0d0d0e,
            1.0,
        ),
        "cab_4x12" | "cab_1x15" | "cab_4x10" | "cab_4x15" => {}
        "cab_2x12" if look == Look::Dartford => {
            cv.strap(c.center().x, a.min.y, w * 0.5, top_room * 0.95, false)
        }
        _ => cv.strap(c.center().x, a.min.y, w * 0.28, top_room * 0.95, false),
    }
    match id {
        "cab_4x12" | "cab_8x10" | "cab_4x10" | "cab_4x15" => {
            for x in [c.min.x + w * 0.16, c.max.x - w * 0.16] {
                let cy = (c.max.y + a.max.y) * 0.5;
                let cr = (a.max.y - c.max.y) * 0.5;
                cv.rr(
                    Rect::from_center_size(pos2(x, c.max.y), vec2(cr * 1.6, cr)),
                    0.5,
                    0x8a9096,
                    1.0,
                );
                cv.disc(pos2(x, cy), cr, 0x0c0c0c, 1.0);
                cv.ring(pos2(x, cy), cr, 0.6, 0x333333, 1.0);
                cv.ball(pos2(x, cy), cr * 0.35, &[(0.0, 0xffffff), (1.0, 0x53585e)]);
            }
        }
        _ => {
            let fh = (a.max.y - c.max.y).max(2.0);
            for x in [c.min.x + w * 0.06, c.max.x - w * 0.06 - w * 0.12] {
                let q = Rect::from_min_size(pos2(x, c.max.y - 1.0), vec2(w * 0.12, fh));
                if id == "cab_1x10" || (id == "cab_2x12" && look == Look::Blackface) {
                    cv.chrome(q, fh * 0.5);
                } else {
                    cv.rr(q, 1.5, 0x0a0a0a, 1.0);
                }
            }
        }
    }
    // the covering
    match (id, look) {
        ("cab_1x12", _) | ("cab_4x10_open", _) => cv.pattern(c, rad, |x, y| {
            let d = cloth(Cloth::Tweed, x, y, 1.0);
            let e = (x.min(w - x).min(y).min(h - y) / (w.min(h) * 0.12)).clamp(0.0, 1.0);
            let burn = (1.0 - e).powf(1.5) * 0.55;
            let lit = 0.08 * (1.0 - y / h * 3.0).max(0.0);
            let f = |v: f32, kk: f32| v * (1.0 - burn * kk) + lit;
            [f(d[0], 0.9), f(d[1], 1.05), f(d[2], 1.25), 1.0]
        }),
        ("cab_2x12", Look::Dartford) => cv.pattern(c, rad, |x, y| {
            let d = cloth(Cloth::Basket, x, y, k);
            let s = 0.05 * (1.0 - y / h * 4.0).max(0.0) - 0.1 * ((y / h - 0.7) * 3.0).max(0.0);
            [d[0] + s, d[1] + s, d[2] + s, 1.0]
        }),
        ("cab_1x15", _) => cv.pattern(c, rad, |x, y| {
            let d = cloth(Cloth::BlueCheck, x, y, 1.0);
            let s = 0.05 * (1.0 - y / h * 4.0).max(0.0) - 0.12 * ((y / h - 0.7) * 3.0).max(0.0);
            [d[0] + s, d[1] + s, d[2] + s, 1.0]
        }),
        ("cab_4x12", _) | ("cab_4x15", _) => cv.pattern(c, rad, tolex(0x121212, 1.4, h)),
        ("cab_4x10", _) => cv.pattern(c, rad, tolex(0x202022, 0.6, h)),
        _ => cv.pattern(c, rad, tolex(0x141414, 0.9, h)),
    }
    // the cloth and the speakers behind it
    let g = gs[0];
    match (id, look) {
        ("cab_1x10", _) | ("cab_2x12", Look::Blackface) => {
            grille(&mut cv, g, 2.5, Cloth::SilverTurq, k);
            for &(sc, sr) in &sps {
                ghost(&mut cv, sc, sr, false, id == "cab_2x12");
            }
        }
        ("cab_1x12", _) => {
            grille(&mut cv, g, w * 0.025, Cloth::TweedGrille, k);
            ghost(&mut cv, sps[0].0, sps[0].1, false, false);
            let q = linen_plate(c);
            cv.shadow(q.translate(vec2(0.0, 1.5)), q.height() * 0.5, 1.5, 0.5);
            cv.rr(q, q.height() * 0.5, 0x5a1a1e, 1.0);
            cv.rr_stroke(q, q.height() * 0.5, 0.8, 0xc9a45c, 1.0);
        }
        ("cab_4x10_open", _) => {
            grille(&mut cv, g, w * 0.025, Cloth::TweedGrille, k);
            for &(sc, sr) in &sps {
                ghost(&mut cv, sc, sr, false, false);
            }
            let q = linen_plate(c);
            cv.shadow(q.translate(vec2(0.0, 1.5)), q.height() * 0.5, 1.5, 0.5);
            cv.rr(q, q.height() * 0.5, 0x5a1a1e, 1.0);
            cv.rr_stroke(q, q.height() * 0.5, 0.8, 0xc9a45c, 1.0);
        }
        ("cab_2x12", Look::Dartford) => {
            cv.rr_stroke(g.expand(1.5), 2.5, 2.0, 0xc8a867, 1.0);
            grille(&mut cv, g, 2.0, Cloth::Diamond, k);
            for &(sc, sr) in &sps {
                ghost(&mut cv, sc, sr, false, false);
            }
            let by = c.min.y + h * 0.22;
            cv.rr_v(
                Rect::from_min_max(pos2(c.min.x + 3.0, by - 1.3), pos2(c.max.x - 3.0, by + 1.3)),
                1.0,
                &[(0.0, 0xf0d898, 1.0), (0.5, 0xc8a867, 1.0), (1.0, 0x7d6634, 1.0)],
            );
            let s = (w * 0.07).clamp(6.0, 18.0);
            let tk = (s * 0.28).max(2.0);
            for (x, dir) in [(c.min.x, 1.0f32), (c.max.x, -1.0)] {
                let y = c.min.y;
                cv.poly(
                    &[
                        pos2(x, y),
                        pos2(x + dir * s, y),
                        pos2(x + dir * s, y + tk),
                        pos2(x + dir * tk, y + tk),
                        pos2(x + dir * tk, y + s),
                        pos2(x, y + s),
                    ],
                    0xd7dadd,
                    1.0,
                );
            }
        }
        ("cab_4x12", _) => {
            cv.rr_stroke(c.shrink(w * 0.052), 3.0, 2.0, 0xedede8, 1.0);
            grille(&mut cv, g, 1.5, Cloth::SaltPepper, k);
            for &(sc, sr) in &sps {
                ghost(&mut cv, sc, sr, false, false);
            }
            // the slant: the top half catches the light, a seam across the middle
            let (top, _) = g.split_top_bottom_at_fraction(0.5);
            cv.rr(top, 0.0, 0xffffff, 0.08);
            cv.seg(g.left_center(), g.right_center(), 2.2, 0x000000, 0.55);
            cv.seg(
                g.left_center() + vec2(0.0, 2.0),
                g.right_center() + vec2(0.0, 2.0),
                0.8,
                0xffffff,
                0.15,
            );
            amps::corners_plastic(&mut cv, c, (w * 0.09).clamp(6.0, 22.0));
        }
        ("cab_8x10", _) => {
            for gg in &gs {
                cv.rr_stroke(gg.expand(2.5), 3.0, 1.4, 0x8d9196, 1.0);
                grille(&mut cv, *gg, 2.0, Cloth::Sparkle, k);
            }
            for &(sc, sr) in &sps {
                ghost(&mut cv, sc, sr, true, false);
            }
            for q in [fridge_plate(c), fridge_tag(c)] {
                cv.shadow(q.translate(vec2(0.0, 1.0)), 2.0, 1.5, 0.5);
                cv.rr(q.expand(1.0), 2.5, 0xc3c8cc, 1.0);
                cv.rr(q, 2.0, 0x0b0b0c, 1.0);
            }
            amps::corners_metal(&mut cv, c, (w * 0.12).clamp(6.0, 18.0), true);
        }
        ("cab_4x10", _) | ("cab_2x10", _) => {
            perforated(&mut cv, g, 2.0);
            for &(sc, sr) in &sps {
                ghost(&mut cv, sc, sr, true, false);
                cv.ring(sc, sr * 1.02, 1.0, 0x000000, 0.35);
            }
            if let Some(q) = horn(id, c) {
                cv.rr(q.expand(1.5), 2.0, 0x000000, 0.6);
                cv.rr(q, 2.0, 0x141517, 1.0);
                cv.rr(
                    q.shrink2(vec2(q.width() * 0.3, q.height() * 0.32)),
                    1.0,
                    0x020202,
                    1.0,
                );
                cv.rr_stroke(q, 2.0, 0.8, 0x6a6e73, 0.6);
            }
            if id == "cab_2x10" {
                for q in ports(c) {
                    cv.rr(q, q.height() * 0.5, 0x020202, 1.0);
                    cv.rr_stroke(q, q.height() * 0.5, 1.0, 0x3a3c40, 0.8);
                }
            } else {
                amps::corners_metal(&mut cv, c, (w * 0.1).clamp(6.0, 18.0), false);
            }
        }
        ("cab_4x15", _) => {
            cv.rr_stroke(g.expand(2.5), 3.0, 1.6, 0xc3c8cc, 1.0);
            grille(&mut cv, g, 2.0, Cloth::BlackWeave, k);
            for &(sc, sr) in &sps {
                ghost(&mut cv, sc, sr, true, false);
            }
            let q = big_plate(c);
            cv.shadow(q.translate(vec2(0.0, 1.0)), 2.0, 1.5, 0.5);
            cv.rr(q.expand(1.0), 2.5, 0xc3c8cc, 1.0);
            cv.rr(q, 2.0, 0x0b0b0c, 1.0);
            amps::corners_metal(&mut cv, c, (w * 0.11).clamp(6.0, 18.0), true);
        }
        _ => {
            // the flip-top: the seam and latches, a chrome ring round the grille
            let seam = c.min.y + h * 0.21;
            cv.seg(
                pos2(c.min.x + 1.0, seam),
                pos2(c.max.x - 1.0, seam),
                2.0,
                0x0b1020,
                1.0,
            );
            cv.seg(
                pos2(c.min.x + 1.0, seam + 1.6),
                pos2(c.max.x - 1.0, seam + 1.6),
                0.7,
                0x8aa0d0,
                0.35,
            );
            for x in [c.min.x + w * 0.12, c.max.x - w * 0.12 - w * 0.08] {
                cv.chrome(
                    Rect::from_min_size(pos2(x, seam - w * 0.035), vec2(w * 0.08, w * 0.07)),
                    1.5,
                );
            }
            cv.rr_stroke(g.expand(2.0), w * 0.035, 3.0, 0xc3c8cc, 1.0);
            grille(&mut cv, g, w * 0.03, Cloth::SilverSparkle, k);
            ghost(&mut cv, sps[0].0, sps[0].1, false, false);
            let q = flip_plate(c);
            cv.shadow(q.translate(vec2(0.0, 1.0)), 2.0, 1.5, 0.5);
            cv.rr(q.expand(1.0), 2.5, 0xc3c8cc, 1.0);
            cv.rr(q, 2.0, 0x0b0b0c, 1.0);
        }
    }
    // light from above, shade at the sides
    cv.rr_v(
        c,
        rad,
        &[
            (0.0, 0xffffff, 0.09),
            (0.25, 0xffffff, 0.02),
            (0.7, 0x000000, 0.05),
            (1.0, 0x000000, 0.36),
        ],
    );
    cv.rr_grad(
        c,
        rad,
        c.left_center(),
        c.right_center(),
        &[
            (0.0, 0x000000, 0.26),
            (0.1, 0x000000, 0.0),
            (0.9, 0x000000, 0.0),
            (1.0, 0x000000, 0.3),
        ],
    );
    cv.rr_stroke(c, rad, 1.0, 0x000000, 0.7);
    cv.image()
}

fn linen_plate(c: Rect) -> Rect {
    Rect::from_center_size(
        pos2(c.center().x, c.min.y + c.height() * 0.105),
        vec2(c.width() * 0.36, c.height() * 0.11),
    )
}

fn fridge_plate(c: Rect) -> Rect {
    Rect::from_min_size(
        c.min + vec2(c.width() * 0.18, c.height() * 0.07),
        vec2(c.width() * 0.42, c.height() * 0.05),
    )
}

fn fridge_tag(c: Rect) -> Rect {
    Rect::from_min_size(
        pos2(c.max.x - c.width() * 0.36, c.max.y - c.height() * 0.1),
        vec2(c.width() * 0.17, c.height() * 0.032),
    )
}

fn big_plate(c: Rect) -> Rect {
    Rect::from_center_size(
        pos2(c.center().x, c.min.y + c.height() * 0.065),
        vec2(c.width() * 0.4, c.height() * 0.055),
    )
}

fn flip_plate(c: Rect) -> Rect {
    Rect::from_center_size(
        pos2(c.center().x, c.min.y + c.height() * 0.1),
        vec2(c.width() * 0.44, c.height() * 0.09),
    )
}

/// A bass cab's horn (tweeter) on its front: between the speakers (P37.6c; the 4x10 and 2x10 have one, as the real
/// cabs do).
fn horn(id: &str, c: Rect) -> Option<Rect> {
    let (gs, _, _) = parts(id, Look::Blackface, c);
    let g = gs[0];
    let w = c.width();
    match id {
        "cab_4x10" => Some(Rect::from_center_size(g.center(), vec2(w * 0.13, w * 0.075))),
        "cab_2x10" => Some(Rect::from_center_size(
            pos2(g.center().x, g.min.y + g.height() * 0.22),
            vec2(w * 0.1, w * 0.06),
        )),
        _ => None,
    }
}

/// The 2x10's ports: two slots under the grille.
fn ports(c: Rect) -> [Rect; 2] {
    let (w, h) = (c.width(), c.height());
    let y = c.min.y + h * 0.83;
    [0.12, 0.56].map(|fx| Rect::from_min_size(pos2(c.min.x + w * fx, y), vec2(w * 0.32, h * 0.075)))
}

/// Perforated black steel (the modern bass cabs' grille): staggered round holes, the light catching the top.
fn perforated(cv: &mut Cv, q: Rect, rad: f32) {
    let h = q.height();
    cv.pattern(q, rad, |x, y| {
        let s = 3.4;
        let row = (y / s).floor();
        let ox = if (row as i32) % 2 == 0 { 0.0 } else { s * 0.5 };
        let tx = (x + ox).rem_euclid(s) - s * 0.5;
        let ty = y.rem_euclid(s) - s * 0.5;
        let d = (tx * tx + ty * ty).sqrt();
        let lit = 0.07 * (1.0 - y / h * 2.5).max(0.0);
        let c = if d < 1.05 {
            amps::rgbf(0x060607)
        } else if d < 1.35 && ty > 0.0 {
            amps::rgbf(0x55595e)
        } else {
            amps::rgbf(0x2c2e31)
        };
        [c[0] + lit, c[1] + lit, c[2] + lit, 1.0]
    });
    cv.rr_stroke(q, rad, 1.4, 0x000000, 0.7);
}

pub(super) fn layout(b: &Block, r: Rect, detail: Detail) -> Layout {
    let def = b.def();
    let a = art_rect(def.id, r, detail);
    let c = cab_rect(def.id, a);
    let (gs, sps, mic) = parts(def.id, look_of(b), c);
    let mut l = Layout {
        radius: if detail == Detail::Full {
            10.0
        } else {
            c.width() * 0.035
        },
        panel: gs[0],
        name: gs[0],
        cone: sps.get(mic).copied(),
        led: pos2(r.max.x - 10.0, r.min.y + 10.0),
        switch: Rect::NOTHING,
        ..Default::default()
    };
    if detail == Detail::Full {
        // the engineer's strip beside the cab (P37.13: its own width, scaled with the face): mic place and distance
        // as faders across, the mic type, then the cuts (and a bass cab's Horn and DI) each in its own column
        let k = r.height() / UNIT_H;
        let x1 = r.max.x - 14.0 * k;
        let x0 = (x1 - (STRIP_W - 30.0) * k).max(a.max.x + 10.0);
        let cw = (x1 - x0).max(40.0);
        let mut y = r.min.y + 26.0 * k;
        for i in [0usize, 1] {
            l.faders
                .push((i, Rect::from_min_size(pos2(x0, y), vec2(cw, 8.0 * k))));
            y += 44.0 * k;
        }
        l.choices
            .push((2, Rect::from_min_size(pos2(x0, y - 6.0 * k), vec2(cw, 20.0 * k))));
        y += 26.0 * k;
        let row: Vec<usize> = (3..def.front_len()).collect();
        let n = row.len().max(1) as f32;
        let col = cw / n;
        let d = (r.max.y - y - 22.0 * k).min(30.0 * k).min(col * 0.6).max(12.0);
        for (j, kk) in row.iter().enumerate() {
            let cx = x0 + col * (j as f32 + 0.5);
            l.knobs.push((*kk, pos2(cx, y + d * 0.5 + 2.0 * k), d));
        }
    }
    l
}

/// The body (cached per size): in the editor, the strip the controls sit on; the cabinet itself is drawn (and cached
/// by its look) in `controls`.
pub(super) fn body(_b: &Block, r: Rect, l: &Layout, ppp: f32) -> Image {
    if l.faders.is_empty() {
        return Image::new(1, 1);
    }
    let Some(mut cv) = Cv::new(r, ppp) else {
        return Image::new(1, 1);
    };
    cv.rr(r.shrink(0.5), 10.0, 0x1d1f23, 1.0);
    cv.rr_stroke(r.shrink(0.5), 10.0, 1.0, 0xffffff, 0.07);
    cv.image()
}

/// The mic types' silhouettes, the head at `m`; returns where the stand holds it.
fn mic(p: &Painter, m: Pos2, kind: usize, s: f32) -> Pos2 {
    let poly = |pts: &[Vec2], fill: Color32| {
        let v: Vec<Pos2> = pts.iter().map(|&q| m + q * s).collect();
        p.add(Shape::convex_polygon(
            v,
            fill,
            Stroke::new(0.5, Color32::from_black_alpha(160)),
        ));
    };
    match kind {
        0 => {
            // a 57-style dynamic, end-on, angled down and right
            let (sn, cs) = 36f32.to_radians().sin_cos();
            let ax = vec2(cs, sn);
            let pp = vec2(-sn, cs);
            let quad = |a0: f32, a1: f32, w0: f32, w1: f32| {
                vec![
                    ax * a0 - pp * w0,
                    ax * a1 - pp * w1,
                    ax * a1 + pp * w1,
                    ax * a0 + pp * w0,
                ]
            };
            let sh: Vec<Pos2> = quad(0.0, 140.0, 9.6, 6.6)
                .iter()
                .map(|&q| m + q * s + vec2(6.0, 7.0) * s)
                .collect();
            p.add(Shape::convex_polygon(
                sh,
                Color32::from_black_alpha(90),
                Stroke::NONE,
            ));
            poly(&quad(40.0, 128.0, 7.6, 6.6), c32(0x141517));
            poly(&quad(128.0, 140.0, 6.4, 6.4), c32(0xc3c8cd));
            poly(&quad(36.0, 40.0, 7.7, 7.7), c32(0x1b1c1e));
            poly(&quad(0.0, 36.0, 9.6, 7.7), c32(0x5c6065));
            for k in 1..6 {
                let a = 6.0 * k as f32;
                p.line_segment(
                    [m + (ax * a - pp * 8.6) * s, m + (ax * a + pp * 8.6) * s],
                    Stroke::new(0.6, ca(0x26282b, 0.8)),
                );
            }
            p.line_segment(
                [m + (ax * 42.0 - pp * 5.5) * s, m + (ax * 126.0 - pp * 4.8) * s],
                Stroke::new(0.8, ca(0xffffff, 0.16)),
            );
            m + (ax * 104.0 + pp * 14.0) * s
        }
        1 => {
            // a slim ribbon, side-on
            let body = Rect::from_min_max(m + vec2(-11.0, -64.0) * s, m + vec2(11.0, 60.0) * s);
            p.rect_filled(
                body.translate(vec2(7.0, 5.0) * s),
                11.0 * s,
                Color32::from_black_alpha(90),
            );
            p.rect_filled(body, 11.0 * s, c32(0xbfc4c9));
            p.rect_filled(
                Rect::from_min_max(body.min, pos2(body.min.x + 6.0 * s, body.max.y)),
                6.0 * s,
                ca(0xffffff, 0.35),
            );
            let win = Rect::from_min_max(m + vec2(-7.0, -55.0) * s, m + vec2(7.0, 15.0) * s);
            p.rect_filled(win, 7.0 * s, c32(0x26282b));
            p.rect_filled(win.shrink2(vec2(4.5 * s, 3.0 * s)), 3.0 * s, ca(0x0d0e0f, 0.8));
            p.rect_filled(
                Rect::from_min_max(m + vec2(-8.0, 59.0) * s, m + vec2(8.0, 67.0) * s),
                1.5 * s,
                c32(0x2b2d31),
            );
            m + vec2(0.0, 76.0) * s
        }
        _ => {
            // a large-diaphragm condenser in its shock mount
            let head = Rect::from_min_max(m + vec2(-16.0, -66.0) * s, m + vec2(16.0, -6.0) * s);
            let body = Rect::from_min_max(m + vec2(-15.0, 0.0) * s, m + vec2(15.0, 78.0) * s);
            p.rect_filled(
                Rect::from_min_max(head.min, body.max).translate(vec2(9.0, 6.0) * s),
                14.0 * s,
                Color32::from_black_alpha(90),
            );
            p.rect_filled(body, 6.0 * s, c32(0xb9bec3));
            p.rect_filled(
                Rect::from_min_max(body.min, pos2(body.min.x + 8.0 * s, body.max.y)),
                4.0 * s,
                ca(0xffffff, 0.3),
            );
            p.rect_filled(head, 15.0 * s, c32(0xc9cdd2));
            for k in 0..8 {
                let y = head.min.y + head.height() * (k as f32 + 0.5) / 8.0;
                p.line_segment(
                    [pos2(head.min.x + 2.0 * s, y), pos2(head.max.x - 2.0 * s, y)],
                    Stroke::new(0.5, ca(0x6f757b, 0.7)),
                );
            }
            p.line_segment(
                [
                    pos2(head.min.x, head.center().y),
                    pos2(head.max.x, head.center().y),
                ],
                Stroke::new(1.2 * s.max(0.6), c32(0xa7adb3)),
            );
            p.rect_filled(
                Rect::from_min_max(m + vec2(-17.5, -8.0) * s, m + vec2(17.5, -1.0) * s),
                2.0 * s,
                c32(0x9da3a9),
            );
            // the shock mount's ring
            let ring: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24.0 * std::f32::consts::TAU;
                    m + vec2(30.0 * a.cos(), 18.0 + 9.0 * a.sin()) * s
                })
                .collect();
            p.add(Shape::line(ring, Stroke::new((3.0 * s).max(1.0), c32(0x1b1d20))));
            m + vec2(41.0, 33.0) * s
        }
    }
}

/// The stand from the clip down to the floor: a boom, a pole, three legs.
fn stand(p: &Painter, joint: Pos2, floor: f32, s: f32) {
    let dark = c32(0x1b1d20);
    let w = (3.6 * s).max(1.2);
    let px = joint.x + 34.0 * s;
    let py = (joint.y + 14.0 * s).min(floor - 10.0 * s);
    let hub = floor - (26.0 * s).max(6.0);
    p.line_segment([joint, pos2(px, py)], Stroke::new(w, dark));
    p.line_segment([pos2(px, py), pos2(px, hub)], Stroke::new(w, dark));
    p.line_segment(
        [pos2(px - w * 0.3, py), pos2(px - w * 0.3, hub)],
        Stroke::new((w * 0.25).max(0.5), c32(0x62676d)),
    );
    let leg = (24.0 * s).max(6.0);
    for dx in [-leg, leg, leg * 0.25] {
        p.line_segment([pos2(px, hub), pos2(px + dx, floor)], Stroke::new(w * 0.9, dark));
    }
    p.circle_filled(joint, w * 1.1, c32(0x2a2b2e));
}

/// What's drawn live over the body: the cabinet (cached), the mic, the controls in the editor.
pub(super) fn controls(p: &Painter, r: Rect, b: &Block, l: &Layout, detail: Detail, hot: Hot) {
    let def = b.def();
    let th = t();
    let ctx = p.ctx();
    let mut look = look_of(b);
    if detail == Detail::Board && def.id == "cab_2x12" {
        look = match amps::amp_before(ctx.cumulative_pass_nr(), r.min.x) {
            Some(Amp::Ac30) => Look::Dartford,
            _ => Look::Blackface,
        };
        LOOKS.with(|m| m.borrow_mut().insert(b.uid, look));
    }
    let a = art_rect(def.id, r, detail);
    let c = cab_rect(def.id, a);
    let ppp = p.pixels_per_point();
    let k = super::key((
        "cab-art",
        def.id,
        look,
        (a.width() * ppp) as i32,
        (a.height() * ppp) as i32,
    ));
    let (tex, _) = paint::texture(ctx, Store::Face, k, || art(def.id, look, a, ppp));
    p.image(
        tex,
        a,
        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    // the names (not on a miniature, P37.12)
    let (gs, sps, mic_i) = parts(def.id, look, c);
    let (w, h) = (c.width(), c.height());
    let g = gs[0];
    match (def.id, look) {
        _ if super::mini_now() => {}
        ("cab_1x10", _) | ("cab_2x12", Look::Blackface) => {
            let word = if def.id == "cab_1x10" { "Bedroom" } else { "Gemini" };
            logo(
                p,
                word,
                g.min + vec2(g.width() * 0.05, g.height() * 0.03),
                Align2::LEFT_TOP,
                (g.height() * 0.2).clamp(9.0, 40.0),
                c32(0xe9ecef),
                g.width() * 0.5,
                Lettering::Script,
                false,
            );
        }
        ("cab_1x12", _) | ("cab_4x10_open", _) => {
            let q = linen_plate(c);
            super::lettering(
                p,
                def.name,
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.85,
                c32(0xf0e6cc),
                q.width() * 0.9,
                Lettering::Script,
                false,
            );
        }
        ("cab_2x12", Look::Dartford) => {
            logo(
                p,
                "DARTFORD",
                pos2(c.center().x, c.min.y + h * 0.12),
                Align2::CENTER_CENTER,
                (h * 0.12).clamp(8.0, 30.0),
                c32(0xd4b262),
                w * 0.8,
                Lettering::Engraved,
                true,
            );
        }
        ("cab_4x12", _) => {
            logo(
                p,
                "SLANT STACK",
                g.min + vec2(g.width() * 0.06, g.height() * 0.05),
                Align2::LEFT_TOP,
                (g.height() * 0.075).clamp(6.0, 22.0),
                c32(0xf4f2ec),
                g.width() * 0.6,
                Lettering::Heavy,
                true,
            );
        }
        ("cab_8x10", _) => {
            let q = fridge_plate(c);
            super::lettering(
                p,
                "the fridge",
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.8,
                c32(0xe8ebee),
                q.width() * 0.9,
                Lettering::Rounded,
                false,
            );
            let q = fridge_tag(c);
            super::lettering(
                p,
                "8x10",
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.8,
                c32(0xe8ebee),
                q.width() * 0.9,
                Lettering::Rounded,
                false,
            );
        }
        ("cab_4x10", _) | ("cab_2x10", _) => {
            let word = if def.id == "cab_4x10" {
                "PUNCH 410"
            } else {
                "COMPACT 210"
            };
            // (the 4x10's badge at the grille's top right: at its foot the mic stand, on the lower right speaker, hid
            // half of it, P38.1)
            let (y, al) = if def.id == "cab_4x10" {
                (g.min.y + g.height() * 0.035, Align2::RIGHT_TOP)
            } else {
                (c.max.y - h * 0.035, Align2::RIGHT_BOTTOM)
            };
            logo(
                p,
                word,
                pos2(g.max.x - g.width() * 0.04, y),
                al,
                (h * 0.06).clamp(6.0, 18.0),
                c32(0xd8dbe0),
                g.width() * 0.45,
                Lettering::Heavy,
                false,
            );
        }
        ("cab_4x15", _) => {
            let q = big_plate(c);
            super::lettering(
                p,
                "big box 4x15",
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.8,
                c32(0xe8ebee),
                q.width() * 0.9,
                Lettering::Rounded,
                false,
            );
        }
        _ => {
            let q = flip_plate(c);
            super::lettering(
                p,
                "flip-top",
                q.center(),
                Align2::CENTER_CENTER,
                q.height() * 0.8,
                c32(0xe8ebee),
                q.width() * 0.9,
                Lettering::Rounded,
                false,
            );
        }
    }
    // the mic on its stand in front of one speaker: Mic slides it from the edge (0) to the cap (10), Distance pulls
    // it back (smaller, its shadow further off)
    if let Some(&(sc, sr)) = sps.get(mic_i) {
        let u = (b.v[0] / 10.0).clamp(0.0, 1.0);
        let dist = (b.v[1] / 10.0).clamp(0.0, 1.0);
        let kind = b.v[2].round().clamp(0.0, 2.0) as usize;
        let s = sr * 0.0105 * (1.0 - dist * 0.3) * if kind == 0 { 1.0 } else { 0.85 };
        let m = pos2(sc.x - (1.0 - u) * sr * 0.85, sc.y);
        let floor = a.max.y;
        // the stand goes behind the mic
        let joint = match kind {
            0 => {
                let (sn, cs) = 36f32.to_radians().sin_cos();
                m + (vec2(cs, sn) * 104.0 + vec2(-sn, cs) * 14.0) * s
            }
            1 => m + vec2(0.0, 76.0) * s,
            _ => m + vec2(41.0, 33.0) * s,
        };
        stand(p, joint, floor, s * 1.6);
        mic(p, m, kind, s);
        let any_hot = matches!(hot.hover.or(hot.active), Some(0..=2));
        if any_hot {
            p.circle_stroke(m, sr * 0.22, Stroke::new(1.5, th.accent));
        }
    }
    if detail == Detail::Full {
        let ink = th.caption;
        for &(i, track) in &l.faders {
            super::fader(
                p,
                track,
                &def.knobs[i],
                b.v[i],
                Face::Cab,
                ink,
                hot.active == Some(i) || hot.hover == Some(i),
            );
        }
        for &(i, cr) in &l.choices {
            super::choice(
                p,
                cr,
                def.knobs[i],
                b.v[i],
                Face::Cab,
                ink,
                def.colour,
                hot.hover == Some(i),
            );
        }
        for &(i, kc, d) in &l.knobs {
            let kd = &def.knobs[i];
            knobs::knob_in(
                p,
                kc,
                d,
                kd.to_u(b.v[i]),
                Scale {
                    zero: None,
                    ..Default::default()
                },
                Some(KnobStyle::Chrome),
                qrs_ui::glass::State::of(hot.hover == Some(i), hot.active == Some(i)),
            );
            let shown = hot.active == Some(i) || hot.hover == Some(i);
            let pos = pos2(kc.x, kc.y + d * 0.5 + 10.0);
            let n = l.knobs.len().max(1) as f32;
            let col = l.faders.first().map_or(d + 26.0, |f| f.1.width() / n).max(d);
            if shown {
                qrs_ui::text::draw(
                    p,
                    &kd.text(b.v[i]),
                    pos,
                    Align2::CENTER_CENTER,
                    qrs_ui::text::Style::new(10.0, 800, th.accent_ink),
                    Some(col),
                );
            } else {
                super::label(p, kd.label, pos, 8.0, ink, col - 4.0);
            }
        }
    }
}

pub(super) fn board_size(b: &Block) -> Vec2 {
    size(b.def().id)
}
