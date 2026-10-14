//! The note widget (P7.3; port of rsplay/notewidget.py and notefeed.py): a small always-on-top window showing the
//! note you're playing and the key you seem to be in. The Python app ran it as its own process fed over UDP; here
//! it's a second window of the app (an egui viewport) fed directly: the pitch from the guitar input the engine
//! captures (the tuner's McLeod method, the median of the last five readings, held for a second), the key from
//! `qrs_analysis::keyguess` (a fading pitch-class histogram against the Krumhansl-Kessler profiles).
//!
//! Left box: the note, on a disc coloured by the note (each of the 12 its own colour). Right box: the key, on a
//! disc coloured by which degree of the key the note is (1 to 7; grey outside the key). Drag it to move it (it
//! remembers where); right-click for its menu. Settings > Audio switches it on or off (config "note_widget").

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{
    Align, Align2, Color32, CursorIcon, Pos2, Rect, Sense, ViewportBuilder, ViewportCommand, ViewportId,
    pos2, vec2,
};
use qrs_analysis::keyguess::{KeyGuess, Mode};
use qrs_analysis::spectrum::Spectra;
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
    /// The drawn background for (which discs are live, the scale it was drawn at).
    bg: Option<(((bool, bool), u32), egui::TextureHandle)>,
    /// The window has its rounded corners and border (Windows 11).
    styled: bool,
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
        }
    }
}

fn pos_file(st: &AppState) -> std::path::PathBuf {
    st.data.cache().join("note_widget.json")
}

impl NoteWidget {
    /// Every frame: listen (20 times a second) and show the window while it's on.
    pub fn ui(&mut self, ctx: &egui::Context, st: &mut AppState) {
        let on = st.config.bool("note_widget", true);
        if on && !self.was_on {
            self.closed = false; // switched on (again) in Settings
        }
        self.was_on = on;
        if !on || self.closed {
            return;
        }
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
        let mut builder = ViewportBuilder::default()
            .with_title("quickRS notes")
            .with_inner_size([W, H])
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false);
        if let Some(p) = self.pos {
            builder = builder.with_position(p);
        }
        let mut save_pos = None;
        let mut reset = false;
        let mut close = false;
        let id = ViewportId::from_hash_of("quickrs-note-widget");
        if !self.styled {
            self.styled = style_window(); // (once the window exists)
        }
        ctx.show_viewport_immediate(id, builder, |ui, _class| {
            let wctx = ui.ctx().clone();
            let area = wctx.content_rect();
            self.draw(ui, area);
            let resp = ui.interact(area, egui::Id::new("note-widget-drag"), Sense::click_and_drag());
            if resp.drag_started() {
                wctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            if resp.hovered() {
                wctx.set_cursor_icon(CursorIcon::Grab);
            }
            if resp.drag_stopped() || wctx.input(|i| i.pointer.primary_released()) {
                save_pos = wctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
            }
            resp.context_menu(|ui| {
                if ui.button("Forget the key (start guessing again)").clicked() {
                    reset = true;
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

    /// Screenshot mode: the widget as it looks with Eb3 a few cents sharp, in Eb major, drawn into `rect` of the
    /// main window.
    pub fn preview(&mut self, ui: &egui::Ui, rect: Rect) {
        self.status = "listening".into();
        self.note.set(Some(note_color(3)), "Eb", "Eb3  +3¢");
        self.deg.set(Some(degree_color("1")), "1", "Eb major");
        let p = ui.painter();
        p.rect_filled(
            rect.translate(vec2(0.0, 10.0)).expand(6.0),
            18.0,
            Color32::from_black_alpha(60),
        );
        let r = Rect::from_min_size(rect.min, vec2(W, H));
        self.draw(ui, r);
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
        let u = area.width() / W;
        let live = (self.note.target.is_some(), self.deg.target.is_some());
        let k = u * ctx.pixels_per_point(); // screen pixels a widget pixel
        let key = (live, k.to_bits());
        if self.bg.as_ref().is_none_or(|(kk, _)| *kk != key) {
            let img = background(live, k);
            let tex = ctx.load_texture("note-widget-bg", img, egui::TextureOptions::LINEAR);
            self.bg = Some((key, tex));
        }
        let o = area.min;
        if let Some((_, tex)) = &self.bg {
            p.image(
                tex.id(),
                Rect::from_min_size(o, vec2(W, H) * u),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let at = |x: f32, y: f32| pos2(o.x + x * u, o.y + y * u);
        let st = |size: f32, weight: u16, color: Color32| Style::new(size * u, weight, color);
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
            at(W - PAD - 2.0, PAD + 4.0),
            Align2::RIGHT_TOP,
            st(11.0, 500, DIM),
        );
        for (i, (caption, b)) in [("NOTE", &self.note), ("KEY", &self.deg)].into_iter().enumerate() {
            let bx = box_rect(i);
            // small caps, letter-spaced by 1.2 px
            let mut x = bx.min.x + 12.0;
            for ch in caption.chars() {
                let cs = st(9.0, 800, DIM);
                pil_text(&p, &ch.to_string(), at(x, bx.min.y + 10.0), Align2::LEFT_TOP, cs);
                let adv = CAPTION_ADV.iter().find(|(c, _)| *c == ch).map(|a| a.1);
                x += adv.unwrap_or_else(|| text::width(&ctx, &ch.to_string(), cs) / u) + 1.2;
            }
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

/// The NOTE (0) and KEY (1) boxes, in the window's points.
fn box_rect(i: usize) -> Rect {
    Rect::from_min_size(
        pos2(PAD + i as f32 * (BOX_W + GAP), PAD + 30.0),
        vec2(BOX_W, BOX_H),
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

/// The panel, its two sunken boxes and each disc raised (live) or sunk in (empty), at `k` pixels a point
/// (`background`; the text goes on top each frame).
fn background(live: (bool, bool), k: f32) -> egui::ColorImage {
    let (wp, hp) = ((W * k).round() as usize, (H * k).round() as usize);
    let mut l = Layers::new(wp, hp);
    l.over_all(qrs_ui::raster::rgbf(BASE), 1.0);
    // the panel's own edge catches the light on the top left
    let panel = shape(
        Rect::from_min_size(Pos2::ZERO, vec2(W, H)),
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
}
