//! The room's shared pieces (P37.1): the big knob the quick controls use, a shelf cell (`Entry`), a capture's
//! family, the open unit's wheel places.

use eframe::egui;
use egui::{Align2, Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};
use qrs_audio::board::{Block, Kind, MODELS, Unit};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;

use super::RoomView;
use crate::ampbutton::{rgb, rgba};
use crate::state::AppState;

/// Where the wheel turns something of the room's: (where, block, knob).
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub rect: Rect,
    pub uid: u32,
    pub knob: usize,
}

/// A big knob (the selected card's): its scale as an arc, the value's arc in the accent, a black cap and its
/// pointer; the name above, the value under.
#[allow(clippy::too_many_arguments)]
pub(super) fn big_knob(p: &Painter, c: Pos2, r: f32, u: f32, name: &str, value: &str, hot: bool, held: bool) {
    let th = t();
    if hot {
        p.circle_filled(c, r + px(16.0), th.accent.gamma_multiply(0.07));
    }
    let arc = |a0: f32, a1: f32, col: Color32| {
        let n = 32;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                pos2(c.x + (r + px(9.0)) * a.sin(), c.y - (r + px(9.0)) * a.cos())
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(px(4.0), col)));
    };
    arc(-135.0, 135.0, th.track);
    let a = -135.0 + 270.0 * u.clamp(0.0, 1.0);
    if u > 0.004 {
        arc(-135.0, a, th.accent);
    }
    if qrs_ui::knob_art::on() {
        // the dark themes: the knob pack's pedal black, the room in it (P34.8)
        qrs_ui::knobs::knob_in(
            p,
            c,
            2.0 * r,
            u,
            qrs_ui::knobs::Scale {
                ticks: false,
                ..Default::default()
            },
            Some(qrs_ui::knobs::KnobStyle::Pedal),
            qrs_ui::glass::State::of(hot, held),
        );
    } else {
        // the cap: a shadow, a dark body, a ribbed rim, a lit top
        p.circle_filled(c + vec2(0.0, px(3.0)), r, rgba(0, 0, 0, 0.45));
        p.circle_filled(c, r, rgb([0x16, 0x17, 0x1a]));
        for i in 0..36 {
            let aa = (i as f32 * 10.0).to_radians();
            let o = pos2(c.x + r * aa.sin(), c.y - r * aa.cos());
            let n = pos2(c.x + (r - px(2.6)) * aa.sin(), c.y - (r - px(2.6)) * aa.cos());
            p.line_segment([o, n], Stroke::new(1.0, rgba(255, 255, 255, 0.07)));
        }
        p.circle_filled(c, r * 0.72, rgb([0x24, 0x26, 0x2a]));
        p.circle_filled(c + vec2(-r * 0.15, -r * 0.2), r * 0.42, rgba(255, 255, 255, 0.04));
        let ar = a.to_radians();
        p.line_segment(
            [
                pos2(c.x + r * 0.25 * ar.sin(), c.y - r * 0.25 * ar.cos()),
                pos2(c.x + r * 0.86 * ar.sin(), c.y - r * 0.86 * ar.cos()),
            ],
            Stroke::new(px(3.0), rgb([0xee, 0xee, 0xea])),
        );
    }
    text::caps(
        p,
        name,
        pos2(c.x, c.y - r - px(22.0)),
        10.5,
        th.caption,
        Align2::CENTER_CENTER,
        Some(r * 3.2),
    );
    text::draw(
        p,
        value,
        pos2(c.x, c.y + r + px(25.0)),
        Align2::CENTER_CENTER,
        Style::new(15.0, 800, th.text),
        Some(r * 3.4),
    );
}

/// A cell on the shelf: one of our models, or one of your captures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Entry {
    Model(usize),
    Capture(u32),
}

impl Entry {
    pub(super) fn key(self) -> (usize, u32) {
        match self {
            Entry::Model(m) => (m, 0),
            Entry::Capture(id) => (usize::MAX, id),
        }
    }

    /// The block it adds (a capture: its file model with the capture on it).
    pub(super) fn block(self) -> Block {
        match self {
            Entry::Model(m) => Block::new(m),
            Entry::Capture(id) => {
                let kind = crate::captures::with(|l| l.get(id).map(|i| i.kind))
                    .unwrap_or(crate::captures::Kind::Nam);
                let mut b = Block::of(kind.model()).expect("file model");
                b.set_asset(id);
                b
            }
        }
    }

    pub(super) fn about(self) -> String {
        match self {
            Entry::Model(m) => format!("{} \u{b7} after {}", MODELS[m].shown_name(), MODELS[m].after),
            Entry::Capture(id) => {
                crate::captures::with(|l| l.get(id).map(|i| format!("{} \u{b7} {}", i.name, i.detail())))
                    .unwrap_or_default()
            }
        }
    }
}

/// Our model's family, for a capture: its skin's kind (a pedal without one is a drive).
pub(super) fn capture_family(i: &crate::captures::Item) -> Kind {
    use crate::captures::Kind as C;
    let skin = i
        .skin
        .as_deref()
        .and_then(qrs_audio::board::catalog::find)
        .map(|m| MODELS[m].kind);
    match i.kind {
        C::Nam => Kind::Amp,
        C::Ir => Kind::Cab,
        C::Pedal => match skin {
            Some(Kind::Fuzz) => Kind::Fuzz,
            _ => Kind::Drive,
        },
    }
}

/// The wheel over a knob of the selected card.
pub fn wheel(room: &mut RoomView, st: &mut AppState, pos: Pos2, notches: f32, shift: bool) -> bool {
    let Some(s) = room.spots.iter().rev().find(|s| s.rect.contains(pos)).copied() else {
        return false;
    };
    let b = room.rack.board_now(st);
    let Some(i) = b.index_of(s.uid) else { return false };
    let blk = b.blocks[i];
    let Some(kd) = blk.def().knobs.get(s.knob) else {
        return false;
    };
    let v = match kd.unit {
        Unit::Choice(c) => (blk.v[s.knob] + notches.signum()).rem_euclid(c.len() as f32),
        _ => kd.from_u(kd.to_u(blk.v[s.knob]) + notches * if shift { 0.005 } else { 0.025 }),
    };
    room.rack.set_knob(st, s.uid, s.knob, v);
    room.wheel_at = Some(std::time::Instant::now());
    true
}
