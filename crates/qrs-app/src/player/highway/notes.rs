//! Notes and chords on the 3D highway (research/spec/02 notes, 03 chords, 04 feedback).
//!
//! A gem is a solid box (1.9 × 0.8 × 0.4 gaps) travelling at constant speed on its own string's line straight to its
//! fret: no lane, no drop, no lerp. It fades in at the far end, rolls flat (90° → 0°) between 1.8 and 0.3 s out, and
//! single notes carry a stem to a teal plate on the floor (gone 0.25 s before the hit). Its container waits on the
//! string from 0.9 s. A chord is an upright glass box over the hand's window and all six strings with its gems at
//! their own string and fret; repeats are half-height empty boxes; the shape waits at the strings through the hand
//! shape with its finger rings. Hits light the gem and flash the fret wires (a chord: its box's outer posts);
//! misses show MISS / LATE on the string.

use eframe::egui::{self, Align2, Color32, Stroke, vec2};
use qrs_analysis::Judgement;
use qrs_formats::Techs;
use qrs_ui::text::Style;

use super::gfx::*;
use super::view::{FLOOR, V3, fret_w, slot, v3, wire};
use super::{Cache, HwChord, HwNote, chord_span, sharp_name, window_at};

const GEM_W: f32 = 1.9;
const GEM_H: f32 = 0.8;
const GEM_D: f32 = 0.4;
/// A hit's post flash and the hot gem (04 §4): flat top to 0.27 s, off by 0.30.
const FLASH: f32 = 0.30;
/// A hit's burst at the note (our own, louder than the game's wire flash: testers asked for a clearer "you got it").
const BURST: f32 = 0.45;
/// MISS / LATE words: in 0.03 s, hold, out by 0.43 s.
const WORD: f32 = 0.43;
/// Chord box: 1.2 g above the top string, 0.7 g below the bottom one; repeats half as tall.
const BOX_TOP: f32 = 1.2;
const BOX_BOT: f32 = 0.7;
const BOX_EDGE: Color32 = rgb(90, 230, 210);
const BOX_GLASS: Color32 = rgb(10, 20, 30);
const GOLD: Color32 = rgb(230, 170, 40);
/// Chord box fade over its last 15 game units; names clamped 5 units out (00 C1; `View::units`).
const BOX_NEAR: f32 = 15.0;
const NAME_CLAMP: f32 = 5.0;

/// What happened at the strings this frame, for the layers drawn after the strings.
#[derive(Default)]
pub(super) struct Out {
    /// Single-note wire flashes: (fret, age).
    wires: Vec<(i32, f32)>,
    /// Chord post flashes: (left wire, right wire, age).
    posts: Vec<(i32, i32, f32)>,
    /// (string, fret, word, age).
    words: Vec<(i32, i32, &'static str, f32)>,
    /// Hit bursts: (where, string colour, age).
    bursts: Vec<(V3, Color32, f32)>,
}

fn has(t: Techs, f: Techs) -> bool {
    t.0 & f.0 != 0
}

fn mark(n: &HwNote, marks: &[Option<(Judgement, f32)>]) -> Option<(Judgement, f32)> {
    n.group.and_then(|k| marks.get(k).copied().flatten())
}

/// Opacity of a lit hit at age a (flat top, fast cut).
fn flash_k(a: f32) -> f32 {
    if a < 0.27 {
        1.0
    } else {
        ((FLASH - a) / 0.03).clamp(0.0, 1.0)
    }
}

enum Item {
    Note(usize),
    Chord(usize),
    Arp(usize),
}

pub(super) fn draw(pen: &Pen, c: &Cache, marks: &[Option<(Judgement, f32)>]) -> Out {
    let v = pen.v;
    let now = v.now;
    let t_far = now + v.look();
    let mut out = Out::default();
    let i0 = c.notes.partition_point(|n| n.t < now - 12.0);
    let i1 = c.notes.partition_point(|n| n.t <= t_far);
    let mut items: Vec<(f64, Item)> = Vec::new();
    for (i, n) in c.notes[i0..i1].iter().enumerate() {
        if n.chord.is_none() && n.t + n.sus > now - 0.5 {
            items.push((n.t, Item::Note(i0 + i)));
        }
    }
    let c0 = c.chords.partition_point(|ch| ch.t < now - 12.0);
    for (k, ch) in c.chords.iter().enumerate().skip(c0) {
        if ch.t > t_far {
            break;
        }
        items.push((ch.t, Item::Chord(k)));
    }
    for (k, a) in c.arps.iter().enumerate() {
        if a.t <= t_far && a.t > now - 0.5 {
            items.push((a.t, Item::Arp(k)));
        }
    }
    // far to near
    items.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, it) in &items {
        match *it {
            Item::Note(i) => note(pen, c, &c.notes[i], marks),
            Item::Chord(k) => chord(pen, c, k, marks),
            Item::Arp(k) => arp(pen, c, &c.arps[k]),
        }
    }
    // at the strings: the held chord boxes, containers, finger rings, hot gems
    held(pen, c, marks, &mut out);
    for n in &c.notes[c.notes.partition_point(|n| n.t < now - 12.0)..i1] {
        let u = (n.t - now) as f32;
        let m = mark(n, marks);
        let repeat = n.chord.is_some_and(|k| c.chords[k].repeat);
        if let Some((j, age)) = m
            && j != Judgement::Miss
            && age < BURST
        {
            // an open string bursts in the middle of the hand's window
            let x = if n.fret > 0 {
                slot(n.fret)
            } else {
                window_at(c, n.t).map_or(slot(3), |(a, w)| {
                    (wire(a as f32 - 1.0) + wire((a + w) as f32 - 1.0)) / 2.0
                })
            };
            out.bursts.push((
                v3(x, pen.v.sy(n.string), -0.05),
                pen.pal.str_game[si(n.string)],
                age,
            ));
        }
        if repeat {
            continue;
        }
        if n.fret > 0 {
            container(pen, c, n, u, m);
        }
        if let Some((j, age)) = m {
            let s = n.string;
            if j != Judgement::Miss {
                let sounding = n.sus > 0.05 && now >= n.t && now <= n.t + n.sus;
                if age < FLASH || sounding {
                    hot(pen, c, n, age, sounding);
                }
                if age < FLASH && n.chord.is_none() && n.fret > 0 {
                    out.wires.push((n.fret, age));
                }
            }
            if j != Judgement::Hit && age < WORD {
                let w = match j {
                    Judgement::Miss => "MISS",
                    Judgement::Late => "LATE",
                    _ => "EARLY",
                };
                out.words.push((s, n.fret, w, age));
            }
        }
    }
    out
}

/// A gem: a solid box at `at` (its centre), rolled in its face plane by `roll` radians (counter-clockwise),
/// coloured by string; `a` opacity, `lit` 0..1 (hot after a hit).
fn gem(pen: &Pen, at: V3, roll: f32, s: usize, a: f32, lit: f32, size: f32) {
    let v = pen.v;
    let size = size * pen.gem;
    let (hw, hh, hd) = (GEM_W / 2.0 * size, GEM_H / 2.0 * size, GEM_D / 2.0 * size);
    let (sn, cs) = roll.sin_cos();
    let r = |x: f32, y: f32| (x * cs - y * sn, x * sn + y * cs);
    // corners: front (z − hd) then back, each tl tr br bl
    let corner = |x: f32, y: f32, z: f32| {
        let (dx, dy) = r(x, y);
        v3(at.x + dx, at.y + dy, at.z + z)
    };
    let f = [
        corner(-hw, hh, -hd),
        corner(hw, hh, -hd),
        corner(hw, -hh, -hd),
        corner(-hw, -hh, -hd),
    ];
    let b = [
        corner(-hw, hh, hd),
        corner(hw, hh, hd),
        corner(hw, -hh, hd),
        corner(-hw, -hh, hd),
    ];
    let white = Color32::WHITE;
    let lit_c = |c: Color32, k: f32| mix(c, white, k * lit);
    let top_c = lit_c(pen.pal.top_face[s], 0.6);
    // the sides that face the camera (top, bottom, left, right), behind the front face
    let eye = v.eye();
    let sides = [
        ([f[0], f[1], b[1], b[0]], top_c),
        ([f[3], f[2], b[2], b[3]], scale(top_c, 0.6)),
        ([f[0], f[3], b[3], b[0]], scale(top_c, 0.85)),
        ([f[1], f[2], b[2], b[1]], scale(top_c, 0.85)),
    ];
    let mid = |q: &[V3; 4]| (q[0] + q[1] + q[2] + q[3]) * 0.25;
    for (q, col) in sides {
        let m = mid(&q);
        // outward normal ≈ face centre − box centre; visible when it points toward the eye
        let n = m - at;
        let to_eye = eye - m;
        if n.x * to_eye.x + n.y * to_eye.y + n.z * to_eye.z > 0.0 {
            pen.poly(&q, alpha(col, a));
        }
    }
    // the front face: the material's own picture (Bakelite in its bezel, patina, worn paint), white-hot when lit
    if let Some(tex) = pen.gems {
        let k = (a * 255.0) as u8;
        pen.tex_quad(
            f,
            tex,
            super::skin::gem_uv(s),
            Color32::from_rgba_premultiplied(k, k, k, k),
        );
        if lit > 0.0 {
            pen.poly(&f, add(Color32::WHITE, 0.8 * lit * a));
        }
        if let Some(sf) = f.iter().map(|q| v.pt(*q)).collect::<Option<Vec<_>>>() {
            pen.halo(
                &sf,
                (0.06 + 0.5 * lit) * v.scale(at),
                add(pen.pal.str_game[s], (0.08 + 0.5 * lit) * a),
            );
        }
        return;
    }
    // the front face: a vertical gradient top → middle → bottom
    let (ct, cm, cb) = (
        lit_c(pen.pal.face_top[s], 0.8),
        lit_c(pen.pal.face_mid[s], 0.8),
        lit_c(pen.pal.face_bot[s], 0.75),
    );
    let ml = (f[0] + f[3]) * 0.5;
    let mr = (f[1] + f[2]) * 0.5;
    pen.grad(
        [f[0], f[1], mr, ml],
        [alpha(ct, a), alpha(ct, a), alpha(cm, a), alpha(cm, a)],
    );
    pen.grad(
        [ml, mr, f[2], f[3]],
        [alpha(cm, a), alpha(cm, a), alpha(cb, a), alpha(cb, a)],
    );
    // the pale inset outline
    let px = v.scale(at) * GEM_H * size;
    if px > 6.0 {
        let ins = 0.065 * size;
        let o = [
            corner(-hw + ins, hh - ins, -hd - 0.01),
            corner(hw - ins, hh - ins, -hd - 0.01),
            corner(hw - ins, -hh + ins, -hd - 0.01),
            corner(-hw + ins, -hh + ins, -hd - 0.01),
        ];
        let rim = alpha(lit_c(pen.pal.rim[s], 1.0), a);
        for k in 0..4 {
            pen.line(o[k], o[(k + 1) % 4], 0.08 * size, 0.01, rim, rim, 0.8);
        }
    }
    // a soft halo of the string colour (more when lit)
    if let Some(sf) = f.iter().map(|q| v.pt(*q)).collect::<Option<Vec<_>>>() {
        pen.halo(
            &sf,
            (0.1 + 0.5 * lit) * v.scale(at),
            add(pen.pal.str_game[s], (0.15 + 0.5 * lit) * a),
        );
    }
}

/// The technique marks on a gem's face (02 §5), drawn in screen space over its front face.
fn icons(pen: &Pen, at: V3, tech: Techs, s: usize, a: f32) {
    let v = pen.v;
    let Some(c) = v.pt(v3(at.x, at.y, at.z - GEM_D / 2.0)) else {
        return;
    };
    let k = v.scale(at) * pen.gem;
    let (w, h) = (GEM_W * k, GEM_H * k);
    // (from 3.5 px: at 5 the marks only showed in the last moment before the strings at 1440 x 900, P37.6d)
    if h < 3.5 {
        return;
    }
    let p = pen.p;
    let white = alpha(Color32::WHITE, a);
    if has(tech, Techs::HAMMER_ON) {
        let hw = w * 0.225;
        p.add(egui::Shape::convex_polygon(
            vec![
                c + vec2(-hw, -h * 0.5),
                c + vec2(hw, -h * 0.5),
                c + vec2(0.0, h * 0.62),
            ],
            white,
            Stroke::NONE,
        ));
    }
    if has(tech, Techs::PULL_OFF) {
        let hw = w * 0.3;
        p.add(egui::Shape::convex_polygon(
            vec![
                c + vec2(-hw, h * 0.5),
                c + vec2(0.0, -h * 0.8),
                c + vec2(hw, h * 0.5),
            ],
            alpha(rgb(255, 235, 240), a),
            Stroke::NONE,
        ));
    }
    if has(tech, Techs::PALM_MUTE) {
        // a flat X across the face, dark-edged with a pale core (in the string's rim colour alone it vanished into
        // the gem: the technique shots, P37.6d); the fret-hand mute's is taller and white
        let (dx, dy) = (w * 0.42, h * 0.42);
        for st in [
            Stroke::new((h * 0.24).max(1.8), alpha(rgb(16, 12, 20), 0.75 * a)),
            Stroke::new(
                (h * 0.11).max(1.0),
                alpha(mix(pen.pal.str_game[s], Color32::WHITE, 0.75), a),
            ),
        ] {
            p.line_segment([c + vec2(-dx, -dy), c + vec2(dx, dy)], st);
            p.line_segment([c + vec2(-dx, dy), c + vec2(dx, -dy)], st);
        }
    }
    if has(tech, Techs::FRET_HAND_MUTE) {
        let d = h * 0.6;
        for st in [
            Stroke::new((h * 0.3).max(2.0), alpha(pen.pal.str_game[s], a)),
            Stroke::new((h * 0.17).max(1.2), white),
        ] {
            p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], st);
            p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], st);
        }
    }
    if has(tech, Techs::PINCH_HARMONIC) || has(tech, Techs::HARMONIC) {
        let r = w
            * 0.55
            * if has(tech, Techs::PINCH_HARMONIC) {
                1.0
            } else {
                0.7
            };
        p.circle_filled(c, r, alpha(rgb(20, 16, 24), 0.6 * a));
        p.circle_stroke(c, r, Stroke::new((h * 0.12).max(1.0), white));
        p.circle_stroke(
            c,
            r * 0.6,
            Stroke::new((h * 0.08).max(1.0), alpha(pen.pal.rim[s], a)),
        );
        p.circle_filled(c, r * 0.25, alpha(rgb(10, 8, 12), a));
    }
    if has(tech, Techs::TAP) {
        let hw = w * 0.45;
        let st = Stroke::new((h * 0.22).max(1.5), white);
        p.line_segment([c + vec2(-hw, -h * 0.9), c + vec2(0.0, h * 0.4)], st);
        p.line_segment([c + vec2(hw, -h * 0.9), c + vec2(0.0, h * 0.4)], st);
    }
    if has(tech, Techs::SLAP) || has(tech, Techs::POP) {
        let y = if has(tech, Techs::SLAP) {
            h * 0.75
        } else {
            -h * 0.75
        };
        // a bar under the gem (slap: the thumb) or over it (pop), dark-edged so it reads on any string (it was a
        // thin white sliver, P37.6d)
        let bar = egui::Rect::from_center_size(c + vec2(0.0, y), vec2(w * 0.7, (h * 0.26).max(2.0)));
        p.rect_filled(
            bar.expand((h * 0.08).max(1.0)),
            2.0,
            alpha(rgb(16, 12, 20), 0.7 * a),
        );
        p.rect_filled(bar, 1.5, white);
    }
}

/// Bend chevrons stacked just above a gem, one per half step (02 §4).
fn chevrons(pen: &Pen, at: V3, n: usize, s: usize, a: f32) {
    let v = pen.v;
    let k = v.scale(at) * pen.gem;
    let (hw, ch) = (GEM_W * 0.36 * k, GEM_H * 0.32 * k);
    if ch < 1.5 {
        return;
    }
    let Some(top) = v.pt(v3(at.x, at.y + GEM_H * pen.gem / 2.0, at.z - GEM_D / 2.0)) else {
        return;
    };
    for i in 0..n.min(4) {
        let y = top.y - ch * 0.4 - i as f32 * ch * 1.2;
        let pts = vec![
            top + vec2(-hw, y - top.y + ch),
            top + vec2(0.0, y - top.y),
            top + vec2(hw, y - top.y + ch),
        ];
        // (P43.3: a dark edge, then the string's colour lifted, then a white core: bolder than face_bot + white)
        pen.p.add(egui::Shape::line(
            pts.clone(),
            Stroke::new(ch * 0.95, alpha(rgb(10, 10, 14), 0.75 * a)),
        ));
        pen.p.add(egui::Shape::line(
            pts.clone(),
            Stroke::new(ch * 0.7, alpha(mix(pen.pal.str_game[s], Color32::WHITE, 0.2), a)),
        ));
        pen.p.add(egui::Shape::line(
            pts,
            Stroke::new(ch * 0.3, alpha(Color32::WHITE, a)),
        ));
    }
}

/// The gem's roll: 90° far off, flat from 0.3 s (02 §1): 90° × r^1.15, r = (u − 0.3) / 1.5.
fn roll(u: f32) -> f32 {
    let r = ((u - 0.3) / 1.5).clamp(0.0, 1.0);
    std::f32::consts::FRAC_PI_2 * r.powf(1.15)
}

/// A single note on its way in: tail, stem and plate, the gem.
fn note(pen: &Pen, c: &Cache, n: &HwNote, marks: &[Option<(Judgement, f32)>]) {
    let v = pen.v;
    let u = (n.t - v.now) as f32;
    let s = si(n.string);
    let z = v.z(n.t);
    let a = v.fade(z);
    let m = mark(n, marks);
    if n.sus > 0.05 {
        let held = m.is_some_and(|(j, _)| j != Judgement::Miss);
        tail(pen, c, n, held);
    }
    if u < 0.0 || a <= 0.0 {
        return;
    }
    let y = v.sy(n.string);
    if n.fret <= 0 {
        open_bar(pen, c, n, z, a);
        return;
    }
    let x = slot(n.fret);
    // the stem down to a teal plate on the floor, and the fret number there on the first note per fret per anchor
    let sf = a * smooth((u - 0.25) / 0.1);
    if sf > 0.0 {
        let col = alpha(scale(pen.pal.face_mid[s], 1.15), sf);
        pen.line(
            v3(x, y - GEM_H / 2.0, z),
            v3(x, FLOOR, z),
            0.06,
            0.02,
            col,
            col,
            1.0,
        );
        pen.poly(
            &[
                v3(x - GEM_W / 2.0, FLOOR, z - 0.125),
                v3(x + GEM_W / 2.0, FLOOR, z - 0.125),
                v3(x + GEM_W / 2.0, FLOOR, z + 0.125),
                v3(x - GEM_W / 2.0, FLOOR, z + 0.125),
            ],
            alpha(rgb(60, 140, 160), 0.5 * sf),
        );
        if n.num {
            pen.text(
                &n.fret.to_string(),
                v3(x, FLOOR + 0.05, z),
                1.9,
                Align2::CENTER_BOTTOM,
                800,
                alpha(rgb(240, 170, 40), sf),
            );
        }
    }
    let at = v3(x, y, z);
    gem(pen, at, roll(u), s, a, 0.0, 1.0);
    icons(pen, at, n.tech, s, a);
    if n.bend > 0.0 {
        chevrons(pen, at, n.bend.round().max(1.0) as usize, s, a);
        bend_label(pen, at, n.bend, a);
    }
}

/// A bend's amount beside its chevrons (P43.3, a tester: bends were hard to see): ¼, ½, full, 1½, 2 steps.
fn bend_label(pen: &Pen, at: V3, bend: f64, a: f32) {
    let steps = (bend * 2.0).round() / 4.0;
    let txt = match (steps * 4.0) as i32 {
        ..=1 => "¼".to_string(),
        2 => "½".to_string(),
        4 => "full".to_string(),
        q if q % 4 == 0 => format!("{}", q / 4),
        q if q % 4 == 2 => format!("{}½", q / 4),
        _ => format!("{steps}"),
    };
    let x = at.x + GEM_W * 0.5 * pen.gem + 0.15;
    let y = at.y + GEM_H * 0.5 * pen.gem;
    pen.text(
        &txt,
        v3(x, y, at.z),
        1.6 * pen.gem,
        Align2::LEFT_BOTTOM,
        800,
        alpha(Color32::WHITE, a),
    );
}

/// An open string: a thin tapered bar across the hand's window on a translucent cyan plate (02 §5).
fn open_bar(pen: &Pen, c: &Cache, n: &HwNote, z: f32, a: f32) {
    let v = pen.v;
    let (wf, ww) = window_at(c, n.t).unwrap_or((1, 4));
    let (l, r) = (wire((wf - 1) as f32) + 0.3, wire((wf - 1 + ww) as f32) - 0.3);
    let s = si(n.string);
    let y = v.sy(n.string);
    pen.poly(
        &[
            v3(l, FLOOR, z - 0.15),
            v3(r, FLOOR, z - 0.15),
            v3(r, FLOOR, z + 0.15),
            v3(l, FLOOR, z + 0.15),
        ],
        alpha(rgb(60, 160, 190), 0.35 * a),
    );
    bar(pen, l, r, y, z, s, a);
}

/// A tapered bar at height y from l to r (0.25 g thick, pointed ends) in the string's colour, a brighter core.
fn bar(pen: &Pen, l: f32, r: f32, y: f32, z: f32, s: usize, a: f32) {
    let h = 0.125;
    let t = (r - l) * 0.08;
    pen.poly(
        &[
            v3(l, y, z),
            v3(l + t, y + h, z),
            v3(r - t, y + h, z),
            v3(r, y, z),
            v3(r - t, y - h, z),
            v3(l + t, y - h, z),
        ],
        alpha(scale(pen.pal.face_mid[s], 0.95), a),
    );
    let col = alpha(pen.pal.rim[s], 0.8 * a);
    pen.line(
        v3(l + t, y + 0.02, z - 0.01),
        v3(r - t, y + 0.02, z - 0.01),
        0.06,
        0.02,
        col,
        col,
        0.8,
    );
    if let Some(q) = [
        v3(l, y, z),
        v3(l + t, y + h, z),
        v3(r - t, y + h, z),
        v3(r, y, z),
        v3(r - t, y - h, z),
    ]
    .iter()
    .map(|q| pen.v.pt(*q))
    .collect::<Option<Vec<_>>>()
    {
        pen.halo(
            &q,
            0.12 * pen.v.scale(v3(l, y, z)),
            add(pen.pal.str_game[s], 0.25 * a),
        );
    }
}

/// A sustain's tail: a flat ribbon at the string's height from the gem back along its path (02 §4): body in the face
/// colour, pale rails; slides ease from fret to fret, bends swing toward the next string, vibrato is a stepped
/// zigzag. Held (played and sounding): rails white, brighter, a halo.
fn tail(pen: &Pen, c: &Cache, n: &HwNote, played: bool) {
    let v = pen.v;
    let (z0, z1) = (v.z(n.t).max(0.0), v.z(n.t + n.sus).min(v.far()));
    if z1 <= z0 {
        return;
    }
    let s = si(n.string);
    let y = v.sy(n.string);
    let sounding = played && v.now >= n.t && v.now <= n.t + n.sus;
    let k = if sounding { 1.0 } else { 0.0 };
    if n.fret <= 0 {
        // open sustain: two rails along the window's edges, a translucent fill between
        let (wf, ww) = window_at(c, n.t).unwrap_or((1, 4));
        let (l, r) = (wire((wf - 1) as f32) + 0.3, wire((wf - 1 + ww) as f32) - 0.3);
        let zs: Vec<f32> = (0..=12).map(|i| z0 + (z1 - z0) * i as f32 / 12.0).collect();
        for w in zs.windows(2) {
            let (fa, fb) = (v.fade(w[0]), v.fade(w[1]));
            pen.grad(
                [v3(l, y, w[0]), v3(r, y, w[0]), v3(r, y, w[1]), v3(l, y, w[1])],
                [
                    alpha(pen.pal.face_mid[s], 0.3 * fa),
                    alpha(pen.pal.face_mid[s], 0.3 * fa),
                    alpha(pen.pal.face_mid[s], 0.3 * fb),
                    alpha(pen.pal.face_mid[s], 0.3 * fb),
                ],
            );
            for x in [l, r] {
                // a wide opaque rail lying flat along the window's edge (Panic's open A / e), a pale edge
                let rc = mix(pen.pal.face_top[s], Color32::WHITE, 0.15 + 0.4 * k);
                let xi = if x < (l + r) / 2.0 { x + 0.4 } else { x - 0.4 };
                pen.grad(
                    [v3(x, y, w[0]), v3(xi, y, w[0]), v3(xi, y, w[1]), v3(x, y, w[1])],
                    [alpha(rc, fa), alpha(rc, fa), alpha(rc, fb), alpha(rc, fb)],
                );
                let e = mix(pen.pal.rim[s], Color32::WHITE, k);
                pen.line(
                    v3(x, y, w[0]),
                    v3(x, y, w[1]),
                    0.06,
                    0.03,
                    alpha(e, fa),
                    alpha(e, fb),
                    1.0,
                );
            }
        }
        return;
    }
    let vib = has(n.tech, Techs::VIBRATO);
    let trem = has(n.tech, Techs::TREMOLO);
    // (a tremolo's zigzag repeats every 0.6 g and a vibrato's steps every 0.85 g: sampled every 1.2 g they aliased
    // to a straight tail, P37.6d's technique shots; a quarter of the period draws them)
    let step = if trem {
        0.15
    } else if vib {
        0.21
    } else {
        1.2
    };
    let n_seg = (((z1 - z0) / step).ceil() as usize).clamp(4, 480);
    let from = slot(n.fret);
    let to = if n.slide_to > 0 { slot(n.slide_to) } else { from };
    let up = if n.string == 0 && !v.invert { -1.0 } else { 1.0 };
    let mut left = Vec::with_capacity(n_seg + 1);
    let mut right = Vec::with_capacity(n_seg + 1);
    let mut fades = Vec::with_capacity(n_seg + 1);
    // (P43.3: wider than RS's 0.85 and with the note boxes' size, a tester found sustains hard to see)
    let hw = 0.5 * pen.gem;
    for i in 0..=n_seg {
        let z = z0 + (z1 - z0) * i as f32 / n_seg as f32;
        let tt = v.now + v.secs(z) - n.t;
        let w = (tt / n.sus.max(1e-3)).clamp(0.0, 1.0) as f32;
        let mut x = from + (to - from) * smooth(w);
        let mut yy = y;
        if n.bend > 0.0 {
            // up and back like a bend curve: rise over the first half, hold
            yy += up * 0.5 * n.bend as f32 * smooth(w * 2.0);
        }
        if vib {
            let cell = ((z - v.z(n.t)) / 0.85).floor() as i32;
            // (02: a stepped zigzag ≈0.25 of the tail width; on Cowboys it reads nearly straight: kept small)
            x += if cell % 2 == 0 { 0.08 } else { -0.08 };
        }
        if trem {
            x += ((((z - v.z(n.t)) / 0.6).fract()) - 0.5) * 0.3;
        }
        left.push(v3(x - hw, yy, z));
        right.push(v3(x + hw, yy, z));
        fades.push(v.fade(z));
    }
    let body = if sounding {
        scale(pen.pal.face_top[s], 1.2)
    } else {
        pen.pal.face_top[s]
    };
    let rail = if sounding {
        rgb(255, 247, 238)
    } else {
        pen.pal.rim[s]
    };
    let rows: Vec<Vec<V3>> = (0..=n_seg).map(|i| vec![left[i], right[i]]).collect();
    let cols: Vec<Vec<Color32>> = fades
        .iter()
        .map(|&f| vec![alpha(body, f), alpha(body, f)])
        .collect();
    pen.grid(&rows, &cols);
    for i in 0..n_seg {
        for e in [&left, &right] {
            pen.line(
                e[i],
                e[i + 1],
                0.11,
                0.04,
                alpha(rail, fades[i]),
                alpha(rail, fades[i + 1]),
                1.0,
            );
        }
        if sounding {
            let m0 = (left[i] + right[i]) * 0.5;
            let m1 = (left[i + 1] + right[i + 1]) * 0.5;
            pen.line(
                m0,
                m1,
                0.85,
                0.3,
                add(pen.pal.str_game[s], 0.25),
                add(pen.pal.str_game[s], 0.25),
                1.0,
            );
        }
    }
}

/// The note's container waiting on the string (02 §5): a hollow frame in the string colour with a dark window and a
/// bar, from 0.9 s out at ⅓ size growing to full by 0.2 s; a chord's from 1 s (its shape, kept through the hand
/// shape). A missed note's lingers unlit, then goes.
fn container(pen: &Pen, c: &Cache, n: &HwNote, u: f32, m: Option<(Judgement, f32)>) {
    let (show, size) = match n.chord.map(|k| &c.chords[k]) {
        Some(ch) => {
            let end = (ch.hold_end.max(ch.t) - pen.v.now) as f32;
            let k = smooth((1.0 - u) / 0.5);
            (u <= 1.0 && end > -0.05, k)
        }
        None => {
            let k = ((1.08 - u) / 0.85).clamp(1.0 / 3.0, 1.0);
            let gone = match m {
                Some((Judgement::Miss, age)) => age > 0.35,
                Some(_) => true,
                None => u < -0.15,
            };
            (u <= 0.9 && !gone, k)
        }
    };
    if !show {
        return;
    }
    // a hit shows the hot gem instead (drawn after)
    if n.chord.is_none() && m.is_some_and(|(j, _)| j != Judgement::Miss) {
        return;
    }
    let a = if n.chord.is_some() {
        size
    } else {
        ((0.9 - u) / 0.1).clamp(0.0, 1.0) * (0.5 + 0.5 * size)
    };
    let a = match m {
        Some((Judgement::Miss, age)) if n.chord.is_none() => a * (1.0 - (age - 0.25).max(0.0) / 0.1),
        _ => a,
    };
    if a <= 0.0 {
        return;
    }
    let v = pen.v;
    let s = si(n.string);
    let (x, y) = (slot(n.fret), v.sy(n.string));
    let sz = if n.chord.is_some() { 1.0 } else { size };
    let (hw, hh) = (GEM_W / 2.0 * sz, GEM_H / 2.0 * sz);
    let frame = alpha(scale(pen.pal.face_mid[s], 1.2), a);
    pen.poly(
        &[
            v3(x - hw, y + hh, 0.0),
            v3(x + hw, y + hh, 0.0),
            v3(x + hw, y - hh, 0.0),
            v3(x - hw, y - hh, 0.0),
        ],
        // a chord's shape reads as dim solid gems on the strings (Panic, Otherside); a single note's window is dark
        if n.chord.is_some() {
            alpha(scale(pen.pal.face_mid[s], 0.8), 0.7 * a)
        } else {
            alpha(scale(pen.pal.face_mid[s], 0.42), 0.6 * a)
        },
    );
    let t = 0.1 * sz;
    for (p0, p1) in [
        (
            v3(x - hw, y + hh - t / 2.0, 0.0),
            v3(x + hw, y + hh - t / 2.0, 0.0),
        ),
        (
            v3(x - hw, y - hh + t / 2.0, 0.0),
            v3(x + hw, y - hh + t / 2.0, 0.0),
        ),
        (
            v3(x - hw + t / 2.0, y + hh, 0.0),
            v3(x - hw + t / 2.0, y - hh, 0.0),
        ),
        (
            v3(x + hw - t / 2.0, y + hh, 0.0),
            v3(x + hw - t / 2.0, y - hh, 0.0),
        ),
        (v3(x - hw, y, 0.0), v3(x + hw, y, 0.0)),
    ] {
        pen.line(p0, p1, t, 0.03, frame, frame, 1.0);
    }
    // slide arrow / bend chevron on it
    if n.slide_to > 0 && n.chord.is_none() {
        let dir = if n.slide_to > n.fret { 1.0 } else { -1.0 };
        let bx = x + dir * (hw + 0.15);
        let tip = v3(bx + dir * 0.45, y, 0.0);
        pen.line(v3(bx, y + hh, 0.0), tip, 0.1, 0.03, frame, frame, 1.0);
        pen.line(tip, v3(bx, y - hh, 0.0), 0.1, 0.03, frame, frame, 1.0);
    }
    if n.bend > 0.0 && n.chord.is_none() {
        let yb = y + hh + 0.15;
        pen.line(
            v3(x - 0.4, yb, 0.0),
            v3(x, yb + 0.3, 0.0),
            0.1,
            0.03,
            frame,
            frame,
            1.0,
        );
        pen.line(
            v3(x, yb + 0.3, 0.0),
            v3(x + 0.4, yb, 0.0),
            0.1,
            0.03,
            frame,
            frame,
            1.0,
        );
    }
}

/// A hit gem, hot at the strings (04 §4): near white with a string-coloured rim and bloom; through its sustain it
/// follows the slide / bend.
fn hot(pen: &Pen, c: &Cache, n: &HwNote, age: f32, sounding: bool) {
    let v = pen.v;
    let s = si(n.string);
    let k = if sounding { 1.0 } else { flash_k(age) };
    if k <= 0.0 {
        return;
    }
    let mut y = v.sy(n.string);
    if n.fret <= 0 {
        let (wf, ww) = window_at(c, n.t).unwrap_or((1, 4));
        let (l, r) = (wire((wf - 1) as f32) + 0.3, wire((wf - 1 + ww) as f32) - 0.3);
        bar(pen, l, r, y, 0.0, s, k);
        let col = add(rgb(255, 250, 240), 0.6 * k);
        pen.line(
            v3(l + 0.4, y, -0.02),
            v3(r - 0.4, y, -0.02),
            0.12,
            0.25,
            col,
            col,
            1.0,
        );
        return;
    }
    let w = ((v.now - n.t) / n.sus.max(1e-3)).clamp(0.0, 1.0) as f32;
    let mut x = slot(n.fret);
    if n.slide_to > 0 && sounding {
        x += (slot(n.slide_to) - x) * smooth(w);
    }
    if n.bend > 0.0 && sounding {
        let up = if n.string == 0 && !v.invert { -1.0 } else { 1.0 };
        y += up * 0.5 * n.bend as f32 * smooth(w * 2.0);
    }
    gem(pen, v3(x, y, 0.0), 0.0, s, k, 0.85, 1.0);
}

/// The box's corners: x from the window's left wire to its right, y from the floor to above the top string.
fn box_x(c: &Cache, ch: &HwChord) -> (f32, f32) {
    let (lo, hi) = chord_span(c, ch);
    (wire((lo - 1) as f32), wire(hi as f32))
}

/// A chord box's outline: double teal edges (outer and an inner line inset 0.15), a brighter bottom rail, a soft
/// additive glow, short receding top and right sides.
fn box_edges(pen: &Pen, l: f32, r: f32, b: f32, t: f32, z: f32, a: f32, depth: f32) {
    let e = alpha(BOX_EDGE, a);
    let ei = alpha(BOX_EDGE, 0.5 * a);
    let g = add(BOX_EDGE, 0.25 * a);
    let q = [v3(l, t, z), v3(r, t, z), v3(r, b, z), v3(l, b, z)];
    // the receding top and right sides
    let (tb, rb) = (v3(l, t, z + depth), v3(r, t, z + depth));
    let br = v3(r, b, z + depth);
    let ef = alpha(BOX_EDGE, 0.45 * a);
    for (p0, p1) in [(q[0], tb), (q[1], rb), (tb, rb), (rb, br), (q[2], br)] {
        pen.line(p0, p1, 0.05, 0.03, ef, ef, 0.8);
    }
    for k in 0..4 {
        let (p0, p1) = (q[k], q[(k + 1) % 4]);
        pen.line(p0, p1, 0.3, 0.3, g, g, 1.0);
        // (Otherside close-up: the outline reads heavier than 03's 0.08 g; the bottom rail brightest)
        let w = if k == 2 { 0.2 } else { 0.12 };
        pen.line(p0, p1, w, 0.03, e, e, 1.0);
    }
    let i = 0.15;
    let qi = [
        v3(l + i, t - i, z),
        v3(r - i, t - i, z),
        v3(r - i, b + i, z),
        v3(l + i, b + i, z),
    ];
    for k in 0..4 {
        pen.line(qi[k], qi[(k + 1) % 4], 0.05, 0.02, ei, ei, 0.8);
    }
}

/// A chord on its way in (03): the glass box over the window at its depth, its gems and open bars inside, gold fret
/// digits on its bottom edge, its name at its upper left (clamped near the line while held). Repeats: a half-height
/// empty box.
fn chord(pen: &Pen, c: &Cache, k: usize, marks: &[Option<(Judgement, f32)>]) {
    let v = pen.v;
    let ch = &c.chords[k];
    let z = v.z(ch.t);
    let (l, r) = box_x(c, ch);
    let (bot, top) = (v.sy_bottom() - BOX_BOT, v.top() + BOX_TOP);
    // sustains first (they run back from the box)
    for &i in &c.chord_notes[k] {
        let n = &c.notes[i];
        if n.sus > 0.05 && !ch.repeat {
            let held = mark(n, marks).is_some_and(|(j, _)| j != Judgement::Miss);
            tail(pen, c, n, held);
        }
    }
    name(pen, c, k, z);
    if z < 0.0 || z > v.far() {
        return;
    }
    let a = v.fade(z) * (z / v.units(BOX_NEAR)).clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let t = if ch.repeat { bot + (top - bot) * 0.5 } else { top };
    let depth = 0.15 * fret_w(chord_span(c, ch).1 as f32);
    pen.poly(
        &[v3(l, t, z), v3(r, t, z), v3(r, bot, z), v3(l, bot, z)],
        alpha(BOX_GLASS, 0.45 * a),
    );
    if !ch.repeat {
        for &i in &c.chord_notes[k] {
            let n = &c.notes[i];
            let s = si(n.string);
            if n.fret <= 0 {
                bar(pen, l + 0.2, r - 0.2, v.sy(n.string), z, s, a);
            } else {
                let at = v3(slot(n.fret), v.sy(n.string), z);
                gem(pen, at, 0.0, s, a, 0.0, 1.0);
                icons(pen, at, n.tech, s, a);
            }
        }
    }
    box_edges(pen, l, r, bot, t, z, a, depth);
    if !ch.repeat {
        let mut fr: Vec<i32> = ch.frets.iter().copied().filter(|&f| f > 0).collect();
        fr.sort_unstable();
        fr.dedup();
        for f in fr {
            pen.text(
                &f.to_string(),
                v3(slot(f), bot - 0.1, z),
                1.1,
                Align2::CENTER_TOP,
                800,
                alpha(GOLD, a),
            );
        }
    }
}

/// The chord's name (03 §3): light grey-white at 70%, the quality as a smaller raised suffix, ♯ drawn by hand; it
/// rides above the box's upper left and stops near the line while the hand shape holds.
fn name(pen: &Pen, c: &Cache, k: usize, z: f32) {
    let v = pen.v;
    let ch = &c.chords[k];
    if ch.repeat || ch.name.trim().is_empty() {
        return;
    }
    // held at the line until the shape ends or the next named chord arrives
    let next = c.chords[k + 1..]
        .iter()
        .find(|o| !o.repeat && !o.name.trim().is_empty())
        .map_or(f64::INFINITY, |o| o.t);
    let end = ch.hold_end.max(ch.t + 0.25).min(next - 0.25);
    let zn = if z < v.units(NAME_CLAMP) {
        if v.now > end {
            return;
        }
        v.units(NAME_CLAMP)
    } else {
        z
    };
    if zn > v.far() {
        return;
    }
    let a = v.fade(zn);
    let (l, _) = box_x(c, ch);
    let at = v3(l - 0.2, v.top() + BOX_TOP + 0.4, zn);
    let Some(p0) = v.pt(at) else { return };
    let size = 1.65 * v.scale(at);
    if size < 5.0 {
        return;
    }
    let col = alpha(rgb(235, 230, 240), 0.72 * a);
    // root (and its ♯) at full size, the rest smaller and raised; as a number in the key, its numeral (bVII, vi) is
    // the root
    let nm = sharp_name(ch.name.trim());
    let roman = |c: char| matches!(c, 'I' | 'V' | 'i' | 'v');
    let split = if nm.trim_start_matches(['b', '♯']).starts_with(roman) {
        nm.char_indices()
            .find(|&(_, c)| !(roman(c) || c == 'b' || c == '♯'))
            .map_or(nm.len(), |(i, _)| i)
    } else {
        nm.char_indices()
            .find(|&(i, ch)| i > 0 && ch != '♯' && ch != '#')
            .map_or(nm.len(), |(i, _)| i)
    };
    let (root, rest) = nm.split_at(split);
    let st = Style::new(size, 500, col);
    let st2 = Style::new(size * 0.7, 500, col);
    let wr = qrs_ui::text::width(pen.p.ctx(), &root.replace('♯', "  "), st);
    let w2 = qrs_ui::text::width(pen.p.ctx(), &rest.replace('♯', "  "), st2);
    let x0 = p0.x - wr - w2;
    let w = text_sharp(pen.p, root, egui::pos2(x0, p0.y), st);
    text_sharp(
        pen.p,
        rest,
        egui::pos2(x0 + w + size * 0.05, p0.y - size * 0.22),
        st2,
    );
}

/// An arpeggio's hand shape (03 §5): a floor plate across the window with gold-white corner ticks.
fn arp(pen: &Pen, c: &Cache, ch: &HwChord) {
    let v = pen.v;
    let z = v.z(ch.t);
    if z < 0.0 || z > v.far() {
        return;
    }
    let a = v.fade(z);
    let (l, r) = box_x(c, ch);
    let d = 1.2;
    pen.poly(
        &[
            v3(l, FLOOR + 0.02, z),
            v3(r, FLOOR + 0.02, z),
            v3(r, FLOOR + 0.02, z + d),
            v3(l, FLOOR + 0.02, z + d),
        ],
        alpha(rgb(120, 200, 220), 0.22 * a),
    );
    let tick = add(rgb(255, 225, 150), 0.9 * a);
    let len = 0.6;
    for (x, dx) in [(l, 1.0), (r, -1.0)] {
        for zz in [z, z + d] {
            pen.line(
                v3(x, FLOOR + 0.03, zz),
                v3(x + dx * len, FLOOR + 0.03, zz),
                0.1,
                0.06,
                tick,
                tick,
                1.0,
            );
        }
        pen.line(
            v3(x, FLOOR + 0.03, z),
            v3(x, FLOOR + 0.03, z + d),
            0.1,
            0.06,
            tick,
            tick,
            1.0,
        );
    }
}

/// Chords at the strings: the held box through the hand shape (light glass, teal edges), the finger rings, and the
/// post flash of every strum in it.
fn held(pen: &Pen, c: &Cache, marks: &[Option<(Judgement, f32)>], out: &mut Out) {
    let v = pen.v;
    let now = v.now;
    let c0 = c.chords.partition_point(|ch| ch.t < now - 12.0);
    let (bot, top) = (v.sy_bottom() - BOX_BOT * 0.6, v.top() + 1.0);
    for (k, ch) in c.chords.iter().enumerate().skip(c0) {
        if ch.t > now + 1.0 {
            break;
        }
        // its strum's flash (full or repeat), at the judged time
        if let Some((j, age)) = c.chord_notes[k].iter().find_map(|&i| mark(&c.notes[i], marks))
            && j != Judgement::Miss
            && age < FLASH
        {
            let (lo, hi) = chord_span(c, ch);
            out.posts.push((lo - 1, hi, age));
        }
        if ch.repeat {
            continue;
        }
        let (l, r) = box_x(c, ch);
        let end = ch.hold_end.max(ch.t + 0.25);
        if now >= ch.t && now <= end {
            // open strings held: bright flat tiles at the window's ends (03 §2)
            let ta = ((end - now) / 0.08).clamp(0.0, 1.0) as f32;
            for &i in &c.chord_notes[k] {
                let n = &c.notes[i];
                if n.fret > 0 {
                    continue;
                }
                let (y, s) = (v.sy(n.string), si(n.string));
                for (x0, x1) in [(l + 0.25, l + 1.0), (r - 1.0, r - 0.25)] {
                    pen.poly(
                        &[
                            v3(x0, y + 0.1, 0.0),
                            v3(x1, y + 0.1, 0.0),
                            v3(x1, y - 0.1, 0.0),
                            v3(x0, y - 0.1, 0.0),
                        ],
                        alpha(mix(pen.pal.str_lit[s], Color32::WHITE, 0.25), ta),
                    );
                }
            }
            let a = ((end - now) / 0.08).clamp(0.0, 1.0) as f32;
            pen.poly(
                &[v3(l, top, 0.0), v3(r, top, 0.0), v3(r, bot, 0.0), v3(l, bot, 0.0)],
                alpha(BOX_GLASS, 0.15 * a),
            );
            let e = alpha(BOX_EDGE, 0.5 * a);
            for (p0, p1) in [
                (v3(l, top, 0.0), v3(r, top, 0.0)),
                (v3(l, bot, 0.0), v3(r, bot, 0.0)),
                (v3(l, bot, 0.0), v3(l, top, 0.0)),
                (v3(r, bot, 0.0), v3(r, top, 0.0)),
            ] {
                pen.line(p0, p1, 0.08, 0.05, e, e, 1.0);
            }
        }
        // finger rings: grey digits beside the ghost containers from 1 s out, white ringed numerals from 0.15 s,
        // one per finger (a barre: on its top string), kept through the hand shape
        let u = (ch.t - now) as f32;
        if u > 1.0 || now > end {
            continue;
        }
        let mut seen: Vec<i32> = Vec::new();
        let mut order: Vec<usize> = (0..ch.frets.len()).collect();
        order.sort_by(|&a, &b| v.sy(b as i32).total_cmp(&v.sy(a as i32)));
        for s in order {
            let (fret, finger) = (ch.frets[s], ch.fingers.get(s).copied().unwrap_or(-1));
            if fret <= 0 || finger < 0 || s >= v.n_str || seen.contains(&finger) {
                continue;
            }
            seen.push(finger);
            let at = v3(slot(fret), v.sy(s as i32), -0.05);
            let Some(q) = v.pt(at) else { continue };
            let px = v.scale(at);
            let digit = if finger == 0 {
                "T".to_string()
            } else {
                finger.to_string()
            };
            // (03 measured 0.15 s from arrival; the compare frames show the rings from ≈0.3 s)
            if u > 0.3 {
                let a = smooth((1.0 - u) / 0.5);
                let off = (GEM_W / 2.0 + 0.4) * px;
                qrs_ui::text::draw(
                    pen.p,
                    &digit,
                    q + vec2(off, 0.0),
                    Align2::CENTER_CENTER,
                    Style::new(0.9 * px, 700, alpha(rgb(170, 170, 175), a)),
                    None,
                );
            } else {
                let rad = 0.55 * px;
                // a barre (the finger on several strings at this fret): a white rounded capsule down them
                let low = (0..ch.frets.len())
                    .filter(|&o| {
                        o < v.n_str && ch.frets[o] == fret && ch.fingers.get(o).copied() == Some(finger)
                    })
                    .map(|o| v.sy(o as i32))
                    .fold(v.sy(s as i32), f32::min);
                if low < v.sy(s as i32) - 0.5
                    && let Some(qb) = v.pt(v3(slot(fret), low, -0.05))
                {
                    let r = egui::Rect::from_min_max(
                        egui::pos2(q.x - rad, q.y - rad),
                        egui::pos2(q.x + rad, qb.y + rad),
                    );
                    pen.p.rect_filled(r, rad, alpha(rgb(10, 12, 16), 0.35));
                    pen.p.rect_stroke(
                        r,
                        rad,
                        Stroke::new((0.07 * px).max(1.0), Color32::WHITE),
                        egui::StrokeKind::Middle,
                    );
                }
                pen.p.circle_filled(q, rad, alpha(rgb(10, 12, 16), 0.55));
                pen.p
                    .circle_stroke(q, rad, Stroke::new((0.07 * px).max(1.0), Color32::WHITE));
                qrs_ui::text::draw(
                    pen.p,
                    &digit,
                    q,
                    Align2::CENTER_CENTER,
                    Style::new(0.85 * px, 700, Color32::WHITE),
                    None,
                );
            }
        }
    }
}

/// The post flashes (additive, after the strings and posts): white-hot core with a magenta edge, swelling to 0.4 g
/// at 0.12 s and narrowing, off at 0.3 s; chords add cyan flares at the post ends.
pub(super) fn flashes(pen: &Pen, out: &Out) {
    let post = |f: i32, age: f32, chord: bool| {
        let k = flash_k(age);
        if k <= 0.0 {
            return;
        }
        let w = if age < 0.12 {
            0.22 + 0.18 * (1.0 - (1.0 - age / 0.12).powi(2))
        } else {
            0.40 - 0.20 * ((age - 0.12) / 0.15).min(1.0)
        };
        let x = wire(f as f32);
        let (b, t) = (v3(x, -0.45, -0.02), v3(x, 5.45, -0.02));
        // the edge: magenta on a single note; a chord's posts glow cyan with a pink fringe (Panic, 03 §1)
        if chord {
            let cy = add(rgb(34, 226, 255), 0.4 * k);
            pen.line(b, t, w * 1.1, w * 0.9, cy, cy, 1.5);
        }
        let edge = add(rgb(239, 82, 255), if chord { 0.22 } else { 0.4 } * k);
        pen.line(b, t, w * 0.9, w * 0.5, edge, edge, 1.5);
        let core = add(rgb(254, 248, 235), k);
        pen.line(b, t, w * 0.45, w * 0.2, core, core, 1.2);
        if chord && age > 0.03 {
            let fl = add(rgb(34, 226, 255), 0.7 * k);
            let len = 0.5 * fret_w(f.max(1) as f32);
            for y in [-0.45, 5.45] {
                pen.line(
                    v3(x - len / 2.0, y, -0.02),
                    v3(x + len / 2.0, y, -0.02),
                    0.15,
                    0.25,
                    fl,
                    fl,
                    1.0,
                );
            }
        }
    };
    for &(f, age) in &out.wires {
        post(f - 1, age, false);
        post(f, age, false);
    }
    for &(l, r, age) in &out.posts {
        post(l, age, true);
        post(r, age, true);
    }
    for &(at, col, age) in &out.bursts {
        burst(pen, at, col, age);
    }
}

/// A hit at the strings: a white-hot core in a glow of the string's colour, and a ring thrown outward, gone by
/// `BURST`.
fn burst(pen: &Pen, at: V3, col: Color32, age: f32) {
    let Some(c) = pen.v.pt(at) else {
        return;
    };
    let g = pen.v.scale(at);
    let u = (age / BURST).clamp(0.0, 1.0);
    let k = (1.0 - u).powf(1.5);
    let glow = (1.1 + 0.5 * u) * g;
    pen.glow(
        c,
        glow,
        add(mix(col, Color32::WHITE, 0.35), 0.9 * k),
        add(col, 0.0),
    );
    pen.glow(c, 0.45 * glow, add(Color32::WHITE, k), add(col, 0.5 * k));
    let ring = (0.5 + 2.2 * smooth(u.powf(0.6))) * g;
    pen.p.circle_stroke(
        c,
        ring,
        Stroke::new(
            (0.16 * g * (1.0 - u)).max(1.0),
            add(mix(col, Color32::WHITE, 0.5), k),
        ),
    );
}

/// MISS / LATE / EARLY on the note's string, centred in its fret space (04 §5).
pub(super) fn overlays(pen: &Pen, _c: &Cache, out: &Out) {
    let v = pen.v;
    let mut placed: Vec<(i32, i32)> = Vec::new();
    for &(s, f, word, age) in out.words.iter().rev() {
        if placed.contains(&(s, f)) {
            continue;
        }
        placed.push((s, f));
        let a = (age / 0.03).min(1.0) * ((WORD - age) / 0.05).clamp(0.0, 1.0);
        let x = if f > 0 { slot(f) } else { slot(3) };
        let col = mix(pen.pal.str_game[si(s)], Color32::WHITE, 0.5);
        let at = v3(x, v.sy(s) - 0.25, -0.05);
        pen.text(word, at, 0.95, Align2::CENTER_BOTTOM, 800, alpha(col, a));
    }
}
