//! The Guitar tone page (port of settingsview.py `draw_tone`, `tone_display`, `draw_naming`, `tone_curve`,
//! `echo_taps`): the tone you hear, and the custom tone as three modules of amp knobs (drag up or down, scroll,
//! double-click to reset), each with a picture of what it does; saved tones along the bottom. quickRS has two
//! tones more than the Python app (Bass, Acoustic), so its tone keys are a little narrower to fit.

use eframe::egui;
use egui::{
    Align2, Color32, CursorIcon, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2,
};
use qrs_audio::tone::{KNOBS, ToneParams, ToneSpec};
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap, keycap_width};
use qrs_ui::knobs::{self, Scale};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::{pixels, px, to_px};
use qrs_ui::widgets::{self, ButtonStyle, Segmented, interact, segment_widths};

use super::{KnobDrag, SettingsView, caption_lines, centre_x, centre_y, centred, snap};
use crate::state::AppState;
use crate::tones;

pub const GROUPS: [(&str, &[&str]); 3] = [
    ("DYNAMICS & DRIVE", &["gate", "comp", "drive", "volume"]),
    ("EQ", &["bass", "mid", "treble", "presence"]),
    (
        "EFFECTS",
        &["chorus", "delay", "delay_time", "feedback", "reverb", "room"],
    ),
];

fn knob_name(key: &str) -> &'static str {
    match key {
        "gate" => "GATE",
        "comp" => "COMP",
        "drive" => "DRIVE",
        "volume" => "VOLUME",
        "bass" => "BASS",
        "mid" => "MID",
        "treble" => "TREBLE",
        "presence" => "PRESENCE",
        "chorus" => "CHORUS",
        "delay" => "DELAY",
        "delay_time" => "TIME",
        "feedback" => "REPEATS",
        "reverb" => "REVERB",
        "room" => "ROOM",
        _ => "",
    }
}

/// (label, low, high, default, unit) of a knob.
fn spec(key: &str) -> Option<(&'static str, &'static str, f32, f32, f32, &'static str)> {
    KNOBS.iter().find(|k| k.0 == key).copied()
}

pub fn range(key: &str) -> (f32, f32) {
    spec(key).map(|k| (k.2, k.3)).unwrap_or((0.0, 1.0))
}

pub fn param(st: &AppState, key: &str) -> f32 {
    tones::custom(&st.config).get(key).unwrap_or(0.0)
}

pub fn fmt_param(v: f32, unit: &str) -> String {
    match unit {
        "dB" => {
            if v.round_ties_even() != 0.0 {
                format!("{v:+.0} dB").replace('-', "\u{2212}")
            } else {
                "0 dB".into()
            }
        }
        "%" => format!("{:.0}%", v * 100.0),
        "s" => format!("{:.0} ms", v * 1000.0),
        _ => format!("{v:.1}"),
    }
}

/// A module's picture from its settings, as (x, y) points with both 0..1 (y up): the dynamics' level in to level
/// out (-80..0 dB in, -80..+6 dB out), or the EQ's response from 40 Hz to 16 kHz.
pub fn tone_curve(kind: &str, p: &ToneParams, n: usize) -> Vec<(f32, f32)> {
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        if kind == "dyn" {
            let mut o = -80.0 + 80.0 * t;
            if o < p.gate {
                o = p.gate + (o - p.gate) * 6.0;
            }
            if p.comp > 0.05 {
                let (thr, ratio) = (-14.0 - 2.0 * p.comp, 1.0 + 0.7 * p.comp);
                if o > thr {
                    o = thr + (o - thr) / ratio;
                }
            }
            if p.drive > 0.5 {
                let g = o + p.drive;
                o = g - 6.0 * (1.0 + 2f32.powf(g / 6.0)).log2() - 0.45 * p.drive;
            }
            o += p.volume;
            pts.push((t, ((o + 80.0) / 86.0).clamp(0.0, 1.0)));
        } else {
            let hz = 40.0 * (16000.0f32 / 40.0).powf(t);
            let mut g = p.bass / (1.0 + (hz / 120.0).powi(2));
            g += p.mid * (-((hz / 800.0).log2() / 0.9).powi(2)).exp();
            g += p.treble * (hz / 3200.0).powi(2) / (1.0 + (hz / 3200.0).powi(2));
            g += p.presence * (-((hz / 5000.0).log2() / 0.7).powi(2)).exp();
            let cut = if p.drive < 8.0 { 9000.0 } else { 6500.0 };
            g -= 10.0 * (1.0 + (80.0 / hz).powi(4)).log10() + 10.0 * (1.0 + (hz / cut).powi(4)).log10();
            pts.push((t, (0.5 + g / 30.0).clamp(0.0, 1.0)));
        }
    }
    pts
}

/// The delay's echoes as (time 0..1 of 2.5 s, height 0..1), and the reverb's level (0..1) for its tail.
pub fn echo_taps(p: &ToneParams) -> (Vec<(f32, f32)>, f32) {
    let mut taps = Vec::new();
    let (mut level, mut t) = (p.delay / 0.6, p.delay_time);
    while level > 0.02 && t < 2.5 {
        taps.push((t / 2.5, level));
        level *= p.feedback;
        t += p.delay_time;
        if p.delay_time <= 0.0 {
            break;
        }
    }
    (taps, p.reverb / 0.6)
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// A filled band between a curve and a flat line at `base_y` (a strip of quads; the curve may cross it).
fn band(p: &Painter, pts: &[Pos2], base_y: f32, color: Color32) {
    let mut mesh = Mesh::default();
    for q in pts {
        mesh.colored_vertex(*q, color);
        mesh.colored_vertex(pos2(q.x, base_y), color);
    }
    for i in 0..pts.len().saturating_sub(1) as u32 {
        let a = 2 * i;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    p.add(Shape::mesh(mesh));
}

/// A module's small screen: the dynamics' level in to level out, the EQ's response, or the echoes and the reverb's
/// tail, drawn from the knobs. (Lines are as wide as the Python app's, in whole pixels; a level or upright line
/// sits on the pixel row or column pygame draws it on.)
fn tone_display(p: &Painter, rect: Rect, kind: &str, params: &ToneParams) {
    let th = t();
    paint::display(p, rect, Some(14.0), 3.0);
    let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
    let (w, h) = (rect.width(), rect.height());
    let m = px(6.0);
    let at = |x: f32, y: f32| {
        pos2(
            rect.min.x + m + x * (w - 2.0 * m),
            rect.max.y - m - y * (h - 2.0 * m),
        )
    };
    // the middle of the pixel a point falls in: pygame draws a 1 px line through (int(x), int(y))
    let mid = |q: Pos2| pos2(snap(q.x) + pixels(0.5), snap(q.y) + pixels(0.5));
    let line = Stroke::new(pixels(1.0), th.line);
    let seg = |a: Pos2, b: Pos2| {
        p.line_segment([mid(a), mid(b)], line);
    };
    match kind {
        "dyn" => {
            seg(at(0.0, 80.0 / 86.0), at(1.0, 80.0 / 86.0)); // 0 dB out
            seg(at(0.0, 0.0), at(1.0, 80.0 / 86.0)); // unchanged
        }
        "eq" => seg(at(0.0, 0.5), at(1.0, 0.5)),
        _ => seg(at(0.0, 0.0), at(1.0, 0.0)),
    }
    if kind == "fx" {
        let (taps, verb) = echo_taps(params);
        if verb > 0.01 {
            // the reverb's tail, faint
            let decay = 0.3 + 1.7 * params.room;
            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let tt = i as f32 / 40.0;
                    at(tt, (0.25 + 0.75 * verb).min(1.0) * (-tt * 2.5 / decay).exp())
                })
                .collect();
            band(&p, &pts, at(0.0, 0.0).y, with_alpha(th.accent, 48));
            let wd = px(1.5).max(pixels(1.0));
            p.add(Shape::line(pts, Stroke::new(wd, with_alpha(th.accent, 120))));
        }
        // max(2, int(3 * s)) pixels wide, on the columns pygame fills: w // 2 either side of int(x), one fewer
        // on the left when w is even
        let wd = px(3.0).max(pixels(2.0));
        let off = if (to_px(wd).round() as i64) % 2 == 1 {
            0.5
        } else {
            1.0
        };
        for (tt, lvl) in &taps {
            // (from the baseline's pixel row up to the top one, both filled)
            let (a, b) = (at(*tt, 0.0), at(*tt, lvl.min(1.0)));
            let x = snap(a.x) + pixels(off);
            p.line_segment(
                [pos2(x, snap(a.y) + pixels(1.0)), pos2(x, snap(b.y))],
                Stroke::new(wd, th.accent),
            );
        }
        if taps.is_empty() && verb <= 0.01 {
            text::draw(
                &p,
                "dry",
                pos2(centre_x(rect), centre_y(rect)),
                Align2::CENTER_CENTER,
                Style::new(11.0, 700, th.caption),
                None,
            );
        }
    } else {
        let pts: Vec<Pos2> = tone_curve(kind, params, 64)
            .into_iter()
            .map(|(x, y)| at(x, y))
            .collect();
        if kind == "eq" {
            band(&p, &pts, at(0.0, 0.5).y, with_alpha(th.accent, 28));
        }
        // a soft glow under the line: the Python app draws it twice onto its (SRCALPHA) picture, which writes the
        // colour rather than blending it, so it's one layer at alpha 60
        p.add(Shape::line(
            pts.clone(),
            Stroke::new(px(6.0).max(pixels(3.0)), with_alpha(th.accent, 60)),
        ));
        p.add(Shape::line(pts, Stroke::new(px(2.5).max(pixels(2.0)), th.accent)));
    }
}

impl SettingsView {
    /// The tone page's actions (keys, clicks, knobs).
    pub(super) fn tone_act(&mut self, st: &mut AppState, verb: &str, arg: &str) {
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let current = match &engine {
            Some(e) => tones::name_of(&e.tone()),
            None => tones::name_of(&tones::current(&st.config)),
        };
        let apply = |st: &mut AppState, spec: ToneSpec| {
            tones::store(&mut st.config, &spec);
            if let Some(e) = &engine {
                e.set_tone(spec);
            }
        };
        match verb {
            "tone" => {
                let spec = tones::spec_of(arg, tones::custom(&st.config));
                apply(st, spec);
            }
            "tonecycle" => {
                let next = tones::next(current);
                let spec = tones::spec_of(next, tones::custom(&st.config));
                apply(st, spec);
                self.say(format!("Tone: {}", tones::label(next)));
            }
            "preset" => {
                if let Some((_, params)) = tones::presets(&st.config).into_iter().find(|(n, _)| n == arg) {
                    st.config.set("tone_preset", arg);
                    apply(st, ToneSpec::Custom { params });
                }
            }
            "delpreset" => {
                tones::delete_preset(&mut st.config, arg);
                self.say(format!("Deleted \u{201c}{arg}\u{201d}"));
            }
            "savepreset" => self.naming = Some(st.config.str("tone_preset", "")),
            "savenamed" => {
                let name = arg.trim();
                if !name.is_empty() {
                    let p = tones::custom(&st.config);
                    tones::save_preset(&mut st.config, name, &p);
                    self.say(format!("Saved \u{201c}{name}\u{201d}"));
                }
            }
            "resettone" => {
                let params = ToneParams::default();
                st.config.set("tone_custom", tones::params_json(&params));
                if current == "custom" {
                    apply(st, ToneSpec::Custom { params });
                }
            }
            "param" => {
                // turning a knob: the custom tone turns on and is heard straight away
                let Some((key, v)) = arg.split_once(':') else {
                    return;
                };
                let Ok(v) = v.parse::<f32>() else { return };
                let (lo, hi) = range(key);
                let mut params = tones::custom(&st.config);
                params.set(key, (v.clamp(lo, hi) * 1000.0).round() / 1000.0);
                apply(st, ToneSpec::Custom { params });
            }
            _ => {}
        }
    }

    pub(super) fn draw_tone(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let cur = match &st.audio {
            Some(a) => tones::name_of(&a.engine.tone()),
            None => tones::name_of(&tones::current(&st.config)),
        };
        // the title, and the tone you hear at the right (its keys narrower than the Python app's, pad 9 for its
        // 14, for the two tones more)
        let labels: Vec<&str> = tones::TONES.iter().map(|n| tones::label(n)).collect();
        let widths = segment_widths(&ctx, &labels, 14.0, 9.0);
        let seg_w = widths.iter().sum::<f32>() + px(4.0) * (widths.len() - 1) as f32 + 2.0 * px(5.0);
        let kw = keycap_width(&ctx, "T", 11.0) + px(12.0);
        let seg = Rect::from_min_size(pos2(body.max.x - seg_w - kw, body.min.y), vec2(seg_w, px(52.0)));
        let sel = tones::TONES.iter().position(|n| *n == cur);
        let mut so = Segmented::new(&labels, sel);
        so.size = 14.0;
        so.widths = Some(widths);
        so.pad = 5.0;
        let (_, clicked) = widgets::segmented(ui, seg, "set-tone", &so);
        if let Some(i) = clicked {
            self.acts.push(format!("tone:{}", tones::TONES[i]));
        }
        keycap(
            &p,
            "T",
            pos2(seg.max.x + px(10.0), centre_y(seg)),
            Align2::LEFT_CENTER,
            11.0,
            CapOn::Material,
        );
        text::draw(
            &p,
            "Guitar tone",
            pos2(body.min.x, body.min.y + px(2.0)),
            Align2::LEFT_TOP,
            Style::new(22.0, 800, th.text),
            None,
        );
        let ty = caption_lines(
            &p,
            "The tone you hear through quickRS. \u{201c}As is\u{201d} keeps your amp modeller's own sound.",
            body.min.x,
            body.min.y + px(34.0),
            seg.min.x - body.min.x - px(20.0),
            13.0,
            2,
        );
        let mut y = seg.max.y.max(ty) + px(20.0);
        let params = tones::custom(&st.config);
        let lit = cur == "custom";
        // three modules in a chain: dynamics & drive -> EQ -> effects
        let arrow = px(26.0);
        let flex = [2.0, 2.0, 3.0];
        let mw = (body.width() - 2.0 * arrow) / flex.iter().sum::<f32>();
        let (pad_x, pad_y) = (px(18.0), px(18.0));
        let bx = px(96.0);
        let row_h = bx + px(4.0) + px(14.0) + px(2.0) + px(18.0);
        let mod_h = pad_y + px(16.0) + px(14.0) + px(70.0) + px(14.0) + 2.0 * row_h + px(12.0) + px(18.0);
        let mut x = body.min.x;
        let shift = ctx.input(|i| i.modifiers.shift);
        for (i, (((title, keys), fl), kind)) in GROUPS.iter().zip(flex).zip(["dyn", "eq", "fx"]).enumerate() {
            if i > 0 {
                icons::icon(
                    &p,
                    Icon::Right,
                    pos2(snap(x) + snap(arrow / 2.0), y + snap(mod_h / 2.0)),
                    16.0,
                    th.tick,
                );
                x += arrow;
            }
            let module = Rect::from_min_size(pos2(snap(x), y), vec2(snap(mw * fl), mod_h));
            x += mw * fl;
            paint::card(&p, module, 24.0, false);
            let hy = module.min.y + pad_y + px(8.0);
            text::caps(
                &p,
                title,
                pos2(module.min.x + pad_x, hy),
                12.0,
                th.caption,
                Align2::LEFT_CENTER,
                None,
            );
            let light = pos2(module.max.x - pad_x - px(4.0), hy);
            if lit {
                paint::led(&p, light, 8.0, th.accent);
            } else {
                paint::dot(&p, light, 4.0, th.tick);
            }
            let disp = Rect::from_min_size(
                pos2(module.min.x + pad_x, module.min.y + pad_y + px(16.0) + px(14.0)),
                vec2(module.width() - 2.0 * pad_x, px(70.0)),
            );
            tone_display(&p, disp, kind, &params);
            let cap = match kind {
                "dyn" => "level in \u{2192} out".to_string(),
                "eq" => "80 Hz \u{2013} 10 kHz".into(),
                _ if params.delay > 0.01 => format!("echoes every {:.0} ms", params.delay_time * 1000.0),
                _ => "reverb tail".into(),
            };
            let cst = Style::new(10.0, 700, th.caption);
            if kind == "dyn" {
                // (the curve rises to the top right; the others leave the top free)
                text::draw(
                    &p,
                    &cap,
                    pos2(disp.max.x - px(10.0), disp.max.y - px(6.0)),
                    Align2::RIGHT_BOTTOM,
                    cst,
                    None,
                );
            } else {
                text::draw(
                    &p,
                    &cap,
                    pos2(disp.max.x - px(10.0), disp.min.y + px(6.0)),
                    Align2::RIGHT_TOP,
                    cst,
                    None,
                );
            }
            let cols = if keys.len() > 4 { 3 } else { 2 };
            let cw = (module.width() - 2.0 * pad_x) / cols as f32;
            let ky = disp.max.y + px(14.0);
            for (k_i, key) in keys.iter().enumerate() {
                let Some((key, _, lo, hi, dflt, unit)) = spec(key) else {
                    continue;
                };
                let (r_, c_) = (k_i / cols, k_i % cols);
                let cx = snap(module.min.x + pad_x + cw * c_ as f32 + cw / 2.0);
                let top = ky + r_ as f32 * (row_h + px(12.0));
                // kb.midtop = (cx, top)
                let kb = Rect::from_min_size(pos2(cx - snap(bx / 2.0), top), Vec2::splat(bx));
                let v = params.get(key).unwrap_or(dflt);
                let zero = (lo < 0.0 && 0.0 < hi).then(|| (0.0 - lo) / (hi - lo));
                knobs::knob(
                    &p,
                    pos2(centre_x(kb), centre_y(kb)),
                    60.0,
                    (v - lo) / (hi - lo),
                    Scale {
                        zero,
                        ..Default::default()
                    },
                    None,
                );
                self.knob_rects.push((kb, key));
                let resp = interact(ui, kb, ("set-knob", key), Sense::click_and_drag());
                if resp.hovered() || resp.dragged() {
                    ctx.set_cursor_icon(CursorIcon::ResizeVertical);
                }
                if resp.double_clicked() {
                    self.knob_drag = None;
                    self.acts.push(format!("param:{key}:{dflt}"));
                } else if resp.is_pointer_button_down_on()
                    && self.knob_drag.as_ref().is_none_or(|d| d.key != key)
                    && let Some(pos) = resp.interact_pointer_pos()
                {
                    self.knob_drag = Some(KnobDrag {
                        key,
                        y0: pos.y,
                        v0: v,
                    });
                }
                if let Some(d) = &self.knob_drag
                    && d.key == key
                {
                    if resp.is_pointer_button_down_on() || resp.dragged() {
                        if let Some(pos) = resp.interact_pointer_pos()
                            && (d.y0 - pos.y).abs() > 0.5
                        {
                            let travel = 200.0 * if shift { 4.0 } else { 1.0 };
                            let nv = d.v0 + (d.y0 - pos.y) / travel * (hi - lo);
                            self.acts.push(format!("param:{key}:{nv}"));
                        }
                    } else {
                        self.knob_drag = None;
                        self.acts.push("saveconfig".into());
                    }
                }
                text::caps(
                    &p,
                    knob_name(key),
                    pos2(cx, kb.max.y + px(11.0)),
                    11.0,
                    th.caption,
                    Align2::CENTER_CENTER,
                    None,
                );
                text::draw(
                    &p,
                    &fmt_param(v, unit),
                    pos2(cx, kb.max.y + px(29.0)),
                    Align2::CENTER_CENTER,
                    Style::new(15.0, 800, if lit { th.text } else { th.text_2 }),
                    None,
                );
            }
        }
        y += mod_h + px(14.0);
        let hint = if lit {
            "Drag a knob up or down (Shift: fine)  \u{b7}  scroll for small steps  \u{b7}  double-click resets it"
        } else {
            "Turn a knob to switch to your custom tone; you hear it straight away."
        };
        text::draw(
            &p,
            hint,
            pos2(body.min.x, y),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, th.caption),
            None,
        );
        y += px(30.0);
        // saved tones: chips flow after the title, wrapping; the tools sit at the right of the last row
        let presets = tones::presets(&st.config);
        let chosen = st.config.str("tone_preset", "");
        let items: Vec<(String, bool)> = presets
            .iter()
            .map(|(n, _)| (n.clone(), lit && chosen == *n))
            .collect();
        let (bh, gx, pad_c) = (px(44.0), px(12.0), px(14.0));
        let mut tools: Vec<(&str, String, &str)> = vec![
            ("Save as\u{2026}", "savepreset".into(), "primary"),
            ("Reset knobs", "resettone".into(), ""),
        ];
        if presets.iter().any(|(n, _)| *n == chosen) {
            tools.insert(0, ("Delete", format!("delpreset:{chosen}"), "danger"));
        }
        let tw = tools
            .iter()
            .map(|t_| widgets::button_width(&ctx, t_.0, "", 14.0, false))
            .sum::<f32>()
            + gx * (tools.len() - 1) as f32;
        let title_w = text::width(&ctx, "SAVED TONES", Style::caps(12.0, th.caption)) + px(22.0);
        let (x0, x1) = (body.min.x + px(20.0), body.max.x - px(20.0));
        let mut lines: Vec<Vec<(String, bool, f32, f32)>> = Vec::new();
        let mut line: Vec<(String, bool, f32, f32)> = Vec::new();
        let mut lx = x0 + title_w;
        for (name, on) in &items {
            let cw_ = widgets::button_width(&ctx, name, "", 14.0, false) + if *on { px(15.0) } else { 0.0 };
            if !line.is_empty() && lx + cw_ > x1 {
                lines.push(std::mem::take(&mut line));
                lx = x0 + title_w;
            }
            line.push((name.clone(), *on, lx, cw_));
            lx += cw_ + gx;
        }
        let last_full = !line.is_empty() && lx + tw > x1;
        if !line.is_empty() || lines.is_empty() {
            lines.push(line);
        }
        if last_full {
            lines.push(Vec::new());
        }
        let nl = lines.len() as f32;
        let card = Rect::from_min_size(
            pos2(body.min.x, y),
            vec2(body.width(), 2.0 * pad_c + nl * bh + (nl - 1.0) * px(12.0)),
        );
        paint::card(&p, card, 22.0, false);
        let mid_y = card.min.y + pad_c + snap(bh / 2.0);
        text::caps(
            &p,
            "SAVED TONES",
            pos2(x0, mid_y),
            12.0,
            th.caption,
            Align2::LEFT_CENTER,
            None,
        );
        for (li, ln) in lines.iter().enumerate() {
            let ly = card.min.y + pad_c + li as f32 * (bh + px(12.0));
            for (name, on, lx_, cw_) in ln {
                self.button(
                    ui,
                    Rect::from_min_size(pos2(*lx_, ly), vec2(*cw_, bh)),
                    name,
                    "",
                    &format!("preset:{name}"),
                    ButtonStyle {
                        active: *on,
                        size: 14.0,
                        r: Some(14.0),
                        ..Default::default()
                    },
                );
            }
        }
        if items.is_empty() {
            text::draw(
                &p,
                "None yet: set the knobs, then Save as\u{2026}",
                pos2(x0 + title_w, mid_y),
                Align2::LEFT_CENTER,
                Style::new(13.0, 500, th.caption),
                None,
            );
        }
        let mut tx = x1 - tw;
        let ty = card.min.y + pad_c + (nl - 1.0) * (bh + px(12.0));
        for (label, action, kind) in &tools {
            let bw = widgets::button_width(&ctx, label, "", 14.0, false);
            self.button(
                ui,
                Rect::from_min_size(pos2(tx, ty), vec2(bw, bh)),
                label,
                "",
                action,
                ButtonStyle {
                    primary: *kind == "primary",
                    danger: *kind == "danger",
                    size: 14.0,
                    r: Some(14.0),
                    ..Default::default()
                },
            );
            tx += bw + gx;
        }
        card.max.y
    }

    /// Name this tone: a sunken field ringed in the accent; Cancel / Save.
    pub(super) fn draw_naming(&mut self, ui: &Ui, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        paint::dim(&p, area, None);
        let _ = interact(ui, area, "set-naming-modal", Sense::click());
        let pad = px(24.0);
        let panel = centred(
            area,
            vec2(
                snap((500.0f32).min(area.width() - 40.0)),
                pad + px(28.0) + px(16.0) + px(52.0) + px(16.0) + px(48.0) + pad,
            ),
        );
        paint::floating(&p, panel, Some(26.0));
        text::draw(
            &p,
            "Name this tone",
            pos2(panel.min.x + pad, panel.min.y + pad),
            Align2::LEFT_TOP,
            Style::new(20.0, 800, th.text),
            None,
        );
        let bx = Rect::from_min_size(
            pos2(panel.min.x + pad, panel.min.y + pad + px(44.0)),
            vec2(panel.width() - 2.0 * pad, px(52.0)),
        );
        paint::inset(&p, bx, Some(16.0), 3.0, Fill::Base);
        paint::ring(&p, bx, Some(16.0), 2.0, th.accent, true);
        let caret = if (ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0 {
            "|"
        } else {
            ""
        };
        let name = self.naming.clone().unwrap_or_default();
        text::draw(
            &p,
            &format!("{name}{caret}"),
            pos2(bx.min.x + px(18.0), centre_y(bx)),
            Align2::LEFT_CENTER,
            Style::new(17.0, 600, th.text),
            Some(bx.width() - px(36.0)),
        );
        let ow = widgets::button_width(&ctx, "Save", "Enter", 14.0, false);
        let cw = widgets::button_width(&ctx, "Cancel", "Esc", 14.0, false);
        let by = bx.max.y + px(16.0);
        let bh = px(48.0);
        let ok = Rect::from_min_size(pos2(panel.max.x - pad - ow, by), vec2(ow, bh));
        let cancel = Rect::from_min_size(pos2(ok.min.x - px(12.0) - cw, by), vec2(cw, bh));
        let st = ButtonStyle {
            size: 14.0,
            r: Some(14.0),
            ..Default::default()
        };
        if widgets::button(ui, cancel, "set-name-cancel", "Cancel", "Esc", st).clicked() {
            self.naming = None;
        }
        if widgets::button(
            ui,
            ok,
            "set-name-ok",
            "Save",
            "Enter",
            ButtonStyle { primary: true, ..st },
        )
        .clicked()
        {
            self.naming = None;
            self.acts.push(format!("savenamed:{}", name.trim()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_like_the_python_app() {
        assert_eq!(fmt_param(-56.0, "dB"), "\u{2212}56 dB");
        assert_eq!(fmt_param(10.0, "dB"), "+10 dB");
        assert_eq!(fmt_param(0.4, "dB"), "0 dB");
        assert_eq!(fmt_param(0.3, "%"), "30%");
        assert_eq!(fmt_param(0.32, "s"), "320 ms");
        assert_eq!(fmt_param(3.0, ""), "3.0");
    }

    #[test]
    fn curves_and_echoes() {
        let p = ToneParams::default();
        let dynp = tone_curve("dyn", &p, 64);
        assert_eq!(dynp.len(), 64);
        assert!(
            dynp.iter()
                .all(|(x, y)| (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y))
        );
        // the gate pulls quiet input down: the curve starts at the floor
        assert_eq!(dynp[0].1, 0.0);
        let eq = tone_curve("eq", &p, 64);
        // a flat EQ sits on the middle line between the cut filters
        assert!((eq[32].1 - 0.5).abs() < 0.02, "{:?}", eq[32]);
        let (taps, verb) = echo_taps(&ToneParams {
            delay: 0.3,
            delay_time: 0.5,
            feedback: 0.5,
            ..p
        });
        assert_eq!(taps.len(), 4);
        assert!((taps[0].0 - 0.2).abs() < 1e-6 && (taps[0].1 - 0.5).abs() < 1e-6);
        assert!((verb - 0.2).abs() < 1e-6);
    }
}
