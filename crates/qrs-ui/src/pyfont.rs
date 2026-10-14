//! How pygame-ce 2.5 lays out one line of text, so a line here is the Python app's to the pixel: SDL_ttf 2.24's
//! `TTF_Size_Internal` and `Render_Line`, shaped by HarfBuzz over FreeType (`hb_ft`, `FT_LOAD_DEFAULT`).
//!
//! - The size: `FT_Set_Char_Size(px * 64)` at 72 dpi, so the scale is `FT_DivFix(px * 64, upem)`; ascent, descent and
//!   height are `FT_CEIL(FT_MulFix(...))` of the font's hhea numbers (the height is of ascender - descender, not the
//!   sum of the rounded two).
//! - Each glyph advances by its hinted advance (FreeType rounds the scaled advance to whole pixels; Manrope has no
//!   glyph instructions, so nothing else moves) plus HarfBuzz's GPOS kerning scaled to 26.6 (`em_mult`).
//! - A glyph is drawn with its origin on the whole pixel `FT_FLOOR(pen)`; the line's box spans every glyph's ink box
//!   (the control box, floored and ceiled as FreeType presets a bitmap) and the pen's end, from x 0 and y 0 to at
//!   least the font's height.
//!
//! Synthetic bold (pygame's `SysFont(bold=True)` for a family without a bold file, Segoe UI Symbol) widens each
//! glyph by `ppem / 10` pixels as SDL_ttf does.

use harfrust::font::{BuiltinFontFuncs, FontFuncs};
use harfrust::{FontRef, GlyphId, ShapeOptions, ShaperData, UnicodeBuffer};
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};

/// One font file.
pub struct Face {
    font: FontRef<'static>,
    shaper: ShaperData,
    upem: i64,
    asc: i64,
    desc: i64,
}

/// A line of text laid out: its box in whole pixels, its baseline from the box's top, and each glyph's origin from
/// the box's left (in the shaper's glyph order, with the char index each glyph's cluster starts at).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub w: i32,
    pub h: i32,
    pub base: i32,
    pub glyphs: Vec<(usize, i32)>,
}

/// `FT_MulFix`: `a * b / 65536`, rounded, the sign apart.
fn mul_fix(a: i64, b: i64) -> i64 {
    let p = (a * b).abs();
    let r = (p + 0x8000) >> 16;
    if (a < 0) != (b < 0) { -r } else { r }
}

/// `FT_CEIL` of a 26.6 value, in pixels.
fn ceil6(v: i64) -> i64 {
    (v + 63) >> 6
}

/// `FT_FLOOR` of a 26.6 value, in pixels.
fn floor6(v: i64) -> i64 {
    v >> 6
}

/// HarfBuzz's advances from FreeType: the hinted advance, already in 26.6.
struct Hinted<'a> {
    face: &'a Face,
    scale: i64,
    bold: i64,
}

impl FontFuncs for Hinted<'_> {
    fn advance_width(&mut self, _: &BuiltinFontFuncs, glyph: GlyphId) -> i32 {
        (self.face.advance(glyph, self.scale) + self.bold) as i32
    }
}

impl Face {
    pub fn new(bytes: &'static [u8]) -> Option<Face> {
        let font = FontRef::new(bytes).ok()?;
        let m = font.metrics(Size::unscaled(), LocationRef::default());
        let shaper = ShaperData::new(&font);
        Some(Face {
            upem: m.units_per_em as i64,
            asc: m.ascent as i64,
            desc: m.descent as i64,
            shaper,
            font,
        })
    }

    /// FreeType's scale for a `px`-pixel size (16.16, font units to 26.6).
    fn scale(&self, px: i64) -> i64 {
        (px * 64 * 65536 + self.upem / 2) / self.upem
    }

    /// (ascent, descent, height) in pixels at `px`, as SDL_ttf's `TTF_initFontMetrics` (descent is negative).
    pub fn metrics(&self, px: i32) -> (i32, i32, i32) {
        let s = self.scale(px as i64);
        (
            ceil6(mul_fix(self.asc, s)) as i32,
            ceil6(mul_fix(self.desc, s)) as i32,
            ceil6(mul_fix(self.asc - self.desc, s)) as i32,
        )
    }

    /// A glyph's hinted advance in 26.6: the scaled advance rounded to whole pixels.
    fn advance(&self, glyph: GlyphId, scale: i64) -> i64 {
        let adv = self
            .font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(glyph)
            .unwrap_or(0.0) as i64;
        (mul_fix(adv, scale) + 32) & !63
    }

    /// A glyph's ink box in pixels from its origin, (left, width, top, rows), as FreeType presets its bitmap.
    fn ink(&self, glyph: GlyphId, scale: i64) -> (i64, i64, i64, i64) {
        let gm = self.font.glyph_metrics(Size::unscaled(), LocationRef::default());
        let Some(b) = gm.bounds(glyph) else {
            return (0, 0, 0, 0);
        };
        if b.x_max <= b.x_min && b.y_max <= b.y_min {
            return (0, 0, 0, 0);
        }
        let f = |v: f32| mul_fix(v as i64, scale);
        let (x0, x1, y0, y1) = (f(b.x_min), f(b.x_max), f(b.y_min), f(b.y_max));
        let (left, top) = (floor6(x0), ceil6(y1));
        (left, ceil6(x1) - left, top, top - floor6(y0))
    }

    /// `c`'s advance in whole pixels at `px`, as pygame's `Font.metrics` gives it (synthetic bold adds its overhang).
    pub fn advance_px(&self, c: char, px: i32, bold: bool) -> i32 {
        let g = self.font.charmap().map(c).unwrap_or_default();
        let over = if bold { px / 10 } else { 0 };
        ((self.advance(g, self.scale(px as i64)) >> 6) as i32) + over
    }

    /// Whether the font maps `c` to a glyph.
    pub fn has(&self, c: char) -> bool {
        self.font.charmap().map(c).is_some_and(|g| g.to_u32() != 0)
    }

    /// `text` at `px` pixels, laid out as pygame lays it out (`bold`: SDL_ttf's synthetic bold).
    pub fn layout(&self, text: &str, px: i32, bold: bool) -> Line {
        let scale = self.scale(px as i64);
        let (ascent, _, height) = self.metrics(px);
        let overhang = if bold { (px / 10) as i64 } else { 0 };
        let mut funcs = Hinted {
            face: self,
            scale,
            bold: overhang * 64,
        };
        let hb_scale = ((scale * self.upem + 32768) >> 16) as i32;
        let shaper = self.shaper.shaper(&self.font).build();
        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let out = shaper.shape(
            buf,
            ShapeOptions::new()
                .scale(Some(hb_scale))
                .font_funcs(Some(&mut funcs)),
        );
        // char index of each byte offset (clusters are byte offsets)
        let starts: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        let (mut minx, mut maxx, mut miny, mut maxy) = (0i64, 0i64, 0i64, height as i64);
        let mut x = 0i64;
        let mut pens = Vec::new();
        for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
            let gid = GlyphId::new(info.glyph_id);
            let pen = x + pos.x_offset as i64;
            x += pos.x_advance as i64;
            let (left, mut width, top, rows) = self.ink(gid, scale);
            if width > 0 || rows > 0 {
                width += overhang;
            }
            let gx = floor6(pen) + left;
            let gy = ascent as i64 - top;
            minx = minx.min(gx);
            maxx = maxx.max(gx + width);
            miny = miny.min(gy);
            maxy = maxy.max(gy + rows);
            let ci = starts.partition_point(|&b| b < info.cluster as usize);
            pens.push((ci, floor6(pen)));
        }
        maxx = maxx.max(floor6(x));
        let xstart = -minx.min(0);
        let ystart = -miny.min(0);
        Line {
            w: (maxx - minx) as i32,
            h: (maxy - miny) as i32,
            base: (ystart + ascent as i64) as i32,
            glyphs: pens.into_iter().map(|(c, p)| (c, (xstart + p) as i32)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Manrope-Medium.ttf");
    const BOLD: &[u8] = include_bytes!("../../../assets/fonts/Manrope-Bold.ttf");

    /// pygame-ce 2.5.8 (`Font.get_height/get_ascent`, `Font.size`) on these strings.
    #[test]
    fn like_pygame() {
        let m = Face::new(MEDIUM).unwrap();
        assert_eq!(m.metrics(11), (12, -3, 16));
        assert_eq!(m.metrics(12), (13, -3, 17));
        assert_eq!(m.metrics(16), (18, -4, 22));
        let _ = BOLD;
    }

    /// Every case pygame recorded (tools: scratch `textprobe/probe.py` writes truth.json), when QRS_TEXT_TRUTH names
    /// the file.
    #[test]
    #[ignore]
    fn truth_file() {
        let Ok(path) = std::env::var("QRS_TEXT_TRUTH") else {
            return;
        };
        let t: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let files: [(u16, &'static [u8]); 4] = [
            (500, MEDIUM),
            (600, include_bytes!("../../../assets/fonts/Manrope-SemiBold.ttf")),
            (700, BOLD),
            (800, include_bytes!("../../../assets/fonts/Manrope-ExtraBold.ttf")),
        ];
        let strings: Vec<&str> = t["strings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        let (mut bad, mut n) = (0, 0);
        for (w, bytes) in files {
            let face = Face::new(bytes).unwrap();
            for px in t["sizes"].as_array().unwrap() {
                let px = px.as_i64().unwrap() as i32;
                let f = &t["fonts"][format!("{w}/{px}")];
                let (a, d, h) = face.metrics(px);
                assert_eq!(
                    (a as i64, d as i64, h as i64),
                    (
                        f["asc"].as_i64().unwrap(),
                        f["desc"].as_i64().unwrap(),
                        f["h"].as_i64().unwrap()
                    ),
                    "{w}/{px} metrics"
                );
                for (s, want) in strings.iter().zip(f["sizes"].as_array().unwrap()) {
                    let l = face.layout(s, px, false);
                    let want = (want[0].as_i64().unwrap() as i32, want[1].as_i64().unwrap() as i32);
                    n += 1;
                    if (l.w, l.h) != want {
                        bad += 1;
                        if bad <= 40 {
                            eprintln!("{w}/{px} {s:?}: ours {:?} pygame {want:?}", (l.w, l.h));
                        }
                    }
                }
            }
        }
        // Segoe UI Symbol (regular, and pygame's synthetic bold), when installed
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        if let Ok(bytes) = std::fs::read(std::path::Path::new(&sys).join("Fonts").join("seguisym.ttf")) {
            let face = Face::new(Box::leak(bytes.into_boxed_slice())).unwrap();
            let sym: Vec<&str> = t["symbol_strings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            for bold in [false, true] {
                for px in t["sizes"].as_array().unwrap() {
                    let px = px.as_i64().unwrap() as i32;
                    let f = &t["fonts"][format!("sym{}/{px}", bold as u8)];
                    for (s, want) in sym.iter().zip(f["sizes"].as_array().unwrap()) {
                        let l = face.layout(s, px, bold);
                        let want = (want[0].as_i64().unwrap() as i32, want[1].as_i64().unwrap() as i32);
                        n += 1;
                        if (l.w, l.h) != want {
                            bad += 1;
                            if bad <= 60 {
                                eprintln!(
                                    "sym bold={bold} {px} {s:?}: ours {:?} pygame {want:?}",
                                    (l.w, l.h)
                                );
                            }
                        }
                    }
                }
            }
        }
        eprintln!("{bad} of {n} differ");
        assert_eq!(bad, 0);
    }
}
