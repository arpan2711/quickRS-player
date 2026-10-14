//! Fonts and text: Manrope (bundled, SIL OFL) at weights 500 / 600 / 700 / 800, sized in points (= the Python
//! app's scale-1 pixels; egui's font size is the em, as pygame's), letter-spaced small caps, ellipsis and word
//! wrapping. Port of the text half of rsplay/ui.py. A line with a character Manrope lacks (★ ✓ ♪ ♭ ♯ ▶ ...) is drawn
//! in Segoe UI Symbol, the whole line, as the Python app does (the system's font, not bundled; without it, egui's
//! own fonts fill the gaps).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::pyfont;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Galley, Painter, Pos2, Rect,
};

pub const CAPS_TRACK: f32 = 0.08;

const MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Manrope-Medium.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Manrope-SemiBold.ttf");
const BOLD: &[u8] = include_bytes!("../../../assets/fonts/Manrope-Bold.ttf");
const EXTRABOLD: &[u8] = include_bytes!("../../../assets/fonts/Manrope-ExtraBold.ttf");

pub const WEIGHTS: [u16; 4] = [500, 600, 700, 800];

/// Install Manrope as egui's fonts (call once at start). The proportional family is Manrope Medium.
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let fallbacks: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (w, bytes) in [(500u16, MEDIUM), (600, SEMIBOLD), (700, BOLD), (800, EXTRABOLD)] {
        let name = format!("manrope-{w}");
        defs.font_data.insert(
            name.clone(),
            Arc::new(FontData::from_static(bytes).tweak(as_pygame(false))),
        );
        let mut list = vec![name];
        list.extend(fallbacks.iter().cloned());
        defs.families.insert(family(w), list);
    }
    if let Some(p) = defs.families.get_mut(&FontFamily::Proportional) {
        p.insert(0, "manrope-500".into());
    }
    let fonts_dir =
        std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
            .join("Fonts");
    if let Ok(bytes) = std::fs::read(fonts_dir.join("seguisym.ttf")) {
        let leaked: &'static [u8] = Box::leak(bytes.clone().into_boxed_slice());
        if let Some(f) = pyfont::Face::new(leaked) {
            let _ = SYMBOL_FACE.set(f);
        }
        if let Some((asc, desc, upem)) = hhea(&bytes) {
            SYMBOL.store(
                ((upem as u64) << 32) | ((asc as u64) << 16) | desc as u64,
                Ordering::Relaxed,
            );
        }
        defs.font_data.insert(
            "segoe-symbol".into(),
            Arc::new(FontData::from_owned(bytes).tweak(as_pygame(true))),
        );
        let mut list = vec!["segoe-symbol".to_string()];
        list.extend(fallbacks.iter().cloned());
        defs.families.insert(FontFamily::Name("symbol".into()), list);
    }
    ctx.set_fonts(defs);
}

/// Glyphs drawn as FreeType draws them for pygame: each on a whole pixel (no subpixel positions; `pyfont` moves
/// them onto pygame's pixels), Manrope unhinted (it has no glyph instructions, so FreeType draws its outlines as
/// they are), a hinted font (Segoe UI Symbol) hinted.
fn as_pygame(hinted: bool) -> egui::FontTweak {
    egui::FontTweak {
        hinting: Some(hinted),
        subpixel_binning: Some(false),
        ..Default::default()
    }
}

/// Segoe UI Symbol's (units per em, ascender, -descender), packed; 0 when it isn't installed.
static SYMBOL: AtomicU64 = AtomicU64::new(0);

/// A font file's hhea ascender, -descender and its units per em.
fn hhea(b: &[u8]) -> Option<(u16, u16, u16)> {
    let u16_at = |o: usize| b.get(o..o + 2).map(|x| u16::from_be_bytes([x[0], x[1]]));
    let n = u16_at(4)? as usize;
    let (mut head, mut hh) = (None, None);
    for i in 0..n {
        let r = 12 + 16 * i;
        let tag = b.get(r..r + 4)?;
        let off = u32::from_be_bytes(b.get(r + 8..r + 12)?.try_into().ok()?) as usize;
        match tag {
            b"head" => head = Some(off),
            b"hhea" => hh = Some(off),
            _ => {}
        }
    }
    let (head, hh) = (head?, hh?);
    let asc = u16_at(hh + 4)? as i16;
    let desc = u16_at(hh + 6)? as i16;
    Some((asc.max(0) as u16, (-desc).max(0) as u16, u16_at(head + 18)?))
}

/// The line is drawn in Segoe UI Symbol (Manrope lacks one of its characters, by its character map: every weight
/// has the same), as `ui.Fonts._font_for`.
fn symbol_line(_ctx: &egui::Context, text: &str) -> bool {
    if text.is_ascii() || SYMBOL.load(Ordering::Relaxed) == 0 {
        return false;
    }
    let m = face(500);
    text.chars().any(|c| !m.has(c))
}

/// The nearest bundled weight.
pub fn snap_weight(w: u16) -> u16 {
    match w {
        0..=549 => 500,
        550..=649 => 600,
        650..=749 => 700,
        _ => 800,
    }
}

pub fn family(weight: u16) -> FontFamily {
    FontFamily::Name(format!("w{}", snap_weight(weight)).into())
}

pub fn font(size: f32, weight: u16) -> FontId {
    FontId::new(size.max(1.0), family(weight))
}

/// A font size as the Python app makes it: whole pixels, rounded down, at least 8 (`ui.Fonts.px`:
/// `max(8, int(size * scale))`), as points at `ppp` (so egui draws exactly that many pixels). Text drawn at
/// another scale than the window's (the note widget's window) rounds the same way at its own.
pub fn quantize(size: f32, ppp: f32) -> f32 {
    let s = crate::units::scale();
    let s = if s > 0.0 && (s as f32 - ppp).abs() < 1e-4 {
        s
    } else {
        ppp as f64
    };
    ((size as f64 * s).floor().max(8.0) / ppp as f64) as f32
}

/// How a piece of text looks.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub size: f32,
    pub weight: u16,
    pub color: Color32,
    /// Letter spacing in ems.
    pub track: f32,
}

impl Style {
    pub fn new(size: f32, weight: u16, color: Color32) -> Style {
        Style {
            size,
            weight,
            color,
            track: 0.0,
        }
    }

    /// A small-caps heading: extra bold, letter-spaced.
    pub fn caps(size: f32, color: Color32) -> Style {
        Style {
            size,
            weight: 800,
            color,
            track: CAPS_TRACK,
        }
    }

    pub fn tracked(mut self, track: f32) -> Style {
        self.track = track;
        self
    }
}

fn format(st: Style, ppp: f32, symbol: bool) -> TextFormat {
    let size = quantize(st.size, ppp).max(1.0);
    TextFormat {
        font_id: if symbol {
            FontId::new(size, FontFamily::Name("symbol".into()))
        } else {
            font(size, st.weight)
        },
        color: st.color,
        ..Default::default()
    }
}

/// One line in one style (tracked text is placed glyph by glyph by `draw`, so the job has no letter spacing).
pub fn job(text: &str, st: Style, ppp: f32, symbol: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, format(st, ppp, symbol));
    job.wrap.max_width = f32::INFINITY;
    job
}

pub fn galley(ctx: &egui::Context, text: &str, st: Style) -> Arc<Galley> {
    let ppp = ctx.pixels_per_point();
    let symbol = symbol_line(ctx, text);
    ctx.fonts_mut(|f| f.layout_job(job(text, st, ppp, symbol)))
}

// -- the Python app's text boxes (pygame / SDL_ttf) ---------------------------------------------------------
// pygame draws a line of text as a surface, laid out by SDL_ttf (`pyfont`): as tall as the font (or its ink), as wide
// as the shaped line in whole pixels, each glyph on a whole pixel, and places it by an anchor with whole-pixel
// arithmetic (`Rect` truncates, a centre is `x - w // 2`). Text here goes in the same box with each glyph where
// pygame puts it (egui's glyphs, moved onto pygame's pixels).

/// The font's size in whole pixels at this scale.
fn font_px(st: Style, ppp: f32) -> f64 {
    (quantize(st.size, ppp) as f64 * ppp as f64).round()
}

fn faces() -> &'static [pyfont::Face; 4] {
    static F: OnceLock<[pyfont::Face; 4]> = OnceLock::new();
    F.get_or_init(|| [MEDIUM, SEMIBOLD, BOLD, EXTRABOLD].map(|b| pyfont::Face::new(b).expect("bundled font")))
}

fn face(weight: u16) -> &'static pyfont::Face {
    &faces()[match snap_weight(weight) {
        500 => 0,
        600 => 1,
        700 => 2,
        _ => 3,
    }]
}

/// Segoe UI Symbol, when installed.
static SYMBOL_FACE: OnceLock<pyfont::Face> = OnceLock::new();

/// A line as pygame lays it out (cached; letter-spaced text glyph by glyph, as `ui.Fonts.render` with `track`).
fn py_line(ctx: &egui::Context, text: &str, st: Style) -> Arc<pyfont::Line> {
    type Key = (String, u16, i32, u32, bool);
    static CACHE: Mutex<Option<HashMap<Key, Arc<pyfont::Line>>>> = Mutex::new(None);
    let px = font_px(st, ctx.pixels_per_point()) as i32;
    let symbol = symbol_line(ctx, text);
    let key = (
        text.to_string(),
        snap_weight(st.weight),
        px,
        st.track.to_bits(),
        symbol,
    );
    if let Some(l) = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|c| c.get(&key))
    {
        return l.clone();
    }
    let (f, bold) = match SYMBOL_FACE.get() {
        Some(sym) if symbol => (sym, st.weight >= 700),
        _ => (face(st.weight), false),
    };
    let line = if tracked(text, st) {
        // each glyph rendered alone and blitted at int(x); advances are the glyphs' own, unkerned
        let gap = st.track as f64 * px as f64;
        let (mut x, mut adv, mut glyphs, mut n) = (0.0f64, 0.0f64, Vec::new(), 0usize);
        let mut buf = [0u8; 4];
        for (i, c) in text.chars().enumerate() {
            let one = f.layout(c.encode_utf8(&mut buf), px, bold);
            if let Some(&(_, o)) = one.glyphs.first() {
                glyphs.push((i, x.trunc() as i32 + o));
            }
            let a = f.advance_px(c, px, bold) as f64;
            x += a + gap;
            adv += a;
            n += 1;
        }
        let (asc, _, h) = f.metrics(px);
        pyfont::Line {
            w: ((adv + gap * (n as f64 - 1.0) + 0.5).trunc() as i32 + 1).max(1),
            h,
            base: asc,
            glyphs,
        }
    } else {
        f.layout(text, px, bold)
    };
    let line = Arc::new(line);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = c.get_or_insert_with(HashMap::new);
    if c.len() > 5000 {
        c.clear();
    }
    c.insert(key, line.clone());
    line
}

fn tracked(text: &str, st: Style) -> bool {
    st.track > 0.0 && text.chars().nth(1).is_some()
}

/// The width of a line of text in pixels, as pygame makes it.
fn width_px(ctx: &egui::Context, text: &str, st: Style) -> f64 {
    py_line(ctx, text, st).w as f64
}

/// The width text takes (points).
pub fn width(ctx: &egui::Context, text: &str, st: Style) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    (width_px(ctx, text, st) / ctx.pixels_per_point() as f64) as f32
}

/// The height of a line in this style (the font's height, as pygame's `get_height`).
pub fn line_height(ctx: &egui::Context, st: Style) -> f32 {
    let ppp = ctx.pixels_per_point();
    (face(st.weight).metrics(font_px(st, ppp) as i32).2 as f64 / ppp as f64) as f32
}

/// Text cut to fit `max_w`, with an ellipsis (as `ui.blit_text`: letters come off until the text and "…" fit,
/// then the "…" goes after the text without its trailing spaces).
pub fn fit(ctx: &egui::Context, text: &str, st: Style, max_w: f32) -> String {
    if width(ctx, text, st) <= max_w {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        let s: String = chars.iter().collect();
        if width(ctx, &format!("{s}…"), st) <= max_w {
            break;
        }
        chars.pop();
    }
    let s: String = chars.iter().collect();
    format!("{}…", s.trim_end())
}

/// egui's galley of `text` with each glyph moved onto pygame's pixel (`line`'s origins and baseline), for a box whose
/// left-top is the galley's position. Cached against egui's galley (a new one, after egui rebuilt its glyph atlas, is
/// moved again).
fn moved(ctx: &egui::Context, text: &str, st: Style, line: &pyfont::Line, ppp: f64) -> Arc<Galley> {
    type Key = (String, u16, u32, u32, u32);
    static CACHE: Mutex<Option<HashMap<Key, (Arc<Galley>, Arc<Galley>)>>> = Mutex::new(None);
    let g = galley(ctx, text, Style { track: 0.0, ..st });
    let key = (
        text.to_string(),
        snap_weight(st.weight),
        st.size.to_bits(),
        st.track.to_bits(),
        (ppp as f32).to_bits(),
    );
    if let Some((orig, done)) = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|c| c.get(&key))
        && Arc::ptr_eq(orig, &g)
    {
        return done.clone();
    }
    let mut out = (*g).clone();
    // each char's glyph origins in pygame's line (a cluster's glyphs on its first char)
    let mut at: HashMap<usize, std::collections::VecDeque<i32>> = HashMap::new();
    for &(c, x) in &line.glyphs {
        at.entry(c).or_default().push_back(x);
    }
    let mut ci = 0usize;
    for placed in &mut out.rows {
        let (px0, py0) = (placed.pos.x as f64, placed.pos.y as f64);
        let row = Arc::make_mut(&mut placed.row);
        let mut shifts = Vec::with_capacity(row.glyphs.len());
        for gl in &row.glyphs {
            let target = at.get_mut(&ci).and_then(|q| q.pop_front());
            ci += 1;
            if gl.uv_rect.is_nothing() {
                continue;
            }
            let Some(tx) = target else { continue };
            let ex = ((px0 + gl.pos.x as f64) * ppp).round();
            let ey = ((py0 + gl.pos.y as f64) * ppp).round();
            let (dx, dy) = (tx as f64 - ex, line.base as f64 - ey);
            shifts.push((gl.first_vertex as usize, (dx / ppp) as f32, (dy / ppp) as f32));
        }
        for (v0, dx, dy) in shifts {
            for v in row.visuals.mesh.vertices.iter_mut().skip(v0).take(4) {
                v.pos.x += dx;
                v.pos.y += dy;
            }
        }
        row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
        ci += usize::from(placed.ends_with_newline);
    }
    out.mesh_bounds = out.rows.iter().fold(Rect::NOTHING, |r, p| {
        r.union(p.row.visuals.mesh_bounds.translate(p.pos.to_vec2()))
    });
    let done = Arc::new(out);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = c.get_or_insert_with(HashMap::new);
    if c.len() > 5000 {
        c.clear();
    }
    c.insert(key, (g, done.clone()));
    done
}

/// Draw `s` in its pygame box, the box's left-top at (`left`, `top`) pixels.
fn line_at(p: &Painter, s: &str, st: Style, left: f64, top: f64, ppp: f64) {
    let ctx = p.ctx();
    let line = py_line(ctx, s, st);
    let g = moved(ctx, s, st, &line, ppp);
    p.galley(Pos2::new((left / ppp) as f32, (top / ppp) as f32), g, st.color);
}

/// The box of a line `w` pixels wide at `pos` by `anchor`, as pygame places it: (left, top) in pixels. (With an
/// allowance: a whole-pixel position in f32 points can come out a hair under its pixel.)
fn place(pos: Pos2, anchor: Align2, w: f64, h: f64, ppp: f64) -> (f64, f64) {
    let whole = |v: f64| (v + 1e-3f64.copysign(v)).trunc();
    let (x, y) = (whole(pos.x as f64 * ppp), whole(pos.y as f64 * ppp));
    let left = match anchor.x() {
        Align::Min => x,
        Align::Center => x - (w / 2.0).floor(),
        Align::Max => x - w,
    };
    let top = match anchor.y() {
        Align::Min => y,
        Align::Center => y - (h / 2.0).floor(),
        Align::Max => y - h,
    };
    (left, top)
}

/// Draw text with one of its points (`anchor`) at `pos`, cut with an ellipsis to `max_w`, as the Python app's
/// `blit_text` does. Returns its box.
pub fn draw(p: &Painter, text: &str, pos: Pos2, anchor: Align2, st: Style, max_w: Option<f32>) -> Rect {
    if text.is_empty() {
        return Rect::from_min_size(pos, egui::Vec2::ZERO);
    }
    let ctx = p.ctx();
    let ppp = p.pixels_per_point() as f64;
    let s = match max_w {
        Some(m) => fit(ctx, text, st, m),
        None => text.to_string(),
    };
    let line = py_line(ctx, &s, st);
    let (w, h) = (line.w as f64, line.h as f64);
    let (left, top) = place(pos, anchor, w, h, ppp);
    line_at(p, &s, st, left, top, ppp);
    Rect::from_min_size(
        Pos2::new((left / ppp) as f32, (top / ppp) as f32),
        egui::vec2((w / ppp) as f32, (h / ppp) as f32),
    )
}

/// A small-caps section heading: 11-13 points, extra bold, letter-spaced, in the caption colour by default.
pub fn caps(
    p: &Painter,
    text: &str,
    pos: Pos2,
    size: f32,
    color: Color32,
    anchor: Align2,
    max_w: Option<f32>,
) -> Rect {
    draw(p, text, pos, anchor, Style::caps(size, color), max_w)
}

/// Words wrapped into lines no wider than `max_w`.
pub fn wrap(ctx: &egui::Context, text: &str, st: Style, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let trial = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if cur.is_empty() || width(ctx, &trial, st) <= max_w {
            cur = trial;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Text in several styles on one line, each run placed after the last (as the Python app blits them one after
/// another), all on the first run's baseline, the line anchored at `pos`. Returns the line's box.
pub fn draw_runs(p: &Painter, runs: &[(&str, Style)], pos: Pos2, anchor: Align2) -> Rect {
    let ctx = p.ctx();
    let ppp = p.pixels_per_point() as f64;
    let Some(&(t0, s0)) = runs.first() else {
        return Rect::from_min_size(pos, egui::Vec2::ZERO);
    };
    let first = py_line(ctx, t0, s0);
    let (base, h) = (first.base as f64, first.h as f64);
    let lines: Vec<Arc<pyfont::Line>> = runs.iter().map(|(t, st)| py_line(ctx, t, *st)).collect();
    let w: f64 = lines.iter().map(|l| l.w as f64).sum();
    let (left, top) = place(pos, anchor, w, h, ppp);
    let mut x = left;
    for ((t, st), l) in runs.iter().zip(&lines) {
        line_at(p, t, *st, x, top + base - l.base as f64, ppp);
        x += l.w as f64;
    }
    Rect::from_min_size(
        Pos2::new((left / ppp) as f32, (top / ppp) as f32),
        egui::vec2((w / ppp) as f32, (h / ppp) as f32),
    )
}
