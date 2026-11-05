//! The generic brushed-silver unit (P37.1, D101; the audit's mock 3): any block without a designed face whose every
//! control is real (a NAM capture, an IR, a capture whose skin doesn't map, a future model without art). Its real
//! name on a black plate, a NAM or IR stamp, a line of facts, and exactly the block's own front knobs from
//! `def().knobs` (drawn from the list, so knobs added to a model later appear by themselves). A file block with no
//! file shows "No capture loaded: you hear your dry guitar".

use eframe::egui::{Align2, Color32, Painter, Rect, Stroke, Vec2, pos2, vec2};
use qrs_audio::board::{Block, Face, Unit};
use qrs_ui::knobs::{self, KnobStyle, Scale};
use qrs_ui::raster::Image;
use qrs_ui::text::{self, Style};

use super::pedals::kit::{self, Cv, Fit};
use super::{Detail, Hot, Layout};

/// The unit's dials and switches (front controls).
fn split(b: &Block) -> (Vec<usize>, Vec<usize>) {
    let def = b.def();
    (0..def.front_len()).partition(|&i| !matches!(def.knobs[i].unit, Unit::Choice(_)))
}

/// The unit's size on the board (points): wide enough for its knobs; a second row for its switches when it has both.
pub(super) fn board_size(b: &Block) -> Vec2 {
    let (dials, chs) = split(b);
    if dials.is_empty() || chs.is_empty() {
        let n = b.def().front_len().max(2) as f32;
        return vec2((n * 58.0 + 60.0).max(240.0), 124.0);
    }
    let w = (dials.len() as f32 * 62.0).max(chs.len() as f32 * 116.0) + 40.0;
    vec2(w.max(240.0), 176.0)
}

/// The knob row (and the switch row), the plate, the stamp, the LED and the on/off key.
pub(super) fn layout(b: &Block, r: Rect, _detail: Detail) -> Layout {
    let def = b.def();
    let (w, h) = (r.width(), r.height());
    let (dials, chs) = split(b);
    let two = !dials.is_empty() && !chs.is_empty();
    let s = (h / if two { 176.0 } else { 124.0 }).clamp(0.3, 3.0);
    let mut l = Layout {
        radius: 5.0 * s,
        panel: r,
        ..Default::default()
    };
    let plate_h = 22.0 * s;
    l.name = Rect::from_min_size(
        pos2(r.min.x + 14.0 * s, r.min.y + 10.0 * s),
        vec2((w * 0.62).min(w - 110.0 * s).max(40.0), plate_h),
    );
    l.led = pos2(r.max.x - 92.0 * s, r.min.y + 21.0 * s);
    l.switch = Rect::from_center_size(l.led, vec2(18.0 * s, 18.0 * s));
    let top = r.min.y + 58.0 * s;
    let bottom = r.max.y - 14.0 * s;
    if two {
        // knobs across, then the switches across under them
        let mid = bottom - 34.0 * s;
        let y = (top + mid) / 2.0 + 2.0 * s;
        let step = (w - 28.0 * s) / dials.len() as f32;
        let d = (step * 0.62).min((mid - top) * 0.55).clamp(12.0, 46.0 * s);
        for (j, &i) in dials.iter().enumerate() {
            l.knobs
                .push((i, pos2(r.min.x + 14.0 * s + step * (j as f32 + 0.5), y), d));
        }
        let cstep = (w - 28.0 * s) / chs.len() as f32;
        for (j, &i) in chs.iter().enumerate() {
            let c = pos2(r.min.x + 14.0 * s + cstep * (j as f32 + 0.5), bottom - 8.0 * s);
            l.choices
                .push((i, Rect::from_center_size(c, vec2(cstep * 0.88, 16.0 * s))));
        }
        return l;
    }
    let n = def.front_len();
    let y = (top + bottom) / 2.0 + 2.0 * s;
    let step = (w - 28.0 * s) / n.max(1) as f32;
    let d = (step * 0.62).min((bottom - top) * 0.62).clamp(12.0, 46.0 * s);
    for i in 0..n {
        let c = pos2(r.min.x + 14.0 * s + step * (i as f32 + 0.5), y);
        if matches!(def.knobs[i].unit, Unit::Choice(_)) {
            l.choices.push((
                i,
                Rect::from_center_size(c, vec2((step * 0.86).min(90.0 * s), 18.0 * s)),
            ));
        } else {
            l.knobs.push((i, c, d));
        }
    }
    l
}

/// The panel: brushed aluminium, a lit top edge, four screws (cached per size by the caller).
pub(super) fn body(_b: &Block, r: Rect, _l: &Layout, ppp: f32) -> Image {
    let (w, h) = (r.width(), r.height());
    let fit = Fit::new(r, w, h, [0.0; 4]);
    let Some(mut cv) = Cv::new(r, &fit, ppp) else {
        return Image {
            w: 1,
            h: 1,
            rgba: vec![0; 4],
        };
    };
    let rad = (h * 0.05).clamp(3.0, 8.0);
    if let Some(p) = kit::rrect(0.0, 0.0, w, h, rad) {
        cv.fill(
            &p,
            &kit::lin(
                0.0,
                0.0,
                0.0,
                h,
                &[(0.0, 0xd9dcdf, 1.0), (0.45, 0xbfc3c7, 1.0), (1.0, 0xa9aeb3, 1.0)],
            ),
        );
        let m = cv.mask(&p);
        kit::brushed(&mut cv, &m, (0.0, 0.0, w, h), 37, 1.0);
        cv.stroke(&p, &kit::col(0x2a2d31, 0.55), 1.2);
    }
    cv.rect(1.0, 1.0, w - 2.0, 1.0, 0.0, &kit::col(0xffffff, 0.6));
    let sr = (h * 0.03).clamp(2.0, 5.0);
    for (x, y) in [
        (sr * 2.2, sr * 2.2),
        (w - sr * 2.2, sr * 2.2),
        (sr * 2.2, h - sr * 2.2),
        (w - sr * 2.2, h - sr * 2.2),
    ] {
        kit::screw(&mut cv, x, y, sr, 30.0, true);
    }
    cv.image()
}

/// A knob's value as read on the unit: a capture step of -1 is "as captured" (the sound agent's Gain on NAM blocks).
pub fn knob_text(b: &Block, k: usize) -> String {
    let def = b.def();
    let Some(kd) = def.knobs.get(k) else {
        return String::new();
    };
    let v = b.v[k];
    if def.file && kd.key == "gain" {
        // (the steps of its unit's gain ladder, P37.6c: one capture has none)
        let steps = qrs_audio::board::nam::ladder(b.asset);
        if steps.len() <= 1 {
            return "one capture".into();
        }
        if v < -0.5 {
            return "as captured".into();
        }
        let n = steps
            .iter()
            .enumerate()
            .min_by(|a, c| (a.1.1 - v).abs().total_cmp(&(c.1.1 - v).abs()))
            .map_or(0, |(j, _)| j);
        return format!("{:.1} \u{b7} {} of {}", steps[n].1, n + 1, steps.len());
    }
    kd.text(v)
}

/// A capture Gain knob's steps as turns of the knob (0..1), when it has more than one (P37.6c).
fn ladder_turns(b: &Block, k: usize) -> Option<Vec<f32>> {
    let def = b.def();
    let kd = def.knobs.get(k)?;
    if !def.file || kd.key != "gain" {
        return None;
    }
    let steps = qrs_audio::board::nam::ladder(b.asset);
    Some(steps.iter().map(|s| kd.to_u(s.1)).collect())
}

/// The name on the plate and the facts line.
fn words(b: &Block) -> (String, String, Option<&'static str>) {
    let def = b.def();
    let stamp = match def.id {
        "cab_ir" => Some("IR"),
        _ if def.file => Some("NAM"),
        _ => None,
    };
    let kind = match def.id {
        "amp_nam" => "NAM AMP CAPTURE",
        "pedal_nam" => "NAM PEDAL CAPTURE",
        "cab_ir" => "IMPULSE RESPONSE",
        _ => "",
    };
    if !def.file {
        return (def.shown_name(), def.kind.label().to_uppercase(), stamp);
    }
    let item = crate::captures::with(|l| l.get(b.asset).cloned());
    match item {
        Some(i) => {
            let mut facts = vec![kind.to_string()];
            if let Some(s) = &i.size {
                facts.push(s.clone());
            }
            if def.id == "amp_nam" {
                facts.push(match i.cab {
                    Some(true) => "includes cab".into(),
                    Some(false) => "no cab: plays into a cab".into(),
                    None => "cab: not said".into(),
                });
            }
            if let Some(c) = &i.creator {
                facts.push(format!("by {c}"));
            }
            if i.source == "tone3000" {
                facts.push("TONE3000".into());
            }
            (i.name.clone(), facts.join("  \u{b7}  "), stamp)
        }
        None => (
            if b.asset == 0 {
                "No capture loaded".into()
            } else {
                "Its file isn't in your library".into()
            },
            "you hear yourself dry".into(),
            stamp,
        ),
    }
}

/// Whether a file block plays nothing of its own (no file, or one not in the library): the dry guitar passes.
pub fn missing(b: &Block) -> bool {
    b.def().file && crate::captures::with(|l| l.get(b.asset).is_none())
}

/// What's drawn live over the panel: the plate, stamp, facts, LED, knobs with their labels and values.
pub(super) fn controls(p: &Painter, r: Rect, b: &Block, l: &Layout, detail: Detail, hot: Hot) {
    let def = b.def();
    let (dials, chs) = split(b);
    let base_h = if !dials.is_empty() && !chs.is_empty() {
        176.0
    } else {
        124.0
    };
    let s = (r.height() / base_h).clamp(0.3, 3.0);
    let ink = Color32::from_rgb(0x22, 0x25, 0x29);
    let (name, facts, stamp) = words(b);
    // (a miniature, P37.12: knobs as dots, no print)
    let mini = super::mini_now();
    let small = r.height() < 70.0 || mini;
    // the plate
    p.rect_filled(l.name, 4.0 * s, Color32::from_rgb(0x1b, 0x1d, 0x21));
    p.rect_stroke(
        l.name,
        4.0 * s,
        Stroke::new(1.0, Color32::from_white_alpha(30)),
        eframe::egui::StrokeKind::Inside,
    );
    if !small {
        // (a long name wraps onto two smaller lines on the plate, never cut, P37.6c)
        let white = Color32::from_rgb(0xf2, 0xf2, 0xf0);
        let big = Style::new((12.5 * s).clamp(6.0, 20.0), 800, white);
        let room = l.name.width() - 14.0 * s;
        if text::width(p.ctx(), &name, big) <= room {
            text::draw(
                p,
                &name,
                pos2(l.name.min.x + 8.0 * s, l.name.center().y),
                Align2::LEFT_CENTER,
                big,
                Some(room),
            );
        } else {
            let st = Style::new((9.0 * s).clamp(5.0, 15.0), 800, white);
            let lines = text::wrap(p.ctx(), &name, st, room);
            let lh = l.name.height() / 2.0;
            for (n, line) in lines.iter().take(2).enumerate() {
                text::draw(
                    p,
                    line,
                    pos2(l.name.min.x + 8.0 * s, l.name.min.y + lh * (n as f32 + 0.5)),
                    Align2::LEFT_CENTER,
                    st,
                    Some(room),
                );
            }
        }
        text::caps(
            p,
            &facts,
            pos2(l.name.min.x + 2.0 * s, l.name.max.y + 10.0 * s),
            (8.5 * s).clamp(5.0, 13.0),
            if missing(b) {
                Color32::from_rgb(0xb8, 0x2e, 0x26)
            } else {
                ink
            },
            Align2::LEFT_CENTER,
            Some(r.width() - 30.0 * s),
        );
    }
    // the stamp (NAM / IR) at the top right
    if let Some(word) = stamp {
        let sr = Rect::from_min_size(
            pos2(r.max.x - 66.0 * s, r.min.y + 12.0 * s),
            vec2(48.0 * s, 18.0 * s),
        );
        let red = Color32::from_rgb(0xb8, 0x2e, 0x26);
        p.rect_filled(sr, 3.0 * s, Color32::from_rgb(0xf1, 0xec, 0xe0));
        p.rect_stroke(
            sr,
            3.0 * s,
            Stroke::new(1.4 * s, red),
            eframe::egui::StrokeKind::Inside,
        );
        if !small {
            text::draw(
                p,
                word,
                sr.center(),
                Align2::CENTER_CENTER,
                Style::new((10.5 * s).clamp(5.0, 16.0), 900, red),
                None,
            );
        }
    }
    // the LED (lit while on; amber while it plays nothing of its own)
    let lit = if missing(b) {
        Color32::from_rgb(0xff, 0xb3, 0x47)
    } else {
        Color32::from_rgb(0x3e, 0xd4, 0x6d)
    };
    let lr = 4.5 * s;
    p.circle_filled(l.led, lr + 1.5 * s, Color32::from_black_alpha(90));
    p.circle_filled(
        l.led,
        lr,
        if b.on {
            lit
        } else {
            Color32::from_rgb(0x55, 0x58, 0x5c)
        },
    );
    if hot.switch {
        p.circle_stroke(l.led, lr + 4.0 * s, Stroke::new(1.0, ink));
    }
    // the knobs
    let dot = super::knob_dot(Face::Rack, r.height());
    for &(i, c, d) in &l.knobs {
        if mini {
            p.circle_filled(c, dot.0, dot.1);
            continue;
        }
        let kd = def.knobs[i];
        let steps = ladder_turns(b, i);
        let u = match &steps {
            // (one capture: nothing to step to, the knob rests at its bottom, dimmed)
            Some(t) if t.len() <= 1 => 0.0,
            _ => kd.to_u(b.v[i]),
        };
        knobs::knob(
            p,
            c,
            d,
            u,
            Scale {
                zero: kd.centred().then(|| kd.to_u(0.0)),
                ticks: steps.is_none(),
                ..Default::default()
            },
            Some(KnobStyle::Chrome),
        );
        if let Some(t) = &steps {
            if t.len() <= 1 {
                p.circle_filled(
                    c,
                    d * 0.5 + 2.0,
                    Color32::from_rgba_unmultiplied(0xc8, 0xcc, 0xd0, 150),
                );
            } else {
                // a detent per capture of the unit, where its gain sits on the dial
                for &tu in t {
                    let a = (-135.0 + 270.0 * tu).to_radians();
                    let q = c + vec2(a.sin(), -a.cos()) * (d * 0.5 + 6.0 * s);
                    p.circle_filled(q, (2.2 * s).max(1.5), ink);
                }
            }
        }
        if small {
            continue;
        }
        let lab = (8.5 * s).clamp(5.0, 13.0);
        // (P41.3: a small face, as in the Rig tile, ran its labels together, "InputOutp... TrebleGate": each label
        // and value fits the space to its neighbours, in smaller print, then shortened, never over the next one)
        let pitch = l
            .knobs
            .iter()
            .filter(|o| o.0 != i && (o.1.y - c.y).abs() < d * 0.5)
            .map(|o| (o.1.x - c.x).abs())
            .fold(f32::INFINITY, f32::min);
        let room_w = if pitch.is_finite() {
            (pitch - 4.0 * s).max(d)
        } else {
            d * 2.6
        };
        let (label, lab_l) = fit_label(p.ctx(), kd.label, lab, room_w, ink);
        // (clear of the tick arc round the knob, P37.6c: "Input", "Lo-mid" sat on it)
        text::caps(
            p,
            label,
            pos2(c.x, c.y - d * 0.85 - 4.0 * s),
            lab_l,
            ink,
            Align2::CENTER_CENTER,
            None,
        );
        let shown = hot.active == Some(i) || hot.hover == Some(i) || detail == Detail::Full;
        if shown {
            let full = knob_text(b, i);
            let v = if text::width(p.ctx(), &full, Style::new(lab, 800, ink)) > room_w {
                // ("6.5 · 4 of 4" reads "6.5" where it doesn't fit)
                full.split(" \u{b7} ").next().unwrap_or(&full).to_string()
            } else {
                full
            };
            let mut vs = lab;
            while vs > 5.0 && text::width(p.ctx(), &v, Style::new(vs, 800, ink)) > room_w {
                vs -= 0.5;
            }
            text::draw(
                p,
                &v,
                pos2(c.x, c.y + d * 0.5 + 9.0 * s),
                Align2::CENTER_CENTER,
                Style::new(vs, 800, ink),
                Some(room_w),
            );
        }
    }
    // choices: their options as a row, the chosen one dark
    for &(i, cr) in &l.choices {
        let kd = def.knobs[i];
        let Unit::Choice(opts) = kd.unit else { continue };
        let n = opts.len().max(1);
        let cw = cr.width() / n as f32;
        if stepped(p.ctx(), opts, cr, s) {
            // (too many words for its row: the value between arrows; a click's side steps it, P37.13)
            p.rect_filled(cr.shrink(1.0), 3.0 * s, Color32::from_rgb(0x22, 0x25, 0x29));
            let st = Style::new((8.0 * s).clamp(5.0, 12.0), 800, Color32::WHITE);
            let cur = opts.get(b.v[i].round() as usize).copied().unwrap_or("");
            text::draw(
                p,
                cur,
                cr.center(),
                Align2::CENTER_CENTER,
                st,
                Some(cr.width() - 24.0 * s),
            );
            let y = cr.center().y;
            text::draw(
                p,
                "\u{2039}",
                pos2(cr.min.x + 7.0 * s, y),
                Align2::CENTER_CENTER,
                st,
                None,
            );
            text::draw(
                p,
                "\u{203a}",
                pos2(cr.max.x - 7.0 * s, y),
                Align2::CENTER_CENTER,
                st,
                None,
            );
        } else {
            for (j, o) in opts.iter().enumerate() {
                let or = Rect::from_min_size(pos2(cr.min.x + cw * j as f32, cr.min.y), vec2(cw, cr.height()));
                let on = b.v[i].round() as usize == j;
                p.rect_filled(
                    or.shrink(1.0),
                    3.0 * s,
                    if on {
                        Color32::from_rgb(0x22, 0x25, 0x29)
                    } else {
                        Color32::from_black_alpha(25)
                    },
                );
                if !small {
                    text::draw(
                        p,
                        o,
                        or.center(),
                        Align2::CENTER_CENTER,
                        Style::new(
                            (8.0 * s).clamp(5.0, 12.0),
                            800,
                            if on { Color32::WHITE } else { ink },
                        ),
                        Some(cw - 2.0),
                    );
                }
            }
        }
        if !small {
            text::caps(
                p,
                kd.label,
                pos2(cr.center().x, cr.min.y - 9.0 * s),
                (8.5 * s).clamp(5.0, 13.0),
                ink,
                Align2::CENTER_CENTER,
                None,
            );
        }
    }
}

/// A knob's label that fits `w`: smaller print down to 5 pt, then its short form ("Output" "OUT"), never cut into
/// its neighbour's. Returns (the words, their size).
fn fit_label(
    ctx: &eframe::egui::Context,
    label: &'static str,
    size: f32,
    w: f32,
    ink: Color32,
) -> (&'static str, f32) {
    let fits = |s: &str, z: f32| text::width(ctx, s, Style::caps(z, ink)) <= w;
    let mut z = size;
    while z > 5.0 && !fits(label, z) {
        z -= 0.5;
    }
    if fits(label, z) {
        return (label, z);
    }
    let short = match label {
        "Input" => "In",
        "Output" => "Out",
        "Treble" => "Treb",
        "Presence" => "Pres",
        "Threshold" => "Thr",
        "Release" => "Rel",
        "Distance" => "Dist",
        "Intensity" => "Int",
        "Volume" => "Vol",
        "Master" => "Mstr",
        _ => label.get(..label.len().min(4)).unwrap_or(label),
    };
    (short, z)
}

/// Whether a silver unit's choice is drawn as a stepper (its options don't fit its row).
fn stepped(ctx: &eframe::egui::Context, opts: &[&str], cr: Rect, s: f32) -> bool {
    let cw = cr.width() / opts.len().max(1) as f32 - 4.0;
    let st = Style::new((8.0 * s).clamp(5.0, 12.0), 800, Color32::WHITE);
    opts.iter().any(|o| text::width(ctx, o, st) > cw)
}

/// Whether the face's choice `k` at `cr` is a stepper (a click on its left half steps back, else on).
pub fn choice_stepped(ctx: &eframe::egui::Context, b: &Block, k: usize, cr: Rect, face_h: f32) -> bool {
    if super::family(b.def()) != super::Family::Silver {
        return false;
    }
    let Unit::Choice(opts) = b.def().knobs[k].unit else {
        return false;
    };
    let (dials, chs) = split(b);
    let base_h = if !dials.is_empty() && !chs.is_empty() {
        176.0
    } else {
        124.0
    };
    stepped(ctx, opts, cr, (face_h / base_h).clamp(0.3, 3.0))
}
