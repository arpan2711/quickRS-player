//! The controls in place of the tiles (port of the controls half of rsplay/app.py `PlayerView`): the practice
//! module (speed and difficulty knobs, the loop on a small screen with its keys, level-up), the playback commands
//! as flat rows with their keys, and Part / Tab view / Tiles / Lyrics / Sound & guitar as rows that open to their
//! keys. Scrolls when it doesn't fit. Laid out in the Python app's whole screen pixels (see `tab`'s helpers).

use std::collections::BTreeSet;

use eframe::egui;
use egui::{Align2, CursorIcon, Sense, Ui, pos2};
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::knobs::{self, Scale};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::Style;
use qrs_ui::theme::{fmt_time, t};
use qrs_ui::widgets::{self, ButtonStyle, interact};
use serde_json::Value;

use super::tab::{PRect, int_, int_pt, ipx, pt, put, rpx, sc, tw};
use super::{PlayerView, SPEEDS, tiles, tone_label};
use crate::state::AppState;

pub fn panel_open(st: &AppState) -> BTreeSet<String> {
    st.config
        .get("panel_open")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

pub fn set_panel_open(st: &mut AppState, open: &BTreeSet<String>) {
    st.config.set(
        "panel_open",
        Value::Array(open.iter().map(|s| Value::String(s.clone())).collect()),
    );
}

/// The first word of a group's title is its key ("SOUND & GUITAR" -> "SOUND").
fn group_key(title: &str) -> &str {
    title.split_whitespace().next().unwrap_or(title)
}

pub fn toggle_group(st: &mut AppState, key: Option<&str>, open_all: Option<bool>) {
    let mut opened = panel_open(st);
    match (key, open_all) {
        (_, Some(true)) => {
            opened = ["PART", "TAB", "TILES", "LYRICS", "SOUND"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        }
        (_, Some(false)) => opened.clear(),
        (Some(k), None) if !opened.remove(k) => {
            opened.insert(k.to_string());
        }
        _ => {}
    }
    set_panel_open(st, &opened);
}

/// A key on the controls: its label, key cap, what it does (None: a read-only value) and whether it's on.
#[derive(Clone, Debug)]
pub struct Btn {
    pub label: String,
    pub key: String,
    pub action: Option<String>,
    pub active: bool,
}

impl Btn {
    fn new(label: impl Into<String>, key: impl Into<String>, action: impl Into<String>) -> Btn {
        Btn {
            label: label.into(),
            key: key.into(),
            action: Some(action.into()),
            active: false,
        }
    }
    fn value(label: impl Into<String>) -> Btn {
        Btn {
            label: label.into(),
            key: String::new(),
            action: None,
            active: false,
        }
    }
    fn onoff(label: &str, on: bool, key: &str, action: &str) -> Btn {
        Btn {
            label: format!("{label}: {}", if on { "on" } else { "off" }),
            key: key.into(),
            action: Some(action.into()),
            active: on,
        }
    }
}

/// Columns: a count, or relative widths.
#[derive(Clone, Debug)]
pub enum Cols {
    N(usize),
    Fr(Vec<f64>),
}

type Section = (String, Cols, Vec<Btn>);

/// Column widths of a "less / value / more" row.
const STEPPER: [f64; 3] = [1.0, 0.46, 1.0];
const PLAYBACK_ROWS: [(&str, &[(&str, &str)]); 4] = [
    ("Restart", &[("Home", "start")]),
    (
        "Previous / next section",
        &[("PgUp", "prev_section"), ("PgDn", "next_section")],
    ),
    ("Back / forward", &[("←", "back"), ("→", "fwd")]),
    ("Rewind 5 seconds", &[("−", "rewind")]),
];

/// `ui.layout_grid`: buttons in a grid of equal columns, or columns of the given relative widths (pixels). Returns
/// their rects and the height used.
fn layout_grid(n: usize, x: f64, y: f64, width: f64, cols: &Cols, bh: f64, gap: f64) -> (Vec<PRect>, f64) {
    let fracs: Vec<f64> = match cols {
        Cols::N(k) => vec![1.0; (*k).max(1)],
        Cols::Fr(v) => v.clone(),
    };
    let nc = fracs.len();
    let room = width - gap * (nc as f64 - 1.0);
    let total: f64 = fracs.iter().sum();
    let mut xs = Vec::new();
    let mut cx = x;
    for fr in &fracs {
        let wd = room * fr / total;
        xs.push((cx, wd));
        cx += wd + gap;
    }
    let rects = (0..n)
        .map(|i| {
            let (r, c) = (i / nc, i % nc);
            let (bx, bw) = xs[c];
            PRect {
                x: int_(bx),
                y: int_(y + r as f64 * (bh + gap)),
                w: int_(bx + bw) - int_(bx),
                h: bh,
            }
        })
        .collect();
    let rows = n.div_ceil(nc);
    (rects, rows as f64 * bh + rows.saturating_sub(1) as f64 * gap)
}

/// `blit_text` in a caption colour and weight (pixels).
#[allow(clippy::too_many_arguments)]
fn txt(
    p: &egui::Painter,
    s: &str,
    x: f64,
    y: f64,
    size: f32,
    color: egui::Color32,
    weight: u16,
    anchor: Align2,
    max_w: Option<f64>,
) -> PRect {
    put(p, s, x, y, anchor, Style::new(size, weight, color), max_w)
}

/// `ui.caps`: a small-caps heading (pixels).
fn caps(
    p: &egui::Painter,
    s: &str,
    x: f64,
    y: f64,
    size: f32,
    color: egui::Color32,
    anchor: Align2,
) -> PRect {
    put(p, s, x, y, anchor, Style::caps(size, color), None)
}

/// `neu.keycap` at (x, y) by `anchor` (pixels), sunk into the material. Returns its rect.
fn cap(p: &egui::Painter, key: &str, x: f64, y: f64, anchor: Align2) -> PRect {
    PRect::of(keycap(
        p,
        key,
        pos2(pt(int_(x)), pt(int_(y))),
        anchor,
        11.0,
        CapOn::Material,
    ))
}

/// `neu.keycap_width` (size 11), in pixels.
fn keycap_w(ctx: &egui::Context, key: &str) -> f64 {
    if key.is_empty() {
        0.0
    } else {
        tw(ctx, key, Style::new(11.0, 700, egui::Color32::WHITE)) + rpx(12.0)
    }
}

impl PlayerView {
    /// (title, hint, sections) for the rows of the controls panel that open to their keys. Each section is
    /// (subtitle, columns (a count, or relative widths), buttons); a blank subtitle runs straight on from the
    /// section above. Every button shows its key.
    fn control_groups(&self, st: &AppState) -> Vec<(&'static str, &'static str, Vec<Section>)> {
        let parts = &self.entry.arrangements;
        let labels = self.part_labels();
        let tile_btns: Vec<Btn> = tiles::tile_order(st)
            .iter()
            .map(|(k, on)| {
                Btn::onoff(
                    tiles::tile_label(k),
                    *on,
                    if *k == "lyrics" { "Y" } else { "" },
                    &format!("tile:{k}"),
                )
            })
            .collect();
        let mut sound: Vec<Section> = vec![(
            "VOLUME".into(),
            Cols::Fr(STEPPER.to_vec()),
            vec![
                Btn::new("Song −", "9", "vol-"),
                Btn::value(format!(
                    "{}%",
                    (self.transport.volume() as f64 * 100.0).round_ties_even() as i64
                )),
                Btn::new("Song +", "0", "vol+"),
            ],
        )];
        if let Some(e) = self.engine() {
            sound.push((
                String::new(),
                Cols::Fr(STEPPER.to_vec()),
                vec![
                    Btn::new("Guitar −", "7", "gvol-"),
                    Btn::value(format!(
                        "{}%",
                        (e.guitar_gain() as f64 * 100.0).round_ties_even() as i64
                    )),
                    Btn::new("Guitar +", "8", "gvol+"),
                ],
            ));
            sound.push((
                "GUITAR".into(),
                Cols::N(2),
                vec![
                    Btn::onoff("Hear guitar", e.monitor(), "M", "monitor"),
                    Btn::new(format!("Tone: {}", tone_label(&e.tone())), "T", "tone"),
                    Btn::new("Timing earlier", "Ctrl+,", "gtime-"),
                    Btn::new("Timing later", "Ctrl+.", "gtime+"),
                ],
            ));
            sound.push((
                String::new(),
                Cols::N(1),
                vec![Btn::new("All settings", "F2", "settings")],
            ));
        }
        let part_btns: Vec<Btn> = parts
            .iter()
            .zip(&labels)
            .enumerate()
            .map(|(i, (a, label))| Btn {
                label: label.clone(),
                key: if i < 6 { (i + 1).to_string() } else { String::new() },
                action: Some(format!("part:{}", a.key)),
                active: a.key == self.part_key,
            })
            .collect();
        let neck = st.config.str("note_view", "piano") == "guitar";
        vec![
            (
                "PART",
                "A next  ·  Shift+A previous  ·  1–6",
                vec![(
                    String::new(),
                    Cols::N(if parts.len() > 1 { 2 } else { 1 }),
                    part_btns,
                )],
            ),
            (
                "TAB VIEW",
                "rhythm notation N  ·  zoom ↑ ↓",
                vec![
                    (
                        String::new(),
                        Cols::N(2),
                        vec![
                            Btn::new("Zoom out", "↓", "zoom_out"),
                            Btn::new("Zoom in", "↑", "zoom_in"),
                        ],
                    ),
                    (
                        "NOTATION".into(),
                        Cols::N(2),
                        vec![
                            Btn::onoff("Notation", self.nota.show, "N", "nota:show"),
                            Btn::onoff("Snap to beats", self.nota.snap, "", "nota:snap"),
                            Btn::onoff("Rests", self.nota.rests, "", "nota:rests"),
                            Btn::onoff("Beams & ties", self.nota.beams, "", "nota:beams"),
                            Btn::onoff("Triplets", self.nota.triplets, "", "nota:triplets"),
                        ],
                    ),
                ],
            ),
            (
                "TILES",
                "show, hide and reorder",
                vec![
                    (String::new(), Cols::N(2), tile_btns),
                    (
                        String::new(),
                        Cols::N(1),
                        vec![Btn::new(
                            format!("Note tile: {}", if neck { "guitar neck" } else { "piano" }),
                            "",
                            "noteview",
                        )],
                    ),
                ],
            ),
            (
                "LYRICS",
                "next line",
                vec![(
                    String::new(),
                    Cols::N(1),
                    vec![Btn::onoff("Next line", self.lyrics_next, "", "lyrics_next")],
                )],
            ),
            (
                "SOUND & GUITAR",
                "tone T  ·  hear guitar M  ·  volumes 7 8 9 0",
                sound,
            ),
        ]
    }

    /// The controls in place of the tiles: the practice module (speed and difficulty knobs, the loop and its keys),
    /// the playback commands, and the rest as rows that open to their keys. Scrolls when it doesn't fit, sliding
    /// out under the console's edges.
    pub(super) fn draw_controls(&mut self, ui: &mut Ui, st: &mut AppState, area: egui::Rect) {
        let p = ui.painter().clone();
        if let Some(target) = self.ctrl_target {
            // glide to a module that was asked for (the speed knob)
            self.ctrl_scroll += (target - self.ctrl_scroll) * 0.3;
            if (target - self.ctrl_scroll).abs() < 1.0 {
                self.ctrl_scroll = target;
                self.ctrl_target = None;
            }
            ui.ctx().request_repaint();
        }
        let s = sc();
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        cui.set_clip_rect(area);
        let a = PRect::of(area);
        let top_y = a.y - int_pt(self.ctrl_scroll);
        self.group_y.clear();
        self.group_y.insert("PRACTICE", 0.0);
        let mut y = self.draw_practice_module(&cui, st, a.x, top_y, a.w) + ipx(12.0);
        self.group_y.insert("PLAYBACK", ((y - top_y) / s) as f32);
        y = self.draw_playback_module(&cui, a.x, y, a.w) + ipx(12.0);
        y = self.draw_more_module(&cui, st, a.x, y, a.w);
        self.ctrl_max = ((y - top_y - a.h).max(0.0) / s) as f32;
        self.ctrl_scroll = self.ctrl_scroll.clamp(0.0, self.ctrl_max);
        if self.ctrl_scroll > 0.5 || self.ctrl_scroll < self.ctrl_max - 0.5 {
            // what's behind the edges (the console's face) laid back over the content, fading out inwards; the
            // console is the area grown by its padding, with the Tiles key under it
            let pad = ipx(16.0);
            let console = PRect {
                x: a.x - pad,
                y: a.y - pad,
                w: a.w + 2.0 * pad,
                h: a.h + ipx(12.0) + ipx(48.0) + 2.0 * pad,
            };
            paint::fade_edges(
                &p,
                area,
                self.ctrl_scroll > 0.5,
                self.ctrl_scroll < self.ctrl_max - 0.5,
                24.0,
                paint::FaceKind::Base {
                    rect: console.rect(),
                    sunk: false,
                },
            );
        }
        if let Some(goto) = self.ctrl_goto.take()
            && let Some(&gy) = self.group_y.get(goto)
        {
            // asked for a module (the speed knob): glide to it
            self.ctrl_target = Some(gy.min(self.ctrl_max));
        }
    }

    /// Key caps in a row ("/" between alternatives), centred on cx (pixels).
    fn key_row(&self, p: &egui::Painter, keys: &[&str], cx: f64, cy: f64) -> PRect {
        let th = t();
        let ctx = p.ctx();
        let gap = ipx(3.0);
        let widths: Vec<f64> = keys
            .iter()
            .map(|k| {
                if *k == "/" {
                    tw(ctx, "/", Style::new(11.0, 500, th.caption)) + ipx(6.0)
                } else {
                    keycap_w(ctx, k)
                }
            })
            .collect();
        let total = widths.iter().sum::<f64>() + gap * (keys.len() as f64 - 1.0);
        let mut x = cx - (total / 2.0).floor();
        let mut out = PRect {
            x,
            y: cy,
            w: total,
            h: 0.0,
        };
        for (k, wd) in keys.iter().zip(&widths) {
            if *k == "/" {
                txt(
                    p,
                    "/",
                    x + (wd / 2.0).floor(),
                    cy,
                    11.0,
                    th.caption,
                    500,
                    Align2::CENTER_CENTER,
                    None,
                );
            } else {
                let r = cap(p, k, x, cy, Align2::LEFT_CENTER);
                let (x0, y0) = (out.x.min(r.x), out.y.min(r.y));
                out = PRect {
                    x: x0,
                    y: y0,
                    w: out.right().max(r.right()) - x0,
                    h: out.bottom().max(r.bottom()) - y0,
                };
            }
            x += wd + gap;
        }
        out
    }

    /// Speed and difficulty as knobs (drag up or down, scroll, or the keys), the loop in a small screen with its
    /// keys, and level-up. Returns the module's bottom (pixels).
    fn draw_practice_module(&mut self, ui: &Ui, st: &mut AppState, x: f64, y: f64, w: f64) -> f64 {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let pad = ipx(14.0);
        let bx = ipx(92.0);
        let (kh, kgap) = (ipx(38.0), ipx(8.0));
        let h = pad
            + ipx(18.0)
            + ipx(8.0)
            + bx
            + ipx(26.0)
            + ipx(24.0)
            + ipx(12.0)
            + ipx(44.0)
            + ipx(10.0)
            + 2.0 * kh
            + kgap
            + ipx(12.0)
            + ipx(26.0)
            + pad;
        let module = PRect { x, y, w, h };
        paint::inset(&p, module.rect(), Some(20.0), 6.0, Fill::Base);
        let (lx, rx) = (module.x + pad, module.right() - pad);
        let yy = module.y + pad + ipx(9.0);
        caps(&p, "PRACTICE", lx, yy, 12.0, th.caption, Align2::LEFT_CENTER);
        txt(
            &p,
            "drag a dial, or the keys",
            rx,
            yy,
            12.0,
            th.caption,
            500,
            Align2::RIGHT_CENTER,
            None,
        );
        let mut yy = module.y + pad + ipx(18.0) + ipx(8.0);
        let colw = (rx - lx) / 2.0;
        let span = SPEEDS[SPEEDS.len() - 1] - SPEEDS[0];
        let dials: [(&str, String, &'static str, f32, &[&str]); 2] = [
            (
                "SPEED",
                format!("{}%", (self.speed() * 100.0).round_ties_even() as i64),
                "speed",
                ((1.0 - SPEEDS[0]) / span) as f32,
                &["Ctrl", "=", "/", "="],
            ),
            (
                "DIFFICULTY",
                format!(
                    "{}%",
                    (self.dial_value("diff") as f64 * 100.0).round_ties_even() as i64
                ),
                "diff",
                1.0,
                &["Ctrl", "↓", "/", "Ctrl", "↑"],
            ),
        ];
        for (i, (label, value, kind, home, keys)) in dials.iter().enumerate() {
            let cx = int_(lx + colw * i as f64 + colw / 2.0);
            let hit = PRect {
                x: cx - (bx / 2.0).floor(),
                y: yy,
                w: bx,
                h: bx,
            };
            self.dial_interact(ui, hit.rect(), kind);
            knobs::knob(
                &p,
                pos2(pt(hit.centerx()), pt(hit.centery())),
                56.0,
                self.dial_value(kind),
                Scale {
                    home: Some(*home),
                    ..Default::default()
                },
                None,
            );
            let ly = hit.bottom() + ipx(13.0);
            let lw = tw(&ctx, label, Style::caps(11.0, th.caption));
            let vw = tw(&ctx, value, Style::new(15.0, 800, th.text));
            let x0 = cx - ((lw + ipx(6.0) + vw) / 2.0).floor();
            caps(&p, label, x0, ly, 11.0, th.caption, Align2::LEFT_CENTER);
            txt(
                &p,
                value,
                x0 + lw + ipx(6.0),
                ly,
                15.0,
                th.text,
                800,
                Align2::LEFT_CENTER,
                None,
            );
            self.key_row(&p, keys, cx, ly + ipx(25.0));
        }
        yy += bx + ipx(26.0) + ipx(24.0) + ipx(12.0);
        // the loop
        let disp = PRect {
            x: lx,
            y: yy,
            w: rx - lx,
            h: ipx(44.0),
        };
        paint::display(&p, disp.rect(), Some(12.0), 3.0);
        caps(
            &p,
            "LOOP",
            disp.x + ipx(14.0),
            disp.centery(),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        match (self.loop_a, self.loop_b) {
            (Some(a), Some(b)) => {
                let ta = txt(
                    &p,
                    &format!("{}–{}", fmt_time(a), fmt_time(b)),
                    disp.x + ipx(62.0),
                    disp.centery(),
                    15.0,
                    th.text,
                    800,
                    Align2::LEFT_CENTER,
                    None,
                );
                if !self.tab.measures.is_empty() {
                    let (ma, mb) = (
                        self.tab.measure_at(a).map(|m| m.number).unwrap_or(0),
                        self.tab.measure_at(b - 1e-3).map(|m| m.number).unwrap_or(0),
                    );
                    txt(
                        &p,
                        &if ma == mb {
                            format!("bar {ma}")
                        } else {
                            format!("bars {ma}–{mb}")
                        },
                        ta.right() + ipx(8.0),
                        disp.centery(),
                        12.0,
                        th.caption,
                        500,
                        Align2::LEFT_CENTER,
                        None,
                    );
                }
                if self.loop_passes > 0 {
                    let s = format!(
                        "pass {}{}",
                        self.loop_passes,
                        self.loop_acc
                            .map(|a| format!("  ·  {}%", (a * 100.0).round_ties_even() as i64))
                            .unwrap_or_default()
                    );
                    txt(
                        &p,
                        &s,
                        disp.right() - ipx(14.0),
                        disp.centery(),
                        13.0,
                        th.accent_ink,
                        800,
                        Align2::RIGHT_CENTER,
                        None,
                    );
                }
            }
            (Some(a), None) => {
                txt(
                    &p,
                    &format!("from {}: set the end with ]", fmt_time(a)),
                    disp.x + ipx(62.0),
                    disp.centery(),
                    13.0,
                    th.text_2,
                    700,
                    Align2::LEFT_CENTER,
                    Some(disp.right() - ipx(14.0) - disp.x - ipx(62.0)),
                );
            }
            _ => {
                txt(
                    &p,
                    "off",
                    disp.x + ipx(62.0),
                    disp.centery(),
                    13.0,
                    th.caption,
                    700,
                    Align2::LEFT_CENTER,
                    None,
                );
            }
        }
        yy = disp.bottom() + ipx(10.0);
        let kw = ((rx - lx - kgap) / 2.0).floor();
        let loop_keys = [
            ("Loop start", "[", "loop_start", self.loop_a.is_some()),
            ("Loop end", "]", "loop_end", self.loop_b.is_some()),
            ("Loop this section", "L", "loop_section", false),
            ("Clear loop", "\\", "clear_loop", false),
        ];
        for (i, (label, key, action, on)) in loop_keys.iter().enumerate() {
            let r = PRect {
                x: lx + (i % 2) as f64 * (kw + kgap),
                y: yy + (i / 2) as f64 * (kh + kgap),
                w: kw,
                h: kh,
            };
            if widgets::button(
                ui,
                r.rect(),
                ("loopkey", *action),
                label,
                key,
                ButtonStyle {
                    active: *on,
                    size: 12.0,
                    r: Some(12.0),
                    ..Default::default()
                },
            )
            .clicked()
            {
                self.acts.push((*action).into());
            }
        }
        yy += 2.0 * kh + kgap + ipx(12.0);
        let row = PRect {
            x: lx,
            y: yy,
            w: rx - lx,
            h: ipx(26.0),
        };
        let (sw_w, sw_h) = (ipx(48.0), ipx(26.0));
        let sw = PRect {
            x: row.right() - sw_w,
            y: row.centery() - (sw_h / 2.0).floor(),
            w: sw_w,
            h: sw_h,
        };
        let resp = interact(ui, row.rect(), "auto-level", Sense::click());
        widgets::draw_switch(&p, sw.rect(), egui::Id::new("auto_level"), self.auto_level);
        txt(
            &p,
            "Level up after a clean loop",
            lx,
            row.centery(),
            13.0,
            th.text,
            700,
            Align2::LEFT_CENTER,
            Some(sw.x - lx - ipx(10.0)),
        );
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.acts.push("auto_level".into());
        }
        let _ = st;
        module.bottom()
    }

    /// The playback commands as flat rows with their keys (each key cap clicks too). Returns the bottom (pixels).
    fn draw_playback_module(&mut self, ui: &Ui, x: f64, y: f64, w: f64) -> f64 {
        let p = ui.painter().clone();
        let th = t();
        let (pad, row_h) = (ipx(10.0), ipx(34.0));
        let module = PRect {
            x,
            y,
            w,
            h: pad + ipx(22.0) + ipx(4.0) + PLAYBACK_ROWS.len() as f64 * row_h + pad,
        };
        paint::inset(&p, module.rect(), Some(20.0), 6.0, Fill::Base);
        let hy = module.y + pad + ipx(11.0);
        caps(
            &p,
            "PLAYBACK",
            module.x + pad + ipx(8.0),
            hy,
            12.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        let tr = txt(
            &p,
            "+ arrows: 1 second",
            module.right() - pad - ipx(8.0),
            hy,
            11.0,
            th.caption,
            500,
            Align2::RIGHT_CENTER,
            None,
        );
        cap(&p, "Shift", tr.x - ipx(5.0), hy, Align2::RIGHT_CENTER);
        let mut ry = module.y + pad + ipx(22.0) + ipx(4.0);
        for (label, keys) in PLAYBACK_ROWS {
            let r = PRect {
                x: module.x + pad,
                y: ry,
                w: module.w - 2.0 * pad,
                h: row_h,
            };
            let resp = interact(ui, r.rect(), ("pb-row", label), Sense::click());
            if resp.hovered() {
                paint::raised(&p, r.rect(), Some(10.0), 3.0, Fill::Base, 0.0);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            txt(
                &p,
                label,
                r.x + ipx(10.0),
                r.centery(),
                13.0,
                th.text,
                700,
                Align2::LEFT_CENTER,
                Some(r.w * 0.62),
            );
            let mut clicked_key = false;
            let mut kx = r.right() - ipx(8.0);
            for (key, action) in keys.iter().rev() {
                let kr = cap(&p, key, kx, r.centery(), Align2::RIGHT_CENTER);
                if interact(
                    ui,
                    kr.inflate(ipx(4.0), ipx(10.0)).rect(),
                    ("pb-key", *action),
                    Sense::click(),
                )
                .clicked()
                {
                    self.acts.push((*action).into());
                    clicked_key = true;
                }
                kx = kr.x - ipx(4.0);
            }
            if resp.clicked() && !clicked_key {
                self.acts.push(keys[0].1.into());
            }
            ry += row_h;
        }
        module.bottom()
    }

    /// Part, tab view, tiles, lyrics and sound as rows; a row opens to its keys. Returns the bottom (pixels).
    fn draw_more_module(&mut self, ui: &Ui, st: &mut AppState, x: f64, y: f64, w: f64) -> f64 {
        let p = ui.painter().clone();
        let th = t();
        let s = sc();
        let groups = self.control_groups(st);
        let opened = panel_open(st);
        let (pad_x, pad_y, row_h) = (ipx(10.0), ipx(8.0), ipx(32.0));
        let (bh, gap) = (ipx(36.0), ipx(6.0));
        let (inner_x, inner_w) = (x + pad_x + ipx(4.0), w - 2.0 * pad_x - ipx(8.0));
        // lay the open groups' keys out first: the module's height depends on them
        let mut yy = y + pad_y;
        let mut placed = Vec::new();
        for (title, hint, sections) in &groups {
            let key = group_key(title);
            let row = PRect {
                x: x + pad_x,
                y: yy,
                w: w - 2.0 * pad_x,
                h: row_h,
            };
            self.group_y.insert(group_static(key), ((yy - y) / s) as f32);
            yy += row_h;
            let mut items: Vec<(Option<String>, f64, Vec<(Btn, PRect)>)> = Vec::new();
            if opened.contains(key) {
                yy += ipx(4.0);
                for (sub, cols, btns) in sections {
                    if !sub.is_empty() {
                        items.push((Some(sub.clone()), yy, Vec::new()));
                        yy += ipx(18.0);
                    }
                    let (rects, used) = layout_grid(btns.len(), inner_x, yy, inner_w, cols, bh, gap);
                    yy += used + gap;
                    items.push((None, 0.0, btns.iter().cloned().zip(rects).collect()));
                }
                yy += ipx(6.0);
            }
            placed.push((*title, *hint, key.to_string(), row, items));
        }
        let module = PRect {
            x,
            y,
            w,
            h: yy + pad_y - y,
        };
        paint::inset(&p, module.rect(), Some(20.0), 6.0, Fill::Base);
        for (title, hint, key, row, items) in placed {
            let is_open = opened.contains(&key);
            let resp = interact(ui, row.rect(), ("group", key.clone()), Sense::click());
            if resp.hovered() {
                paint::raised(&p, row.rect(), Some(10.0), 3.0, Fill::Base, 0.0);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.acts.push(format!("group:{key}"));
            }
            let tr = caps(
                &p,
                title,
                row.x + ipx(10.0),
                row.centery(),
                11.0,
                if is_open { th.text } else { th.text_2 },
                Align2::LEFT_CENTER,
            );
            // `neu.icon`: its box centred on the whole pixel
            let (icx, isz) = (row.right() - ipx(12.0), rpx(14.0).max(6.0));
            icons::icon(
                &p,
                if is_open { Icon::Down } else { Icon::Right },
                pos2(pt(icx), pt(row.centery())),
                14.0,
                th.caption,
            );
            let ic_x = icx - (isz / 2.0).floor();
            txt(
                &p,
                hint,
                tr.right() + ipx(10.0),
                row.centery(),
                12.0,
                th.caption,
                500,
                Align2::LEFT_CENTER,
                Some(ic_x - ipx(8.0) - tr.right() - ipx(10.0)),
            );
            for (sub, at, btns) in items {
                if let Some(sub) = sub {
                    caps(
                        &p,
                        &sub,
                        inner_x + ipx(2.0),
                        at + ipx(8.0),
                        10.0,
                        th.caption,
                        Align2::LEFT_CENTER,
                    );
                    continue;
                }
                for (b, r) in btns {
                    let style = ButtonStyle {
                        active: b.active,
                        value: b.action.is_none(),
                        size: 12.0,
                        align: if b.action.is_none() {
                            widgets::Align::Center
                        } else {
                            widgets::Align::Split
                        },
                        ..Default::default()
                    };
                    let id = (
                        "ctrl-btn",
                        b.action.clone().unwrap_or_else(|| b.label.clone()),
                        (r.x as i32, r.y as i32),
                    );
                    if widgets::button(ui, r.rect(), id, &b.label, &b.key, style).clicked()
                        && let Some(a) = b.action
                    {
                        self.acts.push(a);
                    }
                }
            }
        }
        module.bottom()
    }
}

fn group_static(k: &str) -> &'static str {
    match k {
        "PART" => "PART",
        "TAB" => "TAB",
        "TILES" => "TILES",
        "LYRICS" => "LYRICS",
        "SOUND" => "SOUND",
        _ => "OTHER",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_like_python() {
        // ui.layout_grid at scale 1: a stepper row (1, 0.46, 1) 300 wide, gap 6
        let (r, h) = layout_grid(3, 14.0, 100.0, 300.0, &Cols::Fr(STEPPER.to_vec()), 36.0, 6.0);
        // room 288: 117.07..., 53.85..., 117.07...
        assert_eq!((r[0].x, r[0].w), (14.0, 117.0));
        assert_eq!((r[1].x, r[1].w), (137.0, 53.0));
        assert_eq!((r[2].x, r[2].w), (196.0, 118.0));
        assert_eq!(h, 36.0);
        let (r, h) = layout_grid(5, 0.0, 0.0, 100.0, &Cols::N(2), 36.0, 6.0);
        assert_eq!((r[4].x, r[4].y, r[4].w), (0.0, 84.0, 47.0));
        assert_eq!(h, 3.0 * 36.0 + 2.0 * 6.0);
    }
}
