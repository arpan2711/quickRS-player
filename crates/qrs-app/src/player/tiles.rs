//! The tiles in the console (port of the tile half of rsplay/app.py `PlayerView`): Note & chord (with a piano or
//! the guitar neck, and a chord diagram), Lyrics, Score and Tuner, each a screen sunk into the console. They're
//! chosen and ordered under the controls' TILES heading or by dragging their top edge; each has a close key.
//! Every layout number is the Python app's, in its whole pixels (`qrs_ui::units`).

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};
use qrs_formats::Techs;
use qrs_formats::model::{BASS_OPEN_MIDI, GUITAR_OPEN_MIDI, HandShape};
use qrs_ui::anim;
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{self, CapOn, Gem, keycap};
use qrs_ui::knobs;
use qrs_ui::paint::{self, Fill};
use qrs_ui::raster::mix;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{STRING_PALETTE, string_color, string_colors_on, string_highlight, t};
use qrs_ui::units::{pixels, px, pxr, scale, to_px};
use qrs_ui::widgets::{self, RoundKind, interact};
use serde_json::{Value, json};

use super::{PlayerView, TileDrag};
use crate::state::AppState;

#[cfg(feature = "nux")]
pub const TILES: &[(&str, &str)] = &[
    ("note", "Note & chord"),
    ("lyrics", "Lyrics"),
    ("score", "Score"),
    ("tuner", "Tuner"),
    ("mg300", "MG-300"),
];
/// Without the NUX MG-300 support (the shared build): no MG-300 tile, so the pedal is never looked for.
#[cfg(not(feature = "nux"))]
pub const TILES: &[(&str, &str)] = &[
    ("note", "Note & chord"),
    ("lyrics", "Lyrics"),
    ("score", "Score"),
    ("tuner", "Tuner"),
];
const OFF_BY_DEFAULT: [&str; 1] = ["mg300"];
const TILE_GAP: f32 = 12.0;
/// The top right of every tile, kept free for its close key.
pub const CLOSE_SLOT: f32 = 40.0;
const STD_GUITAR: [&str; 6] = ["E", "A", "D", "G", "B", "e"];
const STD_BASS: [&str; 6] = ["E", "A", "D", "G", "B", "C"];
const TECH_TAGS: [(Techs, &str); 11] = [
    (Techs::HAMMER_ON, "H"),
    (Techs::PULL_OFF, "P"),
    (Techs::TAP, "T"),
    (Techs::PALM_MUTE, "PM"),
    (Techs::HARMONIC, "Harm"),
    (Techs::PINCH_HARMONIC, "PH"),
    (Techs::VIBRATO, "~"),
    (Techs::TREMOLO, "tr"),
    (Techs::SLAP, "S"),
    (Techs::POP, "Pop"),
    (Techs::ACCENT, ">"),
];
pub const TUNINGS: [(&str, Option<[i32; 6]>); 8] = [
    ("song", None),
    ("E Standard", Some([0; 6])),
    ("Eb Standard", Some([-1; 6])),
    ("D Standard", Some([-2; 6])),
    ("Drop D", Some([-2, 0, 0, 0, 0, 0])),
    ("Drop C#", Some([-3, -1, -1, -1, -1, -1])),
    ("Drop C", Some([-4, -2, -2, -2, -2, -2])),
    ("Open G", Some([-2, -2, 0, 0, 0, -2])),
];
const INLAY_FRETS: [(i32, i32); 10] = [
    (3, 1),
    (5, 1),
    (7, 1),
    (9, 1),
    (12, 2),
    (15, 1),
    (17, 1),
    (19, 1),
    (21, 1),
    (24, 2),
];

// -- the Python app's whole pixels ----------------------------------------------------------------------------------

/// The Python app's `int(v)` of a pixel position or length (`v` in points), as `units::snap`, except that a value
/// within f32 rounding of a whole pixel stays on it: a whole pixel kept as f32 points can come back up to ~1e-4 px
/// under it near x = 1000, more than snap's 1e-6 px allowance.
pub(super) fn whole(v: f32) -> f32 {
    if scale() <= 0.0 {
        return v;
    }
    let x = to_px(v);
    let r = x.round();
    pixels(if (x - r).abs() < 1e-3 { r } else { x.trunc() } as f32)
}

/// A whole-pixel length or position (points) as the Python app's int, in its pixels.
fn ipx(v: f32) -> f64 {
    to_px(v).round()
}

/// `a // 2` of a whole-pixel length `a` (points).
pub(super) fn half(a: f32) -> f32 {
    pixels((ipx(a) / 2.0).floor() as f32)
}

/// A pygame Rect's centre: `(r.x + r.w // 2, r.y + r.h // 2)`.
fn center(r: Rect) -> Pos2 {
    pos2(r.min.x + half(r.width()), r.min.y + half(r.height()))
}

/// A position as the Python app's whole pixel (`int` of each coordinate).
fn on_px(p: Pos2) -> Pos2 {
    pos2(whole(p.x), whole(p.y))
}

/// A text anchor as `blit_text` takes it (`int` of each coordinate), set in the middle of that pixel: text
/// placement truncates the anchor's pixels at egui's pixels_per_point, which can be a hair under the app's scale
/// (1.0285713673 against 1.0285714286 at 1440 x 900 on a 125% display), so a whole pixel would land one pixel off.
pub(super) fn at(p: Pos2) -> Pos2 {
    let mid = pixels(0.5);
    pos2(whole(p.x) + mid, whole(p.y) + mid)
}

/// `blit_text`: a line of text with its `anchor` point at `pos`.
fn txt(
    p: &Painter,
    s: &str,
    pos: Pos2,
    size: f32,
    color: Color32,
    weight: u16,
    anchor: Align2,
    max_w: Option<f32>,
) -> Rect {
    crate::library::txt(p, s, at(pos), size, color, weight, anchor, max_w)
}

/// `caps`: a small-caps title in the caption colour.
fn caps(p: &Painter, s: &str, pos: Pos2, size: f32, anchor: Align2, max_w: Option<f32>) -> Rect {
    text::caps(p, s, at(pos), size, t().caption, anchor, max_w)
}

/// `caps` in a colour.
fn caps_in(
    p: &Painter,
    s: &str,
    pos: Pos2,
    size: f32,
    color: Color32,
    anchor: Align2,
    max_w: Option<f32>,
) -> Rect {
    text::caps(p, s, at(pos), size, color, anchor, max_w)
}

/// The font's ascent (points) as pygame's `get_ascent`: ceil(1.066 em) at the font's whole-pixel size.
fn ascent(ctx: &egui::Context, size: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    let em = (text::quantize(size, ppp) as f64 * ppp as f64).round();
    pixels((em * 2132.0 / 2000.0 - 1e-9).ceil() as f32)
}

/// `pygame.draw.line` across, on whole pixels: the row at `y` from x0 to x1, both ends in, `w` pixels thick
/// (pygame grows it up first).
fn hline(p: &Painter, x0: f32, x1: f32, y: f32, w: f64, color: Color32) {
    let y0 = pixels((ipx(y) - ((w - 1.0) / 2.0).floor()) as f32);
    let r = Rect::from_min_max(pos2(x0, y0), pos2(x1 + pixels(1.0), y0 + pixels(w as f32)));
    p.rect_filled(r, 0.0, color);
}

/// `pygame.draw.line` upright, on whole pixels: the column at `x` from y0 to y1, both ends in, `w` pixels thick.
fn vline(p: &Painter, x: f32, y0: f32, y1: f32, w: f64, color: Color32) {
    let x0 = pixels((ipx(x) - ((w - 1.0) / 2.0).floor()) as f32);
    let r = Rect::from_min_max(pos2(x0, y0), pos2(x0 + pixels(w as f32), y1 + pixels(1.0)));
    p.rect_filled(r, 0.0, color);
}

/// A pygame line width (`max(1, int(n * s))` and the like) as a whole number of pixels.
fn line_px(n: f32, least: f64) -> f64 {
    ipx(px(n)).max(least)
}

fn static_key(k: &str) -> Option<&'static str> {
    TILES.iter().map(|t| t.0).find(|x| *x == k)
}

/// The tiles in order, with whether each is shown; tiles added in later versions go in their default place.
pub fn tile_order(st: &AppState) -> Vec<(&'static str, bool)> {
    let mut order: Vec<(&'static str, bool)> = Vec::new();
    if let Some(Value::Array(saved)) = st.config.get("tiles") {
        for item in saved {
            let (Some(k), Some(on)) = (
                item.get(0).and_then(Value::as_str),
                item.get(1).and_then(Value::as_bool),
            ) else {
                continue;
            };
            if let Some(k) = static_key(k)
                && !order.iter().any(|o| o.0 == k)
            {
                order.push((k, on));
            }
        }
    }
    let defaults: Vec<&'static str> = TILES.iter().map(|t| t.0).collect();
    for (i, k) in defaults.iter().enumerate() {
        if order.iter().any(|o| o.0 == *k) {
            continue;
        }
        let pos = defaults[..i]
            .iter()
            .rev()
            .find_map(|prev| order.iter().position(|o| o.0 == *prev).map(|p| p + 1))
            .unwrap_or(0);
        order.insert(pos, (k, !OFF_BY_DEFAULT.contains(k)));
    }
    order
}

pub fn tile_on(st: &AppState, key: &str) -> bool {
    tile_order(st).iter().any(|(k, on)| *k == key && *on)
}

pub fn set_tiles(st: &mut AppState, order: &[(&'static str, bool)]) {
    st.config.set(
        "tiles",
        Value::Array(order.iter().map(|(k, on)| json!([k, on])).collect()),
    );
}

pub fn tile_action(st: &mut AppState, name: &str) {
    let mut order = tile_order(st);
    let Some((verb, key)) = name.split_once(':') else {
        return;
    };
    let Some(i) = order.iter().position(|o| o.0 == key) else {
        return;
    };
    match verb {
        "tile" => order[i].1 = !order[i].1,
        "tileup" if i > 0 => order.swap(i - 1, i),
        "tiledown" if i + 1 < order.len() => order.swap(i, i + 1),
        _ => {}
    }
    set_tiles(st, &order);
}

pub fn tile_label(key: &str) -> &'static str {
    TILES.iter().find(|t| t.0 == key).map(|t| t.1).unwrap_or("")
}

/// What the fretting hand should be doing now, or next.
enum Target {
    Chord(usize, bool),
    Note(usize, bool),
}

impl PlayerView {
    fn play_target(&self, now: f64) -> Option<Target> {
        let c = &self.chart;
        let notes = &c.single_notes;
        let i = c.ntimes.partition_point(|&x| x <= now + 0.05) as i64 - 1;
        let note = if i >= 0 && now - notes[i as usize].time <= notes[i as usize].sustain.max(0.5) {
            Some((i as usize, true))
        } else if ((i + 1) as usize) < notes.len() {
            Some(((i + 1) as usize, false))
        } else {
            None
        };
        let k = c.shape_starts.partition_point(|&x| x <= now + 0.05) as i64 - 1;
        if k >= 0 && now <= c.shapes[k as usize].end + 0.15 {
            return Some(Target::Chord(k as usize, true)); // a held shape wins
        }
        let chord = if ((k + 1) as usize) < c.shapes.len() && c.shapes[(k + 1) as usize].start - now < 12.0 {
            Some(((k + 1) as usize, false))
        } else {
            None
        };
        if let Some((ni, true)) = note {
            return Some(Target::Note(ni, true));
        }
        if let Some((ci, _)) = chord
            && note.is_none_or(|(ni, _)| c.shapes[ci].start <= notes[ni].time)
        {
            return Some(Target::Chord(ci, false));
        }
        note.map(|(ni, on)| Target::Note(ni, on))
    }

    /// "A string" in standard tuning, "A string (Ab)" when it's tuned to Ab.
    fn string_label(&self, string: i32) -> String {
        let std = if self.chart.arr.is_bass {
            STD_BASS
        } else {
            STD_GUITAR
        };
        let name = std.get(string as usize).copied().unwrap_or("?");
        let tuned = self
            .chart
            .string_names
            .get(string as usize)
            .copied()
            .unwrap_or(name);
        if tuned.eq_ignore_ascii_case(name) {
            format!("{name} string")
        } else {
            format!("{name} string ({tuned})")
        }
    }

    /// (big text, detail, colour) for a single note.
    fn note_label(&self, n: &qrs_formats::Note) -> (String, String, Color32) {
        let col = string_color(n.string as usize);
        if n.techniques.has(Techs::FRET_HAND_MUTE) {
            return (
                "Mute".into(),
                format!("{}  ·  muted", self.string_label(n.string)),
                col,
            );
        }
        let midi = self
            .chart
            .opens
            .get(n.string as usize)
            .map(|o| o + n.fret)
            .unwrap_or(0);
        let tags: Vec<&str> = TECH_TAGS
            .iter()
            .filter(|(tc, _)| n.techniques.has(*tc))
            .map(|t| t.1)
            .collect();
        let mut detail = format!("{}  ·  fret {}", self.string_label(n.string), n.fret);
        if !tags.is_empty() {
            detail += &format!("  ·  {}", tags.join(" "));
        }
        (qrs_analysis::pitch::note_label(midi as f64), detail, col)
    }

    /// (name, MIDI note of each open string) the tuner tunes to; the song's own tuning by default.
    fn tuner_targets(&self) -> (String, Vec<i32>) {
        let (name, offs) = TUNINGS[self.tuning_choice % TUNINGS.len()];
        match offs {
            None => (
                format!("{} (this song)", self.chart.arr.tuning_name()),
                self.chart.opens.clone(),
            ),
            Some(o) => {
                let base = if self.chart.arr.is_bass {
                    BASS_OPEN_MIDI
                } else {
                    GUITAR_OPEN_MIDI
                };
                (
                    name.into(),
                    (0..self.chart.opens.len())
                        .map(|i| base[i.min(5)] + o[i.min(5)])
                        .collect(),
                )
            }
        }
    }

    /// C below the part's lowest note to B above its highest (at least two octaves).
    fn piano_range(&self) -> (i32, i32) {
        let ms: Vec<i32> = self
            .chart
            .glyphs
            .iter()
            .filter_map(|g| self.chart.opens.get(g.string as usize).map(|o| o + g.fret))
            .collect();
        let (lo_m, hi_m) = if ms.is_empty() {
            (48, 72)
        } else {
            (*ms.iter().min().unwrap(), *ms.iter().max().unwrap())
        };
        let lo = 12 * lo_m.div_euclid(12);
        let mut hi = 12 * (hi_m.div_euclid(12) + 1) - 1;
        if hi - lo < 23 {
            hi = lo + 23;
        }
        (lo, hi)
    }

    /// The tiles stacked in the console; they scroll inside it when they don't all fit, sliding out under its
    /// edges, with a thin groove in the console's edge showing where you are.
    pub(super) fn draw_tile_column(
        &mut self,
        ui: &mut Ui,
        st: &mut AppState,
        area: Rect,
        now: f64,
        aside: Rect,
    ) {
        let p = ui.painter().clone();
        let th = t();
        let clip = Rect::from_min_size(
            pos2(aside.min.x + px(4.0), area.min.y),
            vec2(aside.width() - px(8.0), area.height()),
        );
        // (the Python app keeps what's under the edges before drawing and lays it back over the tiles after; here
        // the edges are painted over them in the console's face)
        let most = self.panel_max;
        let (fade_top, fade_bottom) = (self.panel_scroll > 0.5, self.panel_scroll < most - 0.5);
        let mut y = area.min.y - whole(self.panel_scroll);
        let top_y = y;
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(clip));
        cui.set_clip_rect(clip);
        if !self.audio_error.is_empty() || self.engine().is_none() {
            y = self.draw_no_audio(&cui, area.min.x, y, area.width()) + px(TILE_GAP);
        }
        y = self.draw_tiles(&cui, st, area.min.x, y, area.width(), now, area);
        let content = y - top_y - px(TILE_GAP);
        self.panel_max = pixels((ipx(content) - ipx(area.height())).max(0.0) as f32);
        self.panel_scroll = self.panel_scroll.clamp(0.0, self.panel_max);
        if fade_top || fade_bottom {
            paint::fade_edges(
                &p,
                clip,
                fade_top,
                fade_bottom,
                24.0,
                paint::FaceKind::Base {
                    rect: aside,
                    sunk: false,
                },
            );
        }
        if self.panel_max > 0.0 {
            let track = Rect::from_min_size(
                pos2(aside.max.x - px(10.0), area.min.y + px(8.0)),
                vec2(px(4.0), area.height() - px(16.0)),
            );
            let thh = px(30.0).max(pixels(
                (ipx(track.height()) * ipx(area.height()) / ipx(content).max(1.0)).trunc() as f32,
            ));
            let ty = track.min.y + (track.height() - thh) * self.panel_scroll / self.panel_max;
            paint::inset(&p, track, None, 2.0, Fill::Base);
            paint::pill(
                &p,
                Rect::from_min_size(pos2(track.min.x, whole(ty)), vec2(track.width(), thh)),
                th.tick,
            );
        }
    }

    /// Draw the visible tiles stacked from y; each glides to its slot, the dragged one follows the mouse. Returns
    /// the y below the last tile.
    fn draw_tiles(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        x: f32,
        y: f32,
        w: f32,
        now: f64,
        panel: Rect,
    ) -> f32 {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let th = t();
        let order: Vec<&'static str> = tile_order(st)
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(k, _)| k)
            .collect();
        let gap = px(TILE_GAP);
        let mut slots = std::collections::HashMap::new();
        let mut cursor = y;
        for k in &order {
            slots.insert(*k, cursor);
            cursor += self.tile_h.get(k).copied().unwrap_or(px(120.0)) + gap;
        }
        self.tile_rects.clear();
        self.tiles_top = y;
        let drag_key = self.tile_drag.as_ref().map(|d| d.key);
        // (in the order the Python app draws them, so the close keys over them come in its order too)
        let mut drawn: Vec<(&'static str, Rect)> = Vec::new();
        for k in &order {
            if drag_key == Some(*k) {
                continue;
            }
            // glide, relative to the panel's scroll
            let rel = anim::smooth(&ctx, ("tile", *k), slots[k] - y, 0.05, None);
            let ty = whole(y + rel);
            let bottom = self.draw_tile(ui, st, k, x, ty, w, now, false);
            self.tile_h.insert(k, bottom - ty);
            drawn.push((k, Rect::from_min_size(pos2(x, ty), vec2(w, bottom - ty))));
        }
        if let Some(d) = self.tile_drag.clone()
            && order.contains(&d.key)
        {
            let ty = whole(d.y);
            let bottom = self.draw_tile(ui, st, d.key, x, ty, w, now, true);
            self.tile_h.insert(d.key, bottom - ty);
            drawn.push((d.key, Rect::from_min_size(pos2(x, ty), vec2(w, bottom - ty))));
            ctx.set_cursor_icon(CursorIcon::Move);
        }
        self.tile_rects.extend(drawn.iter().copied());
        // the grab bar and close key on the tile under the mouse; dragging a tile's top edge reorders
        let mouse = ctx.input(|i| i.pointer.hover_pos());
        for (k, r) in drawn {
            let grab = Self::tile_grab_rect(r);
            let gresp = interact(ui, grab.intersect(panel), ("tile-grab", k), Sense::drag());
            if gresp.drag_started()
                && let Some(pos) = gresp.interact_pointer_pos()
            {
                self.tile_drag = Some(TileDrag {
                    key: k,
                    grab: pos.y - r.min.y,
                    y: r.min.y,
                });
            }
            if let Some(d) = &mut self.tile_drag
                && d.key == k
            {
                if let Some(pos) = gresp.interact_pointer_pos()
                    && gresp.dragged()
                {
                    d.y = pos.y - d.grab;
                    let centre = d.y + self.tile_h.get(k).copied().unwrap_or(0.0) / 2.0;
                    let all = tile_order(st);
                    let others: Vec<&'static str> = all
                        .iter()
                        .filter(|(kk, on)| *on && *kk != k)
                        .map(|o| o.0)
                        .collect();
                    let idx = others
                        .iter()
                        .filter(|o| self.tile_rects.get(*o).is_some_and(|rr| rr.center().y < centre))
                        .count();
                    let mut new_on = others.clone();
                    new_on.insert(idx, k);
                    let cur_on: Vec<&'static str> = all.iter().filter(|o| o.1).map(|o| o.0).collect();
                    if new_on != cur_on {
                        let off: Vec<(&'static str, bool)> = all
                            .iter()
                            .filter(|(kk, on)| !*on && *kk != k)
                            .map(|o| (o.0, false))
                            .collect();
                        let mut order: Vec<(&'static str, bool)> =
                            new_on.iter().map(|kk| (*kk, true)).collect();
                        order.extend(off);
                        set_tiles(st, &order);
                    }
                }
                if gresp.drag_stopped() {
                    let dy = d.y - self.tiles_top;
                    anim::forget(("tile", k));
                    let _ = anim::smooth(&ctx, ("tile", k), dy, 0.05, Some(dy)); // settle from where it was dropped
                    self.tile_drag = None;
                }
            }
            let hovering = mouse.is_some_and(|m| r.contains(m) && panel.contains(m));
            if !(hovering || drag_key == Some(k)) {
                continue;
            }
            let bw = px(36.0);
            let bar = Rect::from_min_size(
                pos2(r.min.x + half(r.width()) - half(bw), r.min.y + px(5.0)),
                vec2(bw, px(5.0)),
            );
            paint::pill(&p, bar, th.tick);
            let cx = Self::tile_close_rect(r);
            if widgets::round_button(
                ui,
                cx,
                ("tile-close", k),
                Icon::XBold,
                13.0,
                RoundKind::Gem(Gem::Ruby),
                false,
            )
            .clicked()
            {
                let order: Vec<(&'static str, bool)> = tile_order(st)
                    .into_iter()
                    .map(|(kk, on)| (kk, on && kk != k))
                    .collect();
                set_tiles(st, &order);
                self.say(
                    format!("{} tile hidden", tile_label(k)),
                    "  ·  bring it back under TILES",
                );
            }
            if mouse.is_some_and(|m| grab.contains(m) && !cx.contains(m)) && self.tile_drag.is_none() {
                ctx.set_cursor_icon(CursorIcon::Move);
            }
        }
        if self.tile_drag.as_ref().is_some_and(|d| !tile_on(st, d.key)) || self.show_controls {
            self.tile_drag = None;
        }
        cursor
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tile(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        key: &str,
        x: f32,
        y: f32,
        w: f32,
        now: f64,
        lifting: bool,
    ) -> f32 {
        match key {
            "note" => self.draw_note_tile(ui, st, x, y, w, now, lifting),
            "lyrics" => self.draw_lyrics_tile(ui, x, y, w, now, lifting),
            "score" => self.draw_score_tile(ui, st, x, y, w, lifting),
            "tuner" => self.draw_tuner_tile(ui, x, y, w, lifting),
            #[cfg(feature = "nux")]
            _ => self.draw_mg300_tile(ui, st, x, y, w, lifting),
            #[cfg(not(feature = "nux"))]
            _ => y,
        }
    }

    /// A tile's close key (a ruby), in the top right corner kept free for it.
    fn tile_close_rect(r: Rect) -> Rect {
        let d = px(26.0);
        Rect::from_min_size(
            pos2(r.max.x - px(19.0) - half(d), r.min.y + px(24.0) - half(d)),
            vec2(d, d),
        )
    }

    /// A tile's top edge, where it's picked up to drag.
    fn tile_grab_rect(r: Rect) -> Rect {
        Rect::from_min_size(r.min, vec2(r.width() - px(40.0), px(16.0)))
    }

    /// A tile: a screen sunk into the console (on a raised plate while it's being dragged).
    fn tile_card(&self, p: &egui::Painter, card: Rect, lifting: bool) {
        if lifting {
            let d = px(10.0);
            let plate = Rect::from_min_size(card.min - Vec2::splat(half(d)), card.size() + Vec2::splat(d));
            paint::raised(p, plate, Some(22.0), 8.0, Fill::Base, 0.0);
        }
        paint::display(p, card, Some(18.0), 6.0);
    }

    /// Two or three small-caps options in a sunken track, the chosen one on a small ivory key.
    fn mini_switch(&self, p: &egui::Painter, rect: Rect, labels: &[&str], sel: usize) {
        let ctx = p.ctx();
        let th = t();
        paint::inset(p, rect, Some(10.0), 2.0, Fill::Base);
        let inner = rect.shrink(px(3.0));
        // (the Python app's pixels, as floats where it has them)
        let widths: Vec<f64> = labels
            .iter()
            .map(|l| ipx(text::width(ctx, l, Style::caps(10.0, th.caption))) + ipx(px(16.0)))
            .collect();
        let k = ipx(inner.width()) / widths.iter().sum::<f64>().max(1.0);
        let mut x = ipx(inner.min.x);
        for (i, (l, wd)) in labels.iter().zip(&widths).enumerate() {
            let r = Rect::from_min_size(
                pos2(pixels(x.trunc() as f32), inner.min.y),
                vec2(pixels((wd * k).trunc() as f32), inner.height()),
            );
            if i == sel {
                keys::key_face(p, r, Some(7.0), false, false);
            }
            caps_in(
                p,
                l,
                center(r),
                10.0,
                if i == sel {
                    qrs_ui::theme::IVORY_INK
                } else {
                    th.caption
                },
                Align2::CENTER_CENTER,
                None,
            );
            x += wd * k;
        }
    }

    /// Text placed by its baseline (so different sizes line up): its top at the baseline less the font's ascent.
    fn text_base(
        &self,
        p: &egui::Painter,
        s: &str,
        x: f32,
        base: f32,
        size: f32,
        color: Color32,
        weight: u16,
        anchor: Align2,
        max_w: Option<f32>,
    ) -> Rect {
        let top = base - ascent(p.ctx(), size);
        let a = match anchor.x() {
            egui::Align::Min => Align2::LEFT_TOP,
            egui::Align::Max => Align2::RIGHT_TOP,
            egui::Align::Center => Align2::CENTER_TOP,
        };
        text::draw(p, s, at(pos2(x, top)), a, Style::new(size, weight, color), max_w)
    }

    /// The note or chord shape to play now (or next), whichever comes first, what follows it and what you're
    /// playing; underneath, where it is on a piano or on the guitar neck (click to switch).
    #[allow(clippy::too_many_arguments)]
    fn draw_note_tile(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        x: f32,
        y: f32,
        w: f32,
        now: f64,
        lifting: bool,
    ) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let piano = st.config.str("note_view", "piano") != "guitar";
        let (pad_x, pad_y) = (px(16.0), px(12.0));
        let inst_h = if piano { px(92.0) } else { px(104.0) };
        let main_h = px(60.0);
        let card = Rect::from_min_size(
            pos2(x, y),
            vec2(w, pad_y + px(24.0) + px(8.0) + main_h + px(8.0) + inst_h + pad_y),
        );
        self.tile_card(&p, card, lifting);
        let (lx, tw) = (card.min.x + pad_x, card.width() - 2.0 * pad_x);
        let hy = card.min.y + pad_y + px(12.0); // the header row's middle
        let sw = ["PIANO", "NECK"]
            .iter()
            .map(|l| text::width(&ctx, l, Style::caps(10.0, th.caption)) + px(16.0))
            .sum::<f32>()
            + px(6.0);
        let mode = Rect::from_min_size(
            pos2(card.max.x - px(CLOSE_SLOT) - sw, hy - px(12.0)),
            vec2(sw, px(24.0)),
        );
        self.mini_switch(&p, mode, &["PIANO", "NECK"], if piano { 0 } else { 1 });
        let main_y = card.min.y + pad_y + px(24.0) + px(8.0);
        let mid = main_y + half(main_h);
        let strip = Rect::from_min_size(pos2(lx, main_y + main_h + px(8.0)), vec2(tw, inst_h));
        // a click on the switch or the instrument swaps them; the hand shows over the switch
        let inst = interact(ui, strip.union(mode), "note-inst", Sense::click());
        if inst.hover_pos().is_some_and(|m| mode.contains(m)) {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if inst.clicked() {
            self.acts.push("noteview".into());
        }
        let title_w = mode.min.x - lx - px(12.0);
        let mut spots: Vec<(i32, i32)> = Vec::new();
        let mut muted: Vec<i32> = Vec::new();
        let mut col = th.accent;
        let mut now_on = false;
        let has_input = self.has_input();
        let you = if !self.tuner_text.is_empty() {
            self.tuner_text.clone()
        } else if has_input {
            "–".into()
        } else {
            "no input".into()
        };
        let you_col = if self.tuner_text.is_empty() {
            th.caption
        } else {
            th.text
        };
        match self.play_target(now) {
            None => {
                caps(&p, "NOTE", pos2(lx, hy), 12.0, Align2::LEFT_CENTER, None);
                txt(
                    &p,
                    "nothing left to play",
                    pos2(lx, mid),
                    14.0,
                    th.caption,
                    500,
                    Align2::LEFT_CENTER,
                    None,
                );
            }
            Some(Target::Chord(k, on)) => {
                now_on = on;
                let sh: HandShape = self.chart.shapes[k].clone();
                let new = self.chart.shape_new[k];
                let when = if on {
                    "NOW".to_string()
                } else {
                    format!("NEXT IN {:.1}s", (sh.start - now).max(0.0))
                };
                caps_in(
                    &p,
                    &format!("{}{when}", if new { "NEW SHAPE  ·  " } else { "CHORD  ·  " }),
                    pos2(lx, hy),
                    12.0,
                    if new { th.accent_ink } else { th.caption },
                    Align2::LEFT_CENTER,
                    Some(title_w),
                );
                if new {
                    paint::ring(&p, card, Some(18.0), 2.0, th.accent, false);
                }
                let bx = Rect::from_min_size(
                    pos2(card.max.x - pad_x - px(64.0), main_y),
                    vec2(px(64.0), main_h),
                );
                draw_chord_box(&p, bx, &sh, self.chart.n_strings);
                let name = if sh.name.is_empty() { "Chord" } else { &sh.name };
                let nr = self.text_base(
                    &p,
                    name,
                    lx,
                    mid + px(12.0),
                    34.0,
                    th.accent_ink,
                    800,
                    Align2::LEFT_TOP,
                    Some(pixels(((ipx(bx.min.x) - ipx(lx)) * 0.5).trunc() as f32)),
                );
                let dx = nr.max.x + px(12.0);
                let dw = bx.min.x - px(10.0) - dx;
                let shape: Vec<String> = sh
                    .frets
                    .iter()
                    .take(self.chart.n_strings)
                    .map(|f| if *f < 0 { "x".into() } else { f.to_string() })
                    .collect();
                let picked = self.chart.shape_picked[k];
                let mut how = if picked {
                    "hold it, pick through".to_string()
                } else {
                    "strum".to_string()
                };
                if let Some(next) = self.chart.shapes.get(k + 1) {
                    how += &format!(
                        "  ·  then {}",
                        if next.name.is_empty() { "chord" } else { &next.name }
                    );
                }
                txt(
                    &p,
                    &shape.join(" "),
                    pos2(dx, mid - px(17.0)),
                    14.0,
                    th.text_2,
                    700,
                    Align2::LEFT_CENTER,
                    Some(dw),
                );
                txt(
                    &p,
                    &how,
                    pos2(dx, mid + px(1.0)),
                    12.0,
                    if picked { th.text } else { th.caption },
                    if picked { 600 } else { 500 },
                    Align2::LEFT_CENTER,
                    Some(dw),
                );
                txt(
                    &p,
                    &format!("you: {you}"),
                    pos2(dx, mid + px(18.0)),
                    12.0,
                    you_col,
                    700,
                    Align2::LEFT_CENTER,
                    Some(dw),
                );
                for (s_, f) in sh.frets.iter().take(self.chart.n_strings).enumerate() {
                    if *f >= 0 {
                        spots.push((s_ as i32, *f));
                    } else {
                        muted.push(s_ as i32);
                    }
                }
            }
            Some(Target::Note(i, on)) => {
                now_on = on;
                let ev = self.chart.single_notes[i].clone();
                let after = self.chart.single_notes.get(i + 1).cloned();
                let when = if on {
                    "NOW".to_string()
                } else {
                    format!("NEXT IN {:.1}s", (ev.time - now).max(0.0))
                };
                caps(
                    &p,
                    &format!("NOTE  ·  {when}"),
                    pos2(lx, hy),
                    12.0,
                    Align2::LEFT_CENTER,
                    Some(title_w),
                );
                // what you're playing, in a small sunken chip at the right
                let yw = text::width(&ctx, "you", Style::new(12.0, 700, th.caption))
                    + px(6.0)
                    + text::width(&ctx, &you, Style::new(12.0, 800, th.text));
                let (chip_w, chip_h) = (yw + px(18.0), px(30.0));
                let chip = Rect::from_min_size(
                    pos2(card.max.x - pad_x - chip_w, mid - half(chip_h)),
                    vec2(chip_w, chip_h),
                );
                paint::inset(&p, chip, Some(9.0), 2.0, Fill::Base);
                let yr = txt(
                    &p,
                    "you",
                    pos2(chip.min.x + px(9.0), mid),
                    12.0,
                    th.caption,
                    700,
                    Align2::LEFT_CENTER,
                    None,
                );
                txt(
                    &p,
                    &you,
                    pos2(yr.max.x + px(6.0), mid),
                    12.0,
                    you_col,
                    800,
                    Align2::LEFT_CENTER,
                    None,
                );
                let (big, detail, scol) = self.note_label(&ev);
                let nr = self.text_base(
                    &p,
                    &big,
                    lx,
                    mid + px(14.0),
                    40.0,
                    if string_colors_on() { scol } else { th.text },
                    800,
                    Align2::LEFT_TOP,
                    None,
                );
                let dx = nr.max.x + px(12.0);
                let dw = chip.min.x - px(10.0) - dx;
                txt(
                    &p,
                    &detail,
                    pos2(dx, mid - px(9.0)),
                    14.0,
                    th.text_2,
                    700,
                    Align2::LEFT_CENTER,
                    Some(dw),
                );
                if let Some(a) = after {
                    let (ab, ad, _) = self.note_label(&a);
                    let first = ad.split("  ").next().unwrap_or("").to_string();
                    txt(
                        &p,
                        &format!("then {ab}  ·  {first}"),
                        pos2(dx, mid + px(11.0)),
                        12.0,
                        th.caption,
                        500,
                        Align2::LEFT_CENTER,
                        Some(dw),
                    );
                }
                col = string_highlight(ev.string as usize);
                if ev.techniques.has(Techs::FRET_HAND_MUTE) {
                    now_on = false;
                } else {
                    spots.push((ev.string, ev.fret));
                }
            }
        }
        if piano {
            let midis: Vec<i32> = spots
                .iter()
                .filter_map(|(s_, f)| self.chart.opens.get(*s_ as usize).map(|o| o + f))
                .collect();
            self.draw_piano(&p, strip, &midis, now_on, col);
        } else {
            self.draw_neck(&p, strip, &spots, now_on, col, &muted);
        }
        card.max.y
    }

    /// Every note the part can reach, C to B, in a sunken box: the keys to play light up with a marker above them
    /// (outlined while they're still coming), and a dot marks the pitch you're playing.
    fn draw_piano(&self, p: &egui::Painter, rect: Rect, midis: &[i32], now_on: bool, col: Color32) {
        let th = t();
        paint::inset(p, rect, Some(12.0), 2.0, Fill::Base);
        let keys_r = Rect::from_min_size(
            pos2(rect.min.x + px(6.0), rect.min.y + px(12.0)),
            vec2(rect.width() - px(12.0), rect.height() - px(18.0)),
        );
        let (lo, hi) = self.piano_range();
        let white = |m: i32| matches!(m.rem_euclid(12), 0 | 2 | 4 | 5 | 7 | 9 | 11);
        let whites: Vec<i32> = (lo..=hi).filter(|m| white(*m)).collect();
        // the keys' left edge and a white key's width in the Python app's pixels (a float, as it has it)
        let kx = ipx(keys_r.min.x);
        let ww = ipx(keys_r.width()) / whites.len().max(1) as f64;
        let rad = px(3.0).max(pixels(1.0));
        let line = px(2.0).max(pixels(1.0));
        let shade_h = px(3.0).max(pixels(2.0));
        let shade = mix(th.key_white, Color32::BLACK, 0.12);
        let label_col = mix(th.key_white, th.key_black, 0.55);
        let mut edge: std::collections::HashMap<i32, f64> = std::collections::HashMap::new();
        let mut key_x: std::collections::HashMap<i32, f32> = std::collections::HashMap::new();
        let r8 = rad.round() as u8;
        let bottom_round = egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: r8,
            se: r8,
        };
        for (i, m) in whites.iter().enumerate() {
            let (xa, xb) = ((kx + i as f64 * ww).trunc(), (kx + (i + 1) as f64 * ww).trunc());
            let r = Rect::from_min_size(
                pos2(pixels((xa + 1.0) as f32), keys_r.min.y),
                vec2(pixels((xb - xa - 1.0).max(2.0) as f32), keys_r.height()),
            );
            key_x.insert(*m, center(r).x);
            edge.insert(*m, kx + (i + 1) as f64 * ww);
            let lit = midis.contains(m);
            if lit && now_on {
                paint::glow(p, r, col, 6.0, None, None);
            }
            p.rect_filled(r, bottom_round, if lit && now_on { col } else { th.key_white });
            if !(lit && now_on) {
                p.rect_filled(
                    Rect::from_min_max(pos2(r.min.x, r.max.y - shade_h), r.max),
                    bottom_round,
                    shade,
                );
            }
            if lit && !now_on {
                p.rect_stroke(r, rad, Stroke::new(line, col), egui::StrokeKind::Inside);
            }
            if m.rem_euclid(12) == 0 && ww >= to_px(9.0) {
                txt(
                    p,
                    &format!("C{}", m.div_euclid(12) - 1),
                    pos2(center(r).x, r.max.y - px(4.0)),
                    7.0,
                    label_col,
                    800,
                    Align2::CENTER_BOTTOM,
                    None,
                );
            }
        }
        let (bw, bh) = (ww * 0.62, ipx(keys_r.height()) * 0.6);
        for m in lo..=hi {
            if white(m) || !edge.contains_key(&(m - 1)) {
                continue;
            }
            // centred on the edge after the white key below it, at a whole pixel
            let cx = edge[&(m - 1)].trunc();
            let r = Rect::from_min_size(
                pos2(pixels((cx - (bw.trunc() / 2.0).floor()) as f32), keys_r.min.y),
                vec2(pixels(bw.trunc() as f32), pixels(bh.trunc() as f32)),
            );
            key_x.insert(m, pixels(cx as f32));
            let lit = midis.contains(&m);
            if lit && now_on {
                paint::glow(p, r, col, 6.0, None, None);
            } else {
                p.rect_filled(
                    r.translate(vec2(0.0, px(2.0).max(pixels(1.0)))),
                    bottom_round,
                    mix(th.key_black, Color32::BLACK, 0.5),
                );
            }
            p.rect_filled(r, bottom_round, if lit && now_on { col } else { th.key_black });
            if lit && !now_on {
                p.rect_stroke(r, rad, Stroke::new(line, col), egui::StrokeKind::Inside);
            }
        }
        if now_on {
            // a marker above each key to play: pygame fills (cx - 5s, y - 9s), (cx + 5s, y - 9s), (cx, y - 2s) on
            // whole pixels, its edge pixels in
            let (y0, y1) = (whole(keys_r.min.y - 9.0), whole(keys_r.min.y - 2.0));
            for m in midis {
                if let Some(&cx) = key_x.get(m) {
                    p.add(Shape::convex_polygon(
                        vec![
                            pos2(whole(cx - 5.0), y0),
                            pos2(whole(cx + 5.0) + pixels(1.0), y0),
                            pos2(cx + pixels(0.5), y1 + pixels(1.0)),
                        ],
                        col,
                        Stroke::NONE,
                    ));
                }
            }
        }
        if let Some(tm) = self.tuner_midi {
            let m = tm.round_ties_even() as i32;
            if let Some(&cx) = key_x.get(&m) {
                let black = !white(m);
                let cy = keys_r.min.y
                    + if black {
                        pixels(bh as f32) - 7.0
                    } else {
                        keys_r.height() - 14.0
                    };
                paint::dot(
                    p,
                    pos2(cx, cy),
                    if black { 3.2 } else { 3.5 },
                    if black { th.key_white } else { th.key_black },
                );
            }
        }
    }

    /// The whole neck, nut to fret 24, low E at the bottom like the tab, on a dark sunken board: each (string, fret)
    /// to play lights up (a ring while it's still coming), muted strings get an x at the nut.
    fn draw_neck(
        &self,
        p: &egui::Painter,
        rect: Rect,
        spots: &[(i32, i32)],
        now_on: bool,
        col: Color32,
        muted: &[i32],
    ) {
        let th = t();
        let n = self.chart.n_strings.max(1) as i32;
        let num_h = px(16.0);
        let board = Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() - num_h));
        paint::inset(p, board, Some(10.0), 3.0, Fill::Color(th.neck));
        let nut_w = pixels((ipx(board.width()) * 0.034).trunc().max(6.0) as f32);
        let fw = (board.width() - nut_w) / 24.0;
        let x0 = board.min.x + nut_w;
        let m_ = px(8.0);
        let gap = (board.height() - 2.0 * m_) / (n - 1).max(1) as f32;
        let sy = |s_: i32| board.min.y + m_ + (n - 1 - s_) as f32 * gap;
        let fx = |f: i32| {
            if f == 0 {
                board.min.x + nut_w / 2.0
            } else {
                x0 + (f as f32 - 0.5) * fw
            }
        };
        let mid = board.min.y + half(board.height());
        for (f, dots) in INLAY_FRETS {
            for j in 0..dots {
                let cy = mid + (j as f32 - (dots - 1) as f32 / 2.0) * gap * 2.0;
                paint::dot(p, pos2(fx(f), cy), 3.0, th.inlay);
            }
        }
        let lw = line_px(1.0, 1.0);
        for k in 1..=24 {
            let xx = whole(x0 + k as f32 * fw);
            vline(p, xx, board.min.y + px(4.0), board.max.y - px(5.0), lw, th.fret);
        }
        p.rect_filled(
            Rect::from_min_size(
                pos2(board.min.x + px(2.0), board.min.y + px(4.0)),
                vec2(nut_w - px(2.0), board.height() - px(8.0)),
            ),
            px(3.0).max(pixels(1.0)),
            th.nut,
        );
        let widths = [2.4, 2.1, 1.8, 1.4, 1.2, 1.0];
        for s_ in 0..n {
            let thk = pxr(widths[(s_ as usize).min(5)]).max(pixels(1.0));
            let c = if string_colors_on() {
                mix(STRING_PALETTE[s_ as usize % 6], th.neck, 0.2)
            } else if s_ < 3 || self.chart.arr.is_bass {
                th.wound
            } else {
                th.plain
            };
            p.rect_filled(
                Rect::from_min_size(
                    pos2(board.min.x + nut_w, whole(sy(s_) - thk / 2.0)),
                    vec2(board.width() - nut_w - px(3.0), thk),
                ),
                0.0,
                c,
            );
        }
        for s_ in muted {
            if *s_ < n {
                txt(
                    p,
                    "×",
                    pos2(fx(0), sy(*s_)),
                    11.0,
                    th.base,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
        }
        for f in [3, 5, 7, 9, 12, 15, 17, 19, 21, 24] {
            txt(
                p,
                &f.to_string(),
                pos2(fx(f), board.max.y + px(2.0)),
                9.0,
                th.caption,
                700,
                Align2::CENTER_TOP,
                None,
            );
        }
        let r = 6.0;
        for &(s_, f) in spots {
            if s_ >= n || f > 24 {
                continue;
            }
            let c = if spots.len() == 1 {
                string_highlight(s_ as usize)
            } else {
                col
            };
            let (cx, cy) = (fx(f), sy(s_));
            if now_on {
                paint::halo(p, pos2(cx, cy), 11.0, c, 72);
                paint::dot(p, pos2(cx, cy), r + 1.5, Color32::WHITE);
                paint::dot(p, pos2(cx, cy), r, c);
            } else {
                // pygame.draw.circle: a ring max(2, int(2 s)) px thick inside radius int(r + s), round the whole
                // pixel (int(cx), int(cy))
                let (ro, wd) = (px(7.0), pixels(line_px(2.0, 2.0) as f32));
                p.circle_stroke(pos2(whole(cx), whole(cy)), ro - wd / 2.0, Stroke::new(wd, c));
            }
        }
    }

    /// Fixed for the chosen options, so the tiles below don't jump as lines change length.
    fn lyrics_tile_height(&self) -> f32 {
        px(12.0 + 16.0 + 8.0 + 2.0 * 24.0 + 12.0 + if self.lyrics_next { 4.0 + 20.0 } else { 0.0 })
    }

    /// The line being sung (up to two rows; sung syllables lit, the one being sung in the accent) and the next line.
    fn draw_lyrics_tile(&mut self, ui: &Ui, x: f32, y: f32, w: f32, now: f64, lifting: bool) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let card = Rect::from_min_size(pos2(x, y), vec2(w, self.lyrics_tile_height()));
        self.tile_card(&p, card, lifting);
        let (pad_x, pad_y) = (px(16.0), px(12.0));
        let tw = card.width() - 2.0 * pad_x;
        caps(
            &p,
            "LYRICS",
            pos2(card.min.x + pad_x, card.min.y + pad_y),
            12.0,
            Align2::LEFT_TOP,
            None,
        );
        let (line_h, size) = (px(24.0), 18.0);
        let top = card.min.y + pad_y + px(24.0);
        let block_h = 2.0 * line_h;
        let cx = card.min.x + half(card.width());
        if self.lyrics.is_empty() {
            txt(
                &p,
                "No lyrics in this song",
                pos2(cx, top + half(block_h)),
                14.0,
                th.caption,
                700,
                Align2::CENTER_CENTER,
                None,
            );
            return card.max.y;
        }
        // a line shows just before it starts
        let i = self.lyric_starts.partition_point(|&s| s <= now + 0.4) as i64 - 1;
        let mut line = if i >= 0 {
            Some(&self.lyrics[i as usize])
        } else {
            None
        };
        let nxt = self.lyrics.get((i + 1) as usize);
        if let Some(l) = line
            && now > l.end() + 2.0
            && nxt.is_none_or(|n| n.start() - now > 2.0)
        {
            line = None; // a long instrumental gap, or the last line long gone
        }
        let main = line.or(nxt.filter(|n| n.start() - now < 8.0));
        match main {
            None => {
                // intro, a long instrumental gap, or after the last line
                icons::icon(&p, Icon::Note, pos2(cx, top + half(block_h)), 24.0, th.caption);
            }
            Some(m) => {
                let st = Style::new(size, 800, th.text);
                // syllables grouped into words, so lines wrap between words
                let mut words: Vec<Vec<&qrs_formats::lyrics::Syllable>> = Vec::new();
                let mut cur: Vec<&qrs_formats::lyrics::Syllable> = Vec::new();
                for syl in &m.syllables {
                    cur.push(syl);
                    if syl.text.ends_with(' ') {
                        words.push(std::mem::take(&mut cur));
                    }
                }
                if !cur.is_empty() {
                    words.push(cur);
                }
                // rows of (syllable, width in the Python app's pixels), filled word by word up to the tile's width
                let tw_px = ipx(tw);
                let mut rows: Vec<Vec<(&qrs_formats::lyrics::Syllable, f64)>> = Vec::new();
                let mut row: Vec<(&qrs_formats::lyrics::Syllable, f64)> = Vec::new();
                let mut row_w = 0.0;
                for word in words {
                    let parts: Vec<(&qrs_formats::lyrics::Syllable, f64)> = word
                        .iter()
                        .map(|s| (*s, ipx(text::width(&ctx, &s.text, st))))
                        .collect();
                    let total: f64 = parts.iter().map(|p| p.1).sum();
                    let pieces: Vec<Vec<(&qrs_formats::lyrics::Syllable, f64)>> = if total <= tw_px {
                        vec![parts]
                    } else {
                        parts.into_iter().map(|p| vec![p]).collect() // too wide: by syllable
                    };
                    for piece in pieces {
                        let pw: f64 = piece.iter().map(|p| p.1).sum();
                        if !row.is_empty() && row_w + pw > tw_px {
                            rows.push(std::mem::take(&mut row));
                            row_w = 0.0;
                        }
                        row.extend(piece);
                        row_w += pw;
                    }
                }
                if !row.is_empty() {
                    rows.push(row);
                }
                if rows.len() > 2 {
                    // a long line: keep the row being sung in view
                    let cur_r = rows
                        .iter()
                        .position(|r| r.iter().any(|(s, _)| s.end > now))
                        .unwrap_or(rows.len() - 1);
                    let start = cur_r.min(rows.len() - 2);
                    rows = rows[start..start + 2].to_vec();
                }
                let mut yy = top + half((2 - rows.len()) as f32 * line_h);
                for r in &rows {
                    let mut xx = ipx(cx) - r.iter().map(|p| p.1).sum::<f64>() / 2.0;
                    for (syl, wd) in r {
                        let col = if line.is_none() {
                            th.caption // what's coming after a gap: all unsung
                        } else if syl.time <= now && now < syl.end {
                            th.accent_ink
                        } else if syl.time <= now {
                            th.text
                        } else {
                            th.caption
                        };
                        txt(
                            &p,
                            &syl.text,
                            pos2(pixels(xx.trunc() as f32), yy),
                            size,
                            col,
                            800,
                            Align2::LEFT_TOP,
                            Some(tw),
                        );
                        xx += wd;
                    }
                    yy += line_h;
                }
            }
        }
        if self.lyrics_next
            && line.is_some()
            && let Some(n) = nxt
        {
            let s: String = n
                .syllables
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
                .trim()
                .to_string();
            txt(
                &p,
                &s,
                pos2(cx, top + block_h + px(4.0)),
                14.0,
                th.caption,
                600,
                Align2::CENTER_TOP,
                Some(tw),
            );
        }
        card.max.y
    }

    /// Tuner: the target tuning's strings, a dial with a needle in cents to the nearest string, and the note you're
    /// playing. Click the tuning's name for the next tuning.
    fn draw_tuner_tile(&mut self, ui: &Ui, x: f32, y: f32, w: f32, lifting: bool) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let (pad_x, pad_y) = (px(16.0), px(12.0));
        let gauge_h = px(100.0);
        let card = Rect::from_min_size(
            pos2(x, y),
            vec2(
                w,
                pad_y + px(18.0) + px(10.0) + px(28.0) + px(6.0) + gauge_h + px(4.0) + px(30.0) + pad_y,
            ),
        );
        self.tile_card(&p, card, lifting);
        let (lx, tw) = (card.min.x + pad_x, card.width() - 2.0 * pad_x);
        let (name, targets) = self.tuner_targets();
        let hy = card.min.y + pad_y + px(9.0);
        let tt = caps(&p, "TUNER", pos2(lx, hy), 12.0, Align2::LEFT_CENTER, None);
        // the tuning's name and an arrow after it: their box, grown by int(12 s), takes the click
        let ic_c = on_px(pos2(card.max.x - px(CLOSE_SLOT + 6.0), hy));
        let isz = pxr(14.0).max(pixels(6.0));
        let ic = Rect::from_min_size(ic_c - Vec2::splat(half(isz)), Vec2::splat(isz));
        let name_st = Style::new(13.0, 700, th.text);
        let name_w = ic.min.x - tt.max.x - px(18.0);
        let lw = text::width(&ctx, &text::fit(&ctx, &name, name_st, name_w), name_st);
        let lh = text::line_height(&ctx, name_st);
        let lr = Rect::from_min_size(
            pos2(whole(ic.min.x - px(2.0)) - lw, whole(hy) - half(lh)),
            vec2(lw, lh),
        );
        let grow = px(12.0);
        let u = lr.union(ic);
        let pick = Rect::from_min_max(
            u.min - Vec2::splat(half(grow)),
            u.max + Vec2::splat(grow - half(grow)),
        );
        let resp = interact(ui, pick, "tuner-tuning", Sense::click());
        let hot = resp.hovered();
        if hot {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.tuning_choice = (self.tuning_choice + 1) % TUNINGS.len();
            let n = self.tuner_targets().0;
            self.say(format!("Tuner: {n}"), "");
        }
        icons::icon(&p, Icon::Right, ic_c, 14.0, if hot { th.text } else { th.text_2 });
        txt(
            &p,
            &name,
            pos2(ic.min.x - px(2.0), hy),
            13.0,
            if hot { th.text } else { th.text_2 },
            700,
            Align2::RIGHT_CENTER,
            Some(name_w),
        );
        let m = self.tuner_midi;
        let near = m.and_then(|m| {
            let i = (0..targets.len()).min_by(|&a, &b| {
                (targets[a] as f64 - m)
                    .abs()
                    .total_cmp(&(targets[b] as f64 - m).abs())
            })?;
            ((targets[i] as f64 - m).abs() <= 2.5).then_some(i) // not close to any string: just show the nearest note
        });
        let n = targets.len() as f32;
        let gap = px(6.0);
        let cw = (tw - gap * (n - 1.0)) / n.max(1.0);
        let cy0 = card.min.y + pad_y + px(18.0) + px(10.0);
        for (i, tg) in targets.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(whole(lx + i as f32 * (cw + gap)), cy0),
                vec2(whole(cw), px(28.0)),
            );
            let on = near == Some(i);
            if on {
                paint::inset(&p, r, Some(9.0), 2.0, Fill::Base);
            } else {
                paint::raised(&p, r, Some(9.0), 3.0, Fill::Base, 0.0);
            }
            let tcol = if on {
                th.accent_ink
            } else if string_colors_on() {
                STRING_PALETTE[i % 6]
            } else {
                th.text_2
            };
            txt(
                &p,
                &qrs_analysis::pitch::note_label(*tg as f64),
                center(r),
                12.0,
                tcol,
                800,
                Align2::CENTER_CENTER,
                None,
            );
        }
        let gauge = Rect::from_min_size(pos2(lx, cy0 + px(28.0) + px(6.0)), vec2(tw, gauge_h));
        let base = gauge.max.y + px(4.0) + px(24.0); // the bottom row's baseline
        let Some(m) = m else {
            knobs::arc_gauge(&p, gauge, None, th.text_2);
            self.text_base(
                &p,
                "play an open string",
                card.min.x + half(card.width()),
                base - px(4.0),
                14.0,
                th.caption,
                600,
                Align2::CENTER_TOP,
                None,
            );
            return card.max.y;
        };
        let target = near.map(|i| targets[i] as f64).unwrap_or(m.round_ties_even());
        let cents = (m - target) * 100.0;
        let col = if cents.abs() <= 5.0 { th.hit } else { th.late };
        knobs::arc_gauge(&p, gauge, Some(cents as f32), col);
        self.text_base(
            &p,
            &qrs_analysis::pitch::note_label(m),
            lx,
            base,
            26.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            None,
        );
        let verdict = if near.is_none() {
            "not near a string"
        } else if cents.abs() <= 5.0 {
            "in tune"
        } else if cents < 0.0 {
            "tune up"
        } else {
            "tune down"
        };
        self.text_base(
            &p,
            &format!("{cents:+.0}¢  ·  {verdict}"),
            card.max.x - pad_x,
            base,
            14.0,
            col,
            800,
            Align2::RIGHT_TOP,
            None,
        );
        card.max.y
    }

    /// Accuracy as a ring; the score with its multiplier and streak, the counts, and your best three runs along the
    /// bottom (a practice run says why it won't be saved).
    fn draw_score_tile(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32, lifting: bool) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let c = self.scorer.counts();
        let (pad_x, pad_y) = (px(16.0), px(14.0));
        let top = st.userdata.top(&self.ids, &self.part_name);
        let practice = !self.run_scored || !self.at_max_settings();
        if !self.has_input() {
            let card = Rect::from_min_size(pos2(x, y), vec2(w, px(72.0)));
            self.tile_card(&p, card, lifting);
            caps(
                &p,
                "SCORE",
                pos2(card.min.x + pad_x, card.min.y + pad_y),
                12.0,
                Align2::LEFT_TOP,
                None,
            );
            let r = txt(
                &p,
                "Scoring is off: no guitar input",
                pos2(card.min.x + pad_x, card.min.y + pad_y + px(24.0)),
                13.0,
                th.caption,
                500,
                Align2::LEFT_TOP,
                None,
            );
            keycap(
                &p,
                "F2",
                on_px(pos2(r.max.x + px(10.0), center(r).y)),
                Align2::LEFT_CENTER,
                11.0,
                CapOn::Material,
            );
            return card.max.y;
        }
        let ring_d = px(96.0);
        let mut lines: Vec<String> = Vec::new();
        let box_h = if practice {
            let why = if self.mastery < 0.999 {
                "below 100% difficulty"
            } else if (self.speed() - 1.0).abs() > 1e-6 {
                "slowed down"
            } else {
                "changed mid-song"
            };
            lines = text::wrap(
                &ctx,
                &format!("Practice run ({why}): not saved  ·  restart at 100% to score"),
                Style::new(12.0, 700, th.late),
                w - 2.0 * pad_x - px(24.0),
            )
            .into_iter()
            .take(2)
            .collect();
            px(14.0) + lines.len() as f32 * px(16.0)
        } else {
            px(30.0)
        };
        let card = Rect::from_min_size(pos2(x, y), vec2(w, pad_y + ring_d + px(10.0) + box_h + pad_y));
        self.tile_card(&p, card, lifting);
        let (lx, rx) = (card.min.x + pad_x, card.max.x - pad_x);
        let rc = pos2(lx + half(ring_d), card.min.y + pad_y + half(ring_d));
        let judged = c.judged();
        let acc = self.scorer.accuracy();
        knobs::ring_gauge(&p, rc, 40.0, 8.0, if judged > 0 { acc as f32 } else { 0.0 }, None);
        txt(
            &p,
            &if judged > 0 {
                format!("{}%", (acc * 100.0).round_ties_even() as i64)
            } else {
                "–".into()
            },
            pos2(rc.x, rc.y - px(5.0)),
            21.0,
            th.text,
            800,
            Align2::CENTER_CENTER,
            None,
        );
        txt(
            &p,
            "accuracy",
            pos2(rc.x, rc.y + px(14.0)),
            10.0,
            th.caption,
            700,
            Align2::CENTER_CENTER,
            None,
        );
        let cx0 = lx + ring_d + px(16.0);
        let ty = card.min.y + pad_y;
        caps(
            &p,
            "SCORE",
            pos2(cx0, ty + px(8.0)),
            12.0,
            Align2::LEFT_CENTER,
            None,
        );
        if !practice {
            let sr = txt(
                &p,
                &self.scorer.streak.to_string(),
                pos2(card.max.x - px(CLOSE_SLOT), ty + px(8.0)),
                12.0,
                th.text,
                800,
                Align2::RIGHT_CENTER,
                None,
            );
            txt(
                &p,
                "streak ",
                pos2(sr.min.x, ty + px(8.0)),
                12.0,
                th.caption,
                500,
                Align2::RIGHT_CENTER,
                None,
            );
        }
        let r = self.text_base(
            &p,
            &crate::home::group_digits(self.scorer.score() as usize),
            cx0,
            ty + px(48.0),
            28.0,
            if practice { th.text_2 } else { th.text },
            800,
            Align2::LEFT_TOP,
            Some(rx - cx0),
        );
        if self.scorer.multiplier() > 1 && !practice {
            self.text_base(
                &p,
                &format!("×{}", self.scorer.multiplier()),
                r.max.x + px(8.0),
                ty + px(48.0),
                16.0,
                th.accent_ink,
                800,
                Align2::LEFT_TOP,
                None,
            );
        }
        let colw = (rx - cx0) / 3.0;
        let col_max = whole(colw) - px(4.0);
        for (i, (val, col, label)) in [
            (c.hit, th.hit, "hit"),
            (c.late + c.early, th.late, "late/early"),
            (c.miss, th.miss, "missed"),
        ]
        .iter()
        .enumerate()
        {
            let xx = whole(cx0 + i as f32 * colw);
            txt(
                &p,
                &val.to_string(),
                pos2(xx, ty + px(58.0)),
                15.0,
                *col,
                800,
                Align2::LEFT_TOP,
                Some(col_max),
            );
            txt(
                &p,
                label,
                pos2(xx, ty + px(78.0)),
                11.0,
                th.caption,
                500,
                Align2::LEFT_TOP,
                Some(col_max),
            );
        }
        let bx = Rect::from_min_size(
            pos2(lx, card.min.y + pad_y + ring_d + px(10.0)),
            vec2(rx - lx, box_h),
        );
        paint::inset(&p, bx, Some(10.0), 2.0, Fill::Base);
        if practice {
            let mut yy = bx.min.y + px(7.0);
            for ln in &lines {
                txt(
                    &p,
                    ln,
                    pos2(bx.min.x + px(12.0), yy),
                    12.0,
                    th.late,
                    700,
                    Align2::LEFT_TOP,
                    Some(bx.width() - px(24.0)),
                );
                yy += px(16.0);
            }
            return card.max.y;
        }
        let bcy = center(bx).y;
        let bxx = caps(
            &p,
            "BEST",
            pos2(bx.min.x + px(12.0), bcy),
            10.0,
            Align2::LEFT_CENTER,
            None,
        )
        .max
        .x + px(14.0);
        let end = bx.max.x - px(12.0);
        if top.is_empty() {
            txt(
                &p,
                "none yet: finish the song to set one",
                pos2(bxx, bcy),
                12.0,
                th.caption,
                500,
                Align2::LEFT_CENTER,
                Some(end - bxx),
            );
            return card.max.y;
        }
        let slot = (end - bxx) / 3.0;
        let first = format!(
            "{}{}",
            crate::home::group_digits(top[0].score as usize),
            top[0]
                .acc
                .map(|a| format!("  ·  {}%", (a * 100.0).round_ties_even() as i64))
                .unwrap_or_default()
        );
        let fr = txt(
            &p,
            &first,
            pos2(bxx, bcy),
            12.0,
            th.accent_ink,
            800,
            Align2::LEFT_CENTER,
            Some(end - bxx),
        );
        let mut xx = whole(fr.max.x + px(16.0)).max(whole(bxx + slot * 1.25));
        for run in top.iter().skip(1) {
            if ipx(xx) > ipx(bx.max.x - px(40.0)) {
                break;
            }
            let rr = txt(
                &p,
                &crate::home::group_digits(run.score as usize),
                pos2(xx, bcy),
                12.0,
                th.text_2,
                700,
                Align2::LEFT_CENTER,
                Some(end - xx),
            );
            xx = whole(rr.max.x + px(16.0));
        }
        card.max.y
    }

    /// The MG-300 as a little pedal (crate::mg300::mini): its screen only shows; its keys, knobs and footswitches
    /// work like the real ones. The expand key (or a click on its screen) opens the big pedal view.
    #[cfg(feature = "nux")]
    fn draw_mg300_tile(&mut self, ui: &Ui, st: &mut AppState, x: f32, y: f32, w: f32, lifting: bool) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let snap = st.pedal().state();
        let (pad_x, pad_y) = (px(10.0), px(12.0));
        let pw = w - 2.0 * pad_x;
        let ph = crate::mg300::mini::MiniPedal::height_for(pw);
        let card = Rect::from_min_size(pos2(x, y), vec2(w, pad_y + px(24.0) + px(8.0) + ph + pad_y));
        self.tile_card(&p, card, lifting);
        let hy = card.min.y + pad_y + px(12.0);
        let tt = caps(
            &p,
            "MG-300",
            pos2(card.min.x + px(16.0), hy),
            12.0,
            Align2::LEFT_CENTER,
            None,
        );
        // (left of the close key's slot)
        let kd = px(26.0);
        let key = Rect::from_min_size(
            pos2(card.max.x - px(CLOSE_SLOT) - kd, hy - half(kd)),
            vec2(kd, kd),
        );
        if widgets::round_button(
            ui,
            key,
            "mg300-expand",
            Icon::Expand,
            12.0,
            widgets::RoundKind::Ivory,
            false,
        )
        .clicked()
        {
            self.acts.push("mg:open".into());
        }
        let id = qrs_mg300::preset_id(snap.program.unwrap_or(0) as usize);
        let status = match (snap.connected, snap.name.is_empty()) {
            (true, false) => format!("{id}  \u{b7}  {}", snap.name),
            (true, true) => id,
            _ => "not connected".into(),
        };
        txt(
            &p,
            &status,
            pos2(tt.max.x + px(10.0), hy),
            13.0,
            if snap.connected { th.accent_ink } else { th.caption },
            800,
            Align2::LEFT_CENTER,
            Some(key.min.x - px(10.0) - tt.max.x - px(10.0)),
        );
        let rect = Rect::from_min_size(pos2(card.min.x + pad_x, hy + px(12.0) + px(8.0)), vec2(pw, ph));
        let pedal = st.pedal();
        if let Some(crate::mg300::mini::MiniAct::Open) =
            self.mg_mini.ui(ui, rect, &snap, pedal, &mut self.mg_lcd)
        {
            self.acts.push("mg:open".into());
        }
        card.max.y
    }
}

/// A chord diagram in a small sunken box: strings run top to bottom (low string on the left), frets across, finger
/// numbers in the dots, x / o above muted / open strings, and a bar for barre fingers.
pub fn draw_chord_box(p: &egui::Painter, rect: Rect, sh: &HandShape, n_strings: usize) {
    let th = t();
    paint::inset(p, rect, Some(12.0), 3.0, Fill::Base);
    let n = n_strings.max(2);
    let frets: Vec<i32> = (0..n).map(|i| sh.frets.get(i).copied().unwrap_or(-1)).collect();
    let fingers: Vec<i32> = (0..n).map(|i| sh.fingers.get(i).copied().unwrap_or(-1)).collect();
    let fretted: Vec<i32> = frets.iter().copied().filter(|f| *f > 0).collect();
    let (lo, hi) = if fretted.is_empty() {
        (1, 1)
    } else {
        (*fretted.iter().min().unwrap(), *fretted.iter().max().unwrap())
    };
    let base = if hi <= 4 { 1 } else { lo };
    let rows = (hi - base + 1).max(4);
    let top = rect.min.y + px(18.0);
    let left = rect.min.x + px(16.0);
    let (gw, gh) = (rect.width() - px(28.0), rect.max.y - px(10.0) - top);
    let (dx, dy) = (gw / (n - 1) as f32, gh / rows as f32);
    let lw = line_px(1.0, 1.0);
    for i in 0..n {
        vline(p, whole(left + i as f32 * dx), top, top + gh, lw, th.tick);
    }
    for r in 0..=rows {
        let y = whole(top + r as f32 * dy);
        let nut = r == 0 && base == 1;
        hline(
            p,
            left,
            left + gw,
            y,
            if nut { line_px(3.0, 2.0) } else { lw },
            if nut {
                th.text_2
            } else {
                Color32::from_rgb(74, 79, 87)
            },
        );
    }
    if base > 1 {
        txt(
            p,
            &format!("{base}fr"),
            pos2(left - px(5.0), top + dy / 2.0),
            9.0,
            th.caption,
            700,
            Align2::RIGHT_CENTER,
            None,
        );
    }
    for (i, f) in frets.iter().enumerate() {
        let x = left + i as f32 * dx;
        if *f < 0 {
            txt(
                p,
                "×",
                pos2(x, top - px(3.0)),
                10.0,
                th.caption,
                800,
                Align2::CENTER_BOTTOM,
                None,
            );
        } else if *f == 0 {
            // pygame.draw.circle: a ring max(1, int(s)) px thick inside radius max(2, int(3 s)), round the whole
            // pixel (int(x), int(top - 8 s))
            let (ro, wd) = (px(3.0).max(pixels(2.0)), pixels(lw as f32));
            p.circle_stroke(
                pos2(whole(x), whole(top - 8.0)),
                ro - wd / 2.0,
                Stroke::new(wd, th.text_2),
            );
        }
    }
    let r = (dx.min(dy) * 0.46).max(pixels(3.0));
    let mut by_finger: std::collections::BTreeMap<(i32, i32), Vec<usize>> = Default::default();
    for (i, f) in frets.iter().enumerate() {
        if *f > 0 && fingers[i] > 0 {
            by_finger.entry((fingers[i], *f)).or_default().push(i);
        }
    }
    for ((_, f), strings) in &by_finger {
        if strings.len() > 1 {
            // a barre: one finger holding several strings at the same fret
            let y = top + ((*f - base) as f32 + 0.5) * dy;
            let (x1, x2) = (
                left + *strings.iter().min().unwrap() as f32 * dx,
                left + *strings.iter().max().unwrap() as f32 * dx,
            );
            paint::pill(
                p,
                Rect::from_min_size(
                    pos2(whole(x1 - r), whole(y - r)),
                    vec2(whole(x2 - x1 + 2.0 * r), whole(2.0 * r)),
                ),
                th.accent,
            );
        }
    }
    for (i, f) in frets.iter().enumerate() {
        if *f > 0 {
            let (x, y) = (left + i as f32 * dx, top + ((*f - base) as f32 + 0.5) * dy);
            paint::dot(p, pos2(x, y), r, th.accent);
            if fingers[i] > 0 && r >= 5.0 {
                txt(
                    p,
                    &fingers[i].to_string(),
                    pos2(x, y),
                    9.0,
                    th.base,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
        }
    }
}
