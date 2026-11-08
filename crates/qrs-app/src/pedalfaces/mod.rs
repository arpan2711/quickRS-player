//! How a pedalboard block looks (PLAN.md P14.3-P14.4): a stompbox in its enclosure colour (powder coat, a bevel,
//! screws, a chrome footswitch), an amp's control panel on tolex (Fender blackface, Marshall gold, tweed, Vox red,
//! Mesa diamond plate, Ampeg silver), a cab's grille with the mic in front of a speaker, a rack unit in brushed
//! aluminium with a readout, a graphic EQ's sliders. The bodies are drawn with tiny-skia once per size and cached
//! (`paint::Store::Face`); knobs, faders, switches, the LED and the print are drawn live on top, so turning a knob
//! costs nothing. `layout` says where each control is, for drawing and for the mouse.

use std::hash::{Hash, Hasher};

use eframe::egui;
use egui::epaint::Shadow;
use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use qrs_audio::board::{Block, Face, Kind, KnobDef, Unit, catalog};
use qrs_ui::knobs::{self, KnobStyle, Scale};
use qrs_ui::paint::{self, Store};
use qrs_ui::raster::{self, Image};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, RadialGradient,
    Rect as SkRect, SpreadMode, Transform,
};

// -- colours --------------------------------------------------------------------------------------------------------

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn luminance(c: [u8; 3]) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
}

/// The print on an enclosure: dark on light colours, light on dark ones.
pub fn ink(c: [u8; 3]) -> Color32 {
    if luminance(c) > 0.55 {
        Color32::from_rgb(27, 28, 31)
    } else {
        Color32::from_rgb(244, 244, 241)
    }
}

pub(crate) fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    let f = |v: u8| -> u8 {
        if k >= 0.0 {
            (v as f32 + (255.0 - v as f32) * k).round().clamp(0.0, 255.0) as u8
        } else {
            (v as f32 * (1.0 + k)).round().clamp(0.0, 255.0) as u8
        }
    };
    [f(c[0]), f(c[1]), f(c[2])]
}

pub(crate) fn sk(c: [u8; 3], a: f32) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub(crate) fn solid(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

pub(crate) fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.quad_to(x + w, y, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.quad_to(x + w, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.quad_to(x, y + h, x, y + h - r);
    b.line_to(x, y + r);
    b.quad_to(x, y, x + r, y);
    b.close();
    b.finish()
}

pub(crate) fn linear(p0: (f32, f32), p1: (f32, f32), stops: Vec<(f32, Color)>) -> Option<Paint<'static>> {
    let stops = stops.into_iter().map(|(o, c)| GradientStop::new(o, c)).collect();
    let shader = LinearGradient::new(
        Point::from_xy(p0.0, p0.1),
        Point::from_xy(p1.0, p1.1),
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    )?;
    Some(Paint {
        shader,
        anti_alias: true,
        ..Default::default()
    })
}

pub(crate) fn radial(c: (f32, f32), r: f32, stops: Vec<(f32, Color)>) -> Option<Paint<'static>> {
    let stops: Vec<GradientStop> = stops.into_iter().map(|(o, c)| GradientStop::new(o, c)).collect();
    let shader = RadialGradient::new(
        Point::from_xy(c.0, c.1),
        0.0,
        Point::from_xy(c.0, c.1),
        r,
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    )?;
    Some(Paint {
        shader,
        anti_alias: true,
        ..Default::default()
    })
}

/// A tiny repeatable random number (powder coat, tolex grain).
pub(crate) fn hash01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

// -- the print font ---------------------------------------------------------------------------------------------------

/// Bahnschrift (Windows' DIN-like face), the pedal print; Manrope's heaviest where there's no such font.
fn print_family(ctx: &egui::Context) -> Option<FontFamily> {
    use egui::FontData;
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};
    const NAME: &str = "pedal-print";
    static BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    static ASKED: AtomicU64 = AtomicU64::new(u64::MAX);
    let bytes = BYTES.get_or_init(|| {
        let dir = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        std::fs::read(dir.join("Fonts").join("bahnschrift.ttf")).ok()
    });
    let bytes = bytes.as_ref()?;
    let family = FontFamily::Name(NAME.into());
    if ctx.fonts(|f| f.families().contains(&family)) {
        return Some(family);
    }
    let pass = ctx.cumulative_pass_nr();
    if ASKED.swap(pass, Ordering::Relaxed) != pass {
        ctx.add_font(FontInsert::new(
            NAME,
            FontData::from_owned(bytes.clone()),
            vec![InsertFontFamily {
                family,
                priority: FontPriority::Highest,
            }],
        ));
    }
    None
}

/// A pedal's name in its print, fitted into `max_w`.
pub fn print(p: &Painter, s: &str, pos: Pos2, anchor: Align2, size: f32, color: Color32, max_w: f32) -> Rect {
    let ctx = p.ctx();
    match print_family(ctx) {
        Some(family) => {
            let mut sz = size;
            let up = s.to_uppercase();
            let mut g =
                ctx.fonts_mut(|f| f.layout_no_wrap(up.clone(), FontId::new(sz, family.clone()), color));
            while g.size().x > max_w && sz > 7.0 {
                sz -= 0.5;
                g = ctx.fonts_mut(|f| f.layout_no_wrap(up.clone(), FontId::new(sz, family.clone()), color));
            }
            let r = anchor.anchor_size(pos, g.size());
            // a faux bold, as the print is heavy
            p.galley(r.min, g.clone(), color);
            p.galley(r.min + vec2(0.45, 0.0), g, color);
            r
        }
        None => text::draw(
            p,
            &s.to_uppercase(),
            pos,
            anchor,
            Style::new(size * 0.85, 800, color),
            Some(max_w),
        ),
    }
}

// -- logo lettering (P21) -----------------------------------------------------------------------------------------------

/// A logo's style of letters, in the spirit of an era (never a real maker's mark). All OFL fonts in assets/fonts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Lettering {
    /// A flowing 50s/60s script (Damion).
    Script,
    /// A brushed, slanted script (Kaushan Script).
    Brush,
    /// A heavy grotesque, 70s pedal print (Archivo Black).
    Heavy,
    /// Tall condensed capitals (Bebas Neue).
    Condensed,
    /// Engraved roman capitals, a brass plate (Cinzel).
    Engraved,
    /// Wide techno capitals, 80s and rack gear (Michroma).
    Wide,
    /// A rounded 70s display face (Righteous).
    Rounded,
    /// The pedal print as today (Bahnschrift, else Manrope).
    Print,
}

impl Lettering {
    fn font(self) -> Option<(&'static str, &'static [u8])> {
        Some(match self {
            Lettering::Script => (
                "logo-script",
                include_bytes!("../../../../assets/fonts/Damion-Regular.ttf"),
            ),
            Lettering::Brush => (
                "logo-brush",
                include_bytes!("../../../../assets/fonts/KaushanScript-Regular.ttf"),
            ),
            Lettering::Heavy => (
                "logo-heavy",
                include_bytes!("../../../../assets/fonts/ArchivoBlack-Regular.ttf"),
            ),
            Lettering::Condensed => (
                "logo-condensed",
                include_bytes!("../../../../assets/fonts/BebasNeue-Regular.ttf"),
            ),
            Lettering::Engraved => (
                "logo-engraved",
                include_bytes!("../../../../assets/fonts/Cinzel-Variable.ttf"),
            ),
            Lettering::Wide => (
                "logo-wide",
                include_bytes!("../../../../assets/fonts/Michroma-Regular.ttf"),
            ),
            Lettering::Rounded => (
                "logo-rounded",
                include_bytes!("../../../../assets/fonts/Righteous-Regular.ttf"),
            ),
            Lettering::Print => return None,
        })
    }
}

/// The font family for a lettering, once it's loaded (it's added to the context on first use and ready a frame
/// later; until then None).
fn lettering_family(ctx: &egui::Context, l: Lettering) -> Option<FontFamily> {
    use egui::FontData;
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    let (name, bytes) = l.font()?;
    let family = FontFamily::Name(name.into());
    if ctx.fonts(|f| f.families().contains(&family)) {
        return Some(family);
    }
    ctx.add_font(FontInsert::new(
        name,
        FontData::from_static(bytes),
        vec![InsertFontFamily {
            family,
            priority: FontPriority::Highest,
        }],
    ));
    None
}

/// A logo or name in a lettering, shrunk to fit `max_w`. `caps` upper-cases it (the script styles read best as
/// written). Falls back to the pedal print while the font loads.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lettering(
    p: &Painter,
    s: &str,
    pos: Pos2,
    anchor: Align2,
    size: f32,
    color: Color32,
    max_w: f32,
    style: Lettering,
    caps: bool,
) -> Rect {
    let ctx = p.ctx();
    let s = if caps { s.to_uppercase() } else { s.to_string() };
    match lettering_family(ctx, style) {
        Some(family) => {
            let mut sz = size;
            let lay = |sz: f32| {
                ctx.fonts_mut(|f| f.layout_no_wrap(s.clone(), FontId::new(sz, family.clone()), color))
            };
            let mut g = lay(sz);
            while g.size().x > max_w && sz > 6.0 {
                sz -= 0.5;
                g = lay(sz);
            }
            let r = anchor.anchor_size(pos, g.size());
            p.galley(r.min, g, color);
            r
        }
        None => print(p, &s, pos, anchor, size, color, max_w),
    }
}

mod amps;
mod cabs;
mod pedals;
/// The drawing kit, for other realistic faces (the tuner's, P48.3).
pub(crate) use pedals::kit;
mod racks;
pub mod silver;

// -- families -------------------------------------------------------------------------------------------------------

/// Which painter draws a model: each family has its own file (layout, body, controls, board size), so they can be
/// redrawn apart (PLAN.md P21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    Amp,
    Cab,
    /// Stompboxes, consoles (tape echo, vibe), the rotary cabinet.
    Pedal,
    /// Rack units and graphic EQs.
    Rack,
    /// The generic brushed-silver unit (`silver`): file blocks whose skin doesn't map every control (D101).
    Silver,
}

pub(crate) fn family(def: &qrs_audio::board::ModelDef) -> Family {
    if def.file {
        return Family::Silver;
    }
    match def.face {
        Face::Amp => Family::Amp,
        Face::Cab => Family::Cab,
        // (a pedal with switches and no drawn face of its own is the silver unit: the old generic face piled its rows
        // of switches on its knobs, P37.13)
        Face::Pedal
            if !pedals::has_spec(def.id)
                && def.knobs[..def.front_len()]
                    .iter()
                    .any(|k| matches!(k.unit, Unit::Choice(_))) =>
        {
            Family::Silver
        }
        Face::Pedal => Family::Pedal,
        Face::Rack | Face::Graphic => Family::Rack,
    }
}

/// How a skin's front controls map onto a file block's real ones (D101): skin Gain (drive, distortion...) is the
/// capture's Input, Master / Volume / Level its Output, a tone knob the block's knob of that name (a post-capture
/// EQ, when it has one). For each skin front control, the file block's knob it turns. None unless every skin control
/// maps AND every front knob of the file block is reached: a face shows only controls that turn something, and
/// every real control is on it.
pub fn skin_map(skin: &qrs_audio::board::ModelDef, own: &qrs_audio::board::ModelDef) -> Option<Vec<usize>> {
    let find = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| own.front_knobs().iter().position(|x| x.key == *k))
    };
    let mut out: Vec<usize> = Vec::new();
    for kd in skin.front_knobs() {
        let j = match kd.key {
            "gain" | "drive" | "dist" | "fuzz" | "sustain" => find(&["input"]),
            "master" | "volume" | "level" | "output" => find(&["output", "level"]),
            "middle" => find(&["mid"]),
            k => find(&[k]),
        }?;
        if out.contains(&j) {
            return None;
        }
        out.push(j);
    }
    (out.len() == own.front_len()).then_some(out)
}

/// A NAM capture or IR drawn as the model of ours it stands in for (P33.3, its library item's `skin`), only where
/// every control of the skin maps to a real one (`skin_map`), its knobs at the block's values; None for the rest
/// (drawn as the brushed-silver unit, D101).
pub fn skinned(b: &Block) -> Option<Block> {
    if !b.def().file {
        return None;
    }
    let skin = crate::captures::with(|l| l.get(b.asset).map(|i| i.skin.clone()))?;
    let mut s = Block::of(skin.as_deref()?)?;
    if s.def().file {
        return None;
    }
    let map = skin_map(s.def(), b.def())?;
    s.on = b.on;
    s.uid = b.uid;
    let (sd, od) = (s.def(), b.def());
    for (i, &j) in map.iter().enumerate() {
        s.v[i] = sd.knobs[i].from_u(od.knobs[j].to_u(b.v[j]));
    }
    Some(s)
}

/// The face a block is drawn with (a skinned capture's: its skin's).
pub fn face_of(b: &Block) -> Face {
    skinned(b).map_or(b.def().face, |s| s.def().face)
}

/// Whether a block is drawn as another (its skin's controls stand for its own; `layout` gives its own indices).
#[cfg(test)]
pub fn is_skinned(b: &Block) -> bool {
    skinned(b).is_some()
}

/// Where everything on a block's face goes, in `r`. A skinned block's indices are its own (the skin's mapped through
/// `skin_map`), so every caller turns real knobs.
pub fn layout(b: &Block, r: Rect, detail: Detail) -> Layout {
    let s = skinned(b);
    let Some(sk) = s.as_ref() else {
        let l = family_layout(b, r, detail);
        let missing = unplaced(b, &l);
        if missing.is_empty() || detail != Detail::Full {
            return l;
        }
        // (its other real knobs beside the face when the rect has the width, else on a strip under it: P37.13)
        let (face, area) = extra_split(b, r, missing.len());
        let mut l = family_layout(b, face, detail);
        for (&k, (c, d, _)) in missing.iter().zip(extra_cells(area, missing.len())) {
            l.extra.push((k, c, d));
        }
        l.extra_area = area;
        return l;
    };
    let mut l = family_layout(sk, r, detail);
    if let Some(map) = skin_map(sk.def(), b.def()) {
        let to = |i: usize| map.get(i).copied().unwrap_or(i);
        for k in &mut l.knobs {
            k.0 = to(k.0);
        }
        for k in &mut l.faders {
            k.0 = to(k.0);
        }
        for k in &mut l.choices {
            k.0 = to(k.0);
        }
    }
    l
}

/// Front knobs a layout doesn't place.
fn unplaced(b: &Block, l: &Layout) -> Vec<usize> {
    (0..b.def().front_len())
        .filter(|i| {
            !l.knobs.iter().any(|k| k.0 == *i)
                && !l.faders.iter().any(|k| k.0 == *i)
                && !l.choices.iter().any(|k| k.0 == *i)
        })
        .collect()
}

/// The extras' cell at a face's own size (P37.13): a knob with its name under it.
pub const EXTRA_W: f32 = 76.0;
pub const EXTRA_H: f32 = 64.0;

/// How many columns `n` extras take beside a face `h` high (at its own size, 190 points for a pedal).
fn extra_cols(n: usize, rows_fit: usize) -> usize {
    n.div_ceil(rows_fit.max(1)).max(1)
}

/// A face with extra knobs, split: the face and the extras' area. Beside the face (a column at the right) when `r`
/// is as wide as the face and that column at its height; else a strip under the face.
pub fn extra_split(b: &Block, r: Rect, n: usize) -> (Rect, Rect) {
    let bs = board_size(b);
    let k = r.height() / bs.y;
    let rows_fit = ((bs.y / EXTRA_H).floor() as usize).max(1);
    let col_w = extra_cols(n, rows_fit) as f32 * EXTRA_W * k;
    if r.width() >= (bs.x * k + col_w) * 0.97 {
        let x = r.max.x - col_w;
        (
            Rect::from_min_max(r.min, pos2(x, r.max.y)),
            Rect::from_min_max(pos2(x, r.min.y), r.max),
        )
    } else {
        let sh = (r.height() * EXTRA_H / (bs.y + EXTRA_H)).max(30.0);
        (
            Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - sh)),
            Rect::from_min_max(pos2(r.min.x, r.max.y - sh), r.max),
        )
    }
}

/// The extras' plate in their area and each one's (centre, diameter, cell width): a grid, one row in a strip, as many
/// rows as fit in a column.
fn extra_cells(area: Rect, n: usize) -> Vec<(Pos2, f32, f32)> {
    let plate = extra_plate(area, n);
    let tall = area.height() > area.width();
    let (cols, rows) = if tall {
        let rows_fit = ((plate.height() / (EXTRA_H * 0.9)).floor() as usize).clamp(1, n.max(1));
        let cols = extra_cols(n, rows_fit);
        (cols, n.div_ceil(cols))
    } else {
        (n.max(1), 1)
    };
    let cw = plate.width() / cols as f32;
    let ch = (plate.height() / rows as f32).min(EXTRA_H * 1.4 * plate.width() / (cols as f32 * EXTRA_W));
    // (room at the top for the plate's "ADDED" caption, P37.6c)
    let top = plate.center().y - ch * rows as f32 / 2.0 + ch * 0.08;
    let d = (ch * 0.5).min(cw * 0.42).max(10.0);
    (0..n)
        .map(|j| {
            let (cx, cy) = (j % cols, j / cols);
            let x = plate.min.x + cw * (cx as f32 + 0.5);
            let y = top + ch * cy as f32 + ch * 0.1 + d * 0.5;
            (pos2(x, y), d, cw)
        })
        .collect()
}

/// The extras' dark plate in their area: as big as their grid needs, centred.
fn extra_plate(area: Rect, n: usize) -> Rect {
    let m = (area.width().min(area.height()) * 0.06).clamp(2.0, 8.0);
    let a = area.shrink(m);
    if a.height() > a.width() {
        a
    } else {
        let k = a.height() / EXTRA_H;
        Rect::from_center_size(
            a.center(),
            vec2((n as f32 * EXTRA_W * k).min(a.width()), a.height()),
        )
    }
}

/// A pedal without a designed face, at the size its controls need in Full (two rows of knobs past four, each choice
/// a row with its name beside it).
fn legacy_pedal_size(b: &Block) -> egui::Vec2 {
    let def = b.def();
    let n = (0..def.front_len())
        .filter(|&i| !matches!(def.knobs[i].unit, Unit::Choice(_)))
        .count()
        .max(1);
    let c = def.front_len() - n.min(def.front_len());
    let per = if n > 4 { n.div_ceil(2) } else { n };
    let rows = if n > 4 { 2.0 } else { 1.0 };
    let w = (per as f32 * 40.0 + 24.0)
        .max(if c > 0 { 230.0 } else { 130.0 })
        .max(130.0);
    let d = ((w - 16.0) / per as f32 * 0.72).min(40.0);
    let h = 20.0 + rows * (d + 16.0) + c as f32 * 22.0 + 110.0;
    vec2(w, h.max(190.0))
}

/// The open unit's face size (P37.13; Full detail, points): what every control and label needs, so a panel sized
/// from it never piles knobs or labels on each other. A cab is wider for its engineer's strip, a face with extra knobs
/// (a drive's Blend and Clean low) wider for their column.
pub fn unit_size(b: &Block) -> egui::Vec2 {
    let s = skinned(b);
    let drawn = s.as_ref().unwrap_or(b);
    let mut sz = match family(drawn.def()) {
        Family::Cab => cabs::unit_size(drawn),
        Family::Pedal if drawn.def().face == Face::Pedal && !pedals::designed(drawn) => {
            legacy_pedal_size(drawn)
        }
        _ => board_size(drawn),
    };
    if s.is_none() {
        let l = family_layout(b, Rect::from_min_size(Pos2::ZERO, sz), Detail::Full);
        let n = unplaced(b, &l).len();
        if n > 0 {
            let rows_fit = ((sz.y / EXTRA_H).floor() as usize).max(1);
            sz.x += extra_cols(n, rows_fit) as f32 * EXTRA_W;
        }
    }
    sz
}

/// The layout in the drawn block's own terms (a skin's indices for a skin).
fn family_layout(b: &Block, r: Rect, detail: Detail) -> Layout {
    match family(b.def()) {
        Family::Amp if amps::designed(b.def().id) => amps::layout(b, r, detail),
        Family::Amp => legacy_layout(b, r, detail),
        Family::Cab => cabs::layout(b, r, detail),
        Family::Pedal => pedals::layout(b, r, detail),
        Family::Rack => racks::layout(b, r, detail),
        Family::Silver => silver::layout(b, r, detail),
    }
}

/// A block's size standing on the big view's board (points, before the board scales them all to fit).
pub fn board_size(b: &Block) -> egui::Vec2 {
    let s = skinned(b);
    let b = s.as_ref().unwrap_or(b);
    match family(b.def()) {
        Family::Amp if amps::designed(b.def().id) => amps::board_size(b),
        Family::Amp => legacy_board_size(b),
        Family::Cab => cabs::board_size(b),
        Family::Pedal => pedals::board_size(b),
        Family::Rack => racks::board_size(b),
        Family::Silver => silver::board_size(b),
    }
}

/// Today's board sizes (each family's `board_size` until it sets its own).
pub(super) fn legacy_board_size(b: &Block) -> egui::Vec2 {
    match b.def().face {
        Face::Amp => vec2(250.0, 150.0),
        Face::Cab => vec2(170.0, 196.0),
        Face::Rack => vec2(240.0, 116.0),
        Face::Graphic => vec2(if b.def().knobs.len() > 6 { 200.0 } else { 150.0 }, 190.0),
        Face::Pedal => vec2(130.0, 190.0),
    }
}

// -- layout ---------------------------------------------------------------------------------------------------------

/// Where a face's controls are (points).
#[derive(Clone, Debug)]
pub struct Layout {
    /// (knob index, centre, diameter).
    pub knobs: Vec<(usize, Pos2, f32)>,
    /// (knob index, the whole switch's rect): a choice drawn as a row of options.
    pub choices: Vec<(usize, Rect)>,
    /// (knob index, the fader's track).
    pub faders: Vec<(usize, Rect)>,
    pub name: Rect,
    pub led: Pos2,
    /// The footswitch (or an amp's standby, a rack's bypass key).
    pub switch: Rect,
    /// An amp's control panel, a cab's grille, a rack's readout.
    pub panel: Rect,
    /// A cab's speaker the mic points at: (centre, radius).
    pub cone: Option<(Pos2, f32)>,
    pub radius: f32,
    /// Front knobs the designed face has no place for (a drive's Blend and Clean low, P37.2): (knob, centre,
    /// diameter) on a strip under the face, so every real control turns on it (D101). Empty on most faces.
    pub extra: Vec<(usize, Pos2, f32)>,
    /// Where the extras sit (beside the face or under it); nothing without extras.
    pub extra_area: Rect,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            knobs: Vec::new(),
            choices: Vec::new(),
            faders: Vec::new(),
            name: Rect::NOTHING,
            led: Pos2::ZERO,
            switch: Rect::NOTHING,
            panel: Rect::NOTHING,
            cone: None,
            radius: 8.0,
            extra: Vec::new(),
            extra_area: Rect::NOTHING,
        }
    }
}

/// How much a face shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Detail {
    /// The tile's face, or the editor's: every control, labels.
    Full,
    /// A pedal standing on the big view's board: smaller labels.
    Board,
}

fn row(n: usize, x0: f32, x1: f32, y: f32, _d: f32) -> Vec<Pos2> {
    if n == 0 {
        return vec![];
    }
    let step = (x1 - x0) / n as f32;
    (0..n).map(|i| pos2(x0 + step * (i as f32 + 0.5), y)).collect()
}

/// Where everything on a block's face goes, in `r`.
pub(super) fn legacy_layout(b: &Block, r: Rect, detail: Detail) -> Layout {
    let def = b.def();
    let (w, h) = (r.width(), r.height());
    let mut l = Layout {
        radius: (w.min(h) * 0.07).clamp(6.0, 14.0),
        ..Default::default()
    };
    let dials: Vec<usize> = (0..def.front_len())
        .filter(|&i| !matches!(def.knobs[i].unit, Unit::Choice(_)))
        .collect();
    let choices: Vec<usize> = (0..def.front_len())
        .filter(|&i| matches!(def.knobs[i].unit, Unit::Choice(_)))
        .collect();
    match def.face {
        Face::Pedal => {
            let n = dials.len().max(1);
            let rows = if n > 4 { 2 } else { 1 };
            let per = n.div_ceil(rows);
            let d = ((w - 16.0) / per as f32 * 0.72)
                .min(h * if rows == 2 { 0.17 } else { 0.27 })
                .clamp(14.0, 50.0);
            let top = r.min.y + h * 0.06 + d * 0.5 + 4.0;
            for (k, chunk) in dials.chunks(per).enumerate() {
                let y = top + k as f32 * (d + 16.0);
                for (i, c) in chunk
                    .iter()
                    .zip(row(chunk.len(), r.min.x + 8.0, r.max.x - 8.0, y, d))
                {
                    l.knobs.push((*i, c, d));
                }
            }
            let below = top + (rows as f32 - 1.0) * (d + 16.0) + d * 0.5 + 14.0;
            let mut y = below;
            // (each choice a row, its name in a column at its left inside the face, P37.13)
            let lab_w = if detail == Detail::Full {
                (w * 0.3).min(64.0)
            } else {
                0.0
            };
            for &c in &choices {
                let cw = (w - 16.0 - lab_w).min(170.0);
                let cx = r.max.x - 8.0 - cw * 0.5;
                let cx = if lab_w > 0.0 { cx } else { r.center().x };
                l.choices
                    .push((c, Rect::from_center_size(pos2(cx, y + 8.0), vec2(cw, 16.0))));
                y += 22.0;
            }
            let sw = (w * 0.2).clamp(18.0, 36.0);
            l.switch =
                Rect::from_center_size(pos2(r.center().x, r.max.y - sw * 0.5 - h * 0.06), vec2(sw, sw));
            l.led = pos2(r.center().x, l.switch.min.y - (h * 0.06).clamp(6.0, 12.0));
            let name_h = (h * 0.13).clamp(12.0, 26.0);
            let name_y = ((y + l.led.y) * 0.5).max(y + name_h * 0.5);
            l.name = Rect::from_center_size(pos2(r.center().x, name_y), vec2(w - 16.0, name_h));
            l.panel = r;
        }
        Face::Graphic => {
            let n = def.front_len();
            let top = r.min.y + h * 0.1 + 6.0;
            let bottom = r.max.y - h * 0.28;
            let x0 = r.min.x + 12.0;
            let x1 = r.max.x - 12.0;
            for (i, c) in row(n, x0, x1, 0.0, 0.0).into_iter().enumerate() {
                l.faders.push((
                    i,
                    Rect::from_min_max(pos2(c.x - 4.0, top), pos2(c.x + 4.0, bottom)),
                ));
            }
            let sw = (h * 0.13).clamp(14.0, 26.0);
            l.switch = Rect::from_center_size(
                pos2(r.max.x - sw * 0.5 - 12.0, r.max.y - sw * 0.5 - 8.0),
                vec2(sw, sw),
            );
            l.led = pos2(l.switch.min.x - 14.0, l.switch.center().y);
            l.name = Rect::from_min_max(
                pos2(r.min.x + 12.0, l.switch.min.y),
                pos2(l.led.x - 10.0, l.switch.max.y),
            );
            l.panel = r;
        }
        Face::Amp => {
            let head = (h * 0.2).clamp(14.0, 30.0);
            let foot = (h * 0.14).clamp(10.0, 22.0);
            l.panel = Rect::from_min_max(
                pos2(r.min.x + 10.0, r.min.y + head + 6.0),
                pos2(r.max.x - 10.0, r.max.y - foot - 6.0),
            );
            let n = dials.len() + choices.len();
            let d = (l.panel.width() / n.max(1) as f32 * 0.62)
                .min(l.panel.height() * 0.56)
                .clamp(14.0, 44.0);
            let y = l.panel.min.y + l.panel.height() * 0.42;
            let spots = row(n, l.panel.min.x + 4.0, l.panel.max.x - 4.0, y, d);
            // (every control has its spot along the row, in the catalogue's order)
            for (i, (kd, &c)) in def.front_knobs().iter().zip(&spots).enumerate() {
                if matches!(kd.unit, Unit::Choice(_)) {
                    l.choices
                        .push((i, Rect::from_center_size(c, vec2(d * 0.62, d * 0.9))));
                } else {
                    l.knobs.push((i, c, d));
                }
            }
            l.name = Rect::from_min_max(
                pos2(r.min.x + 16.0, r.min.y + 4.0),
                pos2(r.max.x - 40.0, r.min.y + head + 4.0),
            );
            l.led = pos2(r.max.x - 22.0, r.min.y + head * 0.5 + 4.0);
            let sw = vec2((w * 0.16).clamp(30.0, 52.0), foot.min(16.0));
            l.switch = Rect::from_min_size(
                pos2(r.max.x - 14.0 - sw.x, r.max.y - foot * 0.5 - sw.y * 0.5 - 4.0),
                sw,
            );
        }
        Face::Cab => {
            let pad = (w.min(h) * 0.05).clamp(5.0, 10.0);
            let gw = if detail == Detail::Board {
                w - 2.0 * pad
            } else {
                (w * 0.46).min(h * 1.05)
            };
            let gh = h - 2.0 * pad;
            l.panel = Rect::from_min_size(pos2(r.min.x + pad, r.min.y + pad), vec2(gw, gh));
            let cones = cones(def.id, l.panel);
            l.cone = cones.first().copied();
            l.name = Rect::from_center_size(
                pos2(l.panel.center().x, l.panel.max.y - 11.0),
                vec2(gw - 10.0, 14.0),
            );
            if detail == Detail::Full {
                // the controls beside the grille: mic place and distance as faders across, mic type, cuts
                let x0 = l.panel.max.x + 14.0;
                let x1 = r.max.x - 12.0;
                let cw = x1 - x0;
                let mut y = r.min.y + 24.0;
                for i in [0usize, 1] {
                    l.faders
                        .push((i, Rect::from_min_size(pos2(x0, y), vec2(cw, 8.0))));
                    y += 38.0;
                }
                l.choices
                    .push((2, Rect::from_min_size(pos2(x0, y - 4.0), vec2(cw, 18.0))));
                y += 30.0;
                let d = ((r.max.y - y) * 0.6).clamp(16.0, 30.0);
                for (k, c) in [3usize, 4].iter().zip(row(2, x0, x1, y + d * 0.5, d)) {
                    l.knobs.push((*k, c, d));
                }
            }
            l.led = pos2(r.max.x - 10.0, r.min.y + 10.0);
            l.switch = Rect::NOTHING;
        }
        Face::Rack => {
            let ear = (w * 0.06).clamp(12.0, 22.0);
            let top = (h * 0.24).clamp(16.0, 30.0);
            l.panel = Rect::from_min_max(
                pos2(r.min.x + ear + 70.0, r.min.y + 8.0),
                pos2(r.max.x - ear - 26.0, r.min.y + top),
            );
            l.name = Rect::from_min_max(
                pos2(r.min.x + ear + 4.0, r.min.y + 6.0),
                pos2(l.panel.min.x - 6.0, r.min.y + top + 2.0),
            );
            l.led = pos2(r.max.x - ear - 12.0, r.min.y + 8.0 + (top - 8.0) * 0.5);
            let n = dials.len().max(1);
            let rows = if n > 5 && detail == Detail::Board { 2 } else { 1 };
            let per = n.div_ceil(rows);
            let area_top = r.min.y + top + 8.0;
            let area_bot = r.max.y - 20.0;
            let d = ((w - 2.0 * ear) / per as f32 * 0.5)
                .min((area_bot - area_top) / rows as f32 * 0.55)
                .clamp(12.0, 34.0);
            for (k, chunk) in dials.chunks(per).enumerate() {
                let y = area_top + (k as f32 + 0.42) * (area_bot - area_top) / rows as f32;
                for (i, c) in
                    chunk
                        .iter()
                        .zip(row(chunk.len(), r.min.x + ear + 4.0, r.max.x - ear - 4.0, y, d))
                {
                    l.knobs.push((*i, c, d));
                }
            }
            let sw = vec2(52.0, 14.0);
            l.switch = Rect::from_min_size(pos2(r.max.x - ear - 6.0 - sw.x, r.max.y - sw.y - 5.0), sw);
        }
    }
    l
}

/// A cab's speakers in its grille: (centre, radius).
fn cones(id: &str, g: Rect) -> Vec<(Pos2, f32)> {
    let (w, h) = (g.width(), g.height());
    let c = |fx: f32, fy: f32, fr: f32| (pos2(g.min.x + w * fx, g.min.y + h * fy), w.min(h) * fr);
    match id {
        "cab_1x10" => vec![c(0.5, 0.46, 0.3)],
        "cab_1x12" => vec![c(0.5, 0.46, 0.36)],
        "cab_1x15" => vec![c(0.5, 0.46, 0.42)],
        "cab_2x12" => vec![c(0.27, 0.46, 0.22), c(0.73, 0.46, 0.22)],
        "cab_4x12" => vec![
            c(0.28, 0.28, 0.21),
            c(0.72, 0.28, 0.21),
            c(0.28, 0.7, 0.21),
            c(0.72, 0.7, 0.21),
        ],
        "cab_8x10" => (0..8)
            .map(|i| c(0.28 + 0.44 * (i % 2) as f32, 0.12 + 0.24 * (i / 2) as f32, 0.11))
            .collect(),
        _ => vec![c(0.5, 0.5, 0.35)],
    }
}

// -- bodies ---------------------------------------------------------------------------------------------------------

fn key(parts: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// The amp's panel look.
fn amp_panel(id: &str) -> (&'static str, [u8; 3]) {
    match id {
        "amp_clean" => ("blackface", [32, 32, 34]),
        "amp_tweed" => ("tweed", [204, 172, 112]),
        "amp_chime" => ("vox", [150, 52, 46]),
        "amp_crunch" | "amp_lead" => ("gold", [214, 180, 96]),
        "amp_modern" => ("plate", [176, 182, 190]),
        "amp_cali" | "amp_surrey" => ("black", [30, 30, 32]),
        "amp_snake" => ("chrome", [200, 204, 208]),
        "amp_tangerine" => ("cream", [236, 228, 208]),
        _ => ("silver", [190, 198, 208]),
    }
}

fn grille(id: &str) -> [u8; 3] {
    match id {
        "cab_4x12" => [62, 59, 51],
        "cab_8x10" | "cab_1x15" => [40, 42, 46],
        _ => [176, 160, 120],
    }
}

/// The body of a face, drawn at `ppp` pixels a point, `r` in points (the texture covers `r`).
pub(super) fn legacy_body(b: &Block, r: Rect, l: &Layout, ppp: f32) -> Image {
    let def = b.def();
    let (w, h) = (
        (r.width() * ppp).round().max(2.0) as u32,
        (r.height() * ppp).round().max(2.0) as u32,
    );
    let mut pm = match Pixmap::new(w, h) {
        Some(p) => p,
        None => return Image::new(1, 1),
    };
    let s = ppp;
    let lp = |p: Pos2| ((p.x - r.min.x) * s, (p.y - r.min.y) * s);
    let lr = |q: Rect| {
        (
            (q.min.x - r.min.x) * s,
            (q.min.y - r.min.y) * s,
            q.width() * s,
            q.height() * s,
        )
    };
    let (fw, fh) = (w as f32, h as f32);
    let rad = l.radius * s;
    let col = def.colour;
    let tr = Transform::identity();
    match def.face {
        Face::Pedal | Face::Graphic => {
            let Some(path) = rrect(0.5, 0.5, fw - 1.0, fh - 1.0, rad) else {
                return Image::new(1, 1);
            };
            if let Some(p) = linear(
                (0.0, 0.0),
                (fw * 0.6, fh),
                vec![
                    (0.0, sk(shade(col, 0.2), 1.0)),
                    (0.45, sk(col, 1.0)),
                    (1.0, sk(shade(col, -0.28), 1.0)),
                ],
            ) {
                pm.fill_path(&path, &p, FillRule::Winding, tr, None);
            }
            // powder coat
            let n = (fw * fh / 60.0) as u32;
            for i in 0..n {
                let (x, y) = (hash01(i * 3) * fw, hash01(i * 3 + 1) * fh);
                let lighter = hash01(i * 3 + 2) > 0.5;
                let c = if lighter {
                    sk([255, 255, 255], 0.05)
                } else {
                    sk([0, 0, 0], 0.06)
                };
                pm.fill_rect(
                    SkRect::from_xywh(x, y, s.max(1.0), s.max(1.0))
                        .unwrap_or(SkRect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                    &solid(c),
                    tr,
                    None,
                );
            }
            // a bevel: light along the top, shade along the bottom, a dark outline
            if let Some(p) = linear(
                (0.0, 0.0),
                (0.0, fh),
                vec![
                    (0.0, sk([255, 255, 255], 0.32)),
                    (0.06, sk([255, 255, 255], 0.0)),
                    (0.92, sk([0, 0, 0], 0.0)),
                    (1.0, sk([0, 0, 0], 0.3)),
                ],
            ) {
                pm.fill_path(&path, &p, FillRule::Winding, tr, None);
            }
            pm.stroke_path(
                &path,
                &solid(sk([0, 0, 0], 0.45)),
                &raster::stroke(s.max(1.0)),
                tr,
                None,
            );
            // screws in the corners
            let sr = (rad * 0.42).clamp(2.5 * s, 4.5 * s);
            for (x, y) in [(rad, rad), (fw - rad, rad), (rad, fh - rad), (fw - rad, fh - rad)] {
                if let (Some(c), Some(p)) = (
                    raster::circle(x, y, sr),
                    radial(
                        (x - sr * 0.3, y - sr * 0.3),
                        sr * 1.4,
                        vec![
                            (0.0, sk([250, 250, 250], 1.0)),
                            (0.55, sk([160, 165, 172], 1.0)),
                            (1.0, sk([84, 88, 95], 1.0)),
                        ],
                    ),
                ) {
                    pm.fill_path(&c, &p, FillRule::Winding, tr, None);
                }
                if let Some(slot) =
                    raster::polyline(&[(x - sr * 0.6, y + sr * 0.35), (x + sr * 0.6, y - sr * 0.35)])
                {
                    pm.stroke_path(
                        &slot,
                        &solid(sk([30, 30, 30], 0.7)),
                        &raster::stroke(s * 0.9),
                        tr,
                        None,
                    );
                }
            }
            if def.face == Face::Graphic {
                // the sliders' slots and a centre line
                for (_, track) in &l.faders {
                    let (x, y, tw, th) = lr(*track);
                    if let Some(p) = rrect(x + tw * 0.5 - 2.5 * s, y, 5.0 * s, th, 2.5 * s) {
                        pm.fill_path(&p, &solid(sk([12, 14, 16], 0.92)), FillRule::Winding, tr, None);
                    }
                    if let Some(line) =
                        raster::polyline(&[(x - 3.0 * s, y + th * 0.5), (x + tw + 3.0 * s, y + th * 0.5)])
                    {
                        pm.stroke_path(&line, &solid(sk([0, 0, 0], 0.35)), &raster::stroke(s), tr, None);
                    }
                }
            }
            // the footswitch: a dark ring and a chrome dome
            if l.switch.is_positive() {
                let (cx, cy) = lp(l.switch.center());
                let fr = l.switch.width() * 0.5 * s;
                if let Some(c) = raster::circle(cx, cy, fr * 1.16) {
                    pm.fill_path(&c, &solid(sk([38, 40, 44], 1.0)), FillRule::Winding, tr, None);
                }
                if let (Some(c), Some(p)) = (
                    raster::circle(cx, cy, fr),
                    radial(
                        (cx - fr * 0.25, cy - fr * 0.35),
                        fr * 1.3,
                        vec![
                            (0.0, sk([255, 255, 255], 1.0)),
                            (0.3, sk([214, 217, 221], 1.0)),
                            (0.7, sk([138, 143, 150], 1.0)),
                            (1.0, sk([82, 86, 92], 1.0)),
                        ],
                    ),
                ) {
                    pm.fill_path(&c, &p, FillRule::Winding, tr, None);
                }
            }
        }
        Face::Amp => {
            let Some(path) = rrect(0.5, 0.5, fw - 1.0, fh - 1.0, rad) else {
                return Image::new(1, 1);
            };
            pm.fill_path(&path, &solid(sk([26, 26, 26], 1.0)), FillRule::Winding, tr, None);
            // tolex grain
            let n = (fw * fh / 9.0) as u32;
            for i in 0..n {
                let (x, y) = (hash01(i * 2) * fw, hash01(i * 2 + 1) * fh);
                pm.fill_rect(
                    SkRect::from_xywh(x, y, s.max(1.0), s.max(1.0))
                        .unwrap_or(SkRect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                    &solid(sk([255, 255, 255], 0.045)),
                    tr,
                    None,
                );
            }
            if let Some(p) = linear(
                (0.0, 0.0),
                (0.0, fh),
                vec![
                    (0.0, sk([255, 255, 255], 0.1)),
                    (0.08, sk([255, 255, 255], 0.0)),
                    (1.0, sk([0, 0, 0], 0.3)),
                ],
            ) {
                pm.fill_path(&path, &p, FillRule::Winding, tr, None);
            }
            let (style, pc) = amp_panel(def.id);
            // piping
            let piping = if style == "gold" {
                [214, 190, 120]
            } else {
                [200, 200, 196]
            };
            if let Some(pp) = rrect(5.0 * s, 5.0 * s, fw - 10.0 * s, fh - 10.0 * s, rad * 0.7) {
                pm.stroke_path(&pp, &solid(sk(piping, 0.55)), &raster::stroke(1.2 * s), tr, None);
            }
            let (x, y, pw, ph) = lr(l.panel);
            if let Some(panel) = rrect(x, y, pw, ph, 3.0 * s) {
                if let Some(p) = linear(
                    (0.0, y),
                    (0.0, y + ph),
                    vec![
                        (0.0, sk(shade(pc, 0.18), 1.0)),
                        (0.5, sk(pc, 1.0)),
                        (1.0, sk(shade(pc, -0.22), 1.0)),
                    ],
                ) {
                    pm.fill_path(&panel, &p, FillRule::Winding, tr, None);
                }
                match style {
                    "gold" | "silver" => {
                        // brushed metal: hairlines across
                        let mut yy = y;
                        let mut i = 0;
                        while yy < y + ph {
                            let a = 0.04 + 0.05 * hash01(i);
                            pm.fill_rect(
                                SkRect::from_xywh(x, yy, pw, s.max(1.0) * 0.6)
                                    .unwrap_or(SkRect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                                &solid(sk([255, 255, 255], a)),
                                tr,
                                None,
                            );
                            yy += 2.0 * s;
                            i += 1;
                        }
                    }
                    "tweed" => {
                        let mut xx = x - ph;
                        while xx < x + pw {
                            if let Some(line) = raster::polyline(&[(xx, y + ph), (xx + ph, y)]) {
                                pm.stroke_path(
                                    &line,
                                    &solid(sk([90, 60, 20], 0.16)),
                                    &raster::stroke(1.5 * s),
                                    tr,
                                    Some(&clip(w, h, &panel)),
                                );
                            }
                            xx += 4.0 * s;
                        }
                    }
                    "plate" => {
                        // diamond plate: little raised lozenges
                        let step = 9.0 * s;
                        let mut row_i = 0;
                        let mut yy = y + step * 0.5;
                        while yy < y + ph {
                            let mut xx = x + if row_i % 2 == 0 { step * 0.5 } else { step };
                            while xx < x + pw {
                                if let Some(lz) = raster::polygon(&[
                                    (xx - 3.0 * s, yy + 1.2 * s),
                                    (xx + 3.0 * s, yy - 1.2 * s),
                                ]) {
                                    let _ = lz;
                                }
                                if let Some(line) = raster::polyline(&[
                                    (xx - 2.6 * s, yy + 1.4 * s),
                                    (xx + 2.6 * s, yy - 1.4 * s),
                                ]) {
                                    pm.stroke_path(
                                        &line,
                                        &solid(sk([255, 255, 255], 0.35)),
                                        &raster::stroke(1.6 * s),
                                        tr,
                                        None,
                                    );
                                }
                                xx += step;
                            }
                            yy += step * 0.55;
                            row_i += 1;
                        }
                    }
                    _ => {}
                }
                pm.stroke_path(
                    &panel,
                    &solid(sk([0, 0, 0], 0.6)),
                    &raster::stroke(1.6 * s),
                    tr,
                    None,
                );
            }
            // the jewel light's bezel
            let (jx, jy) = lp(l.led);
            if let Some(c) = raster::circle(jx, jy, 7.0 * s) {
                pm.fill_path(&c, &solid(sk([200, 200, 196], 0.9)), FillRule::Winding, tr, None);
            }
        }
        Face::Cab => {
            let Some(path) = rrect(0.5, 0.5, fw - 1.0, fh - 1.0, rad) else {
                return Image::new(1, 1);
            };
            pm.fill_path(&path, &solid(sk([24, 24, 24], 1.0)), FillRule::Winding, tr, None);
            let n = (fw * fh / 9.0) as u32;
            for i in 0..n {
                let (x, y) = (hash01(i * 2 + 7) * fw, hash01(i * 2 + 8) * fh);
                pm.fill_rect(
                    SkRect::from_xywh(x, y, s.max(1.0), s.max(1.0))
                        .unwrap_or(SkRect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                    &solid(sk([255, 255, 255], 0.045)),
                    tr,
                    None,
                );
            }
            let (x, y, gw, gh) = lr(l.panel);
            let gc = grille(def.id);
            if let Some(g) = rrect(x, y, gw, gh, 4.0 * s) {
                pm.fill_path(&g, &solid(sk(gc, 1.0)), FillRule::Winding, tr, None);
                let m = clip(w, h, &g);
                // basket weave: two sets of diagonals
                let step = 3.0 * s;
                let mut k = -gh;
                while k < gw + gh {
                    for (dir, a) in [(1.0f32, 0.22f32), (-1.0, 0.16)] {
                        let (x0, x1) = if dir > 0.0 {
                            (x + k, x + k + gh)
                        } else {
                            (x + k + gh, x + k)
                        };
                        if let Some(line) = raster::polyline(&[(x0, y), (x1, y + gh)]) {
                            pm.stroke_path(
                                &line,
                                &solid(sk([0, 0, 0], a)),
                                &raster::stroke(1.1 * s),
                                tr,
                                Some(&m),
                            );
                        }
                    }
                    k += step;
                }
                // the speakers behind the cloth
                for (c, rr) in cones(def.id, l.panel) {
                    let (cx, cy) = lp(c);
                    if let (Some(circ), Some(p)) = (
                        raster::circle(cx, cy, rr * s),
                        radial(
                            (cx, cy),
                            rr * s,
                            vec![
                                (0.0, sk([0, 0, 0], 0.32)),
                                (0.35, sk([0, 0, 0], 0.12)),
                                (0.92, sk([0, 0, 0], 0.22)),
                                (1.0, sk([0, 0, 0], 0.05)),
                            ],
                        ),
                    ) {
                        pm.fill_path(&circ, &p, FillRule::Winding, tr, Some(&m));
                    }
                }
                if let Some(p) = radial(
                    (x + gw * 0.5, y + gh * 0.5),
                    gw.max(gh) * 0.75,
                    vec![(0.6, sk([0, 0, 0], 0.0)), (1.0, sk([0, 0, 0], 0.35))],
                ) {
                    pm.fill_path(&g, &p, FillRule::Winding, tr, None);
                }
                pm.stroke_path(&g, &solid(sk([0, 0, 0], 0.6)), &raster::stroke(1.5 * s), tr, None);
            }
        }
        Face::Rack => {
            let Some(path) = rrect(0.5, 0.5, fw - 1.0, fh - 1.0, 3.0 * s) else {
                return Image::new(1, 1);
            };
            if let Some(p) = linear(
                (0.0, 0.0),
                (0.0, fh),
                vec![(0.0, sk([214, 218, 223], 1.0)), (1.0, sk([140, 147, 155], 1.0))],
            ) {
                pm.fill_path(&path, &p, FillRule::Winding, tr, None);
            }
            let mut yy = 0.0;
            let mut i = 0;
            while yy < fh {
                pm.fill_rect(
                    SkRect::from_xywh(0.0, yy, fw, s.max(1.0) * 0.5)
                        .unwrap_or(SkRect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                    &solid(sk([255, 255, 255], 0.03 + 0.05 * hash01(i + 99))),
                    tr,
                    None,
                );
                yy += 1.6 * s;
                i += 1;
            }
            // the ears and their slots
            let ear = (fw / s * 0.06).clamp(12.0, 22.0) * s;
            for x0 in [0.0, fw - ear] {
                pm.fill_rect(
                    SkRect::from_xywh(x0, 0.0, ear, fh)
                        .unwrap_or(SkRect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                    &solid(sk([0, 0, 0], 0.12)),
                    tr,
                    None,
                );
                for yy in [fh * 0.22, fh * 0.78] {
                    if let Some(slot) = rrect(x0 + ear * 0.25, yy - 2.5 * s, ear * 0.5, 5.0 * s, 2.5 * s) {
                        pm.fill_path(&slot, &solid(sk([20, 22, 24], 0.85)), FillRule::Winding, tr, None);
                    }
                }
            }
            let (x, y, pw, ph) = lr(l.panel);
            if let Some(win) = rrect(x, y, pw, ph, 3.0 * s) {
                pm.fill_path(&win, &solid(sk([10, 22, 16], 1.0)), FillRule::Winding, tr, None);
                pm.stroke_path(
                    &win,
                    &solid(sk([0, 0, 0], 0.7)),
                    &raster::stroke(1.2 * s),
                    tr,
                    None,
                );
            }
            pm.stroke_path(&path, &solid(sk([0, 0, 0], 0.4)), &raster::stroke(s), tr, None);
        }
    }
    raster::from_pixmap(&pm)
}

fn clip(w: u32, h: u32, path: &tiny_skia::Path) -> tiny_skia::Mask {
    let mut m = tiny_skia::Mask::new(w, h).unwrap_or_else(|| tiny_skia::Mask::new(1, 1).expect("1x1 mask"));
    m.fill_path(path, FillRule::Winding, true, Transform::identity());
    m
}

// -- drawing --------------------------------------------------------------------------------------------------------

/// What's going on with a face's controls this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hot {
    /// The knob (or fader, or switch) under the mouse.
    pub hover: Option<usize>,
    /// The one being turned: its value is shown.
    pub active: Option<usize>,
    /// The footswitch under the mouse.
    pub switch: bool,
}

/// Draw a block's face in `r` (points) with the layout `l` (from `layout`).
pub fn draw(p: &Painter, r: Rect, b: &Block, l: &Layout, detail: Detail, hot: Hot) {
    let neural = b.def().file;
    let s = skinned(b);
    // (a skin is drawn with its own layout and indices; the caller's are the block's own)
    let skin_l;
    let mut hot = hot;
    let l = match &s {
        Some(sk) => {
            skin_l = family_layout(sk, r, detail);
            if let Some(map) = skin_map(sk.def(), b.def()) {
                let back = |o: Option<usize>| o.and_then(|j| map.iter().position(|&x| x == j));
                hot.hover = back(hot.hover);
                hot.active = back(hot.active);
            }
            &skin_l
        }
        None => l,
    };
    let b = s.as_ref().unwrap_or(b);
    let def = b.def();
    let th = t();
    let ppp = p.pixels_per_point();
    // (extra knobs: the face, their plate beside or under it)
    let r = if l.extra.is_empty() {
        r
    } else {
        let (f, _) = extra_split(b, r, l.extra.len());
        extra_strip(p, l.extra_area, b, l, hot);
        f
    };
    // the shadow it casts on the screen
    let sh = Shadow {
        offset: [0, 5],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if th.dark { 150 } else { 70 }),
    };
    // (a pedal or rack casts its shadow from its enclosure, which can be narrower than the rect it's given)
    let caster = match family(def) {
        Family::Pedal | Family::Rack if l.panel.is_positive() && r.contains_rect(l.panel) => l.panel,
        _ => r.shrink(2.0),
    };
    p.add(sh.as_shape(caster, l.radius));
    let k = key((
        "face",
        def.id,
        (r.width() * ppp) as i32,
        (r.height() * ppp) as i32,
        detail == Detail::Board,
    ));
    let (tex, _) = paint::texture(p.ctx(), Store::Face, k, || match family(def) {
        Family::Amp if amps::designed(def.id) => amps::body(b, r, l, ppp),
        Family::Amp => legacy_body(b, r, l, ppp),
        Family::Cab => cabs::body(b, r, l, ppp),
        Family::Pedal => pedals::body(b, r, l, ppp),
        Family::Rack => racks::body(b, r, l, ppp),
        Family::Silver => silver::body(b, r, l, ppp),
    });
    p.image(
        tex,
        r,
        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    match family(def) {
        Family::Amp if amps::designed(def.id) => amps::controls(p, r, b, l, detail, hot),
        Family::Amp => legacy_controls(p, r, b, l, detail, hot),
        Family::Cab => cabs::controls(p, r, b, l, detail, hot),
        Family::Pedal => pedals::controls(p, r, b, l, detail, hot),
        Family::Rack => racks::controls(p, r, b, l, detail, hot),
        Family::Silver => silver::controls(p, r, b, l, detail, hot),
    }
    if detail == Detail::Full && family(def) == Family::Pedal && s.is_none() {
        added_marks(p, b, l);
    }
    // (the silver unit carries its own stamp)
    if neural && family(def) != Family::Silver {
        neural_sticker(p, r);
    }
}

/// The plate of a face's extra knobs: dark, each knob in its cell with its name under it (its value while hovered
/// or turned).
fn extra_strip(p: &Painter, area: Rect, b: &Block, l: &Layout, hot: Hot) {
    let plate = extra_plate(area, l.extra.len());
    p.rect_filled(plate, 6.0, Color32::from_rgb(0x1c, 0x1e, 0x22));
    p.rect_stroke(
        plate,
        6.0,
        Stroke::new(1.0, Color32::from_white_alpha(24)),
        egui::StrokeKind::Inside,
    );
    let def = b.def();
    // (P37.6c, D140: our additions say so: "ADDED" over the plate when they all are, else on each one)
    let all_added = l
        .extra
        .iter()
        .all(|e| catalog::is_added(def.id, def.knobs[e.0].key));
    if all_added {
        let rows_top = l.extra.iter().map(|e| e.1.y - e.2 * 0.5).fold(f32::MAX, f32::min);
        text::caps(
            p,
            "ADDED",
            pos2(plate.center().x, (rows_top - 2.0).max(plate.min.y + 5.0)),
            6.5,
            Color32::from_rgb(0x9a, 0xa0, 0xa8),
            Align2::CENTER_BOTTOM,
            Some(plate.width() - 6.0),
        );
    }
    for (&(k, c, d), (_, _, cw)) in l.extra.iter().zip(extra_cells(area, l.extra.len())) {
        let kd = def.knobs[k];
        knobs::knob(
            p,
            c,
            d,
            kd.to_u(b.v[k]),
            Scale {
                ticks: false,
                ..Default::default()
            },
            Some(KnobStyle::Chrome),
        );
        let shown = hot.active == Some(k) || hot.hover == Some(k);
        let y = c.y + d * 0.5 + 8.0;
        let ink = Color32::from_rgb(0xd8, 0xdb, 0xe0);
        if shown {
            text::draw(
                p,
                &kd.text(b.v[k]),
                pos2(c.x, y),
                Align2::CENTER_CENTER,
                Style::new(9.5, 800, ink),
                Some(cw - 6.0),
            );
        } else if !all_added && catalog::is_added(def.id, kd.key) {
            label(
                p,
                &format!("{} (added)", kd.label),
                pos2(c.x, y),
                7.5,
                ink,
                cw - 6.0,
            );
        } else {
            label(p, kd.label, pos2(c.x, y), 7.5, ink, cw - 6.0);
        }
    }
}

/// A small "+" tab on a face's knob that the real unit doesn't have (P37.6c, D140), where the face draws it itself
/// (a designed pedal: its Range, Attack, Depth or Mix).
fn added_marks(p: &Painter, b: &Block, l: &Layout) {
    let def = b.def();
    for &(i, c, d) in &l.knobs {
        if !catalog::is_added(def.id, def.knobs[i].key) {
            continue;
        }
        let at = c + vec2(d * 0.5, -d * 0.5);
        let r = (d * 0.16).clamp(4.0, 7.0);
        p.circle_filled(at, r, Color32::from_rgb(0x1c, 0x1e, 0x22));
        p.circle_stroke(at, r, Stroke::new(1.0, Color32::from_rgb(0xd8, 0xdb, 0xe0)));
        let s = r * 0.55;
        let st = Stroke::new(1.2, Color32::from_rgb(0xd8, 0xdb, 0xe0));
        p.line_segment([at - vec2(s, 0.0), at + vec2(s, 0.0)], st);
        p.line_segment([at - vec2(0.0, s), at + vec2(0.0, s)], st);
    }
}

/// The mark on every NAM capture and IR (owner request, replacing the neuron sticker): a rubber-stamped "NAM" at the
/// top left, a double border round heavy capitals, tilted a little, on a cream label so it reads on any face.
pub fn neural_sticker(p: &Painter, r: Rect) {
    let h = (r.width().min(r.height()) * 0.13).clamp(8.0, 22.0);
    let ink = Color32::from_rgb(0xb8, 0x2e, 0x26);
    let galley = p.layout_no_wrap(
        "NAM".into(),
        egui::FontId::new(h * 0.72, qrs_ui::text::family(800)),
        ink,
    );
    let w = galley.size().x + h * 0.9;
    let angle = -0.1f32;
    let c = r.min + vec2(w * 0.5 + h * 0.35, h * 0.5 + h * 0.35);
    let (sa, ca) = angle.sin_cos();
    let rot = |v: egui::Vec2| c + vec2(v.x * ca - v.y * sa, v.x * sa + v.y * ca);
    let corners = |hw: f32, hh: f32| {
        vec![
            rot(vec2(-hw, -hh)),
            rot(vec2(hw, -hh)),
            rot(vec2(hw, hh)),
            rot(vec2(-hw, hh)),
        ]
    };
    // the label it's stamped on (a soft shadow, cream), then the two borders
    p.add(egui::Shape::convex_polygon(
        corners(w / 2.0, h / 2.0)
            .into_iter()
            .map(|q| q + vec2(0.0, h * 0.08))
            .collect(),
        Color32::from_black_alpha(70),
        Stroke::NONE,
    ));
    p.add(egui::Shape::convex_polygon(
        corners(w / 2.0, h / 2.0),
        Color32::from_rgb(0xf1, 0xec, 0xe0),
        Stroke::NONE,
    ));
    p.add(egui::Shape::closed_line(
        corners(w / 2.0 - h * 0.1, h / 2.0 - h * 0.1),
        Stroke::new((h * 0.07).max(0.8), ink),
    ));
    p.add(egui::Shape::closed_line(
        corners(w / 2.0 - h * 0.2, h / 2.0 - h * 0.2),
        Stroke::new((h * 0.035).max(0.5), ink),
    ));
    // the letters, turned with the label (a text shape turns about its top left)
    let gs = galley.size();
    let tl = rot(vec2(-gs.x / 2.0, -gs.y / 2.0));
    p.add(egui::epaint::TextShape::new(tl, galley, ink).with_angle(angle));
}

/// Faces this tall or smaller are always miniatures, whatever the caller asks (P37.12): their knobs would be under
/// about 8 points across, where the baked knob art, its ticks and its glow turn into a cluster of noise.
pub const MINI_H: f32 = 64.0;

thread_local! {
    /// Set while a cab or the silver unit is drawn as a miniature through `draw` (their faces aren't one cached body):
    /// knobs as dots, no print.
    static MINI: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the face being drawn is a miniature (see `MINI`).
pub(super) fn mini_now() -> bool {
    MINI.with(|m| m.get())
}

/// Whether a face `r` tall is drawn as a miniature (knobs as dots) rather than whole: under the caller's `full_h`,
/// or `MINI_H` and under in any case.
pub fn is_mini(r: Rect, full_h: f32) -> bool {
    r.height() < full_h || r.height() <= MINI_H
}

/// Knobs smaller than this (points across) are drawn as dots on a face drawn small (P37.12).
pub const KNOB_MIN_D: f32 = 13.0;

/// Whether a face's knobs, as laid out, are too small to draw as knobs: one under `KNOB_MIN_D` across, or two
/// overlapping (a layout that clamps its knobs' size, like the silver unit's, crowds them when it's small).
pub fn knobs_cramped(l: &Layout) -> bool {
    l.knobs.iter().any(|k| k.2 < KNOB_MIN_D)
        || l.knobs.iter().enumerate().any(|(i, a)| {
            l.knobs[i + 1..]
                .iter()
                .any(|b| a.1.distance(b.1) < (a.2 + b.2) * 0.5 * 0.95)
        })
}

/// A knob's dot on a miniature: its radius for a face `h` tall, and its colour (a little metal on an amp's panel, dark
/// on the rest). Small and plain on purpose: no cap, ticks, pointer, glow or reflection (P37.12).
pub fn knob_dot(face: Face, h: f32) -> (f32, Color32) {
    let r = (h * 0.025).clamp(0.9, 1.8);
    let c = match face {
        Face::Amp => Color32::from_rgba_unmultiplied(196, 198, 202, 190),
        Face::Rack | Face::Graphic => Color32::from_rgba_unmultiplied(34, 36, 40, 170),
        Face::Cab | Face::Pedal => Color32::from_rgba_unmultiplied(22, 22, 24, 215),
    };
    (r, c)
}

/// A block's face as a miniature (the chain strips, P21.4; the shelf, the sounds): its body as drawn on the board,
/// each knob a tiny dot (P37.12), its LED, but no print (the family painters' labels don't shrink that far). Above
/// `full_h` points tall (and above `MINI_H`, with room for its knobs: `knobs_cramped`) the whole face is drawn. Every
/// small face goes through here (the chain rows, the shelf's cells, the Sounds drawer, the save card), so they all
/// get the dots.
pub fn miniature(p: &Painter, r: Rect, b: &Block, full_h: f32) {
    // (a capture drawn as its skin, with its sticker after)
    if let Some(s) = skinned(b) {
        miniature(p, r, &s, full_h);
        neural_sticker(p, r);
        return;
    }
    let l = layout(b, r, Detail::Board);
    if !is_mini(r, full_h) && !knobs_cramped(&l) {
        draw(p, r, b, &l, Detail::Board, Hot::default());
        return;
    }
    // (a cab's cabinet is drawn with its controls, not in its body, and the silver unit's plate and stamp are live:
    // those two go through `draw`, told it's a miniature)
    if matches!(family(b.def()), Family::Cab | Family::Silver) {
        MINI.with(|m| m.set(true));
        draw(p, r, b, &l, Detail::Board, Hot::default());
        MINI.with(|m| m.set(false));
        return;
    }
    let def = b.def();
    let ppp = p.pixels_per_point();
    let sh = Shadow {
        offset: [0, 2],
        blur: 6,
        spread: 0,
        color: Color32::from_black_alpha(130),
    };
    p.add(sh.as_shape(r, l.radius));
    let k = key((
        "face-mini",
        def.id,
        (r.width() * ppp) as i32,
        (r.height() * ppp) as i32,
    ));
    let (tex, _) = paint::texture(p.ctx(), Store::Face, k, || match family(def) {
        Family::Amp if amps::designed(def.id) => amps::body(b, r, &l, ppp),
        Family::Amp => legacy_body(b, r, &l, ppp),
        Family::Cab => cabs::body(b, r, &l, ppp),
        Family::Pedal => pedals::mini_body(b, r, &l, ppp),
        Family::Rack => racks::body(b, r, &l, ppp),
        Family::Silver => silver::body(b, r, &l, ppp),
    });
    p.image(
        tex,
        r,
        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let (dr, dc) = knob_dot(def.face, r.height());
    for &(_, c, _) in &l.knobs {
        p.circle_filled(c, dr, dc);
    }
    for &(_, tr) in &l.faders {
        p.rect_filled(
            Rect::from_center_size(tr.center(), vec2(dr * 0.8, tr.height())),
            0.0,
            Color32::from_black_alpha(70),
        );
    }
    if def.face != Face::Cab && l.led != Pos2::ZERO && r.contains(l.led) {
        led_at(p, l.led, b.on, (r.width() * 0.09).clamp(2.5, 4.0));
    }
}

/// Today's knobs, switches, LED and print over a body (each family's `controls` until it draws its own).
pub(super) fn legacy_controls(p: &Painter, _r: Rect, b: &Block, l: &Layout, detail: Detail, hot: Hot) {
    let def = b.def();
    let th = t();
    let col = def.colour;
    let (ink_c, label_size) = match def.face {
        Face::Amp => (amp_ink(def.id), if detail == Detail::Board { 6.5 } else { 8.5 }),
        Face::Rack => (
            Color32::from_rgb(27, 29, 32),
            if detail == Detail::Board { 6.5 } else { 8.0 },
        ),
        Face::Cab => (Color32::from_rgb(233, 226, 208), 8.0),
        _ => (ink(col), if detail == Detail::Board { 6.5 } else { 8.5 }),
    };
    // each label gets the room up to its neighbours in the row (an amp's toggles stand in its row too)
    let spots: Vec<Pos2> = l
        .knobs
        .iter()
        .map(|k| k.1)
        .chain(if def.face == Face::Amp {
            l.choices.iter().map(|c| c.1.center()).collect()
        } else {
            vec![]
        })
        .collect();
    let room = |c: Pos2, d: f32| {
        let gap = spots
            .iter()
            .filter(|o| (o.y - c.y).abs() < d * 0.5 && (o.x - c.x).abs() > 0.5)
            .map(|o| (o.x - c.x).abs())
            .fold(f32::INFINITY, f32::min);
        (d + 26.0).min(gap - 4.0)
    };
    // knobs, with their scale printed round them
    for &(i, c, d) in &l.knobs {
        let kd = &def.knobs[i];
        let u = kd.to_u(b.v[i]);
        let style = match def.face {
            Face::Pedal | Face::Graphic => Some(KnobStyle::Pedal),
            Face::Rack | Face::Cab => Some(KnobStyle::Chrome),
            Face::Amp => None,
        };
        let on_screen = def.face == Face::Cab;
        // (the dark themes' baked knobs show hover and a hold)
        let state = qrs_ui::glass::State::of(hot.hover == Some(i), hot.active == Some(i));
        if on_screen {
            knobs::knob_in(
                p,
                c,
                d,
                u,
                Scale {
                    zero: kd.centred().then(|| kd.to_u(0.0)),
                    ..Default::default()
                },
                style,
                state,
            );
        } else {
            ticks(p, c, d, u, kd, ink_c);
            knobs::knob_in(
                p,
                c,
                d,
                u,
                Scale {
                    ticks: false,
                    ..Default::default()
                },
                style,
                state,
            );
        }
        let lab_y = c.y + d * 0.5 + if on_screen { 12.0 } else { 7.0 };
        let shown = hot.active == Some(i) || hot.hover == Some(i);
        let label_color = if on_screen { th.caption } else { ink_c };
        if shown {
            text::draw(
                p,
                &kd.text(b.v[i]),
                pos2(c.x, lab_y),
                Align2::CENTER_CENTER,
                Style::new(
                    label_size + 2.0,
                    800,
                    if on_screen { th.accent_ink } else { ink_c },
                ),
                None,
            );
        } else {
            label(p, kd.label, pos2(c.x, lab_y), label_size, label_color, room(c, d));
        }
    }
    // switches drawn as their options
    for &(i, cr) in &l.choices {
        choice(
            p,
            cr,
            def.knobs[i],
            b.v[i],
            def.face,
            ink_c,
            col,
            hot.hover == Some(i),
        );
    }
    // faders
    for &(i, track) in &l.faders {
        fader(
            p,
            track,
            &def.knobs[i],
            b.v[i],
            def.face,
            ink_c,
            hot.active == Some(i) || hot.hover == Some(i),
        );
    }
    // the print, the light, the switch
    match def.face {
        Face::Pedal => {
            print(
                p,
                def.name,
                l.name.center(),
                Align2::CENTER_CENTER,
                l.name.height(),
                ink_c,
                l.name.width(),
            );
            led(p, l.led, b.on, if detail == Detail::Board { 7.0 } else { 9.0 });
            if hot.switch {
                p.circle_stroke(
                    l.switch.center(),
                    l.switch.width() * 0.6,
                    Stroke::new(1.5, th.accent),
                );
            }
        }
        Face::Graphic => {
            print(
                p,
                def.name,
                l.name.left_center(),
                Align2::LEFT_CENTER,
                l.name.height() * 0.8,
                ink_c,
                l.name.width(),
            );
            led(p, l.led, b.on, 7.0);
            if hot.switch {
                p.circle_stroke(
                    l.switch.center(),
                    l.switch.width() * 0.6,
                    Stroke::new(1.5, th.accent),
                );
            }
        }
        Face::Amp => {
            let logo = match amp_panel(def.id).0 {
                "gold" => Color32::from_rgb(233, 207, 134),
                "tweed" => Color32::from_rgb(233, 226, 208),
                _ => Color32::from_rgb(222, 222, 222),
            };
            print(
                p,
                def.name,
                l.name.left_center(),
                Align2::LEFT_CENTER,
                l.name.height() * 0.62,
                logo,
                l.name.width(),
            );
            led(p, l.led, b.on, 10.0);
            standby(p, l.switch, b.on, hot.switch);
        }
        Face::Cab => {
            print(
                p,
                def.name,
                l.name.center(),
                Align2::CENTER_CENTER,
                11.0,
                Color32::from_rgb(233, 226, 208),
                l.name.width(),
            );
            if let Some((c, rr)) = l.cone {
                // the mic: from the cone's edge (0) to its cap (10)
                let u = b.v[0] / 10.0;
                let m = pos2(c.x + (1.0 - u) * rr * 0.8, c.y);
                p.circle_filled(m + vec2(1.0, 2.0), 6.5, Color32::from_black_alpha(110));
                p.circle_filled(m, 6.0, Color32::from_rgb(70, 74, 80));
                p.circle_filled(m - vec2(1.2, 1.2), 3.0, Color32::from_rgb(190, 194, 200));
                p.circle_stroke(m, 7.5, Stroke::new(1.5, th.accent));
            }
        }
        Face::Rack => {
            print(
                p,
                def.name,
                l.name.left_center(),
                Align2::LEFT_CENTER,
                12.0,
                ink_c,
                l.name.width(),
            );
            let first = def
                .knobs
                .iter()
                .position(|k| !matches!(k.unit, Unit::Choice(_)))
                .unwrap_or(0);
            let shown = hot.active.or(hot.hover).unwrap_or(first);
            if let Some(kd) = def.knobs.get(shown) {
                let s = format!("{} {}", kd.label.to_uppercase(), kd.text(b.v[shown]));
                text::draw(
                    p,
                    &s,
                    l.panel.left_center() + vec2(6.0, 0.0),
                    Align2::LEFT_CENTER,
                    Style::new(9.5, 700, Color32::from_rgb(95, 245, 154)),
                    Some(l.panel.width() - 8.0),
                );
            }
            led(p, l.led, b.on, 7.0);
            bypass_key(p, l.switch, b.on, hot.switch);
        }
    }
}

/// The size (from `size` down to `min`) at which `s` fits `max_w`, if it does.
fn fit_size(p: &Painter, s: &str, size: f32, min: f32, max_w: f32) -> Option<f32> {
    let mut sz = size;
    loop {
        if text::width(p.ctx(), s, Style::caps(sz, Color32::WHITE)) <= max_w {
            return Some(sz);
        }
        if sz <= min {
            return None;
        }
        sz -= 0.5;
    }
}

/// A long label as a panel prints it where there's no room.
fn abbrev(s: &str) -> &str {
    match s {
        "Attack" => "Atk",
        "Balance" => "Bal",
        "Bass freq" => "B freq",
        "Compress" => "Comp",
        "Condenser" => "Cond",
        "Crossover" => "X-over",
        "Distance" => "Dist",
        "Distortion" => "Dist",
        "High cut" => "Hi cut",
        "Intensity" => "Inten",
        "Low cut" => "Lo cut",
        "Makeup" => "Mkup",
        "Master" => "Mstr",
        "Mic type" => "Mic",
        "Output" => "Out",
        "Pre-delay" => "Pre-dly",
        "Presence" => "Pres",
        "Release" => "Rel",
        "Repeats" => "Rpts",
        "Sustain" => "Sus",
        "Threshold" => "Thresh",
        "Treble" => "Treb",
        "Treble freq" => "T freq",
        "Volume" => "Vol",
        _ => s,
    }
}

/// A control's label in caps fitted to `max_w`: smaller, then its short form, then cut.
fn label(p: &Painter, s: &str, pos: Pos2, size: f32, color: Color32, max_w: f32) {
    let (s, sz) = match fit_size(p, s, size, (size - 1.5).max(5.5), max_w) {
        Some(sz) => (s, sz),
        None => {
            let a = abbrev(s);
            (a, fit_size(p, a, size, 5.5, max_w).unwrap_or(5.5))
        }
    };
    text::caps(p, s, pos, sz, color, Align2::CENTER_CENTER, Some(max_w));
}

fn amp_ink(id: &str) -> Color32 {
    match amp_panel(id).0 {
        "gold" | "tweed" | "plate" | "silver" => Color32::from_rgb(40, 31, 12),
        _ => Color32::from_rgb(230, 230, 230),
    }
}

/// A knob's printed scale: 11 ticks, the ones up to its value full, the rest faint.
fn ticks(p: &Painter, c: Pos2, d: f32, u: f32, kd: &KnobDef, color: Color32) {
    let (r0, r1) = (d * 0.5 + 2.5, d * 0.5 + 5.5);
    let zero = kd.centred().then(|| kd.to_u(0.0));
    for i in 0..11 {
        let tu = i as f32 / 10.0;
        let a = (-135.0 + 270.0 * tu).to_radians();
        let (s, co) = a.sin_cos();
        let lit = match zero {
            Some(z) => (tu >= z.min(u) - 1e-3) && (tu <= z.max(u) + 1e-3),
            None => tu <= u + 1e-3,
        };
        let col = if lit { color } else { color.gamma_multiply(0.28) };
        p.line_segment(
            [
                pos2(c.x + r0 * s, c.y - r0 * co),
                pos2(c.x + r1 * s, c.y - r1 * co),
            ],
            Stroke::new(1.3, col),
        );
    }
}

/// The red LED.
pub fn led(p: &Painter, c: Pos2, on: bool, d: f32) {
    led_at(p, c, on, d)
}

fn led_at(p: &Painter, c: Pos2, on: bool, d: f32) {
    if on {
        for (k, a) in [(2.6, 24), (1.8, 50), (1.3, 90)] {
            p.circle_filled(c, d * 0.5 * k, Color32::from_rgba_unmultiplied(255, 60, 40, a));
        }
        p.circle_filled(c, d * 0.5, Color32::from_rgb(255, 74, 58));
        p.circle_filled(
            c - vec2(d * 0.12, d * 0.12),
            d * 0.22,
            Color32::from_rgb(255, 222, 214),
        );
    } else {
        p.circle_filled(c, d * 0.5, Color32::from_rgb(74, 26, 22));
        p.circle_stroke(c, d * 0.5, Stroke::new(1.0, Color32::from_black_alpha(120)));
    }
}

fn standby(p: &Painter, r: Rect, on: bool, hot: bool) {
    let th = t();
    p.rect_filled(r, r.height() * 0.5, Color32::from_rgb(12, 12, 12));
    let k = r.height() - 4.0;
    let x = if on {
        r.max.x - 2.0 - k * 0.5
    } else {
        r.min.x + 2.0 + k * 0.5
    };
    p.circle_filled(pos2(x, r.center().y), k * 0.5, Color32::from_rgb(214, 216, 220));
    text::caps(
        p,
        if on { "ON" } else { "STANDBY" },
        pos2(r.min.x - 6.0, r.center().y),
        7.5,
        Color32::from_rgb(190, 190, 190),
        Align2::RIGHT_CENTER,
        None,
    );
    if hot {
        p.rect_stroke(
            r.expand(2.0),
            r.height() * 0.5 + 2.0,
            Stroke::new(1.5, th.accent),
            egui::StrokeKind::Outside,
        );
    }
}

fn bypass_key(p: &Painter, r: Rect, on: bool, hot: bool) {
    let th = t();
    p.rect_filled(
        r,
        3.0,
        if on {
            Color32::from_rgb(42, 45, 49)
        } else {
            Color32::from_rgb(90, 30, 26)
        },
    );
    text::caps(
        p,
        if on { "BYPASS" } else { "OFF" },
        r.center(),
        7.5,
        Color32::from_rgb(232, 232, 232),
        Align2::CENTER_CENTER,
        None,
    );
    if hot {
        p.rect_stroke(
            r.expand(1.5),
            4.0,
            Stroke::new(1.5, th.accent),
            egui::StrokeKind::Outside,
        );
    }
}

/// A switch with a few positions, its options side by side; the chosen one lit.
fn choice(p: &Painter, r: Rect, kd: KnobDef, v: f32, face: Face, ink_c: Color32, col: [u8; 3], hot: bool) {
    let Unit::Choice(opts) = kd.unit else { return };
    let th = t();
    if face == Face::Amp {
        // an amp's toggle: a chrome nut and a bat handle up (on) or down, ON / OFF printed beside
        let on = v >= 0.5;
        let c = r.center();
        let nut = (r.width() * 0.34).clamp(4.0, 8.0);
        p.circle_filled(c + vec2(0.0, 1.0), nut + 1.0, Color32::from_black_alpha(120));
        p.circle_filled(c, nut, Color32::from_rgb(150, 154, 160));
        p.circle_filled(
            c - vec2(nut * 0.25, nut * 0.25),
            nut * 0.55,
            Color32::from_rgb(220, 222, 226),
        );
        let len = (r.height() * 0.42).clamp(8.0, 16.0);
        let tip = pos2(c.x, if on { c.y - len } else { c.y + len });
        p.line_segment([c, tip], Stroke::new(3.2, Color32::from_rgb(60, 62, 66)));
        p.line_segment([c, tip], Stroke::new(2.2, Color32::from_rgb(236, 236, 240)));
        p.circle_filled(tip, 2.6, Color32::from_rgb(244, 244, 248));
        let opt = if let Unit::Choice(o) = kd.unit { o } else { &[] };
        let state = opt.get(if on { 1 } else { 0 }).copied().unwrap_or("");
        text::caps(
            p,
            state,
            pos2(
                c.x + nut + 3.0,
                if on { c.y - len * 0.6 } else { c.y + len * 0.6 },
            ),
            6.5,
            ink_c.gamma_multiply(0.85),
            Align2::LEFT_CENTER,
            None,
        );
        text::caps(
            p,
            kd.label,
            pos2(c.x, r.max.y + 6.0),
            7.5,
            ink_c,
            Align2::CENTER_CENTER,
            None,
        );
        if hot {
            p.circle_stroke(c, nut + 4.0, Stroke::new(1.5, th.accent));
        }
        return;
    }
    let sel = v.round().max(0.0) as usize;
    let screen = face == Face::Cab;
    let (bg, lit_bg, lit_ink, off_ink) = if screen {
        (th.base_lo, th.accent, th.on_accent, th.caption)
    } else {
        (
            ink_c.gamma_multiply(0.14),
            ink_c,
            rgb(col),
            ink_c.gamma_multiply(0.8),
        )
    };
    p.rect_filled(r, r.height() * 0.5, bg);
    let n = opts.len().max(1);
    let w = r.width() / n as f32;
    for (k, o) in opts.iter().enumerate() {
        let seg = Rect::from_min_size(pos2(r.min.x + w * k as f32, r.min.y), vec2(w, r.height()));
        if k == sel {
            p.rect_filled(seg.shrink(1.5), (r.height() - 3.0) * 0.5, lit_bg);
        }
        label(
            p,
            o,
            seg.center(),
            7.0,
            if k == sel { lit_ink } else { off_ink },
            w - 3.0,
        );
    }
    if !screen {
        text::caps(
            p,
            kd.label,
            pos2(r.min.x - 4.0, r.center().y),
            7.0,
            ink_c.gamma_multiply(0.8),
            Align2::RIGHT_CENTER,
            None,
        );
    }
    if hot {
        p.rect_stroke(
            r.expand(1.5),
            r.height() * 0.5 + 1.5,
            Stroke::new(1.2, th.accent),
            egui::StrokeKind::Outside,
        );
    }
}

/// A fader: upright on a graphic EQ (its cap at the value), across on a cab's controls.
fn fader(p: &Painter, track: Rect, kd: &KnobDef, v: f32, face: Face, ink_c: Color32, hot: bool) {
    let th = t();
    let u = kd.to_u(v);
    if face == Face::Graphic {
        let y = track.max.y - u * track.height();
        let cap = Rect::from_center_size(pos2(track.center().x, y), vec2(16.0, 9.0));
        p.rect_filled(cap.translate(vec2(0.0, 1.5)), 2.0, Color32::from_black_alpha(120));
        p.rect_filled(cap, 2.0, Color32::from_rgb(44, 47, 52));
        p.line_segment(
            [
                pos2(cap.min.x + 2.0, cap.center().y),
                pos2(cap.max.x - 2.0, cap.center().y),
            ],
            Stroke::new(1.2, Color32::from_rgb(240, 240, 236)),
        );
        if hot {
            p.rect_stroke(
                cap.expand(1.5),
                3.0,
                Stroke::new(1.3, th.accent),
                egui::StrokeKind::Outside,
            );
            text::draw(
                p,
                &kd.text(v),
                pos2(track.center().x, track.min.y - 9.0),
                Align2::CENTER_CENTER,
                Style::new(8.5, 800, ink_c),
                None,
            );
        } else {
            text::caps(
                p,
                kd.label,
                pos2(track.center().x, track.min.y - 9.0),
                7.0,
                ink_c,
                Align2::CENTER_CENTER,
                None,
            );
        }
        return;
    }
    // across, on the screen beside a cab
    paint::inset(p, track, Some(4.0), 1.5, paint::Fill::Color(th.track));
    let fill = Rect::from_min_max(track.min, pos2(track.min.x + u * track.width(), track.max.y));
    p.rect_filled(fill.shrink(1.0), 3.0, th.accent);
    let kx = track.min.x + u * track.width();
    qrs_ui::keys::thumb(p, pos2(kx, track.center().y), 14.0);
    let (lo, hi) = match kd.key {
        "mic" => ("EDGE", "CAP"),
        "distance" => ("CLOSE", "FAR"),
        _ => ("", ""),
    };
    text::caps(
        p,
        kd.label,
        pos2(track.min.x, track.min.y - 9.0),
        8.0,
        th.caption,
        Align2::LEFT_CENTER,
        None,
    );
    text::caps(
        p,
        if hot { "" } else { lo },
        pos2(track.min.x, track.max.y + 9.0),
        7.0,
        th.caption,
        Align2::LEFT_CENTER,
        None,
    );
    text::caps(
        p,
        if hot { "" } else { hi },
        pos2(track.max.x, track.max.y + 9.0),
        7.0,
        th.caption,
        Align2::RIGHT_CENTER,
        None,
    );
    if hot {
        text::draw(
            p,
            &kd.text(v),
            pos2(track.max.x, track.min.y - 9.0),
            Align2::RIGHT_CENTER,
            Style::new(9.0, 800, th.accent_ink),
            None,
        );
    }
}

// -- kinds -------------------------------------------------------------------------------------------------------------

/// A kind's swatch colour.
pub fn kind_colour(k: Kind) -> Color32 {
    rgb(k.colour())
}

#[cfg(test)]
mod fit_tests;

#[cfg(test)]
mod reach_tests {
    use super::*;
    use qrs_audio::board::MODELS;

    /// Every front knob of every block is a control on its face (a designed face or the silver unit), at the size the
    /// editor and the tile draw it (B3, P37.1).
    #[test]
    fn every_front_knob_of_every_block_is_on_its_face() {
        for (m, def) in MODELS.iter().enumerate() {
            let b = Block::new(m);
            let sz = board_size(&b);
            let k = (900.0 / sz.x).min(420.0 / sz.y);
            let r = Rect::from_min_size(pos2(10.0, 10.0), sz * k);
            let l = layout(&b, r, Detail::Full);
            let mut on: Vec<usize> = l.knobs.iter().chain(&l.extra).map(|x| x.0).collect();
            on.extend(l.faders.iter().map(|x| x.0));
            on.extend(l.choices.iter().map(|x| x.0));
            for i in 0..def.front_len() {
                assert!(
                    on.contains(&i),
                    "{}: front knob {} ({}) isn't on its face",
                    def.id,
                    i,
                    def.knobs[i].key
                );
            }
            assert!(
                on.iter().all(|&i| i < def.front_len()),
                "{}: a control that isn't a front knob",
                def.id
            );
            // (file blocks without a library item are the silver unit)
            if def.file {
                assert_eq!(family(def), Family::Silver);
                assert!(!is_skinned(&b));
            }
        }
    }

    /// A skin is used only where each of its controls turns a real knob and every real knob is on it.
    #[test]
    fn a_skin_maps_every_control_or_is_not_used() {
        let find = |id: &str| &MODELS[qrs_audio::board::catalog::find(id).unwrap()];
        let (amp, cab) = (find("amp_nam"), find("cab_ir"));
        for skin in MODELS.iter().filter(|m| !m.file) {
            for own in [amp, cab, find("pedal_nam")] {
                if let Some(map) = skin_map(skin, own) {
                    assert_eq!(map.len(), skin.front_len(), "{} on {}", skin.id, own.id);
                    let mut seen = map.clone();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(seen.len(), own.front_len(), "{} on {}", skin.id, own.id);
                }
            }
        }
        // the Perspex 100's Presence has no real control on a capture: never its skin
        assert!(skin_map(find("amp_crunch"), amp).is_none());
        // a cab skin's mic and distance aren't an IR's: never its skin
        assert!(skin_map(find("cab_4x12"), cab).is_none());
    }
}

#[cfg(test)]
mod mini_tests {
    use super::*;
    use qrs_audio::board::MODELS;

    /// Under the caller's size or `MINI_H` a face is a miniature (knobs as dots); big enough, it is drawn whole.
    #[test]
    fn small_faces_are_miniatures_and_big_ones_whole() {
        let r = |h: f32| Rect::from_min_size(Pos2::ZERO, vec2(h * 2.0, h));
        // the chain strips (asked for a miniature under 90)
        assert!(is_mini(r(40.0), 90.0));
        assert!(is_mini(r(85.0), 90.0));
        assert!(!is_mini(r(200.0), 90.0));
        // callers asking for the whole face (0.0) still get dots at thumbnail size, the face above it
        assert!(is_mini(r(MINI_H), 0.0));
        assert!(is_mini(r(30.0), 0.0));
        assert!(!is_mini(r(MINI_H + 1.0), 0.0));
        assert!(!is_mini(r(420.0), 0.0));
        // knobs too small or overlapping are dots whatever the face's height
        let mut l = Layout {
            knobs: vec![(0, pos2(0.0, 0.0), 30.0), (1, pos2(40.0, 0.0), 30.0)],
            ..Default::default()
        };
        assert!(!knobs_cramped(&l));
        l.knobs[1].1 = pos2(20.0, 0.0);
        assert!(knobs_cramped(&l));
        l.knobs = vec![(0, pos2(0.0, 0.0), 10.0)];
        assert!(knobs_cramped(&l));
        // the dot is tiny at every miniature size
        for h in [20.0, 40.0, 64.0, 89.0] {
            let (d, _) = knob_dot(Face::Amp, h);
            assert!((0.9..=2.2).contains(&d), "{h}: {d}");
        }
    }

    /// Every model's miniature draws each knob as one plain dot: at a knob's centre nothing but the dot (no cap,
    /// ticks, pointer, glow or baked knob image), and no knob art anywhere (P37.12).
    #[test]
    fn every_models_miniature_has_dots_not_knob_art() {
        let ctx = egui::Context::default();
        qrs_ui::text::install(&ctx);
        // (the chain rows ask for a miniature under 90; the shelf and the Sounds drawer ask for the whole face, 0.0)
        for (h, full_h) in [(40.0, 90.0), (60.0, 90.0), (44.0, 0.0), (64.0, 0.0)] {
            for (m, def) in MODELS.iter().enumerate() {
                let b = Block::new(m);
                let sz = board_size(&b);
                let r = Rect::from_min_size(pos2(10.0, 10.0), sz * (h / sz.y));
                let l = layout(&b, r, Detail::Board);
                let (dr, _) = knob_dot(def.face, h);
                ctx.begin_pass(Default::default());
                let p = ctx.layer_painter(egui::LayerId::background());
                miniature(&p, r, &b, full_h);
                let mut out = ctx.end_pass();
                out.textures_delta.clear();
                let big = |s: &egui::Shape| {
                    let v = s.visual_bounding_rect();
                    v.width() * v.height() >= r.area() * 0.2
                };
                let mut dots = 0;
                for cs in &out.shapes {
                    let s = &cs.shape;
                    if let egui::Shape::Circle(c) = s
                        && (c.radius - dr).abs() < 1e-3
                    {
                        dots += 1;
                        continue;
                    }
                    // (the body, its shadow, the cab's art: whole-face shapes)
                    if big(s) {
                        continue;
                    }
                    let v = s.visual_bounding_rect();
                    // (the LED and its glow)
                    if l.led != Pos2::ZERO && v.contains(l.led) {
                        continue;
                    }
                    for &(i, c, _) in &l.knobs {
                        assert!(
                            !v.contains(c),
                            "{} at {h}: something besides the dot on knob {} ({:?})",
                            def.id,
                            def.knobs[i].key,
                            v
                        );
                    }
                }
                assert!(
                    dots >= l.knobs.len(),
                    "{} at {h}: {dots} dots for {} knobs",
                    def.id,
                    l.knobs.len()
                );
            }
        }
    }
}
