//! The note widget (P7.3; port of rsplay/notewidget.py and notefeed.py): a small always-on-top window showing the
//! note you're playing and the key you seem to be in. The Python app ran it as its own process fed over UDP; here
//! it's a second window of the app (an egui viewport) fed directly: the pitch from the guitar input the engine
//! captures (the tuner's McLeod method, the median of the last five readings, held for a second), the key from
//! `qrs_analysis::keyguess` (a fading pitch-class histogram against the Krumhansl-Kessler profiles).
//!
//! Left box: the note, on a disc coloured by the note (each of the 12 its own colour). Right box: the key, on a
//! disc coloured by which degree of the key the note is (1 to 7; grey outside the key). Third box (P11.2, not in
//! the Python widget): the key's scale over the whole neck, nut to fret 24, in the open song's tuning (E standard
//! when no song is open), each note a dot in its degree's colour and the note you're playing ringed; the menu
//! hides or shows it (config "note_widget_scale"). A click on the scale box pops it up large (the window shows
//! just the scale, 2.4 times the size); a click on it there puts it back (P11.3). Drag it to move it (it remembers
//! where); right-click for its menu. Settings > Audio switches it on or off (config "note_widget"). The arrow at
//! the top right rolls out a drawer under it (P24.3, chords.rs): the chord you're playing, named, on a neck; and
//! under that (P25, heat.rs; D68) the notes just played on two necks, the tab's and yours, each fading away. The FAST / NORMAL
//! pill by the title switches low-latency output (WASAPI exclusive, Settings > Audio's switch) on and off (D66).
//! ALL / ROOT / OFF by the SCALE caption picks what the neck marks (D69, config "note_widget_scale_marks"): every note
//! of the key in its degree's colour, only the roots in colour (the rest grey), or nothing but what you play (in its
//! note's colour, named). The grip at the bottom right resizes the whole widget (config "note_widget_size", 0.6 to
//! 2 times).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{
    Align, Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, ViewportBuilder, ViewportCommand,
    ViewportId, pos2, vec2,
};
use qrs_analysis::keyguess::{Key, KeyGuess, Mode};
use qrs_analysis::spectrum::Spectra;
use qrs_formats::model::{Arrangement, GUITAR_OPEN_MIDI};
use qrs_ui::keys::hls_to_rgb;
use qrs_ui::raster::{Layers, Mask, mix};
use qrs_ui::text::{self, Style};

use crate::state::{AppState, AudioMode};

mod chords;
mod heat;
use chords::{ChordStrip, DRAWER_H};
use heat::{HEAT_H, HeatStrip};

const W: f32 = 272.0;
const H: f32 = 196.0;
const PAD: f32 = 14.0;
const BOX_W: f32 = 116.0;
const BOX_H: f32 = 140.0;
const GAP: f32 = 12.0;
const DISC: f32 = 72.0;
/// The scale box: as tall as the others, wide enough for the nut and 24 frets.
const NECK_W: f32 = 436.0;
/// A scale note's dot on the neck (radius), and the open strings' column left of the nut (room for a dot).
const SPOT: f32 = 5.5;
const NUT_W: f32 = 18.0;
/// The frets numbered under the neck (and inlaid; 12 and 24 twice), as the player's neck tile.
const FRET_NUMBERS: [i32; 10] = [3, 5, 7, 9, 12, 15, 17, 19, 21, 24];
/// The widget's size range (times its own).
const ZOOM: (f32, f32) = (0.6, 2.0);
/// How much bigger the scale box is when it's popped up.
const BIG: f32 = 2.4;
const EVERY: Duration = Duration::from_millis(50);
/// A background drawn for (which discs are live, the scale box and the drawer, its pixel scale x 256).
type BgKey = ((bool, bool), (bool, bool), u32);
/// How many backgrounds are kept (P27.3): both disc states at the size it's at, and a size or two before.
const BG_KEEP: usize = 4;
/// Idle (no sound for `QUIET_AFTER`, nothing moving), how often it looks for sound (P27.1).
const IDLE_POLL: Duration = Duration::from_millis(250);
const QUIET_AFTER: Duration = Duration::from_secs(3);
/// Keep showing a note this long after the guitar goes quiet.
const HOLD: f32 = 1.0;

use qrs_formats::model::NOTE_NAMES;
/// The scale degree of each semitone above the tonic; the ones outside the key named as raised / lowered degrees.
const DEG_MAJOR: [&str; 12] = ["1", "♭2", "2", "♭3", "3", "4", "♯4", "5", "♭6", "6", "♭7", "7"];
const DEG_MINOR: [&str; 12] = ["1", "♭2", "2", "3", "♯3", "4", "♯4", "5", "6", "♯6", "7", "♯7"];
const IN_MAJOR: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
const IN_MINOR: [usize; 7] = [0, 2, 3, 5, 7, 8, 10];
const OUT_OF_KEY: Color32 = Color32::from_rgb(0x5d, 0x5d, 0x6a);

fn degree_color(d: &str) -> Color32 {
    match d {
        "1" => Color32::from_rgb(0xf5, 0xc5, 0x42),
        "2" => Color32::from_rgb(0x7f, 0xc8, 0xf8),
        "3" => Color32::from_rgb(0x4c, 0xaf, 0x7d),
        "4" => Color32::from_rgb(0xb4, 0x8e, 0xf0),
        "5" => Color32::from_rgb(0xf0, 0x8c, 0x4a),
        "6" => Color32::from_rgb(0x4f, 0xd1, 0xc5),
        "7" => Color32::from_rgb(0xef, 0x6f, 0x8a),
        _ => OUT_OF_KEY,
    }
}

/// A MIDI note's degree (1 to 7) in a key's scale; None outside it.
fn scale_degree((tonic, mode): Key, midi: i32) -> Option<&'static str> {
    let rel = (midi - tonic as i32).rem_euclid(12) as usize;
    let (degs, inside) = if mode == Mode::Major {
        (&DEG_MAJOR, IN_MAJOR.contains(&rel))
    } else {
        (&DEG_MINOR, IN_MINOR.contains(&rel))
    };
    inside.then_some(degs[rel])
}

/// The window's width: the two boxes, or three with the scale (or the drawer, which is as wide).
fn width(scale: bool, more: bool) -> f32 {
    if scale || more { W + GAP + NECK_W } else { W }
}

/// The window's height: with the drawer (the chord and heat strips), the drawer's more.
fn height(more: bool) -> f32 {
    if more { H + DRAWER_H + HEAT_H } else { H }
}

/// A key as the config keeps it ("9 minor"), and back; "auto" (or anything else) is None.
fn key_to_config(key: Option<Key>) -> String {
    match key {
        Some((t, m)) => format!("{t} {}", if m == Mode::Major { "major" } else { "minor" }),
        None => "auto".into(),
    }
}

fn key_from_config(s: &str) -> Option<Key> {
    let (t, m) = s.split_once(' ')?;
    let t: u8 = t.parse().ok().filter(|&t| t < 12)?;
    match m {
        "major" => Some((t, Mode::Major)),
        "minor" => Some((t, Mode::Minor)),
        _ => None,
    }
}

/// What the scale box's neck marks: the key's notes by degree, the roots only (the rest grey), or none (D69).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Marks {
    All,
    Roots,
    Off,
}

impl Marks {
    const ALL: [Marks; 3] = [Marks::All, Marks::Roots, Marks::Off];

    fn label(self) -> &'static str {
        match self {
            Marks::All => "ALL",
            Marks::Roots => "ROOT",
            Marks::Off => "OFF",
        }
    }

    fn config(self) -> &'static str {
        match self {
            Marks::All => "all",
            Marks::Roots => "roots",
            Marks::Off => "off",
        }
    }

    fn from_config(s: &str) -> Marks {
        Marks::ALL
            .into_iter()
            .find(|m| m.config() == s)
            .unwrap_or(Marks::All)
    }

    fn tip(self) -> &'static str {
        match self {
            Marks::All => "Every note of the key, coloured by degree",
            Marks::Roots => "Only the roots in colour, the rest grey",
            Marks::Off => "An empty neck: only the notes you play light up",
        }
    }
}

/// The scale box's ALL / ROOT / OFF switch, segment `i`: after the SCALE caption.
fn marks_rect(i: usize) -> Rect {
    let b = box_rect(2);
    Rect::from_min_size(
        pos2(b.min.x + 60.0 + i as f32 * 33.0, b.min.y + 6.0),
        vec2(33.0, 15.0),
    )
}

/// The grey of the scale's other notes when only the roots are coloured.
const GREY_NOTE: Color32 = Color32::from_rgb(0x6b, 0x71, 0x7b);

/// The low-latency pill, right of the title.
fn fast_rect() -> Rect {
    Rect::from_min_size(pos2(PAD + 72.0, PAD + 1.0), vec2(52.0, 17.0))
}

/// Settings > Audio's FAST green.
const FAST: Color32 = Color32::from_rgb(52, 199, 123);

/// The 12 notes round the colour wheel (C red, E yellow-green, G# blue...).
pub fn note_color(pc: usize) -> Color32 {
    let (r, g, b) = hls_to_rgb(pc as f32 / 12.0, 0.56, 0.62);
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Text that reads on a background colour: (main, secondary).
fn ink(c: Color32) -> Color32 {
    let light = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32 > 150.0;
    if light {
        Color32::from_rgb(0x15, 0x15, 0x1a)
    } else {
        Color32::from_rgb(0xe8, 0xea, 0xed)
    }
}

/// One of the two boxes: what it shows, gliding between colours.
struct Shown {
    fill: Option<Color32>,
    target: Option<Color32>,
    big: String,
    small: String,
}

impl Shown {
    fn new() -> Shown {
        Shown {
            fill: None,
            target: None,
            big: "—".into(),
            small: String::new(),
        }
    }

    fn set(&mut self, target: Option<Color32>, big: &str, small: &str) {
        self.target = target;
        self.big = big.into();
        self.small = small.into();
    }

    /// The glide has arrived (each channel within 2 of the target), or there's nothing to show.
    fn settled(&self) -> bool {
        match (self.fill, self.target) {
            (Some(f), Some(t)) => {
                f.r().abs_diff(t.r()) <= 2 && f.g().abs_diff(t.g()) <= 2 && f.b().abs_diff(t.b()) <= 2
            }
            (None, None) => true,
            _ => false,
        }
    }

    /// Glide toward the target (0.35 of the way every 30 ms, as the Python widget did).
    fn step(&mut self, dt: f32) {
        match (self.fill, self.target) {
            (_, None) => self.fill = None,
            (f, Some(t_)) => {
                // (a disc that lights up glides from the panel's colour, as the Python widget's)
                let k = 1.0 - 0.65f32.powf(dt / 0.03);
                self.fill = Some(mix(f.unwrap_or(BASE), t_, k));
            }
        }
    }
}

pub struct NoteWidget {
    key: KeyGuess,
    hist: VecDeque<f64>,
    midi: Option<f64>,
    seen: Option<Instant>,
    next: Instant,
    last_step: Instant,
    sp: Spectra,
    buf: Vec<f32>,
    note: Shown,
    deg: Shown,
    status: String,
    /// Closed from its menu (until switched on again, or the next start).
    pub closed: bool,
    was_on: bool,
    start: Instant,
    pos: Option<Pos2>,
    /// The drawn background for (which discs are live, the scale box, the scale it was drawn at).
    /// (P27.3: the last few kept, so a note starting or stopping doesn't draw it again; while the grip is dragged one
    /// of the same layout is stretched, and it's drawn sharp once the drag ends)
    bg: Vec<(BgKey, egui::TextureHandle)>,
    /// What the pointer is over: the drawer's arrow; the ZOOM button (1), the dragger (2), ALL SHAPES (3), KEY (4).
    hot: (bool, u8),
    /// The window has its rounded corners and border (Windows 11).
    styled: bool,
    /// The scale box is shown (config "note_widget_scale").
    show_scale: bool,
    /// What the scale box draws: the key, and the note being played (MIDI).
    scale: Option<Key>,
    playing: Option<i32>,
    /// The neck's open strings (MIDI, low to high) and the tuning's name.
    strings: Vec<i32>,
    tuning: String,
    /// The width the window was last fitted on its screen at.
    fitted_w: Option<f32>,
    /// The scale box is popped up large, and where the window was before (it goes back there).
    big: bool,
    small_pos: Option<Pos2>,
    /// The drawer is out (config "note_widget_more"), its chord strip, and its zoom read from the config yet.
    more: bool,
    chords: ChordStrip,
    zoom_read: bool,
    /// The dragger is being dragged.
    dragging: bool,
    /// The key picked in the chord strip's KEY menu (None: the widget's guess), the heat strip, and when the
    /// widget last listened.
    key_pick: Option<Key>,
    /// Low-latency output is on (Some), off, or can't be switched here (None: no engine, silent, not Windows).
    fast: Option<bool>,
    /// The pointer is over the low-latency pill.
    fast_hot: bool,
    /// What the scale box's neck marks, and the switch's segment under the pointer.
    marks: Marks,
    marks_hot: Option<usize>,
    /// How big the whole widget is (config "note_widget_size"), the size being dragged to, and the grip hovered.
    zoom: f32,
    zoom_drag: Option<f32>,
    /// When the input was last above the noise (P27.1: it repaints at 20 Hz for a while after).
    loud_at: Option<Instant>,
    grip_hot: bool,
    heat: HeatStrip,
    last_listen: Option<Instant>,
}

impl Default for NoteWidget {
    fn default() -> Self {
        NoteWidget {
            key: KeyGuess::default(),
            hist: VecDeque::new(),
            midi: None,
            seen: None,
            next: Instant::now(),
            last_step: Instant::now(),
            sp: Spectra::new(),
            buf: vec![0.0; 4096],
            note: Shown::new(),
            deg: Shown::new(),
            status: String::new(),
            closed: false,
            was_on: false,
            start: Instant::now(),
            pos: None,
            bg: Vec::new(),
            hot: (false, 0),
            styled: false,
            show_scale: true,
            scale: None,
            playing: None,
            strings: GUITAR_OPEN_MIDI.to_vec(),
            tuning: "E Standard".into(),
            fitted_w: None,
            big: false,
            small_pos: None,
            more: false,
            chords: ChordStrip::default(),
            zoom_read: false,
            dragging: false,
            key_pick: None,
            fast: None,
            fast_hot: false,
            marks: Marks::All,
            marks_hot: None,
            zoom: 1.0,
            zoom_drag: None,
            loud_at: None,
            grip_hot: false,
            heat: HeatStrip::default(),
            last_listen: None,
        }
    }
}

fn pos_file(st: &AppState) -> std::path::PathBuf {
    st.data.cache().join("note_widget.json")
}

impl NoteWidget {
    /// Every frame: listen (20 times a second) and show the window while it's on. `song` is the part open in the
    /// player (whose tuning the necks take), the chart time at its now-line and whether it's playing.
    pub fn ui(&mut self, ctx: &egui::Context, st: &mut AppState, song: Option<(&Arrangement, f64, bool)>) {
        let arr = song.map(|s| s.0);
        let on = st.config.bool("note_widget", true);
        if on && !self.was_on {
            self.closed = false; // switched on (again) in Settings
        }
        self.was_on = on;
        if !on || self.closed {
            return;
        }
        self.show_scale = st.config.bool("note_widget_scale", true);
        self.big &= self.show_scale;
        self.more = st.config.bool("note_widget_more", false);
        if !self.zoom_read {
            self.zoom_read = true;
            self.chords.zoom = st.config.bool("note_widget_zoom", true);
            self.chords.from = st
                .config
                .f64("note_widget_from", 0.0)
                .clamp(0.0, 24.0 - chords::SPAN as f64) as f32;
            self.chords.set_all(st.config.bool("note_widget_shapes", false));
            self.zoom = (st.config.f64("note_widget_size", 1.0) as f32).clamp(ZOOM.0, ZOOM.1);
            self.key_pick = key_from_config(&st.config.str("note_widget_chord_key", "auto"));
        }
        self.set_tuning(arr);
        let [w, h] = self.size();
        let now = Instant::now();
        if now >= self.next {
            self.next = now + EVERY;
            self.listen(st, now, song);
        }
        // 20 times a second only while something can change (sound in the last 3 s, a song playing, a glide, a
        // drag); else a look 4 times a second for sound. It redrew the whole app ~38 times a second idle (14% of a
        // core): P27.1
        let busy = self.loud_at.is_some_and(|t| now.duration_since(t) < QUIET_AFTER)
            || song.is_some_and(|s| s.2)
            || !self.note.settled()
            || !self.deg.settled()
            || self.zoom_drag.is_some()
            || self.dragging;
        ctx.request_repaint_after(if busy { EVERY } else { IDLE_POLL });
        if self.pos.is_none() {
            self.pos = std::fs::read_to_string(pos_file(st))
                .ok()
                .and_then(|t| serde_json::from_str::<[f32; 2]>(&t).ok())
                .map(|[x, y]| pos2(x, y))
                .or_else(default_pos);
        }
        // The window's size and place are the system's points (as the Python widget's, and `pos`, saved). egui's
        // builder takes its own points, which the main window's zoom scales, so they go in divided by it; then the
        // window keeps its size and place when the zoom changes (it's 1 on the first frame, before the app sets it).
        let z = ctx.zoom_factor();
        let mut builder = ViewportBuilder::default()
            .with_title("quickRS notes")
            .with_inner_size([w / z, h / z])
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top()
            // (opened without taking the focus: it took it from the main window at every start, and the first
            // click there only gave it back, doing nothing: owner report, 2026-11-03)
            .with_active(false)
            .with_taskbar(false);
        if let Some(p) = self.pos {
            builder = builder.with_position(pos2(p.x / z, p.y / z));
        }
        let mut save_pos = None;
        let mut reset = false;
        let mut close = false;
        let mut toggle_scale = false;
        let mut toggle_big = false;
        let mut toggle_more = false;
        let mut toggle_fast = false;
        let mut pick_marks: Option<Marks> = None;
        let mut save_zoom = false;
        self.marks = Marks::from_config(&st.config.str("note_widget_scale_marks", "all"));
        self.fast = match &st.audio {
            Some(a) if cfg!(windows) && st.audio_mode != AudioMode::Silent => Some(a.prefs().exclusive),
            _ => None,
        };
        let mut toggle_zoom = false;
        let mut save_from = false;
        let mut toggle_shapes = false;
        let mut picked: Option<Option<Key>> = None;
        let shown = self.show_scale;
        let more = self.more;
        let id = ViewportId::from_hash_of("quickrs-note-widget");
        if !self.styled {
            self.styled = style_window(); // (once the window exists)
        }
        ctx.show_viewport_immediate(id, builder, |ui, _class| {
            let wctx = ui.ctx().clone();
            // where the window is, in the system's points (egui's points times the zoom)
            let z = wctx.zoom_factor();
            let place = || {
                wctx.input(|i| i.viewport().outer_rect)
                    .map(|r| pos2(r.min.x * z, r.min.y * z))
            };
            // opened or grown by the scale box: slide it left if it would now run off its screen (the builder
            // moves it next frame)
            if self.fitted_w != Some(w)
                && let Some(at) = place()
            {
                self.fitted_w = Some(w);
                let p = keep_on_screen(at, w, wctx.pixels_per_point() / z);
                if p != at {
                    save_pos = Some(p);
                }
            }
            let area = wctx.content_rect();
            // the drawer's controls, in the widget's points and on the screen
            let v = self.view();
            let u = area.width() / v.width();
            let screen =
                |r: Rect| Rect::from_min_max(area.min + (r.min - v.min) * u, area.min + (r.max - v.min) * u);
            let wide = width(self.show_scale, self.more);
            let hover = wctx.input(|i| i.pointer.hover_pos());
            let over = |r: Rect| hover.is_some_and(|h| screen(r).contains(h));
            let (arrow_on, drawer_on) = (!self.big, self.more && !self.big);
            let fast_on = !self.big && self.fast.is_some();
            self.fast_hot = fast_on && over(fast_rect());
            self.grip_hot = self.zoom_drag.is_some() || over(self.grip());
            let marks_on = self.show_scale;
            self.marks_hot = (0..3).find(|&i| marks_on && over(marks_rect(i)));
            self.hot = (
                arrow_on && over(chords::arrow_rect(wide)),
                if drawer_on && over(chords::zoom_rect(wide)) {
                    1
                } else if drawer_on && over(chords::shapes_rect(wide)) {
                    3
                } else if drawer_on && over(chords::key_rect(wide)) {
                    4
                } else if drawer_on && self.chords.zoom && (self.dragging || over(chords::track_rect(wide))) {
                    2
                } else {
                    0
                },
            );
            self.draw(ui, area);
            let resp = ui.interact(area, egui::Id::new("note-widget-drag"), Sense::click_and_drag());
            // (made after the window's own, so they're on top of it)
            // the grip: drag it to make the whole widget bigger or smaller (its top left stays put)
            let g = ui.interact(
                screen(self.grip()),
                egui::Id::new("note-widget-grip"),
                Sense::drag(),
            );
            if g.hovered() || g.dragged() {
                wctx.set_cursor_icon(CursorIcon::ResizeNwSe);
            }
            if g.dragged() {
                let d = g.drag_delta();
                let z = self.zoom_drag.unwrap_or(self.zoom);
                let z = (z * (1.0 + (d.x / area.width() + d.y / area.height()) / 2.0)).clamp(ZOOM.0, ZOOM.1);
                self.zoom_drag = Some(z);
                self.zoom = (z * 50.0).round() / 50.0; // (in 2% steps: the background is drawn again at each)
            }
            if g.drag_stopped() {
                self.zoom_drag = None;
                save_zoom = true;
            }
            if marks_on {
                for (i, m) in Marks::ALL.into_iter().enumerate() {
                    let r = ui.interact(
                        screen(marks_rect(i)),
                        egui::Id::new(("note-widget-marks", i)),
                        Sense::click(),
                    );
                    if r.clicked() {
                        pick_marks = Some(m);
                    }
                    if r.hovered() {
                        wctx.set_cursor_icon(CursorIcon::PointingHand);
                    }
                    r.on_hover_text(m.tip());
                }
            }
            if fast_on {
                let f = ui.interact(
                    screen(fast_rect()),
                    egui::Id::new("note-widget-fast"),
                    Sense::click(),
                );
                if f.clicked() {
                    toggle_fast = true;
                }
                if f.hovered() {
                    wctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                f.on_hover_text(if self.fast == Some(true) {
                    "Low latency: other apps go quiet while quickRS plays. Click for normal."
                } else {
                    "Normal: other apps play too. Click for low latency (other apps go quiet)."
                });
            }
            if arrow_on {
                let a = ui.interact(
                    screen(chords::arrow_rect(wide)),
                    egui::Id::new("note-widget-more"),
                    Sense::click(),
                );
                if a.clicked() {
                    toggle_more = true;
                }
                if a.hovered() {
                    wctx.set_cursor_icon(CursorIcon::PointingHand);
                }
            }
            if drawer_on {
                let z = ui.interact(
                    screen(chords::zoom_rect(wide)),
                    egui::Id::new("note-widget-zoom"),
                    Sense::click(),
                );
                if z.clicked() {
                    toggle_zoom = true;
                }
                if z.hovered() {
                    wctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                let a = ui.interact(
                    screen(chords::shapes_rect(wide)),
                    egui::Id::new("note-widget-shapes"),
                    Sense::click(),
                );
                if a.clicked() {
                    toggle_shapes = true;
                }
                let k = ui.interact(
                    screen(chords::key_rect(wide)),
                    egui::Id::new("note-widget-key"),
                    Sense::click(),
                );
                if a.hovered() || k.hovered() {
                    wctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                // the KEY menu: the guess, or any of the 24 keys (majors left, minors right)
                let pick = self.key_pick;
                egui::Popup::menu(&k).show(|ui| {
                    let label = match self.key.key {
                        Some(key) => format!("Auto: follow my playing ({})", chords::key_name(key)),
                        None => "Auto: follow my playing".into(),
                    };
                    if ui.selectable_label(pick.is_none(), label).clicked() {
                        picked = Some(None);
                        ui.close();
                    }
                    ui.separator();
                    egui::Grid::new("note-widget-keys").show(ui, |ui| {
                        for t in [0u8, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10, 5] {
                            for key in [(t, Mode::Major), ((t + 9) % 12, Mode::Minor)] {
                                if ui
                                    .selectable_label(pick == Some(key), chords::key_name(key))
                                    .clicked()
                                {
                                    picked = Some(Some(key));
                                    ui.close();
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
                if self.chords.zoom {
                    let tr = chords::track_rect(wide);
                    let t = ui.interact(
                        screen(tr),
                        egui::Id::new("note-widget-track"),
                        Sense::click_and_drag(),
                    );
                    if t.clicked()
                        && let Some(h) = t.interact_pointer_pos()
                    {
                        // a click on the track: the thumb's middle goes there
                        let at = v.min.x + (h.x - area.min.x) / u;
                        let th = self.chords.thumb(tr);
                        self.chords.drag(at - th.center().x, tr.width());
                        self.chords.settle();
                        save_from = true;
                    }
                    if t.dragged() {
                        self.dragging = true;
                        self.chords.drag(t.drag_delta().x / u, tr.width());
                    }
                    if t.drag_stopped() {
                        self.dragging = false;
                        self.chords.settle();
                        save_from = true;
                    }
                    if t.hovered() || t.dragged() {
                        wctx.set_cursor_icon(CursorIcon::Grab);
                    }
                }
            }
            if resp.drag_started() {
                wctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            // the scale box (or, popped up, anywhere) under the pointer: where it is in the widget's points
            let on_scale = |pos: Option<Pos2>| {
                pos.is_some_and(|p| {
                    let v = self.view();
                    let at = v.min + (p - area.min) * (v.width() / area.width());
                    self.big || (self.show_scale && box_rect(2).contains(at))
                })
            };
            if resp.clicked() && on_scale(resp.interact_pointer_pos()) {
                toggle_big = true;
            }
            if resp.hovered() {
                let icon = if on_scale(resp.hover_pos()) {
                    CursorIcon::PointingHand
                } else {
                    CursorIcon::Grab
                };
                wctx.set_cursor_icon(icon);
            }
            if resp.drag_stopped() || wctx.input(|i| i.pointer.primary_released()) {
                save_pos = place();
            }
            resp.context_menu(|ui| {
                if ui.button("Forget the key and the trails (start again)").clicked() {
                    reset = true;
                    ui.close();
                }
                let label = if shown {
                    "Hide the scale"
                } else {
                    "Show the scale on the neck"
                };
                if ui.button(label).clicked() {
                    toggle_scale = true;
                    ui.close();
                }
                let label = if more {
                    "Close the drawer"
                } else {
                    "Open the drawer (chords)"
                };
                if ui.button(label).clicked() {
                    toggle_more = true;
                    ui.close();
                }
                if ui.button("Close").clicked() {
                    close = true;
                    ui.close();
                }
            });
            if wctx.input(|i| i.viewport().close_requested()) {
                close = true;
            }
        });
        if reset {
            self.key.reset();
            self.chords.reset();
            self.heat.reset();
        }
        if toggle_shapes {
            self.chords.set_all(!self.chords.all);
            st.config.set("note_widget_shapes", self.chords.all);
            st.save_config();
        }
        if let Some(k) = picked {
            self.key_pick = k;
            st.config.set("note_widget_chord_key", key_to_config(k));
            st.save_config();
        }
        if toggle_more {
            st.config.set("note_widget_more", !more);
            st.save_config();
        }
        // the low-latency switch, as Settings > Audio's (`audio_act` "exclusive"): the route re-planned, saved
        if save_zoom {
            st.config.set("note_widget_size", self.zoom as f64);
            st.save_config();
        }
        if let Some(m) = pick_marks
            && m != self.marks
        {
            self.marks = m;
            st.config.set("note_widget_scale_marks", m.config());
            st.save_config();
        }
        if toggle_fast && let Some(audio) = &st.audio {
            let mut prefs = audio.prefs();
            prefs.exclusive = !prefs.exclusive;
            audio.set_prefs(prefs.clone());
            self.fast = Some(prefs.exclusive);
            st.config
                .set("audio", serde_json::to_value(&prefs).unwrap_or_default());
            st.save_config();
        }
        if toggle_zoom {
            self.chords.zoom = !self.chords.zoom;
            st.config.set("note_widget_zoom", self.chords.zoom);
            st.save_config();
        }
        if save_from {
            st.config.set("note_widget_from", self.chords.from as f64);
            st.save_config();
        }
        if toggle_scale {
            st.config.set("note_widget_scale", !shown);
            st.save_config();
        }
        if toggle_big {
            self.big = !self.big;
            if self.big {
                self.small_pos = self.pos;
            } else if let Some(p) = self.small_pos.take() {
                save_pos = Some(p); // back where it was before it popped up
            }
        }
        if close {
            self.closed = true;
        }
        if let Some(p) = save_pos
            && Some(p) != self.pos
        {
            self.pos = Some(p);
            let f = pos_file(st);
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(f, format!("[{}, {}]", p.x, p.y));
        }
    }

    /// Screenshot mode: the widget as it looks with Eb3 a few cents sharp, in Eb major (the scale box on a guitar in
    /// E standard), drawn in the main window with its right edge at `rect`'s. The Python look (D21; the parity
    /// shots) leaves the scale box out, as the Python widget has none. With `more`, the drawer is out, naming the
    /// Am7 it hears from the notes of x02010.
    pub fn preview(&mut self, ui: &egui::Ui, rect: Rect, more: bool) {
        self.show_scale = !qrs_ui::theme::classic();
        self.more = more;
        self.fast = more.then_some(true); // (shown on the drawer's shot only)
        self.marks = if more { Marks::Roots } else { Marks::All };
        if more {
            self.chords.set_all(true);
            self.chords
                .stage(&[45, 52, 55, 60, 64], &["G", "D", "C", "Am7"], &self.strings);
            self.chords.key = Some((7, Mode::Major));
            self.chords.key_auto = true;
            self.heat.stage(&self.strings);
        }
        self.status = "listening".into();
        self.note.set(Some(note_color(3)), "Eb", "Eb3  +3¢");
        self.deg.set(Some(degree_color("1")), "1", "Eb major");
        self.scale = Some((3, Mode::Major));
        self.playing = Some(51);
        let w = width(self.show_scale, more);
        let r = Rect::from_min_size(pos2(rect.max.x - w, rect.min.y), vec2(w, height(more)));
        let p = ui.painter();
        p.rect_filled(
            r.translate(vec2(0.0, 10.0)).expand(6.0),
            18.0,
            Color32::from_black_alpha(60),
        );
        self.draw(ui, r);
    }

    /// The part of the widget the window shows (its points): all of it, or popped up, the scale box and its margin.
    fn view(&self) -> Rect {
        if self.big {
            let b = box_rect(2);
            Rect::from_min_max(b.min - vec2(PAD, PAD), pos2(b.max.x + PAD, H))
        } else {
            Rect::from_min_size(
                Pos2::ZERO,
                vec2(width(self.show_scale, self.more), height(self.more)),
            )
        }
    }

    /// The resize grip: the bottom right corner of the view.
    fn grip(&self) -> Rect {
        let v = self.view();
        Rect::from_min_max(v.max - vec2(20.0, 20.0), v.max)
    }

    /// The window's size (the system's points): the view at the widget's size, BIG times over when popped up.
    fn size(&self) -> [f32; 2] {
        let v = self.view().size() * self.zoom * if self.big { BIG } else { 1.0 };
        [v.x, v.y]
    }

    /// The neck's strings: the open part's tuning (a bass part's four strings), else a guitar in E standard.
    fn set_tuning(&mut self, arr: Option<&Arrangement>) {
        let strings = arr.map_or_else(|| GUITAR_OPEN_MIDI.to_vec(), |a| a.open_string_midi());
        if strings != self.strings {
            self.tuning = arr.map_or_else(|| "E Standard".into(), |a| a.tuning_name());
            self.strings = strings;
        }
    }

    /// The note you're playing (from the guitar input) and the key it's in (a chord's notes count too); the heat
    /// maps' notes.
    fn listen(&mut self, st: &AppState, now: Instant, song: Option<(&Arrangement, f64, bool)>) {
        let dt = self
            .last_listen
            .map_or(0.0, |l| now.duration_since(l).as_secs_f64().min(0.5));
        self.last_listen = Some(now);
        self.heat.song(song, dt, self.strings.len());
        let mut level = 0.0;
        self.status = match &st.audio {
            None => "audio engine off".into(),
            Some(a) if a.engine.status().input.is_none() => "no guitar input (F2)".into(),
            Some(a) => {
                let e = &a.engine;
                let rate = e.capture().rate();
                level = e.meters().level;
                if level >= 0.002 {
                    self.loud_at = Some(now);
                }
                if self.more {
                    let t = now.duration_since(self.start).as_secs_f64();
                    let cap = e.capture();
                    self.chords
                        .listen(|b| cap.latest(b).is_some(), rate, &self.strings, t);
                }
                if e.meters().level >= 0.002 && e.capture().latest(&mut self.buf).is_some() {
                    let (hz, clarity) = qrs_analysis::pitch::pitch(&mut self.sp, &self.buf, rate);
                    if hz > 0.0 && clarity >= 0.85 {
                        let m = 69.0 + 12.0 * (hz / 440.0).log2();
                        if self.midi.is_some_and(|x| (m - x).abs() > 0.6) {
                            self.hist.clear(); // a new note: don't let the median drag behind it
                        }
                        self.hist.push_back(m);
                        while self.hist.len() > 5 {
                            self.hist.pop_front();
                        }
                        let mut v: Vec<f64> = self.hist.iter().copied().collect();
                        v.sort_by(|a, b| a.total_cmp(b));
                        self.midi = Some(v[v.len() / 2]);
                        self.seen = Some(now);
                    }
                }
                "listening".into()
            }
        };
        let since = self
            .seen
            .map(|s| now.duration_since(s).as_secs_f32())
            .unwrap_or(f32::MAX);
        let held = self.midi.filter(|_| since < HOLD);
        let pc = held
            .filter(|_| since < 0.15)
            .map(|m| (m.round() as i64).rem_euclid(12) as u8);
        let t = now.duration_since(self.start).as_secs_f64();
        let tones = if self.more {
            self.chords.ringing_tones()
        } else {
            Vec::new()
        };
        if pc.is_none() && tones.len() >= 2 {
            self.key.add_all(&tones, t);
        } else {
            self.key.add(pc, t);
        }
        self.scale = self.key.key;
        self.playing = held.map(|m| m.round() as i32);
        self.chords.key = self.key_pick.or(self.key.key);
        self.chords.key_auto = self.key_pick.is_none();
        let sounding = held.filter(|_| since < 0.15).map(|m| m.round() as i32);
        let chord = if self.more {
            self.chords.ringing_shape()
        } else {
            None
        };
        self.heat
            .listen(t, sounding, self.playing, chord, level, &self.strings);
        // what the boxes show
        match held {
            None => self.note.set(None, "—", "play a note"),
            Some(midi) => {
                let m = midi.round() as i64;
                let cents = ((midi - m as f64) * 100.0).round() as i64;
                let name = NOTE_NAMES[m.rem_euclid(12) as usize];
                self.note.set(
                    Some(note_color(m.rem_euclid(12) as usize)),
                    name,
                    &format!("{name}{}  {cents:+}¢", m.div_euclid(12) - 1),
                );
            }
        }
        match self.key.key {
            None => self.deg.set(None, "?", "listening for a key"),
            Some((tonic, mode)) => {
                let name = format!(
                    "{} {}",
                    NOTE_NAMES[tonic as usize % 12],
                    if mode == Mode::Major { "major" } else { "minor" }
                );
                match held {
                    None => {
                        let c = mix(BASE, degree_color("1"), 0.25);
                        self.deg.set(Some(c), NOTE_NAMES[tonic as usize % 12], &name);
                    }
                    Some(midi) => {
                        let rel = (midi.round() as i64 - tonic as i64).rem_euclid(12) as usize;
                        let (degs, inside) = if mode == Mode::Major {
                            (&DEG_MAJOR, IN_MAJOR.contains(&rel))
                        } else {
                            (&DEG_MINOR, IN_MINOR.contains(&rel))
                        };
                        let d = degs[rel];
                        let c = if inside { degree_color(d) } else { OUT_OF_KEY };
                        let small = if inside {
                            name
                        } else {
                            format!("{name} \u{b7} outside")
                        };
                        self.deg.set(Some(c), d, &small);
                    }
                }
            }
        }
    }

    /// The panel, its two sunken boxes with their captions, and each disc raised (live) or sunk in (empty); then
    /// the discs' colours and the text (notewidget.py `background` and `compose`). The widget keeps the Python
    /// widget's own colours (the app's first material, graphite) whatever the app's theme.
    fn draw(&mut self, ui: &egui::Ui, area: Rect) {
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = now.duration_since(self.last_step).as_secs_f32().min(0.2);
        self.last_step = now;
        self.note.step(dt);
        self.deg.step(dt);
        // `u` points a widget pixel: the window is W x H of the system's points, but egui's zoom (the main
        // window's scale) applies to every window, so its content area is W x H / zoom points
        let w = width(self.show_scale, self.more);
        let h = height(self.more);
        let view = self.view();
        let u = area.width() / view.width();
        let live = (self.note.target.is_some(), self.deg.target.is_some());
        let k = u * ctx.pixels_per_point(); // screen pixels a widget pixel
        // (k rounded: it's the same scale when the main window's zoom changes, give or take a float's last digits,
        // and redrawing this background took 45 ms a step of a resize: D70)
        let key = (live, (self.show_scale, self.more), (k * 256.0).round() as u32);
        let have = self.bg.iter().position(|(kk, _)| *kk == key).or_else(|| {
            // dragging the grip: any size of this layout, stretched (P27.3)
            self.zoom_drag
                .and(self.bg.iter().rposition(|(kk, _)| kk.0 == key.0 && kk.1 == key.1))
        });
        let i = match have {
            Some(i) => i,
            None => {
                let img = background(live, self.show_scale, self.more, k);
                let tex = ctx.load_texture("note-widget-bg", img, egui::TextureOptions::LINEAR);
                if self.bg.len() >= BG_KEEP {
                    drop(self.bg.remove(0)); // (the oldest goes, its texture freed)
                }
                self.bg.push((key, tex));
                self.bg.len() - 1
            }
        };
        // (popped up, the window is the scale box's part of the widget: everything else falls outside it)
        let o = area.min - view.min.to_vec2() * u;
        if let Some((_, tex)) = self.bg.get(i) {
            p.image(
                tex.id(),
                Rect::from_min_size(o, vec2(w, h) * u),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let at = |x: f32, y: f32| pos2(o.x + x * u, o.y + y * u);
        let st = |size: f32, weight: u16, color: Color32| Style::new(size * u, weight, color);
        if !self.big {
            pil_text(
                &p,
                "quickRS",
                at(PAD + 2.0, PAD + 2.0),
                Align2::LEFT_TOP,
                st(14.0, 800, FG),
            );
            chords::draw_arrow(&p, u, at, w, self.more, self.hot.0);
            // the grip's three lines in the bottom right corner
            let gr = self.grip();
            let col = if self.grip_hot { FG } else { DIM };
            for k in [4.0, 8.0, 12.0] {
                p.line_segment(
                    [
                        at(gr.max.x - 5.0 - k, gr.max.y - 5.0),
                        at(gr.max.x - 5.0, gr.max.y - 5.0 - k),
                    ],
                    Stroke::new(1.3 * u, col),
                );
            }
            if let Some(on) = self.fast {
                let r = fast_rect();
                let pr = Rect::from_min_max(at(r.min.x, r.min.y), at(r.max.x, r.max.y));
                let (fill, text, edge) = match (on, self.fast_hot) {
                    (true, hot) => (
                        if hot { mix(FAST, FG, 0.25) } else { FAST },
                        Color32::from_rgb(6, 34, 20),
                        FAST,
                    ),
                    (false, true) => (HILITE, FG, DIM),
                    (false, false) => (BASE, DIM, FRET),
                };
                p.rect_filled(pr, 8.5 * u, fill);
                p.rect_stroke(pr, 8.5 * u, Stroke::new(u, edge), egui::StrokeKind::Inside);
                let label = if on { "FAST" } else { "NORMAL" };
                pil_text(&p, label, pr.center(), Align2::CENTER_CENTER, st(9.0, 800, text));
            }
            pil_text(
                &p,
                &self.status,
                at(w - PAD - 26.0, PAD + 4.0),
                Align2::RIGHT_TOP,
                st(11.0, 500, DIM),
            );
        }
        // a box's caption: small caps, letter-spaced by 1.2 px
        let caption = |s: &str, bx: Rect| {
            let mut x = bx.min.x + 12.0;
            for ch in s.chars() {
                let cs = st(9.0, 800, DIM);
                pil_text(&p, &ch.to_string(), at(x, bx.min.y + 10.0), Align2::LEFT_TOP, cs);
                let adv = CAPTION_ADV.iter().find(|(c, _)| *c == ch).map(|a| a.1);
                x += adv.unwrap_or_else(|| text::width(&ctx, &ch.to_string(), cs) / u) + 1.2;
            }
        };
        let boxes = [("NOTE", &self.note), ("KEY", &self.deg)];
        let n = if self.big { 0 } else { boxes.len() };
        for (i, &(name, b)) in boxes[..n].iter().enumerate() {
            let bx = box_rect(i);
            caption(name, bx);
            let (c, _) = disc_box(bx);
            let main = match b.fill {
                Some(fill) if b.target.is_some() => {
                    p.circle_filled(at(c.x, c.y), DISC / 2.0 * u, fill);
                    ink(fill)
                }
                _ => DIM,
            };
            let size = if b.big.chars().count() <= 2 { 30.0 } else { 22.0 };
            pil_text(
                &p,
                &b.big,
                at(c.x, c.y + 1.0),
                Align2::CENTER_CENTER,
                st(size, 800, main),
            );
            pil_text(
                &p,
                &b.small,
                at((bx.min.x + bx.max.x) / 2.0, bx.max.y - 18.0),
                Align2::CENTER_CENTER,
                st(11.0, 500, SUB),
            );
        }
        if self.show_scale {
            caption("SCALE", box_rect(2));
            // ALL / ROOT / OFF: one rounded strip, the picked segment lit
            let whole = Rect::from_min_max(marks_rect(0).min, marks_rect(2).max);
            let wr = Rect::from_min_max(at(whole.min.x, whole.min.y), at(whole.max.x, whole.max.y));
            p.rect_filled(wr, 7.5 * u, SHADE);
            for (i, m) in Marks::ALL.into_iter().enumerate() {
                let r = marks_rect(i).shrink(1.5);
                let sr = Rect::from_min_max(at(r.min.x, r.min.y), at(r.max.x, r.max.y));
                let on = m == self.marks;
                if on {
                    p.rect_filled(sr, 6.0 * u, degree_color("1"));
                } else if self.marks_hot == Some(i) {
                    p.rect_filled(sr, 6.0 * u, HILITE);
                }
                let col = if on {
                    ink(degree_color("1"))
                } else if self.marks_hot == Some(i) {
                    FG
                } else {
                    DIM
                };
                pil_text(
                    &p,
                    m.label(),
                    sr.center(),
                    Align2::CENTER_CENTER,
                    st(8.0, 800, col),
                );
            }
            self.draw_scale(&p, u, at);
        }
        if self.more && !self.big {
            caption("CHORD", chords::strip_rect(w));
            self.chords.draw(&p, u, at, w, &self.strings, self.hot.1);
            caption("HEAT", heat::strip_rect(w));
            self.heat.draw(&p, u, at, w, &self.strings);
        }
    }

    /// The scale box over its sunken board (`background`): the key's name and the tuning, then the neck as the
    /// player's neck tile lays it out (nut to fret 24, low string at the bottom), each note of the scale a dot in
    /// its degree's colour with the degree on it, and the note being played ringed wherever it lies on the neck
    /// (on a grey dot outside the scale; in its note's colour before there's a key). `at` maps widget points to
    /// the screen, `u` points a widget point.
    fn draw_scale(&self, p: &egui::Painter, u: f32, at: impl Fn(f32, f32) -> Pos2) {
        let bx = box_rect(2);
        let board = neck_board();
        let label = match self.scale {
            Some((t, m)) => format!(
                "{} {}  \u{b7}  {}",
                NOTE_NAMES[t as usize % 12],
                if m == Mode::Major { "major" } else { "minor" },
                self.tuning
            ),
            None => format!("listening for a key  \u{b7}  {}", self.tuning),
        };
        let sub = if self.scale.is_some() { SUB } else { DIM };
        pil_text(
            p,
            &label,
            at(bx.max.x - 14.0, bx.min.y + 8.0),
            Align2::RIGHT_TOP,
            Style::new(11.0 * u, 500, sub),
        );
        let n = self.strings.len().max(1);
        let nut_w = NUT_W;
        let x0 = board.min.x + nut_w;
        let fw = (board.width() - nut_w - 2.0) / 24.0;
        let m_ = 8.0;
        let gap = (board.height() - 2.0 * m_) / (n - 1).max(1) as f32;
        let sy = |s: usize| board.min.y + m_ + (n - 1 - s) as f32 * gap;
        let fx = |f: i32| {
            if f == 0 {
                board.min.x + nut_w / 2.0
            } else {
                x0 + (f as f32 - 0.5) * fw
            }
        };
        let mid = (board.min.y + board.max.y) / 2.0;
        for f in FRET_NUMBERS {
            let dots: &[f32] = if f % 12 == 0 { &[-1.0, 1.0] } else { &[0.0] };
            for d in dots {
                p.circle_filled(at(fx(f), mid + d * gap), 2.2 * u, INLAY);
            }
        }
        for f in 1..=24 {
            let x = x0 + f as f32 * fw;
            p.line_segment(
                [at(x, board.min.y + 3.0), at(x, board.max.y - 3.0)],
                Stroke::new(u, FRET),
            );
        }
        p.rect_filled(
            Rect::from_min_max(at(x0 - 3.0, board.min.y + 3.0), at(x0, board.max.y - 3.0)),
            u,
            NUT,
        );
        for s in 0..n {
            let thick = [1.6, 1.4, 1.2, 1.0, 0.9, 0.8][s.min(5)];
            p.line_segment(
                [at(x0, sy(s)), at(board.max.x - 3.0, sy(s))],
                Stroke::new(thick * u, STRING),
            );
        }
        for f in FRET_NUMBERS {
            pil_text(
                p,
                &f.to_string(),
                at(fx(f), board.max.y + 3.0),
                Align2::CENTER_TOP,
                Style::new(9.0 * u, 700, DIM),
            );
        }
        let spots = || {
            self.strings
                .iter()
                .enumerate()
                .flat_map(|(s, &open)| (0..=24).map(move |f| (s, f, open + f)))
        };
        for (s, f, midi) in spots() {
            let deg = self.scale.and_then(|key| scale_degree(key, midi));
            let playing = self.playing == Some(midi);
            // OFF: only the note played, in its note's own colour (as the NOTE disc) with its name: nothing of the
            // key (owner request, 2026-11-02)
            let off = self.marks == Marks::Off;
            let deg = if off { None } else { deg };
            let fill = match (deg, self.marks) {
                (Some("1"), _) | (Some(_), Marks::All) => degree_color(deg.unwrap_or("1")),
                (Some(_), _) => GREY_NOTE,
                (None, Marks::Off) if playing => note_color(midi.rem_euclid(12) as usize),
                (None, _) if playing && self.scale.is_some() => OUT_OF_KEY,
                (None, _) if playing => note_color(midi.rem_euclid(12) as usize),
                (None, _) => continue,
            };
            let c = at(fx(f), sy(s));
            p.circle_filled(c, SPOT * u, fill);
            let label = if off {
                Some(NOTE_NAMES[midi.rem_euclid(12) as usize])
            } else {
                deg
            };
            if let Some(d) = label {
                // (a two-letter name a little smaller, to fit the dot)
                let size = if d.chars().count() > 1 { 6.5 } else { 8.0 };
                pil_text(
                    p,
                    d,
                    c,
                    Align2::CENTER_CENTER,
                    Style::new(size * u, 800, ink(fill)),
                );
            }
        }
        // (the rings last, over any neighbour's dot)
        for (s, f, _) in spots().filter(|&(_, _, midi)| self.playing == Some(midi)) {
            p.circle_stroke(at(fx(f), sy(s)), (SPOT + 2.0) * u, Stroke::new(1.6 * u, FG));
        }
    }
}

/// Where the widget first opens, as the Python one: 300 points left of the screen's right edge, 40 from the top.
#[cfg(windows)]
fn default_pos() -> Option<Pos2> {
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};
    unsafe {
        let (w, dpi) = (GetSystemMetrics(SM_CXSCREEN), GetDpiForSystem());
        (w > 0 && dpi > 0).then(|| pos2(w as f32 / (dpi as f32 / 96.0) - 300.0, 40.0))
    }
}

#[cfg(not(windows))]
fn default_pos() -> Option<Pos2> {
    None
}

/// Where a window `w` points wide at `at` goes so it stays on its screen: moved left (12 points in from the
/// edge) if it would run past the right of the screen's work area, never past its left; else where it is.
/// `at` and `w` are the system's points, `ppp` the screen's pixels a system point.
#[cfg(windows)]
fn keep_on_screen(at: Pos2, w: f32, ppp: f32) -> Pos2 {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    unsafe {
        let pt = POINT {
            x: (at.x * ppp).round() as i32,
            y: (at.y * ppp).round() as i32,
        };
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            return at;
        }
        let (left, right) = (mi.rcWork.left as f32 / ppp, mi.rcWork.right as f32 / ppp);
        if at.x + w > right {
            pos2((right - w - 12.0).max(left), at.y)
        } else {
            at
        }
    }
}

#[cfg(not(windows))]
fn keep_on_screen(at: Pos2, _w: f32, _ppp: f32) -> Pos2 {
    at
}

/// Windows 11: the system's rounded corners and a border in the shadow colour, as the Python widget asks for
/// (`round_corners`), and the window 94% opaque (its `-alpha 0.94`). The window is found by its title among this
/// process's windows; false until it exists.
#[cfg(windows)]
fn style_window() -> bool {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GWL_EXSTYLE, GetWindowLongPtrW, GetWindowThreadProcessId, LWA_ALPHA,
        SetLayeredWindowAttributes, SetWindowLongPtrW, WS_EX_LAYERED,
    };
    use windows::core::w;
    unsafe {
        let mut after: Option<HWND> = None;
        loop {
            let Ok(hwnd) = FindWindowExW(None, after, None, w!("quickRS notes")) else {
                return false;
            };
            if hwnd.is_invalid() {
                return false;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == GetCurrentProcessId() {
                // 33: corner preference, 2 = round; 34: border colour, the shadow #1c1e21 as 0x00BBGGRR
                for (attr, value) in [(33, 2u32), (34, 0x0021_1E1C)] {
                    let _ = DwmSetWindowAttribute(
                        hwnd,
                        DWMWINDOWATTRIBUTE(attr),
                        &value as *const u32 as *const _,
                        4,
                    );
                }
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
                let _ =
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), (0.94f32 * 255.0).round() as u8, LWA_ALPHA);
                return true;
            }
            after = Some(hwnd);
        }
    }
}

#[cfg(not(windows))]
fn style_window() -> bool {
    true
}

// -- drawing (notewidget.py's Pillow drawing, at the window's pixel scale) -----------------------------------
/// Pillow's advances (hinted, whole pixels) of the caption letters, Manrope ExtraBold at 9 px: the Python widget
/// spaces NOTE and KEY by these, plus 1.2 px.
const CAPTION_ADV: [(char, f32); 6] = [
    ('N', 7.0),
    ('O', 8.0),
    ('T', 7.0),
    ('E', 6.0),
    ('K', 6.0),
    ('Y', 5.0),
];

/// Text where Pillow's `ImageDraw.text` puts it with anchor "la", "ra" (the ascender line at y) or "mm" (halfway
/// between the ascender and descender lines): Pillow's ascent is ceil(1.066 em), its descent ceil(0.3 em).
fn pil_text(p: &egui::Painter, s: &str, pos: Pos2, anchor: Align2, st: Style) {
    let ppp = p.pixels_per_point();
    let px = (text::quantize(st.size, ppp) * ppp).round();
    let (asc, desc) = ((px * 1.066 - 1e-4).ceil(), (px * 0.3 - 1e-4).ceil());
    let y = (pos.y * ppp).round();
    let base = match anchor.y() {
        Align::Center => y + asc - ((asc + desc) / 2.0).floor(),
        _ => y + asc,
    };
    // (text::draw puts its box's top at y and the baseline at y + the same ascent)
    text::draw(
        p,
        s,
        pos2(pos.x, (base - asc) / ppp),
        Align2([anchor.x(), Align::Min]),
        st,
        None,
    );
}

// The widget's own colours, as the Python widget's: the app's first material (graphite) and its two shadows.
const BASE: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x32);
const HILITE: Color32 = Color32::from_rgb(0x34, 0x38, 0x3e);
const SHADE: Color32 = Color32::from_rgb(0x1c, 0x1e, 0x21);
const FG: Color32 = Color32::from_rgb(0xe8, 0xea, 0xed);
const SUB: Color32 = Color32::from_rgb(0xb4, 0xb9, 0xc2);
const DIM: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xaa);
// The scale box's neck in the same greys: frets, strings, the nut and the inlays on its dark board.
const FRET: Color32 = Color32::from_rgb(0x4a, 0x4f, 0x57);
const STRING: Color32 = Color32::from_rgb(0x8a, 0x90, 0x9a);
const NUT: Color32 = Color32::from_rgb(0xc9, 0xcc, 0xd1);
/// (plain white, to be seen at a glance: owner request, 2026-11-02)
const INLAY: Color32 = Color32::from_rgb(0xf4, 0xf5, 0xf7);

/// The NOTE (0), KEY (1) and SCALE (2) boxes, in the window's points.
fn box_rect(i: usize) -> Rect {
    Rect::from_min_size(
        pos2(PAD + i as f32 * (BOX_W + GAP), PAD + 30.0),
        vec2(if i == 2 { NECK_W } else { BOX_W }, BOX_H),
    )
}

/// The neck's board in the scale box: under the caption, over the fret numbers.
fn neck_board() -> Rect {
    let bx = box_rect(2);
    Rect::from_min_max(
        pos2(bx.min.x + 12.0, bx.min.y + 30.0),
        pos2(bx.max.x - 12.0, bx.max.y - 26.0),
    )
}

/// The disc in a box: (centre, rect); its centre on the box's whole-pixel middle, as `disc_box`.
fn disc_box(bx: Rect) -> (Pos2, Rect) {
    let cx = ((bx.min.x + bx.max.x) / 2.0).floor();
    let top = bx.min.y + 28.0;
    let r = Rect::from_min_size(pos2(cx - DISC / 2.0, top), vec2(DISC, DISC));
    (r.center(), r)
}

/// A rounded box (or a circle) as an anti-aliased mask on the `wp` x `hp` canvas, at `k` pixels a point (`_shape`).
fn shape(r: Rect, radius: f32, ellipse: bool, k: f32, wp: usize, hp: usize) -> Mask {
    if ellipse {
        Mask::disc(wp, hp, r.center().x * k, r.center().y * k, r.width() / 2.0 * k)
    } else {
        Mask::rrect(
            r.width() * k,
            r.height() * k,
            radius * k,
            r.min.x * k,
            r.min.y * k,
            wp,
            hp,
        )
    }
}

/// `raised`: the shadow down-right, the light up-left (both the shape blurred by `d`), then the face.
fn raised(l: &mut Layers, s: &Mask, d: f32, k: f32) {
    let blur = s.blur(d * k);
    let o = (d * k).round() as i32;
    l.over_c(SHADE, &blur.shift(o, o));
    l.over_c(HILITE, &blur.shift(-o, -o));
    l.over_c(BASE, s);
}

/// `inset`: the face, then the light and the shadow on the inside edges (the shape's blur, moved, cut out of it).
fn inset(l: &mut Layers, s: &Mask, d: f32, k: f32) {
    let blur = s.blur(d * k);
    let o = (d * k).round() as i32;
    l.over_c(BASE, s);
    l.over_c(HILITE, &blur.shift(-o, -o).map(|v| 1.0 - v).times(s));
    l.over_c(SHADE, &blur.shift(o, o).map(|v| 1.0 - v).times(s));
}

/// The panel, its two sunken boxes and each disc raised (live) or sunk in (empty), and with `scale` the scale box
/// with its board sunk in further and darker, at `k` pixels a point (`background`; the text and the neck go on top
/// each frame).
fn background(live: (bool, bool), scale: bool, more: bool, k: f32) -> egui::ColorImage {
    let w = width(scale, more);
    let h = height(more);
    let (wp, hp) = ((w * k).round() as usize, (h * k).round() as usize);
    let mut l = Layers::new(wp, hp);
    l.over_all(qrs_ui::raster::rgbf(BASE), 1.0);
    // the panel's own edge catches the light on the top left
    let panel = shape(
        Rect::from_min_size(Pos2::ZERO, vec2(w, h)),
        18.0,
        false,
        k,
        wp,
        hp,
    );
    let glow = panel.map(|v| 1.0 - v).blur(4.0 * k);
    let o = (3.0 * k).round() as i32;
    l.over_c(HILITE, &glow.shift(o, o).times(&panel));
    l.over_c(SHADE, &glow.shift(-o, -o).times(&panel));
    for (i, on) in [live.0, live.1].into_iter().enumerate() {
        let bx = box_rect(i);
        inset(&mut l, &shape(bx, 18.0, false, k, wp, hp), 4.0, k);
        let disc = shape(disc_box(bx).1, 0.0, true, k, wp, hp);
        if on {
            raised(&mut l, &disc, 3.0, k);
        } else {
            inset(&mut l, &disc, 3.0, k);
        }
    }
    if scale {
        inset(&mut l, &shape(box_rect(2), 18.0, false, k, wp, hp), 4.0, k);
        let board = shape(neck_board(), 8.0, false, k, wp, hp);
        inset(&mut l, &board, 2.0, k);
        l.over_c(SHADE, &board.map(|v| v * 0.5));
    }
    if more {
        inset(
            &mut l,
            &shape(chords::strip_rect(w), 18.0, false, k, wp, hp),
            4.0,
            k,
        );
        let board = shape(chords::board_rect(w), 8.0, false, k, wp, hp);
        inset(&mut l, &board, 2.0, k);
        l.over_c(SHADE, &board.map(|v| v * 0.5));
        inset(
            &mut l,
            &shape(heat::strip_rect(w), 18.0, false, k, wp, hp),
            4.0,
            k,
        );
        for row in 0..2 {
            let board = shape(heat::board_rect(w, row), 8.0, false, k, wp, hp);
            inset(&mut l, &board, 2.0, k);
            l.over_c(SHADE, &board.map(|v| v * 0.5));
        }
    }
    l.image(None).to_egui()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chord_key_is_kept_in_the_config() {
        for k in [None, Some((9, Mode::Major)), Some((6, Mode::Minor))] {
            assert_eq!(key_from_config(&key_to_config(k)), k);
        }
        assert_eq!(key_from_config("12 major"), None);
        assert_eq!(key_from_config("auto"), None);
    }

    /// P27.1: the widget only keeps repainting while a disc's colour is still gliding.
    #[test]
    fn a_glide_settles() {
        let mut sh = Shown::new();
        assert!(sh.settled(), "nothing to show is settled");
        sh.set(Some(Color32::from_rgb(200, 40, 40)), "C", "");
        assert!(!sh.settled());
        for _ in 0..60 {
            sh.step(0.03);
        }
        assert!(sh.settled(), "{:?}", sh.fill);
        sh.set(None, "-", "");
        sh.step(0.03);
        assert!(sh.settled());
    }

    #[test]
    fn colours_go_round_the_wheel() {
        let c = note_color(0);
        assert!(c.r() > c.g() && c.r() > c.b(), "C is red: {c:?}");
        assert_ne!(note_color(4), note_color(8));
        assert_eq!(degree_color("♭3"), OUT_OF_KEY);
        assert_eq!(ink(Color32::WHITE), Color32::from_rgb(0x15, 0x15, 0x1a));
    }

    #[test]
    fn scale_degrees_on_the_neck() {
        let eb_major = (3, Mode::Major);
        assert_eq!(scale_degree(eb_major, 51), Some("1")); // Eb3
        assert_eq!(scale_degree(eb_major, 55), Some("3")); // G3
        assert_eq!(scale_degree(eb_major, 62), Some("7")); // D4
        assert_eq!(scale_degree(eb_major, 57), None); // A3
        let a_minor = (9, Mode::Minor);
        assert_eq!(scale_degree(a_minor, 60), Some("3")); // C4
        assert_eq!(scale_degree(a_minor, 55), Some("7")); // G3
        assert_eq!(scale_degree(a_minor, 56), None); // G#3
        // seven notes in every octave of every string
        let per_octave = (40..52).filter(|&m| scale_degree(eb_major, m).is_some()).count();
        assert_eq!(per_octave, 7);
    }

    /// Popped up, the window is the scale box and its margin, BIG times over; back, it's the whole widget again.
    #[test]
    fn the_scale_box_pops_up() {
        let mut nw = NoteWidget::default();
        assert_eq!(nw.size(), [width(true, false), H]);
        nw.big = true;
        let v = nw.view();
        assert!(v.contains_rect(box_rect(2)) && !v.contains(box_rect(1).center()));
        assert_eq!(nw.size(), [v.width() * BIG, v.height() * BIG]);
        nw.big = false;
        assert_eq!(nw.view().size(), vec2(width(true, false), H));
        // the drawer: as wide as the scale box's window, taller by the drawer
        nw.more = true;
        assert_eq!(nw.size(), [width(true, false), H + DRAWER_H + HEAT_H]);
        nw.show_scale = false;
        assert_eq!(nw.size(), [width(true, false), H + DRAWER_H + HEAT_H]);
    }

    #[test]
    fn the_scale_box_fits_the_wider_window() {
        assert_eq!(width(false, false), W);
        assert_eq!(box_rect(2).max.x + PAD, width(true, false));
        // the drawer's strip under the boxes, its neck right of the chord's name, the dragger under the numbers
        let w = width(false, true);
        let strip = chords::strip_rect(w);
        assert!(strip.min.y > box_rect(0).max.y && strip.max.y < height(true));
        assert!(chords::board_rect(w).min.x > strip.min.x + 170.0);
        assert!(chords::track_rect(w).max.y < strip.max.y - 4.0);
        assert!(chords::zoom_rect(w).max.y < chords::board_rect(w).min.y);
        // ALL SHAPES and KEY in a row left of ZOOM, none over another
        let (key, all, zoom) = (chords::key_rect(w), chords::shapes_rect(w), chords::zoom_rect(w));
        assert!(key.max.x < all.min.x && all.max.x < zoom.min.x && key.width() > 200.0);
        assert!(chords::arrow_rect(w).max.x <= w - PAD + 1.0);
        assert!(box_rect(2).min.x >= box_rect(1).max.x + GAP);
        let board = neck_board();
        assert!(
            board.max.y + 13.0 < box_rect(2).max.y,
            "room for the fret numbers"
        );
        // a fret wide enough for a dot and its ring, and an open string's dot clear of the nut
        assert!((board.width() - NUT_W - 2.0) / 24.0 > 2.0 * (SPOT + 2.0));
        const { assert!(NUT_W / 2.0 + SPOT < NUT_W - 3.0) };
    }
}
