//! Template A, the compact with a hinged pedal plate (Boss style, 73 x 129 mm): knobs along the top with their
//! labels above, a check LED, a black strip with the maker and the name, the plate with the effect's name and a
//! ribbed rubber pad, a thumbscrew at the toe. The whole plate is the switch. Also the slider version (the graphic
//! EQs). After the mockups' `boss()`.

use eframe::egui::{Align2, vec2};

use super::super::Lettering;
use super::face::{Fd, Lab, LabPos, Led, Look, Pr, RED, Spec, Sw, kn};
use super::kit::{self, C, Cv, Finish, Ks};

pub const W: f32 = 160.6;
pub const H: f32 = 283.8;

/// A compact: its look, and its knobs as (knob index, x as a fraction of the width, label).
pub fn spec(id: &'static str, look: Look, knobs: &[(usize, f32, &'static str)]) -> Spec {
    let mut s = Spec::new(id, W, H, body, art);
    s.look = look;
    s.ext = [8.0, 0.0, 8.0, 0.0];
    s.radius = W * 0.06;
    let n = knobs.len();
    let r = if n >= 5 {
        W * 0.074
    } else if n == 4 {
        W * 0.085
    } else {
        W * 0.1
    };
    for &(i, x, text) in knobs {
        s.knobs.push(kn(
            i,
            x * W,
            0.185 * H,
            r,
            Ks::Boss,
            Lab::new(LabPos::At(x * W, 0.09 * H), 6.4, look.ink).text(text),
        ));
    }
    s.led = Some(Led {
        x: 0.09 * W,
        y: 0.045 * H,
        r: 2.3,
        c: RED,
        jewel: false,
        bezel: true,
    });
    s.switch = Sw::Area(0.1 * W, 0.55 * H, 0.8 * W, 0.385 * H);
    s
}

/// The slider version: (knob index, label) left to right, the last the level.
pub fn sliders(id: &'static str, look: Look, bands: &[(usize, &'static str)]) -> Spec {
    let mut s = spec(id, look, &[]);
    let n = bands.len();
    let (x0, x1) = (24.0, W - 12.0);
    let (top, bot) = (0.085 * H, 0.395 * H);
    let step = (x1 - x0) / (n.max(2) - 1) as f32;
    for (j, &(i, text)) in bands.iter().enumerate() {
        s.faders.push(Fd {
            i,
            x: x0 + step * j as f32,
            y0: top,
            y1: bot,
            cap: vec2(11.0, 8.0),
            chrome: false,
            lab: Lab::new(LabPos::At(x0 + step * j as f32, 0.425 * H), 5.6, look.ink).text(text),
        });
    }
    s
}

/// Is this the slider version?
fn has_sliders(s: &Spec) -> bool {
    !s.faders.is_empty()
}

fn strip_y(s: &Spec) -> (f32, f32) {
    if has_sliders(s) {
        (0.455 * H, 0.54 * H)
    } else {
        (0.30 * H, 0.40 * H)
    }
}

fn plate_path(s: &Spec) -> Option<tiny_skia::Path> {
    let (_, y1) = strip_y(s);
    let (py, ins) = (y1 + 0.005 * H, 1.6);
    kit::rpoly(
        &[(ins, py), (W - ins, py), (W - ins, H - ins), (ins, H - ins)],
        &[11.0, 11.0, W * 0.06, W * 0.06],
    )
}

pub fn body(cv: &mut Cv, s: &Spec) {
    let l = &s.look;
    kit::jacks(cv, W, 0.2 * H, 13.0);
    let Some(shell) = kit::rrect(0.0, 0.0, W, H, W * 0.06) else {
        return;
    };
    kit::enclosure(
        cv,
        &shell,
        (0.0, 0.0, W, H),
        &kit::body_grad(l.c, 0.0, 0.0, W, H),
        l.finish,
    );
    if let Some(p) = kit::rrect(1.0, 1.0, W - 2.0, H - 2.0, W * 0.06 - 1.0) {
        cv.stroke(&p, &kit::col(0xffffff, 0.2), 0.9);
    }
    if has_sliders(s) {
        // the dB grid behind the sliders
        let (top, bot) = (0.085 * H, 0.395 * H);
        for k in 0..7 {
            let y = top + (bot - top) * k as f32 / 6.0;
            cv.line(
                19.0,
                y,
                W - 7.0,
                y,
                &kit::col(l.ink, if k == 3 { 0.35 } else { 0.14 }),
                0.8,
            );
        }
        for fd in &s.faders {
            cv.rect(
                fd.x - 2.0,
                fd.y0 - 3.0,
                4.0,
                fd.y1 - fd.y0 + 6.0,
                2.0,
                &kit::col(0x050505, 1.0),
            );
            cv.line(
                fd.x - 2.0,
                fd.y1 + 3.6,
                fd.x + 2.0,
                fd.y1 + 3.6,
                &kit::col(0xffffff, 0.4),
                0.6,
            );
        }
    }
    // the strip
    let (y0, y1) = strip_y(s);
    cv.rect(0.0, y0, W, y1 - y0, 0.0, &kit::col(0x121214, 1.0));
    cv.rect(0.0, y0, W, 0.8, 0.0, &kit::col(0xffffff, 0.12));
    // the hinged plate
    if let Some(d) = plate_path(s) {
        cv.fill(&d, &kit::body_grad(l.c, 0.0, y1, W, H - y1));
        let m = cv.mask(&d);
        l.finish.apply(cv, &m, (0.0, y1, W, H - y1), &d);
        cv.stroke(&d, &kit::col(0, 0.65), 1.3);
        if let Some(d2) = d
            .clone()
            .transform(tiny_skia::Transform::from_translate(0.0, 1.2))
        {
            cv.stroke_in(&d2, &kit::col(0xffffff, 0.28), 0.8, &m);
        }
    }
    if s.id == "acoustic" {
        // the soundhole's rosette
        let (cx, cy) = (W / 2.0, 0.478 * H);
        cv.ring(cx, cy, 15.5, &kit::col(0x2a1606, 0.55), 1.2);
        cv.circle(cx, cy, 13.0, &kit::col(0xf5e2c0, 0.3));
        cv.ring(cx, cy, 10.6, &kit::col(0x2a1606, 0.55), 0.9);
        cv.circle(cx, cy, 9.2, &kit::col(0x1a0e05, 0.75));
    }
    // the pad
    let (px0, px1) = (0.1 * W, 0.9 * W);
    let (py0, py1) = if has_sliders(s) {
        (0.64 * H, 0.935 * H)
    } else {
        (0.55 * H, 0.935 * H)
    };
    kit::rubber(cv, px0, py0, px1 - px0, py1 - py0, 5.0, 0x26272a);
    // the thumbscrew
    let (tx, ty) = (W / 2.0, H - 5.5);
    cv.circle(tx + 0.5, ty + 1.0, 6.4, &kit::col(0, 0.5));
    cv.circle(tx, ty, 6.2, &kit::chrome(tx - 6.2, ty - 6.2, 12.4, 12.4));
    kit::knurl(cv, tx, ty, 5.6, 1.4, 0x555b62, 14, 1.0);
    cv.circle(tx, ty, 3.6, &kit::dome(tx, ty, 3.6));
}

pub fn art(pr: &Pr) {
    let s = pr.spec;
    let l = &s.look;
    pr.t(
        "CHECK",
        0.09 * W + 4.5,
        0.045 * H,
        Align2::LEFT_CENTER,
        4.6,
        l.ink,
        Lettering::Condensed,
        30.0,
    );
    let (y0, y1) = strip_y(s);
    let ym = (y0 + y1) / 2.0;
    pr.t(
        "quickRS",
        7.0,
        ym,
        Align2::LEFT_CENTER,
        8.0,
        0xffffff,
        Lettering::Heavy,
        52.0,
    );
    let name_style = l.style;
    let r = if matches!(name_style, Lettering::Script | Lettering::Brush) {
        pr.ta(
            l.name,
            W - 7.0,
            ym,
            Align2::RIGHT_CENTER,
            15.0,
            l.accent,
            name_style,
            W - 66.0,
        )
    } else {
        pr.ta(
            l.name,
            W - 7.0,
            ym,
            Align2::RIGHT_CENTER,
            14.0,
            l.accent,
            name_style,
            W - 66.0,
        )
    };
    let _ = r;
    if !l.sub.is_empty() {
        let fy = if has_sliders(s) { 0.585 * H } else { 0.47 * H };
        let ink: C = if s.id == "metal" { l.accent } else { l.ink };
        pr.t(
            l.sub,
            W / 2.0,
            fy,
            Align2::CENTER_CENTER,
            7.0,
            ink,
            Lettering::Condensed,
            W * 0.8,
        );
    }
    if has_sliders(s) {
        pr.t(
            "+15",
            4.0,
            0.085 * H,
            Align2::LEFT_CENTER,
            5.0,
            l.ink,
            Lettering::Condensed,
            20.0,
        );
        pr.t(
            "0",
            8.0,
            0.24 * H,
            Align2::LEFT_CENTER,
            5.0,
            l.ink,
            Lettering::Condensed,
            20.0,
        );
        pr.t(
            "-15",
            4.0,
            0.395 * H,
            Align2::LEFT_CENTER,
            5.0,
            l.ink,
            Lettering::Condensed,
            20.0,
        );
        // the faint line through the caps: the curve
        let def = pr.b.def();
        let pts: Vec<_> = s
            .faders
            .iter()
            .take(s.faders.len().saturating_sub(1))
            .map(|fd| {
                let u = def.knobs[fd.i].to_u(pr.b.v[fd.i]);
                pr.f.p(fd.x, fd.y1 - (fd.y1 - fd.y0) * u)
            })
            .collect();
        if pts.len() > 1 {
            pr.p.add(eframe::egui::Shape::line(
                pts,
                eframe::egui::Stroke::new(pr.f.s(1.4).max(0.8), kit::c32a(l.ink, 0.35)),
            ));
        }
    }
}

/// The compacts' looks.
pub fn look(c: C, ink: C, name: &'static str, sub: &'static str) -> Look {
    Look {
        c,
        ink,
        name,
        sub,
        accent: 0xffffff,
        style: Lettering::Condensed,
        finish: Finish::Gloss,
    }
}

/// Every compact on the board.
pub fn of(id: &str) -> Option<Spec> {
    Some(match id {
        "gate" => spec(
            "gate",
            look(0xededea, 0x151515, "Shush", "NOISE GATE"),
            &[(0, 0.25, "THRESHOLD"), (1, 0.75, "RELEASE")],
        ),
        "blues" => spec(
            "blues",
            look(0x2d63b8, 0x111111, "Delta Blue", "BLUES OVERDRIVE"),
            &[(2, 0.2, "LEVEL"), (1, 0.5, "TONE"), (0, 0.8, "GAIN")],
        ),
        "orange" => spec(
            "orange",
            look(0xef6c1a, 0x111111, "Tangerine Dist", "DISTORTION"),
            &[(2, 0.2, "LEVEL"), (1, 0.5, "TONE"), (0, 0.8, "DIST")],
        ),
        "metal" => {
            let mut l = look(0x2a2b2e, 0xf2601e, "MOLTEN", "HEAVY DISTORTION");
            l.accent = 0xf2601e;
            l.style = Lettering::Heavy;
            l.finish = Finish::Sparkle;
            // Level, High / Low, Mid, Gain as on the real one (High and Low side by side here)
            spec(
                "metal",
                l,
                &[
                    (4, 0.11, "LEVEL"),
                    (3, 0.31, "HIGH"),
                    (1, 0.5, "LOW"),
                    (2, 0.69, "MID"),
                    (0, 0.89, "GAIN"),
                ],
            )
        }
        "acoustic" => {
            let mut l = look(0x9a6a3c, 0x1b0f06, "Campfire", "");
            l.finish = Finish::Wood;
            spec(
                "acoustic",
                l,
                &[(2, 0.2, "LEVEL"), (1, 0.5, "TOP"), (0, 0.8, "BODY")],
            )
        }
        "chorus" => spec(
            "chorus",
            look(0x3e9bd6, 0x111111, "Sky Chorus", "CHORUS"),
            &[(0, 0.2, "RATE"), (1, 0.5, "DEPTH"), (2, 0.8, "MIX")],
        ),
        "tremolo" => {
            let mut s = spec(
                "tremolo",
                look(0x14a39a, 0x0b1f1d, "Tremolo", "TREMOLO"),
                &[(0, 0.2, "SPEED"), (1, 0.8, "DEPTH")],
            );
            // the wave: a knob with three positions
            s.choices.push(super::face::Chs {
                i: 2,
                x: 0.5 * W - 20.0,
                y: 0.185 * H - 26.0,
                w: 40.0,
                h: 40.0,
                k: super::face::ChK::Rotary(Ks::Boss),
                lab: Lab::new(LabPos::At(0.5 * W, 0.185 * H + 21.0), 6.4, 0x0b1f1d).text("WAVE"),
                opts: &["SIN", "TRI", "SQR"],
                ink: 0x0b1f1d,
            });
            s
        }
        "octaver" => spec(
            "octaver",
            look(0x2f55b8, 0x0d1530, "Sub Octaver", "OCTAVE"),
            &[(0, 0.2, "SUB"), (1, 0.5, "UP"), (2, 0.8, "DRY")],
        ),
        "digital_delay" => spec(
            "digital_delay",
            look(0xf2f2ef, 0x2a2a2a, "Digital Delay", "DIGITAL DELAY"),
            &[
                (2, 0.15, "MIX"),
                (1, 0.383, "REPEATS"),
                (0, 0.617, "TIME"),
                (3, 0.85, "TONE"),
            ],
        ),
        _ => return None,
    })
}
