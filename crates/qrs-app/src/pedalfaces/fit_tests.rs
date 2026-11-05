//! P37.13: every model's open unit drawn headless at the room's sizes, and what it draws checked: no two controls on
//! top of each other, no label over another label or over a control, everything inside the face it's given.

use eframe::egui;
use egui::{Context, LayerId, Pos2, Rect, Shape, vec2};
use qrs_audio::board::Block;
use qrs_audio::board::catalog::MODELS;

use super::{Detail, Hot, Layout, draw, layout};

/// What a face drew: its control rects (knob discs, faders, switches) and its text boxes (glyphs' boxes, a little
/// inside the line box so two rows of small print may touch).
pub(crate) struct Drawn {
    pub l: Layout,
    pub controls: Vec<(usize, Rect)>,
    pub texts: Vec<(String, Rect)>,
}

fn collect(shape: &Shape, out: &mut Vec<(String, Rect)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        Shape::Text(t) if t.angle.abs() < 1e-3 => {
            let r = t.galley.rect.translate(t.pos.to_vec2());
            let txt = t.galley.job.text.clone();
            if !txt.trim().is_empty() && r.width() > 0.5 {
                // (the line box carries the ascent and descent; the glyphs sit in about its middle 70%)
                out.push((txt, r.shrink2(vec2(0.5, r.height() * 0.15))));
            }
        }
        _ => {}
    }
}

/// Draw `b` in `r` (Full detail) on a headless context and return what went where.
pub(crate) fn drawn(ctx: &Context, b: &Block, r: Rect) -> Drawn {
    let l = layout(b, r, Detail::Full);
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(4000.0, 3000.0))),
        ..Default::default()
    };
    let mut texts = Vec::new();
    let mut out = ctx.run_ui(input, |ui| {
        let p = ui.ctx().layer_painter(LayerId::background());
        draw(&p, r, b, &l, Detail::Full, Hot::default());
    });
    out.textures_delta.clear();
    for cs in &out.shapes {
        collect(&cs.shape, &mut texts);
    }
    let mut controls: Vec<(usize, Rect)> = l
        .knobs
        .iter()
        .chain(&l.extra)
        .map(|&(k, c, d)| (k, Rect::from_center_size(c, vec2(d, d))))
        .collect();
    controls.extend(l.faders.iter().copied());
    controls.extend(l.choices.iter().copied());
    Drawn { l, controls, texts }
}

/// The overlap of two rects (zero when they only touch).
pub(crate) fn overlap(a: Rect, b: Rect) -> f32 {
    let i = a.intersect(b);
    if i.width() <= 0.75 || i.height() <= 0.75 {
        0.0
    } else {
        i.area()
    }
}

/// What's wrong with a drawn face in `r`: each problem in words.
pub(crate) fn problems(id: &str, d: &Drawn, r: Rect) -> Vec<String> {
    let mut bad = Vec::new();
    let inside = r.expand(1.0);
    for (i, &(k, a)) in d.controls.iter().enumerate() {
        if !inside.contains_rect(a) {
            bad.push(format!("{id}: control {k} {a:?} outside the face {r:?}"));
        }
        for &(k2, b) in &d.controls[i + 1..] {
            if overlap(a, b) > 0.0 {
                bad.push(format!("{id}: controls {k} and {k2} overlap"));
            }
        }
    }
    for (i, (s, a)) in d.texts.iter().enumerate() {
        if !inside.contains_rect(*a) {
            bad.push(format!("{id}: \"{s}\" {a:?} outside the face {r:?}"));
        }
        for (s2, b) in &d.texts[i + 1..] {
            // (a word printed twice a hair apart is an emboss: its shadow or its light)
            let emboss = s == s2 && overlap(*a, *b) > 0.6 * a.area().min(b.area());
            // (a skirted knob's own numbers turn with it, round its edge)
            let skirt = d.l.knobs.iter().any(|&(_, c, dd)| {
                let k = Rect::from_center_size(c, vec2(dd, dd) * 1.15);
                k.contains_rect(*a) && k.contains_rect(*b)
            });
            if !emboss && !skirt && overlap(*a, *b) > 0.0 {
                bad.push(format!("{id}: \"{s}\" and \"{s2}\" collide"));
            }
        }
        // a label on a knob's disc (a segmented switch holds its own words)
        for &(k, c, dd) in d.l.knobs.iter().chain(&d.l.extra) {
            let disc = Rect::from_center_size(c, vec2(dd, dd) * 0.8);
            // (numbers printed on a knob's own skirt are part of it)
            let on_skirt = Rect::from_center_size(c, vec2(dd, dd) * 1.15).contains_rect(*a);
            if !on_skirt && overlap(*a, disc) > 0.0 {
                bad.push(format!("{id}: \"{s}\" over knob {k}"));
            }
        }
    }
    bad
}

pub(crate) fn ctx() -> Context {
    let ctx = Context::default();
    qrs_ui::text::install(&ctx);
    ctx
}

/// Every model as the room's open unit at the room's sizes (crate::room::rig::unit_face_rect), and drawn on its own
/// at the sizes a face is asked for: nothing overlaps, nothing outside.
#[test]
fn every_open_unit_is_clean_at_every_size() {
    let ctx = ctx();
    let mut bad = Vec::new();
    for (m, def) in MODELS.iter().enumerate() {
        let b = Block::new(m);
        for (w, h) in [(1100.0, 700.0), (1440.0, 900.0), (2560.0, 1440.0)] {
            let r = crate::room::rig::unit_face_rect(&b, w, h);
            let d = drawn(&ctx, &b, r);
            bad.extend(
                problems(def.id, &d, r)
                    .into_iter()
                    .map(|s| format!("{w}x{h} {s}")),
            );
        }
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.join("\n"));
}
