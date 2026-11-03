//! Drawing in the highway's 3D world: every primitive takes world points (gaps) and projects them through the
//! [`View`]. Two blends, as the game: alpha for solid things, additive (premultiplied colour with alpha 0) for
//! light (lit lines, halos, flashes).

use eframe::egui::{Align2, Color32, Mesh, Painter, Pos2, Shape, Stroke, pos2, vec2};
use qrs_ui::text::{self, Style};

use super::view::{V3, View};

pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// `c` at opacity a (alpha-blended).
pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `c` as added light at intensity k.
pub fn add(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 4.0);
    let f = |v: u8| (v as f32 * k).round().min(255.0) as u8;
    Color32::from_rgba_premultiplied(f(c.r()), f(c.g()), f(c.b()), 0)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// `c` times k (k > 1 brightens, clamped).
pub fn scale(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
    rgb(f(c.r()), f(c.g()), f(c.b()))
}

pub fn smooth(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

pub struct Pen<'a> {
    pub p: &'a Painter,
    pub v: &'a View,
}

impl Pen<'_> {
    fn pts(&self, q: &[V3]) -> Option<Vec<Pos2>> {
        q.iter().map(|x| self.v.pt(*x)).collect()
    }

    /// A flat convex polygon.
    pub fn poly(&self, q: &[V3], col: Color32) {
        if col == Color32::TRANSPARENT {
            return;
        }
        if let Some(s) = self.pts(q) {
            self.p.add(Shape::convex_polygon(s, col, Stroke::NONE));
        }
    }

    /// A quad with a colour per corner.
    pub fn grad(&self, q: [V3; 4], cols: [Color32; 4]) {
        let Some(s) = self.pts(&q) else { return };
        let mut m = Mesh::default();
        for (pt, c) in s.into_iter().zip(cols) {
            m.colored_vertex(pt, c);
        }
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        self.p.add(Shape::mesh(m));
    }

    /// A grid: `rows` of points (each row the same length), a colour per point; consecutive rows joined.
    pub fn grid(&self, rows: &[Vec<V3>], cols: &[Vec<Color32>]) {
        let mut m = Mesh::default();
        let n = rows.first().map_or(0, |r| r.len());
        for (r, c) in rows.iter().zip(cols) {
            for (q, k) in r.iter().zip(c) {
                // (behind the camera: clamp onto the near plane's side; callers keep grids in front)
                let s = self.v.pt(*q).unwrap_or(pos2(-1e4, -1e4));
                m.colored_vertex(s, *k);
            }
        }
        for i in 0..rows.len().saturating_sub(1) as u32 {
            for j in 0..n.saturating_sub(1) as u32 {
                let a = i * n as u32 + j;
                let b = a + n as u32;
                m.add_triangle(a, a + 1, b + 1);
                m.add_triangle(a, b + 1, b);
            }
        }
        self.p.add(Shape::mesh(m));
    }

    /// A screen-facing line from a to b, `w` world units wide (never thinner than `min_px`; thinner lines fade
    /// instead), colours ca at a and cb at b, with a feather `fw` world units each side.
    #[allow(clippy::too_many_arguments)]
    pub fn line(&self, a: V3, b: V3, w: f32, fw: f32, ca: Color32, cb: Color32, min_px: f32) {
        let (Some(sa), Some(sb)) = (self.v.pt(a), self.v.pt(b)) else {
            return;
        };
        let d = sb - sa;
        let len = d.length();
        if len < 0.01 {
            return;
        }
        let n = vec2(-d.y, d.x) / len;
        let mut m = Mesh::default();
        for (s, q, c) in [(sa, a, ca), (sb, b, cb)] {
            let k = self.v.scale(q);
            let wp = w * k;
            let (hw, c) = if wp < min_px {
                (min_px / 2.0, fade_col(c, wp / min_px))
            } else {
                (wp / 2.0, c)
            };
            let f = (fw * k).max(0.5);
            for (off, cc) in [
                (-hw - f, Color32::TRANSPARENT),
                (-hw, c),
                (hw, c),
                (hw + f, Color32::TRANSPARENT),
            ] {
                m.colored_vertex(s + n * off, cc);
            }
        }
        for k in 0..3u32 {
            m.add_triangle(k, k + 1, k + 5);
            m.add_triangle(k, k + 5, k + 4);
        }
        self.p.add(Shape::mesh(m));
    }

    /// A screen-facing band along a polyline with a symmetric cross-section: `stops` (offset in world units from
    /// the centre line, colour), from the centre outward.
    pub fn band(&self, pts: &[V3], stops: &[(f32, Color32)]) {
        let s: Vec<Option<Pos2>> = pts.iter().map(|q| self.v.pt(*q)).collect();
        let k = stops.len();
        if k < 2 {
            return;
        }
        let mut m = Mesh::default();
        // vertices per point: stops mirrored, from -last to +last
        let per = (2 * k - 1) as u32;
        let mut prev: Option<u32> = None;
        for i in 0..pts.len() {
            let Some(p0) = s[i] else {
                prev = None;
                continue;
            };
            let a = s[i.saturating_sub(1)].unwrap_or(p0);
            let b = s[(i + 1).min(pts.len() - 1)].unwrap_or(p0);
            let d = b - a;
            if d.length() < 1e-3 {
                prev = None;
                continue;
            }
            let n = vec2(-d.y, d.x) / d.length();
            let sc = self.v.scale(pts[i]);
            let base = m.vertices.len() as u32;
            for j in (1..k).rev() {
                m.colored_vertex(p0 - n * stops[j].0 * sc, stops[j].1);
            }
            for st in stops {
                m.colored_vertex(p0 + n * st.0 * sc, st.1);
            }
            if let Some(pb) = prev {
                for j in 0..per - 1 {
                    m.add_triangle(pb + j, pb + j + 1, base + j + 1);
                    m.add_triangle(pb + j, base + j + 1, base + j);
                }
            }
            prev = Some(base);
        }
        self.p.add(Shape::mesh(m));
    }

    /// Text standing at a world point, `h` world units tall (the em), screen-aligned.
    pub fn text(&self, s: &str, at: V3, h: f32, anchor: Align2, weight: u16, col: Color32) -> Option<f32> {
        let pt = self.v.pt(at)?;
        let size = h * self.v.scale(at);
        if size < 4.0 || col.a() == 0 {
            return None;
        }
        text::draw(self.p, s, pt, anchor, Style::new(size, weight, col), None);
        Some(size)
    }

    /// A soft additive halo `w` px wide around a convex screen polygon.
    pub fn halo(&self, s: &[Pos2], w: f32, col: Color32) {
        if w < 0.5 || s.len() < 3 {
            return;
        }
        let n = s.len() as f32;
        let c = pos2(
            s.iter().map(|p| p.x).sum::<f32>() / n,
            s.iter().map(|p| p.y).sum::<f32>() / n,
        );
        let out: Vec<Pos2> = s.iter().map(|q| *q + (*q - c).normalized() * w).collect();
        let mut m = Mesh::default();
        for k in 0..s.len() {
            let j = (k + 1) % s.len();
            let i0 = m.vertices.len() as u32;
            m.colored_vertex(s[k], col);
            m.colored_vertex(s[j], col);
            m.colored_vertex(out[j], Color32::TRANSPARENT);
            m.colored_vertex(out[k], Color32::TRANSPARENT);
            m.add_triangle(i0, i0 + 1, i0 + 2);
            m.add_triangle(i0, i0 + 2, i0 + 3);
        }
        self.p.add(Shape::mesh(m));
    }
}

/// A colour faded by k (alpha or additive alike: premultiplied scaling).
pub fn fade_col(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let f = |v: u8| (v as f32 * k).round() as u8;
    Color32::from_rgba_premultiplied(f(c.r()), f(c.g()), f(c.b()), f(c.a()))
}

/// A sharp (♯) drawn as strokes (fonts on some systems lack U+266F: the Mac "C□5" bug, P26.1): `h` px tall,
/// centred at c.
pub fn draw_sharp(p: &Painter, c: Pos2, h: f32, col: Color32) {
    let w = (h * 0.09).max(1.0);
    let st = Stroke::new(w, col);
    let (hx, hy) = (h * 0.22, h * 0.5);
    for dx in [-0.35, 0.35] {
        p.line_segment(
            [
                c + vec2(dx * hx * 2.0 * 0.6, -hy),
                c + vec2(dx * hx * 2.0 * 0.6, hy),
            ],
            st,
        );
    }
    let sw = Stroke::new(w * 1.5, col);
    for dy in [-0.18, 0.18] {
        p.line_segment(
            [
                c + vec2(-hx * 1.3, dy * h + h * 0.06),
                c + vec2(hx * 1.3, dy * h - h * 0.06),
            ],
            sw,
        );
    }
}

/// Draw text with every ♯ (or #) drawn by hand: runs of plain text and a stroked sharp between them. Returns the
/// width drawn. `pos` is the text's left baseline-ish centre (left-centre anchor).
pub fn text_sharp(p: &Painter, s: &str, pos: Pos2, st: Style) -> f32 {
    let mut x = pos.x;
    for (i, part) in s.split(['♯', '#']).enumerate() {
        if i > 0 {
            let w = st.size * 0.45;
            draw_sharp(
                p,
                pos2(x + w / 2.0, pos.y - st.size * 0.12),
                st.size * 0.62,
                st.color,
            );
            x += w;
        }
        if !part.is_empty() {
            let r = text::draw(p, part, pos2(x, pos.y), Align2::LEFT_CENTER, st, None);
            x = r.max.x;
        }
    }
    x - pos.x
}

// Colours, low E (string 0) first. Measured on Rocksmith (research/spec 02, 04, 05), sRGB as decoded.

/// The game's own string hues (`ActorGuitarString.EnabledColors`).
pub const STR_GAME: [Color32; 6] = [
    rgb(255, 0, 0),
    rgb(255, 255, 0),
    rgb(0, 31, 255),
    rgb(255, 138, 48),
    rgb(0, 255, 46),
    rgb(144, 102, 234),
];
/// Gems (02 §1): rim (outline core), face top / middle / bottom, top face.
pub const RIM: [Color32; 6] = [
    rgb(255, 170, 154),
    rgb(255, 250, 144),
    rgb(206, 255, 254),
    rgb(254, 212, 142),
    rgb(207, 255, 172),
    rgb(255, 178, 255),
];
pub const FACE_TOP: [Color32; 6] = [
    rgb(199, 24, 5),
    rgb(237, 171, 0),
    rgb(3, 126, 237),
    rgb(226, 107, 13),
    rgb(67, 165, 38),
    rgb(150, 29, 184),
];
pub const FACE_MID: [Color32; 6] = [
    rgb(180, 26, 8),
    rgb(226, 155, 5),
    rgb(8, 104, 199),
    rgb(220, 97, 0),
    rgb(42, 146, 25),
    rgb(145, 31, 183),
];
pub const FACE_BOT: [Color32; 6] = [
    rgb(133, 17, 7),
    rgb(170, 100, 3),
    rgb(14, 90, 166),
    rgb(159, 85, 3),
    rgb(8, 113, 28),
    rgb(111, 23, 130),
];
pub const TOP_FACE: [Color32; 6] = [
    rgb(106, 18, 8),
    rgb(137, 86, 5),
    rgb(6, 80, 150),
    rgb(150, 70, 5),
    rgb(7, 104, 30),
    rgb(106, 23, 112),
];
/// Strings lit (04 §1.2); active = 0.8 of it, inactive 0.3.
pub const STR_LIT: [Color32; 6] = [
    rgb(228, 74, 70),
    rgb(211, 176, 23),
    rgb(28, 150, 210),
    rgb(241, 149, 29),
    rgb(64, 181, 26),
    rgb(185, 62, 206),
];
/// Gaussian core σ and halo τ per string (gaps).
pub const STR_SIGMA: [f32; 6] = [0.052, 0.039, 0.040, 0.036, 0.027, 0.030];
pub const STR_TAU: [f32; 6] = [0.27, 0.25, 0.17, 0.23, 0.20, 0.26];

/// A string's colour index (bass strings past 6 reuse the guitar's).
pub fn si(s: i32) -> usize {
    s.clamp(0, 5) as usize
}
