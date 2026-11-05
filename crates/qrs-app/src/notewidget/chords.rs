//! The note widget's drawer and its chord strip (P24.3, new in quickRS; D64). The arrow at the widget's top right
//! rolls the drawer out under the boxes (config "note_widget_more"); its first strip names the chord you're playing
//! from the sound alone (`qrs_analysis::chordname`: no chart) and shows a shape that plays it on a neck, each note a
//! dot in its chord tone's colour (the root gold, the third green, the fifth orange, the seventh pink...) with its
//! name on it, and the chord's last few names under it. The neck shows all 24 frets, or zoomed in (the default) 12
//! frets from where the dragger under it puts them (config "note_widget_zoom", "note_widget_from"); a shape out of
//! view brings the view to it unless the dragger was moved in the last few seconds. Experimental: a best guess.
//! P25 (D65): ALL SHAPES also shows every other place on the neck the chord is played, fainter (config
//! "note_widget_shapes"); the KEY picker takes the widget's key guess or a key chosen from its menu (config
//! "note_widget_chord_key"), and the chord's number in that key goes by its name (I, ii, V7, bVII...).

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, Pos2, Rect, Shape as Poly, Stroke, pos2, vec2};
use qrs_analysis::chordname::{self, Listener, Shape};
use qrs_analysis::keyguess::{Key, Mode};
use qrs_ui::text::Style;

use super::{
    BASE, DIM, FG, FRET, FRET_NUMBERS, H, HILITE, INLAY, NUT, NUT_W, OUT_OF_KEY, PAD, SHADE, STRING, SUB,
};
use super::{degree_color, ink, note_color, pil_text};

/// The drawer's height (points, under the widget's `H`).
pub const DRAWER_H: f32 = 172.0;
/// Frets in view when zoomed in.
pub const SPAN: f32 = 12.0;
/// The neck stays where the dragger put it this long before it follows a shape out of view.
const LEFT_ALONE: Duration = Duration::from_secs(8);
/// A chord not heard for this long is shown dimmed.
const STALE: f64 = 4.0;

/// The chord strip's box in the drawer, in the widget's points, for a window `w` wide.
pub fn strip_rect(w: f32) -> Rect {
    Rect::from_min_max(pos2(PAD, H - 4.0), pos2(w - PAD, H + DRAWER_H - PAD))
}

/// The neck's board in the strip, right of the chord's name, over the fret numbers and the dragger.
pub fn board_rect(w: f32) -> Rect {
    let b = strip_rect(w);
    Rect::from_min_max(
        pos2(b.min.x + 190.0, b.min.y + 30.0),
        pos2(b.max.x - 14.0, b.max.y - 40.0),
    )
}

/// The ZOOM button, at the strip's top right.
pub fn zoom_rect(w: f32) -> Rect {
    let b = strip_rect(w);
    Rect::from_min_size(pos2(b.max.x - 14.0 - 96.0, b.min.y + 6.0), vec2(96.0, 19.0))
}

/// The ALL SHAPES button, left of ZOOM.
pub fn shapes_rect(w: f32) -> Rect {
    zoom_rect(w).translate(vec2(-104.0, 0.0))
}

/// The KEY picker, from over the neck's left end to the ALL SHAPES button (with a song open, to the YOU / SONG
/// switch).
pub fn key_rect(w: f32, song: bool) -> Rect {
    let s = shapes_rect(w);
    let right = if song {
        source_rect(w, 0).min.x - 6.0
    } else {
        s.min.x - 8.0
    };
    Rect::from_min_max(pos2(board_rect(w).min.x, s.min.y), pos2(right, s.max.y))
}

/// The YOU / SONG switch's segment `i`, left of ALL SHAPES (only while a song is open).
pub fn source_rect(w: f32, i: usize) -> Rect {
    let s = shapes_rect(w);
    Rect::from_min_size(
        pos2(s.min.x - 8.0 - 2.0 * 38.0 + i as f32 * 38.0, s.min.y),
        vec2(38.0, s.height()),
    )
}

/// A key's name: "A major", "F# minor".
pub fn key_name((tonic, mode): Key) -> String {
    let m = if mode == Mode::Major { "major" } else { "minor" };
    format!("{} {m}", chordname::NOTE_NAMES[tonic as usize % 12])
}

/// The dragger's track, under the fret numbers, as wide as the frets.
pub fn track_rect(w: f32) -> Rect {
    let b = board_rect(w);
    Rect::from_min_max(
        pos2(b.min.x + NUT_W, b.max.y + 18.0),
        pos2(b.max.x - 3.0, b.max.y + 26.0),
    )
}

/// The arrow that opens and closes the drawer, at the widget's top right.
pub fn arrow_rect(w: f32) -> Rect {
    Rect::from_min_size(pos2(w - PAD - 20.0, PAD - 1.0), vec2(20.0, 20.0))
}

/// A chord tone's colour: the key's degree colours by what the tone is in the chord.
pub fn tone_color(iv: &str) -> Color32 {
    degree_color(match iv {
        "R" => "1",
        "b9" | "9" => "2",
        "b3" | "3" => "3",
        "4" => "4",
        "b5" | "5" | "#5" => "5",
        "6" => "6",
        "b7" | "7" => "7",
        _ => "",
    })
}

pub struct ChordStrip {
    listener: Listener,
    buf: Vec<f32>,
    /// The shape shown, and the chord (and how many notes heard) it was found for.
    pub shape: Option<Shape>,
    shape_of: Option<(String, usize)>,
    /// Zoomed in, and the first fret in view (0 = the nut) when it is.
    pub zoom: bool,
    pub from: f32,
    /// When the dragger was last moved.
    moved: Option<Instant>,
    /// Seconds since the widget started, when it last listened.
    now: f64,
    /// Every other place the chord is played is shown too (ALL SHAPES), and those places.
    pub all: bool,
    alts: Vec<Shape>,
    /// The key the chord is numbered in (the widget sets it every frame), and whether it's the widget's guess
    /// (else picked from the KEY menu).
    pub key: Option<Key>,
    pub key_auto: bool,
    /// With a song open, whether the key is the song's (SONG) rather than the guess or the pick (YOU); None
    /// without a song (no switch).
    pub song: Option<bool>,
}

impl Default for ChordStrip {
    fn default() -> Self {
        ChordStrip {
            listener: Listener::default(),
            buf: Vec::new(),
            shape: None,
            shape_of: None,
            zoom: true,
            from: 0.0,
            moved: None,
            now: 0.0,
            all: false,
            alts: Vec::new(),
            key: None,
            key_auto: true,
            song: None,
        }
    }
}

impl ChordStrip {
    /// One frame of the input (`read` fills the buffer with the latest samples; false if there aren't enough), at
    /// `t` seconds; `strings` is the neck's open strings.
    pub fn listen(&mut self, read: impl FnOnce(&mut [f32]) -> bool, rate: u32, strings: &[i32], t: f64) {
        self.now = t;
        self.buf.resize(Listener::frame_len(rate), 0.0);
        if !read(&mut self.buf) {
            return;
        }
        let lowest = strings.iter().copied().min().unwrap_or(40);
        self.listener.feed(&self.buf, rate, lowest, t);
        let Some(c) = &self.listener.current else { return };
        let key = (c.guess.name(), c.heard.len());
        if self.shape_of.as_ref() == Some(&key) {
            return;
        }
        let near = self
            .shape
            .as_ref()
            .and_then(|s| s.iter().copied().filter(|&f| f > 0).min());
        self.shape = chordname::shape(&c.guess, &c.heard, strings, near);
        self.alts = if self.all {
            chordname::shapes(&c.guess, strings)
        } else {
            Vec::new()
        };
        self.shape_of = Some(key);
        // a shape out of view: go to it, unless the dragger was just moved
        if let Some(s) = &self.shape
            && self.zoom
            && self.moved.is_none_or(|m| m.elapsed() > LEFT_ALONE)
        {
            let fretted: Vec<i32> = s.iter().copied().filter(|&f| f > 0).collect();
            if let (Some(&lo), Some(&hi)) = (fretted.iter().min(), fretted.iter().max())
                && (lo as f32 - 1.0 < self.from || hi as f32 > self.from + SPAN)
            {
                self.from = (lo as f32 - 1.0).clamp(0.0, 24.0 - SPAN);
            }
        }
    }

    /// Screenshot mode: the strip as it is after hearing `midis` (named, its shape found), the chords before it in
    /// `history`.
    pub fn stage(&mut self, midis: &[i32], history: &[&str], strings: &[i32]) {
        let heard: Vec<chordname::HeardNote> = midis
            .iter()
            .map(|&midi| chordname::HeardNote { midi, strength: 1.0 })
            .collect();
        let Some(guess) = chordname::name(&heard) else {
            return;
        };
        self.shape = chordname::shape(&guess, &heard, strings, None);
        self.alts = if self.all {
            chordname::shapes(&guess, strings)
        } else {
            Vec::new()
        };
        self.listener.history = history.iter().map(|s| s.to_string()).collect();
        self.listener.current = Some(chordname::Current {
            guess,
            heard,
            at: 0.0,
            last: 0.0,
        });
        self.now = 0.0;
    }

    pub fn reset(&mut self) {
        self.listener.reset();
        self.shape = None;
        self.shape_of = None;
        self.alts.clear();
    }

    /// ALL SHAPES on or off (the places are found at the next listen).
    pub fn set_all(&mut self, on: bool) {
        self.all = on;
        self.shape_of = None;
        if !on {
            self.alts.clear();
        }
    }

    /// The chord ringing now (heard in the last quarter second, three notes or more): its shape.
    pub fn ringing_shape(&self) -> Option<&[i32]> {
        let c = self.listener.current.as_ref()?;
        (self.now - c.last < 0.25 && c.heard.len() >= 3)
            .then_some(self.shape.as_deref())
            .flatten()
    }

    /// The pitch classes of the chord ringing now (empty when none is).
    pub fn ringing_tones(&self) -> Vec<u8> {
        let Some(c) = &self.listener.current else {
            return Vec::new();
        };
        if self.now - c.last >= 0.25 || c.heard.len() < 2 {
            return Vec::new();
        }
        let mut pcs: Vec<u8> = c.heard.iter().map(|h| h.midi.rem_euclid(12) as u8).collect();
        pcs.sort();
        pcs.dedup();
        pcs
    }

    /// Move the zoomed view by `dx` points of the dragger's track (`track_w` wide); true when it moved.
    pub fn drag(&mut self, dx: f32, track_w: f32) -> bool {
        let thumb = track_w * SPAN / 24.0;
        let from = (self.from + dx * (24.0 - SPAN) / (track_w - thumb).max(1.0)).clamp(0.0, 24.0 - SPAN);
        self.moved = Some(Instant::now());
        let moved = from != self.from;
        self.from = from;
        moved
    }

    /// The view put whole frets in after a drag.
    pub fn settle(&mut self) {
        self.from = self.from.round().clamp(0.0, 24.0 - SPAN);
    }

    /// The dragger's thumb in the track.
    pub fn thumb(&self, track: Rect) -> Rect {
        let w = track.width() * SPAN / 24.0;
        let x = track.min.x + (track.width() - w) * self.from / (24.0 - SPAN);
        Rect::from_min_size(pos2(x, track.min.y), vec2(w, track.height()))
    }

    /// The strip over its sunken box (the background has it): the chord's name, its tones, the notes heard, the
    /// last few chords; the ZOOM button; the neck with the shape; the dragger. `at` maps widget points to the
    /// screen, `u` points a widget point; `hot` is what the pointer is over (1 the ZOOM button, 2 the dragger, 3 the
    /// ALL SHAPES button, 4 the KEY picker, 5 and 6 the YOU / SONG switch's segments).
    pub fn draw(
        &self,
        p: &egui::Painter,
        u: f32,
        at: impl Fn(f32, f32) -> Pos2,
        w: f32,
        strings: &[i32],
        hot: u8,
    ) {
        let bx = strip_rect(w);
        let st = |size: f32, weight: u16, color: Color32| Style::new(size * u, weight, color);
        let current = self.listener.current.as_ref();
        let stale = current.is_none_or(|c| self.now - c.last > STALE);
        // the chord's name, its tones in their colours, what was heard
        let x = bx.min.x + 14.0;
        match current {
            None => {
                pil_text(
                    p,
                    "\u{2014}",
                    at(x, bx.min.y + 34.0),
                    Align2::LEFT_TOP,
                    st(34.0, 800, DIM),
                );
                pil_text(
                    p,
                    "strum a chord",
                    at(x, bx.min.y + 84.0),
                    Align2::LEFT_TOP,
                    st(11.0, 500, DIM),
                );
            }
            Some(c) => {
                let name = c.guess.name();
                let size = if name.chars().count() <= 4 {
                    34.0
                } else if name.chars().count() <= 6 {
                    27.0
                } else {
                    21.0
                };
                let col = if stale { DIM } else { FG };
                pil_text(
                    p,
                    &name,
                    at(x, bx.min.y + 34.0),
                    Align2::LEFT_TOP,
                    st(size, 800, col),
                );
                // its number in the key, in its root's degree colour (grey outside the key)
                if let Some(key) = self.key {
                    let n = c.guess.numeral(key);
                    let nx = x + qrs_ui::text::width(p.ctx(), &name, st(size, 800, col)) / u + 10.0;
                    let fill = match (stale, n.diatonic, n.degree) {
                        (false, true, Some(d)) => degree_color(&d.to_string()),
                        _ => OUT_OF_KEY,
                    };
                    let tw = qrs_ui::text::width(p.ctx(), &n.text, st(14.0, 800, FG)) / u;
                    let r = Rect::from_min_size(
                        at(nx, bx.min.y + 37.0 + (size - 21.0) * 0.5),
                        vec2(tw + 16.0, 24.0) * u,
                    );
                    p.rect_filled(r, 12.0 * u, fill);
                    pil_text(
                        p,
                        &n.text,
                        r.center(),
                        Align2::CENTER_CENTER,
                        st(14.0, 800, ink(fill)),
                    );
                }
                for (i, &(pc, iv)) in c.guess.tones.iter().enumerate() {
                    let cen = at(x + 9.0 + i as f32 * 25.0, bx.min.y + 92.0);
                    let fill = if stale { OUT_OF_KEY } else { tone_color(iv) };
                    p.circle_filled(cen, 10.0 * u, fill);
                    pil_text(
                        p,
                        chordname::NOTE_NAMES[pc as usize],
                        cen,
                        Align2::CENTER_CENTER,
                        st(9.0, 800, ink(fill)),
                    );
                    pil_text(
                        p,
                        iv,
                        at(x + 9.0 + i as f32 * 25.0, bx.min.y + 105.0),
                        Align2::CENTER_TOP,
                        st(8.0, 700, DIM),
                    );
                }
                let mut heard: Vec<i32> = c.heard.iter().map(|h| h.midi).collect();
                heard.sort();
                let heard: Vec<String> = heard
                    .iter()
                    .take(7)
                    .map(|&m| qrs_analysis::pitch::note_label(m as f64))
                    .collect();
                pil_text(
                    p,
                    &format!("heard  {}", heard.join(" ")),
                    at(x, bx.min.y + 121.0),
                    Align2::LEFT_TOP,
                    st(9.0, 500, DIM),
                );
            }
        }
        let hist: Vec<&str> = self.listener.history.iter().map(String::as_str).collect();
        let shown = &hist[hist.len().saturating_sub(5)..];
        pil_text(
            p,
            &shown.join("  \u{b7}  "),
            at(x, bx.max.y - 18.0),
            Align2::LEFT_TOP,
            st(10.0, 600, SUB),
        );
        // ALL SHAPES, and the KEY picker
        let a = shapes_rect(w);
        let ar = Rect::from_min_max(at(a.min.x, a.min.y), at(a.max.x, a.max.y));
        p.rect_filled(ar, 9.5 * u, if hot == 3 { HILITE } else { BASE });
        p.rect_stroke(ar, 9.5 * u, Stroke::new(u, FRET), egui::StrokeKind::Inside);
        pil_text(
            p,
            if self.all { "ONE SHAPE" } else { "ALL SHAPES" },
            ar.center(),
            Align2::CENTER_CENTER,
            st(10.0, 800, if hot == 3 { FG } else { SUB }),
        );
        let k = key_rect(w, self.song.is_some());
        let kr = Rect::from_min_max(at(k.min.x, k.min.y), at(k.max.x, k.max.y));
        p.rect_filled(kr, 9.5 * u, if hot == 4 { HILITE } else { BASE });
        p.rect_stroke(kr, 9.5 * u, Stroke::new(u, FRET), egui::StrokeKind::Inside);
        let (kname, kcol) = match self.key {
            Some(key) => (key_name(key), if hot == 4 { FG } else { SUB }),
            None => ("listening for a key".into(), DIM),
        };
        let how = match (self.song, self.key_auto) {
            (Some(true), _) => "song",
            (_, true) => "auto",
            (_, false) => "set",
        };
        if let Some(on) = self.song {
            let rects = [source_rect(w, 0), source_rect(w, 1)];
            let hot = [5, 6].iter().position(|&h| h == hot);
            super::draw_switch(p, u, &at, &rects, &["YOU", "SONG"], on as usize, hot, 9.0);
        }
        pil_text(
            p,
            &format!("KEY  {kname}  \u{b7}  {how}"),
            at(k.min.x + 10.0, k.center().y),
            Align2::LEFT_CENTER,
            st(10.0, 800, kcol),
        );
        let c = k.right_center() - vec2(12.0, 0.0);
        p.add(Poly::convex_polygon(
            vec![
                at(c.x - 4.0, c.y - 2.0),
                at(c.x + 4.0, c.y - 2.0),
                at(c.x, c.y + 3.0),
            ],
            kcol,
            Stroke::NONE,
        ));
        // ZOOM
        let z = zoom_rect(w);
        let zr = Rect::from_min_max(at(z.min.x, z.min.y), at(z.max.x, z.max.y));
        p.rect_filled(zr, 9.5 * u, if hot == 1 { HILITE } else { BASE });
        p.rect_stroke(zr, 9.5 * u, Stroke::new(u, FRET), egui::StrokeKind::Inside);
        let label = if self.zoom { "ALL 24 FRETS" } else { "ZOOM IN" };
        pil_text(
            p,
            label,
            zr.center(),
            Align2::CENTER_CENTER,
            st(10.0, 800, if hot == 1 { FG } else { SUB }),
        );
        self.draw_neck(p, u, &at, w, strings, current.map(|c| &c.guess), stale);
        if self.zoom {
            let tr = track_rect(w);
            let r = Rect::from_min_max(at(tr.min.x, tr.min.y), at(tr.max.x, tr.max.y));
            p.rect_filled(r, 4.0 * u, SHADE);
            // where the shape is, as ticks on the track (the other places dimmer)
            for s in &self.alts {
                for &f in s.iter().filter(|&&f| f > 0) {
                    let fx = tr.min.x + (f as f32 - 0.5) / 24.0 * tr.width();
                    p.line_segment(
                        [at(fx, tr.min.y), at(fx, tr.max.y)],
                        Stroke::new(u, degree_color("1").gamma_multiply(0.45)),
                    );
                }
            }
            if let Some(s) = &self.shape {
                for &f in s.iter().filter(|&&f| f > 0) {
                    let fx = tr.min.x + (f as f32 - 0.5) / 24.0 * tr.width();
                    p.line_segment(
                        [at(fx, tr.min.y - 2.0), at(fx, tr.max.y + 2.0)],
                        Stroke::new(1.5 * u, degree_color("1")),
                    );
                }
            }
            let th = self.thumb(tr);
            let col = if hot == 2 {
                Color32::from_rgb(0x8a, 0x90, 0x9a)
            } else {
                Color32::from_rgb(0x5d, 0x63, 0x6c)
            };
            p.rect_filled(
                Rect::from_min_max(at(th.min.x, th.min.y), at(th.max.x, th.max.y)),
                4.0 * u,
                col.gamma_multiply(0.85),
            );
        }
    }

    /// The neck: an open-string column left of the nut (the shape's open strings as dots, its muted ones as x),
    /// then the frets in view; the shape's notes as dots in their chord tone's colour, a barre as a bar across its
    /// strings, an arrow at the edge for any note out of view.
    fn draw_neck(
        &self,
        p: &egui::Painter,
        u: f32,
        at: &impl Fn(f32, f32) -> Pos2,
        w: f32,
        strings: &[i32],
        guess: Option<&chordname::Guess>,
        stale: bool,
    ) {
        let board = board_rect(w);
        let n = strings.len().max(1);
        let x0 = board.min.x + NUT_W;
        let x1 = board.max.x - 3.0;
        let (from, span) = if self.zoom { (self.from, SPAN) } else { (0.0, 24.0) };
        let fw = (x1 - x0) / span;
        let m = 8.0;
        let gap = (board.height() - 2.0 * m) / (n - 1).max(1) as f32;
        let sy = |s: usize| board.min.y + m + (n - 1 - s) as f32 * gap;
        let wire = |f: f32| x0 + (f - from) * fw;
        let fx = |f: i32| {
            if f == 0 {
                board.min.x + NUT_W / 2.0
            } else {
                wire(f as f32 - 0.5)
            }
        };
        let clip = p.with_clip_rect(Rect::from_min_max(
            at(x0 - 0.5, board.min.y),
            at(x1 + 0.5, board.max.y + 16.0),
        ));
        let mid = (board.min.y + board.max.y) / 2.0;
        for f in FRET_NUMBERS {
            let dots: &[f32] = if f % 12 == 0 { &[-1.0, 1.0] } else { &[0.0] };
            for d in dots {
                clip.circle_filled(at(fx(f), mid + d * gap), 2.4 * u, INLAY);
            }
            pil_text(
                &clip,
                &f.to_string(),
                at(fx(f), board.max.y + 3.0),
                Align2::CENTER_TOP,
                Style::new(9.0 * u, 700, DIM),
            );
        }
        for f in 1..=24 {
            clip.line_segment(
                [
                    at(wire(f as f32), board.min.y + 3.0),
                    at(wire(f as f32), board.max.y - 3.0),
                ],
                Stroke::new(u, FRET),
            );
        }
        if from < 0.05 {
            p.rect_filled(
                Rect::from_min_max(at(x0 - 3.0, board.min.y + 3.0), at(x0, board.max.y - 3.0)),
                u,
                NUT,
            );
        } else {
            p.line_segment(
                [at(x0, board.min.y + 3.0), at(x0, board.max.y - 3.0)],
                Stroke::new(1.5 * u, FRET),
            );
        }
        for s in 0..n {
            let thick = [1.6, 1.4, 1.2, 1.0, 0.9, 0.8][s.min(5)];
            p.line_segment([at(x0, sy(s)), at(x1, sy(s))], Stroke::new(thick * u, STRING));
        }
        let (Some(shape), Some(g)) = (&self.shape, guess) else {
            return;
        };
        let r = (fw * 0.36).min(gap * 0.46).min(7.5);
        // ALL SHAPES: the other places first, smaller and fainter, under the one heard
        for alt in self.alts.iter().filter(|a| Some(*a) != self.shape.as_ref()) {
            for (s, &f) in alt.iter().enumerate().take(n) {
                let out = (f as f32 - 0.5) < from || (f as f32 - 0.5) > from + span;
                if f <= 0 || out || shape.get(s) == Some(&f) {
                    continue;
                }
                let pc = (strings[s] + f).rem_euclid(12) as u8;
                let fill = match (stale, g.interval(pc)) {
                    (false, Some(iv)) => tone_color(iv),
                    _ => OUT_OF_KEY,
                }
                .gamma_multiply(0.5);
                let c = at(fx(f), sy(s));
                clip.circle_filled(c, r * 0.82 * u, fill);
                pil_text(
                    &clip,
                    chordname::NOTE_NAMES[pc as usize],
                    c,
                    Align2::CENTER_CENTER,
                    Style::new(6.5 * u, 800, FG.gamma_multiply(0.7)),
                );
            }
        }
        // a barre: the lowest fret held across strings by one finger
        let fretted: Vec<i32> = shape.iter().copied().filter(|&f| f > 0).collect();
        if let Some(&minf) = fretted.iter().min()
            && fretted.len() > 4
            && fretted.iter().filter(|&&f| f == minf).count() > 1
        {
            let on: Vec<usize> = (0..shape.len()).filter(|&s| shape[s] == minf).collect();
            let (a, b) = (on[0], on[on.len() - 1]);
            let bar = Rect::from_min_max(
                at(fx(minf) - r * 0.7, sy(b) - r * 0.7),
                at(fx(minf) + r * 0.7, sy(a) + r * 0.7),
            );
            clip.rect_filled(
                bar,
                r * 0.7 * u,
                Color32::from_rgba_unmultiplied(0xe8, 0xea, 0xed, 40),
            );
        }
        let (mut left, mut right) = (false, false);
        for (s, &f) in shape.iter().enumerate().take(n) {
            if f < 0 {
                pil_text(
                    p,
                    "\u{d7}",
                    at(fx(0), sy(s)),
                    Align2::CENTER_CENTER,
                    Style::new(11.0 * u, 800, DIM),
                );
                continue;
            }
            if f > 0 && (f as f32 - 0.5) < from {
                left = true;
                continue;
            }
            if f > 0 && (f as f32 - 0.5) > from + span {
                right = true;
                continue;
            }
            let midi = strings[s] + f;
            let pc = midi.rem_euclid(12) as u8;
            let fill = match (stale, g.interval(pc)) {
                (true, _) => OUT_OF_KEY,
                (false, Some(iv)) => tone_color(iv),
                (false, None) => note_color(pc as usize),
            };
            let c = at(fx(f), sy(s));
            let painter = if f == 0 { p } else { &clip };
            painter.circle_filled(c, r * u, fill);
            pil_text(
                painter,
                chordname::NOTE_NAMES[pc as usize],
                c,
                Align2::CENTER_CENTER,
                Style::new(7.5 * u, 800, ink(fill)),
            );
        }
        // notes out of view: an arrow at that edge
        let gold = degree_color("1");
        for (on, x, dir) in [(left, x0 + 6.0, -1.0), (right, x1 - 6.0, 1.0)] {
            if on {
                let pts = vec![
                    at(x + dir * 5.0, mid),
                    at(x - dir * 2.0, mid - 7.0),
                    at(x - dir * 2.0, mid + 7.0),
                ];
                p.add(Poly::convex_polygon(pts, gold, Stroke::NONE));
            }
        }
    }
}

/// The drawer's arrow: pointing down to open it, up to close it.
pub fn draw_arrow(p: &egui::Painter, u: f32, at: impl Fn(f32, f32) -> Pos2, w: f32, open: bool, hot: bool) {
    let r = arrow_rect(w);
    let c = r.center();
    if hot {
        p.circle_filled(at(c.x, c.y), 10.0 * u, HILITE);
    }
    let d = if open { -1.0 } else { 1.0 };
    let pts = [
        at(c.x - 5.0, c.y - 2.5 * d),
        at(c.x, c.y + 2.5 * d),
        at(c.x + 5.0, c.y - 2.5 * d),
    ];
    let col = if hot { FG } else { SUB };
    p.line_segment([pts[0], pts[1]], Stroke::new(2.0 * u, col));
    p.line_segment([pts[1], pts[2]], Stroke::new(2.0 * u, col));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dragger_moves_the_view_along_the_neck() {
        let mut c = ChordStrip::default();
        let tr = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 8.0));
        assert_eq!(c.thumb(tr).min.x, 0.0);
        assert!(c.drag(1000.0, tr.width()));
        assert_eq!(c.from, 24.0 - SPAN, "stops at the 24th fret");
        assert!((c.thumb(tr).max.x - tr.max.x).abs() < 1e-3);
        c.drag(-61.0, tr.width());
        c.settle();
        assert_eq!(c.from, c.from.round());
        assert!(!c.drag(-10_000.0, tr.width()) || c.from == 0.0);
    }

    #[test]
    fn chord_tones_take_the_degree_colours() {
        assert_eq!(tone_color("R"), degree_color("1"));
        assert_eq!(tone_color("b3"), tone_color("3"));
        assert_eq!(tone_color("b7"), degree_color("7"));
    }
}
