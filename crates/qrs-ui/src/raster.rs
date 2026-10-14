//! CPU rasterizing for the neumorphic look: a port of rsplay/neu.py's numpy code. Coverage masks (anti-aliased
//! rounded rectangles and discs), the same blur recipe, shifts, and "over" compositing of colours through masks.
//! Everything here works in physical pixels; the results become egui textures (crate::paint).

use egui::Color32;

/// A coverage / alpha mask, row-major, values usually 0..1.
#[derive(Clone, Debug)]
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
}

impl Mask {
    pub fn new(w: usize, h: usize) -> Mask {
        Mask {
            w,
            h,
            v: vec![0.0; w * h],
        }
    }

    pub fn filled(w: usize, h: usize, value: f32) -> Mask {
        Mask {
            w,
            h,
            v: vec![value; w * h],
        }
    }

    pub fn from_fn(w: usize, h: usize, mut f: impl FnMut(usize, usize) -> f32) -> Mask {
        let mut v = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                v.push(f(x, y));
            }
        }
        Mask { w, h, v }
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.v[y * self.w + x]
    }

    /// Anti-aliased coverage of a `w` x `h` rounded rectangle with radius `r` at (`x0`, `y0`) on a `cw` x `ch`
    /// canvas (neu.py `_rrect`).
    pub fn rrect(w: f32, h: f32, r: f32, x0: f32, y0: f32, cw: usize, ch: usize) -> Mask {
        let r = r.max(0.0).min(w / 2.0).min(h / 2.0);
        let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
        Mask::from_fn(cw, ch, |x, y| {
            let qx = (x as f32 + 0.5 - cx).abs() - hx;
            let qy = (y as f32 + 0.5 - cy).abs() - hy;
            let sd = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
            (0.5 - sd).clamp(0.0, 1.0)
        })
    }

    /// `rrect` filling the whole canvas.
    pub fn rrect_full(w: usize, h: usize, r: f32) -> Mask {
        Mask::rrect(w as f32, h as f32, r, 0.0, 0.0, w, h)
    }

    /// An anti-aliased disc of radius `r` centred at (`cx`, `cy`).
    pub fn disc(cw: usize, ch: usize, cx: f32, cy: f32, r: f32) -> Mask {
        Mask::from_fn(cw, ch, |x, y| {
            let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
            (r - d + 0.5).clamp(0.0, 1.0)
        })
    }

    /// The mask moved by (dx, dy), zeros shifted in (neu.py `_shift`).
    pub fn shift(&self, dx: i32, dy: i32) -> Mask {
        let mut out = Mask::new(self.w, self.h);
        let (w, h) = (self.w as i32, self.h as i32);
        let (y0, y1, x0, x1) = (dy.max(0), (h + dy).min(h), dx.max(0), (w + dx).min(w));
        if y1 > y0 && x1 > x0 {
            for y in y0..y1 {
                let src = ((y - dy) * w + (x0 - dx)) as usize;
                let dst = (y * w + x0) as usize;
                let n = (x1 - x0) as usize;
                out.v[dst..dst + n].copy_from_slice(&self.v[src..src + n]);
            }
        }
        out
    }

    /// Gaussian blur of standard deviation `sigma` px, zeros beyond the edges (neu.py `_blur`: three box blurs for
    /// sigma > 3, an exact separable kernel below).
    pub fn blur(&self, sigma: f32) -> Mask {
        self.blur64(sigma as f64)
    }

    /// `blur` with sigma as the Python float it is there (the kernel's choices are made in f64, its weights in
    /// f32, as numpy does).
    pub fn blur64(&self, sigma: f64) -> Mask {
        let rad = (3.0 * sigma).ceil() as i32;
        if rad < 1 {
            return self.clone();
        }
        if sigma > 3.0 {
            let s2 = sigma * sigma;
            let mut wl = (12.0 * s2 / 3.0 + 1.0).sqrt() as i32;
            wl -= 1 - wl % 2;
            let m = ((12.0 * s2 - 3.0 * (wl * wl) as f64 - 12.0 * wl as f64 - 9.0) / (-4.0 * wl as f64 - 4.0))
                .round_ties_even() as i32;
            let mut a = self.clone();
            for i in 0..3 {
                let w = if i < m { wl } else { wl + 2 };
                a = a.box_blur(w as usize, true).box_blur(w as usize, false);
            }
            return a;
        }
        let d = (2.0 * sigma * sigma) as f32;
        let k: Vec<f32> = (-rad..=rad).map(|x| (-((x * x) as f32) / d).exp()).collect();
        let sum: f32 = k.iter().sum();
        let k: Vec<f32> = k.iter().map(|v| v / sum).collect();
        self.convolve(&k, true).convolve(&k, false)
    }

    fn convolve(&self, k: &[f32], across: bool) -> Mask {
        let rad = (k.len() / 2) as isize;
        let mut out = Mask::new(self.w, self.h);
        let (w, h) = (self.w as isize, self.h as isize);
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0;
                for (i, &kv) in k.iter().enumerate() {
                    let o = i as isize - rad;
                    let (sx, sy) = if across { (x + o, y) } else { (x, y + o) };
                    if sx >= 0 && sx < w && sy >= 0 && sy < h {
                        acc += kv * self.v[(sy * w + sx) as usize];
                    }
                }
                out.v[(y * w + x) as usize] = acc;
            }
        }
        out
    }

    /// A centred box blur `w` (odd) wide, zeros beyond the edges (neu.py `_box`).
    fn box_blur(&self, w: usize, across: bool) -> Mask {
        let r = (w / 2) as isize;
        let mut out = Mask::new(self.w, self.h);
        let (cw, ch) = (self.w as isize, self.h as isize);
        let inv = 1.0 / w as f32;
        let (n, lines) = if across { (cw, ch) } else { (ch, cw) };
        let idx = |line: isize, i: isize| -> usize {
            if across {
                (line * cw + i) as usize
            } else {
                (i * cw + line) as usize
            }
        };
        for line in 0..lines {
            // running sum over [i - r, i + r]
            let mut acc = 0.0f32;
            for i in -r..=r {
                if (0..n).contains(&i) {
                    acc += self.v[idx(line, i)];
                }
            }
            for i in 0..n {
                out.v[idx(line, i)] = acc * inv;
                let add = i + r + 1;
                let sub = i - r;
                if add < n {
                    acc += self.v[idx(line, add)];
                }
                if sub >= 0 {
                    acc -= self.v[idx(line, sub)];
                }
            }
        }
        out
    }

    pub fn map(&self, f: impl Fn(f32) -> f32) -> Mask {
        Mask {
            w: self.w,
            h: self.h,
            v: self.v.iter().map(|&v| f(v)).collect(),
        }
    }

    pub fn zip(&self, o: &Mask, f: impl Fn(f32, f32) -> f32) -> Mask {
        debug_assert_eq!((self.w, self.h), (o.w, o.h));
        Mask {
            w: self.w,
            h: self.h,
            v: self.v.iter().zip(&o.v).map(|(&a, &b)| f(a, b)).collect(),
        }
    }

    pub fn scale(&self, k: f32) -> Mask {
        self.map(|v| v * k)
    }

    /// clip(self - o, 0, 1): an edge where `o` is the shape moved.
    pub fn minus(&self, o: &Mask) -> Mask {
        self.zip(o, |a, b| (a - b).clamp(0.0, 1.0))
    }

    pub fn times(&self, o: &Mask) -> Mask {
        self.zip(o, |a, b| a * b)
    }

    pub fn crop(&self, x0: usize, y0: usize, w: usize, h: usize) -> Mask {
        Mask::from_fn(w, h, |x, y| self.at(x0 + x, y0 + y))
    }
}

/// Colours composited "over" each other through masks, kept premultiplied (neu.py `_Layers`).
pub struct Layers {
    pub w: usize,
    pub h: usize,
    /// Premultiplied colour, 0..1.
    pub rgb: Vec<[f32; 3]>,
    pub a: Vec<f32>,
}

pub fn rgbf(c: Color32) -> [f32; 3] {
    [c.r() as f32, c.g() as f32, c.b() as f32]
}

impl Layers {
    pub fn new(w: usize, h: usize) -> Layers {
        Layers {
            w,
            h,
            rgb: vec![[0.0; 3]; w * h],
            a: vec![0.0; w * h],
        }
    }

    /// A flat colour (0..255 channels) over everything, through `alpha`.
    pub fn over(&mut self, color: [f32; 3], alpha: &Mask) {
        let c = [color[0] / 255.0, color[1] / 255.0, color[2] / 255.0];
        for (i, &al) in alpha.v.iter().enumerate() {
            if al <= 0.0 {
                continue;
            }
            let al = al.min(1.0);
            let p = &mut self.rgb[i];
            for ch in 0..3 {
                p[ch] = p[ch] * (1.0 - al) + al * c[ch];
            }
            self.a[i] = self.a[i] * (1.0 - al) + al;
        }
    }

    pub fn over_c(&mut self, color: Color32, alpha: &Mask) {
        self.over(rgbf(color), alpha)
    }

    /// A flat colour over everything with one alpha.
    pub fn over_all(&mut self, color: [f32; 3], alpha: f32) {
        self.over(color, &Mask::filled(self.w, self.h, alpha));
    }

    /// A colour per pixel (0..255) over everything, through `alpha`.
    pub fn over_img(&mut self, colors: &[[f32; 3]], alpha: &Mask) {
        for (i, &al) in alpha.v.iter().enumerate() {
            if al <= 0.0 {
                continue;
            }
            let al = al.min(1.0);
            let c = colors[i];
            let p = &mut self.rgb[i];
            for ch in 0..3 {
                p[ch] = p[ch] * (1.0 - al) + al * c[ch] / 255.0;
            }
            self.a[i] = self.a[i] * (1.0 - al) + al;
        }
    }

    /// Unpremultiplied RGBA bytes, alpha optionally clipped by a mask (neu.py `rgba`).
    pub fn rgba(&self, clip: Option<&Mask>) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for i in 0..self.w * self.h {
            let a = self.a[i];
            let safe = if a > 1e-6 { a } else { 1.0 };
            for ch in 0..3 {
                out.push((self.rgb[i][ch] / safe * 255.0 + 0.5).clamp(0.0, 255.0) as u8);
            }
            let ca = clip.map(|m| m.v[i]).unwrap_or(1.0);
            out.push((a * ca * 255.0 + 0.5).clamp(0.0, 255.0) as u8);
        }
        out
    }

    pub fn image(&self, clip: Option<&Mask>) -> Image {
        Image {
            w: self.w,
            h: self.h,
            rgba: self.rgba(clip),
        }
    }
}

/// Unpremultiplied RGBA8 pixels.
#[derive(Clone, Debug)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(w: usize, h: usize) -> Image {
        Image {
            w,
            h,
            rgba: vec![0; w * h * 4],
        }
    }

    pub fn to_egui(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([self.w, self.h], &self.rgba)
    }

    /// Area-averaged downscale by an integer factor (pygame's smoothscale of a supersampled drawing), averaging
    /// premultiplied colour so edges don't darken.
    pub fn downsample(&self, k: usize) -> Image {
        if k <= 1 {
            return self.clone();
        }
        let (w, h) = (self.w / k, self.h / k);
        let mut out = Image::new(w, h);
        let n = (k * k) as f32;
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for sy in 0..k {
                    for sx in 0..k {
                        let i = ((y * k + sy) * self.w + x * k + sx) * 4;
                        let a = self.rgba[i + 3] as f32;
                        for ch in 0..3 {
                            acc[ch] += self.rgba[i + ch] as f32 * a;
                        }
                        acc[3] += a;
                    }
                }
                let o = (y * w + x) * 4;
                let a = acc[3] / n;
                for ch in 0..3 {
                    out.rgba[o + ch] = if acc[3] > 0.0 {
                        (acc[ch] / acc[3] + 0.5).min(255.0) as u8
                    } else {
                        0
                    };
                }
                out.rgba[o + 3] = (a + 0.5).min(255.0) as u8;
            }
        }
        out
    }

    /// Draw `src` over this image with its top-left at (x, y), scaled in alpha by `alpha`.
    pub fn blit(&mut self, src: &Image, x: i32, y: i32, alpha: f32) {
        for sy in 0..src.h as i32 {
            let dy = y + sy;
            if dy < 0 || dy >= self.h as i32 {
                continue;
            }
            for sx in 0..src.w as i32 {
                let dx = x + sx;
                if dx < 0 || dx >= self.w as i32 {
                    continue;
                }
                let si = ((sy as usize) * src.w + sx as usize) * 4;
                let di = ((dy as usize) * self.w + dx as usize) * 4;
                let sa = src.rgba[si + 3] as f32 / 255.0 * alpha;
                if sa <= 0.0 {
                    continue;
                }
                let da = self.rgba[di + 3] as f32 / 255.0;
                let oa = sa + da * (1.0 - sa);
                for ch in 0..3 {
                    let sc = src.rgba[si + ch] as f32;
                    let dc = self.rgba[di + ch] as f32;
                    let v = if oa > 0.0 {
                        (sc * sa + dc * da * (1.0 - sa)) / oa
                    } else {
                        0.0
                    };
                    self.rgba[di + ch] = (v + 0.5).clamp(0.0, 255.0) as u8;
                }
                self.rgba[di + 3] = (oa * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// The alpha channel as a mask.
    pub fn alpha(&self) -> Mask {
        Mask {
            w: self.w,
            h: self.h,
            v: self.rgba.chunks_exact(4).map(|p| p[3] as f32 / 255.0).collect(),
        }
    }

    /// The same pixels with alpha multiplied by `k`.
    pub fn faded(&self, k: f32) -> Image {
        let mut out = self.clone();
        for p in out.rgba.chunks_exact_mut(4) {
            p[3] = (p[3] as f32 * k + 0.5).min(255.0) as u8;
        }
        out
    }
}

/// tiny-skia drawing for vector shapes (icons, facets, pointers, arcs): a premultiplied pixmap converted to Image.
pub fn from_pixmap(p: &tiny_skia::Pixmap) -> Image {
    let mut rgba = Vec::with_capacity(p.data().len());
    for px in p.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Image {
        w: p.width() as usize,
        h: p.height() as usize,
        rgba,
    }
}

pub fn skia_color(c: Color32, alpha: f32) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r(), c.g(), c.b(), (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
}

pub fn paint(c: Color32, alpha: f32) -> tiny_skia::Paint<'static> {
    let mut p = tiny_skia::Paint::default();
    p.set_color(skia_color(c, alpha));
    p.anti_alias = true;
    p
}

/// A polygon path (closed).
pub fn polygon(points: &[(f32, f32)]) -> Option<tiny_skia::Path> {
    let mut b = tiny_skia::PathBuilder::new();
    let (first, rest) = points.split_first()?;
    b.move_to(first.0, first.1);
    for p in rest {
        b.line_to(p.0, p.1);
    }
    b.close();
    b.finish()
}

/// An open polyline path.
pub fn polyline(points: &[(f32, f32)]) -> Option<tiny_skia::Path> {
    let mut b = tiny_skia::PathBuilder::new();
    let (first, rest) = points.split_first()?;
    b.move_to(first.0, first.1);
    for p in rest {
        b.line_to(p.0, p.1);
    }
    b.finish()
}

pub fn circle(cx: f32, cy: f32, r: f32) -> Option<tiny_skia::Path> {
    tiny_skia::PathBuilder::from_circle(cx, cy, r.max(0.01))
}

pub fn stroke(width: f32) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    }
}

/// Mix two colours (t = 0 gives a), as rsplay.anim.mix (truncating).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// pygame's software renderer, as the Python app uses it (pygame-ce 2.5.8: src_c/draw.c, transform.c,
/// simd_transform_sse2.c, rotozoom.c, simd_blitters_avx2.c): the non-anti-aliased primitives its small art is drawn
/// with at 3-4x, the anti-aliased circles and lines it draws on the screen, smoothscale (the SSE2 filters it runs
/// with), gaussian_blur, rotozoom, its alpha blit and BLEND_RGBA_MULT fills, each ported line by line so the art
/// comes out the same to the pixel. Coordinates are whole pixels as pygame takes them (`int()` of a float).
pub mod pg {
    use super::Image;
    use egui::Color32;

    /// A colour as pygame maps it onto an SRCALPHA surface: (r, g, b) is opaque.
    pub type Rgba = [u8; 4];

    pub fn rgb(c: Color32) -> Rgba {
        [c.r(), c.g(), c.b(), 255]
    }

    /// `int(v)` of a Python float (toward zero), as pygame takes coordinates.
    pub fn ti(v: f64) -> i32 {
        v as i32
    }

    /// What pygame's draw functions write to.
    pub trait Target {
        fn size(&self) -> (i32, i32);
        /// set_at, clipped: the pixel becomes `c`.
        fn set(&mut self, x: i32, y: i32, c: Rgba);
        /// get_antialiased_color then set_at, clipped: `c` over the pixel by `b` (0..1).
        fn blend(&mut self, x: i32, y: i32, c: Rgba, b: f32);
    }

    /// A pygame SRCALPHA surface: straight (not premultiplied) RGBA, (0, 0, 0, 0) when new.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Surf {
        pub w: i32,
        pub h: i32,
        pub px: Vec<Rgba>,
    }

    impl Target for Surf {
        fn size(&self) -> (i32, i32) {
            (self.w, self.h)
        }

        fn set(&mut self, x: i32, y: i32, c: Rgba) {
            if x >= 0 && y >= 0 && x < self.w && y < self.h {
                self.px[(y * self.w + x) as usize] = c;
            }
        }

        fn blend(&mut self, x: i32, y: i32, c: Rgba, b: f32) {
            if x >= 0 && y >= 0 && x < self.w && y < self.h {
                let p = &mut self.px[(y * self.w + x) as usize];
                for i in 0..4 {
                    p[i] = (b * c[i] as f32 + (1.0 - b) * p[i] as f32) as u8;
                }
            }
        }
    }

    /// The screen under pygame's drawing, as colour and coverage over whatever is there (premultiplied): a set
    /// pixel is covered, a blended one covered by its brightness, so the shape can be drawn over the real
    /// background as a texture and come out as pygame drew it. It keeps the box (ox, oy, w, h) of the screen's
    /// pixels, and is drawn on in the screen's own coordinates (so float positions round as they do there).
    pub struct Cover {
        pub ox: i32,
        pub oy: i32,
        pub w: i32,
        pub h: i32,
        px: Vec<[f32; 4]>,
    }

    impl Cover {
        pub fn new(w: i32, h: i32) -> Cover {
            Cover::at(0, 0, w, h)
        }

        /// The box (x, y, w, h) of the screen.
        pub fn at(ox: i32, oy: i32, w: i32, h: i32) -> Cover {
            Cover {
                ox,
                oy,
                w: w.max(1),
                h: h.max(1),
                px: vec![[0.0; 4]; (w.max(1) * h.max(1)) as usize],
            }
        }

        fn index(&self, x: i32, y: i32) -> Option<usize> {
            let (x, y) = (x - self.ox, y - self.oy);
            (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| (y * self.w + x) as usize)
        }

        pub fn image(&self) -> Image {
            let mut img = Image::new(self.w as usize, self.h as usize);
            for (i, p) in self.px.iter().enumerate() {
                let a = p[3];
                if a > 1e-6 {
                    for ch in 0..3 {
                        img.rgba[i * 4 + ch] = (p[ch] / a * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                    }
                }
                img.rgba[i * 4 + 3] = (a * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
            img
        }
    }

    impl Target for Cover {
        /// The screen as far as clipping goes (it is never reached: the box is only what is kept).
        fn size(&self) -> (i32, i32) {
            (1 << 24, 1 << 24)
        }

        fn set(&mut self, x: i32, y: i32, c: Rgba) {
            if let Some(i) = self.index(x, y) {
                let a = c[3] as f32 / 255.0;
                self.px[i] = [
                    c[0] as f32 / 255.0 * a,
                    c[1] as f32 / 255.0 * a,
                    c[2] as f32 / 255.0 * a,
                    a,
                ];
            }
        }

        fn blend(&mut self, x: i32, y: i32, c: Rgba, b: f32) {
            if let Some(i) = self.index(x, y) {
                let p = &mut self.px[i];
                for ch in 0..3 {
                    p[ch] = c[ch] as f32 / 255.0 * b + p[ch] * (1.0 - b);
                }
                p[3] = b + p[3] * (1.0 - b);
            }
        }
    }

    impl Surf {
        pub fn new(w: i32, h: i32) -> Surf {
            let (w, h) = (w.max(0), h.max(0));
            Surf {
                w,
                h,
                px: vec![[0; 4]; (w * h) as usize],
            }
        }

        /// A surface from an image (straight RGBA).
        pub fn from_image(img: &Image) -> Surf {
            Surf {
                w: img.w as i32,
                h: img.h as i32,
                px: img
                    .rgba
                    .chunks_exact(4)
                    .map(|p| [p[0], p[1], p[2], p[3]])
                    .collect(),
            }
        }

        pub fn image(&self) -> Image {
            Image {
                w: self.w as usize,
                h: self.h as usize,
                rgba: self.px.iter().flatten().copied().collect(),
            }
        }

        pub fn at(&self, x: i32, y: i32) -> Rgba {
            self.px[(y * self.w + x) as usize]
        }

        /// `fill(c, special_flags=BLEND_RGBA_MULT)`: every channel times `c`'s, `(a * b + 255) >> 8`.
        pub fn fill_mult(&mut self, c: Rgba) {
            for p in &mut self.px {
                for i in 0..4 {
                    p[i] = ((p[i] as u32 * c[i] as u32 + 255) >> 8) as u8;
                }
            }
        }

        /// `self.blit(src, (x, y))` of one SRCALPHA surface onto another: pygame's own alpha blend (where this one
        /// is clear the source is copied; else RGB = (d (256 - a) + s (a + 1)) >> 8, A = a + d - a d / 255).
        pub fn blit(&mut self, src: &Surf, x: i32, y: i32) {
            for sy in 0..src.h {
                let dy = y + sy;
                if dy < 0 || dy >= self.h {
                    continue;
                }
                for sx in 0..src.w {
                    let dx = x + sx;
                    if dx < 0 || dx >= self.w {
                        continue;
                    }
                    let s = src.px[(sy * src.w + sx) as usize];
                    let d = &mut self.px[(dy * self.w + dx) as usize];
                    *d = blend_over(s, *d);
                }
            }
        }

        // -- transform.smoothscale (the SSE2 filters) ---------------------------------------------------------

        /// `pygame.transform.smoothscale(self, (w, h))`.
        pub fn smoothscale(&self, w: i32, h: i32) -> Surf {
            if w == self.w && h == self.h {
                return self.clone();
            }
            let (sw, sh) = (self.w, self.h);
            let tmp = if w < sw {
                shrink_x(&self.px, sw, sh, w)
            } else if w > sw {
                expand_x(&self.px, sw, sh, w)
            } else {
                self.px.clone()
            };
            let px = if h < sh {
                shrink_y(&tmp, w, sh, h)
            } else if h > sh {
                expand_y(&tmp, w, sh, h)
            } else {
                tmp
            };
            Surf { w, h, px }
        }

        /// `pygame.transform.gaussian_blur(self, radius)` (edge pixels repeated): a kernel of sigma `radius` out to
        /// twice that, down then across, in f32, truncated to bytes.
        pub fn gaussian_blur(&self, radius: i32) -> Surf {
            let (w, h) = (self.w, self.h);
            let mut out = Surf::new(w, h);
            if w == 0 || h == 0 {
                return out;
            }
            let mut sigma = radius.max(0);
            if sigma > w.min(h) {
                sigma = w.min(h) - 1;
            }
            let kr = sigma * 2;
            let mut lut = vec![0f32; (kr + 1) as usize];
            let mut sum = 0f32;
            for i in 0..=kr {
                lut[i as usize] = (-(i as f32).powf(2.0) / (2.0 * (sigma as f32).powf(2.0))).exp();
                sum += lut[i as usize] * 2.0;
            }
            sum -= lut[0];
            for v in lut.iter_mut() {
                *v /= sum;
            }
            let n = (w * 4) as usize;
            let mut buf = vec![0f32; n];
            let mut buf2 = vec![0f32; n];
            let byte = |x: i32, y: i32, c: usize| self.px[(y * w + x) as usize][c] as f32;
            for y in 0..h {
                for j in -kr..=kr {
                    let k = lut[j.unsigned_abs() as usize];
                    let yy = (y + j).clamp(0, h - 1);
                    for x in 0..w {
                        for c in 0..4 {
                            buf[(x * 4) as usize + c] += byte(x, yy, c) * k;
                        }
                    }
                }
                for x in 0..w {
                    for j in -kr..=kr {
                        let k = lut[j.unsigned_abs() as usize];
                        let xx = (x + j).clamp(0, w - 1);
                        for c in 0..4 {
                            buf2[(x * 4) as usize + c] += buf[(xx * 4) as usize + c] * k;
                        }
                    }
                }
                for x in 0..w {
                    let p = &mut out.px[(y * w + x) as usize];
                    for c in 0..4 {
                        p[c] = buf2[(x * 4) as usize + c] as u8;
                    }
                }
                buf.iter_mut().for_each(|v| *v = 0.0);
                buf2.iter_mut().for_each(|v| *v = 0.0);
            }
            out
        }

        /// `pygame.transform.rotozoom(self, angle, 1.0)` (degrees, counter-clockwise; bilinear).
        pub fn rotozoom(&self, angle: f32) -> Surf {
            let angle = angle as f64;
            if angle.abs() <= 0.001 {
                return self.zoom1();
            }
            let rad = angle * (std::f64::consts::PI / 180.0);
            let (sa, ca) = (rad.sin(), rad.cos());
            let (x, y) = ((self.w / 2) as f64, (self.h / 2) as f64);
            let (cx, cy, sx, sy) = (ca * x, ca * y, sa * x, sa * y);
            let half_w = ((cx + sy)
                .abs()
                .max((cx - sy).abs())
                .max((-cx + sy).abs())
                .max((-cx - sy).abs())
                .ceil() as i32)
                .max(1);
            let half_h = ((sx + cy)
                .abs()
                .max((sx - cy).abs())
                .max((-sx + cy).abs())
                .max((-sx - cy).abs())
                .ceil() as i32)
                .max(1);
            let (dw, dh) = (2 * half_w, 2 * half_h);
            let (isin, icos) = ((sa * 65536.0) as i32, (ca * 65536.0) as i32);
            let mut out = Surf::new(dw, dh);
            let (cx, cy) = (dw / 2, dh / 2);
            let xd = (self.w - dw).wrapping_mul(1 << 15);
            let yd = (self.h - dh).wrapping_mul(1 << 15);
            let ax = (cx << 16).wrapping_sub(icos.wrapping_mul(cx));
            let ay = (cy << 16).wrapping_sub(isin.wrapping_mul(cx));
            let (sw, sh) = (self.w - 1, self.h - 1);
            let src = |x: i32, y: i32| self.px[(y * self.w + x) as usize];
            for y in 0..dh {
                let dy = cy - y;
                let mut sdx = ax.wrapping_add(isin.wrapping_mul(dy)).wrapping_add(xd);
                let mut sdy = ay.wrapping_sub(icos.wrapping_mul(dy)).wrapping_add(yd);
                for x in 0..dw {
                    let (dx, dy) = (sdx >> 16, sdy >> 16);
                    if dx >= -1 && dy >= -1 && dx < self.w && dy < self.h {
                        let (c00, c01, c10, c11);
                        if dx >= 0 && dy >= 0 && dx < sw && dy < sh {
                            c00 = src(dx, dy);
                            c01 = src(dx + 1, dy);
                            c10 = src(dx, dy + 1);
                            c11 = src(dx + 1, dy + 1);
                        } else if dx == sw && dy == sh {
                            c00 = src(dx, dy);
                            (c01, c10, c11) = (c00, c00, c00);
                        } else if dx == -1 && dy == -1 {
                            c00 = src(0, 0);
                            (c01, c10, c11) = (c00, c00, c00);
                        } else if dx == -1 && dy == sh {
                            c00 = src(0, dy);
                            (c01, c10, c11) = (c00, c00, c00);
                        } else if dx == sw && dy == -1 {
                            c00 = src(dx, 0);
                            (c01, c10, c11) = (c00, c00, c00);
                        } else if dx == -1 {
                            c00 = src(0, dy);
                            (c01, c10) = (c00, c00);
                            c11 = src(0, dy + 1);
                        } else if dy == -1 {
                            c00 = src(dx, 0);
                            (c01, c10) = (c00, c00);
                            c11 = src(dx + 1, 0);
                        } else if dx == sw {
                            c00 = src(dx, dy);
                            c01 = c00;
                            c10 = src(dx, dy + 1);
                            c11 = c10;
                        } else if dy == sh {
                            c00 = src(dx, dy);
                            c01 = src(dx + 1, dy);
                            (c10, c11) = (c01, c01);
                        } else {
                            c00 = src(0, 0);
                            (c01, c10, c11) = (c00, c00, c00);
                        }
                        let (ex, ey) = (sdx & 0xffff, sdy & 0xffff);
                        let p = &mut out.px[(y * dw + x) as usize];
                        for c in 0..4 {
                            let (a, b, cc, d) = (c00[c] as i32, c01[c] as i32, c10[c] as i32, c11[c] as i32);
                            let t1 = ((((b - a) * ex) >> 16) + a) & 0xff;
                            let t2 = ((((d - cc) * ex) >> 16) + cc) & 0xff;
                            p[c] = ((((t2 - t1) * ey) >> 16) + t1) as u8;
                        }
                    }
                    sdx = sdx.wrapping_add(icos);
                    sdy = sdy.wrapping_add(isin);
                }
            }
            out
        }

        /// rotozoom's zoom-only path at 1.0 (smooth): resampled by (w - 1) / w.
        fn zoom1(&self) -> Surf {
            let (w, h) = (self.w, self.h);
            let mut out = Surf::new(w, h);
            if w < 2 || h < 2 {
                return self.clone();
            }
            let sx = (65536.0 * (w - 1) as f32 as f64 / w as f32 as f64) as i32;
            let sy = (65536.0 * (h - 1) as f32 as f64 / h as f32 as f64) as i32;
            let steps = |n: i32, s: i32| -> Vec<i32> {
                let mut v = Vec::with_capacity(n as usize + 1);
                let mut c = 0i32;
                for _ in 0..=n {
                    v.push(c);
                    c &= 0xffff;
                    c += s;
                }
                v
            };
            let (sax, say) = (steps(w, sx), steps(h, sy));
            let mut row = 0i32;
            for y in 0..h {
                let mut col = 0i32;
                for x in 0..w {
                    let (ex, ey) = (sax[x as usize] & 0xffff, say[y as usize] & 0xffff);
                    let get = |xx: i32, yy: i32| self.px[(yy.min(h - 1) * w + xx.min(w - 1)) as usize];
                    let (c00, c01, c10, c11) = (
                        get(col, row),
                        get(col + 1, row),
                        get(col, row + 1),
                        get(col + 1, row + 1),
                    );
                    let p = &mut out.px[(y * w + x) as usize];
                    for c in 0..4 {
                        let (a, b, cc, d) = (c00[c] as i32, c01[c] as i32, c10[c] as i32, c11[c] as i32);
                        let t1 = ((((b - a) * ex) >> 16) + a) & 0xff;
                        let t2 = ((((d - cc) * ex) >> 16) + cc) & 0xff;
                        p[c] = ((((t2 - t1) * ey) >> 16) + t1) as u8;
                    }
                    col += sax[x as usize + 1] >> 16;
                }
                row += say[y as usize + 1] >> 16;
            }
            out
        }
    }

    /// pygame's alpha blend of `s` over `d` (both straight RGBA).
    pub fn blend_over(s: Rgba, d: Rgba) -> Rgba {
        if d[3] == 0 {
            return s;
        }
        let a = s[3] as u32;
        let mut o = [0u8; 4];
        for c in 0..3 {
            o[c] = ((d[c] as u32 * (256 - a) + s[c] as u32 * (a + 1)) >> 8) as u8;
        }
        o[3] = (a + d[3] as u32 - (a * d[3] as u32) / 255) as u8;
        o
    }

    fn packus(v: u16) -> u8 {
        if (v as i16) < 0 { 0 } else { v.min(255) as u8 }
    }

    fn mulhi(a: u16, b: u16) -> u16 {
        ((a as u32 * b as u32) >> 16) as u16
    }

    /// filter_shrink_X_SSE2: area averaging across, 14-bit fixed point.
    fn shrink_x(src: &[Rgba], sw: i32, h: i32, dw: i32) -> Vec<Rgba> {
        let mut out = vec![[0u8; 4]; (dw * h) as usize];
        let xspace = 0x4000 * sw / dw;
        let xrecip = (0x4000_0000 / xspace) as u16;
        for y in 0..h {
            let mut acc = [0u16; 4];
            let mut xcounter = xspace;
            let mut di = 0;
            for x in 0..sw {
                let s = src[(y * sw + x) as usize];
                if xcounter > 0x4000 {
                    for c in 0..4 {
                        acc[c] = acc[c].wrapping_add(s[c] as u16);
                    }
                    xcounter -= 0x4000;
                } else {
                    let xfrac = 0x4000 - xcounter;
                    let mut o = [0u8; 4];
                    for c in 0..4 {
                        let s4 = (s[c] as u16) << 2;
                        let d = mulhi(s4, xcounter as u16).wrapping_add(acc[c]);
                        acc[c] = mulhi(s4, xfrac as u16);
                        o[c] = packus(mulhi(d, xrecip));
                    }
                    if di < dw {
                        out[(y * dw + di) as usize] = o;
                    }
                    di += 1;
                    xcounter = xspace - xfrac;
                }
            }
        }
        out
    }

    /// filter_shrink_Y_SSE2: area averaging down, 14-bit fixed point.
    fn shrink_y(src: &[Rgba], w: i32, sh: i32, dh: i32) -> Vec<Rgba> {
        let mut out = vec![[0u8; 4]; (w * dh) as usize];
        let yspace = 0x4000 * sh / dh;
        let yrecip = (0x4000_0000 / yspace) as u16;
        let mut acc = vec![[0u16; 4]; w as usize];
        let mut ycounter = yspace;
        let mut dy = 0;
        for y in 0..sh {
            if ycounter > 0x4000 {
                for x in 0..w {
                    let s = src[(y * w + x) as usize];
                    for c in 0..4 {
                        acc[x as usize][c] = acc[x as usize][c].wrapping_add(s[c] as u16);
                    }
                }
                ycounter -= 0x4000;
            } else {
                let yfrac = 0x4000 - ycounter;
                for x in 0..w {
                    let s = src[(y * w + x) as usize];
                    let mut o = [0u8; 4];
                    for c in 0..4 {
                        let s4 = (s[c] as u16) << 2;
                        let nd = mulhi(s4, yfrac as u16);
                        let sv = mulhi(s4, ycounter as u16);
                        let old = acc[x as usize][c];
                        acc[x as usize][c] = nd;
                        o[c] = packus(mulhi(sv.wrapping_add(old), yrecip));
                    }
                    if dy < dh {
                        out[(dy * w + x) as usize] = o;
                    }
                }
                dy += 1;
                ycounter = yspace - yfrac;
            }
        }
        out
    }

    /// filter_expand_X_SSE2: bilinear across, 8-bit weights.
    fn expand_x(src: &[Rgba], sw: i32, h: i32, dw: i32) -> Vec<Rgba> {
        let mut out = vec![[0u8; 4]; (dw * h) as usize];
        let f: Vec<(i32, u16, u16)> = (0..dw)
            .map(|x| {
                let m1 = 0x100 * ((x * (sw - 1)) % dw) / dw;
                (x * (sw - 1) / dw, (0x100 - m1) as u16, m1 as u16)
            })
            .collect();
        for y in 0..h {
            for (x, &(i, m0, m1)) in f.iter().enumerate() {
                let a = src[(y * sw + i) as usize];
                let b = src[(y * sw + (i + 1).min(sw - 1)) as usize];
                let o = &mut out[(y * dw) as usize + x];
                for c in 0..4 {
                    let v = (a[c] as u16)
                        .wrapping_mul(m0)
                        .wrapping_add((b[c] as u16).wrapping_mul(m1));
                    o[c] = packus(v >> 8);
                }
            }
        }
        out
    }

    /// filter_expand_Y_SSE2: bilinear down, 8-bit weights.
    fn expand_y(src: &[Rgba], w: i32, sh: i32, dh: i32) -> Vec<Rgba> {
        let mut out = vec![[0u8; 4]; (w * dh) as usize];
        for y in 0..dh {
            let i = y * (sh - 1) / dh;
            let m1 = (0x100 * ((y * (sh - 1)) % dh) / dh) as u16;
            let m0 = 0x100 - m1;
            for x in 0..w {
                let a = src[(i * w + x) as usize];
                let b = src[((i + 1).min(sh - 1) * w + x) as usize];
                let o = &mut out[(y * w + x) as usize];
                for c in 0..4 {
                    let v = (a[c] as u16)
                        .wrapping_mul(m0)
                        .wrapping_add((b[c] as u16).wrapping_mul(m1));
                    o[c] = packus(v >> 8);
                }
            }
        }
        out
    }

    // -- draw.c ---------------------------------------------------------------------------------------------------

    /// drawhorzlineclipbounding: x1..=x2 at y (either order), clipped.
    fn hline<T: Target>(t: &mut T, x1: i32, y: i32, x2: i32, c: Rgba) {
        let (w, h) = t.size();
        if y < 0 || y >= h {
            return;
        }
        let (a, b) = if x2 < x1 { (x2, x1) } else { (x1, x2) };
        let (a, b) = (a.max(0), b.min(w - 1));
        if b < 0 || a >= w {
            return;
        }
        for x in a..=b {
            t.set(x, y, c);
        }
    }

    /// drawvertlineclipbounding: y1..=y2 at x (either order), clipped.
    fn vline<T: Target>(t: &mut T, x: i32, y1: i32, y2: i32, c: Rgba) {
        let (w, h) = t.size();
        if x < 0 || x >= w {
            return;
        }
        let (a, b) = if y2 < y1 { (y2, y1) } else { (y1, y2) };
        let (a, b) = (a.max(0), b.min(h - 1));
        if b < 0 || a >= h {
            return;
        }
        for y in a..=b {
            t.set(x, y, c);
        }
    }

    /// draw_line: a 1 px Bresenham line.
    fn line_thin<T: Target>(t: &mut T, mut x1: i32, mut y1: i32, x2: i32, y2: i32, c: Rgba) {
        if x1 == x2 && y1 == y2 {
            t.set(x1, y1, c);
            return;
        }
        if y1 == y2 {
            let d = if x1 < x2 { 1 } else { -1 };
            for k in 0..=(x1 - x2).abs() {
                t.set(x1 + d * k, y1, c);
            }
            return;
        }
        if x1 == x2 {
            let d = if y1 < y2 { 1 } else { -1 };
            for k in 0..=(y1 - y2).abs() {
                t.set(x1, y1 + d * k, c);
            }
            return;
        }
        let (dx, sx) = ((x2 - x1).abs(), if x1 < x2 { 1 } else { -1 });
        let (dy, sy) = ((y2 - y1).abs(), if y1 < y2 { 1 } else { -1 });
        let mut err = (if dx > dy { dx } else { -dy }) / 2;
        while x1 != x2 || y1 != y2 {
            t.set(x1, y1, c);
            let e2 = err;
            if e2 > -dx {
                err -= dy;
                x1 += sx;
            }
            if e2 < dy {
                err += dx;
                y1 += sy;
            }
        }
        t.set(x2, y2, c);
    }

    /// `pygame.draw.line(surf, c, a, b, width)` (draw_line_width): Bresenham with a span `width` across the minor
    /// axis at every step (so a diagonal line is `width` px wide across, not square to it).
    pub fn line<T: Target>(t: &mut T, c: Rgba, a: (i32, i32), b: (i32, i32), width: i32) {
        let ((mut x1, mut y1), (x2, y2)) = (a, b);
        let (w, h) = t.size();
        let (end_x, end_y) = (w - 1, h - 1);
        if width < 1 {
            return;
        }
        let extra = 1 - (width % 2);
        if width == 1 {
            line_thin(t, x1, y1, x2, y2, c);
            return;
        }
        let width = width / 2;
        let xinc = (x1 - x2).abs() <= (y1 - y2).abs();
        if x1 == x2 && y1 == y2 {
            if y1 >= 0 && y1 <= end_y {
                let (s, e) = ((x1 - width + extra).max(0), end_x.min(x1 + width));
                for x in s..=e {
                    t.set(x, y1, c);
                }
            }
            return;
        }
        let (dx, dy) = ((x2 - x1).abs(), (y2 - y1).abs());
        let (sx, sy) = (if x2 > x1 { 1 } else { -1 }, if y2 > y1 { 1 } else { -1 });
        let mut err = (if dx > dy { dx } else { -dy }) / 2;
        if xinc {
            while y1 != y2 + sy {
                if y1 >= 0 && y1 <= end_y {
                    let (s, e) = ((x1 - width + extra).max(0), end_x.min(x1 + width));
                    for x in s..=e {
                        t.set(x, y1, c);
                    }
                }
                let e2 = err;
                if e2 > -dx {
                    err -= dy;
                    x1 += sx;
                }
                if e2 < dy {
                    err += dx;
                    y1 += sy;
                }
            }
        } else {
            while x1 != x2 + sx {
                if x1 >= 0 && x1 <= end_x {
                    let (s, e) = ((y1 - width + extra).max(0), end_y.min(y1 + width));
                    for y in s..=e {
                        t.set(x1, y, c);
                    }
                }
                let e2 = err;
                if e2 > -dx {
                    err -= dy;
                    x1 += sx;
                }
                if e2 < dy {
                    err += dx;
                    y1 += sy;
                }
            }
        }
    }

    /// `pygame.draw.line` with float points (`int()` of each).
    pub fn line_f<T: Target>(t: &mut T, c: Rgba, a: (f64, f64), b: (f64, f64), width: i32) {
        line(t, c, (ti(a.0), ti(a.1)), (ti(b.0), ti(b.1)), width);
    }

    /// `pygame.draw.lines(surf, c, closed, points, width)`.
    pub fn lines<T: Target>(t: &mut T, c: Rgba, closed: bool, pts: &[(f64, f64)], width: i32) {
        let p: Vec<(i32, i32)> = pts.iter().map(|&(x, y)| (ti(x), ti(y))).collect();
        if p.len() < 2 || width < 1 {
            return;
        }
        for k in 1..p.len() {
            line(t, c, p[k - 1], p[k], width);
        }
        if closed && p.len() > 2 {
            line(t, c, p[p.len() - 1], p[0], width);
        }
    }

    /// draw_circle_filled: spans from x0 - x to x0 + x - 1 (the circle centred on the corner at (x0, y0)).
    fn circle_filled<T: Target>(t: &mut T, x0: i32, y0: i32, radius: i32, c: Rgba) {
        let (mut f, mut ddf_x, mut ddf_y) = (1 - radius, 0, -2 * radius);
        let (mut x, mut y) = (0, radius);
        while x < y {
            if f >= 0 {
                y -= 1;
                ddf_y += 2;
                f += ddf_y;
            }
            x += 1;
            ddf_x += 2;
            f += ddf_x + 1;
            if f >= 0 {
                hline(t, x0 - x, y0 + y - 1, x0 + x - 1, c);
                hline(t, x0 - x, y0 - y, x0 + x - 1, c);
            }
            hline(t, x0 - y, y0 + x - 1, x0 + y - 1, c);
            hline(t, x0 - y, y0 - x, x0 + y - 1, c);
        }
    }

    /// draw_circle_bresenham_thin: a 1 px ring.
    fn circle_thin<T: Target>(t: &mut T, x0: i32, y0: i32, radius: i32, c: Rgba) {
        let (mut f, mut ddf_x, mut ddf_y) = (1 - radius, 0, -2 * radius);
        let (mut x, mut y) = (0, radius);
        while x < y {
            if f >= 0 {
                y -= 1;
                ddf_y += 2;
                f += ddf_y;
            }
            x += 1;
            ddf_x += 2;
            f += ddf_x + 1;
            for (px, py) in [
                (x0 + x - 1, y0 + y - 1),
                (x0 - x, y0 + y - 1),
                (x0 + x - 1, y0 - y),
                (x0 - x, y0 - y),
                (x0 + y - 1, y0 + x - 1),
                (x0 + y - 1, y0 - x),
                (x0 - y, y0 + x - 1),
                (x0 - y, y0 - x),
            ] {
                t.set(px, py, c);
            }
        }
    }

    /// draw_circle_bresenham: a ring `thickness` px wide inside `radius`.
    fn circle_ring<T: Target>(t: &mut T, x0: i32, y0: i32, radius: i32, thickness: i32, c: Rgba) {
        let (mut x, mut y): (i64, i64) = (0, radius as i64);
        let r2 = (radius * radius) as i64;
        let dr2 = 2 * r2;
        let mut d1 = r2 as f64 * (1.25 - radius as f64);
        let (mut dx, mut dy): (i64, i64) = (0, dr2 * y);
        let mut line = true;
        let ri = radius as i64 - thickness as i64 + 1;
        let (mut xi, mut yi): (i64, i64) = (0, ri);
        let ri2 = ri * ri;
        let dri2 = 2 * ri2;
        let mut d1i = ri2 as f64 * (1.25 - ri as f64);
        let mut d2i = 0f64;
        let (mut dxi, mut dyi): (i64, i64) = (0, dri2 * yi);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let spans = |t: &mut T, x: i64, y: i64, xi: i64, line: bool| {
            let h = |t: &mut T, a: i64, yy: i64, b: i64| hline(t, a as i32, yy as i32, b as i32, c);
            if line {
                h(t, x0 - x, y0 - y, x0 + x - 1);
                h(t, x0 - x, y0 + y - 1, x0 + x - 1);
            } else {
                h(t, x0 - x, y0 - y, x0 - xi);
                h(t, x0 - x, y0 + y - 1, x0 - xi);
                h(t, x0 + xi - 1, y0 - y, x0 + x - 1);
                h(t, x0 + xi - 1, y0 + y - 1, x0 + x - 1);
            }
        };
        while dx < dy {
            while d1 < 0.0 {
                x += 1;
                dx += dr2;
                d1 += (dx + r2) as f64;
            }
            spans(t, x, y, xi, line);
            x += 1;
            y -= 1;
            dx += dr2;
            dy -= dr2;
            d1 += (dx - dy + r2) as f64;
            if line && y < ri {
                line = false;
            }
            if !line {
                while d1i < 0.0 {
                    xi += 1;
                    dxi += dri2;
                    d1i += (dxi + ri2) as f64;
                }
                xi += 1;
                yi -= 1;
                dxi += dri2;
                dyi -= dri2;
                d1i += (dxi - dyi + ri2) as f64;
            }
        }
        d1 = r2 as f64 * ((x as f64 + 0.5) * (x as f64 + 0.5) + ((y - 1) * (y - 1)) as f64 - r2 as f64);
        while y >= 0 {
            spans(t, x, y, xi, line);
            if d1 > 0.0 {
                y -= 1;
                dy -= dr2;
                d1 += (r2 - dy) as f64;
            } else {
                y -= 1;
                x += 1;
                dx += dr2;
                dy -= dr2;
                d1 += (dx - dy + r2) as f64;
            }
            if line && y < ri {
                line = false;
            }
            if !line {
                if dxi < dyi {
                    while d1i < 0.0 {
                        xi += 1;
                        dxi += dri2;
                        d1i += (dxi + ri2) as f64;
                    }
                    xi += 1;
                    yi -= 1;
                    dxi += dri2;
                    dyi -= dri2;
                    d1i += (dxi - dyi + ri2) as f64;
                } else {
                    if d2i == 0.0 {
                        d2i = ri2 as f64
                            * ((xi as f64 + 0.5) * (xi as f64 + 0.5) + ((yi - 1) * (yi - 1)) as f64
                                - ri2 as f64);
                    }
                    if d2i > 0.0 {
                        yi -= 1;
                        dyi -= dri2;
                        d2i += (ri2 - dyi) as f64;
                    } else {
                        yi -= 1;
                        xi += 1;
                        dxi += dri2;
                        dyi -= dri2;
                        d2i += (dxi - dyi + ri2) as f64;
                    }
                }
            }
        }
    }

    /// `pygame.draw.circle(surf, c, center, radius, width)`: centre and radius taken as `int()`.
    pub fn circle<T: Target>(t: &mut T, c: Rgba, center: (f64, f64), radius: f64, width: i32) {
        let (x0, y0, radius) = (ti(center.0), ti(center.1), ti(radius));
        if radius < 1 || width < 0 {
            return;
        }
        let width = width.min(radius);
        if width == 0 || width == radius {
            circle_filled(t, x0, y0, radius, c);
        } else if width == 1 {
            circle_thin(t, x0, y0, radius, c);
        } else {
            circle_ring(t, x0, y0, radius, width, c);
        }
    }

    /// draw_circle_quadrant (filled with thickness 0, else a ring that wide), for rounded rects.
    fn circle_quadrant<T: Target>(
        t: &mut T,
        x0: i32,
        y0: i32,
        radius: i32,
        thickness: i32,
        c: Rgba,
        q: [bool; 4],
    ) {
        let [tr, tl, bl, br] = q;
        let (mut f, mut ddf_x, mut ddf_y) = (1 - radius, 0, -2 * radius);
        let (mut x, mut y) = (0, radius);
        let mut i_y = radius - thickness;
        let (mut i_f, mut i_ddf_x, mut i_ddf_y) = (1 - i_y, 0, -2 * i_y);
        if radius == 1 {
            if tr {
                t.set(x0, y0 - 1, c);
            }
            if tl {
                t.set(x0 - 1, y0 - 1, c);
            }
            if bl {
                t.set(x0 - 1, y0, c);
            }
            if br {
                t.set(x0, y0, c);
            }
            return;
        }
        let mut thickness = thickness;
        if thickness != 0 {
            while x < y {
                if f >= 0 {
                    y -= 1;
                    ddf_y += 2;
                    f += ddf_y;
                }
                if i_f >= 0 {
                    i_y -= 1;
                    i_ddf_y += 2;
                    i_f += i_ddf_y;
                }
                x += 1;
                ddf_x += 2;
                f += ddf_x + 1;
                i_ddf_x += 2;
                i_f += i_ddf_x + 1;
                if thickness > 1 {
                    thickness = y - i_y;
                }
                for i in 0..thickness {
                    let y1 = y - i;
                    if tr {
                        if (y0 - y1) < (y0 - x) {
                            t.set(x0 + x - 1, y0 - y1, c);
                        }
                        if (x0 + y1 - 1) >= (x0 + x - 1) {
                            t.set(x0 + y1 - 1, y0 - x, c);
                        }
                    }
                }
                for i in 0..thickness {
                    let y1 = y - i;
                    if tl {
                        if (y0 - y1) <= (y0 - x) {
                            t.set(x0 - x, y0 - y1, c);
                        }
                        if (x0 - y1) < (x0 - x) {
                            t.set(x0 - y1, y0 - x, c);
                        }
                    }
                }
                for i in 0..thickness {
                    let y1 = y - i;
                    if bl {
                        if (x0 - y1) <= (x0 - x) {
                            t.set(x0 - y1, y0 + x - 1, c);
                        }
                        if (y0 + y1 - 1) > (y0 + x - 1) {
                            t.set(x0 - x, y0 + y1 - 1, c);
                        }
                    }
                }
                for i in 0..thickness {
                    let y1 = y - i;
                    if br {
                        if (y0 + y1 - 1) >= (y0 + x - 1) {
                            t.set(x0 + x - 1, y0 + y1 - 1, c);
                        }
                        if (x0 + y1 - 1) > (x0 + x - 1) {
                            t.set(x0 + y1 - 1, y0 + x - 1, c);
                        }
                    }
                }
            }
        } else {
            while x < y {
                if f >= 0 {
                    y -= 1;
                    ddf_y += 2;
                    f += ddf_y;
                }
                x += 1;
                ddf_x += 2;
                f += ddf_x + 1;
                if tr {
                    for y1 in (y0 - x)..=y0 {
                        t.set(x0 + y - 1, y1, c);
                    }
                    for y1 in (y0 - y)..=y0 {
                        t.set(x0 + x - 1, y1, c);
                    }
                }
                if tl {
                    for y1 in (y0 - x)..=y0 {
                        t.set(x0 - y, y1, c);
                    }
                    for y1 in (y0 - y)..=y0 {
                        t.set(x0 - x, y1, c);
                    }
                }
                if bl {
                    for y1 in y0..(y0 + x) {
                        t.set(x0 - y, y1, c);
                    }
                    for y1 in y0..(y0 + y) {
                        t.set(x0 - x, y1, c);
                    }
                }
                if br {
                    for y1 in y0..(y0 + x) {
                        t.set(x0 + y - 1, y1, c);
                    }
                    for y1 in y0..(y0 + y) {
                        t.set(x0 + x - 1, y1, c);
                    }
                }
            }
        }
    }

    /// draw_eight_symetric_pixels: the point (x, y) mirrored into the chosen quadrants, blended by opacity / 255.
    fn eight<T: Target>(t: &mut T, x0: i32, y0: i32, c: Rgba, x: i32, y: i32, opacity: f32, q: [bool; 4]) {
        let b = opacity / 255.0;
        let [tr, tl, bl, br] = q;
        if tr {
            t.blend(x0 + x, y0 - y, c, b);
            t.blend(x0 + y, y0 - x, c, b);
        }
        if tl {
            t.blend(x0 - x, y0 - y, c, b);
            t.blend(x0 - y, y0 - x, c, b);
        }
        if bl {
            t.blend(x0 - x, y0 + y, c, b);
            t.blend(x0 - y, y0 + x, c, b);
        }
        if br {
            t.blend(x0 + x, y0 + y, c, b);
            t.blend(x0 + y, y0 + x, c, b);
        }
    }

    /// draw_circle_xiaolinwu: anti-aliased rings from radius - thickness to radius.
    fn wu<T: Target>(t: &mut T, x0: i32, y0: i32, radius: i32, thickness: i32, c: Rgba, q: [bool; 4]) {
        for layer in (radius - thickness)..=radius {
            let (mut x, mut y) = (0i32, layer);
            let pr = (layer as f64).powi(2);
            let mut prev = 0f64;
            while x < y {
                let height = (pr - (x as f64).powi(2)).sqrt();
                let opacity = 255.0 * (height.ceil() - height);
                if opacity < prev {
                    y -= 1;
                }
                prev = opacity;
                if layer == radius - thickness {
                    eight(t, x0, y0, c, x, y, 255.0, q);
                    eight(t, x0, y0, c, x, y - 1, opacity as f32, q);
                } else if layer == radius {
                    eight(t, x0, y0, c, x, y, 255.0 - opacity as f32, q);
                    eight(t, x0, y0, c, x, y - 1, 255.0, q);
                } else {
                    eight(t, x0, y0, c, x, y, 255.0, q);
                    eight(t, x0, y0, c, x, y - 1, 255.0, q);
                }
                x += 1;
            }
        }
    }

    /// draw_circle_xiaolinwu_thin.
    fn wu_thin<T: Target>(t: &mut T, x0: i32, y0: i32, radius: i32, c: Rgba, q: [bool; 4]) {
        let (mut x, mut y) = (0i32, radius);
        let pr = (radius as f64).powi(2);
        let mut prev = 0f64;
        while x < y {
            let height = (pr - (x as f64).powi(2)).sqrt();
            let opacity = 255.0 * (height.ceil() - height);
            if opacity < prev {
                y -= 1;
            }
            prev = opacity;
            eight(t, x0, y0, c, x, y, 255.0 - opacity as f32, q);
            eight(t, x0, y0, c, x, y - 1, opacity as f32, q);
            x += 1;
        }
    }

    /// `pygame.draw.aacircle(surf, c, center, radius, width, draw_top_right=.., ...)`: centre and radius taken as
    /// `int()`; `quads` (top right, top left, bottom left, bottom right) for only some quarters.
    pub fn aacircle<T: Target>(
        t: &mut T,
        c: Rgba,
        center: (f64, f64),
        radius: f64,
        width: i32,
        quads: Option<[bool; 4]>,
    ) {
        let (x0, y0, radius) = (ti(center.0), ti(center.1), ti(radius));
        if radius < 1 || width < 0 {
            return;
        }
        let width = width.min(radius);
        match quads {
            None => {
                let all = [true; 4];
                if width == 0 || width == radius {
                    circle_filled(t, x0, y0, radius - 1, c);
                    wu(t, x0, y0, radius, 2, c, all);
                } else if width == 1 {
                    wu_thin(t, x0, y0, radius, c, all);
                } else {
                    wu(t, x0, y0, radius, width - 1, c, all);
                }
            }
            Some(q) => {
                if width == 0 || width == radius {
                    wu(t, x0, y0, radius, radius, c, q);
                } else if width == 1 {
                    wu_thin(t, x0, y0, radius, c, q);
                } else {
                    wu(t, x0, y0, radius, width - 1, c, q);
                }
            }
        }
    }

    /// draw_filltri.
    fn filltri<T: Target>(t: &mut T, p: &[(i32, i32)], c: Rgba) {
        let (mut p0, mut p1, mut p2) = (p[0], p[1], p[2]);
        if p1.1 < p0.1 {
            std::mem::swap(&mut p1, &mut p0);
        }
        if p2.1 < p1.1 {
            std::mem::swap(&mut p1, &mut p2);
            if p1.1 < p0.1 {
                std::mem::swap(&mut p1, &mut p0);
            }
        }
        if p0.1 == p1.1 && p1.1 == p2.1 && p0.0 == p1.0 && p1.0 != p2.0 {
            std::mem::swap(&mut p1, &mut p2);
        }
        let d = |a: (i32, i32), b: (i32, i32)| ((b.0 - a.0) as f64 / ((b.1 - a.1) as f64 + 1e-17)) as f32;
        let (d1, d2, d3) = (d(p0, p2), d(p0, p1), d(p1, p2));
        for y in p0.1..=p2.1 {
            let x1 = p0.0 + ((y - p0.1) as f32 * d1) as i32;
            let x2 = if y < p1.1 {
                p0.0 + ((y - p0.1) as f32 * d2) as i32
            } else {
                p1.0 + ((y - p1.1) as f32 * d3) as i32
            };
            hline(t, x1, y, x2, c);
        }
    }

    /// draw_fillpoly: scanlines between sorted edge crossings.
    fn fillpoly<T: Target>(t: &mut T, p: &[(i32, i32)], c: Rgba) {
        let n = p.len();
        let miny = p.iter().map(|q| q.1).min().unwrap_or(0);
        let maxy = p.iter().map(|q| q.1).max().unwrap_or(0);
        if miny == maxy {
            let minx = p.iter().map(|q| q.0).min().unwrap_or(0);
            let maxx = p.iter().map(|q| q.0).max().unwrap_or(0);
            hline(t, minx, miny, maxx, c);
            return;
        }
        let mut xs: Vec<i32> = Vec::with_capacity(n);
        for y in miny..=maxy {
            xs.clear();
            for i in 0..n {
                let ip = if i > 0 { i - 1 } else { n - 1 };
                let (y1, y2, x1, x2) = if p[ip].1 < p[i].1 {
                    (p[ip].1, p[i].1, p[ip].0, p[i].0)
                } else if p[ip].1 > p[i].1 {
                    (p[i].1, p[ip].1, p[i].0, p[ip].0)
                } else {
                    continue;
                };
                if (y >= y1 && y < y2) || (y == maxy && y2 == maxy) {
                    let inter = ((y - y1) * (x2 - x1)) as f32 / (y2 - y1) as f32;
                    let inter = if xs.len().is_multiple_of(2) {
                        inter.floor()
                    } else {
                        inter.ceil()
                    };
                    xs.push(inter as i32 + x1);
                }
            }
            xs.sort_unstable();
            let mut k = 0;
            while k + 1 < xs.len() {
                hline(t, xs[k], y, xs[k + 1], c);
                k += 2;
            }
        }
        for i in 0..n {
            let ip = if i > 0 { i - 1 } else { n - 1 };
            let y = p[i].1;
            if miny < y && p[ip].1 == y && y < maxy {
                hline(t, p[i].0, y, p[ip].0, c);
            }
        }
    }

    /// `pygame.draw.polygon(surf, c, points, width)`: points taken as `int()`; with a width, its outline.
    pub fn polygon<T: Target>(t: &mut T, c: Rgba, pts: &[(f64, f64)], width: i32) {
        if width != 0 {
            lines(t, c, true, pts, width);
            return;
        }
        let p: Vec<(i32, i32)> = pts.iter().map(|&(x, y)| (ti(x), ti(y))).collect();
        match p.len() {
            0..=2 => {}
            3 => filltri(t, &p, c),
            _ => fillpoly(t, &p, c),
        }
    }

    /// draw_round_rect (x2, y2 inclusive).
    fn round_rect<T: Target>(
        t: &mut T,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        radius: i32,
        width: i32,
        c: Rgba,
    ) {
        let (mut tl, mut tr, mut bl, mut br) = (radius, radius, radius, radius);
        if (tl + tr) > (x2 - x1 + 1)
            || (bl + br) > (x2 - x1 + 1)
            || (tl + bl) > (y2 - y1 + 1)
            || (tr + br) > (y2 - y1 + 1)
        {
            let q_top = (x2 - x1 + 1) as f32 / (tl + tr) as f32;
            let q_left = (y2 - y1 + 1) as f32 / (tl + bl) as f32;
            let q_bottom = (x2 - x1 + 1) as f32 / (bl + br) as f32;
            let q_right = (y2 - y1 + 1) as f32 / (tr + br) as f32;
            let f = q_top.min(q_left).min(q_bottom).min(q_right);
            tl = (tl as f32 * f) as i32;
            tr = (tr as f32 * f) as i32;
            bl = (bl as f32 * f) as i32;
            br = (br as f32 * f) as i32;
        }
        if width == 0 {
            let xs = [x1, x1 + tl, x2 - tr, x2, x2, x2 - br, x1 + bl, x1];
            let ys = [y1 + tl, y1, y1, y1 + tr, y2 - br, y2, y2, y2 - bl];
            let pts: Vec<(i32, i32)> = xs.iter().zip(ys.iter()).map(|(&x, &y)| (x, y)).collect();
            fillpoly(t, &pts, c);
            circle_quadrant(t, x2 - tr + 1, y1 + tr, tr, 0, c, [true, false, false, false]);
            circle_quadrant(t, x1 + tl, y1 + tl, tl, 0, c, [false, true, false, false]);
            circle_quadrant(t, x1 + bl, y2 - bl + 1, bl, 0, c, [false, false, true, false]);
            circle_quadrant(t, x2 - br + 1, y2 - br + 1, br, 0, c, [false, false, false, true]);
        } else {
            if x2 - tr == x1 + tl {
                for i in 0..width {
                    t.set(x1 + tl, y1 + i, c);
                }
            } else {
                let yy = y1 + width / 2 - 1 + width % 2;
                line(t, c, (x1 + tl, yy), (x2 - tr, yy), width);
            }
            if y2 - bl == y1 + tl {
                for i in 0..width {
                    t.set(x1 + i, y1 + tl, c);
                }
            } else {
                let xx = x1 + width / 2 - 1 + width % 2;
                line(t, c, (xx, y1 + tl), (xx, y2 - bl), width);
            }
            if x2 - br == x1 + bl {
                for i in 0..width {
                    t.set(x1 + bl, y2 - i, c);
                }
            } else {
                let yy = y2 - width / 2;
                line(t, c, (x1 + bl, yy), (x2 - br, yy), width);
            }
            if y2 - br == y1 + tr {
                for i in 0..width {
                    t.set(x2 - i, y1 + tr, c);
                }
            } else {
                let xx = x2 - width / 2;
                line(t, c, (xx, y1 + tr), (xx, y2 - br), width);
            }
            circle_quadrant(t, x2 - tr + 1, y1 + tr, tr, width, c, [true, false, false, false]);
            circle_quadrant(t, x1 + tl, y1 + tl, tl, width, c, [false, true, false, false]);
            circle_quadrant(t, x1 + bl, y2 - bl + 1, bl, width, c, [false, false, true, false]);
            circle_quadrant(
                t,
                x2 - br + 1,
                y2 - br + 1,
                br,
                width,
                c,
                [false, false, false, true],
            );
        }
    }

    /// `pygame.draw.rect(surf, c, (x, y, w, h), width, border_radius=radius)`: the rect taken as `int()` of each.
    pub fn rect<T: Target>(t: &mut T, c: Rgba, r: (f64, f64, f64, f64), width: i32, radius: i32) {
        let (x, y, w, h) = (ti(r.0), ti(r.1), ti(r.2), ti(r.3));
        if width < 0 {
            return;
        }
        if radius <= 0 || w.abs() < 2 || h.abs() < 2 {
            let (sw, sh) = t.size();
            let (cx0, cy0) = (x.max(0), y.max(0));
            let (cx1, cy1) = ((x + w).min(sw), (y + h).min(sh));
            if cx1 <= cx0 || cy1 <= cy0 {
                return;
            }
            if width > 0 && width * 2 < cx1 - cx0 && width * 2 < cy1 - cy0 {
                let (x2, y2) = (x + w - 1, y + h - 1);
                for i in 0..width {
                    hline(t, x, y + i, x2, c);
                    hline(t, x, y2 - i, x2, c);
                }
                for i in 0..((y2 - y) - 2 * width + 1) {
                    hline(t, x, y + width + i, x + width - 1, c);
                    hline(t, x2 - width + 1, y + width + i, x2, c);
                }
            } else {
                for yy in cy0..cy1 {
                    for xx in cx0..cx1 {
                        t.set(xx, yy, c);
                    }
                }
            }
            return;
        }
        let (mut x, mut y, mut w, mut h) = (x, y, w, h);
        if w < 0 {
            x += w;
            w = -w;
        }
        if h < 0 {
            y += h;
            h = -h;
        }
        let mut width = width;
        if width > w / 2 || width > h / 2 {
            width = (w / 2).max(h / 2);
        }
        round_rect(t, x, y, x + w - 1, y + h - 1, radius, width, c);
    }

    /// `pygame.draw.ellipse(surf, c, rect)` (filled): draw_ellipse_filled, the rect taken as `int()` of each.
    pub fn ellipse<T: Target>(t: &mut T, c: Rgba, r: (f64, f64, f64, f64)) {
        let (x0, y0, width, height) = (ti(r.0), ti(r.1), ti(r.2), ti(r.3));
        if width <= 0 || height <= 0 {
            return;
        }
        if width == 1 {
            line_thin(t, x0, y0, x0, y0 + height - 1, c);
            return;
        }
        if height == 1 {
            hline(t, x0, y0, x0 + width - 1, c);
            return;
        }
        let x0 = x0 + width / 2;
        let y0 = y0 + height / 2;
        let x_off = (width + 1) % 2;
        let y_off = (height + 1) % 2;
        let (w, h) = ((width / 2) as i64, (height / 2) as i64);
        let (mut x, mut y): (i64, i64) = (0, h);
        let mut d1 = (h * h - w * w * h) as f64 + 0.25 * (w * w) as f64;
        let mut dx = 2 * h * h * x;
        let mut dy = 2 * w * w * y;
        let (x0, y0, x_off, y_off) = (x0 as i64, y0 as i64, x_off as i64, y_off as i64);
        let two = |t: &mut T, x: i64, y: i64| {
            hline(t, (x0 - x) as i32, (y0 - y) as i32, (x0 + x - x_off) as i32, c);
            hline(
                t,
                (x0 - x) as i32,
                (y0 + y - y_off) as i32,
                (x0 + x - x_off) as i32,
                c,
            );
        };
        while dx < dy {
            two(t, x, y);
            if d1 < 0.0 {
                x += 1;
                dx += 2 * h * h;
                d1 += (dx + h * h) as f64;
            } else {
                x += 1;
                y -= 1;
                dx += 2 * h * h;
                dy -= 2 * w * w;
                d1 += (dx - dy + h * h) as f64;
            }
        }
        let mut d2 = (h * h) as f64 * ((x as f64 + 0.5) * (x as f64 + 0.5))
            + (w * w) as f64 * ((y - 1) * (y - 1)) as f64
            - (w * w * h * h) as f64;
        while y >= 0 {
            two(t, x, y);
            if d2 > 0.0 {
                y -= 1;
                dy -= 2 * w * w;
                d2 += (w * w - dy) as f64;
            } else {
                y -= 1;
                x += 1;
                dx += 2 * h * h;
                dy -= 2 * w * w;
                d2 += (dx - dy + w * w) as f64;
            }
        }
    }

    /// check_pixel_in_arc.
    #[allow(clippy::too_many_arguments)]
    fn in_arc(
        x: i32,
        y: i32,
        min_dot: f64,
        ir1: f64,
        ir2: f64,
        iir1: f64,
        iir2: f64,
        xm: f64,
        ym: f64,
    ) -> bool {
        let (xf, yf) = (x as f64, y as f64);
        if xf * xf * ir1 + yf * yf * ir2 > 1.0 {
            return false;
        }
        if xf * xf * iir1 + yf * yf * iir2 < 1.0 {
            return false;
        }
        xf * xm + yf * ym >= min_dot * (xf * xf + yf * yf).sqrt()
    }

    /// `pygame.draw.arc(surf, c, rect, start, stop, width)` (radians, counter-clockwise from 3 o'clock).
    pub fn arc<T: Target>(t: &mut T, c: Rgba, r: (f64, f64, f64, f64), a0: f64, a1: f64, width: i32) {
        let (rx, ry, rw, rh) = (ti(r.0), ti(r.1), ti(r.2), ti(r.3));
        if width < 0 {
            return;
        }
        let mut width = width;
        if width > rw / 2 || width > rh / 2 {
            width = (rw / 2).max(rh / 2);
        }
        let mut a1 = a1;
        if a1 < a0 {
            a1 += 2.0 * std::f64::consts::PI;
        }
        width = width.min(rw.min(rh) / 2);
        let (xc, yc, r1, r2) = (rx + rw / 2, ry + rh / 2, rw / 2, rh / 2);
        if width <= 0 {
            return;
        }
        // (draw_arc checks the angles again)
        if a1 < a0 {
            a1 += 2.0 * std::f64::consts::PI;
        }
        if a1 <= a0 {
            return;
        }
        let am = 0.5 * (a0 + a1);
        let ad = am - a0;
        let (xm, ym) = (am.cos(), -am.sin());
        let (ir1_i, ir2_i) = (r1 - width, r2 - width);
        let ir1 = 1.0 / (r1 * r1) as f64;
        let ir2 = 1.0 / (r2 * r2) as f64;
        let iir1 = 1.0 / (ir1_i * ir1_i) as f64;
        let iir2 = 1.0 / (ir2_i * ir2_i) as f64;
        let min_dot = if ad < std::f64::consts::PI {
            (am - a0).cos()
        } else {
            -1.0
        };
        // calc_arc_bounds
        let (sw, sh) = t.size();
        let (xs, ys) = (a0.cos(), -a0.sin());
        let (xe, ye) = (a1.cos(), -a1.sin());
        let rnd = |v: f64| (v + 0.5) as i32;
        let (xsi, ysi, xei, yei) = (
            rnd(xs * ir1_i as f64),
            rnd(ys * ir2_i as f64),
            rnd(xe * ir1_i as f64),
            rnd(ye * ir2_i as f64),
        );
        let (xso, yso, xeo, yeo) = (
            rnd(xs * r1 as f64),
            rnd(ys * r2 as f64),
            rnd(xe * r1 as f64),
            rnd(ye * r2 as f64),
        );
        let mut minx = -r1;
        if -xm < min_dot {
            minx = xsi.min(xei).min(xso.min(xeo));
        }
        minx = minx.max(-xc);
        let mut miny = -r2;
        if -ym < min_dot {
            miny = ysi.min(yei).min(yso.min(yeo));
        }
        miny = miny.max(-yc);
        let mut maxx = r1;
        if xm < min_dot {
            maxx = xsi.max(xei).max(xso.max(xeo));
        }
        maxx = maxx.min(sw - xc - 1);
        let mut maxy = r2;
        if ym < min_dot {
            maxy = ysi.max(yei).max(yso.max(yeo));
        }
        maxy = maxy.min(sh - yc - 1);
        if minx >= maxx || miny >= maxy {
            return;
        }
        let hit = |x: i32, y: i32| in_arc(x, y, min_dot, ir1, ir2, iir1, iir2, xm, ym);
        loop {
            if miny >= maxy {
                return;
            }
            if (minx..=maxx).any(|x| hit(x, miny)) {
                break;
            }
            miny += 1;
        }
        loop {
            if maxy <= miny {
                return;
            }
            if (minx..=maxx).any(|x| hit(x, maxy)) {
                break;
            }
            maxy -= 1;
        }
        loop {
            if minx >= maxx {
                return;
            }
            if (miny..=maxy).any(|y| hit(minx, y)) {
                break;
            }
            minx += 1;
        }
        loop {
            if minx >= maxx {
                return;
            }
            if (miny..=maxy).any(|y| hit(maxx, y)) {
                break;
            }
            maxx -= 1;
        }
        // draw_arc's scan
        let max_y = maxy.max(-miny);
        for y in 0..=max_y {
            let pos_y = y >= miny && y <= maxy;
            let neg_y = -y >= miny && -y <= maxy;
            let y2 = y * y;
            let x_outer = (r1 as f64 * (1.0 - y2 as f64 * ir2).sqrt()) as i32;
            let x_inner = if y < ir2_i {
                (ir1_i as f64 * (1.0 - y2 as f64 * iir2).sqrt()) as i32
            } else {
                0
            };
            let (py, ny) = (yc + y, yc - y);
            let y_dot = y as f64 * ym;
            for x in x_inner..=x_outer {
                let pos_x = x >= minx && x <= maxx;
                let neg_x = -x >= minx && -x <= maxx;
                if !(pos_x || neg_x) {
                    continue;
                }
                let (px_, nx) = (xc + x, xc - x);
                let cmp = min_dot * ((x * x + y2) as f64).sqrt();
                let x_dot = x as f64 * xm;
                if pos_y && pos_x && x_dot + y_dot >= cmp {
                    t.set(px_, py, c);
                }
                if pos_y && neg_x && -x_dot + y_dot >= cmp {
                    t.set(nx, py, c);
                }
                if neg_y && pos_x && x_dot - y_dot >= cmp {
                    t.set(px_, ny, c);
                }
                if neg_y && neg_x && -x_dot - y_dot >= cmp {
                    t.set(nx, ny, c);
                }
            }
        }
    }

    /// draw_aaline (1 px).
    fn aaline1<T: Target>(t: &mut T, c: Rgba, mut fx: f32, mut fy: f32, mut tx: f32, mut ty: f32) {
        let (sw, sh) = t.size();
        let mut dx = tx - fx;
        let mut dy = ty - fy;
        if dx.abs() < 0.0001 && dy.abs() < 0.0001 {
            t.blend((fx + 0.5).floor() as i32, (fy + 0.5).floor() as i32, c, 1.0);
            return;
        }
        let mut cl = -1.0f32;
        let mut cr = cl + sw as f32 + 1.0;
        let mut ct = -1.0f32;
        let mut cb = ct + sh as f32 + 1.0;
        let steep = dx.abs() < dy.abs();
        if steep {
            std::mem::swap(&mut fx, &mut fy);
            std::mem::swap(&mut tx, &mut ty);
            std::mem::swap(&mut dx, &mut dy);
            std::mem::swap(&mut cl, &mut ct);
            std::mem::swap(&mut cr, &mut cb);
        }
        if dx < 0.0 {
            std::mem::swap(&mut fx, &mut tx);
            std::mem::swap(&mut fy, &mut ty);
            dx = -dx;
            dy = -dy;
        }
        if tx <= cl || fx >= cr {
            return;
        }
        let g = dy / dx;
        if fx < cl {
            fy += g * (cl - fx);
            fx = cl;
        }
        if tx > cr {
            ty += g * (cr - tx);
            tx = cr;
        }
        if g > 0.0 {
            if ty <= ct || fy >= cb {
                return;
            }
            if fy < ct {
                fx += (ct - fy) / g;
                fy = ct;
            }
            if ty > cb {
                tx += (cb - ty) / g;
                ty = cb;
            }
        } else {
            if fy <= ct || ty >= cb {
                return;
            }
            if ty < ct {
                tx += (ct - ty) / g;
                ty = ct;
            }
            if fy > cb {
                fx += (cb - fy) / g;
                fy = cb;
            }
        }
        fy += 1.0;
        ty += 1.0;
        let put = |t: &mut T, a: i32, b: i32, br: f32| {
            if steep {
                t.blend(b, a, c, br)
            } else {
                t.blend(a, b, c, br)
            }
        };
        // first endpoint
        let mut xs = fx as i32;
        let ye = fy + g * (xs as f32 - fx);
        let mut iy = ye;
        if tx > cl + 1.0 {
            let gap = 1.0 + xs as f32 - fx;
            let b = ye - (ye as i32) as f32;
            let (a, mut bb) = (xs, ye as i32);
            if ((ye as i32) as f32) < ye {
                put(t, a, bb, b * gap);
            }
            bb -= 1;
            put(t, a, bb, (1.0 - b) * gap);
            iy += g;
            xs += 1;
        }
        // second endpoint
        let xe = tx.ceil() as i32;
        if fx < cr - 1.0 {
            let ye2 = ty + g * (xe as f32 - tx);
            let gap = 1.0 - xe as f32 + tx;
            let b = ye2 - (ye2 as i32) as f32;
            let (a, mut bb) = (xe, ye2 as i32);
            if ((ye2 as i32) as f32) < ye2 {
                put(t, a, bb, b * gap);
            }
            bb -= 1;
            put(t, a, bb, (1.0 - b) * gap);
        }
        for x in xs..xe {
            let y = iy as i32;
            let b = 1.0 - iy + y as f32;
            put(t, x, y - 1, b);
            if (y as f32) < iy {
                put(t, x, y, 1.0 - b);
            }
            iy += g;
        }
    }

    /// `pygame.draw.aaline(surf, c, a, b, width)`: points as f32 (pg_TwoFloatsFromObj).
    pub fn aaline<T: Target>(t: &mut T, c: Rgba, a: (f32, f32), b: (f32, f32), width: i32) {
        if width <= 1 {
            aaline1(t, c, a.0, a.1, b.0, b.1);
            return;
        }
        let (sw, sh) = t.size();
        let extra = 1 - (width % 2);
        let width = width / 2;
        let (mut fx, mut fy, mut tx, mut ty) = (a.0, a.1, b.0, b.1);
        let mut dx = tx - fx;
        let mut dy = ty - fy;
        let steep = dx.abs() < dy.abs();
        if dx.abs() < 0.0001 && dy.abs() < 0.0001 {
            let x = (fx + 0.5).floor() as i32;
            let y = (fy + 0.5).floor() as i32;
            t.blend(x, y, c, 1.0);
            if dx != 0.0 && dy != 0.0 {
                if steep {
                    hline(t, x - width + extra, y, x + width - 1, c);
                } else {
                    vline(t, x, y - width + extra, y + width - 1, c);
                }
            }
            return;
        }
        let mut cl = -1.0f32;
        let mut cr = cl + sw as f32 + 1.0;
        let mut ct = -1.0f32;
        let mut cb = ct + sh as f32 + 1.0;
        if steep {
            std::mem::swap(&mut fx, &mut fy);
            std::mem::swap(&mut tx, &mut ty);
            std::mem::swap(&mut dx, &mut dy);
            std::mem::swap(&mut cl, &mut ct);
            std::mem::swap(&mut cr, &mut cb);
        }
        if dx < 0.0 {
            std::mem::swap(&mut fx, &mut tx);
            std::mem::swap(&mut fy, &mut ty);
            dx = -dx;
            dy = -dy;
        }
        if tx <= cl || fx >= cr {
            return;
        }
        let g = dy / dx;
        if fx < cl + 1.0 {
            fy += g * (cl + 1.0 - fx);
            fx = cl + 1.0;
        }
        if tx > cr - 1.0 {
            ty += g * (cr - 1.0 - tx);
            tx = cr - 1.0;
        }
        let wf = width as f32;
        if g > 0.0 {
            if fx < cl + 1.0 {
                if ty <= ct || fy >= cb {
                    return;
                }
                if fy < ct - wf {
                    fx += (ct - wf - fy) / g;
                    fy = ct - wf;
                }
                if ty > cb + wf {
                    tx += (cb + wf - ty) / g;
                    ty = cb + wf;
                }
            }
        } else if tx > cr - 1.0 {
            if fy <= ct || ty >= cb {
                return;
            }
            if ty < ct - wf {
                tx += (ct - wf - ty) / g;
                ty = ct - wf;
            }
            if fy > cb + wf {
                fx += (cb + wf - fy) / g;
                fy = cb + wf;
            }
        }
        fy += 1.0;
        ty += 1.0;
        // (x, y) in the line's own axes: x along, y across
        let put = |t: &mut T, x: i32, y: i32, br: f32| {
            if steep {
                t.blend(y, x, c, br)
            } else {
                t.blend(x, y, c, br)
            }
        };
        let span = |t: &mut T, x: i32, y0: i32, y1: i32| {
            if steep {
                hline(t, y0, x, y1, c)
            } else {
                vline(t, x, y0, y1, c)
            }
        };
        // first endpoint
        let mut xs = fx as i32;
        let ye = fy + g * (xs as f32 - fx);
        let mut iy = ye;
        if tx > cl + 1.0 {
            let b = ye - (ye as i32) as f32;
            let (x, y) = (xs, ye as i32);
            if ((ye as i32) as f32) < ye {
                put(t, x, y + width, b);
            }
            let b = 1.0 - b;
            put(t, x, y - width + extra - 1, b);
            span(t, x, y - width + extra, y + width - 1);
            iy += g;
            xs += 1;
        }
        // second endpoint
        let xe = tx.ceil() as i32;
        if fx < cr - 1.0 {
            let ye2 = ty + g * (xe as f32 - tx);
            let b = ye2 - (ye2 as i32) as f32;
            let (x, y) = (xe, ye2 as i32);
            if ((ye2 as i32) as f32) < ye2 {
                put(t, x, y + width, b);
            }
            let b = 1.0 - b;
            put(t, x, y - width + extra - 1, b);
            if steep {
                span(t, x, y - width, y + width - 1);
            } else {
                span(t, x, y - width + extra, y + width - 1);
            }
        }
        for x in xs..xe {
            let y = iy as i32;
            let b = 1.0 - iy + y as f32;
            put(t, x, y - width + extra - 1, b);
            if (y as f32) < iy {
                put(t, x, y + width, 1.0 - b);
            }
            span(t, x, y - width + extra, y + width - 1);
            iy += g;
        }
    }
}

/// Tests that switch the (global) theme take this, so they don't run at the same time.
#[cfg(test)]
pub(crate) static THEME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Compare an image with a raw dump of the Python app's own surface (u32 w, u32 h, RGBA): None when they're the
/// same to within one level everywhere, else what differs.
#[cfg(test)]
pub(crate) fn tests_cmp(dir: &str, name: &str, got: &Image) -> Option<String> {
    let Ok(b) = std::fs::read(format!("{dir}/{name}.raw")) else {
        return Some(format!("{name}: no dump"));
    };
    let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let h = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    if (w, h) != (got.w, got.h) {
        return Some(format!("{name}: size {}x{} vs {w}x{h}", got.w, got.h));
    }
    let (mut n1, mut n2, mut worst) = (0, 0, 0);
    for (a, e) in got.rgba.chunks_exact(4).zip(b[8..].chunks_exact(4)) {
        // (the colour of a pixel no one can see doesn't count)
        let chans = if a[3] <= 1 && e[3] <= 1 { 3..4 } else { 0..4 };
        let d = chans
            .map(|i| (a[i] as i32 - e[i] as i32).abs())
            .max()
            .unwrap_or(0);
        if d > 0 {
            n1 += 1;
        }
        if d > 1 {
            n2 += 1;
        }
        worst = worst.max(d);
    }
    (n2 > 0).then(|| format!("{name}: {n2} px > 1 level apart ({n1} differ, max {worst})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrect_coverage_edges() {
        let m = Mask::rrect(10.0, 6.0, 0.0, 0.0, 0.0, 10, 6);
        assert!(m.v.iter().all(|&v| (v - 1.0).abs() < 1e-6));
        let m = Mask::rrect(4.0, 4.0, 0.0, 1.5, 1.5, 7, 7);
        assert!((m.at(1, 3) - 0.5).abs() < 1e-6); // half-covered column
        let r = Mask::rrect_full(20, 20, 10.0);
        assert_eq!(r.at(0, 0), 0.0);
        assert_eq!(r.at(10, 10), 1.0);
    }

    #[test]
    fn blur_keeps_mass_and_spreads() {
        let mut m = Mask::new(41, 41);
        m.v[20 * 41 + 20] = 1.0;
        for sigma in [1.5f32, 5.0] {
            let b = m.blur(sigma);
            let total: f32 = b.v.iter().sum();
            assert!((total - 1.0).abs() < 1e-3, "{sigma}: {total}");
            assert!(b.at(20, 20) < 0.2 && b.at(22, 20) > 0.0);
            // symmetric
            assert!((b.at(17, 20) - b.at(23, 20)).abs() < 1e-5);
        }
    }

    #[test]
    fn shift_moves_content() {
        let m = Mask::from_fn(4, 3, |x, y| (y * 4 + x) as f32);
        let s = m.shift(1, 1);
        assert_eq!(s.at(0, 0), 0.0);
        assert_eq!(s.at(1, 1), m.at(0, 0));
        assert_eq!(s.at(3, 2), m.at(2, 1));
    }

    /// The pygame ports against pygame itself: QRS_PG_DUMPS=<dir> of raw dumps (u32 w, u32 h, RGBA) written by a
    /// script running the same cases through pygame-ce 2.5.8 (skipped without it).
    #[test]
    fn pygame_parity() {
        use super::pg::{self, Surf};
        let Ok(dir) = std::env::var("QRS_PG_DUMPS") else {
            return;
        };
        let load = |name: &str| -> Option<Surf> {
            let b = std::fs::read(format!("{dir}/{name}.raw")).ok()?;
            let w = u32::from_le_bytes(b[0..4].try_into().ok()?) as i32;
            let h = u32::from_le_bytes(b[4..8].try_into().ok()?) as i32;
            let px = b[8..].chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
            Some(Surf { w, h, px })
        };
        let mut bad = Vec::new();
        let mut n = 0;
        let mut check = |name: String, got: Surf| {
            let Some(want) = load(&name) else {
                bad.push(format!("{name}: no dump"));
                return;
            };
            n += 1;
            if (want.w, want.h) != (got.w, got.h) {
                bad.push(format!(
                    "{name}: size {}x{} vs {}x{}",
                    got.w, got.h, want.w, want.h
                ));
                return;
            }
            let mut diff = 0;
            let mut worst = 0;
            for (a, b) in got.px.iter().zip(&want.px) {
                let d = (0..4)
                    .map(|i| (a[i] as i32 - b[i] as i32).abs())
                    .max()
                    .unwrap_or(0);
                if d > 0 {
                    diff += 1;
                    worst = worst.max(d);
                }
            }
            if diff > 0 {
                bad.push(format!("{name}: {diff} px differ (max {worst})"));
            }
        };
        let c = [200, 100, 50, 255];
        let pattern = |w: i32, h: i32, k: i32| {
            let mut s = Surf::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    s.px[(y * w + x) as usize] = [
                        ((x * 37 + y * 11 + k) % 256) as u8,
                        ((x * 13 + y * 57 + 3 * k) % 256) as u8,
                        ((x * x + y + k) % 256) as u8,
                        ((x * 7 + y * 3 + 5 * k) % 256) as u8,
                    ];
                }
            }
            s
        };
        let disc = |w: i32, h: i32, r: i32| {
            let mut s = Surf::new(w, h);
            pg::circle(
                &mut s,
                [230, 190, 90, 255],
                ((w / 2) as f64, (h / 2) as f64),
                r as f64,
                0,
            );
            s
        };
        for wd in 1..7 {
            for ang in [0, 17, 45, 60, 90, 135, 200, 300, 333] {
                let mut s = Surf::new(64, 64);
                let a = (ang as f64).to_radians();
                let p1 = (32.0 + 25.0 * a.cos(), 32.0 + 25.0 * a.sin());
                pg::line_f(&mut s, c, (32.6, 31.2), p1, wd);
                check(format!("line_w{wd}_a{ang}"), s);
            }
        }
        for r in 1..13 {
            for (cx, cy) in [(20.0, 20.0), (20.7, 20.3)] {
                let tag = (cx * 10.0f64) as i32;
                let mut s = Surf::new(44, 44);
                pg::circle(&mut s, c, (cx, cy), r as f64, 0);
                check(format!("circle_r{r}_{tag}"), s);
                let mut s = Surf::new(44, 44);
                pg::circle(&mut s, c, (cx, cy), r as f64 + 0.6, 0);
                check(format!("circlef_r{r}_{tag}"), s);
            }
        }
        for r in [6, 10, 17] {
            for wd in [1, 2, 3, 5, 9] {
                let mut s = Surf::new(44, 44);
                pg::circle(&mut s, c, (21.0, 22.0), r as f64, wd);
                check(format!("ring_r{r}_w{wd}"), s);
            }
        }
        for r in 1..11 {
            let mut s = Surf::new(30, 30);
            pg::aacircle(&mut s, c, (14.0, 15.0), r as f64, 0, None);
            check(format!("aac_r{r}"), s);
            for wd in [1, 2, 3] {
                let mut s = Surf::new(30, 30);
                pg::aacircle(&mut s, c, (14.0, 15.0), r as f64, wd, None);
                check(format!("aac_r{r}_w{wd}"), s);
                let mut s = Surf::new(30, 30);
                pg::aacircle(
                    &mut s,
                    c,
                    (14.0, 15.4),
                    r as f64,
                    wd,
                    Some([false, false, true, true]),
                );
                check(format!("aacq_r{r}_w{wd}"), s);
            }
        }
        let pi = std::f64::consts::PI;
        let star: Vec<(f64, f64)> = (0..10)
            .map(|i| {
                let r = if i % 2 == 0 { 16.0 } else { 7.0 };
                let a = -pi / 2.0 + i as f64 * pi / 5.0;
                (22.0 + r * a.cos(), 22.0 + r * a.sin())
            })
            .collect();
        let oct: Vec<(f64, f64)> = (0..8)
            .map(|j| {
                let a = (22.5 + 45.0 * j as f64).to_radians();
                (22.0 + 15.0 * a.cos(), 22.0 + 15.0 * a.sin())
            })
            .collect();
        let polys: Vec<(&str, Vec<(f64, f64)>)> = vec![
            ("tri", vec![(7.3, 4.5), (7.3, 19.5), (20.2, 12.0)]),
            ("tri2", vec![(30.1, 3.3), (5.9, 25.7), (40.4, 38.8)]),
            ("quad", vec![(10.2, 5.5), (30.7, 9.1), (25.3, 33.8), (4.4, 28.2)]),
            ("star", star),
            ("oct", oct),
        ];
        for (name, pts) in &polys {
            for wd in 0..5 {
                let mut s = Surf::new(44, 44);
                pg::polygon(&mut s, c, pts, wd);
                check(format!("poly_{name}_w{wd}"), s);
            }
        }
        for (x, y, w, h, wd, br) in [
            (3.7, 4.2, 30.5, 20.9, 0, 5),
            (2.0, 2.0, 40.0, 40.0, 0, 8),
            (5.0, 6.0, 17.0, 17.0, 4, 5),
            (1.0, 1.0, 30.0, 9.0, 0, 4),
            (3.0, 3.0, 20.0, 20.0, 0, 0),
            (3.0, 3.0, 20.0, 20.0, 2, 0),
            (4.4, 3.9, 9.9, 9.9, 3, 2),
            (2.0, 2.0, 28.0, 12.0, 0, 1),
        ] {
            let mut s = Surf::new(44, 44);
            pg::rect(&mut s, c, (x, y, w, h), wd, br);
            check(
                format!("rect_{}_{}_{wd}_{br}", (x * 10.0f64) as i32, (w * 10.0f64) as i32),
                s,
            );
        }
        for (x, y, w, h) in [
            (4.0, 15.0, 7.0, 5.0),
            (3.6, 5.2, 28.4, 17.3),
            (2.0, 2.0, 30.0, 30.0),
            (5.0, 5.0, 21.0, 8.0),
            (1.5, 2.5, 13.7, 9.9),
        ] {
            let mut s = Surf::new(44, 44);
            pg::ellipse(&mut s, c, (x, y, w, h));
            check(
                format!("ell_{}_{}", (x * 10.0f64) as i32, (w * 10.0f64) as i32),
                s,
            );
        }
        for (r, a0, a1, wd) in [
            ((10.7, 4.7, 32.0, 44.0), 0.0, pi, 8),
            (
                (5.3, 5.3, 64.0, 64.0),
                (-60.0f64).to_radians(),
                200.0f64.to_radians(),
                9,
            ),
            ((2.0, 2.0, 40.0, 40.0), 0.3, 2.5, 3),
            ((3.0, 3.0, 30.0, 20.0), 4.0, 1.0, 2),
        ] {
            let mut s = Surf::new(70, 70);
            pg::arc(&mut s, c, r, a0, a1, wd);
            check(format!("arc_{}_{wd}", r.2 as i32), s);
        }
        for wd in [1, 2, 3, 4, 5, 7] {
            for ang in [0, 10, 30, 45, 60, 80, 90, 120, 160, 200, 250, 300] {
                let mut s = Surf::new(64, 64);
                let a = (ang as f64).to_radians();
                let p0 = ((31.6 + 3.3 * a.sin()) as f32, (32.4 - 3.3 * a.cos()) as f32);
                let p1 = ((31.6 + 24.7 * a.sin()) as f32, (32.4 - 24.7 * a.cos()) as f32);
                pg::aaline(&mut s, c, p0, p1, wd);
                check(format!("aal_w{wd}_a{ang}"), s);
            }
        }
        for (w, h, dw, dh, k) in [
            (72, 72, 24, 24, 0),
            (96, 96, 24, 24, 1),
            (60, 45, 20, 15, 2),
            (24, 24, 100, 61, 3),
            (24, 24, 31, 200, 4),
            (90, 30, 30, 30, 5),
            (30, 90, 30, 30, 6),
        ] {
            check(
                format!("ss_{w}x{h}_{dw}x{dh}"),
                pattern(w, h, k).smoothscale(dw, dh),
            );
        }
        check("ss_disc".into(), disc(99, 99, 40).smoothscale(33, 33));
        for r in [1, 2, 3, 5] {
            check(format!("blur_{r}"), pattern(40, 33, r).gaussian_blur(r));
            check(format!("blurd_{r}"), disc(41, 41, 12).gaussian_blur(r));
        }
        for ang in [-135, -100, -45, -17, -1, 0, 1, 30, 90, 135] {
            check(format!("rot_{ang}"), pattern(21, 21, 7).rotozoom(ang as f32));
            check(format!("rotd_{ang}"), disc(30, 30, 9).rotozoom(ang as f32));
        }
        let mut a = pattern(37, 29, 8);
        a.blit(&pattern(37, 29, 9), 0, 0);
        check("blit_pp".into(), a);
        let mut cl = Surf::new(37, 29);
        cl.blit(&pattern(37, 29, 10), 3, -2);
        check("blit_clear".into(), cl);
        let mut a = pattern(37, 29, 11);
        a.fill_mult([255, 255, 255, 90]);
        check("mult".into(), a);
        let mut a = pattern(37, 29, 12);
        a.fill_mult([0, 0, 0, 90]);
        check("mult0".into(), a);
        eprintln!("pygame parity: {n} cases");
        assert!(
            n > 300 && bad.is_empty(),
            "{n} cases, {} differ:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    #[test]
    fn layers_compose_like_python() {
        let mut l = Layers::new(1, 1);
        l.over([255.0, 0.0, 0.0], &Mask::filled(1, 1, 0.5));
        l.over([0.0, 0.0, 255.0], &Mask::filled(1, 1, 0.5));
        let px = l.rgba(None);
        // premult: r 0.25, b 0.5, a 0.75 -> unpremultiplied 85, 0, 170
        assert_eq!(px, vec![85, 0, 170, 191]);
    }
}
