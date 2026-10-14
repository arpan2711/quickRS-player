//! Page tab view (like Songsterr; port of rsplay/tabview.py): the whole chart as rows of tab measures on one dark
//! screen, the current bar a lighter band on it, a glowing accent cursor moving through that bar (the notes under it
//! lit), rhythm notation under the staff, a page that scrolls to keep the cursor in view, and Ctrl-drag selections
//! with their toolbar.
//!
//! Laid out in the Python app's whole screen pixels: the layout is kept in pixels (f64, so the arithmetic is the
//! Python app's own, `int(n * s)` and all) and turned into points only to draw; what the Python app draws with
//! pygame.draw (lines, bars, chips, the cursor's marker, stems and rests) is drawn pixel for pixel as pygame does,
//! aliased. Only the scroll (and what's derived from it for the player) stays in points.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2};
use qrs_formats::Techs;
use qrs_formats::ascii_tab::section_label;
use qrs_ui::Mat;
use qrs_ui::icons::Icon;
use qrs_ui::paint::{self, Fill};
use qrs_ui::raster::mix;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{judge_color, string_color, t};
use qrs_ui::units;
use qrs_ui::widgets::{self, ButtonStyle, RoundKind, interact};

use super::{Chart, Glyph};

/// How far the loop's bars are tinted towards the accent, and the Ctrl-drag selection.
pub const LOOP_WASH: f32 = 0.05;
pub const SEL_WASH: f32 = 0.08;
/// Seconds of playback after you scroll away before the page follows the cursor again.
const RELOCK_AFTER: f32 = 4.0;
/// Two notes on a string closer than this (grace notes, flams) are drawn side by side.
pub const GRACE: f64 = 0.05;
/// The bend's amount, small at the top right inside its note's chip.
const BEND_SIZE: f32 = 10.0;

// -- the Python app's whole pixels --------------------------------------------------------------------------------
// It works in screen pixels and makes every layout number whole as it goes (`int(n * s)`; pygame Rects truncate).
// The layout here is that arithmetic in f64 pixels, in the same order, so it comes out bit for bit as the Python
// app's (its float quirks too: a row that adds up to 953.99999 ends at 953). A whole pixel read back from f32 points
// (the callers' rects, a scroll they keep) has ~1e-4 px of noise, so it's truncated with a little slack (`int_pt`),
// and one handed to something that truncates it again (text anchors) goes a hair inside its pixel (`pt`).

/// The Python app's scale (1 until the app has set one).
pub(super) fn sc() -> f64 {
    let s = units::scale();
    if s > 0.0 { s } else { 1.0 }
}

/// `int(v)` of a pixel value: toward zero.
pub(super) fn int_(v: f64) -> f64 {
    v.trunc()
}

/// `int(v)` of a pixel value held in points (with slack for their f32 noise).
pub(super) fn int_pt(v: f32) -> f64 {
    let v = px_of(v);
    if v >= 0.0 {
        (v + 1e-3).trunc()
    } else {
        (v - 1e-3).trunc()
    }
}

/// `int(n * s)` of a scale-1 number, in pixels.
pub(super) fn ipx(n: f64) -> f64 {
    (n * sc()).trunc()
}

/// `neu.px(n)`, `int(round(n * s))` with Python's round (halves to even), in pixels.
pub(super) fn rpx(n: f64) -> f64 {
    (n * sc()).round_ties_even()
}

/// Pixels as points (a whole pixel a hair inside itself).
pub(super) fn pt(v: f64) -> f32 {
    ((v + if v < 0.0 { -1e-3 } else { 1e-3 }) / sc()) as f32
}

/// Points as pixels.
pub(super) fn px_of(v: f32) -> f64 {
    v as f64 * sc()
}

/// pygame's width of a line of text, in pixels.
pub(super) fn tw(ctx: &egui::Context, s: &str, st: Style) -> f64 {
    (text::width(ctx, s, st) as f64 * ctx.pixels_per_point() as f64).round()
}

/// pygame's height of a line of text in this style, in pixels.
pub(super) fn th_px(ctx: &egui::Context, st: Style) -> f64 {
    (text::line_height(ctx, st) as f64 * ctx.pixels_per_point() as f64).round()
}

/// `blit_text`: text with its `anchor` at (x, y) (pixels; truncated, as pygame's Rect takes them), cut with an
/// ellipsis to `max_w` pixels. Returns its box.
pub(super) fn put(
    p: &Painter,
    s: &str,
    x: f64,
    y: f64,
    anchor: Align2,
    st: Style,
    max_w: Option<f64>,
) -> PRect {
    let r = text::draw(
        p,
        s,
        pos2(pt(int_(x)), pt(int_(y))),
        anchor,
        st,
        max_w.map(|m| ((m + 1e-3) / sc()) as f32),
    );
    PRect::of(r)
}

/// A pygame Rect: whole screen pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct PRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl PRect {
    /// `pygame.Rect(x, y, w, h)` of floats: each truncated.
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> PRect {
        PRect {
            x: int_(x),
            y: int_(y),
            w: int_(w),
            h: int_(h),
        }
    }

    /// A rect in points that sits on whole pixels (as the components draw it: edges rounded).
    pub fn of(r: Rect) -> PRect {
        let s = sc();
        let (x, y) = ((r.min.x as f64 * s).round(), (r.min.y as f64 * s).round());
        PRect {
            x,
            y,
            w: (r.max.x as f64 * s).round() - x,
            h: (r.max.y as f64 * s).round() - y,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn centerx(&self) -> f64 {
        self.x + (self.w / 2.0).floor()
    }

    pub fn centery(&self) -> f64 {
        self.y + (self.h / 2.0).floor()
    }

    /// pygame's `inflate`: grown by (dx, dy) whole pixels, half on each side (C's truncating halves).
    pub fn inflate(&self, dx: f64, dy: f64) -> PRect {
        PRect {
            x: self.x - (dx / 2.0).trunc(),
            y: self.y - (dy / 2.0).trunc(),
            w: self.w + dx,
            h: self.h + dy,
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.x <= x && x < self.right() && self.y <= y && y < self.bottom()
    }

    /// In points, for the components.
    pub fn rect(&self) -> Rect {
        Rect::from_min_max(
            pos2(pt(self.x), pt(self.y)),
            pos2(pt(self.right()), pt(self.bottom())),
        )
    }
}

/// What pygame.draw lights, pixel for pixel (aliased), as whole-pixel boxes drawn as one solid mesh.
#[derive(Default)]
pub(super) struct Pix(Vec<[i32; 4]>);

/// pygame's filled circle of radius `r` (`draw_circle_filled`, a midpoint algorithm), as rows relative to its
/// centre: (dy, from dx, to dx), both ends drawn.
fn circle_rows(r: i32) -> BTreeMap<i32, (i32, i32)> {
    let mut rows: BTreeMap<i32, (i32, i32)> = BTreeMap::new();
    let mut span = |a: i32, y: i32, b: i32| {
        if b >= a {
            let e = rows.entry(y).or_insert((a, b));
            *e = (e.0.min(a), e.1.max(b));
        }
    };
    let (mut f, mut ddx, mut ddy, mut x, mut y) = (1 - r, 0, -2 * r, 0, r);
    while x < y {
        if f >= 0 {
            y -= 1;
            ddy += 2;
            f += ddy;
        }
        x += 1;
        ddx += 2;
        f += ddx + 1;
        span(-x, y - 1, x - 1);
        span(-x, -y, x - 1);
        span(-y, x - 1, y - 1);
        span(-y, -x, y - 1);
    }
    rows
}

impl Pix {
    /// Pixels x0..=x1 of row y.
    fn span(&mut self, x0: i32, x1: i32, y: i32) {
        if x1 >= x0 {
            self.0.push([x0, y, x1 + 1, y + 1]);
        }
    }

    fn block(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if w > 0 && h > 0 {
            self.0.push([x, y, x + w, y + h]);
        }
    }

    fn dot(&mut self, x: i32, y: i32) {
        self.0.push([x, y, x + 1, y + 1]);
    }

    /// `pygame.draw.line` (pygame-ce's draw_line / draw_line_width): its ends truncated and both drawn, `width`
    /// pixels thick across its main direction (an even width's extra pixel after it).
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64), width: f64) {
        let (mut x1, mut y1) = (int_(a.0) as i32, int_(a.1) as i32);
        let (x2, y2) = (int_(b.0) as i32, int_(b.1) as i32);
        let width = width as i32;
        if width < 1 {
            return;
        }
        let (dx, dy) = ((x2 - x1).abs(), (y2 - y1).abs());
        if width == 1 {
            if y1 == y2 {
                self.span(x1.min(x2), x1.max(x2), y1);
            } else if x1 == x2 {
                self.block(x1, y1.min(y2), 1, dy + 1);
            } else {
                let (sx, sy) = (if x1 < x2 { 1 } else { -1 }, if y1 < y2 { 1 } else { -1 });
                let mut err = (if dx > dy { dx } else { -dy }) / 2;
                while x1 != x2 || y1 != y2 {
                    self.dot(x1, y1);
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
                self.dot(x2, y2);
            }
            return;
        }
        let extra = 1 - width % 2;
        let half = width / 2;
        let across = 2 * half + 1 - extra;
        let xinc = dx <= dy; // thick across x (the line is upright), else across y
        if x1 == x2 && y1 == y2 {
            self.span(x1 - half + extra, x1 + half, y1);
        } else if x1 == x2 {
            self.block(x1 - half + extra, y1.min(y2), across, dy + 1);
        } else if y1 == y2 {
            self.block(x1.min(x2), y1 - half + extra, dx + 1, across);
        } else {
            let (sx, sy) = (if x2 > x1 { 1 } else { -1 }, if y2 > y1 { 1 } else { -1 });
            let mut err = (if dx > dy { dx } else { -dy }) / 2;
            loop {
                if xinc {
                    if y1 == y2 + sy {
                        break;
                    }
                    self.span(x1 - half + extra, x1 + half, y1);
                } else {
                    if x1 == x2 + sx {
                        break;
                    }
                    self.block(x1, y1 - half + extra, 1, across);
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

    /// `pygame.draw.lines` (open): each segment a line.
    pub fn lines(&mut self, pts: &[(f64, f64)], width: f64) {
        for w in pts.windows(2) {
            self.line(w[0], w[1], width);
        }
    }

    /// `pygame.draw.rect`, filled, with `border_radius` (its corners pygame's circle quadrants).
    pub fn rect(&mut self, r: PRect, radius: f64) {
        let (x, y, w, h) = (r.x as i32, r.y as i32, r.w as i32, r.h as i32);
        if w <= 0 || h <= 0 {
            return;
        }
        let rad = (radius as i32).min(w / 2).min(h / 2);
        if rad <= 0 {
            self.block(x, y, w, h);
            return;
        }
        let rows = circle_rows(rad);
        let (l, rt) = (x + rad, x + w - rad); // the corner circles' centres (columns)
        for (cy, ys) in [(y + rad, y..y + rad), (y + h - rad, y + h - rad..y + h)] {
            for py in ys {
                if let Some(&(a, b)) = rows.get(&(py - cy)) {
                    self.span((l + a).max(x), (rt + b).min(x + w - 1), py);
                }
            }
        }
        self.block(x, y + rad, w, h - 2 * rad);
    }

    /// `pygame.draw.circle`, filled (its centre truncated).
    pub fn circle(&mut self, c: (f64, f64), r: f64) {
        let (cx, cy, r) = (int_(c.0) as i32, int_(c.1) as i32, r as i32);
        if r < 1 {
            return;
        }
        for (dy, (a, b)) in circle_rows(r) {
            self.span(cx + a, cx + b, cy + dy);
        }
    }

    /// `pygame.draw.polygon`, filled (pygame-ce's draw_fillpoly; its points truncated).
    pub fn polygon(&mut self, pts: &[(f64, f64)]) {
        let xs: Vec<i32> = pts.iter().map(|p| int_(p.0) as i32).collect();
        let ys: Vec<i32> = pts.iter().map(|p| int_(p.1) as i32).collect();
        let n = xs.len();
        if n < 3 {
            return;
        }
        let (miny, maxy) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        if miny == maxy {
            self.span(*xs.iter().min().unwrap(), *xs.iter().max().unwrap(), miny);
            return;
        }
        let mut inter = Vec::with_capacity(n);
        for y in miny..=maxy {
            inter.clear();
            for i in 0..n {
                let ip = if i > 0 { i - 1 } else { n - 1 };
                let (y1, y2, x1, x2) = match ys[ip].cmp(&ys[i]) {
                    std::cmp::Ordering::Less => (ys[ip], ys[i], xs[ip], xs[i]),
                    std::cmp::Ordering::Greater => (ys[i], ys[ip], xs[i], xs[ip]),
                    std::cmp::Ordering::Equal => continue,
                };
                if (y1 <= y && y < y2) || (y == maxy && y2 == maxy) {
                    inter.push((y - y1) * (x2 - x1) / (y2 - y1) + x1);
                }
            }
            inter.sort_unstable();
            for pair in inter.chunks_exact(2) {
                self.span(pair[0], pair[1], y);
            }
        }
        for i in 0..n {
            let ip = if i > 0 { i - 1 } else { n - 1 };
            let y = ys[i];
            if miny < y && ys[ip] == y && y < maxy {
                self.span(xs[i].min(xs[ip]), xs[i].max(xs[ip]), y);
            }
        }
    }

    /// Draw what's been lit, in one colour.
    pub fn draw(&mut self, p: &Painter, color: Color32) {
        if self.0.is_empty() {
            return;
        }
        let s = p.pixels_per_point();
        let mut mesh = Mesh::default();
        for &[x0, y0, x1, y1] in &self.0 {
            mesh.add_colored_rect(
                Rect::from_min_max(
                    pos2(x0 as f32 / s, y0 as f32 / s),
                    pos2(x1 as f32 / s, y1 as f32 / s),
                ),
                color,
            );
        }
        p.add(Shape::mesh(mesh));
        self.0.clear();
    }
}

/// One shape in one colour, pixel for pixel.
fn pg(p: &Painter, color: Color32, f: impl FnOnce(&mut Pix)) {
    let mut pix = Pix::default();
    f(&mut pix);
    pix.draw(p, color);
}

/// `neu.wash`: `rect` tinted towards `color`.
fn wash(p: &Painter, r: PRect, color: Color32, alpha: f32) {
    if r.w >= 1.0 && r.h >= 1.0 {
        paint::wash(p, r.rect(), color, alpha);
    }
}

/// An ellipse's outline (or its lower half: a tie) inside a pygame rect, anti-aliased along pygame's pixels.
fn ellipse(p: &Painter, r: PRect, width: f64, lower_half: bool, col: Color32) {
    if r.w < 2.0 || r.h < 2.0 {
        return;
    }
    let s = sc();
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (rx, ry) = (r.w / 2.0 - width / 2.0, r.h / 2.0 - width / 2.0);
    let n = 24;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = if lower_half {
                std::f64::consts::PI * i as f64 / n as f64
            } else {
                std::f64::consts::TAU * i as f64 / n as f64
            };
            pos2(((cx + rx * a.cos()) / s) as f32, ((cy + ry * a.sin()) / s) as f32)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new((width / s) as f32, col)));
}

#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    pub start: f64,
    pub end: f64,
    pub number: i32,
    pub beats: usize,
    pub row: usize,
    /// Where it's drawn across the page, in the Python app's pixels.
    pub x: f64,
    pub w: f64,
}

/// A bent note's amount in half steps ("1", "0.5"), or "".
pub fn bend_text(g: &Glyph) -> String {
    if g.tech.has(Techs::BEND) && g.bend != 0.0 {
        let s = format!("{}", (g.bend * 1000.0).round() / 1000.0);
        s.trim_end_matches(".0").to_string()
    } else {
        String::new()
    }
}

/// What a click on the tab or its toolbar asks the player to do.
#[derive(Clone, Debug, PartialEq)]
pub enum TabAct {
    Seek(f64),
    Selected,
    Act(&'static str),
}

/// What the tab needs from the player to draw.
pub struct TabCtx<'a> {
    pub chart: &'a Chart,
    pub states: &'a [Option<qrs_analysis::Judgement>],
    pub now: f64,
    pub loop_ab: Option<(f64, f64)>,
    pub synth_t: Option<f64>,
    pub synth_state: qrs_audio::synth::SynthState,
    pub playing: bool,
    pub notation: bool,
    pub snap: bool,
    pub beams: bool,
    pub sel_loop: bool,
    pub sel_slow: f64,
    pub sel_track: bool,
    pub track_ok: bool,
    pub selection_speed: f64,
}

pub struct TabView {
    pub zoom: f32,
    /// How far the page is scrolled (points; kept from `scroll_px`, and taken up again if changed).
    pub scroll: f32,
    /// The page tracks the cursor until you scroll away; the lock key brings it back.
    pub follow: bool,
    /// Go straight to the cursor's row next frame instead of gliding there.
    pub jump: bool,
    /// Screenshot mode: after that jump, scroll this far (points) and stop following.
    pub nudge: Option<f32>,
    /// How far the page can scroll (points).
    pub max_scroll: f32,
    /// The scroll and its most, in the Python app's pixels (its `scroll`, `max_scroll`).
    scroll_px: f64,
    max_scroll_px: f64,
    idle: f32,
    last: Instant,
    /// (pointer y, scroll) when the scrollbar's thumb was grabbed (pixels).
    drag: Option<(f64, f64)>,
    /// Ctrl-drag selection (chart seconds), played by the synth.
    pub sel: Option<(f64, f64)>,
    sel_anchor: Option<f64>,
    sel_press: Pos2,
    sel_bar: Option<usize>,
    // the layout, in the Python app's pixels
    /// The well's inside, where rows are drawn (clipped), and the screen y of the first row at scroll 0.
    region: PRect,
    top: f64,
    key: Option<(u64, u64, u64, u64, u32, u32, u64)>,
    pub measures: Vec<Measure>,
    mstarts: Vec<f64>,
    rows: Vec<Vec<usize>>,
    names_x: f64,
    left: f64,
    row_w: f64,
    line_gap: f64,
    staff_h: f64,
    top_m: f64,
    row_h: f64,
    pad: f64,
    note_k: f64,
    fret_size: f32,
    chip_h: f64,
    chip_pad: f64,
}

impl Default for TabView {
    fn default() -> Self {
        TabView {
            zoom: 1.0,
            scroll: 0.0,
            follow: true,
            jump: false,
            nudge: None,
            max_scroll: 0.0,
            scroll_px: 0.0,
            max_scroll_px: 0.0,
            idle: 0.0,
            last: Instant::now(),
            drag: None,
            sel: None,
            sel_anchor: None,
            sel_press: Pos2::ZERO,
            sel_bar: None,
            region: PRect::default(),
            top: 0.0,
            key: None,
            measures: Vec::new(),
            mstarts: Vec::new(),
            rows: Vec::new(),
            names_x: 0.0,
            left: 0.0,
            row_w: 0.0,
            line_gap: 22.0,
            staff_h: 110.0,
            top_m: 72.0,
            row_h: 200.0,
            pad: 12.0,
            note_k: 1.0,
            fret_size: 15.0,
            chip_h: 22.0,
            chip_pad: 10.0,
        }
    }
}

fn lower_bound(v: &[f64], x: f64) -> usize {
    v.partition_point(|&a| a < x)
}

/// A time to the millisecond, as Python's `round(t, 3)` (correctly rounded, as the rhythm's onsets are).
fn r3(t: f64) -> f64 {
    format!("{t:.3}").parse().unwrap_or(t)
}

fn upper_bound(v: &[f64], x: f64) -> usize {
    v.partition_point(|&a| a <= x)
}

/// `neu.keycap_width` (size 11), in pixels.
fn keycap_w(ctx: &egui::Context, key: &str) -> f64 {
    if key.is_empty() {
        0.0
    } else {
        tw(ctx, key, Style::new(11.0, 700, Color32::WHITE)) + rpx(12.0)
    }
}

/// `neu.button_width`, in pixels.
fn button_w(ctx: &egui::Context, label: &str, key: &str, size: f32, icon: bool) -> f64 {
    let mut w = tw(ctx, label, Style::new(size, 700, Color32::WHITE)) + rpx(28.0);
    if !key.is_empty() {
        w += keycap_w(ctx, key) + rpx(8.0);
    }
    if icon {
        w += rpx(22.0);
    }
    w
}

impl TabView {
    /// Let go of a drag or a selection being made (a panel opens over the tab).
    pub fn cancel_drags(&mut self) {
        self.drag = None;
        self.sel_anchor = None;
    }

    /// Where a chart time sits on the page: its snapped time when snapping is on. The rhythm's onsets are the
    /// chart's times to the millisecond, so it's looked up by the time rounded so (`round(t, 3)`, as the Python
    /// app does; the times themselves rarely are).
    pub fn tt(chart: &Chart, snap: bool, time: f64) -> f64 {
        match &chart.rhythm {
            Some(r) if snap => r.snapped.get(&r3(time).to_bits()).copied().unwrap_or(time),
            _ => time,
        }
    }

    fn measures_of(chart: &Chart) -> Vec<Measure> {
        let downs: Vec<&qrs_formats::model::Beat> = chart.beats.iter().filter(|b| b.is_downbeat()).collect();
        let end = chart
            .length
            .max(chart.gtimes.last().map(|t| t + 1.0).unwrap_or(0.0));
        if downs.is_empty() {
            // no bar lines in the chart: 2-second blocks
            let n = ((end / 2.0) as usize + 1).max(1);
            return (0..n)
                .map(|i| Measure {
                    start: i as f64 * 2.0,
                    end: (i + 1) as f64 * 2.0,
                    number: i as i32 + 1,
                    beats: 4,
                    row: 0,
                    x: 0.0,
                    w: 0.0,
                })
                .collect();
        }
        let mut out = Vec::new();
        for (i, d) in downs.iter().enumerate() {
            let stop = if i + 1 < downs.len() {
                downs[i + 1].time
            } else {
                end.min(d.time + 4.0 * 60.0 / 120.0)
            };
            let nb = lower_bound(&chart.btimes, stop) - lower_bound(&chart.btimes, d.time);
            out.push(Measure {
                start: d.time,
                end: stop.max(d.time + 0.1),
                number: d.measure,
                beats: nb.max(1),
                row: 0,
                x: 0.0,
                w: 0.0,
            });
        }
        if let Some(&first) = chart.gtimes.first()
            && first < out[0].start
        {
            // notes before the first bar line (a pickup)
            let s0 = out[0].start;
            out.insert(
                0,
                Measure {
                    start: (first - 0.2).min(s0 - 0.5),
                    end: s0,
                    number: 0,
                    beats: 1,
                    row: 0,
                    x: 0.0,
                    w: 0.0,
                },
            );
        }
        out
    }

    /// How wide a note's fret number is, with its bend's amount after it (pixels).
    fn label_w(&self, ctx: &egui::Context, token: &str, bend: &str) -> f64 {
        tw(ctx, token, Style::new(self.fret_size, 800, Color32::WHITE))
            + if bend.is_empty() {
                0.0
            } else {
                ipx(1.0) + tw(ctx, bend, Style::new(BEND_SIZE, 800, Color32::WHITE))
            }
    }

    /// Measures packed into rows across `content` (the well's inside, less its padding).
    fn layout(&mut self, ctx: &egui::Context, c: &TabCtx, content: PRect) {
        let chart = c.chart;
        let s = sc();
        let key = (
            chart.id,
            content.x.to_bits(),
            content.w.to_bits(),
            s.to_bits(),
            self.zoom.to_bits(),
            (c.notation as u32) | (c.snap as u32) << 1,
            chart.rhythm_id,
        );
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let zoom = self.zoom as f64;
        self.names_x = content.x + ipx(12.0); // string names, centred in the margin
        self.left = content.x + ipx(40.0);
        self.row_w = content.right() - self.left - ipx(6.0);
        self.line_gap = (22.0 * s * zoom.clamp(0.8, 1.3)).trunc();
        self.staff_h = self.line_gap * (chart.n_strings.max(1) - 1) as f64;
        // above the staff, top down: section labels, chord names, then a line of its own for the bar numbers
        self.top_m = ipx(72.0);
        let lane_h = if c.notation && chart.rhythm.is_some() {
            ipx(46.0)
        } else {
            0.0
        };
        let bot_m = ipx(26.0) + lane_h;
        self.row_h = self.top_m + self.staff_h + bot_m;
        self.pad = ipx(12.0);
        // the fret numbers (on chips) shrink a little when zoomed out, with the strings (never below 80%)
        self.note_k = zoom.clamp(0.8, 1.0);
        self.fret_size = (15.0 * self.note_k).round_ties_even() as f32;
        self.chip_h = (22.0 * s * self.note_k).trunc();
        self.chip_pad = (10.0 * s * self.note_k).trunc();
        let mut chip_ws: HashMap<(String, String), f64> = HashMap::new();
        let mut chip_w = |g: &Glyph| -> f64 {
            let k = (g.token.clone(), bend_text(g));
            if let Some(w) = chip_ws.get(&k) {
                return *w;
            }
            let w = self.chip_h.max(self.label_w(ctx, &k.0, &k.1) + self.chip_pad);
            chip_ws.insert(k, w);
            w
        };
        let beat_px = 34.0 * s * zoom;
        let min_px = 26.0 * s * zoom;
        let mut times: Vec<f64> = chart.gtimes.iter().map(|&t| r3(t)).collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        let mut ms = Self::measures_of(chart);
        // each bar's notes on each string, where they're drawn, with their chips' widths
        let starts: Vec<f64> = ms.iter().map(|m| m.start).collect();
        let mut strings: Vec<BTreeMap<i32, Vec<(f64, f64, f64)>>> = vec![Default::default(); ms.len()];
        for g in &chart.glyphs {
            let tt = Self::tt(chart, c.snap, g.t);
            let i = upper_bound(&starts, tt) as i64 - 1;
            if i >= 0 && (i as usize) < ms.len() {
                strings[i as usize]
                    .entry(g.string)
                    .or_default()
                    .push((tt, g.t, chip_w(g)));
            }
        }
        let clear = 3.0 * s; // at least this much screen between two chips on a string
        for (m, by_string) in ms.iter_mut().zip(strings.iter_mut()) {
            let (i0, i1) = (
                lower_bound(&times, m.start - 1e-3),
                lower_bound(&times, m.end - 1e-3),
            );
            let ts = &times[i0..i1.max(i0)];
            if ts.is_empty() {
                m.w = (40.0 * s).max(m.beats as f64 * 16.0 * s * zoom);
                continue;
            }
            let span = m.end - m.start;
            // (grace notes, strums: see below)
            let gaps = ts.windows(2).map(|w| w[1] - w[0]).filter(|&d| d >= GRACE);
            // the notes' rhythm stays readable...
            let mut need = match gaps.fold(None, |a: Option<f64>, b| Some(a.map_or(b, |a| a.min(b)))) {
                Some(g) => span / g * min_px,
                None => 0.0,
            };
            // ...and however far out you zoom, chips never overlap
            for notes in by_string.values_mut() {
                notes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                // the last run of grace notes (its first note) and the last note
                let (mut c_t, mut p_t, mut p_raw): (f64, Option<f64>, Option<f64>) = (0.0, None, None);
                // how far right of its first note the run's chips reach; the last chip's width
                let (mut c_right, mut p_w) = (0.0f64, 0.0f64);
                for &(tt, raw, w) in notes.iter() {
                    if p_raw.is_some_and(|p| (raw - p).abs() <= 1e-3) {
                        continue; // the same note twice
                    }
                    if p_raw.is_some_and(|p| raw - p < GRACE) {
                        c_right += clear + w; // a grace note: drawn right after the note before
                    } else {
                        if let Some(pt_) = p_t {
                            for (dt, room) in [
                                (tt - pt_, (p_w + w) / 2.0 + clear),
                                (tt - c_t, c_right + w / 2.0 + clear),
                            ] {
                                if dt > 1e-3 {
                                    need = need.max(span * room / dt);
                                }
                            }
                        }
                        c_t = tt;
                        c_right = w / 2.0;
                    }
                    p_t = Some(tt);
                    p_raw = Some(raw);
                    p_w = w;
                }
            }
            m.w = self
                .row_w
                .min((m.beats as f64 * beat_px).max(need + 2.0 * self.pad));
        }
        // pack into rows, then stretch each full row to the page width
        let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
        let mut used = 0.0f64;
        for (i, m) in ms.iter().enumerate() {
            if !rows.last().unwrap().is_empty() && used + m.w > self.row_w {
                rows.push(Vec::new());
                used = 0.0;
            }
            rows.last_mut().unwrap().push(i);
            used += m.w;
        }
        let nrows = rows.len();
        for (r, row) in rows.iter().enumerate() {
            let total: f64 = row.iter().map(|&i| ms[i].w).sum();
            let k = if (r < nrows - 1 || total > self.row_w * 0.7) && total != 0.0 {
                self.row_w / total
            } else {
                1.0
            };
            let mut x = self.left;
            for &i in row {
                ms[i].row = r;
                ms[i].x = x;
                ms[i].w *= k;
                x += ms[i].w;
            }
        }
        self.mstarts = ms.iter().map(|m| m.start).collect();
        self.measures = ms;
        self.rows = rows;
    }

    pub fn measure_index(&self, time: f64) -> usize {
        (upper_bound(&self.mstarts, time) as i64 - 1).max(0) as usize
    }

    pub fn measure_at(&self, time: f64) -> Option<&Measure> {
        if self.measures.is_empty() {
            return None;
        }
        Some(&self.measures[self.measure_index(time).min(self.measures.len() - 1)])
    }

    /// Where a chart time sits across the page (pixels).
    fn x_of(&self, time: f64, m: Option<&Measure>) -> f64 {
        let Some(m) = m.or_else(|| self.measure_at(time)) else {
            return self.left;
        };
        let frac = (time - m.start) / (m.end - m.start);
        m.x + self.pad + frac.clamp(0.0, 1.0) * (m.w - 2.0 * self.pad)
    }

    fn row_top(&self, r: usize) -> f64 {
        r as f64 * self.row_h
    }

    pub fn wheel(&mut self, notches: f32) {
        self.take_scroll();
        self.set_scroll(
            (self.scroll_px - notches as f64 * self.row_h * 0.5)
                .min(self.max_scroll_px)
                .max(0.0),
        );
        self.follow = false;
        self.idle = 0.0;
    }

    /// The scroll in pixels, and in points for the player.
    fn set_scroll(&mut self, px: f64) {
        self.scroll_px = px;
        self.scroll = (px / sc()) as f32;
    }

    /// If the scroll in points was changed from outside, the pixels follow it.
    fn take_scroll(&mut self) {
        if (self.scroll - (self.scroll_px / sc()) as f32).abs() > 1e-3 {
            self.scroll_px = px_of(self.scroll);
        }
    }

    /// A pointer position as the Python app's mouse pixels (whole).
    fn mouse_px(pos: Pos2) -> (f64, f64) {
        (px_of(pos.x).round(), px_of(pos.y).round())
    }

    /// (bar, chart time) under a point, clamped to the row it's over; None off the page.
    fn hit(&self, pos: Pos2) -> Option<(usize, f64)> {
        let (x, y) = Self::mouse_px(pos);
        if !self.region.contains(x, y) || self.rows.is_empty() {
            return None;
        }
        let r = ((y - self.top + self.scroll_px) / self.row_h).floor();
        if r < 0.0 || r as usize >= self.rows.len() {
            return None;
        }
        let row = &self.rows[r as usize];
        let (first, last) = (&self.measures[row[0]], &self.measures[*row.last()?]);
        let x = first.x.max(x.min(last.x + last.w - 1.0));
        for &i in row {
            let m = &self.measures[i];
            if m.x <= x && x < m.x + m.w {
                let frac = (x - m.x - self.pad) / (m.w - 2.0 * self.pad).max(1.0);
                return Some((i, m.start + frac.clamp(0.0, 1.0) * (m.end - m.start)));
            }
        }
        None
    }

    /// Select from t0 to t1 (either order), widened out to the beats on either side.
    fn set_sel(&mut self, chart: &Chart, t0: f64, t1: f64) {
        let (mut a, mut b) = (t0.min(t1), t0.max(t1));
        let bt = &chart.btimes;
        if !bt.is_empty() {
            let i = upper_bound(bt, a + 1e-3) as i64 - 1;
            if i >= 0 {
                a = bt[i as usize];
            }
            let j = lower_bound(bt, b - 1e-3);
            if j < bt.len() {
                b = bt[j];
            }
        }
        self.sel = Some((a, b));
    }

    /// The beat nearest a click (for seeking), or None off the page.
    fn time_at(&self, chart: &Chart, pos: Pos2) -> Option<f64> {
        let (x, y) = Self::mouse_px(pos);
        if !self.region.contains(x, y) {
            return None;
        }
        let r = ((y - self.top + self.scroll_px) / self.row_h).floor();
        if r < 0.0 || r as usize >= self.rows.len() {
            return None;
        }
        for &i in &self.rows[r as usize] {
            let m = &self.measures[i];
            if m.x <= x && x < m.x + m.w {
                let frac = (x - m.x - self.pad) / (m.w - 2.0 * self.pad).max(1.0);
                return Some(chart.snap(m.start + frac.clamp(0.0, 1.0) * (m.end - m.start)));
            }
        }
        None
    }

    // -- rhythm notation --------------------------------------------------------------------------------------

    fn draw_rhythm(&self, p: &Painter, c: &TabCtx, top: f64, r_start: f64, r_end: f64) {
        let Some(rh) = &c.chart.rhythm else { return };
        let th = t();
        let s = sc();
        let times = &rh.piece_times;
        let (i0, i1) = (
            lower_bound(times, r_start - 1e-4),
            lower_bound(times, r_end - 1e-4),
        );
        let pieces = &rh.pieces[i0..i1.max(i0)];
        let stem = ipx(24.0);
        let bottom = top + stem;
        let mid = top + stem / 2.0;
        let w = ipx(1.5).max(1.0);
        let xs: Vec<f64> = pieces
            .iter()
            .map(|pc| match pc.onset {
                Some(o) if !c.snap => self.x_of(o, None), // unsnapped: stems stay under the numbers
                _ => self.x_of(pc.t, None),
            })
            .collect();
        let mut beamed = HashSet::new();
        if c.beams {
            // runs of neighbouring eighths-or-shorter in the same beat get beamed (a rest breaks the run)
            let mut runs: Vec<Vec<usize>> = Vec::new();
            let mut run: Vec<usize> = Vec::new();
            for (k, pc) in pieces.iter().enumerate() {
                let beamable = !pc.rest && pc.flags >= 1;
                if beamable
                    && run
                        .last()
                        .is_some_and(|&l| l + 1 == k && pieces[l].beat() == pc.beat())
                {
                    run.push(k);
                } else {
                    if run.len() >= 2 {
                        runs.push(std::mem::take(&mut run));
                    }
                    run = if beamable { vec![k] } else { Vec::new() };
                }
            }
            if run.len() >= 2 {
                runs.push(run);
            }
            let thick = ipx(3.0).max(2.0);
            let mut pix = Pix::default();
            for run in &runs {
                beamed.extend(run.iter().copied());
                // one beam per level: eighths 1, sixteenths 2, 32nds 3
                for lvl in 1..=3 {
                    let yb = bottom - (lvl - 1) as f64 * 5.0 * s;
                    for (idx, &k) in run.iter().enumerate() {
                        if pieces[k].flags < lvl {
                            continue;
                        }
                        let nxt = run.get(idx + 1).copied();
                        let prv = if idx > 0 { Some(run[idx - 1]) } else { None };
                        if let Some(n) = nxt.filter(|&n| pieces[n].flags >= lvl) {
                            pix.line((xs[k], yb), (xs[n], yb), thick);
                        } else if prv.is_none_or(|pv| pieces[pv].flags < lvl) {
                            // alone at this level: a short stub
                            let d = if nxt.is_some() { 7.0 * s } else { -7.0 * s };
                            pix.line((xs[k], yb), (xs[k] + d, yb), thick);
                        }
                    }
                }
            }
            pix.draw(p, th.rhythm);
        }
        let mut prev_note_x: Option<f64> = None;
        for (k, (pc, &x)) in pieces.iter().zip(&xs).enumerate() {
            let col = if pc.t >= c.now - 0.05 {
                th.rhythm
            } else {
                th.bar_line
            };
            let mut pix = Pix::default();
            if pc.rest {
                rest(&mut pix, x, mid, pc.flags);
            } else if pc.flags == -2 {
                // whole note: no stem
                ellipse(
                    p,
                    PRect::new(x - 5.0 * s, mid - 3.0 * s, 10.0 * s, 6.0 * s),
                    w,
                    false,
                    col,
                );
            } else {
                // half notes get a short stem
                let y_end = top + if pc.flags == -1 { stem / 2.0 } else { stem };
                pix.line((x, top), (x, y_end), w);
                if pc.flags >= 1 && !beamed.contains(&k) {
                    // flags
                    for lvl in 0..pc.flags {
                        let yb = bottom - lvl as f64 * 5.0 * s;
                        pix.line((x, yb), (x + 7.0 * s, yb - 6.0 * s), ipx(2.0).max(2.0));
                    }
                }
                if pc.tied
                    && c.beams
                    && let Some(px) = prev_note_x
                    && x > px
                {
                    // the tie: an arc under the two notes
                    ellipse(
                        p,
                        PRect::new(px, bottom + 2.0 * s, x - px, 10.0 * s),
                        ipx(1.5).max(1.0),
                        true,
                        col,
                    );
                }
            }
            if pc.dots > 0 {
                pix.circle((x + 6.0 * s, mid + 4.0 * s), ipx(2.0).max(2.0));
            }
            pix.draw(p, col);
            if !pc.rest {
                prev_note_x = Some(x);
            }
        }
        // triplets: a "3" under each beat that is written in triplets
        let mut k = 0;
        while k < pieces.len() {
            if pieces[k].triplet {
                let mut j = k;
                while j + 1 < pieces.len()
                    && pieces[j + 1].triplet
                    && pieces[j + 1].beat() == pieces[k].beat()
                {
                    j += 1;
                }
                put(
                    p,
                    "3",
                    (xs[k] + xs[j]) / 2.0,
                    bottom + 12.0 * s,
                    Align2::CENTER_CENTER,
                    Style::new(10.0, 700, th.rhythm),
                    None,
                );
                k = j + 1;
            } else {
                k += 1;
            }
        }
    }

    // -- drawing ----------------------------------------------------------------------------------------------

    /// The screen with the rows of tab, the selection's toolbar and caption, and the scrollbar in `groove`.
    /// Returns what was clicked.
    pub fn ui(&mut self, ui: &Ui, c: &TabCtx, well: Rect, groove: Rect) -> Vec<TabAct> {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let th = t();
        let chart = c.chart;
        let s = sc();
        let mut acts = Vec::new();
        paint::display(&p, well, Some(24.0), 6.0);
        let well = PRect::of(well);
        let groove = PRect::of(groove);
        let (pad_x, pad_y) = (ipx(28.0), ipx(22.0));
        self.layout(&ctx, c, well.inflate(-2.0 * pad_x, -2.0 * pad_y));
        let d = ipx(10.0);
        let region = well.inflate(-d, -d); // rows are clipped just inside the well's edge
        self.region = region;
        self.top = well.y + pad_y;
        if self.measures.is_empty() {
            return acts;
        }
        let now = c.now;
        let cur_i = self.measure_index(now).min(self.measures.len() - 1);
        let cur = self.measures[cur_i].clone();
        // follow the cursor: keep its row in the upper third of the view (pixels)
        let target = (self.row_top(cur.row) - (self.row_h * 0.9).min(region.h * 0.3)).max(0.0);
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        if !self.follow && self.drag.is_none() && c.playing {
            // a long gap between draws (another view was showing) isn't playback
            self.idle += dt.min(0.1);
            if self.idle >= RELOCK_AFTER {
                self.follow = true;
            }
        }
        self.take_scroll();
        if std::mem::take(&mut self.jump) {
            self.scroll_px = target;
            if let Some(dn) = self.nudge.take() {
                self.scroll_px = (target + dn as f64 * s).max(0.0);
                self.follow = false;
            }
        }
        if self.follow {
            let before = self.scroll_px;
            self.scroll_px += (target - self.scroll_px) * (dt as f64 * 6.0).min(1.0);
            if (self.scroll_px - before).abs() > 0.05 {
                ctx.request_repaint();
            }
        }
        self.max_scroll_px = (self.rows.len() as f64 * self.row_h - region.h * 0.5).max(0.0);
        self.max_scroll = (self.max_scroll_px / s) as f32;
        self.set_scroll(self.scroll_px.min(self.max_scroll_px).max(0.0));

        // the page: a click seeks to the beat under it; Ctrl+drag (or Ctrl+click) selects
        let resp = interact(ui, region.rect(), "tab-page", Sense::click_and_drag());
        let ctrl = ui.input(|i| i.modifiers.command);
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.drag_started()
                && ctrl
                && let Some((bar, tm)) = self.hit(pos)
            {
                self.sel_bar = Some(bar);
                self.sel_anchor = Some(tm);
                self.sel_press = pos;
                self.set_sel(chart, tm, tm);
            } else if resp.dragged()
                && let Some(anchor) = self.sel_anchor
                && let Some((_, tm)) = self.hit(pos)
            {
                self.set_sel(chart, anchor, tm);
            }
        }
        if resp.drag_stopped() && self.sel_anchor.take().is_some() {
            let end = resp.interact_pointer_pos().unwrap_or(self.sel_press);
            // a Ctrl-click (no drag): the whole bar clicked on
            if (end.x - self.sel_press.x).abs() + (end.y - self.sel_press.y).abs() <= 4.0
                && let Some(b) = self.sel_bar
            {
                let m = &self.measures[b];
                self.sel = Some((m.start, m.end));
            }
            acts.push(TabAct::Selected);
        }
        if resp.clicked()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            if ctrl {
                if let Some((bar, _)) = self.hit(pos) {
                    let m = &self.measures[bar];
                    self.sel = Some((m.start, m.end));
                    acts.push(TabAct::Selected);
                }
            } else if let Some(tm) = self.time_at(chart, pos) {
                acts.push(TabAct::Seek(tm));
            }
        }

        let pc = p.with_clip_rect(region.rect());
        let n_strings = chart.n_strings.max(1) as i32;
        let scroll = self.scroll_px;
        // the next two note times after the cursor get a ring
        let mut nxt: Vec<f64> = Vec::new();
        for &tm in &chart.gtimes[upper_bound(&chart.gtimes, now + 0.06)..] {
            if !nxt.contains(&tm) {
                nxt.push(tm);
            }
            if nxt.len() == 2 {
                break;
            }
        }
        let first = ((scroll - (self.top - region.y)) / self.row_h).floor().max(0.0) as usize;
        let last = (((scroll + region.bottom() - self.top) / self.row_h)
            .floor()
            .max(0.0) as usize)
            .min(self.rows.len() - 1);
        let gap = self.line_gap;
        let lw = s.trunc().max(1.0);
        for r in first..=last {
            let row = &self.rows[r];
            let top = self.top + self.row_top(r) - scroll;
            let y0 = top + self.top_m; // the highest string
            let last_m = &self.measures[*row.last().unwrap()];
            let x1 = last_m.x + last_m.w;
            // the current bar's band: from its number to just below the bottom string
            let band_top = y0 - ipx(30.0);
            let band_h = y0 + self.staff_h + (gap * 0.45).max(self.chip_h / 2.0 + 2.0 * s) - band_top;
            let ys = |st: i32| y0 + (n_strings - 1 - st) as f64 * gap;
            for &mi in row {
                // the current bar: a lighter band on the screen
                if mi == cur_i {
                    let m = &self.measures[mi];
                    let band = PRect::new(m.x, band_top, m.w, band_h);
                    pg(&pc, th.screen_hi, |px| px.rect(band, ipx(10.0)));
                }
            }
            if let Some((la_t, lb_t)) = c.loop_ab {
                // the loop: a faint accent wash, accent lines at its ends
                let mut edges = Pix::default();
                for &mi in row {
                    let m = &self.measures[mi];
                    if m.end > la_t && m.start < lb_t {
                        let la = if la_t <= m.start {
                            m.x
                        } else {
                            self.x_of(la_t, Some(m))
                        };
                        let lb = if lb_t >= m.end {
                            m.x + m.w
                        } else {
                            self.x_of(lb_t, Some(m))
                        };
                        wash(
                            &pc,
                            PRect {
                                x: int_(la),
                                y: int_(band_top),
                                w: int_(lb - la).max(1.0),
                                h: int_(band_h),
                            },
                            th.accent,
                            LOOP_WASH,
                        );
                        for (edge, te) in [(la, la_t), (lb, lb_t)] {
                            if m.start <= te && te < m.end {
                                edges.line(
                                    (int_(edge), int_(band_top)),
                                    (int_(edge), int_(band_top + band_h)),
                                    ipx(2.0).max(2.0),
                                );
                                edges.draw(&pc, th.accent);
                            }
                        }
                    }
                }
            }
            if let Some((sa_t, sb_t)) = self.sel {
                // the selection: tinted, ringed in the accent
                let mut segs: Vec<(f64, f64)> = Vec::new();
                for &mi in row {
                    let m = &self.measures[mi];
                    if m.end > sa_t && m.start < sb_t {
                        let sa = if sa_t <= m.start {
                            m.x
                        } else {
                            self.x_of(sa_t, Some(m))
                        };
                        let sb = if sb_t >= m.end {
                            m.x + m.w
                        } else {
                            self.x_of(sb_t, Some(m))
                        };
                        segs.push((sa, sb));
                    }
                }
                if !segs.is_empty() {
                    let sa = segs.iter().map(|s| s.0).fold(f64::MAX, f64::min);
                    let sb = segs.iter().map(|s| s.1).fold(f64::MIN, f64::max);
                    let bx = PRect {
                        x: int_(sa),
                        y: int_(band_top) - ipx(2.0),
                        w: ipx(8.0).max(int_(sb - sa)),
                        h: int_(band_h) + ipx(6.0),
                    };
                    wash(&pc, bx, th.accent, SEL_WASH);
                    paint::ring(&pc, bx.rect(), Some(14.0), 2.0, th.accent, false);
                }
            }
            // strings and their names
            for st in 0..n_strings {
                let y = ys(st);
                pg(&pc, th.line, |px| px.line((self.left, y), (x1, y), lw));
                let name = chart.string_names.get(st as usize).copied().unwrap_or("?");
                put(
                    &pc,
                    name,
                    self.names_x,
                    y,
                    Align2::CENTER_CENTER,
                    Style::new(13.0, 800, string_color(st as usize)),
                    None,
                );
            }
            // bar lines, and the bar numbers on their line, clear above the top string's notes
            let mut bars = Pix::default();
            for &mi in row {
                let m = &self.measures[mi];
                bars.line((int_(m.x), y0), (int_(m.x), y0 + self.staff_h), lw);
                if m.number > 0 {
                    put(
                        &pc,
                        &m.number.to_string(),
                        m.x + ipx(6.0),
                        y0 - ipx(14.0),
                        Align2::LEFT_BOTTOM,
                        Style::new(12.0, 700, th.caption),
                        None,
                    );
                }
            }
            bars.line((int_(x1), y0), (int_(x1), y0 + self.staff_h), lw);
            bars.draw(&pc, th.bar_line);
            let (r_start, r_end) = (self.measures[row[0]].start, last_m.end);
            for sec in &chart.sections {
                // section names on small sunken labels at the top of the row
                if r_start <= sec.start && sec.start < r_end {
                    let label = section_label(sec).to_uppercase();
                    let st = Style::caps(10.0, th.text_2).tracked(0.1);
                    let pill = PRect {
                        x: int_(self.x_of(sec.start, None)) - ipx(4.0),
                        y: int_(top + 2.0 * s),
                        w: tw(&ctx, &label, st) + ipx(16.0),
                        h: ipx(20.0),
                    };
                    paint::inset(&pc, pill.rect(), Some(7.0), 2.0, Fill::Base);
                    put(
                        &pc,
                        &label,
                        pill.centerx(),
                        pill.centery(),
                        Align2::CENTER_CENTER,
                        st,
                        None,
                    );
                }
            }
            // chord names, above the current bar's panel
            let (c0, c1) = (
                lower_bound(&chart.ctimes, r_start),
                lower_bound(&chart.ctimes, r_end),
            );
            let mut last_name: Option<&str> = None;
            let mut last_x = -1e9f64;
            for ch in &chart.chords[c0..c1.max(c0)] {
                let cx = self.x_of(Self::tt(chart, c.snap, ch.time), None);
                if !ch.name.is_empty() && (last_name != Some(ch.name.as_str()) || cx - last_x > 160.0 * s) {
                    put(
                        &pc,
                        &ch.name,
                        cx,
                        band_top - ipx(1.0),
                        Align2::CENTER_BOTTOM,
                        Style::new(13.0, 800, th.accent_ink),
                        None,
                    );
                    last_name = Some(ch.name.as_str());
                    last_x = cx;
                }
            }
            // the cursor, under the notes so their knock-outs mask it instead of it slicing through digits
            if cur.row == r {
                // a glowing line with a small marker on top
                let cx = int_(self.x_of(now, Some(&cur)));
                let cw = ipx(3.0).max(2.0);
                let bar = PRect {
                    x: cx - (cw / 2.0).floor(),
                    y: int_(y0 - gap * 0.64),
                    w: cw,
                    h: int_(self.staff_h + gap * 1.1),
                };
                paint::glow(&pc, bar.rect(), th.accent, 6.0, None, None);
                let mk = 6.0 * s;
                pg(&pc, th.accent, |px| {
                    px.rect(bar, ipx(2.0).max(1.0));
                    px.polygon(&[
                        (cx - mk, bar.y - 8.0 * s),
                        (cx + mk, bar.y - 8.0 * s),
                        (cx, bar.y - 1.0 * s),
                    ]);
                });
            }
            if let Some(stt) = c.synth_t
                && r_start <= stt
                && stt < r_end
            {
                // the synth's cursor: white, so it can't be taken for the (paused) playback cursor
                let sx = int_(self.x_of(stt, None));
                let bar = PRect {
                    x: sx - lw,
                    y: int_(y0 - gap * 0.64),
                    w: ipx(2.0).max(2.0),
                    h: int_(self.staff_h + gap * 1.1),
                };
                pg(&pc, th.text, |px| px.rect(bar, lw));
            }
            // notes: fret numbers on knock-outs of the screen's colour; the ones being played now lit
            let (g0, g1) = (
                lower_bound(&chart.gtimes, r_start),
                lower_bound(&chart.gtimes, r_end),
            );
            let mut pm_runs: Vec<f64> = Vec::new();
            // each string's note before: (time, its chip's right edge)
            let mut before: HashMap<i32, (f64, f64)> = HashMap::new();
            for g in &chart.glyphs[g0..g1.max(g0)] {
                let x = self.x_of(Self::tt(chart, c.snap, g.t), None);
                let y = ys(g.string);
                let state = g.group.and_then(|i| c.states.get(i).copied().flatten());
                let now_note = (g.t - now).abs() <= 0.06;
                let col = if now_note {
                    th.on_accent
                } else if let Some(s) = state {
                    judge_color(match s {
                        qrs_analysis::Judgement::Hit => "hit",
                        qrs_analysis::Judgement::Late => "late",
                        qrs_analysis::Judgement::Early => "early",
                        qrs_analysis::Judgement::Miss => "miss",
                    })
                } else if g.t < now - 0.06 {
                    th.caption
                } else {
                    th.text
                };
                let bend = bend_text(g);
                let text_w = self.label_w(&ctx, &g.token, &bend);
                let (bw, bh) = (self.chip_h.max(text_w + self.chip_pad), self.chip_h);
                let mut bx = PRect {
                    x: int_(x) - (bw / 2.0).floor(),
                    y: int_(y) - (bh / 2.0).floor(),
                    w: bw,
                    h: bh,
                };
                if let Some(&(pt_, pr)) = before.get(&g.string)
                    && 1e-3 < g.t - pt_
                    && g.t - pt_ < GRACE
                    && bx.x < pr + ipx(3.0)
                {
                    bx.x = pr + ipx(3.0); // a grace note (or a flam): its chip goes just after the one before
                }
                before.insert(g.string, (g.t, bx.right()));
                let tok = Style::new(self.fret_size, 800, col);
                let chip = if now_note {
                    paint::glow(
                        &pc,
                        bx.rect(),
                        th.accent,
                        6.0,
                        None,
                        Some((11.0 * self.note_k) as f32),
                    );
                    th.accent
                } else {
                    let base = if cur.start <= g.t && g.t < cur.end {
                        th.screen_hi
                    } else {
                        th.screen
                    };
                    match c.loop_ab {
                        Some((a, b)) if a <= g.t && g.t < b => mix(base, th.accent, LOOP_WASH),
                        _ => base,
                    }
                };
                pg(&pc, chip, |px| px.rect(bx, (bx.h / 2.0).floor()));
                let tr = put(
                    &pc,
                    &g.token,
                    bx.centerx() - (text_w / 2.0).floor(),
                    bx.centery(),
                    Align2::LEFT_CENTER,
                    tok,
                    None,
                );
                if !bend.is_empty() {
                    // how far it bends, raised after the fret number, inside the chip
                    put(
                        &pc,
                        &bend,
                        tr.right() + ipx(1.0),
                        tr.centery() + ipx(2.0),
                        Align2::LEFT_BOTTOM,
                        Style::new(
                            BEND_SIZE,
                            800,
                            if now_note { th.on_accent } else { th.accent_ink },
                        ),
                        None,
                    );
                }
                if nxt.contains(&g.t) && !now_note {
                    // the next notes to play
                    paint::ring(
                        &pc,
                        bx.rect(),
                        Some((11.0 * self.note_k) as f32),
                        1.5,
                        th.accent,
                        false,
                    );
                }
                if g.tech.has(Techs::PALM_MUTE) {
                    pm_runs.push(x);
                }
            }
            // palm mute marks under the staff
            if !pm_runs.is_empty() {
                let yb = y0 + self.staff_h + ipx(14.0);
                pm_runs.sort_by(f64::total_cmp);
                let mut start = pm_runs[0];
                let mut prev = pm_runs[0];
                let it = pm_runs[1..].iter().map(|&x| Some(x)).chain(std::iter::once(None));
                for x in it {
                    if x.is_none_or(|x| x - prev > 60.0 * s) {
                        put(
                            &pc,
                            "PM",
                            start,
                            yb,
                            Align2::LEFT_CENTER,
                            Style::new(10.0, 800, th.caption),
                            None,
                        );
                        if prev - start > 30.0 * s {
                            pg(&pc, th.caption, |px| {
                                px.line((start + 20.0 * s, yb), (prev + 6.0 * s, yb), lw)
                            });
                        }
                        if let Some(x) = x {
                            start = x;
                        }
                    }
                    if let Some(x) = x {
                        prev = x;
                    }
                }
            }
            if c.notation {
                self.draw_rhythm(&pc, c, y0 + self.staff_h + ipx(10.0), r_start, r_end);
            }
        }
        acts.extend(self.draw_sel_buttons(ui, c, well));
        self.draw_scrollbar(ui, groove, region, target);
        acts
    }

    /// A floating toolbar under the selection: what it plays from (an abalone key: a sine wave for the synth
    /// guitar, a waveform for the song's own track), then Play or Stop, Loop, Slow and x in tortoiseshell; and a
    /// caption saying what's selected.
    fn draw_sel_buttons(&mut self, ui: &Ui, c: &TabCtx, well: PRect) -> Vec<TabAct> {
        let mut acts = Vec::new();
        let Some((sa, sb)) = self.sel else { return acts };
        if self.sel_anchor.is_some() {
            return acts;
        }
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let th = t();
        let s = sc();
        use qrs_audio::synth::SynthState;
        let slow = c.sel_slow < 0.999;
        let (play, icon) = match c.synth_state {
            SynthState::Rendering => ("Preparing…", None),
            SynthState::Playing => ("Stop", Some(Icon::Stop)),
            SynthState::Idle => ("Play", Some(Icon::Play)),
        };
        let (bh, gap, pad) = (ipx(44.0), ipx(8.0), ipx(8.0));
        let track = c.sel_track && c.track_ok;
        let slow_label = if slow {
            format!("Slow {}%", (c.sel_slow * 100.0).round_ties_even() as i64)
        } else {
            "Slow".to_string()
        };
        let items: [(&str, &str, &'static str, f64); 5] = [
            ("", "", "sel_source", bh),
            (
                play,
                "P",
                "sel_play",
                button_w(&ctx, play, "P", 14.0, icon.is_some()),
            ),
            // (its light)
            (
                "Loop",
                "",
                "sel_loop",
                button_w(&ctx, "Loop", "", 14.0, false) + ipx(15.0),
            ),
            // (as wide in all three states)
            (
                &slow_label,
                "S",
                "sel_slow",
                button_w(&ctx, "Slow 70%", "S", 14.0, false) + ipx(18.0),
            ),
            ("", "", "sel_clear", bh),
        ];
        let total = 2.0 * pad + items.iter().map(|i| i.3).sum::<f64>() + gap * (items.len() as f64 - 1.0);
        let mi = self.measure_index(sb - 1e-3).min(self.measures.len() - 1);
        let m = self.measures[mi].clone();
        let row = &self.rows[m.row];
        let first = &self.measures[row[0]];
        let x0 = if sa <= first.start {
            first.x
        } else {
            self.x_of(sa, None)
        };
        let x1 = if sb < m.end {
            self.x_of(sb, Some(&m))
        } else {
            m.x + m.w
        };
        let x = int_((x0 + x1) / 2.0 - total / 2.0);
        let y = int_(
            self.top + self.row_top(m.row) - self.scroll_px
                + self.top_m
                + self.staff_h
                + self.line_gap * 0.64
                + 12.0 * s,
        );
        let x = (well.x + ipx(12.0)).max(x.min(well.right() - ipx(12.0) - total));
        let y = (well.y + ipx(12.0)).max(y.min(well.bottom() - bh - 2.0 * pad - ipx(40.0)));
        let bx = PRect {
            x,
            y,
            w: total,
            h: bh + 2.0 * pad,
        };
        paint::raised(&p, bx.rect(), Some(18.0), 8.0, Fill::Base, 0.0);
        // a click on the toolbar's plate doesn't reach the tab under it
        let _ = interact(ui, bx.rect(), "sel-plate", Sense::click());
        let mut kx = bx.x + pad;
        let mut tip: Option<(PRect, String)> = None;
        for (i, (label, key, action, w)) in items.iter().enumerate() {
            let r = PRect {
                x: kx,
                y: bx.y + pad,
                w: *w,
                h: bh,
            };
            let st = ButtonStyle {
                size: 14.0,
                r: Some(13.0),
                mat: Some(Mat::Tortoise),
                seed: i as u32,
                ..Default::default()
            };
            let resp = match *action {
                "sel_source" => {
                    let resp = widgets::button(
                        ui,
                        r.rect(),
                        "sel-source",
                        "",
                        "",
                        ButtonStyle {
                            size: 13.0,
                            mat: Some(Mat::Abalone),
                            icon: Some(if track { Icon::Track } else { Icon::Synth }),
                            icon_size: 22.0,
                            enabled: c.track_ok,
                            seed: 0,
                            ..st
                        },
                    );
                    if resp.hovered() {
                        tip = Some((
                            r,
                            if !c.track_ok {
                                "Plays the synth guitar (no song audio)".into()
                            } else if track {
                                "Plays the song  ·  click: the synth".into()
                            } else {
                                "Plays the synth guitar  ·  click: the song".into()
                            },
                        ));
                    }
                    resp
                }
                "sel_play" => widgets::button(
                    ui,
                    r.rect(),
                    "sel-play",
                    label,
                    key,
                    ButtonStyle {
                        accent: true,
                        icon,
                        ..st
                    },
                ),
                "sel_loop" => widgets::button(
                    ui,
                    r.rect(),
                    "sel-loop",
                    label,
                    "",
                    ButtonStyle {
                        active: c.sel_loop,
                        ..st
                    },
                ),
                "sel_slow" => widgets::button(
                    ui,
                    r.rect(),
                    "sel-slow",
                    label,
                    key,
                    ButtonStyle { active: slow, ..st },
                ),
                _ => widgets::button(
                    ui,
                    r.rect(),
                    "sel-clear",
                    "",
                    "",
                    ButtonStyle {
                        size: 13.0,
                        icon: Some(Icon::X),
                        ..st
                    },
                ),
            };
            if resp.clicked() {
                acts.push(TabAct::Act(action));
            }
            kx += w + gap;
        }
        if let Some((r, tip)) = tip {
            // what the first key does, over it
            let st = Style::new(13.0, 700, th.text);
            let pw = tw(&ctx, &tip, st) + ipx(28.0);
            let ph = ipx(34.0);
            let plate = PRect {
                x: (well.x + ipx(8.0))
                    .max((r.centerx() - (pw / 2.0).floor()).min(well.right() - ipx(8.0) - pw)),
                y: bx.y - ipx(8.0) - ph,
                w: pw,
                h: ph,
            };
            paint::raised(&p, plate.rect(), Some(14.0), 4.0, Fill::Base, 0.0);
            put(
                &p,
                &tip,
                plate.centerx(),
                plate.centery(),
                Align2::CENTER_CENTER,
                st,
                None,
            );
        }
        // what's selected, at the bottom of the well
        let a = self.measure_at(sa).map(|m| m.number).unwrap_or(0);
        let bars = if a == m.number {
            format!("Bar {a}")
        } else {
            format!("Bars {a}–{}", m.number)
        };
        let speed = (c.selection_speed * 100.0).round_ties_even() as i64;
        let caption = format!(
            "{bars} selected  ·  plays {}{}  ·  Esc clears it",
            if track { "the song" } else { "on the synth guitar" },
            if speed != 100 {
                format!(" at {speed}% speed")
            } else {
                String::new()
            }
        );
        let st = Style::new(13.0, 500, th.caption);
        let h = th_px(&ctx, st);
        let tr = PRect {
            x: well.x + ipx(28.0),
            y: well.bottom() - ipx(18.0) - h,
            w: tw(&ctx, &caption, st),
            h,
        };
        pg(&p, th.screen, |px| {
            px.rect(tr.inflate(ipx(16.0), ipx(6.0)), ipx(8.0))
        });
        put(&p, &caption, tr.x, tr.y, Align2::LEFT_TOP, st, None);
        acts
    }

    /// A sunken groove beside the screen with a raised thumb and an accent mark where the cursor is; when you've
    /// scrolled away, a lock key in its middle goes back to following the cursor. `cursor_scroll` in pixels.
    fn draw_scrollbar(&mut self, ui: &Ui, groove: PRect, region: PRect, cursor_scroll: f64) {
        let p = ui.painter().clone();
        let th = t();
        if self.max_scroll_px <= 0.0 {
            self.follow = true;
            self.drag = None;
            paint::inset(&p, groove.rect(), None, 3.0, Fill::Base);
            return;
        }
        let max_scroll = self.max_scroll_px;
        let thumb_h = ipx(40.0).max(int_(groove.h * region.h / (max_scroll + region.h)));
        let room = groove.h - thumb_h;
        let thumb = PRect {
            x: groove.x,
            y: groove.y + int_(room * self.scroll_px / max_scroll),
            w: groove.w,
            h: thumb_h,
        };
        let resp = interact(
            ui,
            groove.inflate(ipx(10.0), 0.0).rect(),
            "tab-scroll",
            Sense::click_and_drag(),
        );
        if let Some(pos) = resp.interact_pointer_pos()
            && resp.is_pointer_button_down_on()
        {
            let (mx, my) = Self::mouse_px(pos);
            if self.drag.is_none() {
                self.follow = false;
                self.idle = 0.0;
                if !thumb.contains(mx, my) {
                    // click the track: the thumb jumps there
                    let frac = (my - thumb.h / 2.0 - groove.y) / room.max(1.0);
                    self.set_scroll((frac * max_scroll).min(max_scroll).max(0.0));
                }
                self.drag = Some((my, self.scroll_px));
            } else if let Some((y0, s0)) = self.drag {
                self.set_scroll(
                    (s0 + (my - y0) * max_scroll / room.max(1.0))
                        .min(max_scroll)
                        .max(0.0),
                );
                self.idle = 0.0;
            }
        } else {
            self.drag = None;
        }
        let hot = self.drag.is_some() || resp.hovered();
        let cy = groove.y + room * (cursor_scroll / max_scroll).min(1.0) + thumb_h / 2.0;
        widgets::scroll_groove(
            &p,
            groove.rect(),
            thumb.rect(),
            Some(pt(int_(cy))),
            hot,
            Some(Mat::Tortoise),
        );
        if hot {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if self.follow {
            return;
        }
        let (lw_, lh) = (ipx(34.0), ipx(34.0));
        let lock = PRect {
            x: groove.centerx() - (lw_ / 2.0).floor(),
            y: region.centery() - (lh / 2.0).floor(),
            w: lw_,
            h: lh,
        };
        let lr = widgets::round_button(
            ui,
            lock.rect(),
            "tab-lock",
            Icon::Lock,
            16.0,
            RoundKind::Mat(Mat::Tortoise, 3, None),
            false,
        );
        if lr.clicked() {
            self.follow = true;
        }
        if lr.hovered() {
            let st = Style::new(13.0, 700, th.text);
            let (pw, ph) = (tw(ui.ctx(), "Follow playback", st) + ipx(28.0), ipx(34.0));
            let pill = PRect {
                x: lock.x - ipx(10.0) - pw,
                y: lock.centery() - (ph / 2.0).floor(),
                w: pw,
                h: ph,
            };
            paint::raised(&p, pill.rect(), Some(14.0), 4.0, Fill::Base, 0.0);
            put(
                &p,
                "Follow playback",
                pill.centerx(),
                pill.centery(),
                Align2::CENTER_CENTER,
                st,
                None,
            );
        }
    }
}

/// A rest centred on (x, ym) (pixels), as `TabView._rest` draws it.
fn rest(pix: &mut Pix, x: f64, ym: f64, flags: i32) {
    let s = sc();
    if flags <= -1 {
        // whole rest hangs below a line, half rest sits on it
        let y = if flags == -2 { ym - 4.0 * s } else { ym };
        let line_y = if flags == -2 { ym } else { ym + 4.0 * s };
        pix.rect(PRect::new(x - 5.0 * s, y, 10.0 * s, 4.0 * s), 0.0);
        pix.line((x - 8.0 * s, line_y), (x + 8.0 * s, line_y), 1.0);
    } else if flags == 0 {
        // quarter rest: a zigzag
        let pts: Vec<(f64, f64)> = [(-2.0, -10.0), (3.0, -4.0), (-2.0, 1.0), (3.0, 6.0), (-1.0, 10.0)]
            .iter()
            .map(|(dx, dy)| (x + dx * s, ym + dy * s))
            .collect();
        pix.lines(&pts, ipx(2.0).max(2.0));
    } else {
        // eighth / sixteenth / 32nd rest: a slash with one hook per level
        pix.line(
            (x + 3.0 * s, ym - 7.0 * s),
            (x - 2.0 * s, ym + 8.0 * s),
            ipx(1.5).max(1.0),
        );
        for lvl in 0..flags.min(3) {
            pix.circle(
                (
                    x - 2.0 * s - lvl as f64 * 1.5 * s,
                    ym - 5.0 * s + lvl as f64 * 5.0 * s,
                ),
                ipx(2.5).max(2.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(pix: &Pix) -> std::collections::BTreeSet<(i32, i32)> {
        let mut out = std::collections::BTreeSet::new();
        for &[x0, y0, x1, y1] in &pix.0 {
            for y in y0..y1 {
                for x in x0..x1 {
                    out.insert((x, y));
                }
            }
        }
        out
    }

    #[test]
    fn pygame_shapes() {
        // pygame-ce 2.5: circle((10, 10), 2) lights a 4 x 4 block less its corners
        let mut pix = Pix::default();
        pix.circle((10.7, 10.2), 2.0);
        let want: std::collections::BTreeSet<(i32, i32)> = [
            (9, 8),
            (10, 8),
            (8, 9),
            (9, 9),
            (10, 9),
            (11, 9),
            (8, 10),
            (9, 10),
            (10, 10),
            (11, 10),
            (9, 11),
            (10, 11),
        ]
        .into_iter()
        .collect();
        assert_eq!(lit(&pix), want);
        // a 2 px line across 10..=11, its ends included; 3 px centred
        let mut pix = Pix::default();
        pix.line((10.2, 2.5), (10.2, 20.5), 2.0);
        assert_eq!(lit(&pix).len(), 2 * 19);
        assert!(lit(&pix).contains(&(11, 20)) && !lit(&pix).contains(&(9, 2)));
        let mut pix = Pix::default();
        pix.line((2.0, 10.0), (20.0, 10.0), 3.0);
        assert!(lit(&pix).contains(&(2, 9)) && lit(&pix).contains(&(20, 11)) && lit(&pix).len() == 3 * 19);
        // a rounded rect's corner is its circle's quadrant: 22 x 22, r 11, as pygame (368 pixels)
        let mut pix = Pix::default();
        pix.rect(
            PRect {
                x: 3.0,
                y: 4.0,
                w: 22.0,
                h: 22.0,
            },
            11.0,
        );
        assert_eq!(lit(&pix).len(), 368);
        // the polygon (the cursor's marker), as pygame fills (4,4) (16,4) (10,11)
        let mut pix = Pix::default();
        pix.polygon(&[(4.0, 4.0), (16.0, 4.0), (10.0, 11.0)]);
        let l = lit(&pix);
        assert_eq!(l.len(), 62);
        assert!(l.contains(&(4, 4)) && l.contains(&(16, 4)) && l.contains(&(10, 11)));
        assert!(!l.contains(&(9, 11)) && !l.contains(&(11, 11)) && l.contains(&(16, 5)));
    }

    #[test]
    fn inflate_like_pygame() {
        let r = PRect {
            x: 28.0,
            y: 143.0,
            w: 960.0,
            h: 633.0,
        };
        let i = r.inflate(-11.0, -10.0);
        assert_eq!((i.x, i.y, i.w, i.h), (33.0, 148.0, 949.0, 623.0));
        assert_eq!(r.centerx(), 508.0);
    }
}
