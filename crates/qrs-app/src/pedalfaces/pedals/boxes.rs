//! Templates B to E: the small die-cast box (MXR style, 60 x 112 mm), the boutique and big flat boxes, the
//! overdrive with its kidney-shaped switch plate (TS style). After the mockups' `box()` and `ts()`, and the
//! effects mockups' MXR boxes.

use eframe::egui::{Align2, Color32, Stroke};

use super::super::Lettering;
use super::face::{Lab, LabPos, Led, Look, Pr, RED, Spec, Sw, kn, led};
use super::kit::{self, C, Cv, Finish, Ks};

/// A plain box's spec: its size, look and corner radius; jacks out of the side walls at `jack_y` (fraction of
/// the height).
fn boxed(id: &'static str, w: f32, h: f32, look: Look, body: fn(&mut Cv, &Spec), art: fn(&Pr)) -> Spec {
    let mut s = Spec::new(id, w, h, body, art);
    s.look = look;
    s.ext = [8.0, 0.0, 8.0, 0.0];
    s
}

fn look(c: C, ink: C, finish: Finish) -> Look {
    Look {
        c,
        ink,
        finish,
        ..Default::default()
    }
}

/// The enclosure: jacks, the shell in its colour and finish, a light inner edge.
fn shell(cv: &mut Cv, s: &Spec, jack_y: f32) {
    let (w, h) = (s.w, s.h);
    if jack_y > 0.0 {
        kit::jacks(cv, w, jack_y * h, 13.0);
    }
    let Some(p) = kit::rrect(0.0, 0.0, w, h, s.radius) else {
        return;
    };
    kit::enclosure(
        cv,
        &p,
        (0.0, 0.0, w, h),
        &kit::body_grad(s.look.c, 0.0, 0.0, w, h),
        s.look.finish,
    );
    if let Some(p) = kit::rrect(1.0, 1.0, w - 2.0, h - 2.0, (s.radius - 1.0).max(0.0)) {
        cv.stroke(&p, &kit::col(0xffffff, 0.2), 0.9);
    }
}

/// An effects-mockup box: a vertical shade of its colour (light top edge, dark bottom).
fn shell_v(cv: &mut Cv, s: &Spec, jack_y: f32) {
    let (w, h, c) = (s.w, s.h, s.look.c);
    if jack_y > 0.0 {
        kit::jacks(cv, w, jack_y * h, 18.0);
    }
    let Some(p) = kit::rrect(0.0, 0.0, w, h, s.radius) else {
        return;
    };
    // the box's depth under it
    if let Some(d) = kit::rrect(0.0, 4.0, w, h, s.radius) {
        cv.fill(&d, &kit::col(kit::dk(c, 0.5), 1.0));
    }
    cv.fill(
        &p,
        &kit::lin(
            0.0,
            0.0,
            0.0,
            h,
            &[
                (0.0, kit::lt(c, 0.14), 1.0),
                (0.26, c, 1.0),
                (0.7, c, 1.0),
                (1.0, kit::dk(c, 0.14), 1.0),
            ],
        ),
    );
    let m = cv.mask(&p);
    s.look.finish.apply(cv, &m, (0.0, 0.0, w, h), &p);
    cv.stroke(&p, &kit::col(0, 0.35), 1.0);
    if let Some(p) = kit::rrect(1.0, 1.0, w - 2.0, h - 2.0, (s.radius - 1.0).max(0.0)) {
        cv.stroke(&p, &kit::col(0xffffff, 0.3), 0.9);
    }
}

fn screws4(cv: &mut Cv, w: f32, h: f32, inset: f32, r: f32) {
    for (i, (x, y)) in [
        (inset, inset),
        (w - inset, inset),
        (inset, h - inset),
        (w - inset, h - inset),
    ]
    .into_iter()
    .enumerate()
    {
        kit::screw(cv, x, y, r, 20.0 + 40.0 * i as f32, true);
    }
}

fn above(size: f32, c: C) -> Lab {
    Lab::new(LabPos::Above, size, c)
}

fn below(size: f32, c: C) -> Lab {
    Lab::new(LabPos::Below, size, c)
}

pub fn of(id: &str) -> Option<Spec> {
    Some(match id {
        "squeeze" => squeeze(),
        "bass_comp" => low_lock(),
        "boost" => echo_pre(),
        "green" => mean_green(),
        "gold" => gold_pegasus(),
        "germanium" => copper_top(),
        "rodent" => rodent(),
        "bass_drive" => bass_pilot(),
        "big_pi" => wall_of_wool(),
        "phaser" => script_phase(),
        "flanger" => jet117(),
        "analog_delay" => graphite_echo(),
        "mod_delay" => mod_delay(),
        "room" => room(),
        "hall" => hall(),
        _ => return None,
    })
}

// -- dynamics ---------------------------------------------------------------------------------------------------------

const MW: f32 = 132.0;
const MH: f32 = 246.4;

fn squeeze() -> Spec {
    let mut s = boxed(
        "squeeze",
        MW,
        MH,
        look(0xc8202a, 0x111111, Finish::Gloss),
        squeeze_body,
        squeeze_art,
    );
    s.radius = MW * 0.045;
    s.knobs = vec![
        kn(
            2,
            0.27 * MW,
            0.15 * MH,
            13.0,
            Ks::Mxr,
            below(6.2, 0x111111).gap(3.0),
        ),
        kn(
            0,
            0.73 * MW,
            0.15 * MH,
            13.0,
            Ks::Mxr,
            below(6.2, 0x111111).gap(3.0),
        ),
        kn(1, 0.5 * MW, 0.3 * MH, 7.2, Ks::Mxr, below(5.6, 0x111111).gap(2.0)),
    ];
    s.led = led(0.5 * MW, 0.062 * MH, 2.3);
    s.switch = Sw::Stomp(0.5 * MW, 0.82 * MH, 16.5);
    s
}

fn squeeze_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.22);
    cv.rect(
        0.1 * MW,
        0.415 * MH,
        0.44 * MW,
        2.2,
        0.0,
        &kit::col(0x111111, 1.0),
    );
}

fn squeeze_art(pr: &Pr) {
    pr.t(
        "QRS",
        0.1 * MW,
        0.4 * MH - 6.0,
        Align2::LEFT_CENTER,
        17.0,
        0x111111,
        Lettering::Heavy,
        60.0,
    );
    pr.rot(
        "squeeze",
        MW / 2.0 + 2.0,
        0.55 * MH,
        30.0,
        0x111111,
        Lettering::Brush,
        -7.0,
    );
    pr.t(
        "COMPRESSOR",
        MW / 2.0,
        0.645 * MH,
        Align2::CENTER_CENTER,
        6.4,
        0x111111,
        Lettering::Condensed,
        MW * 0.8,
    );
}

const LW: f32 = 206.8;
const LH: f32 = 261.8;

fn low_lock() -> Spec {
    let mut s = boxed(
        "bass_comp",
        LW,
        LH,
        look(0x39424d, 0xb8c4d0, Finish::Grain),
        low_lock_body,
        low_lock_art,
    );
    s.radius = LW * 0.045;
    let lab = |c: C| below(6.0, c).gap(3.0);
    let cyan = 0x38c6e8;
    s.knobs = vec![
        kn(0, 0.27 * LW, 0.17 * LH, 20.0, Ks::Modern(cyan), lab(0xb8c4d0)),
        kn(3, 0.73 * LW, 0.17 * LH, 20.0, Ks::Modern(0xffffff), lab(0xb8c4d0)),
        kn(1, 0.27 * LW, 0.41 * LH, 20.0, Ks::Modern(cyan), lab(0xb8c4d0)),
        kn(2, 0.73 * LW, 0.41 * LH, 20.0, Ks::Modern(0xffffff), lab(0xb8c4d0)),
    ];
    s.led = Some(Led {
        x: 0.5 * LW,
        y: 0.765 * LH,
        r: 2.4,
        c: 0xeaf6ff,
        jewel: false,
        bezel: true,
    });
    s.switch = Sw::Stomp(0.5 * LW, 0.87 * LH, 21.0);
    s
}

fn low_lock_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.22);
    cv.rect(
        0.14 * LW,
        0.585 * LH,
        0.72 * LW,
        0.6,
        0.0,
        &kit::col(0xcfd8e2, 0.5),
    );
    cv.rect(
        0.14 * LW,
        0.705 * LH,
        0.72 * LW,
        0.6,
        0.0,
        &kit::col(0xcfd8e2, 0.5),
    );
}

fn low_lock_art(pr: &Pr) {
    // the gain-reduction bar: amber and green lit while it's on
    let (bx, by) = (LW / 2.0 - 4.0, 0.12 * LH);
    for (i, (c, lit)) in [(0xf0645a, false), (0xf2b53c, true), (0x3ec46d, true)]
        .into_iter()
        .enumerate()
    {
        let y = by + i as f32 * 26.0;
        let on = lit && pr.on();
        let r = pr.f.rect(bx, y, 8.0, 22.0);
        if on {
            pr.p.rect_filled(r.expand(pr.f.s(2.5)), 4.0, kit::c32a(c, 0.3));
        }
        pr.p.rect_filled(r, pr.f.s(2.5), kit::c32(if on { c } else { kit::mix(c, 0, 0.7) }));
        pr.p.rect_filled(
            pr.f.rect(bx + 1.5, y + 1.5, 2.0, 9.0),
            1.0,
            Color32::from_white_alpha(if on { 120 } else { 30 }),
        );
    }
    pr.t(
        "GR",
        LW / 2.0,
        by + 84.0,
        Align2::CENTER_CENTER,
        5.5,
        0x8fa0b0,
        Lettering::Condensed,
        20.0,
    );
    pr.t(
        "LOW LOCK",
        LW / 2.0,
        0.645 * LH - 5.0,
        Align2::CENTER_CENTER,
        15.0,
        0xf1f4f7,
        Lettering::Wide,
        LW * 0.72,
    );
    pr.t(
        "MULTIBAND BASS COMPRESSOR",
        LW / 2.0,
        0.685 * LH,
        Align2::CENTER_CENTER,
        5.6,
        0x8fa0b0,
        Lettering::Condensed,
        LW * 0.72,
    );
}

const EW: f32 = 83.6;
const EH: f32 = 198.0;
const BROWN: C = 0x3b2a1c;

fn echo_pre() -> Spec {
    let mut s = boxed(
        "boost",
        EW,
        EH,
        look(0xd9d3c4, BROWN, Finish::Gloss),
        echo_pre_body,
        echo_pre_art,
    );
    s.radius = EW * 0.06;
    s.knobs = vec![
        kn(
            0,
            0.5 * EW,
            0.215 * EH,
            18.0,
            Ks::Mxr,
            Lab::new(LabPos::At(EW / 2.0, 0.34 * EH), 6.4, BROWN),
        ),
        kn(
            1,
            0.5 * EW,
            0.455 * EH,
            7.5,
            Ks::Mxr,
            Lab::new(LabPos::At(EW / 2.0, 0.53 * EH), 5.8, BROWN),
        ),
    ];
    s.led = Some(Led {
        x: 0.5 * EW,
        y: 0.055 * EH,
        r: 2.7,
        c: 0x3aa0ff,
        jewel: false,
        bezel: true,
    });
    s.switch = Sw::Stomp(0.5 * EW, 0.855 * EH, 14.5);
    s
}

fn echo_pre_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.3);
    let p = kit::col(BROWN, 1.0);
    cv.rect(0.12 * EW, 0.585 * EH, 0.76 * EW, 0.8, 0.0, &p);
    cv.rect(0.12 * EW, 0.594 * EH, 0.76 * EW, 0.5, 0.0, &p);
    cv.rect(0.12 * EW, 0.717 * EH, 0.76 * EW, 0.5, 0.0, &p);
    cv.rect(0.12 * EW, 0.725 * EH, 0.76 * EW, 0.8, 0.0, &p);
}

fn echo_pre_art(pr: &Pr) {
    pr.t(
        "ECHO",
        EW / 2.0,
        0.622 * EH,
        Align2::CENTER_CENTER,
        15.0,
        BROWN,
        Lettering::Condensed,
        EW * 0.76,
    );
    pr.t(
        "PRE",
        EW / 2.0,
        0.692 * EH,
        Align2::CENTER_CENTER,
        15.0,
        BROWN,
        Lettering::Condensed,
        EW * 0.76,
    );
}

// -- drive ------------------------------------------------------------------------------------------------------------------

const TW: f32 = 162.8;
const TH: f32 = 272.8;

fn mean_green() -> Spec {
    let mut s = boxed(
        "green",
        TW,
        TH,
        look(0x3f8a4a, 0xf4f4f0, Finish::Satin),
        mean_green_body,
        mean_green_art,
    );
    s.radius = TW * 0.07;
    let lab = above(6.2, 0xf4f4f0).gap(4.0);
    s.knobs = vec![
        kn(0, 0.25 * TW, 0.135 * TH, 16.0, Ks::Mxr, lab.text("OVERDRIVE")),
        kn(2, 0.75 * TW, 0.135 * TH, 16.0, Ks::Mxr, lab),
        kn(1, 0.5 * TW, 0.265 * TH, 11.5, Ks::Mxr, lab),
    ];
    s.led = led(TW / 2.0, 0.07 * TH, 2.4);
    let (x0, x1) = (0.065 * TW, 0.935 * TW);
    s.switch = Sw::Area(x0, 0.422 * TH, x1 - x0, 0.556 * TH);
    s
}

fn kidney() -> Option<tiny_skia::Path> {
    let (w, h) = (TW, TH);
    let (x0, x1) = (0.065 * w, 0.935 * w);
    let mut b = tiny_skia::PathBuilder::new();
    b.move_to(x0, 0.53 * h);
    b.cubic_to(x0, 0.462 * h, 0.2 * w, 0.428 * h, 0.5 * w, 0.422 * h);
    b.cubic_to(0.8 * w, 0.428 * h, x1, 0.462 * h, x1, 0.53 * h);
    b.cubic_to(x1 - 0.025 * w, 0.68 * h, x1 - 0.025 * w, 0.8 * h, x1, 0.915 * h);
    b.quad_to(x1, 0.978 * h, 0.84 * w, 0.978 * h);
    b.line_to(0.16 * w, 0.978 * h);
    b.quad_to(x0, 0.978 * h, x0, 0.915 * h);
    b.cubic_to(x0 + 0.025 * w, 0.8 * h, x0 + 0.025 * w, 0.68 * h, x0, 0.53 * h);
    b.close();
    b.finish()
}

fn mean_green_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.2);
    let Some(d) = kidney() else { return };
    if let Some(sh) = d
        .clone()
        .transform(tiny_skia::Transform::from_translate(0.0, 2.0))
    {
        cv.fill(&sh, &kit::col(0, 0.4));
    }
    cv.fill(&d, &kit::body_grad(s.look.c, 0.0, 0.42 * TH, TW, 0.56 * TH));
    cv.fill(&d, &kit::gloss(0.0, 0.42 * TH, TW, 0.56 * TH));
    cv.stroke(&d, &kit::col(0, 0.6), 1.3);
    let m = cv.mask(&d);
    if let Some(d2) = d
        .clone()
        .transform(tiny_skia::Transform::from_translate(0.0, 1.1))
    {
        cv.stroke_in(&d2, &kit::col(0xffffff, 0.3), 0.8, &m);
    }
    let ink = s.look.ink;
    cv.rect(0.17 * TW, 0.535 * TH, 0.66 * TW, 0.9, 0.0, &kit::col(ink, 0.8));
    cv.rect(0.17 * TW, 0.618 * TH, 0.66 * TW, 0.9, 0.0, &kit::col(ink, 0.8));
}

fn mean_green_art(pr: &Pr) {
    let ink = 0xf4f4f0;
    pr.t(
        "OVER DRIVE",
        TW / 2.0,
        0.5 * TH,
        Align2::CENTER_CENTER,
        7.4,
        ink,
        Lettering::Condensed,
        TW * 0.6,
    );
    pr.t(
        "MEAN GREEN",
        TW / 2.0,
        0.577 * TH,
        Align2::CENTER_CENTER,
        15.0,
        ink,
        Lettering::Heavy,
        TW * 0.64,
    );
    pr.ta(
        "quickRS",
        TW / 2.0,
        0.915 * TH,
        Align2::CENTER_CENTER,
        8.0,
        ink,
        Lettering::Heavy,
        TW * 0.5,
    );
}

const PW: f32 = 206.8;
const PH: f32 = 259.6;
const OX: C = 0x5a1a1a;

const PEGASUS: &[(&str, f32, bool)] = &[
    (
        "M44 44 C40 30 30 16 12 7 C14 14 18 18 16 22 C22 24 24 26 22 30 C28 31 30 33 28 37 C34 37 36 40 36 43 C40 43 42 44 44 44",
        1.25,
        true,
    ),
    (
        "M40 38 C34 28 26 20 16 12 M36 41 C30 34 24 28 18 22 M42 33 C37 25 30 18 22 13",
        0.65,
        false,
    ),
    (
        "M30 52 C32 44 48 42 60 45 C66 46 70 50 69 56 C66 62 50 64 40 62 C34 61 30 58 30 52 Z",
        1.25,
        false,
    ),
    (
        "M60 45 C64 38 68 30 72 25 C74 21 77 18 80 17 L82 12.5 L83.4 17 C86 17 89 19 92 23 C94 25 93 28 90 28 C87 28 85 27 83 28 C81 30 79 33 78 37 C76 42 72 48 69 52",
        1.25,
        false,
    ),
    (
        "M72 24 C70 28 68 30 66 34 M75 22 C72 27 71 31 68 36 M70 30 C67 34 65 37 63 41",
        0.75,
        false,
    ),
    (
        "M64 58 C70 60 74 58 76 54 C77 51 80 50 81.5 52.5 M76 54 L80.5 55.5",
        1.25,
        false,
    ),
    ("M58 61 C62 66 66 70 70 70 C72 70 73 68 72 66", 1.25, false),
    ("M36 60 C34 66 32 72 30 78 L28 84 L33 84", 1.25, false),
    ("M44 62 C46 68 46 74 43 80 L42 85 L47 85", 1.25, false),
    (
        "M30 53 C22 50 16 56 14 66 C13 72 10 76 8 78 C14 76 18 70 20 64 C22 59 26 57 30 56",
        1.25,
        false,
    ),
    (
        "M36 58 L40 54 M40.5 60.5 L45.5 55 M45.5 61.5 L50.5 56 M50.5 61.5 L55.5 56.5 M55.5 61 L60.5 56.5 M60.5 59.5 L64.5 56",
        0.55,
        false,
    ),
    ("M20 86 L80 86", 0.7, false),
];

fn gold_pegasus() -> Spec {
    let mut l = look(0xc9a54c, OX, Finish::Brushed);
    l.style = Lettering::Engraved;
    let mut s = boxed("gold", PW, PH, l, gold_body, gold_art);
    s.radius = PW * 0.05;
    let lab = above(6.6, OX).style(Lettering::Engraved);
    s.knobs = vec![
        kn(0, 0.2 * PW, 0.17 * PH, 17.0, Ks::Mxr, lab),
        kn(1, 0.5 * PW, 0.17 * PH, 17.0, Ks::Mxr, lab),
        kn(2, 0.8 * PW, 0.17 * PH, 17.0, Ks::Mxr, lab),
    ];
    s.led = led(0.5 * PW, 0.725 * PH, 2.6);
    s.switch = Sw::Stomp(0.5 * PW, 0.865 * PH, 21.0);
    s
}

fn gold_body(cv: &mut Cv, s: &Spec) {
    kit::jacks(cv, PW, 0.22 * PH, 13.0);
    let Some(p) = kit::rrect(0.0, 0.0, PW, PH, s.radius) else {
        return;
    };
    let gold = kit::lin(
        0.0,
        0.0,
        0.8 * PW,
        PH,
        &[
            (0.0, 0xecd58e, 1.0),
            (0.25, 0xcfae58, 1.0),
            (0.5, 0xe2c779, 1.0),
            (0.78, 0xb8953f, 1.0),
            (1.0, 0x8f7230, 1.0),
        ],
    );
    kit::enclosure(cv, &p, (0.0, 0.0, PW, PH), &gold, Finish::Brushed);
    // the winged horse in a ring, engraved
    let sc = 0.82;
    let (dx, dy) = (0.27 * PW - 50.0 * sc, 0.5 * PH - 46.0 * sc);
    let ink = kit::col(OX, 1.0);
    cv.ring(dx + 50.0 * sc, dy + 46.0 * sc, 47.0 * sc, &ink, 1.4 * sc);
    cv.ring(dx + 50.0 * sc, dy + 46.0 * sc, 43.5 * sc, &ink, 0.6 * sc);
    for &(d, w, fill) in PEGASUS {
        if let Some(path) = kit::svg_path(d, dx, dy, sc) {
            if fill {
                cv.fill(&path, &kit::col(OX, 0.12));
            }
            cv.stroke(&path, &ink, w * sc);
        }
    }
    cv.circle(dx + 84.5 * sc, dy + 20.5 * sc, 0.9 * sc, &ink);
    cv.rect(0.53 * PW, 0.465 * PH, 0.38 * PW, 0.7, 0.0, &ink);
    cv.rect(0.53 * PW, 0.555 * PH, 0.38 * PW, 0.7, 0.0, &ink);
}

fn gold_art(pr: &Pr) {
    pr.t(
        "GOLD",
        0.72 * PW,
        0.415 * PH,
        Align2::CENTER_CENTER,
        19.0,
        OX,
        Lettering::Engraved,
        0.38 * PW,
    );
    pr.t(
        "PEGASUS",
        0.72 * PW,
        0.512 * PH,
        Align2::CENTER_CENTER,
        12.5,
        OX,
        Lettering::Engraved,
        0.38 * PW,
    );
    pr.ta(
        "transparent overdrive",
        0.72 * PW,
        0.6 * PH,
        Align2::CENTER_CENTER,
        6.4,
        OX,
        Lettering::Engraved,
        0.4 * PW,
    );
}

const CREAM: C = 0xefe3c6;

fn copper_top() -> Spec {
    let mut s = boxed(
        "germanium",
        PW,
        261.8,
        look(0xb45a2e, CREAM, Finish::Hammer),
        copper_body,
        copper_art,
    );
    let h = 261.8;
    s.radius = PW * 0.045;
    let lab = below(6.2, CREAM).gap(3.0);
    s.knobs = vec![
        kn(0, 0.2 * PW, 0.18 * h, 18.0, Ks::Chrome, lab),
        kn(1, 0.5 * PW, 0.18 * h, 18.0, Ks::Chrome, lab),
        kn(2, 0.8 * PW, 0.18 * h, 18.0, Ks::Chrome, lab),
    ];
    s.led = Some(Led {
        x: 0.5 * PW,
        y: 0.71 * h,
        r: 2.6,
        c: 0xffb000,
        jewel: false,
        bezel: true,
    });
    s.switch = Sw::Stomp(0.5 * PW, 0.855 * h, 21.0);
    s
}

fn copper_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.22);
    let (w, h) = (s.w, s.h);
    screws4(cv, w, h, 9.0, 3.0);
    let (lx, ly, lw, lh) = (0.15 * w, 0.40 * h, 0.70 * w, 0.23 * h);
    cv.rect(lx + 0.6, ly + 1.6, lw, lh, 3.0, &kit::col(0, 0.4));
    cv.rect(lx, ly, lw, lh, 3.0, &kit::col(CREAM, 1.0));
    if let Some(p) = kit::rrect(lx, ly, lw, lh, 3.0) {
        cv.stroke(&p, &kit::col(0x6b3a1c, 1.0), 0.8);
    }
    if let Some(p) = kit::rrect(lx + 3.0, ly + 3.0, lw - 6.0, lh - 6.0, 2.0) {
        cv.stroke(&p, &kit::col(0x1c1a17, 1.0), 0.7);
    }
    cv.rect(lx + 8.0, ly + 9.0, 34.0, 38.0, 1.5, &kit::col(0xfff8e8, 1.0));
    if let Some(p) = kit::rrect(lx + 8.0, ly + 9.0, 34.0, 38.0, 1.5) {
        cv.stroke(&p, &kit::col(0x1c1a17, 1.0), 1.2);
    }
}

fn copper_art(pr: &Pr) {
    let (w, h) = (pr.spec.w, pr.spec.h);
    let (lx, ly, lw, lh) = (0.15 * w, 0.40 * h, 0.70 * w, 0.23 * h);
    let ink = 0x1c1a17;
    pr.t(
        "32",
        lx + 11.0,
        ly + 14.0,
        Align2::LEFT_CENTER,
        6.0,
        ink,
        Lettering::Condensed,
        14.0,
    );
    pr.ta(
        "Ge",
        lx + 25.0,
        ly + 30.0,
        Align2::CENTER_CENTER,
        18.0,
        ink,
        Lettering::Heavy,
        30.0,
    );
    pr.t(
        "72.63",
        lx + 25.0,
        ly + 42.0,
        Align2::CENTER_CENTER,
        4.6,
        ink,
        Lettering::Condensed,
        30.0,
    );
    pr.t(
        "COPPER",
        lx + 50.0,
        ly + 20.0,
        Align2::LEFT_CENTER,
        17.0,
        ink,
        Lettering::Condensed,
        lw - 56.0,
    );
    pr.t(
        "TOP",
        lx + 50.0,
        ly + 37.0,
        Align2::LEFT_CENTER,
        17.0,
        ink,
        Lettering::Condensed,
        lw - 56.0,
    );
    pr.t(
        "GERMANIUM OVERDRIVE",
        lx + lw / 2.0,
        ly + lh - 7.0,
        Align2::CENTER_CENTER,
        5.0,
        ink,
        Lettering::Condensed,
        lw - 10.0,
    );
}

const RW: f32 = 220.0;
const RH: f32 = 259.6;

fn rodent() -> Spec {
    let mut s = boxed(
        "rodent",
        RW,
        RH,
        look(0x1d1e21, 0xf0f0ee, Finish::Crinkle),
        rodent_body,
        rodent_art,
    );
    s.radius = RW * 0.035;
    let lab = above(6.4, 0xf0f0ee);
    s.knobs = vec![
        kn(0, 0.2 * RW, 0.145 * RH, 14.0, Ks::Mxr, lab),
        kn(1, 0.5 * RW, 0.145 * RH, 14.0, Ks::Mxr, lab),
        kn(2, 0.8 * RW, 0.145 * RH, 14.0, Ks::Mxr, lab),
    ];
    s.led = led(0.5 * RW, 0.715 * RH, 2.6);
    s.switch = Sw::Stomp(0.5 * RW, 0.85 * RH, 23.0);
    s
}

fn rodent_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.22);
}

fn rodent_art(pr: &Pr) {
    let r = pr.t(
        "RODENT",
        RW / 2.0,
        0.49 * RH,
        Align2::CENTER_CENTER,
        44.0,
        0xf4f4f2,
        Lettering::Heavy,
        0.8 * RW,
    );
    // worn print: dark specks over the logo
    let n = 70;
    for i in 0..n {
        let x = r.min.x + kit::hash01(i * 3 + 500) * r.width();
        let y = r.min.y + r.height() * (0.2 + 0.6 * kit::hash01(i * 3 + 501));
        let rad = pr.f.s(0.5 + 1.0 * kit::hash01(i * 3 + 502)).max(0.4);
        pr.p.circle_filled(eframe::egui::pos2(x, y), rad, Color32::from_rgb(29, 30, 33));
    }
    pr.plain(
        "distortion",
        RW / 2.0,
        0.6 * RH,
        Align2::CENTER_CENTER,
        10.0,
        0xe8e8e6,
        800,
    );
}

const BW: f32 = 209.0;
const BH: f32 = 264.0;

fn bass_pilot() -> Spec {
    let mut s = boxed(
        "bass_drive",
        BW,
        BH,
        look(0xb9bcbf, 0x151515, Finish::Brushed),
        bass_pilot_body,
        bass_pilot_art,
    );
    s.radius = BW * 0.04;
    let lab = above(6.0, 0x151515);
    s.knobs = vec![
        kn(3, 0.14 * BW, 0.15 * BH, 12.0, Ks::Mxr, lab),
        kn(1, 0.38 * BW, 0.15 * BH, 12.0, Ks::Mxr, lab),
        kn(2, 0.62 * BW, 0.15 * BH, 12.0, Ks::Mxr, lab),
        kn(0, 0.86 * BW, 0.15 * BH, 12.0, Ks::Mxr, lab),
    ];
    s.led = led(0.28 * BW + 34.0, 0.815 * BH, 2.5);
    s.switch = Sw::Stomp(0.28 * BW, 0.815 * BH, 21.0);
    s
}

fn bass_pilot_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.22);
    screws4(cv, BW, BH, 9.0, 3.0);
    cv.rect(
        0.12 * BW,
        0.545 * BH,
        0.76 * BW,
        5.0,
        2.5,
        &kit::col(0x7b6748, 1.0),
    );
    // the XLR socket
    let (cx, cy, r) = (0.74 * BW, 0.815 * BH, 15.0);
    cv.rect(
        cx - r * 1.2 + 0.8,
        cy - r * 1.42 + 1.2,
        r * 2.4,
        r * 2.75,
        3.0,
        &kit::col(0, 0.35),
    );
    cv.rect(
        cx - r * 1.2,
        cy - r * 1.42,
        r * 2.4,
        r * 2.75,
        3.0,
        &kit::chrome(cx - r, cy - r, 2.0 * r, 2.0 * r),
    );
    kit::screw(cv, cx - r * 0.92, cy + r * 1.08, 1.6, 20.0, true);
    kit::screw(cv, cx + r * 0.92, cy - r * 1.14, 1.6, 70.0, true);
    cv.circle(cx, cy, r, &kit::col(0x1b1c1f, 1.0));
    cv.circle(cx, cy, r * 0.84, &kit::col(0x0d0d0f, 1.0));
    for k in 0..3 {
        let a = (-90.0f32 + 120.0 * k as f32 + 60.0).to_radians();
        cv.circle(
            cx + r * 0.45 * a.cos(),
            cy + r * 0.45 * a.sin(),
            r * 0.13,
            &kit::col(0x555a60, 1.0),
        );
    }
}

fn bass_pilot_art(pr: &Pr) {
    pr.t(
        "BASS",
        BW / 2.0,
        0.37 * BH,
        Align2::CENTER_CENTER,
        25.0,
        0x151515,
        Lettering::Rounded,
        BW * 0.7,
    );
    pr.t(
        "PILOT",
        BW / 2.0,
        0.475 * BH,
        Align2::CENTER_CENTER,
        25.0,
        0x151515,
        Lettering::Rounded,
        BW * 0.7,
    );
    pr.t(
        "TUBE-STYLE BASS PREAMP \u{b7} DI",
        BW / 2.0,
        0.595 * BH,
        Align2::CENTER_CENTER,
        5.8,
        0x222222,
        Lettering::Condensed,
        BW * 0.8,
    );
    pr.t(
        "DI OUT",
        0.74 * BW,
        0.815 * BH + 28.0,
        Align2::CENTER_CENTER,
        5.6,
        0x222222,
        Lettering::Condensed,
        40.0,
    );
}

// -- fuzz -----------------------------------------------------------------------------------------------------------------

const WW: f32 = 363.0;
const WH: f32 = 286.0;
const WOOL_RED: C = 0xc8102e;

fn wall_of_wool() -> Spec {
    let mut s = boxed(
        "big_pi",
        WW,
        WH,
        look(0xc3c6ca, 0x111111, Finish::Grain),
        wool_body,
        wool_art,
    );
    s.radius = WW * 0.025;
    let lab = below(7.0, 0x111111).style(Lettering::Heavy).gap(3.0);
    s.knobs = vec![
        kn(2, 0.2 * WW, 0.155 * WH, 21.0, Ks::SilverCap, lab),
        kn(1, 0.5 * WW, 0.155 * WH, 21.0, Ks::SilverCap, lab),
        kn(0, 0.8 * WW, 0.155 * WH, 21.0, Ks::SilverCap, lab),
    ];
    s.led = led(0.5 * WW, 0.72 * WH, 2.8);
    s.switch = Sw::Stomp(0.5 * WW, 0.85 * WH, 25.0);
    s
}

fn wool_body(cv: &mut Cv, s: &Spec) {
    shell(cv, s, 0.22);
    // the ball of wool
    let (bx, by, br) = (0.86 * WW, 0.505 * WH, 30.0);
    cv.circle(bx + 1.0, by + 2.0, br, &kit::col(0, 0.25));
    cv.circle(bx, by, br, &kit::col(0x111111, 1.0));
    let strands = [
        [(-0.9, -0.3), (-0.2, -0.9), (0.5, -0.5), (0.95, 0.1)],
        [(-0.95, 0.05), (-0.2, -0.5), (0.5, -0.1), (0.85, 0.5)],
        [(-0.85, 0.45), (-0.2, -0.05), (0.45, 0.35), (0.6, 0.8)],
        [(-0.3, -0.95), (0.2, -0.3), (0.1, 0.4), (-0.35, 0.93)],
        [(0.25, -0.97), (0.65, -0.3), (0.55, 0.4), (0.2, 0.97)],
    ];
    for st in strands {
        let p = |i: usize| (bx + br * st[i].0, by + br * st[i].1);
        let mut b = tiny_skia::PathBuilder::new();
        b.move_to(p(0).0, p(0).1);
        b.cubic_to(p(1).0, p(1).1, p(2).0, p(2).1, p(3).0, p(3).1);
        if let Some(path) = b.finish() {
            cv.stroke(&path, &kit::col(0xc3c6ca, 1.0), 1.6);
        }
    }
    let mut b = tiny_skia::PathBuilder::new();
    b.move_to(bx + br * 0.6, by + br * 0.8);
    b.cubic_to(
        bx + br * 0.9,
        by + br * 1.3,
        bx + br * 0.2,
        by + br * 1.45,
        bx - br * 0.4,
        by + br * 1.5,
    );
    if let Some(path) = b.finish() {
        cv.stroke(&path, &kit::col(0x111111, 1.0), 2.0);
    }
}

fn wool_art(pr: &Pr) {
    pr.ta(
        "woolly-works",
        0.4 * WW,
        0.315 * WH,
        Align2::CENTER_CENTER,
        15.0,
        WOOL_RED,
        Lettering::Brush,
        WW * 0.5,
    );
    pr.t(
        "WALL OF WOOL",
        0.4 * WW,
        0.5 * WH,
        Align2::CENTER_CENTER,
        34.0,
        0x111111,
        Lettering::Heavy,
        0.68 * WW,
    );
    pr.t(
        "FUZZ \u{b7} WOOL \u{b7} SUSTAINER",
        0.4 * WW,
        0.615 * WH,
        Align2::CENTER_CENTER,
        8.5,
        WOOL_RED,
        Lettering::Condensed,
        0.6 * WW,
    );
}

// -- modulation ---------------------------------------------------------------------------------------------------------------

fn script_phase() -> Spec {
    let ink = 0x1a0b03;
    let mut s = boxed(
        "phaser",
        MW,
        MH,
        look(0xe0561c, ink, Finish::Gloss),
        script_body,
        script_art,
    );
    s.radius = 6.0;
    let lab = |at: (f32, f32)| {
        Lab::new(LabPos::At(at.0, at.1), 10.0, ink)
            .style(Lettering::Script)
            .text("")
    };
    s.knobs = vec![
        kn(
            0,
            MW / 2.0,
            60.0,
            23.0,
            Ks::Mxr,
            lab((MW / 2.0, 28.0)).text("speed"),
        ),
        kn(1, 38.0, 103.0, 11.0, Ks::Mxr, lab((38.0, 121.0)).text("depth")),
        kn(2, 94.0, 103.0, 11.0, Ks::Mxr, lab((94.0, 121.0)).text("mix")),
    ];
    s.led = led(MW / 2.0, 13.0, 2.6);
    s.switch = Sw::Stomp(MW / 2.0, MH * 0.845, 16.0);
    s
}

fn script_body(cv: &mut Cv, s: &Spec) {
    shell_v(cv, s, 0.27);
}

fn script_art(pr: &Pr) {
    let ink = 0x1a0b03;
    pr.rot("Script", 50.0, 147.0, 34.0, ink, Lettering::Script, -7.0);
    pr.rot("Phase", 84.0, 172.0, 34.0, ink, Lettering::Script, -7.0);
}

const JW: f32 = 330.0;
const JH: f32 = 231.0;

fn jet117() -> Spec {
    let ink = 0xf4ecf1;
    let mut s = boxed(
        "flanger",
        JW,
        JH,
        look(0x5a2747, ink, Finish::Gloss),
        jet_body,
        jet_art,
    );
    s.radius = 7.0;
    let lab = Lab::new(LabPos::Above, 9.0, ink).gap(5.0);
    s.knobs = [0usize, 1, 2, 3]
        .iter()
        .zip([60.0f32, 130.0, 200.0, 270.0])
        .map(|(&i, x)| kn(i, x, 60.0, 21.0, Ks::Mxr, lab))
        .collect();
    s.led = led(JW / 2.0 + 52.0, 196.0, 3.6);
    s.switch = Sw::Stomp(JW / 2.0, 196.0, 18.0);
    s
}

fn jet_body(cv: &mut Cv, s: &Spec) {
    shell_v(cv, s, 0.3);
    screws4(cv, JW, JH, 12.0, 4.5);
    cv.rect(24.0, 91.0, JW - 48.0, 2.0, 0.0, &kit::col(0xf4ecf1, 0.85));
    cv.rect(24.0, 95.0, JW - 48.0, 1.0, 0.0, &kit::col(0xf4ecf1, 0.6));
}

fn jet_art(pr: &Pr) {
    pr.t(
        "JET 117",
        JW / 2.0,
        128.0,
        Align2::CENTER_CENTER,
        40.0,
        0xf8f2f5,
        Lettering::Heavy,
        JW * 0.8,
    );
    pr.t(
        "F L A N G E R",
        JW / 2.0,
        161.0,
        Align2::CENTER_CENTER,
        12.0,
        0xf4ecf1,
        Lettering::Condensed,
        JW * 0.6,
    );
}

// -- delay -----------------------------------------------------------------------------------------------------------------------

const SILVER: C = 0xdfe5e1;

fn graphite_echo() -> Spec {
    let mut s = boxed(
        "analog_delay",
        MW,
        MH,
        look(0x1f4a33, SILVER, Finish::Gloss),
        graphite_body,
        graphite_art,
    );
    s.radius = 6.0;
    let lab = |x: f32, y: f32| Lab::new(LabPos::At(x, y), 7.6, SILVER);
    s.knobs = vec![
        kn(1, 36.0, 50.0, 17.0, Ks::Mxr, lab(36.0, 28.0)),
        kn(2, 96.0, 50.0, 17.0, Ks::Mxr, lab(96.0, 28.0)),
        kn(3, MW / 2.0, 49.0, 8.0, Ks::Mxr, lab(MW / 2.0, 33.5)),
        kn(0, MW / 2.0, 98.0, 18.0, Ks::Mxr, lab(MW / 2.0, 125.0)),
    ];
    s.led = led(MW / 2.0, 12.0, 2.6);
    s.switch = Sw::Stomp(MW / 2.0, MH * 0.86, 15.0);
    s
}

fn graphite_body(cv: &mut Cv, s: &Spec) {
    shell_v(cv, s, 0.27);
    // the diagonal pinstripe
    if let Some(p) = kit::rrect(0.0, 0.0, MW, MH, 6.0) {
        let m = cv.mask(&p);
        let mut k = -MH;
        while k < MW {
            if let Some(l) = qrs_ui::raster::polyline(&[(k, MH), (k + MH, 0.0)]) {
                cv.stroke_in(&l, &kit::col(0, 0.09), 3.0, &m);
            }
            k += 9.0;
        }
    }
    kit::led_bezel(cv, MW / 2.0, 66.0, 1.8);
}

fn graphite_art(pr: &Pr) {
    // the modulation LED
    kit::led_lens(pr.p, &pr.f, MW / 2.0, 66.0, 1.8, RED, pr.on() && pr.u(3) > 0.02);
    pr.t(
        "GRAPHITE",
        MW / 2.0,
        150.0,
        Align2::CENTER_CENTER,
        19.0,
        0xf2f5f3,
        Lettering::Heavy,
        MW * 0.86,
    );
    pr.t(
        "ECHO",
        MW / 2.0,
        170.0,
        Align2::CENTER_CENTER,
        15.0,
        0xb8c2bc,
        Lettering::Heavy,
        MW * 0.6,
    );
    pr.t(
        "ANALOG DELAY",
        MW / 2.0,
        186.0,
        Align2::CENTER_CENTER,
        7.5,
        SILVER,
        Lettering::Condensed,
        MW * 0.7,
    );
}

const DW: f32 = 374.0;
const DH: f32 = 264.0;

fn mod_delay() -> Spec {
    let mut s = boxed(
        "mod_delay",
        DW,
        DH,
        look(0xc9ccce, 0x111111, Finish::Brushed),
        mod_delay_body,
        mod_delay_art,
    );
    s.radius = 7.0;
    let xs = [50.0f32, 118.0, 187.0, 256.0, 324.0];
    s.knobs = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let c = if i >= 3 { 0xc81e1e } else { 0x111111 };
            kn(
                i,
                x,
                62.0,
                21.0,
                Ks::Mxr,
                Lab::new(LabPos::Above, 9.5, c).gap(5.0),
            )
        })
        .collect();
    s.led = led(DW / 2.0 + 46.0, 222.0, 3.6);
    s.switch = Sw::Stomp(DW / 2.0, 222.0, 18.0);
    s
}

fn mod_delay_body(cv: &mut Cv, s: &Spec) {
    shell_v(cv, s, 0.3);
    screws4(cv, DW, DH, 12.0, 4.5);
    cv.rect(
        0.0,
        104.0,
        DW,
        12.0,
        0.0,
        &kit::lin(
            0.0,
            104.0,
            0.0,
            116.0,
            &[(0.0, 0x3d6fc4, 1.0), (0.6, 0x2a58a6, 1.0), (1.0, 0x1d437f, 1.0)],
        ),
    );
    cv.rect(0.0, 116.0, DW, 1.0, 0.0, &kit::col(0xffffff, 0.5));
    kit::led_bezel(cv, 326.0, 151.0, 3.0);
}

fn mod_delay_art(pr: &Pr) {
    pr.t(
        "MOD",
        24.0,
        149.0,
        Align2::LEFT_CENTER,
        34.0,
        0xc81e1e,
        Lettering::Heavy,
        100.0,
    );
    pr.t(
        "DELAY",
        118.0,
        149.0,
        Align2::LEFT_CENTER,
        34.0,
        0x141414,
        Lettering::Heavy,
        170.0,
    );
    pr.t(
        "ECHO WITH CHORUS ON THE REPEATS",
        26.0,
        181.0,
        Align2::LEFT_CENTER,
        8.5,
        0x3a3d40,
        Lettering::Condensed,
        260.0,
    );
    kit::led_lens(pr.p, &pr.f, 326.0, 151.0, 3.0, RED, pr.on());
    pr.t(
        "LFO",
        326.0,
        165.0,
        Align2::CENTER_CENTER,
        7.5,
        0x3a3d40,
        Lettering::Condensed,
        30.0,
    );
}

// -- reverb ------------------------------------------------------------------------------------------------------------------------

const OW: f32 = 207.0;
const OH: f32 = 262.0;

fn room() -> Spec {
    let ink = 0xe3e8ef;
    let mut s = boxed(
        "room",
        OW,
        OH,
        look(0x4a5568, ink, Finish::Gloss),
        room_body,
        room_art,
    );
    s.radius = 7.0;
    let lab = Lab::new(LabPos::Above, 8.5, ink).gap(5.0);
    s.knobs = vec![
        kn(0, 40.0, 54.0, 19.0, Ks::Mxr, lab),
        kn(1, 103.5, 54.0, 19.0, Ks::Mxr, lab),
        kn(2, 167.0, 54.0, 19.0, Ks::Mxr, lab),
    ];
    s.led = led(OW / 2.0, 13.0, 2.6);
    s.switch = Sw::Stomp(OW / 2.0, 234.0, 16.0);
    s
}

fn room_body(cv: &mut Cv, s: &Spec) {
    shell_v(cv, s, 0.27);
    // the room seen from above: walls, the amp, its first reflections, a mic
    let (ox, oy) = (24.0, 86.0);
    let ink = 0xe3e8ef;
    cv.rect(ox + 6.0, oy + 6.0, 148.0, 84.0, 2.0, &kit::col(0, 0.18));
    if let Some(p) = kit::rrect(ox + 6.0, oy + 6.0, 148.0, 84.0, 2.0) {
        cv.stroke(&p, &kit::col(ink, 1.0), 2.0);
    }
    if let Some(p) = kit::rrect(ox + 22.0, oy + 18.0, 116.0, 60.0, 1.0) {
        cv.dashed(&p, &kit::col(ink, 0.35), 1.0, 3.0, 3.0);
    }
    cv.rect(ox + 20.0, oy + 56.0, 18.0, 14.0, 2.0, &kit::col(ink, 1.0));
    for (pts, a) in [
        (
            vec![
                (38.0, 63.0),
                (96.0, 6.0),
                (154.0, 52.0),
                (110.0, 90.0),
                (38.0, 63.0),
            ],
            0.6,
        ),
        (vec![(38.0, 60.0), (6.0, 30.0), (70.0, 6.0)], 0.45),
    ] {
        let pts: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (ox + x, oy + y)).collect();
        if let Some(p) = qrs_ui::raster::polyline(&pts) {
            cv.stroke(&p, &kit::col(ink, a), 1.0);
        }
    }
    cv.ring(ox + 124.0, oy + 34.0, 5.0, &kit::col(ink, 1.0), 1.6);
}

fn room_art(pr: &Pr) {
    pr.t(
        "R O O M",
        OW / 2.0,
        201.0,
        Align2::CENTER_CENTER,
        18.0,
        0xe3e8ef,
        Lettering::Wide,
        OW * 0.8,
    );
}

const HW: f32 = 330.0;
const HH: f32 = 264.0;

fn hall() -> Spec {
    let ink = 0xe3e9f4;
    let mut s = boxed(
        "hall",
        HW,
        HH,
        look(0x28406a, ink, Finish::Gloss),
        hall_body,
        hall_art,
    );
    s.radius = 7.0;
    let lab = Lab::new(LabPos::Above, 9.0, ink).gap(5.0);
    s.knobs = [0usize, 1, 2, 3]
        .iter()
        .zip([62.0f32, 131.0, 200.0, 269.0])
        .map(|(&i, x)| kn(i, x, 58.0, 20.0, Ks::Mxr, lab))
        .collect();
    s.led = led(HW / 2.0 + 44.0, 228.0, 3.4);
    s.switch = Sw::Stomp(HW / 2.0, 228.0, 17.0);
    s
}

fn hall_body(cv: &mut Cv, s: &Spec) {
    shell_v(cv, s, 0.3);
    screws4(cv, HW, HH, 12.0, 4.5);
    let (x, y, w, h) = (HW / 2.0 - 80.0, 94.0, 160.0, 56.0);
    cv.rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, 7.0, &kit::col(0x0b0f18, 1.0));
    cv.rect(
        x,
        y,
        w,
        h,
        6.0,
        &kit::lin(x, y, x, y + h, &[(0.0, 0x120607, 1.0), (1.0, 0x1d0a0b, 1.0)]),
    );
}

/// Seven-segment digits.
fn seven_seg(pr: &Pr, s: &str, x: f32, y: f32, h: f32, on: Color32, off: Color32) -> f32 {
    // segments a..g as line pairs in a 0..1 x 0..2 cell
    const SEG: [((f32, f32), (f32, f32)); 7] = [
        ((0.0, 0.0), (1.0, 0.0)),
        ((1.0, 0.0), (1.0, 1.0)),
        ((1.0, 1.0), (1.0, 2.0)),
        ((0.0, 2.0), (1.0, 2.0)),
        ((0.0, 1.0), (0.0, 2.0)),
        ((0.0, 0.0), (0.0, 1.0)),
        ((0.0, 1.0), (1.0, 1.0)),
    ];
    let digit = |c: char| -> u8 {
        match c {
            '0' => 0b0111111,
            '1' => 0b0000110,
            '2' => 0b1011011,
            '3' => 0b1001111,
            '4' => 0b1100110,
            '5' => 0b1101101,
            '6' => 0b1111101,
            '7' => 0b0000111,
            '8' => 0b1111111,
            '9' => 0b1101111,
            _ => 0,
        }
    };
    let u = h / 2.0;
    let cw = u * 1.0;
    let mut cx = x;
    let sk = 0.12; // a slant
    for ch in s.chars() {
        if ch == '.' {
            pr.p.circle_filled(pr.f.p(cx - u * 0.15, y + h), pr.f.s(u * 0.12).max(1.0), on);
            cx += u * 0.25;
            continue;
        }
        let bits = digit(ch);
        for (k, ((x0, y0), (x1, y1))) in SEG.iter().enumerate() {
            let lit = bits & (1 << k) != 0;
            let px = |xx: f32, yy: f32| pr.f.p(cx + xx * cw + (2.0 - yy) * u * sk, y + yy * u);
            // shorten the ends a little so the segments don't touch
            let a = px(x0 + (x1 - x0) * 0.1, y0 + (y1 - y0) * 0.1);
            let b = px(x0 + (x1 - x0) * 0.9, y0 + (y1 - y0) * 0.9);
            pr.p.line_segment(
                [a, b],
                Stroke::new(pr.f.s(u * 0.2).max(1.0), if lit { on } else { off }),
            );
        }
        cx += cw + u * 0.5;
    }
    cx
}

fn hall_art(pr: &Pr) {
    let def = pr.b.def();
    let secs = def.knobs[0].text(pr.b.v[0]).replace(" s", "");
    let (on, off) = if pr.on() {
        (
            Color32::from_rgb(255, 59, 47),
            Color32::from_rgba_unmultiplied(255, 60, 40, 22),
        )
    } else {
        (
            Color32::from_rgb(110, 30, 26),
            Color32::from_rgba_unmultiplied(255, 60, 40, 14),
        )
    };
    if pr.on() {
        let glow = pr.f.rect(HW / 2.0 - 64.0, 104.0, 98.0, 38.0);
        pr.p.rect_filled(glow, 8.0, Color32::from_rgba_unmultiplied(255, 40, 20, 16));
    }
    seven_seg(pr, &secs, HW / 2.0 - 56.0, 104.0, 34.0, on, off);
    pr.t(
        "SEC",
        HW / 2.0 + 52.0,
        136.0,
        Align2::CENTER_CENTER,
        10.0,
        0xff3b2f,
        Lettering::Condensed,
        30.0,
    );
    pr.t(
        "H A L L",
        HW / 2.0,
        171.0,
        Align2::CENTER_CENTER,
        18.0,
        0xe3e9f4,
        Lettering::Wide,
        HW * 0.6,
    );
    pr.t(
        "DIGITAL REVERB \u{b7} DECAY",
        HW / 2.0,
        193.0,
        Align2::CENTER_CENTER,
        8.0,
        0xa9b8d4,
        Lettering::Condensed,
        HW * 0.6,
    );
}
