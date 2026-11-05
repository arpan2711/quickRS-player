//! The quiet look (P37.3, D110-D112; new in quickRS): on the dark themes the secondary keys, switches and segmented
//! controls sit pressed into the panel instead of standing on it as glass. A quiet key is a dark well (a shadow
//! falling in from the top lip, a faint light on the bottom one), its label in the text colours; hover lifts the
//! ink, a press sinks the well deeper. A switch is a dark well with a graphite thumb; on, its floor takes a little of
//! the accent and the thumb carries a lit accent light (position and light, not colour alone). The page's main
//! action keeps the glass key (`ButtonStyle::primary` or `main`). Daylight and the classic look never come here
//! (`on()` is `glass::on()`), so they draw exactly as before.

use egui::{Color32, Painter};

use crate::paint::{self, PxRect};
use crate::raster::mix;
use crate::theme::t;

/// The quiet look is the glass look's partner: the dark themes only.
pub fn on() -> bool {
    crate::glass::on()
}

/// How deep a quiet key's well is pressed (0-1 of the rubber recipe): at rest, and while pressed.
pub const DEPTH: f32 = 0.45;
pub const DEPTH_DOWN: f32 = 0.8;
/// The light along a well's bottom lip, against the rubber's (a touch more, so the edge reads on a dark floor).
pub const CATCH: f32 = 2.6;

/// A key's state, for its floor and ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Normal,
    Hover,
    Pressed,
}

impl State {
    pub fn of(hot: bool, down: bool) -> State {
        if hot && down {
            State::Pressed
        } else if hot {
            State::Hover
        } else {
            State::Normal
        }
    }
}

/// A well's floor: the panel's screen colour taken a fifth of the way to black; a breath lighter while hovered,
/// darker while pressed.
pub fn floor(st: State) -> Color32 {
    let th = t();
    let f = mix(th.screen, Color32::BLACK, 0.22);
    match st {
        State::Normal => f,
        State::Hover => mix(f, th.hilite, 0.035),
        State::Pressed => mix(th.screen, Color32::BLACK, 0.5),
    }
}

/// A switch's floor while on: the dark floor with a little of the accent in it.
pub fn floor_on() -> Color32 {
    mix(floor(State::Normal), t().accent, 0.34)
}

/// The chosen option's pad in a quiet segmented control (a raised pad lightens it a little more on its own).
pub fn pad() -> Color32 {
    t().base
}

/// A quiet key's label: the second text colour, the first while hovered or pressed (or when `active`); the accent's
/// ink for an accent label, the miss colour for a danger one.
pub fn ink(st: State, accent: bool, danger: bool, active: bool) -> Color32 {
    let th = t();
    if danger {
        th.miss
    } else if accent {
        th.accent_ink
    } else if active || st != State::Normal {
        th.text
    } else {
        th.text_2
    }
}

/// The graphite thumb's (top, bottom, sheen) colours.
pub fn thumb_colours() -> (Color32, Color32, Color32) {
    let th = t();
    let top = mix(th.base, th.hilite, 0.2);
    let bot = mix(th.screen, Color32::BLACK, 0.2);
    (top, bot, mix(top, th.hilite, 0.35))
}

/// A quiet key's well at `pr` (whole pixels), radius `r` points.
pub(crate) fn key_px(p: &Painter, pr: PxRect, r: Option<f64>, st: State) {
    let d = if st == State::Pressed { DEPTH_DOWN } else { DEPTH };
    paint::quiet_well_px(p, pr, r, d, floor(st), CATCH);
}

/// A switch's track: the well, its floor lit a little when on.
pub(crate) fn track_px(p: &Painter, pr: PxRect, on: bool) {
    let f = if on { floor_on() } else { floor(State::Normal) };
    paint::quiet_well_px(p, pr, None, DEPTH, f, CATCH);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{self, ThemeName};

    fn lum(c: Color32) -> f32 {
        0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32
    }

    #[test]
    fn only_the_dark_themes_are_quiet() {
        let _lock = crate::raster::THEME_LOCK.lock();
        for (name, quiet) in [
            (ThemeName::Midnight, true),
            (ThemeName::Graphite, true),
            (ThemeName::Daylight, false),
        ] {
            theme::set_theme(name);
            assert_eq!(on(), quiet, "{name:?}");
        }
        theme::set_theme(ThemeName::Midnight);
    }

    #[test]
    fn the_floor_is_darker_than_the_panel_and_presses_deeper() {
        let _lock = crate::raster::THEME_LOCK.lock();
        for name in [ThemeName::Midnight, ThemeName::Graphite] {
            theme::set_theme(name);
            let th = t();
            let (n, h, d) = (floor(State::Normal), floor(State::Hover), floor(State::Pressed));
            assert!(lum(n) < lum(th.screen) && lum(th.screen) < lum(th.base));
            assert!(lum(h) > lum(n) && lum(d) < lum(n));
            // on is lit but stays a dark floor (no bright fill)
            assert!(lum(floor_on()) > lum(n) && lum(floor_on()) < lum(th.base) + 60.0);
            // the thumb is graphite: nowhere near the ivory one
            let (top, bot, sheen) = thumb_colours();
            let msg = format!("{name:?} {top:?} {bot:?} {sheen:?} {:?}", th.thumb_top);
            assert!(
                lum(top) < 100.0 && lum(bot) < lum(top) && lum(sheen) < 150.0,
                "{msg}"
            );
            assert!(lum(th.thumb_top) > 200.0, "{msg}");
        }
        theme::set_theme(ThemeName::Midnight);
    }

    #[test]
    fn ink_follows_state() {
        let _lock = crate::raster::THEME_LOCK.lock();
        theme::set_theme(ThemeName::Midnight);
        let th = t();
        assert_eq!(ink(State::Normal, false, false, false), th.text_2);
        assert_eq!(ink(State::Hover, false, false, false), th.text);
        assert_eq!(ink(State::Normal, false, false, true), th.text);
        assert_eq!(ink(State::Normal, true, false, false), th.accent_ink);
        assert_eq!(ink(State::Hover, true, true, false), th.miss);
    }
}
