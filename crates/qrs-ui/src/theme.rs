//! Colour tokens: one material, light from the top left, one accent (port of rsplay/ui.py, REDESIGN.md "Round 2").
//! Three themes: Midnight (the default dark, a navy), Graphite (the first dark) and Daylight (light, orange accent).
//! `t()` is the current theme; `set_theme` switches it (and the caches drawn in the old colours are dropped).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use egui::Color32;

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeName {
    Midnight,
    Graphite,
    Daylight,
}

impl ThemeName {
    pub const ALL: [ThemeName; 3] = [ThemeName::Midnight, ThemeName::Graphite, ThemeName::Daylight];

    /// The config.json spelling.
    pub fn key(self) -> &'static str {
        match self {
            ThemeName::Midnight => "midnight",
            ThemeName::Graphite => "graphite",
            ThemeName::Daylight => "daylight",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Midnight => "Midnight",
            ThemeName::Graphite => "Graphite",
            ThemeName::Daylight => "Daylight",
        }
    }

    /// Unknown names fall back to Midnight.
    pub fn parse(s: &str) -> ThemeName {
        match s {
            "graphite" => ThemeName::Graphite,
            "daylight" => ThemeName::Daylight,
            _ => ThemeName::Midnight,
        }
    }
}

/// Every colour of a theme, and its alphas.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub name: ThemeName,
    pub dark: bool,
    /// The one material; its face runs BASE_HI (top left) to BASE_LO.
    pub base: Color32,
    pub base_hi: Color32,
    pub base_lo: Color32,
    /// Readouts sit in a darker screen; SCREEN_HI is the current bar on it.
    pub screen: Color32,
    pub screen_hi: Color32,
    /// Light / dark shadows and the 1 px edge lights.
    pub hilite: Color32,
    pub shade: Color32,
    pub rim: Color32,
    pub rim_lo: Color32,
    /// Main text, secondary, unselected options, captions.
    pub text: Color32,
    pub text_2: Color32,
    pub text_3: Color32,
    pub caption: Color32,
    /// Tab staff lines, bar lines, stems; centre ticks and grips.
    pub line: Color32,
    pub bar_line: Color32,
    pub rhythm: Color32,
    pub tick: Color32,
    /// List rows, seek sections, an empty star, grooves.
    pub divider: Color32,
    pub section: Color32,
    pub star_off: Color32,
    pub track: Color32,
    /// The unlit ticks around a knob; unlit gauge tracks and LEDs on a screen.
    pub knob_tick: Color32,
    pub unlit: Color32,
    /// The ONE accent: fills, cursor, lit notes.
    pub accent: Color32,
    pub accent_hi: Color32,
    pub accent_lo: Color32,
    pub accent_edge: Color32,
    /// The accent as text (darker in Daylight), and text on an accent fill.
    pub accent_ink: Color32,
    pub on_accent: Color32,
    /// The accent as text on an ivory key.
    pub accent_on_ivory: Color32,
    /// Judged hit / late or early / miss (late turns gold when the accent is orange).
    pub hit: Color32,
    pub late: Color32,
    pub miss: Color32,
    pub key_white: Color32,
    pub key_black: Color32,
    /// The guitar neck.
    pub neck: Color32,
    pub nut: Color32,
    pub fret: Color32,
    pub wound: Color32,
    pub plain: Color32,
    pub inlay: Color32,
    /// The dimming behind a dialog, a dialog's drop shadow, a knob's shadow.
    pub scrim: Color32,
    pub drop: Color32,
    pub cast: Color32,
    /// Ivory switch and slider thumbs.
    pub thumb_top: Color32,
    pub thumb_bot: Color32,
    /// An ivory key's shadow colour.
    pub key_sh: Color32,
    /// A floating panel's drop shadow, 1 px edge and top light.
    pub float_sh: Color32,
    pub float_edge: Color32,
    pub float_rim: Color32,
    pub hi_a: f32,
    pub sh_a: f32,
    pub rim_a: f32,
    pub rimlo_a: f32,
    pub scrim_a: f32,
    pub drop_a: f32,
    pub cast_a: f32,
    pub keysh_a: f32,
    pub glow_a: f32,
    pub f_sh1_a: f32,
    pub f_sh2_a: f32,
    pub f_edge_a: f32,
    pub f_rim_a: f32,
}

// ivory keys are the same in every theme
pub const IVORY_TOP: Color32 = c(255, 252, 244);
pub const IVORY_MID: Color32 = c(243, 236, 220);
pub const IVORY_BOT: Color32 = c(229, 218, 194);
pub const IVORY_EDGE: Color32 = c(205, 189, 152);
pub const IVORY_INK: Color32 = c(42, 36, 28);
pub const IVORY_KBD: Color32 = c(106, 95, 77);
pub const DANGER_ON_IVORY: Color32 = c(179, 38, 30);

pub const MIDNIGHT: Theme = Theme {
    name: ThemeName::Midnight,
    dark: true,
    base: c(17, 31, 43),
    base_hi: c(21, 37, 52),
    base_lo: c(13, 25, 35),
    screen: c(10, 21, 30),
    screen_hi: c(18, 32, 44),
    hilite: c(140, 180, 230),
    shade: c(0, 4, 10),
    rim: c(160, 200, 240),
    rim_lo: c(0, 0, 0),
    text: c(231, 238, 246),
    text_2: c(178, 192, 207),
    text_3: c(160, 175, 191),
    caption: c(131, 148, 166),
    line: c(34, 49, 63),
    bar_line: c(61, 79, 97),
    rhythm: c(90, 107, 125),
    tick: c(77, 96, 115),
    divider: c(22, 35, 47),
    section: c(21, 36, 50),
    star_off: c(51, 68, 85),
    track: c(8, 18, 26),
    knob_tick: c(51, 71, 90),
    unlit: c(27, 42, 56),
    accent: c(242, 181, 60),
    accent_hi: c(248, 200, 98),
    accent_lo: c(226, 164, 43),
    accent_edge: c(196, 138, 30),
    accent_ink: c(242, 181, 60),
    on_accent: c(27, 29, 32),
    accent_on_ivory: c(154, 95, 0),
    hit: c(92, 203, 134),
    late: c(240, 138, 75),
    miss: c(240, 100, 90),
    key_white: c(223, 230, 238),
    key_black: c(15, 26, 36),
    neck: c(14, 24, 33),
    nut: c(216, 210, 196),
    fret: c(96, 112, 128),
    wound: c(185, 169, 138),
    plain: c(201, 204, 211),
    inlay: c(36, 52, 68),
    scrim: c(3, 8, 14),
    drop: c(0, 0, 0),
    cast: c(0, 4, 10),
    thumb_top: c(251, 247, 236),
    thumb_bot: c(221, 210, 187),
    key_sh: c(0, 4, 10),
    float_sh: c(0, 4, 10),
    float_edge: c(0, 0, 0),
    float_rim: c(160, 200, 240),
    hi_a: 0.07,
    sh_a: 0.7,
    rim_a: 0.08,
    rimlo_a: 0.35,
    scrim_a: 0.5,
    drop_a: 0.6,
    cast_a: 0.6,
    keysh_a: 0.55,
    glow_a: 0.3,
    f_sh1_a: 0.45,
    f_sh2_a: 0.62,
    f_edge_a: 0.42,
    f_rim_a: 0.1,
};

pub const GRAPHITE: Theme = Theme {
    name: ThemeName::Graphite,
    dark: true,
    base: c(42, 45, 50),
    base_hi: c(47, 51, 56),
    base_lo: c(38, 41, 45),
    screen: c(33, 36, 40),
    screen_hi: c(42, 45, 50),
    hilite: c(255, 255, 255),
    shade: c(0, 0, 0),
    rim: c(255, 255, 255),
    rim_lo: c(0, 0, 0),
    text: c(236, 238, 241),
    text_2: c(191, 196, 204),
    text_3: c(180, 185, 194),
    caption: c(154, 160, 170),
    line: c(61, 66, 73),
    bar_line: c(88, 94, 103),
    rhythm: c(111, 117, 126),
    tick: c(106, 112, 121),
    divider: c(45, 48, 53),
    section: c(48, 52, 58),
    star_off: c(74, 79, 87),
    track: c(27, 30, 33),
    knob_tick: c(86, 92, 101),
    unlit: c(52, 56, 62),
    accent: c(242, 181, 60),
    accent_hi: c(248, 200, 98),
    accent_lo: c(226, 164, 43),
    accent_edge: c(196, 138, 30),
    accent_ink: c(242, 181, 60),
    on_accent: c(27, 29, 32),
    accent_on_ivory: c(154, 95, 0),
    hit: c(92, 203, 134),
    late: c(240, 138, 75),
    miss: c(240, 100, 90),
    key_white: c(223, 226, 232),
    key_black: c(31, 34, 38),
    neck: c(37, 40, 45),
    nut: c(216, 210, 196),
    fret: c(108, 114, 124),
    wound: c(185, 169, 138),
    plain: c(201, 204, 211),
    inlay: c(61, 65, 72),
    scrim: c(14, 15, 17),
    drop: c(0, 0, 0),
    cast: c(0, 0, 0),
    thumb_top: c(251, 247, 236),
    thumb_bot: c(221, 210, 187),
    key_sh: c(0, 0, 0),
    float_sh: c(0, 0, 0),
    float_edge: c(0, 0, 0),
    float_rim: c(255, 255, 255),
    hi_a: 0.075,
    sh_a: 0.6,
    rim_a: 0.08,
    rimlo_a: 0.3,
    scrim_a: 0.5,
    drop_a: 0.55,
    cast_a: 0.55,
    keysh_a: 0.5,
    glow_a: 0.3,
    f_sh1_a: 0.4,
    f_sh2_a: 0.55,
    f_edge_a: 0.4,
    f_rim_a: 0.08,
};

pub const DAYLIGHT: Theme = Theme {
    name: ThemeName::Daylight,
    dark: false,
    base: c(233, 229, 225),
    base_hi: c(241, 238, 235),
    base_lo: c(224, 219, 214),
    screen: c(227, 222, 217),
    screen_hi: c(236, 232, 228),
    hilite: c(255, 255, 255),
    shade: c(166, 152, 141),
    rim: c(255, 255, 255),
    rim_lo: c(140, 126, 116),
    text: c(46, 42, 39),
    text_2: c(94, 88, 82),
    text_3: c(104, 97, 91),
    caption: c(107, 100, 94),
    line: c(207, 200, 193),
    bar_line: c(178, 169, 161),
    rhythm: c(151, 143, 135),
    tick: c(163, 154, 146),
    divider: c(214, 208, 202),
    section: c(222, 217, 212),
    star_off: c(196, 188, 181),
    track: c(214, 208, 202),
    knob_tick: c(184, 175, 167),
    unlit: c(209, 202, 195),
    accent: c(242, 122, 26),
    accent_hi: c(248, 154, 69),
    accent_lo: c(224, 101, 12),
    accent_edge: c(184, 86, 12),
    accent_ink: c(156, 68, 0),
    on_accent: c(27, 29, 32),
    accent_on_ivory: c(156, 68, 0),
    hit: c(17, 112, 60),
    late: c(122, 95, 0),
    miss: c(176, 47, 38),
    key_white: c(251, 250, 249),
    key_black: c(58, 54, 51),
    neck: c(43, 38, 34),
    nut: c(216, 210, 196),
    fret: c(150, 140, 130),
    wound: c(185, 169, 138),
    plain: c(201, 204, 211),
    inlay: c(80, 72, 66),
    scrim: c(110, 95, 85),
    drop: c(110, 95, 85),
    cast: c(70, 50, 30),
    thumb_top: c(255, 253, 246),
    thumb_bot: c(231, 220, 197),
    key_sh: c(60, 40, 20),
    float_sh: c(60, 40, 20),
    float_edge: c(60, 40, 20),
    float_rim: c(255, 255, 255),
    hi_a: 0.95,
    sh_a: 0.55,
    rim_a: 0.9,
    rimlo_a: 0.12,
    scrim_a: 0.22,
    drop_a: 0.32,
    cast_a: 0.32,
    keysh_a: 0.3,
    glow_a: 0.3,
    f_sh1_a: 0.12,
    f_sh2_a: 0.28,
    f_edge_a: 0.09,
    f_rim_a: 0.9,
};

static THEMES: [Theme; 3] = [MIDNIGHT, GRAPHITE, DAYLIGHT];
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicUsize = AtomicUsize::new(0);
static STRING_COLORS: AtomicBool = AtomicBool::new(false);

/// The current theme.
#[inline]
pub fn t() -> &'static Theme {
    &THEMES[CURRENT.load(Ordering::Relaxed)]
}

pub fn set_theme(name: ThemeName) {
    let i = ThemeName::ALL.iter().position(|n| *n == name).unwrap_or(0);
    if CURRENT.swap(i, Ordering::Relaxed) != i {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn theme() -> ThemeName {
    t().name
}

pub fn is_dark() -> bool {
    t().dark
}

/// Bumped on every theme switch (caches drawn in the old colours compare it).
pub fn generation() -> usize {
    GENERATION.load(Ordering::Relaxed)
}

/// Rocksmith string colours, low string first: E red, A yellow, D blue, G orange, B green, e purple.
pub const STRING_PALETTE: [Color32; 6] = [
    c(235, 60, 60),
    c(240, 205, 30),
    c(40, 140, 255),
    c(255, 140, 30),
    c(50, 200, 80),
    c(190, 90, 255),
];

/// String colours are optional (Settings > Display), off by default.
pub fn set_string_colors(on: bool) {
    STRING_COLORS.store(on, Ordering::Relaxed);
}

pub fn string_colors_on() -> bool {
    STRING_COLORS.load(Ordering::Relaxed)
}

/// A string's colour: its Rocksmith colour when they're on, else the neutral caption grey.
pub fn string_color(string: usize) -> Color32 {
    if string_colors_on() {
        STRING_PALETTE[string % 6]
    } else {
        t().caption
    }
}

/// The colour that marks a note to play on this string: its string colour, or the accent when colours are off
/// (the neutral grey wouldn't stand out).
pub fn string_highlight(string: usize) -> Color32 {
    if string_colors_on() {
        STRING_PALETTE[string % 6]
    } else {
        t().accent
    }
}

/// hit / late / early / miss
pub fn judge_color(kind: &str) -> Color32 {
    let th = t();
    match kind {
        "hit" => th.hit,
        "late" | "early" => th.late,
        _ => th.miss,
    }
}

pub fn fmt_time(t: f64) -> String {
    let t = t.max(0.0) as i64;
    format!("{}:{:02}", t / 60, t % 60)
}
