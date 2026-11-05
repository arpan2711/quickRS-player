//! The editor's view behind the panel (PLAN.md P21.5, D45), from the approved mockup Editor-AmpBack: an amp's back
//! (the chassis, its preamp and power tubes glowing with what the board plays, the transformers) and the controls
//! a real amp hides inside (sag, bias, power tubes, rectifier, power master), each with a plain-English caption; a
//! drive's insides (the circuit board, its clipping diodes as they are now) and the choice of diodes. The knobs
//! themselves are the catalogue's back knobs (`KnobDef::back`); their tips are in `gearinfo`. New code (no Python
//! source).

use std::f32::consts::TAU;

use qrs_ui::knobs::{self as kn, KnobStyle, Scale};

use super::*;
use crate::player::gearinfo as gi;

/// What each power tube is, in a few words (`catalog::TUBES` order).
const TUBE_WORDS: [&str; 5] = [
    "British bark, early crunch",
    "American: clean lows, glassy",
    "Huge headroom, tight lows",
    "Chime, breaks up early",
    "Small and sweet, early",
];

/// What each diode pair does, in a few words (`catalog::DIODES` order).
const DIODE_WORDS: [&str; 4] = [
    "The classic, even clip",
    "One side first: warmer, even harmonics",
    "More headroom, louder, harder edge",
    "Soft and round, compresses early",
];

/// Preamp tubes and power tubes in each amp (the real units').
fn tube_counts(id: &str) -> (usize, usize) {
    match id {
        "amp_clean" => (4, 4),
        "amp_tweed" => (2, 2),
        "amp_chime" => (3, 4),
        "amp_modern" => (5, 4),
        "amp_cali" => (5, 4),
        "amp_snake" => (5, 4),
        "amp_tangerine" => (4, 2),
        "amp_surrey" => (4, 4),
        "amp_bass" => (3, 6),
        _ => (3, 4),
    }
}

/// A soft glow: `col` at the centre fading out to nothing at the ellipse's edge.
fn radial(p: &Painter, c: Pos2, rx: f32, ry: f32, col: Color32) {
    if col.a() == 0 || rx < 0.5 || ry < 0.5 {
        return;
    }
    let mut m = egui::Mesh::default();
    let mid = col.gamma_multiply(0.42);
    let n = 36u32;
    m.colored_vertex(c, col);
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        m.colored_vertex(c + vec2(a.cos() * rx * 0.45, a.sin() * ry * 0.45), mid);
    }
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        m.colored_vertex(c + vec2(a.cos() * rx, a.sin() * ry), Color32::TRANSPARENT);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_triangle(0, 1 + i, 1 + j);
        m.add_triangle(1 + i, 1 + n + i, 1 + j);
        m.add_triangle(1 + j, 1 + n + i, 1 + n + j);
    }
    p.add(m);
}

/// A rect shaded from `top` to `bottom`.
fn vgrad(p: &Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), top);
    m.colored_vertex(r.right_top(), top);
    m.colored_vertex(r.right_bottom(), bottom);
    m.colored_vertex(r.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(m);
}

/// A rect shaded across, through `cols` evenly.
fn hgrad(p: &Painter, r: Rect, cols: &[Color32]) {
    let n = cols.len().max(2) - 1;
    let mut m = egui::Mesh::default();
    for (i, &c) in cols.iter().enumerate() {
        let x = r.min.x + r.width() * i as f32 / n as f32;
        m.colored_vertex(pos2(x, r.min.y), c);
        m.colored_vertex(pos2(x, r.max.y), c);
    }
    for i in 0..n as u32 {
        let k = i * 2;
        m.add_triangle(k, k + 1, k + 3);
        m.add_triangle(k, k + 3, k + 2);
    }
    p.add(m);
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// One tube standing on the chassis: `c` the centre of its base line, `w` by `h` points, of `kind` (`TUBES` index,
/// or `None` for a small preamp tube), glowing `glow` (0..1).
fn tube(p: &Painter, base: Pos2, w: f32, h: f32, kind: Option<usize>, glow: f32) {
    let small = kind.is_none();
    let top = base.y - h;
    let x = |f: f32| base.x - w / 2.0 + w * f;
    let y = |f: f32| top + h * f;
    let bh = h * if small { 0.13 } else { 0.15 };
    let gy = base.y - bh;
    let glass_col = rgba(40, 30, 25, 0.55);
    let edge = Stroke::new(1.0, Color32::from_white_alpha(70));
    // the glass: a KT88's shoulder, a 6L6's coke bottle, the others straight
    match kind {
        Some(2) => {
            let lower = Rect::from_min_max(pos2(x(0.06), y(0.42)), pos2(x(0.94), gy));
            let upper = Rect::from_min_max(pos2(x(0.2), y(0.03)), pos2(x(0.8), y(0.5)));
            p.rect_filled(upper, w * 0.28, glass_col);
            p.rect_filled(lower, w * 0.2, glass_col);
            p.rect_stroke(upper, w * 0.28, edge, egui::StrokeKind::Inside);
            p.rect_stroke(lower, w * 0.2, edge, egui::StrokeKind::Inside);
        }
        Some(1) | Some(4) => {
            let body = Rect::from_min_max(pos2(x(0.12), y(0.03)), pos2(x(0.88), gy));
            let belly = Rect::from_min_max(pos2(x(0.04), y(0.3)), pos2(x(0.96), y(0.78)));
            p.rect_filled(body, w * 0.36, glass_col);
            p.rect_filled(belly, w * 0.45, glass_col);
            p.rect_stroke(body, w * 0.36, edge, egui::StrokeKind::Inside);
        }
        _ => {
            let body = Rect::from_min_max(pos2(x(0.12), y(0.03)), pos2(x(0.88), gy));
            p.rect_filled(body, w * 0.36, glass_col);
            p.rect_stroke(body, w * 0.36, edge, egui::StrokeKind::Inside);
        }
    }
    // the getter's silvering at the top
    let getter = Rect::from_min_max(pos2(x(0.16), y(0.035)), pos2(x(0.84), y(0.17)));
    vgrad(
        p,
        getter.shrink2(vec2(w * 0.06, 0.0)),
        Color32::from_rgb(201, 205, 211),
        Color32::from_rgb(40, 41, 44),
    );
    // the plates and their mica spacers
    let (py0, py1) = (y(if small { 0.36 } else { 0.34 }), y(0.70));
    let plates = Rect::from_min_max(pos2(x(0.27), py0), pos2(x(0.73), py1));
    p.rect_filled(plates, 2.0, Color32::from_rgb(47, 50, 54));
    for k in 1..5 {
        let lx = plates.min.x + plates.width() * k as f32 / 5.0;
        p.line_segment(
            [pos2(lx, py0), pos2(lx, py1)],
            Stroke::new(1.0, Color32::from_rgb(29, 31, 34)),
        );
    }
    for my in [py0 - 4.0, py1 + 1.0] {
        p.rect_filled(
            Rect::from_min_size(pos2(x(0.18), my), vec2(w * 0.64, 3.0)),
            0.0,
            rgba(215, 207, 184, 0.75),
        );
    }
    // the glow: the plates' orange, the filament, and a power tube's blue haze over them
    let pc = plates.center();
    radial(
        p,
        pc,
        w * 0.9 * (0.6 + 0.4 * glow),
        (py1 - py0) * 1.1,
        rgba(255, 122, 26, 0.38 * glow),
    );
    radial(
        p,
        pc,
        w * 0.44,
        (py1 - py0) * 0.75,
        rgba(255, 179, 71, 0.95 * glow),
    );
    p.rect_filled(
        Rect::from_min_max(pos2(x(0.46), py0 + 6.0), pos2(x(0.54), py1 - 6.0)),
        2.0,
        rgba(255, 244, 207, 0.85 * glow),
    );
    if !small {
        radial(
            p,
            pos2(pc.x, py0 - h * 0.07),
            w * 0.3,
            h * 0.05,
            rgba(122, 124, 255, 0.3 * glow),
        );
    }
    // the tip, the base, a highlight down the glass
    p.rect_filled(
        Rect::from_min_size(pos2(x(0.45), top), vec2(w * 0.1, h * 0.04)),
        2.0,
        Color32::from_rgb(143, 148, 154),
    );
    if small {
        p.rect_filled(
            Rect::from_min_max(pos2(x(0.1), gy), pos2(x(0.9), base.y)),
            2.0,
            Color32::from_rgb(191, 195, 200),
        );
        p.rect_filled(
            Rect::from_min_max(pos2(x(0.1), base.y - 4.0), pos2(x(0.9), base.y)),
            0.0,
            Color32::from_rgb(106, 111, 117),
        );
    } else {
        p.rect_filled(
            Rect::from_min_max(pos2(x(0.08), gy), pos2(x(0.92), base.y)),
            3.0,
            Color32::from_rgb(20, 20, 20),
        );
        p.rect_filled(
            Rect::from_min_max(pos2(x(0.08), gy), pos2(x(0.92), gy + 3.0)),
            0.0,
            Color32::from_rgb(44, 44, 44),
        );
    }
    p.rect_filled(
        Rect::from_min_max(pos2(x(0.2), y(0.14)), pos2(x(0.27), gy - h * 0.06)),
        2.0,
        Color32::from_white_alpha(46),
    );
}

/// A small label pill over the art.
fn tag(p: &Painter, c: Pos2, s: &str, size: f32, col: Color32, lit: bool) {
    let ctx = p.ctx();
    let st = Style::caps(size, col);
    let w = text::width(ctx, s, st) + size * 1.6;
    let r = Rect::from_center_size(c, vec2(w, size * 2.1));
    p.rect_filled(r, r.height() / 2.0, rgba(14, 15, 17, 0.86));
    let ring = if lit {
        Stroke::new(1.0, rgba(242, 181, 60, 0.6))
    } else {
        Stroke::new(1.0, Color32::from_white_alpha(20))
    };
    p.rect_stroke(r, r.height() / 2.0, ring, egui::StrokeKind::Inside);
    text::draw(p, s, r.center(), Align2::CENTER_CENTER, st, None);
}

/// The back of an amp head in `area`: the tolex box, the dark cavity, the chassis with its tubes and transformers.
fn amp_back(p: &Painter, area: Rect, b: &Block, glow: f32) {
    let def = b.def();
    // the mockup's art is 860 x 340
    let k = (area.width() / 860.0).min(area.height() / 340.0);
    let o = area.center() - vec2(430.0, 170.0) * k;
    let at = |x: f32, y: f32| o + vec2(x, y) * k;
    let rect = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(at(x, y), vec2(w, h) * k);
    let fs = |s: f32| (s * k).clamp(6.5, 13.0);
    // the handle, the box in its covering
    p.rect_filled(
        rect(330.0, 0.0, 200.0, 16.0),
        9.0 * k,
        Color32::from_rgb(22, 22, 22),
    );
    let tolex = if def.id == "amp_tweed" {
        Color32::from_rgb(171, 138, 86)
    } else if def.id == "amp_modern" || def.id == "amp_bass" {
        Color32::from_rgb(24, 24, 26)
    } else {
        Color32::from_rgb(30, 29, 28)
    };
    let boxr = rect(0.0, 12.0, 860.0, 326.0);
    p.rect_filled(
        boxr.translate(vec2(0.0, 10.0 * k)),
        12.0 * k,
        Color32::from_black_alpha(110),
    );
    p.rect_filled(boxr, 12.0 * k, tolex);
    p.rect_stroke(
        boxr,
        12.0 * k,
        Stroke::new(1.0, Color32::from_white_alpha(30)),
        egui::StrokeKind::Inside,
    );
    // the cavity, lit from the tubes
    let cav = rect(22.0, 34.0, 816.0, 282.0);
    p.rect_filled(cav, 6.0 * k, Color32::from_rgb(10, 10, 10));
    radial(
        p,
        at(430.0, 150.0),
        340.0 * k,
        150.0 * k,
        rgba(255, 120, 30, 0.22 * glow),
    );
    radial(
        p,
        at(160.0, 190.0),
        160.0 * k,
        80.0 * k,
        rgba(255, 120, 30, 0.14 * glow),
    );
    // the chassis: the top band and the rear face with its sockets
    vgrad(
        p,
        rect(40.0, 236.0, 780.0, 12.0),
        Color32::from_rgb(225, 228, 231),
        Color32::from_rgb(158, 163, 168),
    );
    let face = rect(40.0, 248.0, 780.0, 58.0);
    vgrad(
        p,
        face,
        Color32::from_rgb(195, 199, 203),
        Color32::from_rgb(141, 146, 151),
    );
    let ink = Color32::from_rgb(27, 28, 30);
    // (the small print only where it's big enough to read)
    let print = 7.5 * k >= 6.0;
    let lab = |x: f32, y: f32, s: &str| {
        if !print {
            return;
        }
        text::draw(
            p,
            s,
            at(x, y),
            Align2::CENTER_CENTER,
            Style::caps(fs(7.5), ink),
            None,
        );
    };
    p.rect_filled(
        rect(66.0, 262.0, 44.0, 30.0),
        4.0 * k,
        Color32::from_rgb(21, 21, 21),
    );
    lab(88.0, 299.0, "MAINS");
    for (fx, s) in [(140.0, "MAINS FUSE"), (196.0, "HT FUSE")] {
        p.circle_filled(at(fx, 275.0), 10.0 * k, Color32::from_rgb(60, 60, 60));
        p.circle_filled(at(fx, 275.0), 4.0 * k, Color32::from_rgb(17, 17, 17));
        lab(fx, 299.0, s);
    }
    p.circle_filled(at(447.0, 275.0), 17.0 * k, Color32::from_rgb(30, 30, 30));
    p.line_segment(
        [at(447.0, 275.0), at(455.0, 262.0)],
        Stroke::new(3.0 * k, Color32::from_rgb(221, 221, 221)),
    );
    lab(447.0, 299.0, "IMPEDANCE");
    for sx in [520.0, 570.0] {
        p.circle_filled(at(sx, 275.0), 11.0 * k, Color32::from_rgb(200, 204, 208));
        p.circle_filled(at(sx, 275.0), 5.0 * k, Color32::from_rgb(8, 8, 8));
    }
    lab(545.0, 299.0, "SPEAKER OUTPUTS");
    p.rect_filled(
        rect(680.0, 262.0, 56.0, 30.0),
        4.0 * k,
        Color32::from_rgb(26, 26, 26),
    );
    p.rect_filled(
        rect(688.0, 268.0, 18.0, 18.0),
        3.0 * k,
        Color32::from_rgb(204, 51, 51),
    );
    lab(708.0, 299.0, "VOLTAGE 230");
    // the preamp tubes, then the power tubes (the chosen kind)
    let (npre, npow) = tube_counts(def.id);
    for i in 0..npre {
        let gx = 110.0 + 50.0 * i as f32 - if npre > 3 { 25.0 } else { 0.0 };
        tube(
            p,
            at(gx, 238.0),
            30.0 * k,
            78.0 * k,
            None,
            (0.7 + 0.1 * (i % 3) as f32) * glow.max(0.3),
        );
    }
    let kind = def
        .knob_index("tubes")
        .map(|i| b.v[i].round().clamp(0.0, 4.0) as usize)
        .unwrap_or(0);
    let (tw, th) = match kind {
        2 => (56.0, 150.0),
        3 => (36.0, 112.0),
        4 => (42.0, 124.0),
        _ => (50.0, 142.0),
    };
    let step = if npow > 4 { 52.0 } else { 62.0 };
    for i in 0..npow {
        let gx = 423.0 + (i as f32 - (npow as f32 - 1.0) / 2.0) * step;
        let flick = [0.95, 1.0, 0.9, 1.0, 0.93, 0.97][i % 6];
        tube(p, at(gx, 238.0), tw * k, th * k, Some(kind), glow * flick);
    }
    // the filter caps and the transformers
    for cx in [600.0, 632.0] {
        let r = rect(cx - 13.0, 166.0, 26.0, 70.0);
        hgrad(
            p,
            r,
            &[
                Color32::from_rgb(108, 112, 117),
                Color32::from_rgb(230, 233, 236),
                Color32::from_rgb(166, 171, 176),
                Color32::from_rgb(91, 95, 99),
            ],
        );
        p.rect_filled(
            rect(cx - 13.0, 166.0, 26.0, 8.0),
            0.0,
            Color32::from_rgb(42, 44, 47),
        );
    }
    for (x, y, w, h, s) in [
        (660.0, 126.0, 96.0, 110.0, "MAINS"),
        (766.0, 150.0, 62.0, 86.0, "OUTPUT"),
    ] {
        let r = rect(x, y, w, h);
        p.rect_filled(
            r.translate(vec2(0.0, 6.0 * k)),
            6.0 * k,
            Color32::from_black_alpha(120),
        );
        hgrad(
            p,
            r,
            &[
                Color32::from_rgb(13, 13, 13),
                Color32::from_rgb(43, 44, 46),
                Color32::from_rgb(22, 23, 24),
                Color32::from_rgb(11, 11, 11),
            ],
        );
        p.rect_filled(
            rect(x + 8.0, y + 10.0, w - 16.0, h * 0.38),
            4.0 * k,
            Color32::from_rgb(45, 46, 49),
        );
        if !print {
            continue;
        }
        text::draw(
            p,
            s,
            at(x + w / 2.0, y + h * 0.7),
            Align2::CENTER_CENTER,
            Style::caps(fs(7.5), Color32::from_rgb(138, 141, 146)),
            None,
        );
    }
    // what's what
    let tname = catalog::TUBES[kind];
    tag(
        p,
        at(if npre > 3 { 135.0 } else { 160.0 }, 132.0),
        &format!("PREAMP \u{b7} {npre} \u{d7} 12AX7"),
        fs(9.0),
        Color32::from_rgb(201, 205, 211),
        false,
    );
    tag(
        p,
        at(423.0, 66.0),
        &format!("POWER \u{b7} {npow} \u{d7} {tname}"),
        fs(9.0),
        Color32::from_rgb(245, 196, 94),
        true,
    );
    tag(
        p,
        at(745.0, 108.0),
        "TRANSFORMERS",
        fs(9.0),
        Color32::from_rgb(201, 205, 211),
        false,
    );
    // how hard the power tubes work, live (sized in points, at the cavity's top right)
    let ctx = p.ctx();
    let ls = Style::caps(7.5, Color32::from_rgb(160, 164, 170));
    let vs = Style::new(9.5, 800, Color32::from_rgb(255, 195, 107));
    let work = format!("{:.0}%", (glow * 100.0).round());
    let (lw, vw, bw) = (
        text::width(ctx, "POWER TUBES WORKING", ls),
        text::width(ctx, "100%", vs),
        40.0,
    );
    let mw = lw + bw + vw + 32.0;
    let mr = Rect::from_min_size(pos2(cav.max.x - 10.0 - mw, cav.min.y + 8.0), vec2(mw, 20.0));
    p.rect_filled(mr, 9.0, rgba(14, 15, 17, 0.86));
    text::draw(
        p,
        "POWER TUBES WORKING",
        pos2(mr.min.x + 8.0, mr.center().y),
        Align2::LEFT_CENTER,
        ls,
        None,
    );
    let bar = Rect::from_min_size(pos2(mr.min.x + 16.0 + lw, mr.center().y - 3.0), vec2(bw, 6.0));
    p.rect_filled(bar, 3.0, Color32::from_rgb(40, 42, 46));
    let fill = Rect::from_min_size(bar.min, vec2(bar.width() * glow.clamp(0.0, 1.0), bar.height()));
    hgrad(
        p,
        fill,
        &[Color32::from_rgb(255, 138, 42), Color32::from_rgb(255, 195, 107)],
    );
    text::draw(
        p,
        &work,
        pos2(mr.max.x - 8.0, mr.center().y),
        Align2::RIGHT_CENTER,
        vs,
        None,
    );
}

/// A drive's insides in `area`: the circuit board, its pots and chip, and the clipping diodes as they are now.
fn drive_inside(p: &Painter, area: Rect, b: &Block, glow: f32) {
    let def = b.def();
    let k = (area.width() / 720.0).min(area.height() / 330.0);
    let o = area.center() - vec2(360.0, 165.0) * k;
    let at = |x: f32, y: f32| o + vec2(x, y) * k;
    let rect = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(at(x, y), vec2(w, h) * k);
    let fs = |s: f32| (s * k).clamp(6.5, 13.0);
    // the open enclosure, its colour, and the board in it
    let encl = rect(0.0, 0.0, 720.0, 330.0);
    let [r, g, bl] = def.colour;
    p.rect_filled(
        encl.translate(vec2(0.0, 8.0 * k)),
        18.0 * k,
        Color32::from_black_alpha(110),
    );
    p.rect_filled(encl, 18.0 * k, Color32::from_rgb(r, g, bl));
    let well = encl.shrink(16.0 * k);
    p.rect_filled(well, 10.0 * k, Color32::from_rgb(52, 54, 56));
    let pcb = well.shrink(14.0 * k);
    vgrad(
        p,
        pcb,
        Color32::from_rgb(28, 92, 52),
        Color32::from_rgb(18, 66, 38),
    );
    // traces
    let trace = Stroke::new(2.0 * k, rgba(120, 200, 140, 0.28));
    for (a, bb) in [
        ((90.0, 120.0), (300.0, 120.0)),
        ((300.0, 120.0), (300.0, 200.0)),
        ((300.0, 200.0), (520.0, 200.0)),
        ((520.0, 200.0), (520.0, 250.0)),
        ((140.0, 250.0), (640.0, 250.0)),
        ((420.0, 90.0), (420.0, 200.0)),
        ((200.0, 90.0), (200.0, 160.0)),
        ((600.0, 90.0), (600.0, 250.0)),
    ] {
        p.line_segment([at(a.0, a.1), at(bb.0, bb.1)], trace);
    }
    // the pots along the top
    for (i, x) in [200.0, 360.0, 520.0].into_iter().enumerate() {
        let c = at(x, 70.0);
        p.circle_filled(c, 26.0 * k, Color32::from_rgb(40, 40, 42));
        p.circle_filled(c, 18.0 * k, Color32::from_rgb(170, 174, 180));
        p.circle_filled(c, 7.0 * k, Color32::from_rgb(90, 94, 100));
        if let Some(kd) = def.front_knobs().get(i) {
            text::draw(
                p,
                kd.label,
                c + vec2(0.0, 36.0 * k),
                Align2::CENTER_CENTER,
                Style::caps(fs(8.0), Color32::from_rgb(215, 235, 220)),
                None,
            );
        }
    }
    // the chip, resistors, capacitors
    let chip = rect(110.0, 150.0, 70.0, 48.0);
    p.rect_filled(chip, 3.0 * k, Color32::from_rgb(20, 20, 22));
    for j in 0..4 {
        for side in [chip.min.y - 5.0 * k, chip.max.y] {
            p.rect_filled(
                Rect::from_min_size(
                    pos2(chip.min.x + (8.0 + 16.0 * j as f32) * k, side),
                    vec2(6.0 * k, 5.0 * k),
                ),
                0.0,
                Color32::from_rgb(190, 192, 196),
            );
        }
    }
    for (x, y) in [(240.0, 170.0), (240.0, 200.0), (560.0, 160.0), (560.0, 190.0)] {
        let rr = rect(x, y, 46.0, 12.0);
        p.rect_filled(rr, 6.0 * k, Color32::from_rgb(214, 196, 150));
        for (n, c) in [
            (10.0, (160, 60, 40)),
            (18.0, (40, 40, 40)),
            (26.0, (200, 140, 30)),
        ] {
            p.rect_filled(rect(x + n, y, 4.0, 12.0), 0.0, Color32::from_rgb(c.0, c.1, c.2));
        }
    }
    for (x, y) in [(480.0, 140.0), (640.0, 130.0)] {
        p.rect_filled(rect(x, y, 28.0, 22.0), 4.0 * k, Color32::from_rgb(46, 92, 170));
    }
    // the diodes, highlighted
    let d = def
        .knob_index("diodes")
        .map(|i| b.v[i].round().clamp(0.0, 3.0) as usize)
        .unwrap_or(0);
    let zone = rect(330.0, 222.0, 170.0, 62.0);
    radial(
        p,
        zone.center(),
        zone.width() * 0.8,
        zone.height() * 1.2,
        rgba(242, 181, 60, 0.12),
    );
    p.rect_stroke(
        zone,
        8.0 * k,
        Stroke::new(1.5, rgba(242, 181, 60, 0.75)),
        egui::StrokeKind::Outside,
    );
    let n = if d == 1 { 3 } else { 2 };
    for j in 0..n {
        let cx = zone.min.x + zone.width() * (j as f32 + 0.5) / n as f32;
        let c = pos2(cx, zone.center().y);
        // legs
        p.line_segment(
            [c - vec2(0.0, 26.0 * k), c + vec2(0.0, 26.0 * k)],
            Stroke::new(2.0 * k, Color32::from_rgb(190, 192, 196)),
        );
        match d {
            2 => {
                // a red LED, lit as the pedal clips
                let lit = 0.25 + 0.75 * glow;
                radial(p, c, 30.0 * k, 30.0 * k, rgba(255, 40, 30, 0.5 * lit));
                p.circle_filled(c, 11.0 * k, Color32::from_rgb(200, 30, 30));
                p.circle_filled(c - vec2(3.0, 3.0) * k, 4.0 * k, rgba(255, 200, 190, 0.6 * lit));
            }
            3 => {
                // germanium: a bigger clear glass body, a copper band
                let body = Rect::from_center_size(c, vec2(14.0, 30.0) * k);
                p.rect_filled(body, 6.0 * k, rgba(220, 210, 180, 0.55));
                p.rect_filled(
                    Rect::from_min_size(body.min, vec2(body.width(), 6.0 * k)),
                    0.0,
                    Color32::from_rgb(184, 115, 51),
                );
            }
            _ => {
                // small orange glass with a black band (one pair the other way round)
                let body = Rect::from_center_size(c, vec2(10.0, 24.0) * k);
                p.rect_filled(body, 4.0 * k, Color32::from_rgb(222, 120, 40));
                let flip = j % 2 == 1 && d == 0;
                let band_y = if flip { body.max.y - 5.0 * k } else { body.min.y };
                p.rect_filled(
                    Rect::from_min_size(pos2(body.min.x, band_y), vec2(body.width(), 5.0 * k)),
                    0.0,
                    Color32::from_rgb(20, 20, 20),
                );
            }
        }
    }
    let words = match d {
        0 => "CLIPPING DIODES \u{b7} A SILICON PAIR",
        1 => "CLIPPING DIODES \u{b7} ASYMMETRIC (2 + 1)",
        2 => "CLIPPING DIODES \u{b7} TWO LEDS",
        _ => "CLIPPING DIODES \u{b7} GERMANIUM",
    };
    tag(
        p,
        pos2(zone.center().x, zone.min.y - 16.0 * k),
        words,
        fs(9.0),
        Color32::from_rgb(245, 196, 94),
        true,
    );
}

impl Rack {
    /// The Front panel / Back panel tabs (Outside / Inside for a pedal) at the left of the face card's bar; returns
    /// where the bar carries on.
    pub(super) fn back_tabs(&mut self, ui: &Ui, p: &Painter, bar: Rect, blk: Block) -> f32 {
        let amp = blk.def().kind == Kind::Amp;
        let labels: [String; 2] = if amp {
            ["Front panel".into(), "Back panel".into()]
        } else {
            ["Outside".into(), "Inside".into()]
        };
        let w = if amp { px(210.0) } else { px(150.0) };
        let tr = Rect::from_min_size(bar.min, vec2(w, bar.height()));
        let rects = pills(p, tr, &labels, Some(self.board.back as usize), 10.0);
        for (n, rr) in rects.iter().enumerate() {
            let resp = interact(ui, *rr, ("board-back-tab", n), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.board.back = n == 1;
                self.board.back_hot = None;
            }
        }
        tr.max.x + px(12.0)
    }

    /// The art behind the panel in `area`, its tubes (or LEDs) glowing with the board's output.
    pub(super) fn back_art(&mut self, ui: &Ui, st: &mut AppState, area: Rect, blk: Block) {
        let p = ui.painter().clone();
        let level = if st.audio_mode == crate::state::AudioMode::Shots {
            0.85
        } else if self.board_mode() {
            self.engine()
                .map(|e| (e.meters().tone_peak * 1.6).clamp(0.0, 1.0))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let target = if blk.on {
            if st.audio_mode == crate::state::AudioMode::Shots {
                level
            } else {
                0.3 + 0.7 * level
            }
        } else {
            0.0
        };
        let g = &mut self.board.glow;
        *g += (target - *g) * 0.25;
        if (target - *g).abs() > 0.01 {
            ui.ctx().request_repaint();
        } else {
            *g = target;
        }
        let glow = *g;
        if blk.def().kind == Kind::Amp {
            amp_back(&p, area, &blk, glow);
        } else {
            drive_inside(&p, area, &blk, glow);
        }
    }

    /// A back-panel knob or slider turned with the mouse (drag; a slider's track jumps where clicked; double-click
    /// for the default; the wheel). True while it's under the mouse or being turned.
    fn back_drag(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        blk: Block,
        k: usize,
        hit: Rect,
        track: Option<Rect>,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let kd = blk.def().knobs[k];
        let across = track.is_some();
        let len = track.map(|t| t.width()).unwrap_or(160.0);
        self.board.wheel.push(WheelSpot {
            uid: blk.uid,
            knob: k,
            rect: hit,
        });
        let resp = interact(ui, hit, ("board-back", blk.uid, k), Sense::click_and_drag());
        let hov = resp.hovered() || resp.dragged();
        if hov {
            ctx.set_cursor_icon(if across {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::ResizeVertical
            });
        }
        if resp.double_clicked() {
            self.board.drag = None;
            self.set_knob(st, blk.uid, k, kd.def);
            self.board_commit(st);
            return hov;
        }
        let mine = self
            .board
            .drag
            .is_some_and(|d| d.tag == "back" && d.uid == blk.uid && d.knob == k);
        if resp.is_pointer_button_down_on()
            && !mine
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let u0 = match track {
                Some(tr) => ((pos.x - tr.min.x) / tr.width()).clamp(0.0, 1.0),
                None => kd.to_u(blk.v[k]),
            };
            if across {
                self.set_knob(st, blk.uid, k, kd.from_u(u0));
            }
            self.board.drag = Some(KnobDrag {
                tag: "back",
                uid: blk.uid,
                knob: k,
                start: pos,
                u0,
                across,
                len,
            });
        }
        if let Some(d) = self.board.drag
            && d.tag == "back"
            && d.uid == blk.uid
            && d.knob == k
        {
            if ctx.input(|i| i.pointer.any_down()) {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    let moved = if d.across {
                        pos.x - d.start.x
                    } else {
                        d.start.y - pos.y
                    };
                    let fine = if ctx.input(|i| i.modifiers.shift) {
                        0.25
                    } else {
                        1.0
                    };
                    let v = kd.from_u((d.u0 + moved / d.len.max(20.0) * fine).clamp(0.0, 1.0));
                    if (v - blk.v[k]).abs() > 1e-6 {
                        self.set_knob(st, blk.uid, k, v);
                    }
                }
                return true;
            }
            self.board.drag = None;
            self.board_commit(st);
        }
        hov
    }

    /// A choice behind the panel: one of its options as a key. True while it's under the mouse.
    fn back_pick(&mut self, ui: &Ui, st: &mut AppState, blk: Block, k: usize, n: usize, r: Rect) -> bool {
        let resp = interact(ui, r, ("board-back-pick", blk.uid, k, n), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.set_knob(st, blk.uid, k, n as f32);
            self.board_commit(st);
        }
        resp.hovered()
    }

    /// The card of controls behind the panel, each with what it does.
    pub(super) fn back_card(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Block) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let def = blk.def();
        paint::display(&p, r, Some(18.0), 6.0);
        let amp = def.kind == Kind::Amp;
        let y0 = r.min.y + px(20.0);
        let hr = caps(
            &p,
            if amp { "BEHIND THE PANEL" } else { "INSIDE THE BOX" },
            pos2(r.min.x + px(18.0), y0),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        text::draw(
            &p,
            if amp {
                "the controls a real amp hides inside or on its back: they change how the power stage behaves"
            } else {
                "what a modder would change: the diodes that clip the signal"
            },
            pos2(hr.max.x + px(12.0), y0),
            Align2::LEFT_CENTER,
            Style::new(11.0, 600, th.caption),
            Some(r.max.x - hr.max.x - px(30.0)),
        );
        let inner = Rect::from_min_max(
            pos2(r.min.x + px(18.0), y0 + px(16.0)),
            pos2(r.max.x - px(18.0), r.max.y - px(12.0)),
        );
        let mut hot: Option<usize> = None;
        let title = |x: f32, s: &str| {
            text::draw(
                &p,
                s,
                pos2(x, inner.min.y + px(10.0)),
                Align2::LEFT_CENTER,
                Style::new(14.0, 800, th.text),
                None,
            );
        };
        let caption = |x: f32, y: f32, w: f32, s: &str| {
            let cs = Style::new(11.5, 500, th.text_2);
            let lh = px(16.0);
            let room = ((inner.max.y - y) / lh).floor().max(0.0) as usize;
            for (n, line) in text::wrap(&ctx, s, cs, w).iter().take(room).enumerate() {
                text::draw(
                    &p,
                    line,
                    pos2(x, y + lh * (n as f32 + 0.5)),
                    Align2::LEFT_CENTER,
                    cs,
                    None,
                );
            }
        };
        let value_pill = |c: Pos2, s: &str| {
            let vs = Style::new(11.5, 800, th.accent_ink);
            let w = text::width(&ctx, s, vs) + px(14.0);
            let pr = Rect::from_center_size(c, vec2(w, px(20.0)));
            p.rect_filled(pr, 6.0, Color32::from_rgb(30, 32, 36));
            p.rect_stroke(
                pr,
                6.0,
                Stroke::new(1.0, th.accent.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
            text::draw(&p, s, pr.center(), Align2::CENTER_CENTER, vs, None);
        };
        let ctl_top = inner.min.y + px(26.0);
        if !amp {
            // the diodes: one key each, with what it does
            let Some(k) = def.knob_index("diodes") else {
                return;
            };
            let x = inner.min.x;
            title(x, "Clipping diodes");
            let w = inner.width().min(px(760.0));
            let cols = 2;
            let cw = (w - px(10.0)) / cols as f32;
            let rh = px(40.0);
            let cur = blk.v[k].round() as usize;
            let own = gi::tip(def.id, "diodes").map(|t| t.sweet.0.round() as usize);
            for (n, name) in catalog::DIODES.iter().enumerate() {
                let rr = Rect::from_min_size(
                    pos2(
                        x + (n % cols) as f32 * (cw + px(10.0)),
                        ctl_top + (n / cols) as f32 * (rh + px(6.0)),
                    ),
                    vec2(cw, rh),
                );
                let on = n == cur;
                if self.back_pick(ui, st, blk, k, n, rr) {
                    hot = Some(k);
                }
                p.rect_filled(
                    rr,
                    10.0,
                    if on {
                        th.accent.gamma_multiply(0.14)
                    } else {
                        Color32::from_rgb(43, 47, 52)
                    },
                );
                if on {
                    p.rect_stroke(
                        rr,
                        10.0,
                        Stroke::new(1.2, th.accent.gamma_multiply(0.7)),
                        egui::StrokeKind::Inside,
                    );
                }
                let nr = text::draw(
                    &p,
                    name,
                    pos2(rr.min.x + px(12.0), rr.min.y + px(13.0)),
                    Align2::LEFT_CENTER,
                    Style::new(12.5, 800, if on { th.accent_ink } else { th.text }),
                    None,
                );
                if own == Some(n) {
                    text::draw(
                        &p,
                        "the pedal's own",
                        pos2(nr.max.x + px(8.0), nr.center().y),
                        Align2::LEFT_CENTER,
                        Style::new(10.0, 700, th.caption),
                        None,
                    );
                }
                text::draw(
                    &p,
                    DIODE_WORDS[n],
                    pos2(rr.min.x + px(12.0), rr.max.y - px(12.0)),
                    Align2::LEFT_CENTER,
                    Style::new(10.5, 600, th.caption),
                    Some(rr.width() - px(20.0)),
                );
            }
            let cap_y = ctl_top + 2.0 * (rh + px(6.0)) + px(4.0);
            caption(
                x,
                cap_y,
                w,
                "Swapping diodes changes where and how hard the pedal clips: the Drive knob still sets how much.",
            );
            self.board.back_hot = hot;
            return;
        }
        // an amp's five: sag, bias, power tubes, rectifier, power master
        let idx = |key: &str| def.knob_index(key);
        let (Some(ks), Some(kb), Some(kt), Some(kr), Some(kp)) = (
            idx("sag"),
            idx("bias"),
            idx("tubes"),
            idx("rectifier"),
            idx("power_master"),
        ) else {
            return;
        };
        let fr = [0.17f32, 0.2, 0.28, 0.17, 0.18];
        let gap = px(14.0);
        let total = inner.width() - gap * 4.0;
        let mut xs = [0.0f32; 5];
        let mut x = inner.min.x;
        for (i, f) in fr.iter().enumerate() {
            xs[i] = x;
            x += total * f + gap;
        }
        let ws: Vec<f32> = fr.iter().map(|f| total * f).collect();
        for &x in &xs[1..] {
            p.line_segment(
                [pos2(x - gap / 2.0, inner.min.y), pos2(x - gap / 2.0, inner.max.y)],
                Stroke::new(1.0, Color32::from_white_alpha(14)),
            );
        }
        let ctl_h = ((inner.max.y - ctl_top) * 0.48).clamp(px(56.0), px(96.0));
        let cap_y = ctl_top + ctl_h + px(6.0);
        // sag and power master: knobs
        for (col, k, lo, hi, cap) in [
            (
                0usize,
                ks,
                "tight",
                "spongy",
                "How much the amp gives when you dig in: more squashes and blooms, less is tight.",
            ),
            (
                4,
                kp,
                "",
                "",
                "How hard the power tubes work: the cranked-amp sound at bedroom level.",
            ),
        ] {
            let kd = def.knobs[k];
            title(xs[col], kd.label);
            let size = (ctl_h - px(28.0)).min(ws[col] * 0.5).clamp(px(30.0), px(56.0));
            let c = pos2(xs[col] + ws[col] / 2.0, ctl_top + size / 2.0 + px(4.0));
            let hit = Rect::from_center_size(c, Vec2::splat(size + px(14.0)));
            let hov = self.back_drag(ui, st, blk, k, hit, None);
            if hov {
                hot = Some(k);
            }
            // (the dark themes' baked knobs show hover and a hold)
            let held = self.board.drag.is_some_and(|d| d.uid == blk.uid && d.knob == k);
            kn::knob_in(
                &p,
                c,
                size,
                kd.to_u(blk.v[k]),
                Scale::default(),
                Some(KnobStyle::Chrome),
                qrs_ui::glass::State::of(hov, held),
            );
            value_pill(pos2(c.x, c.y + size / 2.0 + px(14.0)), &kd.text(blk.v[k]));
            let ls = Style::new(10.0, 700, th.caption);
            if !lo.is_empty() {
                text::draw(
                    &p,
                    lo,
                    pos2(xs[col], c.y + size / 2.0 + px(14.0)),
                    Align2::LEFT_CENTER,
                    ls,
                    None,
                );
                text::draw(
                    &p,
                    hi,
                    pos2(xs[col] + ws[col], c.y + size / 2.0 + px(14.0)),
                    Align2::RIGHT_CENTER,
                    ls,
                    None,
                );
            }
            caption(xs[col], cap_y, ws[col], cap);
        }
        // bias: a slider from cold (blue) to hot (orange)
        {
            let k = kb;
            let kd = def.knobs[k];
            title(xs[1], kd.label);
            let cy = ctl_top + ctl_h * 0.45;
            let track = Rect::from_min_max(
                pos2(xs[1] + px(6.0), cy - px(5.0)),
                pos2(xs[1] + ws[1] - px(6.0), cy + px(5.0)),
            );
            hgrad(
                &p,
                track,
                &[
                    Color32::from_rgb(77, 143, 224),
                    Color32::from_rgb(138, 166, 196),
                    Color32::from_rgb(224, 163, 90),
                    Color32::from_rgb(255, 122, 42),
                ],
            );
            for i in 0..11 {
                let tx = track.min.x + track.width() * i as f32 / 10.0;
                p.line_segment(
                    [
                        pos2(tx, track.max.y + px(4.0)),
                        pos2(tx, track.max.y + px(if i % 5 == 0 { 10.0 } else { 7.0 })),
                    ],
                    Stroke::new(1.0, Color32::from_rgb(111, 117, 127)),
                );
            }
            let u = kd.to_u(blk.v[k]);
            let tx = track.min.x + track.width() * u;
            keys::thumb(&p, pos2(tx, cy), px(20.0));
            let hit = track.expand2(vec2(px(8.0), px(14.0)));
            if self.back_drag(ui, st, blk, k, hit, Some(track)) {
                hot = Some(k);
            }
            let word = match blk.v[k] {
                v if v < 2.0 => "cold",
                v if v < 4.5 => "cool",
                v if v <= 5.5 => "as built",
                v if v <= 8.0 => "warm",
                _ => "hot",
            };
            value_pill(
                pos2(
                    tx.clamp(xs[1] + px(40.0), xs[1] + ws[1] - px(40.0)),
                    cy - px(22.0),
                ),
                &format!("{} \u{b7} {word}", kd.text(blk.v[k])),
            );
            let ls = Style::new(10.0, 700, th.caption);
            text::draw(
                &p,
                "cold",
                pos2(track.min.x, track.max.y + px(20.0)),
                Align2::LEFT_CENTER,
                ls,
                None,
            );
            text::draw(
                &p,
                "hot",
                pos2(track.max.x, track.max.y + px(20.0)),
                Align2::RIGHT_CENTER,
                ls,
                None,
            );
            caption(
                xs[1],
                cap_y,
                ws[1],
                "Cold: tighter, a bit gritty on quiet notes. Hot: warmer and fatter, squashes sooner.",
            );
        }
        // the power tubes: a key each
        {
            let k = kt;
            let own = gi::tip(def.id, "tubes").map(|t| t.sweet.0.round() as usize);
            title(xs[2], "Power tubes");
            if let Some(o) = own.and_then(|o| catalog::TUBES.get(o)) {
                text::draw(
                    &p,
                    &format!("the amp's own: {o}"),
                    pos2(xs[2] + ws[2], inner.min.y + px(10.0)),
                    Align2::RIGHT_CENTER,
                    Style::new(10.5, 700, th.caption),
                    None,
                );
            }
            let n = catalog::TUBES.len();
            let rows_h = inner.max.y - ctl_top;
            let rh = ((rows_h - px(4.0) * (n as f32 - 1.0)) / n as f32).clamp(px(16.0), px(30.0));
            let cur = blk.v[k].round() as usize;
            for (j, name) in catalog::TUBES.iter().enumerate() {
                let rr =
                    Rect::from_min_size(pos2(xs[2], ctl_top + j as f32 * (rh + px(4.0))), vec2(ws[2], rh));
                let on = j == cur;
                if self.back_pick(ui, st, blk, k, j, rr) {
                    hot = Some(k);
                }
                p.rect_filled(
                    rr,
                    8.0,
                    if on {
                        rgba(242, 181, 60, 0.12)
                    } else {
                        Color32::from_rgb(43, 47, 52)
                    },
                );
                if on {
                    p.rect_stroke(
                        rr,
                        8.0,
                        Stroke::new(1.0, rgba(242, 181, 60, 0.6)),
                        egui::StrokeKind::Inside,
                    );
                    radial(
                        &p,
                        pos2(rr.min.x + px(14.0), rr.center().y),
                        px(8.0),
                        rh * 0.4,
                        rgba(255, 154, 58, 0.85),
                    );
                }
                let nr = text::draw(
                    &p,
                    name,
                    pos2(rr.min.x + px(28.0), rr.center().y),
                    Align2::LEFT_CENTER,
                    Style::new(
                        12.0,
                        800,
                        if on {
                            Color32::from_rgb(245, 196, 94)
                        } else {
                            th.text
                        },
                    ),
                    None,
                );
                text::draw(
                    &p,
                    TUBE_WORDS[j],
                    pos2(nr.max.x + px(8.0), rr.center().y),
                    Align2::LEFT_CENTER,
                    Style::new(10.5, 600, th.caption),
                    Some(rr.max.x - nr.max.x - px(14.0)),
                );
            }
        }
        // the rectifier: tube or solid state
        {
            let k = kr;
            title(xs[3], "Rectifier");
            let seg = Rect::from_min_size(pos2(xs[3], ctl_top + px(6.0)), vec2(ws[3], px(30.0)));
            let labels: Vec<String> = catalog::RECTIFIERS.iter().map(|s| s.to_string()).collect();
            let cur = blk.v[k].round() as usize;
            let rects = pills(&p, seg, &labels, Some(cur), 10.5);
            for (n, rr) in rects.iter().enumerate() {
                if self.back_pick(ui, st, blk, k, n, *rr) {
                    hot = Some(k);
                }
            }
            text::draw(
                &p,
                if cur == 0 {
                    "a valve rectifier: adds to Sag"
                } else {
                    "silicon diodes: a stiff supply"
                },
                pos2(xs[3], seg.max.y + px(14.0)),
                Align2::LEFT_CENTER,
                Style::new(10.5, 600, th.caption),
                Some(ws[3]),
            );
            caption(
                xs[3],
                cap_y,
                ws[3],
                "Tube: the supply droops on big chords, a looser feel. Solid: firm and punchy.",
            );
        }
        // (screenshots: a back knob shown as hovered)
        if hot.is_none()
            && let Some(key) = self.board.shot_hover.as_deref()
            && let Some(k) = def.knob_index(key)
            && def.knobs[k].back
        {
            hot = Some(k);
        }
        self.board.back_hot = hot;
    }
}
