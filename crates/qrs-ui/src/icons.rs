//! Drawn icons on a 24-unit grid (like the boards' SVGs). Port of rsplay/neu.py `_icon_surface` and `icon`: each
//! is drawn with pygame's own primitives at 4x (crate::raster::pg, the same pixels) and smoothscaled down, then
//! cached per size and colour.

use egui::{Color32, Painter, Pos2, Rect, Vec2};

use crate::paint::{Store, blit, ck, ipx, key_of, pxr, sc, texture};
use crate::raster::pg::{self, Surf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Play,
    Pause,
    Stop,
    Back,
    Fwd,
    Down,
    Up,
    Right,
    Left,
    Search,
    Music,
    X,
    XBold,
    Check,
    Plus,
    Minus,
    Grip,
    Star,
    StarOff,
    Lock,
    Note,
    Home,
    Sliders,
    Grid,
    Restart,
    Expand,
    ArrowLeft,
    Synth,
    Track,
    Gear,
}

impl Icon {
    pub fn from_name(name: &str) -> Option<Icon> {
        Some(match name {
            "play" => Icon::Play,
            "pause" => Icon::Pause,
            "stop" => Icon::Stop,
            "back" => Icon::Back,
            "fwd" => Icon::Fwd,
            "down" => Icon::Down,
            "up" => Icon::Up,
            "right" => Icon::Right,
            "left" => Icon::Left,
            "search" => Icon::Search,
            "music" => Icon::Music,
            "x" => Icon::X,
            "x_bold" => Icon::XBold,
            "check" => Icon::Check,
            "plus" => Icon::Plus,
            "minus" => Icon::Minus,
            "grip" => Icon::Grip,
            "star" => Icon::Star,
            "star_off" => Icon::StarOff,
            "lock" => Icon::Lock,
            "note" => Icon::Note,
            "home" => Icon::Home,
            "sliders" => Icon::Sliders,
            "grid" => Icon::Grid,
            "restart" => Icon::Restart,
            "expand" => Icon::Expand,
            "arrow_left" => Icon::ArrowLeft,
            "synth" => Icon::Synth,
            "track" => Icon::Track,
            "gear" => Icon::Gear,
            _ => return None,
        })
    }
}

/// neu.SS: icons are drawn at 4x and smoothed down.
const SS: i32 = 4;

/// The pen of `_icon_surface`: one SVG unit is `u` pixels of the 4x surface.
struct Pen {
    s: Surf,
    u: f64,
    col: pg::Rgba,
}

impl Pen {
    #[allow(non_snake_case)]
    fn P(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.u, y * self.u)
    }

    /// `stroke(points, width)`: lines `max(1, int(width * u))` wide with a disc on every point.
    fn stroke(&mut self, points: &[(f64, f64)], width: f64) {
        let w = ((width * self.u) as i32).max(1);
        let pts: Vec<(f64, f64)> = points.iter().map(|&(x, y)| self.P(x, y)).collect();
        for ab in pts.windows(2) {
            pg::line_f(&mut self.s, self.col, ab[0], ab[1], w);
        }
        for &p in &pts {
            pg::circle(&mut self.s, self.col, p, w as f64 / 2.0, 0);
        }
    }

    fn polygon(&mut self, points: &[(f64, f64)], width: i32) {
        let pts: Vec<(f64, f64)> = points.iter().map(|&(x, y)| self.P(x, y)).collect();
        pg::polygon(&mut self.s, self.col, &pts, width);
    }

    /// `pygame.draw.circle(surf, col, P(x, y), r * u, width)`.
    fn circle(&mut self, x: f64, y: f64, r: f64, width: i32) {
        let c = self.P(x, y);
        pg::circle(&mut self.s, self.col, c, r * self.u, width);
    }

    /// `pygame.draw.rect(surf, col, (*P(x, y), w * u, h * u), width, border_radius=radius)`.
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, width: i32, radius: i32) {
        let (px, py) = self.P(x, y);
        pg::rect(
            &mut self.s,
            self.col,
            (px, py, w * self.u, h * self.u),
            width,
            radius,
        );
    }

    fn arc(&mut self, x: f64, y: f64, w: f64, h: f64, a0: f64, a1: f64, width: i32) {
        let (px, py) = self.P(x, y);
        pg::arc(
            &mut self.s,
            self.col,
            (px, py, w * self.u, h * self.u),
            a0,
            a1,
            width,
        );
    }

    fn ellipse(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let (px, py) = self.P(x, y);
        pg::ellipse(&mut self.s, self.col, (px, py, w * self.u, h * self.u));
    }
}

/// The icon as an image `size` pixels square (`_icon_surface`).
pub fn icon_image(icon: Icon, size: u32, color: Color32) -> crate::raster::Image {
    let size = size.max(1) as i32;
    let n = size * SS;
    let u = n as f64 / 24.0;
    let mut p = Pen {
        s: Surf::new(n, n),
        u,
        col: pg::rgb(color),
    };
    let iu = |k: f64| (k * u) as i32;
    use std::f64::consts::PI;
    match icon {
        Icon::Play => p.polygon(&[(7.0, 4.5), (7.0, 19.5), (20.0, 12.0)], 0),
        Icon::Pause => {
            p.stroke(&[(8.0, 5.0), (8.0, 19.0)], 2.6);
            p.stroke(&[(16.0, 5.0), (16.0, 19.0)], 2.6);
        }
        Icon::Stop => p.rect(6.0, 6.0, 12.0, 12.0, 0, iu(2.0)),
        Icon::Back => {
            p.stroke(&[(11.0, 19.0), (4.0, 12.0), (11.0, 5.0)], 2.0);
            p.stroke(&[(19.0, 19.0), (12.0, 12.0), (19.0, 5.0)], 2.0);
        }
        Icon::Fwd => {
            p.stroke(&[(13.0, 5.0), (20.0, 12.0), (13.0, 19.0)], 2.0);
            p.stroke(&[(5.0, 5.0), (12.0, 12.0), (5.0, 19.0)], 2.0);
        }
        Icon::Down => p.stroke(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)], 2.0),
        Icon::Up => p.stroke(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)], 2.0),
        Icon::Right => p.stroke(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)], 2.0),
        Icon::Left => p.stroke(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)], 2.0),
        Icon::Search => {
            p.circle(11.0, 11.0, 7.0, iu(2.0));
            p.stroke(&[(16.7, 16.7), (21.0, 21.0)], 2.0);
        }
        Icon::Music => {
            p.stroke(&[(9.0, 18.0), (9.0, 5.0), (21.0, 3.0), (21.0, 16.0)], 2.0);
            p.circle(6.0, 18.0, 3.0, iu(2.0));
            p.circle(18.0, 16.0, 3.0, iu(2.0));
        }
        Icon::X | Icon::XBold => {
            let w = if icon == Icon::X { 2.4 } else { 4.2 };
            p.stroke(&[(6.0, 6.0), (18.0, 18.0)], w);
            p.stroke(&[(18.0, 6.0), (6.0, 18.0)], w);
        }
        Icon::Check => p.stroke(&[(5.0, 12.5), (10.0, 17.5), (19.0, 7.0)], 2.6),
        Icon::Plus => {
            p.stroke(&[(12.0, 5.0), (12.0, 19.0)], 2.4);
            p.stroke(&[(5.0, 12.0), (19.0, 12.0)], 2.4);
        }
        Icon::Minus => p.stroke(&[(5.0, 12.0), (19.0, 12.0)], 2.4),
        Icon::Grip => {
            for x in [9.0, 15.0] {
                for y in [6.0, 12.0, 18.0] {
                    p.circle(x, y, 1.7, 0);
                }
            }
        }
        Icon::Star | Icon::StarOff => {
            let pts: Vec<(f64, f64)> = (0..10)
                .map(|i| {
                    let ang = -PI / 2.0 + i as f64 * PI / 5.0;
                    let rad = if i % 2 == 0 { 10.0 } else { 4.4 };
                    (12.0 + rad * ang.cos(), 12.6 + rad * ang.sin())
                })
                .collect();
            let width = if icon == Icon::Star { 0 } else { iu(1.8).max(1) };
            p.polygon(&pts, width);
        }
        Icon::Lock => {
            p.arc(8.0, 3.5, 8.0, 11.0, 0.0, PI, iu(2.2));
            p.stroke(&[(8.0, 9.0), (8.0, 11.0)], 2.2);
            p.stroke(&[(16.0, 9.0), (16.0, 11.0)], 2.2);
            p.rect(5.5, 10.5, 13.0, 10.0, 0, iu(2.0));
        }
        Icon::Note => {
            p.ellipse(4.0, 15.0, 7.0, 5.0);
            p.ellipse(13.0, 13.0, 7.0, 5.0);
            p.stroke(&[(10.4, 17.0), (10.4, 5.0)], 2.0);
            p.stroke(&[(19.4, 15.0), (19.4, 3.0)], 2.0);
            p.polygon(&[(9.4, 4.4), (20.4, 2.4), (20.4, 6.0), (9.4, 8.0)], 0);
        }
        Icon::Home => {
            p.stroke(&[(4.0, 11.0), (12.0, 4.0), (20.0, 11.0)], 2.2);
            p.stroke(&[(6.0, 10.0), (6.0, 20.0), (18.0, 20.0), (18.0, 10.0)], 2.2);
        }
        Icon::Sliders => {
            p.stroke(&[(4.0, 7.0), (14.0, 7.0)], 2.2);
            p.stroke(&[(18.0, 7.0), (20.0, 7.0)], 2.2);
            p.circle(16.0, 7.0, 2.4, iu(2.2));
            p.stroke(&[(4.0, 17.0), (8.0, 17.0)], 2.2);
            p.stroke(&[(12.0, 17.0), (20.0, 17.0)], 2.2);
            p.circle(10.0, 17.0, 2.4, iu(2.2));
        }
        Icon::Grid => {
            for (x, y) in [(4.0, 4.0), (13.0, 4.0), (4.0, 13.0), (13.0, 13.0)] {
                p.rect(x, y, 7.0, 7.0, iu(2.2), iu(2.0));
            }
        }
        Icon::Restart => {
            p.arc(
                4.0,
                4.0,
                16.0,
                16.0,
                (-60.0f64).to_radians(),
                200.0f64.to_radians(),
                iu(2.4),
            );
            p.polygon(&[(3.0, 5.0), (9.5, 5.5), (4.5, 11.0)], 0);
        }
        Icon::Expand => {
            p.stroke(&[(14.0, 4.0), (20.0, 4.0), (20.0, 10.0)], 2.4);
            p.stroke(&[(20.0, 4.0), (13.5, 10.5)], 2.4);
            p.stroke(&[(10.0, 20.0), (4.0, 20.0), (4.0, 14.0)], 2.4);
            p.stroke(&[(4.0, 20.0), (10.5, 13.5)], 2.4);
        }
        Icon::ArrowLeft => {
            p.stroke(&[(19.0, 12.0), (5.5, 12.0)], 2.4);
            p.stroke(&[(11.5, 5.5), (5.5, 12.0), (11.5, 18.5)], 2.4);
        }
        Icon::Synth => {
            let pts: Vec<(f64, f64)> = (0..49)
                .map(|i| {
                    (
                        3.0 + 18.0 * i as f64 / 48.0,
                        12.0 - 6.5 * (3.0 * PI * i as f64 / 48.0).sin(),
                    )
                })
                .collect();
            p.stroke(&pts, 2.3);
        }
        Icon::Track => {
            for (x, hh) in [(4.0, 5.0), (8.0, 11.0), (12.0, 17.0), (16.0, 9.0), (20.0, 13.0)] {
                p.stroke(&[(x, 12.0 - hh / 2.0), (x, 12.0 + hh / 2.0)], 2.4);
            }
        }
        Icon::Gear => {
            p.circle(12.0, 12.0, 3.0, iu(2.2));
            for i in 0..8 {
                let a = i as f64 * PI / 4.0;
                p.stroke(
                    &[
                        (12.0 + 6.2 * a.cos(), 12.0 + 6.2 * a.sin()),
                        (12.0 + 8.6 * a.cos(), 12.0 + 8.6 * a.sin()),
                    ],
                    2.6,
                );
            }
            p.circle(12.0, 12.0, 6.6, iu(2.2));
        }
    }
    p.s.smoothscale(size, size).image()
}

/// The icon's size in pixels: `max(6, px(size))`.
fn icon_px(size: f32, s: f64) -> i32 {
    pxr(size, s).max(6)
}

/// Draw a cached icon with its centre at (cx, cy) whole pixels, as `surf.get_rect(center=...)` places it.
fn put(p: &Painter, icon: Icon, sz: i32, color: Color32, cx: i32, cy: i32, tint: Color32) {
    let key = key_of(("icon", icon, sz, ck(color)));
    let (tex, isize) = texture(p.ctx(), Store::Small, key, || icon_image(icon, sz as u32, color));
    blit(p, tex, isize, (cx - sz / 2) as f32, (cy - sz / 2) as f32, tint);
}

/// A drawn icon `size` points square centred on `center` (`neu.icon`: the centre taken as `int()`). Returns its
/// rect.
pub fn icon(p: &Painter, icon: Icon, center: Pos2, size: f32, color: Color32) -> Rect {
    let ppp = p.pixels_per_point();
    let s = sc(ppp);
    let sz = icon_px(size, s);
    let (cx, cy) = (ipx(center.x, ppp), ipx(center.y, ppp));
    put(p, icon, sz, color, cx, cy, Color32::WHITE);
    Rect::from_min_size(
        Pos2::new((cx - sz / 2) as f32 / ppp, (cy - sz / 2) as f32 / ppp),
        Vec2::splat(sz as f32 / ppp),
    )
}

/// An icon on a key; on a dark material with a soft shadow under it (`neu._ink_icon`: the black icon at
/// `shadow` alpha, `max(1, px(1))` lower).
pub fn ink_icon(p: &Painter, ic: Icon, center: Pos2, size: f32, color: Color32, shadow: f32) -> Rect {
    if shadow > 0.0 {
        let ppp = p.pixels_per_point();
        let s = sc(ppp);
        let sz = icon_px(size, s);
        let (cx, cy) = (ipx(center.x, ppp), ipx(center.y, ppp));
        let a = (255.0 * shadow.clamp(0.0, 1.0)) as u8;
        put(
            p,
            ic,
            sz,
            Color32::BLACK,
            cx,
            cy + pxr(1.0, s).max(1),
            Color32::from_white_alpha(a),
        );
    }
    icon(p, ic, center, size, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [(Icon, &str); 30] = [
        (Icon::Play, "play"),
        (Icon::Pause, "pause"),
        (Icon::Stop, "stop"),
        (Icon::Back, "back"),
        (Icon::Fwd, "fwd"),
        (Icon::Down, "down"),
        (Icon::Up, "up"),
        (Icon::Right, "right"),
        (Icon::Left, "left"),
        (Icon::Search, "search"),
        (Icon::Music, "music"),
        (Icon::X, "x"),
        (Icon::XBold, "x_bold"),
        (Icon::Check, "check"),
        (Icon::Plus, "plus"),
        (Icon::Minus, "minus"),
        (Icon::Grip, "grip"),
        (Icon::Star, "star"),
        (Icon::StarOff, "star_off"),
        (Icon::Lock, "lock"),
        (Icon::Note, "note"),
        (Icon::Home, "home"),
        (Icon::Sliders, "sliders"),
        (Icon::Grid, "grid"),
        (Icon::Restart, "restart"),
        (Icon::Expand, "expand"),
        (Icon::ArrowLeft, "arrow_left"),
        (Icon::Synth, "synth"),
        (Icon::Track, "track"),
        (Icon::Gear, "gear"),
    ];

    #[test]
    fn names_round_trip() {
        for (ic, name) in ALL {
            assert_eq!(Icon::from_name(name), Some(ic));
        }
    }

    /// Every icon against rsplay.neu's `_icon_surface` (QRS_NEU_DUMPS, see keys.rs).
    #[test]
    fn neu_parity_icons() {
        let Ok(dir) = std::env::var("QRS_NEU_DUMPS") else {
            return;
        };
        let mut bad = Vec::new();
        let mut n = 0;
        for (ic, name) in ALL {
            for sz in [12u32, 14, 16, 21, 23, 27, 45] {
                n += 1;
                let got = icon_image(ic, sz, Color32::from_rgb(178, 192, 207));
                if let Some(e) = crate::raster::tests_cmp(&dir, &format!("icon_{name}_{sz}"), &got) {
                    bad.push(e);
                }
            }
        }
        eprintln!("neu icons parity: {n} cases");
        assert!(bad.is_empty(), "{} of {n} differ:\n{}", bad.len(), bad.join("\n"));
    }
}
