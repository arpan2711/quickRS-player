//! The AMP ROOM button at the foot of the sidebar, on Home and every library page (PLAN.md P22.1): since D73 an amp
//! head (head.rs, the owner's mockup A6): its VU meter's needle follows the guitar's input with a VU's ballistics,
//! the peak lamp in the meter glows with how hard you play, the room's rig is engraved under "Amp Room"; click it or
//! press A for the amp room. New code (no Python source).

use std::f32::consts::PI;

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Painter, Pos2, Rect, Response, Sense, Ui, pos2};
use qrs_ui::paint;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use crate::listen::Listen;

mod head;
use crate::state::AppState;

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A rounded rect's outline (corner arcs of `seg` steps each), clockwise from the top left.
pub fn rounded_points(r: Rect, rad: f32, seg: usize) -> Vec<Pos2> {
    let rad = rad.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    let corners = [
        (pos2(r.min.x + rad, r.min.y + rad), PI),
        (pos2(r.max.x - rad, r.min.y + rad), 1.5 * PI),
        (pos2(r.max.x - rad, r.max.y - rad), 0.0),
        (pos2(r.min.x + rad, r.max.y - rad), 0.5 * PI),
    ];
    let mut out = Vec::with_capacity(4 * (seg + 1));
    for (c, a0) in corners {
        for i in 0..=seg {
            let a = a0 + 0.5 * PI * i as f32 / seg as f32;
            out.push(pos2(c.x + rad * a.cos(), c.y + rad * a.sin()));
        }
    }
    out
}

/// A rounded rect shaded from `top` to `bottom`.
pub fn rounded_vgrad(p: &Painter, r: Rect, rad: f32, top: Color32, bottom: Color32) {
    let pts = rounded_points(r, rad, 6);
    let mut m = egui::Mesh::default();
    let lerp = |y: f32| {
        let k = ((y - r.min.y) / r.height().max(1.0)).clamp(0.0, 1.0);
        top.lerp_to_gamma(bottom, k)
    };
    m.colored_vertex(r.center(), lerp(r.center().y));
    for q in &pts {
        m.colored_vertex(*q, lerp(q.y));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(m);
}

/// What the button shows under its label.
pub struct Look<'a> {
    /// The rig ("Reptilia lead, warmer", or its chain).
    pub rig: &'a str,
}

/// The button in `r` (it stays still under the pointer: no lift on hover. Owner request, 2026-11-03). Returns its
/// response.
pub fn button(ui: &Ui, r: Rect, listen: &Listen, look: &Look) -> Response {
    let resp = interact(ui, r, "amp-room-button", Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    draw(ui.painter(), r, listen, look.rig);
    resp
}

pub fn draw(p: &Painter, r: Rect, listen: &Listen, rig: &str) {
    let hot = listen.hot();
    let body = r;
    // a red glow under it while the input is too hot (the green one while it heard you went: owner, 2026-11-03)
    if hot {
        paint::glow(
            p,
            body,
            Color32::from_rgb(0xf0, 0x64, 0x5a),
            px(16.0),
            Some(0.45),
            Some(body.height() * 0.11),
        );
    }
    // the amp head (D73): the lamp in its meter glows with how hard you play
    let sub = if hot {
        format!("Too hot · input {:.0} dB", listen.peak_db.min(0.0).round() + 0.0)
    } else {
        format!("Rig · {rig}")
    };
    head::draw(p, body, listen.needle, head::lamp_glow(listen), &sub, hot);
}

/// The button's state: the guitar's input, and the room's rig as it names it (read again every 2 s).
#[derive(Default)]
pub struct Amp {
    pub listen: Listen,
    rig_text: Option<(String, std::time::Instant)>,
    /// Screenshots: the button shown hovered.
    pub shot_hover: bool,
}

impl Amp {
    /// The button in `body`: the input on its VU, the rig and the note; it repaints only while there's sound or
    /// the needle is still moving (and looks for sound four times a second otherwise). True when clicked.
    pub fn ui(&mut self, ui: &Ui, st: &AppState, body: Rect, key: bool) -> bool {
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let live = self.listen.update(engine.as_deref());
        if live {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        } else if engine.as_ref().is_some_and(|e| e.status().input.is_some()) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self
            .rig_text
            .as_ref()
            .is_none_or(|(_, at)| at.elapsed().as_secs_f32() > 2.0)
        {
            self.rig_text = Some((crate::room::rig_label(&st.config), std::time::Instant::now()));
        }
        let rig = self.rig_text.as_ref().map(|r| r.0.clone()).unwrap_or_default();
        let look = Look { rig: &rig };
        let resp = button(ui, body, &self.listen, &look);
        if resp.hovered() || self.shot_hover {
            let l = &self.listen;
            let how = if engine.is_none() {
                "audio is off".to_string()
            } else if l.hot() {
                format!("input {:.0} dB, too hot", l.peak_db)
            } else if l.level_db < -60.0 {
                "nothing heard yet".into()
            } else {
                let word = if l.level_db < -40.0 {
                    "quiet"
                } else {
                    "a good level"
                };
                match l.note() {
                    Some((n, o, c)) => {
                        format!("input {:.0} dB, {word} \u{b7} {n}{o} {c:+}\u{a2}", l.level_db)
                    }
                    None => format!("input {:.0} dB, {word}", l.level_db),
                }
            };
            // (over the page drawn after the strip)
            let top = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("amp-room-tip"),
            ));
            crate::player::draw_tip(
                &top,
                &format!("Open the amp room \u{b7} {how}"),
                if key { "A" } else { "" },
                pos2(body.center().x, body.min.y - px(10.0)),
                Align2::CENTER_BOTTOM,
                st.ctx.content_rect(),
            );
        }
        resp.clicked()
    }
}
