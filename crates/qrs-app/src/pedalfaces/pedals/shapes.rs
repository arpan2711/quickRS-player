//! The units with a shape of their own: the round fuzz with its smile (template F), the sandcast wedge, the
//! rocket, the amp-top treble booster, the wah's treadle (template G), the wide envelope filter and the transistor
//! radio. After the mockups' `grin_fuzz`, `carnaby`, `rocket`, `top_shelf`, `tear_jerker`, `quack_box` and
//! `pocket_radio`.

use eframe::egui::{Align2, Color32, Pos2, Stroke, vec2};

use super::super::Lettering;
use super::face::{ChK, Chs, Lab, LabPos, Led, Look, Pr, RED, Spec, Sw, kn, led};
use super::kit::{self, C, Cv, Finish, Ks};

pub fn of(id: &str) -> Option<Spec> {
    Some(match id {
        "round_fuzz" => grin_fuzz(),
        "bender" => carnaby(),
        "octave_fuzz" => rocket(),
        "treble_boost" => top_shelf(),
        "wah" => tear_jerker(),
        "env_filter" => quack_box(),
        "lofi" => pocket_radio(),
        _ => return None,
    })
}

fn look(c: C, ink: C, finish: Finish) -> Look {
    Look {
        c,
        ink,
        finish,
        ..Default::default()
    }
}

// -- the round fuzz -----------------------------------------------------------------------------------------------------------

const D: f32 = 352.0;

fn grin_fuzz() -> Spec {
    let mut s = Spec::new("round_fuzz", D, D, grin_body, grin_art);
    s.look = look(0xc4303c, 0x1a0a0b, Finish::Grain);
    s.radius = D / 2.0;
    let lab = Lab::new(LabPos::Below, 8.0, 0x1a0a0b).gap(8.0);
    s.knobs = vec![
        kn(1, 0.33 * D, 0.32 * D, 26.0, Ks::SilverCap, lab),
        kn(0, 0.67 * D, 0.32 * D, 26.0, Ks::SilverCap, lab),
    ];
    s.led = led(D / 2.0, 0.3 * D, 2.6);
    s.switch = Sw::Stomp(D / 2.0, 0.55 * D, 27.0);
    s
}

/// The smile: a band from 12 to 168 degrees (0 = right, clockwise) between radii r1 and r2, round ended.
fn smile(cx: f32, cy: f32, r1: f32, r2: f32) -> Option<tiny_skia::Path> {
    let (a0, a1) = (12.0f32, 168.0f32);
    let hr = (r2 - r1) / 2.0;
    let rm = (r1 + r2) / 2.0;
    let mut pts = kit::arc_pts(cx, cy, r2, a0, a1, 48);
    let cap = |a: f32, from_outer: bool| -> Vec<(f32, f32)> {
        let t = a.to_radians();
        let (u, tn) = ((t.cos(), t.sin()), (-t.sin(), t.cos()));
        let c = (cx + rm * u.0, cy + rm * u.1);
        (1..12)
            .map(|i| {
                let ph = std::f32::consts::PI * i as f32 / 12.0;
                let (cs, sn) = (ph.cos(), ph.sin());
                if from_outer {
                    (
                        c.0 + hr * (cs * u.0 + sn * tn.0),
                        c.1 + hr * (cs * u.1 + sn * tn.1),
                    )
                } else {
                    (
                        c.0 + hr * (-cs * u.0 - sn * tn.0),
                        c.1 + hr * (-cs * u.1 - sn * tn.1),
                    )
                }
            })
            .collect()
    };
    pts.extend(cap(a1, true));
    pts.extend(kit::arc_pts(cx, cy, r1, a1, a0, 48));
    pts.extend(cap(a0, false));
    kit::poly(&pts)
}

fn grin_body(cv: &mut Cv, s: &Spec) {
    let c = s.look.c;
    let (cx, cy) = (D / 2.0, D / 2.0);
    cv.circle(cx, cy, D / 2.0, &kit::col(0x0b0b0c, 1.0));
    let r = D / 2.0 - 3.2;
    if let Some(p) = kit::circle(cx, cy, r) {
        cv.fill(
            &p,
            &kit::rad_box(
                cx - r,
                cy - r,
                2.0 * r,
                2.0 * r,
                0.36,
                0.3,
                0.8,
                &[
                    (0.0, kit::lt(c, 0.2), 1.0),
                    (0.55, c, 1.0),
                    (1.0, kit::dk(c, 0.35), 1.0),
                ],
            ),
        );
        let m = cv.mask(&p);
        Finish::Grain.apply(cv, &m, (cx - r, cy - r, 2.0 * r, 2.0 * r), &p);
    }
    cv.ring(cx, cy, D / 2.0 - 4.2, &kit::col(0xffffff, 0.25), 1.2);
    cv.ring(cx, cy, D / 2.0 - 12.0, &kit::col(0, 0.08), 5.0);
    let (r1, r2) = (0.2 * D, 0.32 * D);
    if let Some(band) = smile(cx, cy, r1, r2) {
        if let Some(sh) = band
            .clone()
            .transform(tiny_skia::Transform::from_translate(1.0, 3.0))
        {
            cv.fill(&sh, &kit::col(0, 0.4));
        }
        cv.fill(
            &band,
            &kit::lin(
                0.0,
                cy,
                0.0,
                cy + r2,
                &[(0.0, 0xf1f3f5, 1.0), (0.5, 0xb8bec5, 1.0), (1.0, 0x8a9097, 1.0)],
            ),
        );
        let m = cv.mask(&band);
        kit::brushed(cv, &m, (cx - r2, cy, 2.0 * r2, r2 + 4.0), 21, 1.0);
        cv.stroke(&band, &kit::col(0x4a4f55, 1.0), 0.9);
    }
    let rs = 0.26 * D;
    for a in [19.0f32, 161.0] {
        let t = a.to_radians();
        kit::screw(cv, cx + rs * t.cos(), cy + rs * t.sin(), 2.6, a, true);
    }
}

fn grin_art(pr: &Pr) {
    kit::text_arc(
        pr.p,
        &pr.f,
        "GRIN FUZZ",
        D / 2.0,
        D / 2.0,
        0.27 * D,
        180.0,
        21.0,
        5.0,
        Color32::from_rgb(17, 17, 17),
        Lettering::Heavy,
        true,
    );
}

// -- the sandcast wedge -----------------------------------------------------------------------------------------------------

const CW: f32 = 231.0;
const CH: f32 = 308.0;
const CINSET: f32 = (231.0 - 184.8) / 2.0;

fn carnaby() -> Spec {
    let mut s = Spec::new("bender", CW, CH, carnaby_body, carnaby_art);
    s.look = look(0x6b7078, 0x111111, Finish::Grain);
    s.radius = 10.0;
    let lab = Lab::new(LabPos::Below, 7.0, 0x111111).gap(3.0);
    s.knobs = vec![
        kn(0, 0.33 * CW, 0.155 * CH, 20.0, Ks::Chrome, lab),
        kn(1, 0.67 * CW, 0.155 * CH, 20.0, Ks::Chrome, lab),
    ];
    // no light on the real one: a small one where the knobs leave room
    s.led = led(0.5 * CW, 0.12 * CH, 2.2);
    s.switch = Sw::Stomp(0.5 * CW, 0.79 * CH, 27.0);
    s
}

fn carnaby_body(cv: &mut Cv, s: &Spec) {
    let Some(shape) = kit::rpoly(
        &[(CINSET, 0.0), (CW - CINSET, 0.0), (CW, CH), (0.0, CH)],
        &[10.0, 10.0, 9.0, 9.0],
    ) else {
        return;
    };
    cv.fill(&shape, &kit::body_grad(s.look.c, 0.0, 0.0, CW, CH));
    let m = cv.mask(&shape);
    kit::specks(
        cv,
        &m,
        (0.0, 0.0, CW, CH),
        15.0,
        &[(0, 0.28), (0, 0.16), (0xffffff, 0.08)],
        0.4,
        1.1,
        5,
        false,
    );
    cv.fill(&shape, &kit::matte(0.0, 0.0, CW, CH));
    cv.stroke(&shape, &kit::col(0, 0.6), 1.1);
    let sil = 0xe1e4e8;
    let (lx, ly, lw, lh) = (0.2 * CW, 0.35 * CH, 0.6 * CW, 0.2 * CH);
    cv.rect(lx + 0.6, ly + 1.6, lw, lh, 3.0, &kit::col(0, 0.45));
    cv.rect(lx, ly, lw, lh, 3.0, &kit::col(0x141416, 1.0));
    if let Some(p) = kit::rrect(lx, ly, lw, lh, 3.0) {
        cv.stroke(&p, &kit::col(sil, 0.75), 1.0);
    }
    if let Some(p) = kit::rrect(lx + 3.0, ly + 3.0, lw - 6.0, lh - 6.0, 2.0) {
        cv.stroke(&p, &kit::col(sil, 0.35), 0.6);
    }
    for (x, y) in [
        (lx + 5.0, ly + 5.0),
        (lx + lw - 5.0, ly + 5.0),
        (lx + 5.0, ly + lh - 5.0),
        (lx + lw - 5.0, ly + lh - 5.0),
    ] {
        kit::screw(cv, x, y, 1.8, 40.0, false);
    }
}

fn carnaby_art(pr: &Pr) {
    let sil = 0xe1e4e8;
    let (ly, lw) = (0.35 * CH, 0.6 * CW);
    pr.ta(
        "Carnaby",
        CW / 2.0,
        ly + 22.0,
        Align2::CENTER_CENTER,
        30.0,
        sil,
        Lettering::Script,
        lw - 12.0,
    );
    pr.t(
        "FUZZ  \u{b7}  MK II",
        CW / 2.0,
        ly + 43.0,
        Align2::CENTER_CENTER,
        8.5,
        sil,
        Lettering::Condensed,
        lw - 12.0,
    );
    pr.t(
        "PROFESSIONAL",
        CW / 2.0,
        ly + 53.0,
        Align2::CENTER_CENTER,
        5.0,
        sil,
        Lettering::Condensed,
        lw - 12.0,
    );
}

// -- the rocket -------------------------------------------------------------------------------------------------------------------

const OW: f32 = 158.4;
const OH: f32 = 275.0;
const ONY: f32 = 0.25 * OH;

fn rocket() -> Spec {
    let mut s = Spec::new("octave_fuzz", OW, OH, rocket_body, rocket_art);
    s.look = look(0x6c3ea6, 0xf3edfb, Finish::Gloss);
    s.ext = [24.0, 0.0, 24.0, 0.0];
    s.radius = 12.0;
    let lab = Lab::new(LabPos::Above, 6.8, 0xf3edfb).gap(4.5);
    s.knobs = vec![
        kn(0, 0.29 * OW, 0.41 * OH, 17.0, Ks::Mxr, lab),
        kn(1, 0.71 * OW, 0.41 * OH, 17.0, Ks::Mxr, lab),
    ];
    s.led = Some(Led {
        x: OW / 2.0,
        y: 0.15 * OH,
        r: 3.0,
        c: RED,
        jewel: false,
        bezel: false,
    });
    s.switch = Sw::Stomp(OW / 2.0, 0.82 * OH, 20.0);
    s
}

fn rocket_shape() -> Option<tiny_skia::Path> {
    let mut pts: Vec<(f32, f32)> = (0..=40)
        .map(|i| {
            let t = std::f32::consts::PI * (1.0 + i as f32 / 40.0);
            (OW / 2.0 + OW / 2.0 * t.cos(), ONY + ONY * t.sin())
        })
        .collect();
    pts.push((OW, OH - 6.0));
    pts.push((OW - 1.8, OH - 1.8));
    pts.push((OW - 6.0, OH));
    pts.push((6.0, OH));
    pts.push((1.8, OH - 1.8));
    pts.push((0.0, OH - 6.0));
    kit::poly(&pts)
}

fn rocket_body(cv: &mut Cv, s: &Spec) {
    let c = s.look.c;
    let fins = [
        format!(
            "M{} {} C{} {} {} {} {} {} L{} {} Z",
            OW - 2.0,
            0.6 * OH,
            OW + 14.0,
            0.72 * OH,
            OW + 22.0,
            0.86 * OH,
            OW + 24.0,
            OH,
            OW - 2.0,
            OH
        ),
        format!(
            "M2 {} C-14 {} -22 {} -24 {} L2 {} Z",
            0.6 * OH,
            0.72 * OH,
            0.86 * OH,
            OH,
            OH
        ),
    ];
    for f in &fins {
        if let Some(p) = kit::svg_path(f, 0.0, 0.0, 1.0) {
            if let Some(sh) = p
                .clone()
                .transform(tiny_skia::Transform::from_translate(1.0, 3.0))
            {
                cv.fill(&sh, &kit::col(0, 0.4));
            }
            cv.fill(
                &p,
                &kit::lin(
                    -24.0,
                    0.6 * OH,
                    OW + 24.0,
                    OH,
                    &[(0.0, kit::lt(c, 0.1), 1.0), (1.0, kit::dk(c, 0.45), 1.0)],
                ),
            );
            cv.fill(&p, &kit::gloss(-24.0, 0.6 * OH, OW + 48.0, 0.4 * OH));
            cv.stroke(&p, &kit::col(0, 0.6), 1.0);
        }
    }
    let Some(shape) = rocket_shape() else { return };
    kit::enclosure(
        cv,
        &shape,
        (0.0, 0.0, OW, OH),
        &kit::body_grad(c, 0.0, 0.0, OW, OH),
        Finish::Gloss,
    );
    // the nose seam and its rivets
    cv.rect(1.0, ONY - 0.6, OW - 2.0, 1.2, 0.0, &kit::col(0, 0.4));
    cv.rect(1.0, ONY + 0.8, OW - 2.0, 0.8, 0.0, &kit::col(0xffffff, 0.25));
    for i in 0..7 {
        let x = 10.0 + i as f32 * (OW - 20.0) / 6.0;
        cv.circle(x, ONY + 5.0, 1.3, &kit::chrome(x - 1.3, ONY + 3.7, 2.6, 2.6));
    }
    // the porthole: the LED shines through its glass
    let (px, py) = (OW / 2.0, 0.15 * OH);
    cv.circle(px + 0.6, py + 1.5, 17.0, &kit::col(0, 0.45));
    cv.circle(px, py, 17.0, &kit::chrome(px - 17.0, py - 17.0, 34.0, 34.0));
    cv.ring(px, py, 17.0, &kit::col(0x33373c, 1.0), 0.7);
    for i in 0..8 {
        let a = (i as f32 * 45.0 + 22.5).to_radians();
        cv.circle(
            px + 14.4 * a.cos(),
            py + 14.4 * a.sin(),
            1.2,
            &kit::col(0x5a6067, 1.0),
        );
    }
    cv.circle(px, py, 12.0, &kit::col(0x1a0e26, 1.0));
}

fn rocket_art(pr: &Pr) {
    let (px, py) = (OW / 2.0, 0.15 * OH);
    if pr.on() {
        pr.p.circle_filled(
            pr.f.p(px, py),
            pr.f.s(11.0),
            Color32::from_rgba_unmultiplied(255, 59, 48, 60),
        );
    }
    // the glass over the light (drawn after the lens by `face`, so only its glint here)
    pr.p.circle_filled(
        pr.f.p(px - 4.0, py - 5.0),
        pr.f.s(3.0),
        Color32::from_white_alpha(40),
    );
    pr.ta(
        "Rocket",
        OW / 2.0,
        0.538 * OH,
        Align2::CENTER_CENTER,
        27.0,
        0xffffff,
        Lettering::Brush,
        OW * 0.86,
    );
    pr.ta(
        "Octave",
        OW / 2.0,
        0.638 * OH,
        Align2::CENTER_CENTER,
        20.0,
        0xf2e9ff,
        Lettering::Brush,
        OW * 0.7,
    );
}

// -- the amp-top treble booster ------------------------------------------------------------------------------------------------

const SW: f32 = 330.0;
const SH: f32 = 206.8;
const SFH: f32 = 36.0;

fn top_shelf() -> Spec {
    let mut s = Spec::new("treble_boost", SW, SH, shelf_body, shelf_art);
    s.look = look(0x8d939b, 0x16171a, Finish::Hammer);
    s.ext = [0.0, 0.0, 34.0, 40.0];
    s.radius = 6.0;
    s.knobs = vec![
        kn(
            0,
            84.0,
            98.0,
            28.0,
            Ks::SilverCap,
            Lab::new(LabPos::Below, 7.5, 0x16171a).gap(6.0),
        ),
        kn(
            1,
            198.0,
            98.0,
            15.0,
            Ks::Chrome,
            Lab::new(LabPos::Above, 7.5, 0x16171a).gap(6.0),
        ),
    ];
    s.led = Some(Led {
        x: 275.0,
        y: 98.0,
        r: 7.5,
        c: 0xf0352e,
        jewel: true,
        bezel: true,
    });
    s.switch = Sw::Slide(52.0, SH - 22.0, 34.0, 7.0);
    s
}

fn shelf_body(cv: &mut Cv, s: &Spec) {
    let (w, h, fh) = (SW, SH, SFH);
    // the cable out of the right wall, down to its plug
    let cy0 = h - fh + 14.0;
    let cable = format!(
        "M{} {} C{} {} {} {} {} {}",
        w - 2.0,
        cy0,
        w + 26.0,
        cy0,
        w + 22.0,
        h + 2.0,
        w + 20.0,
        h + 12.0
    );
    if let Some(p) = kit::svg_path(&cable, 0.0, 0.0, 1.0) {
        cv.stroke(&p, &kit::col(0x0e0e0f, 1.0), 7.0);
        if let Some(hl) = p
            .clone()
            .transform(tiny_skia::Transform::from_translate(-1.0, -1.4))
        {
            cv.stroke(&hl, &kit::col(0x45474c, 1.0), 1.6);
        }
    }
    cv.rect(w - 3.0, cy0 - 6.5, 12.0, 13.0, 3.0, &kit::col(0x1a1b1d, 1.0));
    let (px, py) = (w + 20.0, h + 10.0);
    cv.rect(
        px - 7.0,
        py,
        14.0,
        22.0,
        2.5,
        &kit::chrome_h(px - 7.0, py, 14.0, 22.0),
    );
    for k in 0..3 {
        cv.rect(
            px - 7.0,
            py + 5.0 + k as f32 * 4.0,
            14.0,
            1.2,
            0.0,
            &kit::col(0x5a6067, 1.0),
        );
    }
    cv.rect(
        px - 3.0,
        py + 22.0,
        6.0,
        8.0,
        1.0,
        &kit::chrome_h(px - 3.0, py + 22.0, 6.0, 8.0),
    );
    // the front face, then the sloping top
    if let Some(front) = kit::rpoly(
        &[(0.0, h - fh), (w, h - fh), (w, h), (0.0, h)],
        &[2.0, 2.0, 5.0, 5.0],
    ) {
        cv.fill(
            &front,
            &kit::lin(0.0, h - fh, 0.0, h, &[(0.0, 0x6c7279, 1.0), (1.0, 0x4a4f55, 1.0)]),
        );
        let m = cv.mask(&front);
        kit::hammer(cv, &m, (0.0, h - fh, w, fh), 3);
        cv.stroke(&front, &kit::col(0, 0.6), 1.1);
    }
    if let Some(top) = kit::rpoly(
        &[(12.0, 0.0), (w - 12.0, 0.0), (w, h - fh), (0.0, h - fh)],
        &[6.0, 6.0, 3.0, 3.0],
    ) {
        kit::enclosure(
            cv,
            &top,
            (0.0, 0.0, w, h - fh),
            &kit::body_grad(s.look.c, 0.0, 0.0, w, h - fh),
            Finish::Hammer,
        );
    }
    cv.rect(4.0, h - fh + 0.5, w - 8.0, 1.0, 0.0, &kit::col(0xffffff, 0.25));
    // the black label strip
    cv.rect(32.0, 13.5, w - 64.0, 42.0, 4.0, &kit::col(0, 0.45));
    cv.rect(32.0, 12.0, w - 64.0, 42.0, 4.0, &kit::col(0x111214, 1.0));
    if let Some(p) = kit::rrect(32.0, 12.0, w - 64.0, 42.0, 4.0) {
        cv.stroke(&p, &kit::col(0xc9cdd3, 0.7), 1.0);
    }
    for (x, y, a) in [
        (38.0, 18.0, 30.0),
        (w - 38.0, 18.0, 60.0),
        (38.0, 48.0, 100.0),
        (w - 38.0, 48.0, 10.0),
    ] {
        kit::screw(cv, x, y, 2.0, a, false);
    }
    // the range scale
    for (j, a) in [-120.0f32, 0.0, 120.0].iter().enumerate() {
        let (x0, y0) = kit::polar(198.0, 98.0, 18.0, *a);
        let (x1, y1) = kit::polar(198.0, 98.0, 22.0, *a);
        cv.line(
            x0,
            y0,
            x1,
            y1,
            &kit::col(0x16171a, 1.0),
            if j == 1 { 1.0 } else { 1.3 },
        );
    }
}

fn shelf_art(pr: &Pr) {
    let (w, h) = (SW, SH);
    pr.ta(
        "Top Shelf",
        w / 2.0 - 22.0,
        33.0,
        Align2::CENTER_CENTER,
        28.0,
        0xeef0f2,
        Lettering::Script,
        150.0,
    );
    pr.t(
        "TREBLE",
        w - 46.0,
        26.0,
        Align2::RIGHT_CENTER,
        9.0,
        0xd9dce0,
        Lettering::Condensed,
        60.0,
    );
    pr.t(
        "BOOSTER",
        w - 46.0,
        39.0,
        Align2::RIGHT_CENTER,
        9.0,
        0xd9dce0,
        Lettering::Condensed,
        60.0,
    );
    pr.t(
        "LO",
        198.0 - 22.0,
        98.0 + 16.0,
        Align2::CENTER_CENTER,
        5.6,
        0x16171a,
        Lettering::Condensed,
        14.0,
    );
    pr.t(
        "HI",
        198.0 + 22.0,
        98.0 + 16.0,
        Align2::CENTER_CENTER,
        5.6,
        0x16171a,
        Lettering::Condensed,
        14.0,
    );
    pr.t(
        "PILOT",
        275.0,
        118.0,
        Align2::CENTER_CENTER,
        6.0,
        0x16171a,
        Lettering::Condensed,
        40.0,
    );
    pr.t(
        "OFF",
        36.0,
        h - 18.5,
        Align2::CENTER_CENTER,
        6.5,
        0xe6e8eb,
        Lettering::Condensed,
        20.0,
    );
    pr.t(
        "ON",
        102.0,
        h - 18.5,
        Align2::CENTER_CENTER,
        6.5,
        0xe6e8eb,
        Lettering::Condensed,
        20.0,
    );
    pr.ta(
        "AMP-TOP UNIT \u{b7} quickRS",
        w - 14.0,
        h - 18.5,
        Align2::RIGHT_CENTER,
        6.0,
        0xd9dce0,
        Lettering::Condensed,
        150.0,
    );
}

// -- the wah ------------------------------------------------------------------------------------------------------------------------

const AW: f32 = 220.0;
const AH: f32 = 561.0;
const WALL: f32 = 15.0;
const TX0: f32 = WALL + 2.0;
const TX1: f32 = AW - WALL - 2.0;
const TY0: f32 = 0.05 * AH;
const TY1: f32 = 0.85 * AH;

fn tear_jerker() -> Spec {
    let mut s = Spec::new("wah", AW, AH, wah_body, wah_art);
    s.look = look(0x1d1e21, 0xc9cdd3, Finish::Crinkle);
    s.radius = 22.0;
    let tread_r = (TX1 - TX0) / 2.0 - 6.0;
    let lab = Lab::new(LabPos::Below, 6.0, 0xc9cdd3).gap(3.0);
    s.knobs = vec![
        // the treadle itself is the Pedal knob: drag it up for the toe
        kn(0, AW / 2.0, (TY0 + TY1) / 2.0, tread_r, Ks::Hidden, Lab::none()),
        kn(2, 132.0, 518.0, 11.0, Ks::Mxr, lab),
        kn(3, 184.0, 518.0, 11.0, Ks::Mxr, lab),
    ];
    s.choices = vec![Chs {
        i: 1,
        x: 18.0,
        y: 496.0,
        w: 72.0,
        h: 44.0,
        k: ChK::Toggle,
        lab: Lab::new(LabPos::None, 6.0, 0xc9cdd3),
        opts: &["PEDAL", "AUTO"],
        ink: 0xc9cdd3,
    }];
    s.led = led(AW - WALL / 2.0, 0.07 * AH, 2.4);
    s.switch = Sw::Area(TX0 + 10.0, TY0 + 6.0, TX1 - TX0 - 20.0, 30.0);
    s
}

fn wah_body(cv: &mut Cv, s: &Spec) {
    let (w, h) = (AW, AH);
    let Some(shell) = kit::rrect(0.0, 0.0, w, h - 5.0, 22.0) else {
        return;
    };
    kit::enclosure(
        cv,
        &shell,
        (0.0, 0.0, w, h - 5.0),
        &kit::body_grad(s.look.c, 0.0, 0.0, w, h),
        Finish::Crinkle,
    );
    cv.rect(
        3.0,
        h - 9.0,
        w - 6.0,
        9.0,
        3.0,
        &kit::chrome_v(3.0, h - 9.0, w - 6.0, 9.0),
    );
    let m = cv.mask(&shell);
    cv.fill_in(
        &tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, WALL, h).unwrap()),
        &kit::col(0, 0.22),
        &m,
    );
    cv.fill_in(
        &tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(w - WALL, 0.0, WALL, h).unwrap()),
        &kit::col(0xffffff, 0.05),
        &m,
    );
    // the treadle and its diamond tread
    cv.rect(TX0, TY0 + 3.0, TX1 - TX0, TY1 - TY0, 14.0, &kit::col(0, 0.55));
    cv.rect(
        TX0,
        TY0,
        TX1 - TX0,
        TY1 - TY0,
        14.0,
        &kit::lin(0.0, TY0, 0.0, TY1, &[(0.0, 0x3a3c41, 1.0), (1.0, 0x121315, 1.0)]),
    );
    if let Some(p) = kit::rrect(TX0, TY0, TX1 - TX0, TY1 - TY0, 14.0) {
        cv.stroke(&p, &kit::col(0, 1.0), 1.0);
    }
    let (rx0, rx1, ry0, ry1) = (TX0 + 8.0, TX1 - 8.0, TY0 + 10.0, TY1 - 12.0);
    if let Some(p) = kit::rrect(rx0, ry0, rx1 - rx0, ry1 - ry0, 9.0) {
        let m = cv.mask(&p);
        kit::diamond(cv, &m, (rx0, ry0, rx1 - rx0, ry1 - ry0));
        cv.stroke(&p, &kit::col(0x0a0a0b, 1.0), 1.0);
    }
    // the hinge axle and its nuts
    let ay = 0.865 * h;
    cv.rect(4.0, ay - 5.0, w - 8.0, 10.0, 3.0, &kit::col(0x0b0b0c, 1.0));
    cv.rect(4.0, ay - 5.0, w - 8.0, 3.0, 2.0, &kit::col(0x3a3c40, 0.7));
    for x in [6.0f32, w - 6.0] {
        let hex: Vec<(f32, f32)> = (0..6)
            .map(|i| {
                let a = (60.0 * i as f32).to_radians();
                (x + 7.5 * a.cos(), ay + 7.5 * a.sin())
            })
            .collect();
        if let Some(p) = kit::poly(&hex) {
            cv.fill(&p, &kit::chrome(x - 7.5, ay - 7.5, 15.0, 15.0));
            cv.stroke(&p, &kit::col(0x3d4147, 1.0), 0.6);
        }
        cv.circle(x, ay, 3.4, &kit::dome(x, ay, 3.4));
    }
    // the gauge on the left wall
    let (gx, g0, g1) = (7.5f32, 0.1 * h, 0.8 * h);
    for i in 0..11 {
        let t = i as f32 / 10.0;
        let y = g1 - (g1 - g0) * t;
        let xx = gx + 2.0 - 24.0 * t * (1.0 - t) * 0.25;
        let big = i % 5 == 0;
        cv.rect(
            xx - if big { 3.0 } else { 1.8 },
            y - 0.4,
            if big { 6.0 } else { 3.6 },
            0.8,
            0.0,
            &kit::col(0x7d8188, 1.0),
        );
    }
}

fn wah_art(pr: &Pr) {
    let (w, h) = (AW, AH);
    let u = pr.u(0);
    // the treadle's tilt: lit at the toe when it's down, at the heel when it's back
    let tread = pr.f.rect(TX0 + 2.0, TY0 + 2.0, TX1 - TX0 - 4.0, TY1 - TY0 - 4.0);
    let (top, bottom) = (
        Color32::from_white_alpha((10.0 + 36.0 * u) as u8),
        Color32::from_black_alpha((20.0 + 60.0 * u) as u8),
    );
    kit::vgrad(pr.p, tread, top, bottom);
    // the moulded script, raised
    let ty = 0.25 * h;
    for (dx, dy, c) in [(1.2, 1.6, 0x060607), (-0.8, -0.9, 0x4a4c51), (0.0, 0.0, 0x2e3034)] {
        pr.rot(
            "Tear Jerker",
            w / 2.0 + dx,
            ty + dy,
            40.0,
            c,
            Lettering::Script,
            -8.0,
        );
    }
    for (dx, dy, c) in [(0.8, 1.0, 0x060607), (-0.5, -0.6, 0x46484d), (0.0, 0.0, 0x2e3034)] {
        pr.ta(
            "quickRS",
            w / 2.0 + dx,
            0.775 * h + dy,
            Align2::CENTER_CENTER,
            12.0,
            c,
            Lettering::Heavy,
            120.0,
        );
    }
    // the gauge's pointer
    let (gx, g0, g1) = (7.5f32, 0.1 * h, 0.8 * h);
    let yp = g1 - (g1 - g0) * u;
    let xp = gx + 2.0 - 24.0 * u * (1.0 - u) * 0.25 + 5.0;
    let tri: Vec<Pos2> = vec![
        pr.f.p(xp - 4.0, yp - 3.2),
        pr.f.p(xp + 3.0, yp),
        pr.f.p(xp - 4.0, yp + 3.2),
    ];
    pr.p.circle_filled(
        pr.f.p(xp, yp),
        pr.f.s(6.0),
        Color32::from_rgba_unmultiplied(242, 181, 60, 50),
    );
    pr.p.add(eframe::egui::Shape::convex_polygon(
        tri,
        Color32::from_rgb(242, 181, 60),
        Stroke::NONE,
    ));
    pr.rot("TOE", 6.0, g0 - 10.0, 5.5, 0x9aa0aa, Lettering::Condensed, -90.0);
    pr.rot("HEEL", 6.0, g1 + 12.0, 5.5, 0x9aa0aa, Lettering::Condensed, -90.0);
    pr.t(
        "MODE",
        54.0,
        492.0,
        Align2::CENTER_CENTER,
        6.0,
        0xc9cdd3,
        Lettering::Condensed,
        60.0,
    );
}

// -- the envelope filter ------------------------------------------------------------------------------------------------------

const QW: f32 = 473.0;
const QH: f32 = 314.6;
const PURPLE: C = 0x7a52c7;

fn quack_box() -> Spec {
    let mut s = Spec::new("env_filter", QW, QH, quack_body, quack_art);
    s.look = look(0xb7bdc4, 0xd9dce1, Finish::Brushed);
    s.radius = 14.0;
    let lab = Lab::new(LabPos::Below, 7.5, 0xd9dce1).gap(8.0);
    s.knobs = vec![
        kn(0, 86.0, 120.0, 25.0, Ks::SilverCap, lab),
        kn(1, 196.0, 120.0, 25.0, Ks::SilverCap, lab),
        kn(2, 390.0, 104.0, 11.0, Ks::Mxr, Lab::none()),
        kn(3, 390.0, 154.0, 11.0, Ks::Mxr, Lab::none()),
    ];
    s.choices = vec![Chs {
        i: 4,
        x: 281.0,
        y: 80.0,
        w: 70.0,
        h: 70.0,
        k: ChK::Rotary(Ks::Chicken(false)),
        lab: Lab::new(LabPos::At(316.0, 158.0), 7.5, 0xd9dce1),
        opts: &["LP", "BP"],
        ink: 0xd9dce1,
    }];
    s.led = led(QW - 30.0, 16.0 + 190.0 - 14.0, 2.8);
    s.switch = Sw::Area(QW / 2.0 - 62.0, 224.0, 124.0, 60.0);
    s
}

fn quack_body(cv: &mut Cv, s: &Spec) {
    let (w, h) = (QW, QH);
    let Some(shell) = kit::rrect(0.0, 0.0, w, h, 14.0) else {
        return;
    };
    kit::enclosure(
        cv,
        &shell,
        (0.0, 0.0, w, h),
        &kit::body_grad(s.look.c, 0.0, 0.0, w, h),
        Finish::Brushed,
    );
    let (px, py, pw, ph) = (16.0, 16.0, w - 32.0, 190.0);
    cv.rect(px, py + 1.5, pw, ph, 9.0, &kit::col(0, 0.45));
    cv.rect(px, py, pw, ph, 9.0, &kit::col(0x1e2026, 1.0));
    cv.rect(px, py, pw, ph, 9.0, &kit::satin(px, py, pw, ph));
    if let Some(p) = kit::rrect(px, py, pw, ph, 9.0) {
        cv.stroke(&p, &kit::col(0, 1.0), 1.0);
    }
    cv.rect(px + 8.0, py + 8.0, pw - 16.0, 44.0, 7.0, &kit::col(PURPLE, 1.0));
    cv.rect(
        px + 8.0,
        py + 8.0,
        pw - 16.0,
        44.0,
        7.0,
        &kit::gloss(px + 8.0, py + 8.0, pw - 16.0, 44.0),
    );
    // the attack and release wells
    for i in 0..2 {
        let (bx, by) = (372.0, 86.0 + i as f32 * 50.0);
        cv.rect(bx, by, 76.0, 36.0, 5.0, &kit::col(0x0e0f12, 1.0));
        if let Some(p) = kit::rrect(bx, by, 76.0, 36.0, 5.0) {
            cv.stroke(&p, &kit::col(0x5d636a, 1.0), 1.2);
        }
        cv.rect(bx + 1.5, by + 1.5, 73.0, 10.0, 4.0, &kit::col(0xffffff, 0.05));
    }
    // the bar switch
    let (fx, fy, fw, fh) = (w / 2.0 - 62.0, 224.0, 124.0, 60.0);
    cv.rect(fx + 1.0, fy + 4.0, fw, fh, 7.0, &kit::col(0, 0.5));
    cv.rect(
        fx - 4.0,
        fy - 4.0,
        fw + 8.0,
        fh + 8.0,
        9.0,
        &kit::col(0x2a2c30, 1.0),
    );
    cv.rect(fx, fy, fw, fh, 7.0, &kit::chrome_v(fx, fy, fw, fh));
    for k in 0..9 {
        let y = fy + 9.0 + k as f32 * 5.0;
        cv.rect(fx + 10.0, y, fw - 20.0, 1.6, 0.8, &kit::col(0x5a6067, 1.0));
        cv.rect(fx + 10.0, y + 1.6, fw - 20.0, 0.6, 0.3, &kit::col(0xffffff, 0.6));
    }
    for (x, y) in [(28.0, h - 24.0), (w - 28.0, h - 24.0)] {
        kit::screw(cv, x, y, 4.0, 30.0, true);
    }
}

fn quack_art(pr: &Pr) {
    let (px, py, pw) = (16.0, 16.0, QW - 32.0);
    pr.ta(
        "quack box",
        px + 22.0,
        py + 30.0,
        Align2::LEFT_CENTER,
        34.0,
        0xffffff,
        Lettering::Rounded,
        200.0,
    );
    pr.t(
        "ENVELOPE FOLLOWER",
        px + pw - 22.0,
        py + 25.0,
        Align2::RIGHT_CENTER,
        7.0,
        0xefe8fb,
        Lettering::Condensed,
        150.0,
    );
    pr.ta(
        "FILTER \u{b7} quickRS",
        px + pw - 22.0,
        py + 37.0,
        Align2::RIGHT_CENTER,
        7.0,
        0xefe8fb,
        Lettering::Condensed,
        150.0,
    );
    let def = pr.b.def();
    for (i, (lo, hi)) in [(2usize, ("FAST", "SLOW")), (3, ("SHORT", "LONG"))] {
        let (bx, by) = (372.0, 86.0 + (i - 2) as f32 * 50.0);
        pr.t(
            def.knobs[i].label,
            bx + 34.0,
            by + 13.0,
            Align2::LEFT_CENTER,
            6.8,
            0xd9dce1,
            Lettering::Condensed,
            40.0,
        );
        pr.t(
            &format!("{lo} \u{2013} {hi}"),
            bx + 34.0,
            by + 25.0,
            Align2::LEFT_CENTER,
            5.2,
            0x8f96a0,
            Lettering::Condensed,
            40.0,
        );
    }
    pr.t(
        "BYPASS",
        60.0,
        256.0,
        Align2::CENTER_CENTER,
        7.5,
        0x2a2c30,
        Lettering::Condensed,
        60.0,
    );
    pr.ta(
        "quickRS",
        QW - 60.0,
        256.0,
        Align2::CENTER_CENTER,
        8.5,
        0x2a2c30,
        Lettering::Heavy,
        70.0,
    );
}

// -- the radio -------------------------------------------------------------------------------------------------------------------

const YW: f32 = 330.0;
const YH: f32 = 253.0;
const DX0: f32 = 30.0;
const DX1: f32 = YW - 30.0;
const DY: f32 = 25.0;
const RIGHT: f32 = 20.0 + 0.55 * YW + 18.0;

fn pocket_radio() -> Spec {
    let mut s = Spec::new("lofi", YW, YH, radio_body, radio_art);
    s.look = look(0xe2c043, 0x3a3020, Finish::Gloss);
    s.radius = 24.0;
    let lab = Lab::new(LabPos::Below, 7.0, 0x3a3020).gap(6.0);
    s.knobs = vec![
        kn(1, RIGHT + 20.0, 112.0, 21.0, Ks::Wheel, lab),
        kn(2, RIGHT + 76.0, 112.0, 21.0, Ks::Wheel, lab),
    ];
    s.choices = vec![Chs {
        i: 0,
        x: DX0 - 3.0,
        y: DY - 0.5,
        w: DX1 - DX0 + 6.0,
        h: 27.0,
        k: ChK::Art,
        lab: Lab::none(),
        opts: &[],
        ink: 0xb0261f,
    }];
    s.led = led(RIGHT + 18.0, 205.0, 2.6);
    s.switch = Sw::Stomp(YW - 38.0, 206.0, 17.0);
    s
}

fn radio_body(cv: &mut Cv, s: &Spec) {
    let (w, h) = (YW, YH);
    let Some(shell) = kit::rrect(0.0, 0.0, w, h, 24.0) else {
        return;
    };
    kit::enclosure(
        cv,
        &shell,
        (0.0, 0.0, w, h),
        &kit::body_grad(s.look.c, 0.0, 0.0, w, h),
        Finish::Gloss,
    );
    // the dial band
    cv.rect(
        14.0,
        14.0,
        w - 28.0,
        52.0,
        11.0,
        &kit::chrome_v(14.0, 14.0, w - 28.0, 52.0),
    );
    cv.rect(17.0, 17.0, w - 34.0, 46.0, 9.0, &kit::col(0xf3ead0, 1.0));
    cv.rect(
        DX0 - 6.0,
        DY - 3.0,
        DX1 - DX0 + 12.0,
        32.0,
        5.0,
        &kit::col(0x2a2416, 1.0),
    );
    for i in 0..=40 {
        let x = DX0 + (DX1 - DX0) * i as f32 / 40.0;
        let big = i % 5 == 0;
        cv.rect(
            x - 0.35,
            DY + 2.0,
            0.7,
            if big { 6.0 } else { 3.5 },
            0.0,
            &kit::col(0x3a3020, 1.0),
        );
    }
    // the grille
    let (gx, gy, gw, gh) = (20.0, 78.0, 0.55 * w, h - 96.0);
    cv.rect(
        gx - 3.0,
        gy - 3.0,
        gw + 6.0,
        gh + 6.0,
        11.0,
        &kit::chrome_v(gx - 3.0, gy - 3.0, gw + 6.0, gh + 6.0),
    );
    cv.rect(gx, gy, gw, gh, 9.0, &kit::col(0xd9cfb1, 1.0));
    if let Some(p) = kit::rrect(gx, gy, gw, gh, 9.0) {
        let m = cv.mask(&p);
        let mut y = gy + 4.0;
        while y < gy + gh {
            let mut x = gx + 4.0;
            while x < gx + gw {
                if let Some(c) = kit::circle(x, y, 2.15) {
                    cv.fill_in(&c, &kit::col(0x2d2615, 1.0), &m);
                }
                if let Some(c) = kit::circle(x - 0.4, y - 0.5, 1.2) {
                    cv.fill_in(&c, &kit::col(0x14110a, 1.0), &m);
                }
                x += 8.0;
            }
            y += 8.0;
        }
        cv.fill(&p, &kit::satin(gx, gy, gw, gh));
    }
}

fn radio_art(pr: &Pr) {
    let def = pr.b.def();
    let sel = pr.b.v[0].round().clamp(0.0, 2.0) as usize;
    // the dial's window, lit when it's on
    let win = pr.f.rect(DX0 - 3.0, DY - 0.5, DX1 - DX0 + 6.0, 27.0);
    let glass = if pr.on() {
        Color32::from_rgba_unmultiplied(255, 236, 170, 70)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 40)
    };
    pr.p.rect_filled(win, pr.f.s(4.0), glass);
    for (t, fx) in [
        ("55", 0.0),
        ("70", 0.25),
        ("100", 0.5),
        ("140", 0.75),
        ("KC", 1.0),
    ] {
        pr.t(
            t,
            DX0 + (DX1 - DX0) * fx,
            DY + 12.0,
            Align2::CENTER_CENTER,
            4.8,
            0x3a3020,
            Lettering::Condensed,
            20.0,
        );
    }
    for (k, (t, fx)) in [("RADIO", 1.0 / 6.0), ("PHONE", 0.5), ("8-BIT", 5.0 / 6.0)]
        .into_iter()
        .enumerate()
    {
        let c = if k == sel { 0xb0261f } else { 0x8a6a50 };
        pr.t(
            t,
            DX0 + (DX1 - DX0) * fx,
            DY + 20.5,
            Align2::CENTER_CENTER,
            6.8,
            c,
            Lettering::Condensed,
            60.0,
        );
    }
    // the needle at the chosen band
    let nx = DX0 + (DX1 - DX0) * (1.0 + 2.0 * sel as f32) / 6.0;
    pr.p.rect_filled(
        pr.f.rect(nx - 0.9, DY - 1.0, 1.8, 28.0),
        0.0,
        Color32::from_rgb(210, 35, 27),
    );
    pr.t(
        "TYPE",
        YW - 24.0,
        DY + 36.0,
        Align2::RIGHT_CENTER,
        5.8,
        0x3a3020,
        Lettering::Condensed,
        30.0,
    );
    let _ = def;
    let (gx, gy, gw, gh) = (20.0, 78.0, 0.55 * YW, YH - 96.0);
    let (tx, ty) = (gx + gw / 2.0, gy + gh * 0.55);
    pr.rot(
        "Pocket Radio",
        tx + 1.2,
        ty + 2.0,
        32.0,
        0x1a160c,
        Lettering::Script,
        -8.0,
    );
    pr.rot("Pocket Radio", tx, ty, 32.0, 0xe8ecef, Lettering::Script, -8.0);
    pr.t(
        "SIX TRANSISTOR",
        RIGHT + 48.0,
        172.0,
        Align2::CENTER_CENTER,
        5.0,
        0x3a3020,
        Lettering::Condensed,
        80.0,
    );
    pr.t(
        "ON AIR",
        RIGHT + 18.0,
        220.0,
        Align2::CENTER_CENTER,
        6.0,
        0x3a3020,
        Lettering::Condensed,
        40.0,
    );
    let _ = vec2(0.0, 0.0);
}
