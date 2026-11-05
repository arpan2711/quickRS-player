//! The strip along the room's foot, always there (P37.1): the input (meter, kind, trim, Calibrate), the noise gate's
//! knob, the tuner, the scenes (named by you), the true output.

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use qrs_audio::board;
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::draw::{self, KeyLook};
use super::rigs::{SCENE_COLOURS, SCENES};
use super::{RoomView, Typing};
use crate::ampbutton::rgb;
use crate::state::AppState;

const GREEN: Color32 = Color32::from_rgb(0x3e, 0xc4, 0x6d);
const AMBER: Color32 = Color32::from_rgb(0xf2, 0xb5, 0x3c);
const RED: Color32 = Color32::from_rgb(0xf0, 0x64, 0x5a);

/// What the wheel turns on the strip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum What {
    Trim,
    Gate,
    Output,
}

/// The board's noise gate: the first "gate" block (its index).
fn gate_of(b: &board::Board) -> Option<usize> {
    b.blocks.iter().position(|x| x.def().id == "gate")
}

fn gate_model() -> usize {
    board::catalog::find("gate").unwrap_or(0)
}

/// Over this many dB of hiss gain (`board::hiss_gain_db`) a board hisses audibly between notes: the strip offers its
/// gate (P38.2). The song tones get a gate from the same figure (`rs_gear::board_keyed`).
pub const HISS_HINT_DB: f32 = board::GATE_HISS_DB;

/// How much louder a board makes an idle input's hiss (dB), measured off the UI thread (a NAM board takes a moment);
/// None until it's known. The last board's figure is kept.
pub fn hiss_gain(b: &board::Board) -> Option<f32> {
    use std::sync::Mutex;
    static HISS: Mutex<Option<(String, Option<f32>)>> = Mutex::new(None);
    let sig = b.to_json().to_string();
    let mut g = HISS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, v)) = g.as_ref()
        && *s == sig
    {
        return *v;
    }
    *g = Some((sig.clone(), None));
    let b = b.clone();
    std::thread::spawn(move || {
        let v = board::hiss_gain_db(&b);
        let mut g = HISS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((s, slot)) = g.as_mut()
            && *s == sig
        {
            *slot = Some(v);
        }
    });
    None
}

/// Whether to offer the gate: the board hisses between notes and no gate is on.
fn hissy(b: &board::Board) -> Option<f32> {
    if b.blocks.iter().any(|x| x.on && x.def().id == "gate") {
        return None;
    }
    hiss_gain(b).filter(|g| *g > HISS_HINT_DB)
}

/// The trim of the input now (dB), and a change to it kept with its calibration (config.json `input_cal`).
fn trim(st: &AppState) -> f32 {
    crate::calibrate::current_input(st)
        .map(|(id, _)| crate::calibrate::load(&st.config, &id).trim_db)
        .unwrap_or(0.0)
}

fn set_trim(st: &mut AppState, db: f32) {
    let Some((id, _)) = crate::calibrate::current_input(st) else {
        return;
    };
    let mut cal = crate::calibrate::load(&st.config, &id);
    cal.trim_db = (db * 2.0).round() / 2.0;
    cal.trim_db = cal
        .trim_db
        .clamp(crate::calibrate::TRIM.0, crate::calibrate::TRIM.1);
    crate::calibrate::save(&mut st.config, &id, &cal);
    crate::calibrate::apply(st);
}

/// The strip's height (points).
pub const HEIGHT: f32 = 78.0;
/// Its three rows (from its top): the captions, the controls, the small print under them.
const ROW_CAP: f32 = 17.0;
const ROW_MID: f32 = 41.0;
const ROW_LOW: f32 = 64.0;

/// The strip's sections (P37.13): each at least its `min` wide; what's over goes to them by priority up to their
/// `pref` (the scenes first, then the input, the tuner, the gate, the output), and the rest opens the gaps, so no
/// section ever runs into the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripLayout {
    pub input: Rect,
    pub gate: Rect,
    pub tuner: Rect,
    pub scenes: Rect,
    pub output: Rect,
}

const PAD: f32 = 20.0;
const GAP: f32 = 28.0;
/// (min, pref, max) of input, gate, tuner, scenes, output.
const SIZES: [(f32, f32, f32); 5] = [
    (226.0, 290.0, 320.0),
    (104.0, 160.0, 180.0),
    (92.0, 150.0, 170.0),
    (256.0, 480.0, 680.0),
    (110.0, 220.0, 380.0),
];

pub fn strip_layout(r: Rect) -> StripLayout {
    let avail = r.width() - 2.0 * px(PAD);
    let mins: f32 = SIZES.iter().map(|s| px(s.0)).sum();
    let gaps0 = 4.0 * px(GAP);
    let mut w: Vec<f32> = SIZES.iter().map(|s| px(s.0)).collect();
    let mut extra = avail - mins - gaps0;
    if extra < 0.0 {
        // (narrower than every section's least: all shrink alike, the gaps too)
        let k = (avail / (mins + gaps0)).max(0.1);
        w.iter_mut().for_each(|x| *x *= k);
        return place(r, &w, px(GAP) * k);
    }
    for (i, cap) in [(0, 1), (4, 1), (2, 1), (1, 1), (3, 1), (3, 2), (4, 2)] {
        let target = px(if cap == 1 { SIZES[i].1 } else { SIZES[i].2 });
        let add = (target - w[i]).clamp(0.0, extra);
        w[i] += add;
        extra -= add;
    }
    place(r, &w, px(GAP) + extra / 4.0)
}

fn place(r: Rect, w: &[f32], gap: f32) -> StripLayout {
    let mut x = r.min.x + px(PAD);
    let mut out = [Rect::NOTHING; 5];
    for (i, wi) in w.iter().enumerate() {
        out[i] = Rect::from_min_max(pos2(x, r.min.y), pos2(x + wi, r.max.y));
        x += wi + gap;
    }
    StripLayout {
        input: out[0],
        gate: out[1],
        tuner: out[2],
        scenes: out[3],
        output: out[4],
    }
}

/// The first of `options` that fits `w` in small caps (the last cut short if none does): a caption drops its
/// secondary words before it is cut.
fn fit_caps(ctx: &egui::Context, options: &[String], w: f32) -> String {
    for o in options {
        if text::width(ctx, o, Style::caps(10.0, Color32::WHITE)) <= w {
            return o.clone();
        }
    }
    options.last().cloned().unwrap_or_default()
}

/// A small knob on the strip, dragged up and down (B12: the gate and trim were wheel-only over tiny text): its
/// turn now; its value on the strip's low row. Returns the new turn while it's dragged, whether it was
/// double-clicked (back to its default), and whether a drag ended.
fn small_knob(
    room: &mut RoomView,
    ui: &Ui,
    c: Pos2,
    id: &'static str,
    u: f32,
    value: &str,
) -> (Option<f32>, bool, bool) {
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    let r = px(13.0);
    let hit = Rect::from_center_size(c, Vec2::splat(r * 2.0 + px(12.0)));
    let resp = interact(ui, hit, ("strip-knob", id), Sense::click_and_drag());
    let mut out = None;
    let mut ended = false;
    if resp.hovered() || resp.dragged() {
        ctx.set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if resp.is_pointer_button_down_on()
        && room.strip_drag.is_none()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        room.strip_drag = Some((id, pos.y, u));
    }
    if let Some((d, y0, u0)) = room.strip_drag
        && d == id
    {
        if ctx.input(|i| i.pointer.any_down()) {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let span = if ctx.input(|i| i.modifiers.shift) {
                    px(700.0)
                } else {
                    px(160.0)
                };
                out = Some((u0 + (y0 - pos.y) / span).clamp(0.0, 1.0));
            }
        } else {
            room.strip_drag = None;
            ended = true;
        }
    }
    let shown = out.unwrap_or(u);
    qrs_ui::knobs::knob(
        &p,
        c,
        r * 2.0,
        shown,
        qrs_ui::knobs::Scale {
            ticks: false,
            ..Default::default()
        },
        None,
    );
    draw::label(
        &p,
        value,
        pos2(c.x, c.y - px(ROW_MID) + px(ROW_LOW)),
        11.5,
        800,
        th.text,
        Align2::CENTER_CENTER,
    );
    (out, resp.double_clicked(), ended)
}

/// The strip along the room's foot (P37.1, the audit's section 5; laid out by `strip_layout`, P37.13): the input (its
/// meter, what kind it is, trim, Calibrate), the gate, the tuner, the scenes (named by you) and the true output.
pub fn strip(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    room.strip_spots.clear();
    draw::card(&p, r, px(18.0));
    let lay = strip_layout(r);
    let y = r.min.y;
    let (yc, ym, yl) = (y + px(ROW_CAP), y + px(ROW_MID), y + px(ROW_LOW));
    let sep = |a: Rect, b: Rect| {
        let x = ((a.max.x + b.min.x) / 2.0).round();
        p.line_segment(
            [pos2(x, y + px(14.0)), pos2(x, r.max.y - px(14.0))],
            Stroke::new(1.0, th.divider),
        );
    };
    let cap = |s: &str, x: f32, w: f32| {
        text::caps(&p, s, pos2(x, yc), 10.0, th.caption, Align2::LEFT_CENTER, Some(w));
    };
    // A. the input: what it is, its meter and level, Calibrate; its trim at the section's right
    let ar = lay.input;
    let trim_w = px(74.0);
    let main_w = ar.width() - trim_w - px(10.0);
    let input = crate::calibrate::current_input(st);
    let kind = input
        .as_ref()
        .map(|(id, name)| {
            let cal = crate::calibrate::load(&st.config, id);
            let k = match cal.kind {
                crate::calibrate::Kind::Auto => crate::calibrate::auto_kind(st, name),
                k => k,
            };
            match k {
                crate::calibrate::Kind::Modeller => "MODELLER",
                crate::calibrate::Kind::Mic => "MIC",
                _ if cal.instrument.is_bass() => "DRY BASS",
                _ => "DRY GUITAR",
            }
        })
        .unwrap_or("NO INPUT");
    let dev = input
        .as_ref()
        .map(|(_, n)| n.split('(').next().unwrap_or(n).trim().to_uppercase())
        .unwrap_or_default();
    let words = if dev.is_empty() {
        vec![format!("INPUT  \u{b7}  {kind}"), "INPUT".into()]
    } else {
        vec![
            format!("INPUT  \u{b7}  {dev}  \u{b7}  {kind}"),
            format!("INPUT  \u{b7}  {kind}"),
            "INPUT".into(),
        ]
    };
    cap(&fit_caps(&ctx, &words, main_w), ar.min.x, main_w);
    let kind_word = match kind {
        "MODELLER" => "modeller",
        "MIC" => "mic",
        _ => "dry",
    };
    if !dev.is_empty() {
        let hr = Rect::from_min_size(pos2(ar.min.x, yc - px(8.0)), vec2(main_w, px(16.0)));
        if interact(ui, hr, "strip-input-name", Sense::hover()).hovered() {
            room.rack.tip = Some((
                format!(
                    "Input: {}  \u{b7}  {}. {}",
                    input.as_ref().map_or("", |x| x.1.as_str()),
                    kind.to_lowercase(),
                    super::plain::term(kind_word).unwrap_or("")
                ),
                "Settings > Sound changes it".into(),
                pos2(hr.min.x, hr.min.y - px(4.0)),
                Align2::LEFT_BOTTOM,
            ));
        }
    }
    let segs = 24;
    let seg_g = px(2.0);
    let seg_w = ((main_w - seg_g * (segs - 1) as f32) / segs as f32).clamp(px(3.0), px(7.0));
    let lit_to = ((room.listen.peak_db + 60.0) / 60.0 * 24.0).clamp(0.0, 24.0);
    for i in 0..segs {
        let col = if i < 16 {
            GREEN
        } else if i < 21 {
            AMBER
        } else {
            RED
        };
        let sr = Rect::from_min_size(
            pos2(ar.min.x + i as f32 * (seg_w + seg_g), ym - px(7.0)),
            vec2(seg_w, px(14.0)),
        );
        p.rect_filled(
            sr,
            px(1.5),
            if (i as f32) < lit_to {
                col
            } else {
                col.gamma_multiply(0.15)
            },
        );
    }
    let lvl = room.listen.level_db;
    let lr = draw::label(
        &p,
        &if lvl > -90.0 {
            format!("{lvl:.0} dB")
        } else {
            "\u{2013}".into()
        },
        pos2(ar.min.x, yl),
        11.5,
        800,
        th.text_2,
        Align2::LEFT_CENTER,
    );
    let cal = rig_link(
        ui,
        pos2(lr.max.x + px(12.0), yl),
        "strip-calibrate",
        "Calibrate\u{2026}",
    );
    super::plain::hover(&mut room.rack.tip, ui, cal.rect, "calibrate");
    if cal.clicked() {
        room.want_settings = Some("audio");
    }
    let tc = pos2(ar.max.x - trim_w / 2.0, ym);
    let ib = draw::caps(
        &p,
        "INPUT BOOST",
        pos2(tc.x, yc),
        8.5,
        th.caption,
        Align2::CENTER_CENTER,
    );
    super::plain::hover(&mut room.rack.tip, ui, ib.expand(px(4.0)), "input boost");
    let tdb = trim(st);
    if input.is_some() {
        let tu = (tdb - crate::calibrate::TRIM.0) / (crate::calibrate::TRIM.1 - crate::calibrate::TRIM.0);
        let (nu, reset, ended) = small_knob(room, ui, tc, "trim", tu, &format!("{tdb:+.1} dB"));
        if ended {
            st.save_config();
        }
        if let Some(u) = nu {
            set_trim(
                st,
                crate::calibrate::TRIM.0 + u * (crate::calibrate::TRIM.1 - crate::calibrate::TRIM.0),
            );
        }
        if reset {
            set_trim(st, 0.0);
        }
        room.strip_spots
            .push((Rect::from_center_size(tc, Vec2::splat(px(40.0))), What::Trim));
    } else {
        draw::label(&p, "\u{2013}", tc, 13.0, 800, th.caption, Align2::CENTER_CENTER);
    }
    sep(lay.input, lay.gate);
    // B. the gate: its threshold knob and on / off, or (none on this sound) one key that adds it
    let gr = lay.gate;
    cap("NOISE GATE", gr.min.x, gr.width());
    super::plain::hover(
        &mut room.rack.tip,
        ui,
        Rect::from_min_size(pos2(gr.min.x, yc - px(8.0)), vec2(px(90.0), px(16.0))),
        "gate",
    );
    let b = room.rack.board_now(st);
    let hiss = hissy(&b);
    match gate_of(&b).map(|i| b.blocks[i]) {
        Some(g) => {
            let ki = g.def().knob_index("threshold").unwrap_or(0);
            let kd = g.def().knobs[ki];
            let gc = pos2(gr.min.x + px(16.0), ym);
            let (nu, reset, ended) = small_knob(room, ui, gc, "gate", kd.to_u(g.v[ki]), &kd.text(g.v[ki]));
            if let Some(u) = nu {
                room.rack.set_knob(st, g.uid, ki, kd.from_u(u));
            }
            if ended {
                room.rack.board_commit(st);
            }
            if reset {
                room.rack.set_knob(st, g.uid, ki, kd.def);
                room.rack.board_commit(st);
            }
            room.strip_spots
                .push((Rect::from_center_size(gc, Vec2::splat(px(40.0))), What::Gate));
            let open = room.listen.level_db > g.v[ki];
            let (word, col) = if !g.on && hiss.is_some() {
                ("Off \u{b7} hiss? turn it on", AMBER)
            } else if !g.on {
                ("Off", th.caption)
            } else if open {
                // (P41.3: "open" read as letting the hiss through; it says what the gate is doing)
                ("On \u{b7} notes pass", GREEN)
            } else {
                ("On \u{b7} silencing hiss", th.text_2)
            };
            let wx = gc.x + px(26.0);
            p.circle_filled(pos2(wx + px(3.5), ym), px(3.5), col);
            let resp = rig_link(ui, pos2(wx + px(12.0), ym), "strip-gate-on", word);
            if resp.hovered() {
                room.rack.tip = Some((
                    match hiss {
                        Some(h) => format!(
                            "Hiss while you're not playing? Turn on the gate: this sound makes your input's idle noise \
                             {h:.0} dB louder"
                        ),
                        None => format!(
                            "The gate is {}: click to turn it {}",
                            if g.on { "on" } else { "off" },
                            if g.on { "off" } else { "on" }
                        ),
                    },
                    // (P37.6c: a capture has its own gate in front of it; two gates in a row is one too many)
                    if b.blocks
                        .iter()
                        .any(|x| x.on && x.def().file && x.get("gate").is_some_and(|v| v > -89.9))
                    {
                        "this gates the whole chain; the capture's own Gate is on too: one is enough".into()
                    } else {
                        "this gates the whole chain \u{b7} drag the knob: its threshold".into()
                    },
                    pos2(resp.rect.center().x, resp.rect.min.y - px(4.0)),
                    Align2::CENTER_BOTTOM,
                ));
            }
            if resp.clicked() {
                room.rack.toggle_block(st, g.uid);
            }
        }
        None => {
            let label = if hiss.is_some() {
                "Hiss? + Add a gate"
            } else {
                "+ Add a gate"
            };
            let kw = draw::key_w(&ctx, label, "", false).min(gr.width());
            let kr = Rect::from_min_size(pos2(gr.min.x, ym - px(14.0)), vec2(kw, px(28.0)));
            let resp = draw::key(ui, kr, "strip-gate-add", KeyLook::Dark, label, "", None);
            if resp.hovered() {
                room.rack.tip = Some((
                    match hiss {
                        Some(h) => format!(
                            "Hiss while you're not playing? Add a gate: this sound makes your input's idle noise \
                             {h:.0} dB louder"
                        ),
                        None => "No gate on this sound: add one after the amp".into(),
                    },
                    "it quiets hiss between notes".into(),
                    pos2(kr.center().x, kr.min.y - px(4.0)),
                    Align2::CENTER_BOTTOM,
                ));
            }
            if resp.clicked() {
                // (P41.1, D162: after the amp, keyed on the input)
                room.rack.add_block(st, board::gate_slot(&b), gate_model());
            }
        }
    }
    // (P41.1, D163) the gate's threshold over the input's own noise: a couple of seconds hands off
    let learn = match room.noise.progress() {
        Some(f) => format!("Listening\u{2026} hands off {:.0}%", f * 100.0),
        None => "Learn noise".to_string(),
    };
    let lr = rig_link(ui, pos2(gr.min.x, yl), "strip-learn-noise", &learn);
    if lr.hovered() {
        room.rack.tip = Some((
            "Learn your input's noise: keep your hands off the strings for two seconds".into(),
            "the gate then opens 8 dB over it".into(),
            pos2(lr.rect.center().x, lr.rect.min.y - px(4.0)),
            Align2::CENTER_BOTTOM,
        ));
    }
    if lr.clicked() {
        if room.noise.progress().is_some() {
            room.noise.cancel();
        } else {
            room.noise.start();
        }
    }
    // (P41.1, D162) how far the gate closes: a click steps gentle -30, firm -60, Full
    if let Some(g) = gate_of(&b).map(|i| b.blocks[i])
        && let Some(ki) = g.def().knob_index("range")
    {
        let kd = g.def().knobs[ki];
        let v = g.v[ki];
        let rr = rig_link(
            ui,
            pos2(lr.rect.max.x + px(12.0), yl),
            "strip-gate-range",
            &format!("Range {}", kd.text(v)),
        );
        if rr.hovered() {
            room.rack.tip = Some((
                "How far the gate turns the sound down when it closes".into(),
                "click: gentle (-30 dB), firm (-60 dB), Full (silence)".into(),
                pos2(rr.rect.center().x, rr.rect.min.y - px(4.0)),
                Align2::CENTER_BOTTOM,
            ));
        }
        if rr.clicked() {
            let next = if v > -45.0 {
                -60.0
            } else if v > -89.9 {
                -90.0
            } else {
                -30.0
            };
            room.rack.set_knob(st, g.uid, ki, next);
            room.rack.board_commit(st);
        }
    }
    sep(lay.gate, lay.tuner);
    // C. the tuner (the raw input); with nothing heard it asks for a string
    let tr = lay.tuner;
    cap("TUNER", tr.min.x, tr.width());
    match room.listen.note() {
        Some((name, oct, cents)) => {
            let nr = draw::label(
                &p,
                &name,
                pos2(tr.min.x, ym + px(2.0)),
                28.0,
                800,
                th.text,
                Align2::LEFT_CENTER,
            );
            let in_tune = cents.abs() <= 5;
            draw::label_w(
                &p,
                &format!("{oct}   {}{cents}c", if cents > 0 { "+" } else { "" }),
                pos2(nr.max.x + px(8.0), ym + px(5.0)),
                12.0,
                800,
                if in_tune { GREEN } else { AMBER },
                Align2::LEFT_CENTER,
                (tr.max.x - nr.max.x - px(8.0)).max(px(10.0)),
            );
        }
        None => {
            draw::label_w(
                &p,
                "Play a string",
                pos2(tr.min.x, ym),
                12.5,
                700,
                th.caption,
                Align2::LEFT_CENTER,
                tr.width(),
            );
        }
    }
    sep(lay.tuner, lay.scenes);
    // D. the scenes: each its name (yours, or "Scene") and its number as the key that picks it; an empty one is
    // quieter and says on hover that it plays as scene 1
    let dr = lay.scenes;
    // (P41.3: what a scene is, said in its caps; the rest on hover)
    let words = vec![
        "SCENES  \u{b7}  4 VERSIONS OF THIS SOUND (VERSE, SOLO\u{2026})  \u{b7}  KEYS 1-4".to_string(),
        "SCENES  \u{b7}  4 VERSIONS OF THIS SOUND  \u{b7}  KEYS 1-4".to_string(),
        "SCENES  \u{b7}  VERSIONS OF THIS SOUND".to_string(),
        "SCENES  \u{b7}  KEYS 1-4".to_string(),
        "SCENES".to_string(),
    ];
    cap(&fit_caps(&ctx, &words, dr.width()), dr.min.x, dr.width());
    let sh = Rect::from_min_size(pos2(dr.min.x, yc - px(8.0)), vec2(dr.width(), px(16.0)));
    if ui.rect_contains_pointer(sh) {
        room.rack.tip = Some((
            "Scenes: up to 4 versions of this sound (verse, solo...), saved with it. Keys 1-4 pick one; an empty one \
             starts as scene 1."
                .into(),
            "double-click a scene to name it".into(),
            pos2(sh.center().x, sh.min.y - px(4.0)),
            Align2::CENTER_BOTTOM,
        ));
    }
    let kg = px(8.0);
    let kw = (dr.width() - kg * (SCENES - 1) as f32) / SCENES as f32;
    for i in 0..SCENES {
        let sr = Rect::from_min_size(
            pos2(dr.min.x + i as f32 * (kw + kg), ym - px(16.0)),
            vec2(kw, px(32.0)),
        );
        let on = room.state.scene == i;
        let has = room.state.scenes[i].is_some() || on;
        let naming = room.typing == Some(Typing::Scene(i));
        let named = !room.state.names[i].trim().is_empty();
        let label = if naming {
            format!("{}\u{2581}", room.state.names[i])
        } else if named {
            room.state.names[i].trim().to_string()
        } else {
            "Scene".to_string()
        };
        let num = format!("{}", i + 1);
        let resp = draw::key(
            ui,
            sr,
            ("strip-scene", i),
            if on { KeyLook::Lit } else { KeyLook::Dark },
            &label,
            &num,
            None,
        );
        if !has {
            paint::wash(&p, sr.shrink(px(2.0)), th.base, 0.45);
        }
        let col = SCENE_COLOURS[i];
        p.rect_filled(
            Rect::from_min_size(
                pos2(sr.min.x + px(10.0), sr.min.y + px(4.0)),
                vec2(px(16.0), px(3.0)),
            ),
            px(1.5),
            rgb(col).gamma_multiply(if on { 1.0 } else { 0.5 }),
        );
        if resp.hovered() && !naming {
            let what = if has {
                format!(
                    "{}: click to play it",
                    super::rigs::scene_label(&room.state.names, i)
                )
            } else {
                format!(
                    "Scene {} is empty: it plays as scene 1 until you change something on it",
                    i + 1
                )
            };
            room.rack.tip = Some((
                what,
                format!("{} \u{b7} double-click to name", i + 1),
                pos2(sr.center().x, sr.min.y - px(4.0)),
                Align2::CENTER_BOTTOM,
            ));
        }
        if resp.double_clicked() {
            room.typing = Some(Typing::Scene(i));
        } else if resp.clicked() {
            room.scene(st, i);
        }
    }
    sep(lay.scenes, lay.output);
    // E. the true output: the guitar's level to the speakers (each sound's own level is the quick controls' Level)
    let er = lay.output;
    let g = room
        .rack
        .engine()
        .map(|e| e.guitar_gain())
        .unwrap_or(st.config.f64("guitar_volume", 0.8) as f32);
    let db = 20.0 * g.max(1e-4).log10();
    let vr = draw::label(
        &p,
        &if g < 1e-3 {
            "off".to_string()
        } else {
            format!("{db:+.1} dB")
        },
        pos2(er.max.x - px(4.0), yc),
        11.5,
        800,
        th.text,
        Align2::RIGHT_CENTER,
    );
    cap("OUTPUT", er.min.x, (vr.min.x - er.min.x - px(6.0)).max(px(10.0)));
    let track = Rect::from_min_max(
        pos2(er.min.x + px(6.0), ym - px(3.0)),
        pos2(er.max.x - px(6.0), ym + px(3.0)),
    );
    let hit = track.expand2(vec2(px(8.0), px(12.0)));
    let resp = interact(ui, hit, "strip-output", Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    let mut gu = g.clamp(0.0, 1.0);
    if (resp.dragged() || resp.is_pointer_button_down_on())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        gu = ((pos.x - track.min.x) / track.width()).clamp(0.0, 1.0);
        set_output(room, st, gu);
    }
    if resp.drag_stopped() || resp.clicked() {
        st.save_config();
    }
    room.strip_spots.push((hit, What::Output));
    paint::inset(&p, track, Some(px(3.0)), 1.0, paint::Fill::Color(th.track));
    p.rect_filled(
        Rect::from_min_max(track.min, pos2(track.min.x + track.width() * gu, track.max.y)),
        px(3.0),
        th.accent,
    );
    qrs_ui::keys::thumb(
        &p,
        pos2(track.min.x + track.width() * gu, track.center().y),
        px(14.0),
    );
    draw::label_w(
        &p,
        if st.instrument().is_bass() {
            "the bass to your speakers"
        } else {
            "the guitar to your speakers"
        },
        pos2(er.min.x, yl),
        10.0,
        600,
        th.caption,
        Align2::LEFT_CENTER,
        er.width(),
    );
}

/// The guitar's output level (the engine's guitar gain, config.json `guitar_volume`: the player's GUITAR knob).
fn set_output(room: &mut RoomView, st: &mut AppState, g: f32) {
    let g = g.clamp(0.0, 1.0);
    if let Some(e) = room.rack.engine() {
        e.set_guitar_gain(g);
    }
    st.config.set("guitar_volume", serde_json::json!(g));
}

/// A quiet text link on the strip.
fn rig_link(ui: &Ui, pos: Pos2, id: &str, s: &str) -> egui::Response {
    let th = t();
    let stl = Style::new(11.5, 700, th.accent_ink);
    let w = text::width(ui.ctx(), s, stl);
    let r = Rect::from_min_size(pos2(pos.x, pos.y - px(10.0)), vec2(w, px(20.0)));
    let resp = interact(ui, r, id, Sense::click());
    text::draw(ui.painter(), s, r.left_center(), Align2::LEFT_CENTER, stl, None);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

pub fn wheel(room: &mut RoomView, st: &mut AppState, pos: Pos2, notches: f32, shift: bool) -> bool {
    let Some(&(_, what)) = room.strip_spots.iter().rev().find(|(r, _)| r.contains(pos)) else {
        return false;
    };
    let step = if shift { 0.5 } else { 1.0 } * notches.signum();
    match what {
        What::Trim => set_trim(st, trim(st) + step * 0.5),
        What::Gate => {
            let b = room.rack.board_now(st);
            let Some(i) = gate_of(&b) else { return true };
            let g = b.blocks[i];
            let k = g.def().knob_index("threshold").unwrap_or(0);
            room.rack.set_knob(st, g.uid, k, g.v[k] + step);
            room.wheel_at = Some(std::time::Instant::now());
        }
        What::Output => {
            let g = room.rack.engine().map(|e| e.guitar_gain()).unwrap_or(0.8);
            set_output(room, st, g + step * 0.02);
            st.save_config();
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P37.13: the strip's sections never overlap, stay inside it and keep at least their least width, from the
    /// smallest window to a wide one.
    #[test]
    fn the_strip_sections_share_the_width_without_overlapping() {
        for w in [980.0, 1100.0, 1440.0, 1920.0, 2560.0] {
            let area = Rect::from_min_size(Pos2::ZERO, vec2(w, 900.0));
            let (_, _, r) = super::super::room_rects(area);
            let l = strip_layout(r);
            let all = [l.input, l.gate, l.tuner, l.scenes, l.output];
            for (i, a) in all.iter().enumerate() {
                assert!(r.contains_rect(*a), "{w}: section {i} {a:?} outside {r:?}");
                for b in &all[i + 1..] {
                    assert!(a.max.x + 8.0 <= b.min.x, "{w}: section {i} runs into the next");
                }
                if w >= 1100.0 {
                    assert!(
                        a.width() + 0.5 >= px(SIZES[i].0),
                        "{w}: section {i} under its least"
                    );
                }
            }
            if w >= 1920.0 {
                assert!(l.scenes.width() >= px(SIZES[3].1) - 0.5, "{w}: scenes squeezed");
            }
        }
    }
}
