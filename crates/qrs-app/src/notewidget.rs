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
//! where); right-click for its menu. Settings > Audio switches it on or off (config "note_widget").

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

use crate::state::AppState;

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
/// How much bigger the scale box is when it's popped up.
const BIG: f32 = 2.4;
const EVERY: Duration = Duration::from_millis(50);
/// Keep showing a note this long after the guitar goes quiet.
const HOLD: f32 = 1.0;

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
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

/// The window's width: the two boxes, or three with the scale.
fn width(scale: bool) -> f32 {
    if scale { W + GAP + NECK_W } else { W }
}

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
    bg: Option<(((bool, bool), bool, u32), egui::TextureHandle)>,
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
            bg: None,
            styled: false,
            show_scale: true,
            scale: None,
            playing: None,
            strings: GUITAR_OPEN_MIDI.to_vec(),
            tuning: "E Standard".into(),
            fitted_w: None,
            big: false,
            small_pos: None,
        }
    }
}

fn pos_file(st: &AppState) -> std::path::PathBuf {
    st.data.cache().join("note_widget.json")
}

impl NoteWidget {
    /// Every frame: listen (20 times a second) and show the window while it's on. `arr` is the part open in the
    /// player, whose tuning the scale box's neck takes.
    pub fn ui(&mut self, ctx: &egui::Context, st: &mut AppState, arr: Option<&Arrangement>) {
        // Android has one window: a second viewport would be drawn over the app, so there's no floating widget
        if cfg!(target_os = "android") {
            return;
        }
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
        self.set_tuning(arr);
        let [w, h] = self.size();
        let now = Instant::now();
        if now >= self.next {
            self.next = now + EVERY;
            self.listen(st, now);
        }
        ctx.request_repaint_after(EVERY);
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
            .with_taskbar(false);
        if let Some(p) = self.pos {
            builder = builder.with_position(pos2(p.x / z, p.y / z));
        }
        let mut save_pos = None;
        let mut reset = false;
        let mut close = false;
        let mut toggle_scale = false;
        let mut toggle_big = false;
        let shown = self.show_scale;
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
            self.draw(ui, area);
            let resp = ui.interact(area, egui::Id::new("note-widget-drag"), Sense::click_and_drag());
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
                if ui.button("Forget the key (start guessing again)").clicked() {
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
    /// shots) leaves the scale box out, as the Python widget has none.
    pub fn preview(&mut self, ui: &egui::Ui, rect: Rect) {
        self.show_scale = !qrs_ui::theme::classic();
        self.status = "listening".into();
        self.note.set(Some(note_color(3)), "Eb", "Eb3  +3¢");
        self.deg.set(Some(degree_color("1")), "1", "Eb major");
        self.scale = Some((3, Mode::Major));
        self.playing = Some(51);
        let w = width(self.show_scale);
        let r = Rect::from_min_size(pos2(rect.max.x - w, rect.min.y), vec2(w, H));
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
            Rect::from_min_size(Pos2::ZERO, vec2(width(self.show_scale), H))
        }
    }

    /// The window's size (the system's points): the view, BIG times over when popped up.
    fn size(&self) -> [f32; 2] {
        let v = self.view().size() * if self.big { BIG } else { 1.0 };
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

    /// The note you're playing (from the guitar input) and the key it's in.
    fn listen(&mut self, st: &AppState, now: Instant) {
        self.status = match &st.audio {
            None => "audio engine off".into(),
            Some(a) if a.engine.status().input.is_none() => "no guitar input (F2)".into(),
            Some(a) => {
                let e = &a.engine;
                let rate = e.capture().rate();
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
        self.key.add(pc, now.duration_since(self.start).as_secs_f64());
        self.scale = self.key.key;
        self.playing = held.map(|m| m.round() as i32);
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
        let w = width(self.show_scale);
        let view = self.view();
        let u = area.width() / view.width();
        let live = (self.note.target.is_some(), self.deg.target.is_some());
        let k = u * ctx.pixels_per_point(); // screen pixels a widget pixel
        let key = (live, self.show_scale, k.to_bits());
        if self.bg.as_ref().is_none_or(|(kk, _)| *kk != key) {
            let img = background(live, self.show_scale, k);
            let tex = ctx.load_texture("note-widget-bg", img, egui::TextureOptions::LINEAR);
            self.bg = Some((key, tex));
        }
        // (popped up, the window is the scale box's part of the widget: everything else falls outside it)
        let o = area.min - view.min.to_vec2() * u;
        if let Some((_, tex)) = &self.bg {
            p.image(
                tex.id(),
                Rect::from_min_size(o, vec2(w, H) * u),
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
            pil_text(
                &p,
                &self.status,
                at(w - PAD - 2.0, PAD + 4.0),
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
            self.draw_scale(&p, u, at);
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
            let fill = match deg {
                Some(d) => degree_color(d),
                None if playing && self.scale.is_some() => OUT_OF_KEY,
                None if playing => note_color(midi.rem_euclid(12) as usize),
                None => continue,
            };
            let c = at(fx(f), sy(s));
            p.circle_filled(c, SPOT * u, fill);
            if let Some(d) = deg {
                pil_text(
                    p,
                    d,
                    c,
                    Align2::CENTER_CENTER,
                    Style::new(8.0 * u, 800, ink(fill)),
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
const INLAY: Color32 = Color32::from_rgb(0x3e, 0x43, 0x4a);

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
fn background(live: (bool, bool), scale: bool, k: f32) -> egui::ColorImage {
    let w = width(scale);
    let (wp, hp) = ((w * k).round() as usize, (H * k).round() as usize);
    let mut l = Layers::new(wp, hp);
    l.over_all(qrs_ui::raster::rgbf(BASE), 1.0);
    // the panel's own edge catches the light on the top left
    let panel = shape(
        Rect::from_min_size(Pos2::ZERO, vec2(w, H)),
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
    l.image(None).to_egui()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(nw.size(), [width(true), H]);
        nw.big = true;
        let v = nw.view();
        assert!(v.contains_rect(box_rect(2)) && !v.contains(box_rect(1).center()));
        assert_eq!(nw.size(), [v.width() * BIG, v.height() * BIG]);
        nw.big = false;
        assert_eq!(nw.view().size(), vec2(width(true), H));
    }

    #[test]
    fn the_scale_box_fits_the_wider_window() {
        assert_eq!(width(false), W);
        assert_eq!(box_rect(2).max.x + PAD, width(true));
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
