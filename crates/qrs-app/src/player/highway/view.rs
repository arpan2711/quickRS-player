//! The highway's 3D world and its one pinhole camera (P30, D78; research/spec/01_camera_geometry.md).
//!
//! World units are **string gaps** (g). X runs along the neck toward the higher frets (wire n at
//! 3.58 · (1 − 0.9752ⁿ) / (1 − 0.9752): fitted on four Rocksmith videos), Y up (the strings are stacked at the
//! strike plane Z = 0, the bottom string at Y = 0, the top at Y = 5), Z away from the player. The floor is the plane
//! Y = `FLOOR`. Everything on the highway is a point in this world projected through one camera; nothing rides a
//! lane or drops (the old P19.7 look).
//!
//! The camera (spec 01): a pinhole with a 60° horizontal field of view, no roll, a fixed orientation (looking 11.2°
//! toward the low frets and 4.7° down from the notes' travel), that only pans and dollies: the camera's fret on
//! the high e lands at (0.5 W, 0.91 H) and its distance D = 2.8 × the frets across the follow camera wants.
//! **The highway's depth scales with D**: a note Δt seconds out is at Z = (D / c) · Δt with c = 0.375 × the
//! song's DynamicVisualDensity (divided by the player's speed), and the far end is at 4.87 · D, whatever the zoom.

use eframe::egui::{Pos2, Rect, pos2};

/// Fret 1's width (gaps) and the ratio from one fret's width to the next (a real guitar's is 0.944).
pub const W0: f32 = 3.58;
pub const Q: f32 = 0.9752;
/// The floor under the strings (gaps below the bottom string): the chord box stands on it 0.7 g below.
pub const FLOOR: f32 = -0.7;
/// The far end and its fade, in camera distances (D): clip at 4.87 D (the game's 400 units), opaque from 4.07 D.
const FAR_D: f32 = 4.87;
const FADE_D: f32 = 0.8;
/// c (seconds for a note to travel from the strike plane to the camera plane) per unit of DynamicVisualDensity.
pub const C_PER_DVD: f64 = 0.375;

/// Horizontal field of view 60° (Gamebryo's Camera.FOV): f = 0.866 W.
const F_W: f32 = 0.866;
/// The camera's yaw (toward the low frets) and pitch (down), degrees: the notes' vanishing point 0.305 H right of
/// and 0.128 H above the centre at 16:9 (seven videos; Cowboys and Infected differ, see spec 01).
const YAW: f32 = 11.2;
const PITCH: f32 = 4.7;
/// Where the high e (the bottom string) at the camera's fret sits: the screen's centre column, 0.91 H down in
/// spec 01's fits (Panic / Otherside side-by-sides want ≈0.925, Infected ≈0.88: the middle keeps the numbers in view).
const ANCHOR_Y: f32 = 0.91;
/// The camera's fret sits this many frets right of the screen middle: RsCamera's pan target was fitted in the old
/// 2D layout; the side-by-sides put the window 1.2-1.4 frets further right (Panic, Otherside; Cowboys 0.6).
const CENTRE: f32 = -1.3;
/// Camera distance per fret across (spec 01: 2.8; the side-by-sides agree within ~10%: Otherside wants 7% more,
/// Infected 10% less, Cowboys 16% less, which spec 01 also finds different), and its range (26-70 g measured; the
/// floor 35 keeps Panic's low chords at Rocksmith's zoom).
const D_PER_FRET: f32 = 2.8;
const DIST: (f32, f32) = (35.0, 75.0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    fn norm(self) -> V3 {
        let l = self.dot(self).sqrt().max(1e-9);
        v3(self.x / l, self.y / l, self.z / l)
    }
}

impl std::ops::Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl std::ops::Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl std::ops::Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, k: f32) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
}

/// Fret wire `n`'s X (fractional frets fine, the nut is 0).
pub fn wire(n: f32) -> f32 {
    W0 * (1.0 - Q.powf(n)) / (1.0 - Q)
}

/// The middle of fret `f`'s space (between wires f−1 and f).
pub fn slot(f: i32) -> f32 {
    (wire(f as f32 - 1.0) + wire(f as f32)) / 2.0
}

/// Fret `f`'s width (gaps).
pub fn fret_w(f: f32) -> f32 {
    wire(f) - wire(f - 1.0)
}

#[cfg(test)]
/// The fret wire at X (inverse of [`wire`]).
pub fn fret_at(x: f32) -> f32 {
    (1.0 - x * (1.0 - Q) / W0).max(1e-4).ln() / Q.ln()
}

/// The camera for one frame, and the highway's clock and speed.
pub struct View {
    pub well: Rect,
    f: f32,
    c: Pos2,
    /// The camera's right, down and forward axes in world coordinates.
    ax: [V3; 3],
    eye: V3,
    pub now: f64,
    /// The camera's distance to the string plane (g).
    pub d: f32,
    /// Gaps a second (= D / c).
    pub speed: f32,
    pub n_str: usize,
    pub invert: bool,
}

/// The fixed camera rotation: (right, down, forward) in world coordinates.
fn axes() -> [V3; 3] {
    // the world's axes in camera coordinates (x right, y down, z forward): depth looks right of and above the
    // centre by the yaw and pitch, up with no roll, X completing the frame (pointing right on screen)
    let (yaw, pitch) = (YAW.to_radians(), PITCH.to_radians());
    let zw = v3(yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()).norm();
    let yw = v3(0.0, -zw.z, zw.y).norm();
    let mut xw = yw.cross(zw);
    if xw.x < 0.0 {
        xw = xw * -1.0;
    }
    // transpose: rows of [xw yw zw] are the camera axes in world coordinates
    [v3(xw.x, yw.x, zw.x), v3(xw.y, yw.y, zw.y), v3(xw.z, yw.z, zw.z)]
}

impl View {
    /// `cam` = (fret wire at the screen's middle, frets across); `c` = seconds a note takes from the strike plane to
    /// the camera plane (0.375 × DynamicVisualDensity, divided by the speed setting).
    pub fn new(well: Rect, cam: (f32, f32), now: f64, c: f64, n_str: usize, invert: bool) -> View {
        let h = well.height().max(1.0);
        let f = F_W * well.width().max(1.0);
        let ctr = well.center();
        let ax = axes();
        let d = (D_PER_FRET * cam.1).clamp(DIST.0, DIST.1);
        // the ray through the anchor pixel, in world coordinates; the camera sits on it, D from the string plane
        let dc = v3(0.0, (ANCHOR_Y - 0.5) * h / f, 1.0).norm();
        let dw = ax[0] * dc.x + ax[1] * dc.y + ax[2] * dc.z;
        let q = v3(wire(cam.0 + CENTRE), 0.0, 0.0);
        // distance along the ray so the eye is D in front of the string plane (Z)
        let eye = q - dw * (d / dw.z.max(1e-3));
        View {
            well,
            f,
            c: ctr,
            ax,
            eye,
            now,
            d,
            speed: d / c.max(0.05) as f32,
            n_str,
            invert,
        }
    }

    /// A world point in camera coordinates (x right, y down, z forward).
    pub fn cam(&self, p: V3) -> V3 {
        let d = p - self.eye;
        v3(self.ax[0].dot(d), self.ax[1].dot(d), self.ax[2].dot(d))
    }

    /// A world point on screen (None behind the near plane).
    pub fn pt(&self, p: V3) -> Option<Pos2> {
        let c = self.cam(p);
        (c.z > 1.0).then(|| pos2(self.c.x + self.f * c.x / c.z, self.c.y + self.f * c.y / c.z))
    }

    /// Pixels per world unit at p.
    pub fn scale(&self, p: V3) -> f32 {
        self.f / self.cam(p).z.max(1.0)
    }

    /// The camera's position (for which side of a box faces it).
    pub fn eye(&self) -> V3 {
        self.eye
    }

    /// String s's height (0 = low E on top unless inverted; a 4-string bass spreads over the same height).
    pub fn sy(&self, s: i32) -> f32 {
        let n = self.n_str.max(2) as i32;
        let s = s.clamp(0, n - 1);
        let r = if self.invert { s } else { n - 1 - s };
        r as f32 * 5.0 / (n - 1) as f32
    }

    /// The bottom and top strings' heights.
    pub fn sy_bottom(&self) -> f32 {
        0.0
    }

    pub fn top(&self) -> f32 {
        5.0
    }

    /// Depth of chart time t.
    pub fn z(&self, t: f64) -> f32 {
        ((t - self.now) * self.speed as f64) as f32
    }

    /// The far end of the highway (depth): 4.87 D.
    pub fn far(&self) -> f32 {
        FAR_D * self.d
    }

    /// Seconds of highway ahead (the far end's time).
    pub fn look(&self) -> f64 {
        self.secs(self.far())
    }

    /// The far fade at depth z: 1 near, 0 at the far end (smoothstep over 0.8 D).
    pub fn fade(&self, z: f32) -> f32 {
        let u = ((FAR_D - z / self.d) / FADE_D).clamp(0.0, 1.0);
        u * u * (3.0 - 2.0 * u)
    }

    /// The game's distances (its units: the camera sits ≈82 behind the strike plane, so 1 g = 82 / D units).
    pub fn units(&self, u: f32) -> f32 {
        u * self.d / 82.0
    }

    /// Seconds of travel for a depth (for things timed in seconds).
    pub fn secs(&self, z: f32) -> f64 {
        (z / self.speed.max(1e-3)) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        View::new(
            Rect::from_min_size(pos2(0.0, 0.0), eframe::egui::vec2(1920.0, 1080.0)),
            (3.0, 14.0),
            0.0,
            0.49,
            6,
            false,
        )
    }

    #[test]
    fn frets_and_inverse() {
        assert!((wire(1.0) - W0).abs() < 1e-4);
        assert!(fret_w(12.0) < fret_w(1.0));
        assert!((fret_at(wire(7.3)) - 7.3).abs() < 1e-3);
    }

    #[test]
    fn camera_anchor_and_vanishing_point() {
        let v = view();
        // the high e at the camera's fret sits on the centre column at ANCHOR_Y
        let a = v.pt(v3(wire(3.0 + CENTRE), 0.0, 0.0)).unwrap();
        assert!(
            (a.x - 960.0).abs() < 0.5 && (a.y - ANCHOR_Y * 1080.0).abs() < 0.5,
            "{a:?}"
        );
        // the notes' vanishing point: 0.305 H right of and 0.128 H above the centre (spec 01)
        let far = v.pt(v3(wire(3.0), 0.0, 1e7)).unwrap();
        assert!((far.x - (960.0 + 0.305 * 1080.0)).abs() < 3.0, "{far:?}");
        assert!((far.y - (540.0 - 0.128 * 1080.0)).abs() < 3.0, "{far:?}");
        // strings fan out to the right (the neck comes nearer), the low E on top
        let gap = |x: f32| v.pt(v3(x, 0.0, 0.0)).unwrap().y - v.pt(v3(x, 1.0, 0.0)).unwrap().y;
        assert!(gap(wire(12.0)) > gap(wire(0.0)));
        assert!(v.sy(0) > v.sy(5));
    }

    #[test]
    fn depth_scales_with_the_camera_and_the_horizon_is_a_distance() {
        let v = view();
        // D = D_PER_FRET × frets across; the far end at 4.87 D is 4.87 c seconds out
        assert!((v.d - D_PER_FRET * 14.0).abs() < 1e-3);
        assert!((v.look() - 4.87 * 0.49).abs() < 1e-3);
        assert!(v.fade(v.far()) < 1e-3 && (v.fade(4.0 * v.d) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_note_moves_on_a_straight_screen_line() {
        // a point at a fixed (x, y) moving in z stays on the line to the vanishing point: no drop
        let v = view();
        let p = |z: f32| v.pt(v3(slot(5), v.sy(2), z)).unwrap();
        let (a, b, c) = (p(0.0), p(30.0), p(200.0));
        let cross = (b - a).x * (c - a).y - (b - a).y * (c - a).x;
        assert!(cross.abs() < 1.0, "{cross}");
    }
}
