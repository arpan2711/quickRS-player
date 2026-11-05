//! Stompboxes, consoles and the rotary cabinet (PLAN.md P21.3), each drawn after the real unit it models, as in the
//! approved mockups: the enclosure templates (a compact with a hinged plate, a small box, boutique and big flat
//! boxes, the kidney plate, the round fuzz, the wah's treadle), consoles (tape echo, vibe, spring reverb) and the
//! rotary cabinet. Each model's spec (`face::Spec`) says where its controls sit on the real unit; `face` turns it
//! into the layout, the cached body and the live controls.

use eframe::egui::{Painter, Rect, Vec2};
use qrs_audio::board::Block;
use qrs_ui::raster::Image;

use super::{Detail, Hot, Layout};

mod boxes;
pub(crate) mod compact;
mod consoles;
pub(crate) mod face;
pub(crate) mod kit;
mod shapes;

/// A pedal's spec, when it's been drawn.
fn spec(id: &str) -> Option<face::Spec> {
    compact::of(id)
        .or_else(|| boxes::of(id))
        .or_else(|| shapes::of(id))
        .or_else(|| consoles::of(id))
}

/// Whether a model has its own drawn pedal face.
pub(super) fn has_spec(id: &str) -> bool {
    spec(id).is_some()
}

/// Whether a pedal has its own drawn face (else the legacy one).
pub(super) fn designed(b: &Block) -> bool {
    spec(b.def().id).is_some()
}

pub(super) fn layout(b: &Block, r: Rect, detail: Detail) -> Layout {
    match spec(b.def().id) {
        Some(s) => face::layout(&s, r),
        None => super::legacy_layout(b, r, detail),
    }
}

/// The body (cached per size): everything that doesn't move.
pub(super) fn body(b: &Block, r: Rect, l: &Layout, ppp: f32) -> Image {
    match spec(b.def().id) {
        Some(s) => face::body(&s, r, ppp),
        None => super::legacy_body(b, r, l, ppp),
    }
}

/// A miniature's body: a designed face without its knobs (they're dots there, P37.12).
pub(super) fn mini_body(b: &Block, r: Rect, l: &Layout, ppp: f32) -> Image {
    match spec(b.def().id) {
        Some(s) => face::body_bare(&s, r, ppp),
        None => super::legacy_body(b, r, l, ppp),
    }
}

/// What's drawn live over the body: knobs, switches, the LED, labels.
pub(super) fn controls(p: &Painter, r: Rect, b: &Block, l: &Layout, detail: Detail, hot: Hot) {
    match spec(b.def().id) {
        Some(s) => face::controls(&s, p, r, b, detail, hot),
        None => super::legacy_controls(p, r, b, l, detail, hot),
    }
}

pub(super) fn board_size(b: &Block) -> Vec2 {
    match spec(b.def().id) {
        Some(s) => face::board_size(&s),
        None => super::legacy_board_size(b),
    }
}
