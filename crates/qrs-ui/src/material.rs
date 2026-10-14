//! Key materials from the gem-keys study: tortoiseshell (the pickguard), abalone (the inlay shell) and brushed
//! aluminium (the amp panel). Port of rsplay/material.py: SVG feTurbulence noise (the Filter Effects spec's
//! reference code, stitched so the sheet tiles), averaged over its colour channels, stretched by the material's
//! contrast and mapped through its colour ramp. crate::keys cuts keys from the sheets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use egui::Color32;

/// The sheet keys are cut from (scale 1; it tiles).
pub const SHEET: (usize, usize) = (360, 140);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mat {
    Tortoise,
    Abalone,
    Aluminium,
}

pub struct Noise {
    pub freq: (f64, f64),
    pub octaves: u32,
    pub seed: f64,
    pub fractal: bool,
}

/// A material and how a key cut from it looks (see rsplay/material.py for the meaning of each part).
pub struct Material {
    pub name: &'static str,
    pub noise: Noise,
    pub contrast: f32,
    pub mid: f32,
    pub ramp: &'static [&'static str],
    /// (colour, strength, (width, height) as fractions of the key's, where it fades out)
    pub gloss: ([u8; 3], f32, (f32, f32), f32),
    /// (colour, at the top, at the bottom)
    pub veil: Option<([u8; 3], f32, f32)>,
    /// aluminium: (top colour, bottom colour, strength)
    pub tint: Option<([u8; 3], [u8; 3], f32)>,
    /// a band of light down the key: (starts, peaks, ends) as fractions of its width, strength
    pub sheen: Option<(f32, f32, f32, f32)>,
    pub rim: ([u8; 3], f32),
    pub edge: [u8; 3],
    pub inner: ([u8; 3], f32),
    pub ink: Color32,
    pub shadow: f32,
    pub icon: Color32,
    /// The play mark's colour (None: the accent).
    pub play: Option<Color32>,
    /// Dark ink and ivory-style key caps.
    pub light: bool,
}

pub static TORTOISE: Material = Material {
    name: "Tortoiseshell",
    noise: Noise {
        freq: (0.02, 0.032),
        octaves: 4,
        seed: 9.0,
        fractal: true,
    },
    contrast: 3.4,
    mid: 0.53,
    ramp: &[
        "#120803", "#1f0e06", "#341709", "#55260d", "#7c3d13", "#a3591c", "#b8702a",
    ],
    gloss: ([255, 236, 210], 0.42, (1.10, 0.60), 0.55),
    veil: Some(([0, 0, 0], 0.10, 0.28)),
    tint: None,
    sheen: None,
    rim: ([255, 225, 190], 0.40),
    edge: [14, 6, 2],
    inner: ([0, 0, 0], 0.25),
    ink: Color32::from_rgb(251, 238, 221),
    shadow: 0.65,
    icon: Color32::from_rgb(241, 220, 192),
    play: None,
    light: false,
};

pub static ABALONE: Material = Material {
    name: "Abalone",
    noise: Noise {
        freq: (0.016, 0.04),
        octaves: 4,
        seed: 13.0,
        fractal: false,
    },
    contrast: 2.8,
    mid: 0.3,
    ramp: &[
        "#081622", "#0d3341", "#0f5d63", "#1f8a7a", "#2a6aa8", "#4b48a0", "#7d4a92", "#2a8790", "#0d2c3e",
    ],
    gloss: ([220, 255, 255], 0.40, (1.10, 0.60), 0.55),
    veil: Some(([4, 10, 20], 0.30, 0.46)),
    tint: None,
    sheen: None,
    rim: ([200, 255, 250], 0.42),
    edge: [6, 18, 28],
    inner: ([0, 0, 0], 0.25),
    ink: Color32::from_rgb(255, 255, 255),
    shadow: 0.7,
    icon: Color32::from_rgb(226, 241, 244),
    play: None,
    light: false,
};

pub static ALUMINIUM: Material = Material {
    name: "Brushed aluminium",
    noise: Noise {
        freq: (0.0015, 0.8),
        octaves: 2,
        seed: 2.0,
        fractal: true,
    },
    contrast: 2.2,
    mid: 0.5,
    ramp: &["#959ca5", "#aab0b8", "#bcc2c9", "#cdd2d7", "#dde0e4", "#e9ecee"],
    gloss: ([255, 255, 255], 0.60, (1.20, 0.60), 0.55),
    veil: None,
    tint: Some(([255, 255, 255], [20, 30, 40], 0.12)),
    sheen: Some((0.18, 0.42, 0.62, 0.20)),
    rim: ([255, 255, 255], 1.0),
    edge: [134, 141, 150],
    inner: ([30, 40, 50], 0.14),
    ink: Color32::from_rgb(28, 35, 43),
    shadow: 0.0,
    icon: Color32::from_rgb(61, 70, 82),
    play: Some(Color32::from_rgb(138, 84, 0)),
    light: true,
};

impl Mat {
    pub fn get(self) -> &'static Material {
        match self {
            Mat::Tortoise => &TORTOISE,
            Mat::Abalone => &ABALONE,
            Mat::Aluminium => &ALUMINIUM,
        }
    }
}

const BSIZE: usize = 0x100;
const BM: i64 = 0xFF;
const PERLIN_N: f64 = 4096.0;
const RAND_M: i64 = 2147483647;
const RAND_A: i64 = 16807;
const RAND_Q: i64 = 127773;
const RAND_R: i64 = 2836;

fn random(seed: i64) -> i64 {
    let s = RAND_A * (seed.rem_euclid(RAND_Q)) - RAND_R * seed.div_euclid(RAND_Q);
    if s <= 0 { s + RAND_M } else { s }
}

/// The reference code's lattice selector and gradients (one set per colour channel).
fn lattice(seed: f64) -> (Vec<usize>, Vec<[[f64; 2]; BSIZE]>) {
    let mut seed = seed.round_ties_even() as i64;
    if seed <= 0 {
        seed = -(seed.rem_euclid(RAND_M - 1)) + 1;
    }
    seed = seed.min(RAND_M - 1);
    let mut sel: Vec<usize> = (0..BSIZE).collect();
    let mut grad = vec![[[0.0f64; 2]; BSIZE]; 4];
    for g in grad.iter_mut() {
        for v in g.iter_mut() {
            for c in v.iter_mut() {
                seed = random(seed);
                *c = ((seed % (2 * BSIZE as i64)) - BSIZE as i64) as f64 / BSIZE as f64;
            }
            let n = v[0].hypot(v[1]);
            if n != 0.0 {
                v[0] /= n;
                v[1] /= n;
            }
        }
    }
    for i in (1..BSIZE).rev() {
        seed = random(seed);
        let j = (seed % BSIZE as i64) as usize;
        sel.swap(i, j);
    }
    (sel, grad)
}

fn stitch(f: f64, size: f64) -> f64 {
    if f == 0.0 {
        return f;
    }
    let lo = (size * f).floor() / size;
    let hi = (size * f).ceil() / size;
    if lo != 0.0 && f / lo < hi / f { lo } else { hi }
}

/// feTurbulence at (x, y) (filter space), stitched to `tile`: three channels, 0..1.
pub struct Turbulence {
    sel: Vec<usize>,
    grad: Vec<[[f64; 2]; BSIZE]>,
    fx: f64,
    fy: f64,
    octaves: u32,
    fractal: bool,
    sw0: i64,
    sh0: i64,
}

impl Turbulence {
    pub fn new(n: &Noise, tile: (usize, usize)) -> Turbulence {
        let fx = stitch(n.freq.0, tile.0 as f64);
        let fy = stitch(n.freq.1, tile.1 as f64);
        let (sel, grad) = lattice(n.seed);
        Turbulence {
            sel,
            grad,
            fx,
            fy,
            octaves: n.octaves,
            fractal: n.fractal,
            sw0: (tile.0 as f64 * fx).round_ties_even() as i64,
            sh0: (tile.1 as f64 * fy).round_ties_even() as i64,
        }
    }

    pub fn at(&self, xs: f64, ys: f64, ch: usize) -> f64 {
        let g = &self.grad[ch];
        let sel = &self.sel;
        let (mut x, mut y) = (xs * self.fx, ys * self.fy);
        let (mut sw, mut sh) = (self.sw0, self.sh0);
        let mut ratio = 1.0;
        let mut total = 0.0;
        for _ in 0..self.octaves {
            let (tx, ty) = (x + PERLIN_N, y + PERLIN_N);
            let (ix, iy) = (tx.floor() as i64, ty.floor() as i64);
            let (rx0, ry0) = (tx - ix as f64, ty - iy as f64);
            let (wrap_x, wrap_y) = (PERLIN_N as i64 + sw, PERLIN_N as i64 + sh);
            let bx0 = (if ix >= wrap_x { ix - sw } else { ix }) & BM;
            let bx1 = (if ix + 1 >= wrap_x { ix + 1 - sw } else { ix + 1 }) & BM;
            let by0 = (if iy >= wrap_y { iy - sh } else { iy }) & BM;
            let by1 = (if iy + 1 >= wrap_y { iy + 1 - sh } else { iy + 1 }) & BM;
            let i = sel[bx0 as usize] as i64;
            let j = sel[bx1 as usize] as i64;
            let b00 = sel[((i + by0) & BM) as usize];
            let b10 = sel[((j + by0) & BM) as usize];
            let b01 = sel[((i + by1) & BM) as usize];
            let b11 = sel[((j + by1) & BM) as usize];
            let sx = rx0 * rx0 * (3.0 - 2.0 * rx0);
            let sy = ry0 * ry0 * (3.0 - 2.0 * ry0);
            let mut a = rx0 * g[b00][0] + ry0 * g[b00][1];
            a += sx * ((rx0 - 1.0) * g[b10][0] + ry0 * g[b10][1] - a);
            let mut b = rx0 * g[b01][0] + (ry0 - 1.0) * g[b01][1];
            b += sx * ((rx0 - 1.0) * g[b11][0] + (ry0 - 1.0) * g[b11][1] - b);
            let n = a + sy * (b - a);
            total += if self.fractal { n } else { n.abs() } / ratio;
            x *= 2.0;
            y *= 2.0;
            ratio *= 2.0;
            sw *= 2;
            sh *= 2;
        }
        let v = if self.fractal { (total + 1.0) / 2.0 } else { total };
        v.clamp(0.0, 1.0)
    }
}

fn hex(c: &str) -> [f32; 3] {
    let v = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).unwrap_or(0) as f32;
    [v(1), v(3), v(5)]
}

/// feComponentTransfer's table: v (0..1) through the colours, linearly.
fn ramp(v: f32, colours: &[&str]) -> [f32; 3] {
    let n = colours.len() - 1;
    let pos = v.clamp(0.0, 1.0) * n as f32;
    let i0 = (pos.floor() as usize).min(n - 1);
    let (a, b) = (hex(colours[i0]), hex(colours[i0 + 1]));
    let f = pos - i0 as f32;
    [
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
    ]
}

/// A material's sheet: rows x columns of RGB (0..255).
pub struct Sheet {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<[f32; 3]>,
}

impl Sheet {
    pub fn at(&self, x: i64, y: i64) -> [f32; 3] {
        let xx = x.rem_euclid(self.w as i64) as usize;
        let yy = y.rem_euclid(self.h as i64) as usize;
        self.rgb[yy * self.w + xx]
    }
}

/// Build a sheet at `scale` (no caching).
pub fn build_sheet(mat: Mat, scale: f32) -> Sheet {
    build_sheet64(mat, crate::paint::sc(scale))
}

/// Build a sheet at the scale as the Python float it is there (its samples sit at (x + 0.5) / scale).
pub fn build_sheet64(mat: Mat, scale: f64) -> Sheet {
    let m = mat.get();
    let w = ((SHEET.0 as f64 * scale).round_ties_even() as usize).max(1);
    let h = ((SHEET.1 as f64 * scale).round_ties_even() as usize).max(1);
    let tb = Turbulence::new(&m.noise, SHEET);
    let mut rgb = Vec::with_capacity(w * h);
    let k = m.contrast;
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = ((x as f64 + 0.5) / scale, (y as f64 + 0.5) / scale);
            let mut grey = 0.0f32;
            for ch in 0..3 {
                let n = (tb.at(fx, fy, ch) * 255.0).floor() / 255.0; // (8 bits a channel, as the filter keeps them)
                grey += n as f32;
            }
            let grey = 0.33 * grey;
            rgb.push(ramp(k * grey + (0.5 - k * m.mid), m.ramp));
        }
    }
    Sheet { w, h, rgb }
}

type SheetKey = (Mat, i64);

fn sheets() -> &'static Mutex<HashMap<SheetKey, Arc<Sheet>>> {
    static S: OnceLock<Mutex<HashMap<SheetKey, Arc<Sheet>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The material's sheet at `scale` (the window's, as the Python app works it out), built once per scale.
pub fn sheet(mat: Mat, scale: f32) -> Arc<Sheet> {
    sheet64(mat, crate::paint::sc(scale))
}

/// `sheet` at a scale given as the Python float (kept per `round(scale, 3)`, as material.py keeps them).
pub fn sheet64(mat: Mat, scale: f64) -> Arc<Sheet> {
    let key = (mat, (scale * 1000.0).round_ties_even() as i64);
    if let Some(s) = sheets().lock().ok().and_then(|m| m.get(&key).cloned()) {
        return s;
    }
    let s = Arc::new(build_sheet64(mat, scale));
    if let Ok(mut m) = sheets().lock() {
        m.insert(key, s.clone());
    }
    s
}

/// Build every material's sheet at `scale` in the background, so no key waits for one (at start, on a resize).
pub fn warm(scale: f32) {
    std::thread::spawn(move || {
        for m in [Mat::Tortoise, Mat::Abalone, Mat::Aluminium] {
            sheet(m, scale);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_matches_the_reference_generator() {
        // Park-Miller minimal standard: 16807^1 from seed 1
        assert_eq!(random(1), 16807);
        assert_eq!(random(16807), 282475249);
    }

    /// The sheets against rsplay.material's (QRS_NEU_DUMPS, see keys.rs): rounded to bytes, to within a level.
    #[test]
    fn neu_parity_sheets() {
        let Ok(dir) = std::env::var("QRS_NEU_DUMPS") else {
            return;
        };
        let s = 1440.0f64 / 1400.0;
        let mut bad = Vec::new();
        for (m, name) in [
            (Mat::Tortoise, "tortoise"),
            (Mat::Abalone, "abalone"),
            (Mat::Aluminium, "aluminium"),
        ] {
            let sh = build_sheet64(m, s);
            let mut img = crate::raster::Image::new(sh.w, sh.h);
            for (i, c) in sh.rgb.iter().enumerate() {
                for ch in 0..3 {
                    img.rgba[i * 4 + ch] = (c[ch] + 0.5).clamp(0.0, 255.0) as u8;
                }
                img.rgba[i * 4 + 3] = 255;
            }
            if let Some(e) = crate::raster::tests_cmp(&dir, &format!("sheet_{name}"), &img) {
                bad.push(e);
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn sheets_tile_and_stay_in_their_ramp() {
        for m in [Mat::Tortoise, Mat::Abalone, Mat::Aluminium] {
            let s = build_sheet(m, 0.25);
            assert_eq!((s.w, s.h), (90, 35));
            for px in &s.rgb {
                assert!(px.iter().all(|v| (0.0..=255.0).contains(v)));
            }
        }
    }
}
