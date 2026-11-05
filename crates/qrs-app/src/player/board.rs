//! The Pedalboard tile and its big view (PLAN.md P14.3-P14.4, D26).
//!
//! The tile: the song's tones (the one playing lit; SONG follows the chart's tone changes), the chain as small
//! pedals (click one to show it, double-click or its light to switch it, drag to move it, + to add one), the
//! shown block's face with every control (drag a knob up or down, Shift for fine, the wheel for steps,
//! double-click for its default; ‹ › try the other models of its kind; the footswitch), the input and output
//! meters and DRY, which bypasses everything while held on to compare. The expand key opens the big view: the
//! whole board with its cables, a large editor with a picture of what the block does, a browser of all 54 models,
//! the starting boards and yours, Save as, an input level check.
//!
//! Edits go to the running board in place (tails ring on) and are kept when the mouse lets go: a song's tone in
//! userdata.json (`rigs`), your own board in config.json (`tone_board`).

use std::time::{Duration, Instant};

use eframe::egui;
use egui::epaint::CubicBezierShape;
use egui::{
    Align2, Color32, CursorIcon, Event, Key, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2,
};
use qrs_audio::board::picture::{self, Picture};
use qrs_audio::board::{self, Block, Board, Face, Kind, MAX_BLOCKS, MODELS, Unit, catalog};
use qrs_audio::tone::ToneSpec;
use qrs_ui::icons::Icon;
use qrs_ui::keys;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{IVORY_INK, t};
use qrs_ui::units::px;
use qrs_ui::widgets::{self, RoundKind, interact};

use super::PlayerView;
use super::tiles::CLOSE_SLOT;
use crate::pedalboard::{self as pb, SongRig};
use crate::pedalfaces::{self as faces, Detail, Hot};
use crate::state::AppState;

mod back;
pub(crate) mod captures;
mod edit;
mod sounds;

/// A tone picked by hand on the tile (it holds until the song or part changes, or SONG is turned back on).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Tone(usize),
    Mine,
}

/// The block browser: where it adds (or which block it changes) and what it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Picker {
    Kinds { at: usize },
    Models { at: usize, kind: Kind },
}

/// A knob being turned: which copy of the face holds it (`tag`: the tile, the board, the editor, the room's open
/// unit...), the block and knob, where the drag started and the knob's turn there. Only the face with the same tag
/// moves or ends it (B1, P37.1: two copies of one face used to fight over it, and the knob never turned).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KnobDrag {
    pub tag: &'static str,
    pub uid: u32,
    pub knob: usize,
    pub start: Pos2,
    pub u0: f32,
    pub across: bool,
    pub len: f32,
}

/// A block being dragged along a strip to a new place; `tag` is the strip doing it (B2).
#[derive(Clone, Copy, Debug)]
pub(crate) struct MoveDrag {
    pub tag: &'static str,
    pub uid: u32,
    pub grab: f32,
    pub x: f32,
    pub moved: bool,
    /// The pass its strip last drew it in: the button coming up is the strip's to see (and drop) the next pass;
    /// `board_update` runs first and ends only a move whose strip has gone (P38.1).
    pub pass: u64,
}

/// What a control does with a knob drag this frame (`drag_step`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DragAct {
    /// Nothing for this control.
    Idle,
    /// A press on it: a drag starts here.
    Start,
    /// It holds the drag: the pointer has moved this far (points, along its direction) since the start.
    Turn(f32),
    /// It held the drag and the button is up: keep the edit.
    End,
}

/// The drag rule for one control of one face (pure, so it's tested): `tag`/`uid`/`knob` are the control, `down_here`
/// whether the button went down on it (egui gives a press to one widget only), `pos` the pointer, `any_down` whether
/// any button is still held anywhere. A face with another tag never ends a drag it doesn't hold.
pub(crate) fn drag_step(
    state: Option<&KnobDrag>,
    tag: &'static str,
    uid: u32,
    knob: usize,
    down_here: bool,
    pos: Option<Pos2>,
    any_down: bool,
) -> DragAct {
    let owned = state.is_some_and(|d| d.tag == tag && d.uid == uid && d.knob == knob);
    if owned {
        let d = state.expect("owned");
        return match (any_down, pos) {
            (true, Some(p)) => Act::turn(d, p),
            (true, None) => DragAct::Turn(0.0),
            (false, _) => DragAct::End,
        };
    }
    if down_here && pos.is_some() {
        return DragAct::Start;
    }
    DragAct::Idle
}

struct Act;
impl Act {
    fn turn(d: &KnobDrag, p: Pos2) -> DragAct {
        DragAct::Turn(if d.across {
            p.x - d.start.x
        } else {
            d.start.y - p.y
        })
    }
}

/// Where a block dropped on a strip lands: the count of the strip's other blocks whose centres are left of the
/// dropped one's centre. Only the rects of the strip doing the dragging count (B2).
pub(crate) fn drop_index(rects: &[(u32, Rect)], uid: u32, centre_x: f32) -> usize {
    rects
        .iter()
        .filter(|(u, r)| *u != uid && r.center().x < centre_x)
        .count()
}

/// Where a lifted block would land on its strip (`rects`), marked: an accent bar in the gap it would drop into
/// (nothing while it would land where it already is). `gap` is the strip's gap between blocks, `y` the bar's span.
fn drop_marker(p: &egui::Painter, rects: &[(u32, Rect)], uid: u32, centre_x: f32, gap: f32, y: (f32, f32)) {
    let to = drop_index(rects, uid, centre_x);
    if rects.iter().position(|(u, _)| *u == uid) == Some(to) {
        return;
    }
    let others: Vec<Rect> = rects.iter().filter(|(u, _)| *u != uid).map(|(_, r)| *r).collect();
    let x = match (to.checked_sub(1).and_then(|i| others.get(i)), others.get(to)) {
        (Some(a), Some(b)) => (a.max.x + b.min.x) / 2.0,
        (Some(a), None) => a.max.x + gap / 2.0,
        (None, Some(b)) => b.min.x - gap / 2.0,
        (None, None) => return,
    };
    let th = t();
    let bar = Rect::from_min_max(pos2(x - px(1.5), y.0), pos2(x + px(1.5), y.1));
    paint::glow(p, bar, th.accent, 6.0, Some(0.5), Some(px(1.5)));
    p.rect_filled(bar, px(1.5), th.accent);
    for yy in [y.0, y.1] {
        p.circle_filled(pos2(x, yy), px(3.5), th.accent);
    }
}

/// A control the wheel can turn: (block, knob, its rect).
#[derive(Clone, Copy, Debug)]
struct WheelSpot {
    uid: u32,
    knob: usize,
    rect: Rect,
}

pub struct BoardUi {
    /// Where the chain strip drew its units last frame (uid, rect): the room's "i" dots (P37.13).
    pub chips: Vec<(u32, Rect)>,
    /// The engine's clip count last seen, until when the input meter shows CLIP, and when it was last said.
    clips_seen: Option<u32>,
    clip_until: Option<Instant>,
    clip_said: Option<Instant>,
    pub rig: Option<SongRig>,
    rig_for: String,
    /// The song's tone in force now (followed from the chart).
    pub song_tone: usize,
    pub pick: Option<Pick>,
    mine: Option<Board>,
    /// The block shown on the tile and in the editor.
    pub sel: Option<u32>,
    pub picker: Option<Picker>,
    /// The big view is up.
    pub open: bool,
    drag: Option<KnobDrag>,
    pub(crate) moving: Option<MoveDrag>,
    dirty: bool,
    wheeled: Option<Instant>,
    /// Everything bypassed, to compare with the dry guitar.
    pub dry: bool,
    sent: Option<Board>,
    browse: Option<Kind>,
    /// Save as: the name being typed.
    pub naming: Option<String>,
    menu: bool,
    undo: Option<(Board, String)>,
    /// The input check: until when, and the highest peak heard.
    level: Option<(Instant, f32)>,
    level_note: Option<String>,
    wheel: Vec<WheelSpot>,
    tile_rect: Rect,
    /// The big view shows the selected block's editor (else the whole board).
    pub editing: bool,
    /// The editor's model list is open.
    model_menu: bool,
    /// A block's A/B: the slot not playing, and whether B is playing.
    ab: Option<Ab>,
    /// Edits that can be undone (the board before, what changed), newest last; and redone.
    hist: Vec<(Board, String)>,
    redo: Vec<(Board, String)>,
    /// The board as last kept, and which board that is (the history is per board).
    kept: Option<Board>,
    kept_for: String,
    /// The starting point last put on a block: (block, name).
    last_start: Option<(u32, &'static str)>,
    /// Tap tempo: the taps, and the tempo they make.
    taps: Vec<Instant>,
    tap_bpm: Option<f32>,
    /// The cab editor: the mic's angle on the speaker (it only sets how far from the centre), a drag on its pad.
    mic_angle: f32,
    pad_drag: Option<usize>,
    /// Screenshots: the editor's knob shown as hovered (QUICKRS_BOARD_HOVER, a knob key).
    shot_hover: Option<String>,
    /// The editor shows what's behind the panel (an amp's back, a drive's insides), for models that have it.
    pub back: bool,
    /// The back-panel control under the mouse last frame (the tip column beside the art explains it).
    back_hot: Option<usize>,
    /// How brightly the back panel's power tubes glow, following the board's output (smoothed).
    glow: f32,
    /// A board to hear instead of the one shown (the amp room's A/B against the saved sound, a sound previewed).
    pub audition: Option<Board>,
    /// The board is drawn on the amp room's stage (no aluminium board under it).
    pub stage: bool,
    /// Your saved sounds listed on the tile (P23.3), and what putting one on changed (for Back).
    sounds: Option<sounds::SoundPick>,
    sound_before: Option<sounds::Before>,
    /// The captures dialog (NAM captures and IR cabs, P31).
    pub captures: Option<captures::Dialog>,
    /// The frame `begin_frame` last ran in (it runs once a frame, whoever calls it first).
    frame: u64,
    /// Faces and strips drawn now are under an overlay: drawn only, nothing turns or takes the wheel (B1, B4).
    pub inert: bool,
    /// The starting board last put on your board from the tile's pills (song with no rig): lit, and MINE reads
    /// BACK until it's taken back (B6).
    tile_preset: Option<usize>,
}

/// What kind of control a face's control is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ctl {
    Knob,
    Fader {
        across: bool,
    },
    Choice,
    /// The cab editor's mic pad (the Mic knob) and distance slider.
    Pad,
}

/// The control under the mouse (or being turned): its knob, where it is, what it is.
#[derive(Clone, Copy, Debug)]
struct HotCtl {
    knob: usize,
    rect: Rect,
    ctl: Ctl,
}

/// A block's two settings to compare.
#[derive(Clone, Copy, Debug)]
struct Ab {
    uid: u32,
    other: [f32; board::MAX_KNOBS],
    on_b: bool,
}

impl Default for BoardUi {
    fn default() -> Self {
        BoardUi {
            chips: Vec::new(),
            clips_seen: None,
            clip_until: None,
            clip_said: None,
            rig: None,
            rig_for: String::new(),
            song_tone: 0,
            pick: None,
            mine: None,
            sel: None,
            picker: None,
            open: false,
            drag: None,
            moving: None,
            dirty: false,
            wheeled: None,
            dry: false,
            sent: None,
            browse: None,
            naming: None,
            menu: false,
            undo: None,
            level: None,
            level_note: None,
            wheel: Vec::new(),
            tile_rect: Rect::NOTHING,
            editing: false,
            model_menu: false,
            ab: None,
            hist: Vec::new(),
            redo: Vec::new(),
            kept: None,
            kept_for: String::new(),
            last_start: None,
            taps: Vec::new(),
            tap_bpm: None,
            mic_angle: -0.6,
            pad_drag: None,
            shot_hover: None,
            back: false,
            back_hot: None,
            glow: 0.0,
            audition: None,
            stage: false,
            sounds: None,
            sound_before: None,
            captures: None,
            frame: u64::MAX,
            inert: false,
            tile_preset: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Song(usize),
    Mine,
}

/// What the board's UI needs of the song it's shown with (nothing in the amp room: no song, the board is yours).
#[derive(Clone, Debug, Default)]
pub struct SongCtx {
    pub title: String,
    pub display_title: String,
    pub artist: String,
    pub sid: String,
    pub part_name: String,
    /// The part's tones only (`rig`, `base_tone`, `tones`): what `SongRig` reads.
    pub arr: qrs_formats::Arrangement,
    /// The chart's beats (the editor's tempo sync).
    pub btimes: Vec<f64>,
    /// The song's position now.
    pub pos: f64,
}

/// The pedalboard's state and UI (the tile, the big view, the block editor), owned by the player and by the amp
/// room (D47): everything it needs of its host is here, and what it says or asks goes out through `said`, `tip`
/// and `reqs` for the host to show.
pub struct Rack {
    pub board: BoardUi,
    pub song: SongCtx,
    pub engine: Option<std::sync::Arc<qrs_audio::Engine>>,
    /// The tooltip to draw this frame (text, key, where, anchor).
    pub tip: Option<(String, String, Pos2, Align2)>,
    /// Toasts to show: (text, tail).
    pub said: Vec<(String, String)>,
    pub reqs: Vec<super::PlayerReq>,
    /// What "My board" is called here (the amp room: the rig's name).
    pub title: Option<String>,
}

impl Rack {
    pub fn new(engine: Option<std::sync::Arc<qrs_audio::Engine>>) -> Rack {
        Rack {
            board: BoardUi::default(),
            song: SongCtx::default(),
            engine,
            tip: None,
            said: Vec::new(),
            reqs: Vec::new(),
            title: None,
        }
    }

    pub fn engine(&self) -> Option<&std::sync::Arc<qrs_audio::Engine>> {
        self.engine.as_ref()
    }

    pub fn say(&mut self, text: impl Into<String>, tail: impl Into<String>) {
        self.said.push((text.into(), tail.into()));
    }

    /// There's an edit to undo / to redo.
    pub fn can_undo(&self) -> bool {
        !self.board.hist.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.board.redo.is_empty()
    }
}

impl PlayerView {
    /// The rack in step with the player: the engine, the song's position, and the part when it changed.
    fn rack_sync(&mut self) {
        let e = self.transport.engine().cloned();
        let r = &mut self.rack;
        r.engine = e;
        r.song.pos = self.transport.position();
        if r.song.sid != self.sid || r.song.part_name != self.part_name || r.song.title != self.entry.title {
            r.song.title = self.entry.title.clone();
            r.song.display_title = self.entry.display_title();
            r.song.artist = self.entry.artist.clone();
            r.song.sid = self.sid.clone();
            r.song.part_name = self.part_name.clone();
            r.song.arr = qrs_formats::Arrangement {
                rig: self.chart.arr.rig.clone(),
                base_tone: self.chart.arr.base_tone.clone(),
                tones: self.chart.arr.tones.clone(),
                ..Default::default()
            };
        }
        if r.song.btimes.len() != self.chart.btimes.len() {
            r.song.btimes = self.chart.btimes.clone();
        }
    }

    /// What the rack said, its tooltip and its requests, to the player's.
    fn rack_drain(&mut self) {
        for (a, b) in std::mem::take(&mut self.rack.said) {
            self.say(a, b);
        }
        if let Some(t) = self.rack.tip.take() {
            self.tip = Some(t);
        }
        self.reqs.append(&mut self.rack.reqs);
    }

    pub fn board_update(&mut self, st: &mut AppState) {
        self.rack_sync();
        self.rack.board_update(st);
        self.rack_drain();
    }

    pub fn board_leave(&mut self, st: &mut AppState) {
        self.rack_sync();
        self.rack.board_leave(st);
        self.rack_drain();
    }

    pub(crate) fn board_use(&mut self, st: &mut AppState) {
        self.rack_sync();
        self.rack.board_use(st);
        self.rack_drain();
    }

    pub(crate) fn board_shot(&mut self, st: &mut AppState, preset: Option<&str>, sel: Option<&str>) {
        self.rack_sync();
        self.rack.board_shot(st, preset, sel);
        self.rack_drain();
    }

    pub(crate) fn board_sounds_shot(&mut self, st: &mut AppState, on: bool) {
        self.rack_sync();
        self.rack.sounds_shot(st, on);
        self.rack_drain();
    }

    pub fn board_wheel(&mut self, st: &mut AppState, pos: Pos2, notches: f32, shift: bool) -> bool {
        self.rack_sync();
        let used = self.rack.board_wheel(st, pos, notches, shift);
        self.rack_drain();
        used
    }

    pub fn board_key(&mut self, st: &mut AppState, ev: &Event) -> bool {
        let used = self.rack.board_key(st, ev);
        self.rack_drain();
        used
    }

    pub(super) fn draw_board_tile(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        x: f32,
        y: f32,
        w: f32,
        lifting: bool,
    ) -> f32 {
        self.rack_sync();
        let h = self.rack.draw_board_tile(ui, st, x, y, w, lifting);
        self.rack_drain();
        h
    }

    pub(crate) fn draw_board_view(&mut self, ui: &Ui, st: &mut AppState, area: Rect) {
        self.rack_sync();
        self.rack.draw_board_view(ui, st, area);
        self.rack.captures_ui(ui, st, area);
        self.rack_drain();
    }
}

// -- small drawing helpers --------------------------------------------------------------------------------------------

/// What a block is after ("after a 1959 Super Lead"), or the file it runs (a NAM capture's name, P31).
pub(crate) fn after_line(blk: &Block) -> String {
    Rack::file_name(blk).unwrap_or_else(|| format!("after {}", blk.def().after))
}

/// The models to offer for a kind: ours, then a NAM capture or IR cab where captures are on (P31).
pub(crate) fn kind_models(kind: Kind) -> Vec<(usize, &'static board::ModelDef)> {
    catalog::of_kind(kind)
        .chain(catalog::files().filter(move |(_, m)| crate::captures::ON && m.kind == kind))
        .collect()
}

fn caps(p: &Painter, s: &str, pos: Pos2, size: f32, color: Color32, anchor: Align2) -> Rect {
    text::caps(p, s, pos, size, color, anchor, None)
}

/// A row of pills in a sunken track, the chosen one an ivory key. Returns each pill's rect.
fn pills(p: &Painter, track: Rect, labels: &[String], sel: Option<usize>, size: f32) -> Vec<Rect> {
    let th = t();
    let ctx = p.ctx();
    paint::inset(p, track, Some(10.0), 2.0, Fill::Base);
    let inner = track.shrink(px(3.0));
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| text::width(ctx, l, Style::caps(size, th.caption)) + px(14.0))
        .collect();
    let total: f32 = widths.iter().sum::<f32>().max(1.0);
    let k = inner.width() / total;
    let mut x = inner.min.x;
    let mut out = Vec::new();
    for (i, (l, w)) in labels.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(pos2(x, inner.min.y), vec2(w * k, inner.height()));
        if Some(i) == sel {
            keys::key_face(p, r, Some(7.0), false, false);
        }
        text::caps(
            p,
            l,
            r.center(),
            size,
            if Some(i) == sel {
                keys::key_ink(false)
            } else {
                th.caption
            },
            Align2::CENTER_CENTER,
            Some(r.width() - px(4.0)),
        );
        out.push(r);
        x += w * k;
    }
    out
}

/// A small toggle key with a light: (rect, on).
fn lamp_key(p: &Painter, r: Rect, label: &str, on: bool, hot: bool) {
    let th = t();
    if on {
        keys::key_face(p, r, Some(9.0), false, false);
    } else {
        paint::inset(p, r, Some(9.0), 2.0, Fill::Base);
    }
    let lc = pos2(r.min.x + px(11.0), r.center().y);
    if on {
        paint::led(p, lc, 7.0, th.accent);
    } else {
        p.circle_filled(lc, 3.0, th.unlit);
    }
    caps(
        p,
        label,
        pos2(lc.x + px(8.0), r.center().y),
        10.0,
        if on { keys::key_ink(false) } else { th.caption },
        Align2::LEFT_CENTER,
    );
    if hot {
        p.rect_stroke(
            r.expand(1.0),
            10.0,
            Stroke::new(1.0, th.accent.gamma_multiply(0.6)),
            egui::StrokeKind::Outside,
        );
    }
}

/// A level meter: a groove, the target band, the level, and a dB readout (CLIP while the input clips). The label
/// keeps clear of the readout at its widest: a device name after " · " that doesn't fit is cut to its first word
/// ("IN · FOCUSRITE"), or left off if even that doesn't fit (the header names the device too).
fn meter(p: &Painter, r: Rect, label: &str, peak: f32, target: bool, clip: bool) {
    let th = t();
    let db = 20.0 * peak.max(1e-6).log10();
    let ud = |d: f32| ((d + 60.0) / 60.0).clamp(0.0, 1.0);
    let ctx = p.ctx();
    let read_w = text::width(ctx, "-100 dB", Style::new(9.0, 800, th.caption));
    let room = (r.width() - read_w - px(8.0)).max(px(12.0));
    let fits = |s: &str| text::width(ctx, s, Style::caps(9.0, th.caption)) <= room;
    let mut label = label.to_string();
    if !fits(&label)
        && let Some((head, name)) = label.split_once(" \u{b7} ")
    {
        let word = name.split_whitespace().next().unwrap_or(name);
        let short = format!("{head} \u{b7} {word}");
        label = if fits(&short) { short } else { head.to_string() };
    }
    text::caps(
        p,
        &label,
        pos2(r.min.x, r.min.y + px(5.0)),
        9.0,
        th.caption,
        Align2::LEFT_CENTER,
        Some(room),
    );
    let color = if clip || db > -1.0 {
        th.miss
    } else if target && db > -18.0 {
        th.hit
    } else if target {
        th.late
    } else {
        th.hit
    };
    let txt = if clip {
        "CLIP".to_string()
    } else if peak < 1e-5 {
        "\u{2212}\u{221e}".to_string()
    } else {
        format!("{:.0} dB", db)
    };
    text::draw(
        p,
        &txt,
        pos2(r.max.x, r.min.y + px(5.0)),
        Align2::RIGHT_CENTER,
        Style::new(9.0, 800, if peak < 1e-5 { th.caption } else { color }),
        None,
    );
    let g = Rect::from_min_max(pos2(r.min.x, r.max.y - px(8.0)), r.max);
    paint::inset(p, g, Some(4.0), 1.5, Fill::Color(th.track));
    let inner = g.shrink(1.0);
    if target {
        let band = Rect::from_min_max(
            pos2(inner.min.x + inner.width() * ud(-18.0), inner.min.y),
            pos2(inner.min.x + inner.width() * ud(-6.0), inner.max.y),
        );
        p.rect_filled(band, 2.0, th.hit.gamma_multiply(0.18));
    }
    let fill = Rect::from_min_max(inner.min, pos2(inner.min.x + inner.width() * ud(db), inner.max.y));
    p.rect_filled(fill, 3.0, color);
}

/// A tone's label for a pill: its first word, at most six letters ("clean lead" → "CLEAN", "breakdown" → "BREAK").
fn short_tone(label: &str) -> String {
    let w = label.split(' ').next().unwrap_or(label).to_uppercase();
    // (the first word whole up to 8 letters, ACOUSTIC among them; longer ones cut with an ellipsis: "ACOUS" read
    // as a typo, P37.4b; the pills share the row by their labels' widths)
    if w.chars().count() > 8 {
        let mut s: String = w.chars().take(7).collect();
        s.push('…');
        s
    } else {
        w
    }
}

/// Strip a device name to its first words ("Focusrite USB ASIO", "Analogue 1 + 2").
fn short_device(n: &str) -> String {
    let n = n.split('(').next().unwrap_or(n).trim();
    let mut s: String = n.chars().take(22).collect();
    if n.chars().count() > 22 {
        s.push('\u{2026}');
    }
    s.to_uppercase()
}

/// What changed between two boards, in a few words ("Treble 6.0 \u{2192} 6.5", "Added Mean Green").
fn change_label(before: &Board, after: &Board) -> String {
    if before.blocks.len() < after.blocks.len()
        && let Some(n) = after.blocks.iter().find(|b| before.index_of(b.uid).is_none())
    {
        return format!("Added {}", n.def().name);
    }
    if before.blocks.len() > after.blocks.len()
        && let Some(o) = before.blocks.iter().find(|b| after.index_of(b.uid).is_none())
    {
        return format!("Took off {}", o.def().name);
    }
    for (i, a) in after.blocks.iter().enumerate() {
        let Some(j) = before.index_of(a.uid) else {
            continue;
        };
        let o = &before.blocks[j];
        if o.model != a.model {
            return format!("{} \u{2192} {}", o.def().name, a.def().name);
        }
        if o.on != a.on {
            return format!("{} {}", a.def().name, if a.on { "on" } else { "off" });
        }
        let def = a.def();
        let changed: Vec<usize> = (0..def.knobs.len())
            .filter(|&k| (o.v[k] - a.v[k]).abs() > 1e-6)
            .collect();
        match changed.as_slice() {
            [] => {}
            [k] => {
                let kd = def.knobs[*k];
                return format!("{} {} \u{2192} {}", kd.label, kd.text(o.v[*k]), kd.text(a.v[*k]));
            }
            ks => return format!("{} knobs of {}", ks.len(), def.name),
        }
        if i != j {
            return format!("Moved {}", def.name);
        }
    }
    if (before.level_db - after.level_db).abs() > 1e-6 {
        return "Board level".into();
    }
    "The board".into()
}

/// A block as a miniature of its face (the chain strips): selected, under the mouse, off.
fn mini(p: &Painter, r: Rect, b: &Block, sel: bool, hot: bool, dim: bool) {
    let th = t();
    let r = if sel { r.translate(vec2(0.0, -2.0)) } else { r };
    let l = faces::layout(b, r, Detail::Board);
    faces::miniature(p, r, b, px(90.0));
    if dim {
        paint::wash(p, r, Color32::BLACK, 0.5);
    }
    if sel {
        p.rect_stroke(
            r.expand(3.0),
            l.radius + 3.0,
            Stroke::new(2.0, th.accent),
            egui::StrokeKind::Outside,
        );
    } else if hot {
        p.rect_stroke(
            r.expand(2.5),
            l.radius + 2.5,
            Stroke::new(1.2, th.accent.gamma_multiply(0.6)),
            egui::StrokeKind::Outside,
        );
    }
}

impl Rack {
    // -- which board ------------------------------------------------------------------------------------------------

    fn source(&self, st: &AppState) -> Source {
        let n = self.board.rig.as_ref().map(|r| r.tones.len()).unwrap_or(0);
        match self.board.pick {
            Some(Pick::Mine) => Source::Mine,
            Some(Pick::Tone(i)) if i < n => Source::Song(i),
            _ if n > 0 && pb::follow(&st.config) => Source::Song(self.board.song_tone.min(n - 1)),
            _ => Source::Mine,
        }
    }

    fn mine(&mut self, st: &AppState) -> &mut Board {
        if self.board.mine.is_none() {
            self.board.mine = Some(pb::my_board(&st.config));
        }
        self.board.mine.get_or_insert_with(Board::default)
    }

    /// The board the tile shows (and the engine plays, when the tone is the pedalboard).
    pub fn board_now(&mut self, st: &AppState) -> Board {
        match self.source(st) {
            Source::Song(i) => self
                .board
                .rig
                .as_ref()
                .and_then(|r| r.tones.get(i))
                .map(|t| t.board.clone())
                .unwrap_or_default(),
            Source::Mine => self.mine(st).clone(),
        }
    }

    /// What's playing, as words: "Be Yourself · wah solo" or "My board".
    fn board_title(&self, st: &AppState) -> String {
        match self.source(st) {
            Source::Song(i) => {
                let label = self
                    .board
                    .rig
                    .as_ref()
                    .and_then(|r| r.tones.get(i))
                    .map(|t| format!("{}{}", t.label, if t.edited { " (yours)" } else { "" }))
                    .unwrap_or_default();
                format!("{} \u{b7} {label}", self.song.title)
            }
            Source::Mine => self.title.clone().unwrap_or_else(|| "My board".into()),
        }
    }

    pub(crate) fn board_edit(&mut self, st: &mut AppState, f: impl FnOnce(&mut Board)) {
        // (an edit is heard: a board being auditioned, the room's A/B or a preview, steps aside, B8)
        if self.board.audition.take().is_some() {
            self.say(
                "Hearing your rig again",
                "  \u{b7}  you changed it, so the comparison stopped",
            );
        }
        match self.source(st) {
            Source::Song(i) => {
                if let Some(t) = self.board.rig.as_mut().and_then(|r| r.tones.get_mut(i)) {
                    f(&mut t.board);
                }
            }
            Source::Mine => f(self.mine(st)),
        }
        self.board.dirty = true;
        self.board_send(st);
    }

    /// Keep the edits (a song's tone in userdata.json, your board in config.json).
    pub(crate) fn board_commit(&mut self, st: &mut AppState) {
        if !self.board.dirty {
            return;
        }
        self.board.dirty = false;
        self.board.wheeled = None;
        match self.source(st) {
            Source::Song(i) => {
                if let Some(r) = self.board.rig.as_mut()
                    && let Some(b) = r.tones.get(i).map(|t| t.board.clone())
                {
                    r.keep(i, &b, &mut st.userdata);
                }
            }
            Source::Mine => {
                let b = self.mine(st).clone();
                pb::set_my_board(&mut st.config, &b);
                st.save_config();
            }
        }
        // the change goes on the board's history (to undo)
        let key = self.source_key(st);
        if self.board.kept_for == key {
            let now = self.board_now(st);
            if let Some(before) = self.board.kept.take()
                && before != now
            {
                let label = change_label(&before, &now);
                self.board.hist.push((before, label));
                if self.board.hist.len() > 100 {
                    self.board.hist.remove(0);
                }
                self.board.redo.clear();
            }
            self.board.kept = Some(now);
        }
    }

    /// Which board is shown, as a key (the edit history is per board).
    fn source_key(&self, st: &AppState) -> String {
        format!("{}|{:?}", self.board.rig_for, self.source(st))
    }

    pub(crate) fn board_mode(&self) -> bool {
        self.engine()
            .is_some_and(|e| matches!(e.tone(), ToneSpec::Board { .. }))
    }

    /// Give the engine the board the tile shows (only when it changed).
    pub(crate) fn board_send(&mut self, st: &AppState) {
        let Some(e) = self.engine().cloned() else {
            return;
        };
        if !matches!(e.tone(), ToneSpec::Board { .. }) {
            self.board.sent = None;
            return;
        }
        let mut want = match &self.board.audition {
            Some(b) => b.clone(),
            None => self.board_now(st),
        };
        if self.board.dry {
            for b in &mut want.blocks {
                b.on = false;
            }
        }
        if self.board.sent.as_ref() != Some(&want) {
            e.set_tone(ToneSpec::Board { board: want.clone() });
            self.board.sent = Some(want);
        }
    }

    /// Switch the guitar's tone to the pedalboard.
    pub(crate) fn board_use(&mut self, st: &mut AppState) {
        let b = self.board_now(st);
        let spec = ToneSpec::Board { board: b.clone() };
        crate::tones::store(&mut st.config, &spec);
        st.save_config();
        if let Some(e) = self.engine() {
            e.set_tone(spec);
        }
        self.board.sent = Some(b);
        self.say("Tone: Rig", "");
    }

    /// Screenshot mode: a starting board on "mine" and a block shown.
    pub(crate) fn board_shot(&mut self, st: &mut AppState, preset: Option<&str>, sel: Option<&str>) {
        // (the song's rig first, so the next frame doesn't start the tile over)
        self.board_update(st);
        if let Some(name) = preset
            && let Some((_, b)) = board::presets()
                .into_iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            self.board.pick = Some(Pick::Mine);
            self.board.mine = Some(b.renewed());
        }
        // QUICKRS_BOARD_CHAIN="amp_crunch,cab_4x12,...": a board of exactly these models (to see faces side by side)
        if let Ok(chain) = std::env::var("QUICKRS_BOARD_CHAIN") {
            let mut b = Board::default();
            for id in chain.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if let Some(blk) = Block::of(id) {
                    b.blocks.push(blk);
                }
            }
            self.board.pick = Some(Pick::Mine);
            self.board.mine = Some(b.renewed());
        }
        // (P31: the file blocks get the library's newest capture and IR; QUICKRS_BOARD_CAPTURES opens the dialog)
        if crate::captures::ON {
            crate::captures::with(|l| {
                if l.items.is_empty() {
                    *l = crate::captures::Library::open(&st.data.root);
                }
            });
            let newest = |k| {
                crate::captures::with(|l| l.of(k).max_by(|a, c| a.added.total_cmp(&c.added)).map(|i| i.id))
                    .unwrap_or(0)
            };
            let (n, i) = (
                newest(crate::captures::Kind::Nam),
                newest(crate::captures::Kind::Ir),
            );
            if let Some(b) = self.board.mine.as_mut() {
                for blk in &mut b.blocks {
                    match blk.def().id {
                        "amp_nam" => {
                            blk.set_asset(n);
                        }
                        "cab_ir" => {
                            blk.set_asset(i);
                        }
                        _ => {}
                    }
                }
                if let Some(u) = b.blocks.iter().find(|x| x.def().id == "amp_nam").map(|x| x.uid) {
                    captures::engage(b, u, captures::has_cab);
                }
            }
        }
        if let Some(id) = sel {
            let b = self.board_now(st);
            self.board.sel = b.blocks.iter().find(|x| x.def().id == id).map(|x| x.uid);
            // (QUICKRS_BOARD_BAR: the block's bar under the board, not its editor)
            self.board.editing = self.board.sel.is_some() && std::env::var("QUICKRS_BOARD_BAR").is_err();
        }
        // (QUICKRS_BOARD_NAM: press NAM on the shown board, as a click on the switch would)
        if crate::captures::ON && std::env::var("QUICKRS_BOARD_NAM").is_ok() {
            self.use_amp(st, true);
            if let Some(id) = sel {
                let b = self.board_now(st);
                self.board.sel = b.blocks.iter().find(|x| x.def().id == id).map(|x| x.uid);
            }
        }
        if crate::captures::ON
            && let Ok(k) = std::env::var("QUICKRS_BOARD_CAPTURES")
        {
            let kind = match k.as_str() {
                "ir" => crate::captures::Kind::Ir,
                "pedal" => crate::captures::Kind::Pedal,
                _ => crate::captures::Kind::Nam,
            };
            self.board.captures = Some(captures::Dialog::new(kind, self.board.sel));
        }
        self.board.shot_hover = std::env::var("QUICKRS_BOARD_HOVER").ok();
        // QUICKRS_BOARD_TAB=back: the editor on the amp's back panel (a drive's insides)
        self.board.back = std::env::var("QUICKRS_BOARD_TAB").is_ok_and(|t| t.eq_ignore_ascii_case("back"));
        self.board.sent = None;
        self.board_send(st);
    }

    /// Leaving the player: your own board goes back on (when the tone is the pedalboard).
    pub fn board_leave(&mut self, st: &mut AppState) {
        self.board_commit(st);
        if self.board_mode() {
            let mine = pb::my_board(&st.config);
            if let Some(e) = self.engine() {
                e.set_tone(ToneSpec::Board { board: mine });
            }
        }
    }

    /// The interface clipped in the last moments (the input meter shows CLIP).
    fn clipping(&self) -> bool {
        self.board.clip_until.is_some_and(|t| Instant::now() < t)
    }

    /// Every frame: the song's rig, following its tone changes, the engine in step, the level check.
    pub fn board_update(&mut self, st: &mut AppState) {
        let key = format!("{}|{}", self.song.sid, self.song.part_name);
        if self.board.rig_for != key {
            self.board_commit(st);
            self.board.rig_for = key;
            self.board.rig = SongRig::new(&self.song.arr, &self.song.sid, &self.song.part_name, &st.userdata);
            self.sounds_forget();
            // (P41.1: "Always my sound" lasts across songs)
            self.board.pick = pb::always_mine(&st.config).then_some(Pick::Mine);
            self.board.sel = None;
            self.board.picker = None;
            let t = self.song.pos;
            self.board.song_tone = self.board.rig.as_ref().map(|r| r.at(t)).unwrap_or(0);
        }
        if self.board.pick.is_none() && pb::follow(&st.config) {
            let t = self.song.pos;
            if let Some(i) = self.board.rig.as_ref().map(|r| r.at(t))
                && i != self.board.song_tone
            {
                self.board_commit(st);
                self.board.song_tone = i;
                self.board.sel = None;
            }
        }
        // a drag or move whose face wasn't drawn to see the button come up: ended, its edit kept (B1). A move whose
        // strip drew it last pass is left to that strip, which drops it where it was let go (P38.1: ending it here,
        // before any strip ran, threw every drop away)
        let up = !st.ctx.input(|i| i.pointer.any_down());
        let pass = st.ctx.cumulative_pass_nr();
        let strip_gone = self.board.moving.is_some_and(|m| m.pass + 1 < pass);
        if up && (self.board.drag.is_some() || strip_gone) {
            self.board.drag = None;
            self.board.moving = None;
            self.board_commit(st);
        }
        if self.board.drag.is_none()
            && self
                .board
                .wheeled
                .is_some_and(|w| w.elapsed() > Duration::from_millis(700))
        {
            self.board_commit(st);
        }
        // a selection that went away: the amp, else the first block
        let b = self.board_now(st);
        if self.board.sel.is_none_or(|u| b.index_of(u).is_none()) {
            self.board.sel = b
                .first(Kind::Amp)
                .or((!b.blocks.is_empty()).then_some(0))
                .map(|i| b.blocks[i].uid);
        }
        // a board shown for the first time: what it is now is where undo stops
        let key = self.source_key(st);
        if self.board.kept_for != key {
            self.board.kept_for = key;
            self.board.kept = Some(b);
            self.board.hist.clear();
            self.board.redo.clear();
            self.board.ab = None;
            self.board.last_start = None;
        }
        self.board_send(st);
        // the interface clipping while the board plays: CLIP held on the input meter, and said every 30 s at most
        if let Some(clips) = self.engine().map(|e| e.meters().clips) {
            if self.board.clips_seen.is_some_and(|c| clips > c) && self.board_mode() {
                self.board.clip_until = Some(Instant::now() + Duration::from_millis(2500));
                if self
                    .board
                    .clip_said
                    .is_none_or(|t| t.elapsed() > Duration::from_secs(30))
                {
                    self.board.clip_said = Some(Instant::now());
                    self.say(
                        "Your input is clipping",
                        "  \u{b7}  turn the gain on your interface down a little",
                    );
                }
            }
            self.board.clips_seen = Some(clips);
        }
        if let Some((until, peak)) = self.board.level {
            let now = self.engine().map(|e| e.meters().peak).unwrap_or(0.0);
            let peak = peak.max(now);
            if Instant::now() >= until {
                self.board.level = None;
                let db = 20.0 * peak.max(1e-6).log10();
                let note = if peak < 1e-4 {
                    "Nothing heard: is your instrument plugged in, the INST button on?".to_string()
                } else if db > -1.0 {
                    format!(
                        "Peaks at {db:.0} dB: clipping, turn the interface's gain down until its ring stays green"
                    )
                } else if db > -4.0 {
                    format!(
                        "Peaks at {db:.0} dB: a little hot, a touch less gain (amber only on your hardest notes)"
                    )
                } else if db < -22.0 {
                    format!("Peaks at {db:.0} dB: quiet, turn the interface's gain up")
                } else if db < -14.0 {
                    format!("Peaks at {db:.0} dB: a little quiet, a touch more gain")
                } else {
                    format!("Peaks at {db:.0} dB: just right")
                };
                self.board.level_note = Some(note);
            } else {
                self.board.level = Some((until, peak));
            }
        }
    }

    // -- edits --------------------------------------------------------------------------------------------------------

    pub(crate) fn set_knob(&mut self, st: &mut AppState, uid: u32, knob: usize, v: f32) {
        self.board_edit(st, |b| {
            if let Some(i) = b.index_of(uid)
                && let Some(kd) = b.blocks[i].def().knobs.get(knob)
            {
                let blk = b.blocks[i];
                // (a capture's Gain steps through its unit's captures: it snaps to them, and with one capture it
                // has nothing to step to, P37.6c)
                let v = if blk.def().file && kd.key == "gain" {
                    qrs_audio::board::nam::ladder_snap(blk.asset, v)
                } else {
                    v
                };
                b.blocks[i].v[knob] = kd.clamp(v);
            }
        });
    }

    pub(crate) fn toggle_block(&mut self, st: &mut AppState, uid: u32) {
        self.board_edit(st, |b| {
            if let Some(i) = b.index_of(uid) {
                b.blocks[i].on = !b.blocks[i].on;
                // (an amp or cab switched on takes over from the others: one amp plays, P31)
                if crate::captures::ON {
                    captures::engage(b, uid, captures::has_cab);
                }
            }
        });
        self.board_commit(st);
    }

    /// Swap a block's model for another (its knobs carried over by key). An amp for an amp keeps the sound's level
    /// (P37.6c, D139): the board's level moves by the measured loudness of the old amp minus the new one at their
    /// gains (`loudness::swap_trim`). Returns that move, dB.
    pub(crate) fn change_model(&mut self, st: &mut AppState, uid: u32, model: usize) -> f32 {
        let mut moved = 0.0;
        self.board_edit(st, |b| {
            if let Some(i) = b.index_of(uid) {
                let old = b.blocks[i];
                let new = old.changed_to(model);
                let t = qrs_audio::board::loudness::swap_trim(&old, &new);
                if t.abs() >= 0.25 {
                    let to = ((b.level_db + t) * 2.0).round() / 2.0;
                    let to = to.clamp(-24.0, 12.0);
                    moved = to - b.level_db;
                    b.level_db = to;
                }
                b.blocks[i] = new;
            }
        });
        self.board_commit(st);
        moved
    }

    /// Step to the next (or previous) model of the block's kind.
    fn cycle_model(&mut self, st: &mut AppState, uid: u32, dir: i32) {
        let b = self.board_now(st);
        let Some(i) = b.index_of(uid) else { return };
        let blk = b.blocks[i];
        // (a capture steps through your captures of its kind, a model through our models: neither turns into the
        // other and loses its file, B15)
        if blk.def().file {
            let kind = captures::kind_of(&blk);
            let ids: Vec<u32> = crate::captures::with(|l| {
                let mut v: Vec<&crate::captures::Item> = l.of(kind).collect();
                v.sort_by_key(|x| x.name.to_lowercase());
                v.into_iter().map(|x| x.id).collect()
            });
            if ids.is_empty() {
                self.choose_file(blk);
                return;
            }
            let at = ids
                .iter()
                .position(|&x| x == blk.asset)
                .map(|p| p as i32)
                .unwrap_or(-1);
            let next = ids[((at + dir).rem_euclid(ids.len() as i32)) as usize];
            self.set_asset(st, uid, next);
            return;
        }
        let kind = blk.def().kind;
        let same: Vec<usize> = catalog::of_kind(kind).map(|(j, _)| j).collect();
        let at = same.iter().position(|&j| j == b.blocks[i].model).unwrap_or(0) as i32;
        let n = same.len() as i32;
        let next = same[((at + dir).rem_euclid(n)) as usize];
        let _ = self.change_model(st, uid, next);
    }

    pub(crate) fn add_block(&mut self, st: &mut AppState, at: usize, model: usize) {
        if self.board_now(st).blocks.len() >= MAX_BLOCKS {
            self.say("The board is full", "  \u{b7}  12 blocks at most");
            return;
        }
        let mut blk = Block::new(model);
        if let Some(v) = pb::model_default(&st.config, MODELS[model].id) {
            blk.v = v;
        } else if MODELS[model].id == "gate" {
            // (P41.1, D162-D163: a gentle gate keyed on the input, its threshold over the input's measured noise)
            blk.v = board::new_gate(false).v;
        } else if st.instrument().is_bass() {
            // (a bass player's drive or fuzz: Blend at half, Clean low at 120 Hz, P37.2)
            blk = blk.for_bass();
        }
        let uid = blk.uid;
        self.board_edit(st, |b| {
            let at = at.min(b.blocks.len());
            b.blocks.insert(at, blk);
            // (a new amp or cab plays in place of the one there: P31)
            if crate::captures::ON {
                captures::engage(b, uid, captures::has_cab);
            }
        });
        self.board_commit(st);
        self.board.sel = Some(uid);
        self.board.picker = None;
        self.say(format!("Added {}", MODELS[model].shown_name()), "");
    }

    /// Add a capture (or IR) at `at` in one edit, one undo step (B14: it was add_block then set_asset, and at 12
    /// blocks the capture landed on the selected block instead).
    pub(crate) fn add_capture(&mut self, st: &mut AppState, at: usize, asset: u32) {
        let kind =
            crate::captures::with(|l| l.get(asset).map(|i| i.kind)).unwrap_or(crate::captures::Kind::Nam);
        let Some(mut blk) = Block::of(kind.model()) else {
            return;
        };
        if self.board_now(st).blocks.len() >= MAX_BLOCKS {
            self.say("The board is full", "  \u{b7}  12 blocks at most");
            return;
        }
        blk.set_asset(asset);
        let uid = blk.uid;
        let ir = crate::captures::with(|l| l.get(asset).and_then(|i| i.ir));
        self.board_edit(st, |b| {
            let at = at.min(b.blocks.len());
            b.blocks.insert(at, blk);
            if let Some(ir) = ir {
                captures::with_ir(b, uid, ir);
            }
            captures::engage(b, uid, captures::has_cab);
        });
        self.board_commit(st);
        self.board.sel = Some(uid);
        self.board.picker = None;
        if let Some(n) = crate::captures::name_of(asset) {
            self.say(format!("Added {n}"), "");
        }
    }

    pub(crate) fn remove_block(&mut self, st: &mut AppState, uid: u32) {
        let b = self.board_now(st);
        let Some(i) = b.index_of(uid) else { return };
        let name = b.blocks[i].def().name;
        self.board_edit(st, |b| {
            b.blocks.remove(i);
        });
        self.board_commit(st);
        let b = self.board_now(st);
        self.board.sel = b
            .blocks
            .get(i.min(b.blocks.len().saturating_sub(1)))
            .map(|x| x.uid);
        self.say(format!("Removed {name}"), "  \u{b7}  Undo (Ctrl Z)");
    }

    pub(crate) fn move_block(&mut self, st: &mut AppState, uid: u32, to: usize) {
        self.board_edit(st, |b| {
            if let Some(i) = b.index_of(uid) {
                let blk = b.blocks.remove(i);
                let to = to.min(b.blocks.len());
                b.blocks.insert(to, blk);
            }
        });
    }

    /// Put a whole board in (a starting board, a saved one); the one it replaces can be had back.
    pub(crate) fn load_board(&mut self, st: &mut AppState, name: &str, b: Board) {
        let before = self.board_now(st);
        self.board.undo = Some((before, self.board_title(st)));
        let fresh = b.renewed();
        self.board_edit(st, |cur| *cur = fresh);
        self.board_commit(st);
        self.board.sel = None;
        self.say(format!("{name} board on"), "  \u{b7}  Undo in the big view");
    }

    fn pick_tone(&mut self, st: &mut AppState, pick: Pick) {
        self.board_commit(st);
        if matches!(pick, Pick::Tone(_)) && pb::always_mine(&st.config) {
            pb::set_always_mine(&mut st.config, false);
            st.save_config();
        }
        self.board.pick = Some(pick);
        self.board.sel = None;
        self.board.picker = None;
    }

    pub(crate) fn set_follow(&mut self, st: &mut AppState, on: bool) {
        self.board_commit(st);
        pb::set_follow(&mut st.config, on);
        pb::set_always_mine(&mut st.config, false);
        st.save_config();
        self.board.pick = None;
        self.board.sel = None;
        if on {
            let t = self.song.pos;
            self.board.song_tone = self.board.rig.as_ref().map(|r| r.at(t)).unwrap_or(0);
        }
    }

    /// Once a frame, whoever draws first (B4): the wheel's places start empty (it was never cleared in the room, so
    /// the wheel hit stale places and the list grew every frame).
    /// The song's tones read again (after the amp room edited them in the song's context), `pick` the tone the room
    /// left playing; the engine gets the board again.
    pub(crate) fn reload_song(&mut self, st: &AppState, pick: Option<Pick>) {
        self.board.rig = SongRig::new(&self.song.arr, &self.song.sid, &self.song.part_name, &st.userdata);
        self.board.rig_for = format!("{}|{}", self.song.sid, self.song.part_name);
        self.board.pick = pick;
        self.board.mine = None;
        self.board.sel = None;
        self.board.sent = None;
        self.board.kept_for = String::new();
    }

    /// The amp room in a song's context: the song's tones read for it, `pick` the tone playing in the player.
    pub(crate) fn for_song(&mut self, st: &AppState, song: SongCtx, pick: Option<Pick>, song_tone: usize) {
        self.song = song;
        self.reload_song(st, pick);
        self.board.song_tone = song_tone;
    }

    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        let nr = ctx.cumulative_pass_nr();
        if self.board.frame == nr {
            return;
        }
        self.board.frame = nr;
        self.board.wheel.clear();
    }

    /// How many places the wheel can turn this frame (tests: it stays bounded, B4).
    #[cfg(test)]
    pub(crate) fn wheel_len(&self) -> usize {
        self.board.wheel.len()
    }

    /// The wheel over a control turns it (true: the wheel was used here).
    pub fn board_wheel(&mut self, st: &mut AppState, pos: Pos2, notches: f32, shift: bool) -> bool {
        if !self.board.open && self.sounds_wheel(pos, notches) {
            return true;
        }
        let Some(spot) = self
            .board
            .wheel
            .iter()
            .rev()
            .find(|w| w.rect.contains(pos))
            .copied()
        else {
            return false;
        };
        let b = self.board_now(st);
        let Some(i) = b.index_of(spot.uid) else {
            return false;
        };
        let blk = b.blocks[i];
        let Some(kd) = blk.def().knobs.get(spot.knob) else {
            return false;
        };
        let v = match kd.unit {
            Unit::Choice(c) => {
                let n = c.len() as f32;
                (blk.v[spot.knob] + notches.signum()).rem_euclid(n)
            }
            _ => kd.from_u(kd.to_u(blk.v[spot.knob]) + notches * if shift { 0.005 } else { 0.025 }),
        };
        self.set_knob(st, spot.uid, spot.knob, v);
        self.board.wheeled = Some(Instant::now());
        true
    }

    /// Typing a name for Save as (true: the event was used).
    pub fn board_key(&mut self, st: &mut AppState, ev: &Event) -> bool {
        let Some(name) = self.board.naming.as_mut() else {
            // (the sounds list, while it's open and the big view isn't)
            return !self.board.open && self.sounds_key(st, ev);
        };
        match ev {
            Event::Text(s) => {
                if name.chars().count() < 40 {
                    name.push_str(s);
                }
                true
            }
            Event::Key {
                key: Key::Backspace,
                pressed: true,
                ..
            } => {
                name.pop();
                true
            }
            Event::Key {
                key: Key::Escape,
                pressed: true,
                ..
            } => {
                self.board.naming = None;
                true
            }
            Event::Key {
                key: Key::Enter,
                pressed: true,
                ..
            } => {
                let n = name.trim().to_string();
                self.board.naming = None;
                if !n.is_empty() {
                    let b = self.board_now(st);
                    pb::save_as(&mut st.config, &n, &b);
                    st.save_config();
                    self.say(format!("Saved \u{201c}{n}\u{201d}"), "  \u{b7}  in Presets");
                }
                true
            }
            _ => false,
        }
    }

    // -- a block's face, played with the mouse ----------------------------------------------------------------------

    /// A block's face anywhere (the room's open unit): played as on the tile, `tag` its own (B1). Returns the knob
    /// under the mouse or being turned.
    pub(crate) fn unit_face(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        blk: Block,
        r: Rect,
        tag: &'static str,
    ) -> Option<usize> {
        self.face_ui(ui, st, blk, r, Detail::Full, tag).map(|h| h.knob)
    }

    /// A block's face, played with the mouse. Returns the control under the mouse or being turned.
    fn face_ui(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        blk: Block,
        r: Rect,
        detail: Detail,
        tag: &'static str,
    ) -> Option<HotCtl> {
        let ctx = ui.ctx().clone();
        let def = blk.def();
        // (every control on a face is real: a capture's skin only where each of its controls maps to a real knob,
        // else the brushed-silver unit with the block's own knobs; the layout's indices are the block's, B3/D101)
        let l = faces::layout(&blk, r, detail);
        // (under an overlay: drawn only, so a hidden copy neither ends a drag nor takes the wheel, B1/B4)
        if self.board.inert {
            let now = self.board_now(st);
            let shown = now.index_of(blk.uid).map(|i| now.blocks[i]).unwrap_or(blk);
            faces::draw(ui.painter(), r, &shown, &l, detail, Hot::default());
            return None;
        }
        let mut hot = Hot::default();
        let mut shapes: Vec<(usize, Rect, Ctl)> = Vec::new();
        for &(k, c, d) in l.knobs.iter().chain(&l.extra) {
            shapes.push((k, Rect::from_center_size(c, Vec2::splat(d)), Ctl::Knob));
        }
        for &(k, tr) in &l.faders {
            let across = def.face == Face::Cab;
            shapes.push((k, tr, Ctl::Fader { across }));
        }
        let shift = ctx.input(|i| i.modifiers.shift);
        let spots: Vec<(usize, Rect, bool, f32)> = l
            .knobs
            .iter()
            .chain(&l.extra)
            .map(|&(k, c, d)| (k, Rect::from_center_size(c, Vec2::splat(d + 10.0)), false, 160.0))
            .chain(l.faders.iter().map(|&(k, tr)| {
                let across = def.face == Face::Cab;
                let hit = if across {
                    tr.expand2(vec2(8.0, 9.0))
                } else {
                    tr.expand2(vec2(10.0, 7.0))
                };
                (k, hit, across, if across { tr.width() } else { tr.height() })
            }))
            .collect();
        for &(k, cr) in &l.choices {
            shapes.push((k, cr, Ctl::Choice));
        }
        for (k, hit, across, len) in spots {
            self.board.wheel.push(WheelSpot {
                uid: blk.uid,
                knob: k,
                rect: hit,
            });
            let resp = interact(ui, hit, (tag, "knob", blk.uid, k), Sense::click_and_drag());
            let kd = def.knobs[k];
            if resp.hovered() || resp.dragged() {
                hot.hover = Some(k);
                ctx.set_cursor_icon(if across {
                    CursorIcon::ResizeHorizontal
                } else {
                    CursorIcon::ResizeVertical
                });
            }
            if resp.double_clicked() {
                self.board.drag = None;
                self.set_knob(st, blk.uid, k, kd.def);
                self.board_commit(st);
                continue;
            }
            let any_down = ctx.input(|i| i.pointer.any_down());
            let pos = ctx.input(|i| i.pointer.interact_pos());
            let act = drag_step(
                self.board.drag.as_ref(),
                tag,
                blk.uid,
                k,
                resp.is_pointer_button_down_on(),
                pos,
                any_down,
            );
            match act {
                DragAct::Idle => {}
                DragAct::Start => {
                    let pos = pos.unwrap_or(hit.center());
                    // a click on a fader's track jumps there
                    let u0 = if def.face == Face::Graphic || across {
                        let tr = l.faders.iter().find(|f| f.0 == k).map(|f| f.1).unwrap_or(hit);
                        if across {
                            ((pos.x - tr.min.x) / tr.width()).clamp(0.0, 1.0)
                        } else {
                            ((tr.max.y - pos.y) / tr.height()).clamp(0.0, 1.0)
                        }
                    } else {
                        kd.to_u(blk.v[k])
                    };
                    if def.face == Face::Graphic || across {
                        self.set_knob(st, blk.uid, k, kd.from_u(u0));
                    }
                    self.board.drag = Some(KnobDrag {
                        tag,
                        uid: blk.uid,
                        knob: k,
                        start: pos,
                        u0,
                        across,
                        len,
                    });
                    hot.active = Some(k);
                }
                DragAct::Turn(moved) => {
                    hot.active = Some(k);
                    if let Some(d) = self.board.drag {
                        let fine = if shift { 0.25 } else { 1.0 };
                        let u = (d.u0 + moved / d.len.max(20.0) * fine).clamp(0.0, 1.0);
                        let v = kd.from_u(u);
                        if (v - blk.v[k]).abs() > 1e-6 {
                            self.set_knob(st, blk.uid, k, v);
                        }
                    }
                }
                DragAct::End => {
                    self.board.drag = None;
                    self.board_commit(st);
                }
            }
        }
        for &(k, cr) in &l.choices {
            self.board.wheel.push(WheelSpot {
                uid: blk.uid,
                knob: k,
                rect: cr,
            });
            let resp = interact(ui, cr.expand(3.0), (tag, "choice", blk.uid, k), Sense::click());
            if resp.hovered() {
                hot.hover = Some(k);
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked()
                && let Unit::Choice(opts) = def.knobs[k].unit
            {
                let n = opts.len();
                // (an amp's switch steps through its positions: Bright Off/On today, any count tomorrow, B16)
                let stepper = faces::silver::choice_stepped(&ctx, &blk, k, cr, r.height());
                let v = if faces::face_of(&blk) == Face::Amp {
                    (blk.v[k].round() + 1.0).rem_euclid(n as f32)
                } else if stepper {
                    // (a stepper: its left half steps back)
                    let x = resp.interact_pointer_pos().map(|p| p.x).unwrap_or(cr.max.x);
                    let step = if x < cr.center().x { -1.0 } else { 1.0 };
                    (blk.v[k].round() + step).rem_euclid(n as f32)
                } else {
                    let x = resp.interact_pointer_pos().map(|p| p.x).unwrap_or(cr.center().x);
                    (((x - cr.min.x) / cr.width() * n as f32).floor()).clamp(0.0, n as f32 - 1.0)
                };
                self.set_knob(st, blk.uid, k, v);
                self.board_commit(st);
            }
        }
        if l.switch.is_positive() {
            let resp = interact(ui, l.switch.expand(4.0), (tag, "switch", blk.uid), Sense::click());
            if resp.hovered() {
                hot.switch = true;
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.toggle_block(st, blk.uid);
            }
        }
        // (screenshots: a knob shown as hovered in the editor)
        if tag == "editor"
            && hot.hover.is_none()
            && hot.active.is_none()
            && let Some(key) = self.board.shot_hover.as_deref()
        {
            hot.hover = def.knob_index(key).filter(|k| shapes.iter().any(|s| s.0 == *k));
        }
        // the face drawn with the values as they now are
        let now = self.board_now(st);
        let shown = now.index_of(blk.uid).map(|i| now.blocks[i]).unwrap_or(blk);
        faces::draw(ui.painter(), r, &shown, &l, detail, hot);
        let k = hot.active.or(hot.hover)?;
        let &(_, rect, ctl) = shapes.iter().find(|s| s.0 == k)?;
        Some(HotCtl { knob: k, rect, ctl })
    }

    // -- the chain strip ------------------------------------------------------------------------------------------

    /// The chain in `well`, each block a miniature of its face; with `names`, each named under it (the editor's
    /// strip).
    pub(crate) fn chain_ui(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        well: Rect,
        tag: &'static str,
        names: bool,
    ) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let b = self.board_now(st);
        let name_h = if names { px(15.0) } else { 0.0 };
        let h0 = (well.height() - px(14.0) - name_h).max(px(20.0));
        let cy = well.min.y + px(7.0) + h0 / 2.0;
        let jack = |x: f32, right: bool| {
            let c = pos2(x, cy);
            p.rect_filled(
                Rect::from_center_size(c, vec2(px(8.0), px(8.0))),
                2.0,
                Color32::from_rgb(109, 122, 135),
            );
            let tip = if right {
                c + vec2(px(6.0), 0.0)
            } else {
                c - vec2(px(6.0), 0.0)
            };
            p.rect_filled(
                Rect::from_center_size(tip, vec2(px(4.0), px(3.0))),
                1.0,
                Color32::from_rgb(154, 166, 178),
            );
        };
        jack(well.min.x + px(12.0), true);
        jack(well.max.x - px(12.0), false);
        let gap = px(if names { 16.0 } else { 9.0 });
        let add_w = px(26.0);
        let widths: Vec<f32> = b
            .blocks
            .iter()
            .map(|x| {
                let s = faces::board_size(x);
                h0 * s.x / s.y.max(1.0)
            })
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + gap * (b.blocks.len() as f32 + 1.0) + add_w;
        let avail = well.width() - px(44.0);
        let k = (avail / total).min(1.0);
        let h = h0 * k;
        // (room to spare: the blocks spread out, up to four times the gap)
        let gk = if k >= 1.0 {
            (gap + (avail - total) / (b.blocks.len() as f32 + 1.0)).min(gap * 4.0)
        } else {
            gap * k
        };
        let mut x = well.min.x + px(22.0);
        let cable = |x0: f32, x1: f32| {
            p.line_segment(
                [pos2(x0, cy), pos2(x1, cy)],
                Stroke::new(2.0, Color32::from_rgb(71, 89, 107)),
            );
        };
        let mut chips: Vec<(u32, Rect)> = Vec::new();
        cable(well.min.x + px(16.0), x);
        for (blk, w) in b.blocks.iter().zip(&widths) {
            x += gk;
            let r = Rect::from_min_size(pos2(x, cy - h / 2.0), vec2(w * k, h));
            cable(x - gk, x);
            chips.push((blk.uid, r));
            x += w * k;
        }
        self.board.chips = chips.clone();
        // + adds after the shown block (or at the end)
        x += gk;
        cable(x - gk, x);
        let add = Rect::from_center_size(pos2(x + add_w * 0.5, cy), Vec2::splat(add_w));
        cable(add.max.x, well.max.x - px(16.0));
        // the chip being dragged follows the mouse; where it would land shows as a gap in the order
        let moving = self.board.moving;
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let mut lifted: Option<(Rect, Block, bool)> = None;
        for (i, (uid, r)) in chips.iter().enumerate() {
            let blk = b.blocks[i];
            // (owner, P37.6c: the remove key at the top right of the hovered unit, the "i" at its bottom right, apart;
            // the key is answered before the unit, and a press on it never selects or drags the unit. Shown while
            // the pointer is over the unit or the key itself, so reaching for it doesn't hide it.)
            let (xr, _) = chip_keys(*r);
            let near = pointer.is_some_and(|q| r.contains(q) || xr.contains(q));
            let on_x = pointer.is_some_and(|q| xr.contains(q));
            let show_x = near && moving.is_none() && !self.board.inert && !b.blocks.is_empty();
            if show_x {
                let xresp = interact(ui, xr, (tag, "remove", *uid), Sense::click());
                if xresp.clicked() {
                    self.remove_block(st, *uid);
                    return;
                }
            }
            let sense = if self.board.inert || (show_x && on_x) {
                Sense::hover()
            } else {
                Sense::click_and_drag()
            };
            let resp = interact(ui, *r, (tag, "chip", *uid), sense);
            let sel = self.board.sel == Some(*uid);
            let hovered = resp.hovered();
            if hovered {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                self.tip = Some((
                    format!("{} \u{b7} {}", blk.def().shown_name(), after_line(&blk)),
                    if blk.on {
                        "click to show \u{b7} double-click: off \u{b7} drag: reorder".into()
                    } else {
                        "off \u{b7} double-click: on".into()
                    },
                    pos2(r.center().x, well.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.double_clicked() {
                self.toggle_block(st, *uid);
            } else if resp.clicked() {
                self.board.sel = Some(*uid);
                self.board.picker = None;
            }
            if resp.drag_started()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                self.board.moving = Some(MoveDrag {
                    tag,
                    uid: *uid,
                    grab: pos.x - r.min.x,
                    x: r.min.x,
                    moved: false,
                    pass: ctx.cumulative_pass_nr(),
                });
            }
            if let Some(m) = self.board.moving.as_mut()
                && m.uid == *uid
                && m.tag == tag
            {
                m.pass = ctx.cumulative_pass_nr();
                if ctx.input(|i| i.pointer.any_down()) {
                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        m.x = pos.x - m.grab;
                        m.moved = m.moved || resp.dragged();
                    }
                } else {
                    // let go: the chips whose centres are left of the dropped one's come before it
                    let centre = m.x + r.width() / 2.0;
                    let to = drop_index(&chips, *uid, centre);
                    let moved = m.moved;
                    self.board.moving = None;
                    if moved && b.index_of(*uid) != Some(to) {
                        self.move_block(st, *uid, to);
                        self.board_commit(st);
                    }
                }
            }
            match moving {
                Some(m) if m.uid == *uid && m.moved && m.tag == tag => {
                    // (lifted: drawn last, over its neighbours, with where it lands marked; its place a hollow)
                    ctx.set_cursor_icon(CursorIcon::Grabbing);
                    lifted = Some((
                        Rect::from_min_size(pos2(m.x, r.min.y - px(4.0)), r.size()),
                        blk,
                        sel,
                    ));
                    paint::inset(&p, *r, Some(px(6.0)), 1.0, Fill::Base);
                }
                _ => mini(&p, *r, &blk, sel, hovered, !blk.on || self.board.dry),
            }
            if names {
                // (a capture by its own short name)
                let name = match Rack::file_name(&blk) {
                    Some(n) if blk.def().file => crate::room::rig::short_capture_name(&n),
                    _ => blk.def().name.to_string(),
                };
                text::draw(
                    &p,
                    &name,
                    pos2(r.center().x, well.max.y - px(10.0)),
                    Align2::CENTER_CENTER,
                    Style::new(9.5, 800, if sel { th.accent_ink } else { th.caption }),
                    Some(r.width() + gk - px(2.0)),
                );
            }
            // the remove key (answered above, drawn over the unit)
            if show_x {
                let col = if on_x {
                    Color32::from_rgb(0xd2, 0x3a, 0x32)
                } else {
                    Color32::from_rgb(0x8e, 0x22, 0x1c)
                };
                p.circle_filled(xr.center(), xr.width() * 0.5, col);
                p.circle_stroke(
                    xr.center(),
                    xr.width() * 0.5,
                    Stroke::new(1.0, Color32::from_white_alpha(60)),
                );
                let s = xr.width() * 0.2;
                let st_ = Stroke::new(1.6, Color32::WHITE);
                let c = xr.center();
                p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], st_);
                p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], st_);
                if on_x {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                    self.tip = Some((
                        format!("Remove {}", blk.def().shown_name()),
                        "Ctrl Z brings it back".into(),
                        pos2(xr.center().x, xr.min.y - px(4.0)),
                        Align2::CENTER_BOTTOM,
                    ));
                }
            }
            let _ = sel;
        }
        if let Some((lr, blk, sel)) = lifted {
            let y = (well.min.y + px(5.0), cy + h / 2.0 + px(3.0));
            drop_marker(&p, &chips, blk.uid, lr.center().x, gk, y);
            paint::floating(&p, lr, Some(px(6.0)));
            mini(&p, lr, &blk, sel, true, !blk.on || self.board.dry);
        }
        let resp = interact(ui, add, (tag, "add"), Sense::click());
        let hot = resp.hovered();
        p.rect_stroke(
            add,
            px(6.0),
            Stroke::new(
                1.5,
                if hot {
                    th.accent
                } else {
                    Color32::from_rgb(71, 89, 107)
                },
            ),
            egui::StrokeKind::Inside,
        );
        widgets::draw_round_button(
            &p,
            Rect::from_center_size(add.center(), Vec2::splat(px(18.0))),
            Icon::Plus,
            9.0,
            RoundKind::Ivory,
            false,
            hot,
            false,
        );
        if hot {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() && !self.board.inert {
            let at = self
                .board
                .sel
                .and_then(|u| b.index_of(u))
                .map(|i| i + 1)
                .unwrap_or(b.blocks.len());
            self.board.picker = Some(Picker::Kinds { at });
        }
    }

    // -- the tile ---------------------------------------------------------------------------------------------------

    /// The Pedalboard tile at (x, y), `w` wide. Returns its bottom.
    pub(super) fn draw_board_tile(
        &mut self,
        ui: &Ui,
        st: &mut AppState,
        x: f32,
        y: f32,
        w: f32,
        lifting: bool,
    ) -> f32 {
        self.begin_frame(ui.ctx());
        // (the big view over it: the tile is drawn only, B1)
        let inert_was = self.board.inert;
        self.board.inert = inert_was || self.board.open;
        let h = self.tile_body(ui, st, x, y, w, lifting);
        self.board.inert = inert_was;
        h
    }

    fn tile_body(&mut self, ui: &Ui, st: &mut AppState, x: f32, y: f32, w: f32, lifting: bool) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let (pad_x, pad_y) = (px(10.0), px(12.0));
        let (head, tones_h, strip_h, title_h, face_h, foot_h) =
            (px(24.0), px(30.0), px(74.0), px(22.0), px(178.0), px(32.0));
        // (P41.3: the line that says where the sound comes from, and the rig in plain words)
        let src_h = px(36.0);
        let gap = px(10.0);
        let height = pad_y
            + head
            + px(8.0)
            + tones_h
            + src_h
            + gap
            + strip_h
            + gap
            + title_h
            + px(6.0)
            + face_h
            + gap
            + foot_h
            + pad_y;
        let card = Rect::from_min_size(pos2(x, y), vec2(w, height));
        self.board.tile_rect = card;
        super::tiles::tile_card(&p, card, lifting);
        let inner_x0 = card.min.x + pad_x;
        let inner_x1 = card.max.x - pad_x;
        // header: the title, what's playing, the expand key
        let hy = card.min.y + pad_y + head / 2.0;
        let tt = caps(
            &p,
            "RIG",
            pos2(card.min.x + px(16.0), hy),
            12.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        let kd = px(26.0);
        let key = Rect::from_min_size(
            pos2(card.max.x - px(CLOSE_SLOT) - kd, hy - kd / 2.0),
            vec2(kd, kd),
        );
        // your saved sounds (the amp room's), beside the expand key
        let star = key.translate(vec2(-(kd + px(8.0)), 0.0));
        let resp = widgets::round_button(
            ui,
            star,
            "board-sounds",
            Icon::Star,
            12.0,
            RoundKind::Ivory,
            self.sounds_open(),
        );
        if resp.hovered() {
            self.tip = Some((
                "Your sounds".into(),
                "Shift T".into(),
                pos2(star.center().x, star.max.y + px(4.0)),
                Align2::CENTER_TOP,
            ));
        }
        if resp.clicked() {
            self.sounds_toggle();
        }
        let mode = self.board_mode();
        // (not playing: everything under the wash is drawn only, B5)
        let inert_was = self.board.inert;
        self.board.inert = inert_was || !mode;
        let status = if mode {
            self.board_title(st)
        } else {
            "not on: your tone is ".to_string()
                + &crate::player::tone_label(&self.engine().map(|e| e.tone()).unwrap_or_default())
        };
        text::draw(
            &p,
            &status,
            pos2(tt.max.x + px(10.0), hy),
            Align2::LEFT_CENTER,
            Style::new(13.0, 800, if mode { th.accent_ink } else { th.caption }),
            Some(star.min.x - tt.max.x - px(18.0)),
        );
        // (the expand key opens the amp room in the song's context: one screen for the song's tone and yours, D102)
        if widgets::round_button(
            ui,
            key,
            "board-expand",
            Icon::Expand,
            12.0,
            RoundKind::Ivory,
            false,
        )
        .clicked()
        {
            self.board_commit(st);
            self.reqs.push(super::PlayerReq::Room);
        }
        // the tones row
        let ty = card.min.y + pad_y + head + px(8.0);
        let song_w = px(62.0);
        let has_rig = self.board.rig.is_some();
        let follow = pb::follow(&st.config);
        let src = self.source(st);
        let track_r = Rect::from_min_max(
            pos2(inner_x0, ty),
            pos2(
                if has_rig {
                    inner_x1 - song_w - px(8.0)
                } else {
                    inner_x1
                },
                ty + tones_h,
            ),
        );
        let (labels, sel): (Vec<String>, Option<usize>) = match &self.board.rig {
            Some(rig) => {
                let mut l: Vec<String> = rig.tones.iter().map(|t| short_tone(&t.label)).collect();
                // (P41.1: yours on every song says so)
                l.push(
                    if pb::always_mine(&st.config) {
                        "ALWAYS YOURS"
                    } else {
                        "YOURS"
                    }
                    .into(),
                );
                let sel = match src {
                    Source::Song(i) => Some(i),
                    Source::Mine => Some(rig.tones.len()),
                };
                (l, sel)
            }
            None => {
                // (a starting board put on from here is lit; MINE reads BACK until yours is put back, B6)
                let loaded = self.board.tile_preset.filter(|_| self.board.undo.is_some());
                (
                    vec![
                        if loaded.is_some() { "BACK" } else { "YOURS" }.into(),
                        "CLEAN".into(),
                        "CRUNCH".into(),
                        "LEAD".into(),
                        "METAL".into(),
                        "MORE".into(),
                    ],
                    Some(loaded.unwrap_or(0)),
                )
            }
        };
        let rects = pills(&p, track_r, &labels, sel, 9.0);
        for (i, r) in rects.iter().enumerate() {
            let resp = interact(ui, *r, ("board-tone", i), Sense::click());
            if resp.hovered()
                && let Some(t) = self.board.rig.as_ref().and_then(|rg| rg.tones.get(i))
            {
                self.tip = Some((
                    format!(
                        "The song's {} tone{}",
                        t.label,
                        if t.edited { " (changed by you)" } else { "" }
                    ),
                    crate::room::plain::chain_words(&t.board),
                    pos2(r.center().x, r.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.hovered()
                && let Some(n) = self.board.rig.as_ref().map(|rg| rg.tones.len())
                && i == n
            {
                let always = pb::always_mine(&st.config);
                self.tip = Some((
                    if always {
                        "Your sound, on every song: click to keep it for this song only".into()
                    } else if matches!(src, Source::Mine) {
                        "Your sound, for this song: click again to keep it on every song".into()
                    } else {
                        "Your own sound instead of the song's".into()
                    },
                    "FOLLOW goes back to the song's tones".into(),
                    pos2(r.center().x, r.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if !resp.clicked() || self.board.inert {
                continue;
            }
            match self.board.rig.as_ref().map(|r| r.tones.len()) {
                Some(n) if i < n => self.pick_tone(st, Pick::Tone(i)),
                // (P41.1: a second click on YOURS keeps it on every song, a third for this song only)
                Some(_) if matches!(src, Source::Mine) => {
                    let on = !pb::always_mine(&st.config);
                    pb::set_always_mine(&mut st.config, on);
                    st.save_config();
                    self.say(
                        if on {
                            "Your sound on every song"
                        } else {
                            "Your sound for this song"
                        },
                        "  \u{b7}  FOLLOW goes back to the song's tones",
                    );
                }
                Some(_) => self.pick_tone(st, Pick::Mine),
                None => match i {
                    0 => {
                        if self.board.tile_preset.take().is_some()
                            && let Some((b, label)) = self.board.undo.take()
                        {
                            self.board_edit(st, |cur| *cur = b);
                            self.board_commit(st);
                            self.say(format!("Back to {label}"), "");
                        }
                    }
                    5 => {
                        self.board_commit(st);
                        self.reqs.push(super::PlayerReq::Room);
                    }
                    _ => {
                        let name = ["Clean", "Crunch", "Lead", "Metal"][i - 1];
                        if let Some((_, b)) = board::presets().into_iter().find(|(n, _)| *n == name) {
                            // (the board before the first preset is what BACK puts back)
                            let keep = self.board.tile_preset.and(self.board.undo.clone());
                            self.load_board(st, name, b);
                            if keep.is_some() {
                                self.board.undo = keep;
                            }
                            self.board.tile_preset = Some(i);
                            self.said.pop();
                            self.say(
                                format!("{name} is on your board"),
                                "  \u{b7}  BACK puts yours back",
                            );
                        }
                    }
                },
            }
        }
        if has_rig {
            let sr = Rect::from_min_max(pos2(inner_x1 - song_w, ty), pos2(inner_x1, ty + tones_h));
            let resp = interact(ui, sr, "board-follow", Sense::click());
            let on = follow && self.board.pick.is_none();
            lamp_key(&p, sr, "FOLLOW", on, resp.hovered());
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                self.tip = Some((
                    "Follow the song: its own tones change as the song does".into(),
                    String::new(),
                    pos2(sr.center().x, sr.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.clicked() {
                self.set_follow(st, !on);
            }
        }
        // where the sound comes from (P41.3): the song's own tone, or yours with the way back; the rig in words
        let ly = ty + tones_h + px(10.0);
        if mode {
            let b_now = self.board_now(st);
            let (said, back) = match src {
                Source::Song(i) => {
                    let yours = self
                        .board
                        .rig
                        .as_ref()
                        .and_then(|r| r.tones.get(i))
                        .is_some_and(|t| t.edited);
                    (
                        if yours {
                            "The song's own tone (from RS), changed by you"
                        } else {
                            "You're hearing the song's own tone (from RS)"
                        },
                        false,
                    )
                }
                Source::Mine => ("You're hearing your sound", has_rig),
            };
            let ss = Style::new(11.5, 700, th.text_2);
            let link = "Back to the song's tone \u{203a}";
            let ls = Style::new(11.5, 800, th.accent_ink);
            let lw = if back {
                text::width(&ctx, link, ls) + px(12.0)
            } else {
                0.0
            };
            let sr = text::draw(
                &p,
                said,
                pos2(inner_x0 + px(4.0), ly),
                Align2::LEFT_CENTER,
                ss,
                Some(inner_x1 - inner_x0 - px(8.0) - lw),
            );
            if back {
                let lr = Rect::from_min_size(
                    pos2(sr.max.x + px(12.0), ly - px(9.0)),
                    vec2(lw - px(12.0), px(18.0)),
                );
                let resp = interact(ui, lr, "board-back-song", Sense::click());
                text::draw(&p, link, lr.left_center(), Align2::LEFT_CENTER, ls, None);
                if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() && !self.board.inert {
                    self.set_follow(st, true);
                }
            }
            text::draw(
                &p,
                &crate::room::plain::chain_words(&b_now),
                pos2(inner_x0 + px(4.0), ly + px(17.0)),
                Align2::LEFT_CENTER,
                Style::new(10.5, 600, th.caption),
                Some(inner_x1 - inner_x0 - px(8.0)),
            );
        }
        // the chain
        let sy = ty + tones_h + src_h + gap;
        let well = Rect::from_min_max(pos2(inner_x0, sy), pos2(inner_x1, sy + strip_h));
        paint::inset(&p, well, Some(12.0), 2.0, Fill::Base);
        let sel_before = self.board.sel;
        self.chain_ui(ui, st, well, "tile", false);
        // (a block clicked on the chain, or + for the browser: the list makes way)
        if self.board.picker.is_some() || (self.board.sel != sel_before && sel_before.is_some()) {
            self.board.sounds = None;
        }
        // the shown block (or the browser, or your sounds)
        let fy = sy + strip_h + gap;
        let area = Rect::from_min_max(
            pos2(inner_x0 + px(4.0), fy),
            pos2(inner_x1 - px(4.0), fy + title_h + px(6.0) + face_h),
        );
        if self.board.sounds.is_some() {
            self.sounds_ui(ui, st, area);
        } else if let Some(picker) = self.board.picker {
            self.picker_ui(ui, st, area, picker, "tile");
        } else {
            let b = self.board_now(st);
            match self.board.sel.and_then(|u| b.index_of(u)).map(|i| b.blocks[i]) {
                Some(blk) => {
                    let def = blk.def();
                    let tr = Rect::from_min_size(area.min, vec2(area.width(), title_h));
                    let ks = px(22.0);
                    let lk = Rect::from_min_size(pos2(tr.min.x, tr.center().y - ks / 2.0), Vec2::splat(ks));
                    let rk =
                        Rect::from_min_size(pos2(tr.max.x - ks, tr.center().y - ks / 2.0), Vec2::splat(ks));
                    if widgets::round_button(
                        ui,
                        lk,
                        ("board-prev", blk.uid),
                        Icon::Left,
                        9.0,
                        RoundKind::Ivory,
                        false,
                    )
                    .clicked()
                    {
                        self.cycle_model(st, blk.uid, -1);
                    }
                    if widgets::round_button(
                        ui,
                        rk,
                        ("board-next", blk.uid),
                        Icon::Right,
                        9.0,
                        RoundKind::Ivory,
                        false,
                    )
                    .clicked()
                    {
                        self.cycle_model(st, blk.uid, 1);
                    }
                    let kind = def.kind.label().to_uppercase();
                    let kr = caps(
                        &p,
                        &kind,
                        pos2(lk.max.x + px(8.0), tr.center().y),
                        10.0,
                        faces::kind_colour(def.kind),
                        Align2::LEFT_CENTER,
                    );
                    // (P41.3: a capture by its short name and the kind of amp it is, not "NAM capture after a
                    // Neural Amp Modeler..."; a model by its name and what it is in plain words)
                    let shown = match Rack::file_name(&blk) {
                        Some(n) if def.file && blk.asset != 0 => crate::room::rig::short_capture_name(&n),
                        _ if def.file => def.name.to_string(),
                        _ => def.shown_name(),
                    };
                    let nr = text::draw(
                        &p,
                        &shown,
                        pos2(kr.max.x + px(8.0), tr.center().y),
                        Align2::LEFT_CENTER,
                        Style::new(13.0, 800, th.text),
                        Some(rk.min.x - kr.max.x - px(16.0)),
                    );
                    if rk.min.x - nr.max.x > px(70.0) {
                        text::draw(
                            &p,
                            &crate::room::plain::unit_words(&blk),
                            pos2(nr.max.x + px(8.0), tr.center().y),
                            Align2::LEFT_CENTER,
                            Style::new(11.0, 600, th.caption),
                            Some(rk.min.x - nr.max.x - px(14.0)),
                        );
                    }
                    let fr =
                        Rect::from_min_size(pos2(area.min.x, tr.max.y + px(6.0)), vec2(area.width(), face_h));
                    self.face_ui(ui, st, blk, fr, Detail::Full, "tile");
                }
                None => {
                    text::draw(
                        &p,
                        "An empty board: + adds a pedal, an amp, a cab\u{2026}",
                        area.center(),
                        Align2::CENTER_CENTER,
                        Style::new(13.0, 600, th.caption),
                        Some(area.width()),
                    );
                }
            }
            if !mode {
                // not playing: say so over the face, with the way to turn it on
                paint::wash(&p, area, th.screen, 0.72);
                // only an amp or modeller makes the sound itself; a microphone or a dry input is just heard as is
                let modeller = crate::calibrate::current_input(st)
                    .is_some_and(|(id, name)| crate::calibrate::is_modeller_input(&st.config, &id, &name));
                let tone = self.engine().map(|e| e.tone()).unwrap_or_default();
                let name = crate::player::tone_label(&tone);
                let line = match (&tone, modeller) {
                    (ToneSpec::AsIs, true) => {
                        format!("Your tone is {name}: the amp modeller makes the sound.")
                    }
                    (ToneSpec::AsIs, false) => {
                        format!("Your tone is {name}: your input is heard as it comes in.")
                    }
                    _ => format!("Your tone is {name}, not your rig."),
                };
                text::draw(
                    &p,
                    &line,
                    area.center() - vec2(0.0, px(20.0)),
                    Align2::CENTER_CENTER,
                    Style::new(13.0, 700, th.text_2),
                    Some(area.width() - px(20.0)),
                );
                let bw = px(190.0);
                let br = Rect::from_center_size(area.center() + vec2(0.0, px(18.0)), vec2(bw, px(36.0)));
                self.board.inert = inert_was;
                if widgets::button(
                    ui,
                    br,
                    "board-use",
                    "Use your rig",
                    "",
                    widgets::ButtonStyle::primary(),
                )
                .clicked()
                {
                    self.board_use(st);
                }
            }
        }
        self.board.inert = inert_was;
        // meters and DRY
        let my = area.max.y + gap;
        let (in_peak, out_peak, device) = match self.engine() {
            Some(e) => {
                let m = e.meters();
                let name = e
                    .status()
                    .input
                    .map(|i| short_device(&i.device_name))
                    .unwrap_or_else(|| "NO INPUT".into());
                (m.peak, m.tone_peak, name)
            }
            None => (0.0, 0.0, "NO INPUT".into()),
        };
        let dry_w = px(62.0);
        let mw = (inner_x1 - inner_x0 - dry_w - px(20.0)) / 2.0;
        meter(
            &p,
            Rect::from_min_size(pos2(inner_x0 + px(4.0), my), vec2(mw - px(4.0), foot_h - px(4.0))),
            &format!("IN \u{b7} {device}"),
            in_peak,
            true,
            self.clipping(),
        );
        meter(
            &p,
            Rect::from_min_size(
                pos2(inner_x0 + mw + px(10.0), my),
                vec2(mw - px(4.0), foot_h - px(4.0)),
            ),
            "OUT",
            out_peak,
            false,
            false,
        );
        let dr = Rect::from_min_size(
            pos2(inner_x1 - dry_w, my + px(2.0)),
            vec2(dry_w, foot_h - px(6.0)),
        );
        let resp = interact(ui, dr, "board-dry", Sense::click());
        lamp_key(&p, dr, "DRY", self.board.dry, resp.hovered());
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            self.tip = Some((
                "Hear yourself dry (every block off) to compare".into(),
                String::new(),
                pos2(dr.center().x, dr.max.y + px(4.0)),
                Align2::RIGHT_TOP,
            ));
        }
        if resp.clicked() {
            self.board.dry = !self.board.dry;
            self.board_send(st);
        }
        card.max.y
    }

    // -- the browser ------------------------------------------------------------------------------------------------

    fn picker_ui(&mut self, ui: &Ui, st: &mut AppState, area: Rect, picker: Picker, tag: &'static str) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        paint::display(&p, area, Some(12.0), 4.0);
        let inner = area.shrink(px(10.0));
        let title = match picker {
            Picker::Kinds { .. } => "ADD A BLOCK".to_string(),
            Picker::Models { kind, .. } => format!("ADD \u{b7} {}", kind.label().to_uppercase()),
        };
        caps(
            &p,
            &title,
            pos2(inner.min.x, inner.min.y + px(7.0)),
            10.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        let cancel = Rect::from_min_size(
            pos2(inner.max.x - px(60.0), inner.min.y),
            vec2(px(60.0), px(16.0)),
        );
        let cr = interact(ui, cancel, (tag, "pick-cancel"), Sense::click());
        text::draw(
            &p,
            if matches!(picker, Picker::Models { .. }) {
                "\u{2039} Back"
            } else {
                "Cancel"
            },
            cancel.right_center(),
            Align2::RIGHT_CENTER,
            Style::new(11.0, 700, if cr.hovered() { th.accent_ink } else { th.caption }),
            None,
        );
        if cr.clicked() {
            self.board.picker = match picker {
                Picker::Models { at, .. } => Some(Picker::Kinds { at }),
                _ => None,
            };
            return;
        }
        let body = Rect::from_min_max(pos2(inner.min.x, inner.min.y + px(22.0)), inner.max);
        match picker {
            Picker::Kinds { at } => {
                // five across when the longest name fits at its size, else fewer, more rows ("MODULATION" and
                // "DYNAMICS" were squeezed to a smear in the tile, P37.4b)
                let need = Kind::ALL
                    .iter()
                    .map(|k| {
                        text::width(&ctx, &k.label().to_uppercase(), Style::new(12.0, 800, th.text)) * 1.1
                    })
                    .fold(0.0f32, f32::max)
                    + px(12.0);
                let cols = [5usize, 4, 3]
                    .into_iter()
                    .find(|&c| body.width() / c as f32 >= need)
                    .unwrap_or(3);
                let rows = Kind::ALL.len().div_ceil(cols);
                let cw = body.width() / cols as f32;
                let rh = body.height() / rows as f32;
                for (i, kind) in Kind::ALL.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(
                            body.min.x + cw * (i % cols) as f32,
                            body.min.y + rh * (i / cols) as f32,
                        ),
                        vec2(cw, rh),
                    )
                    .shrink(px(3.0));
                    let resp = interact(ui, r, (tag, "kind", i), Sense::click());
                    let col = kind.colour();
                    p.rect_filled(r.translate(vec2(0.0, 2.0)), 9.0, Color32::from_black_alpha(90));
                    p.rect_filled(r, 9.0, faces::rgb(col));
                    p.rect_filled(
                        Rect::from_min_size(r.min + vec2(3.0, 2.0), vec2(r.width() - 6.0, 2.0)),
                        1.0,
                        Color32::from_white_alpha(70),
                    );
                    let ink = faces::ink(col);
                    faces::print(
                        &p,
                        kind.label(),
                        r.center() - vec2(0.0, px(7.0)),
                        Align2::CENTER_CENTER,
                        12.0,
                        ink,
                        r.width() - 6.0,
                    );
                    let n = kind_models(*kind).len();
                    text::draw(
                        &p,
                        &format!("{n} models"),
                        r.center() + vec2(0.0, px(10.0)),
                        Align2::CENTER_CENTER,
                        Style::new(9.0, 700, ink.gamma_multiply(0.8)),
                        None,
                    );
                    if resp.hovered() {
                        ctx.set_cursor_icon(CursorIcon::PointingHand);
                        p.rect_stroke(
                            r.expand(2.0),
                            10.0,
                            Stroke::new(1.5, th.accent),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if resp.clicked() {
                        self.board.picker = Some(Picker::Models { at, kind: *kind });
                    }
                }
            }
            Picker::Models { at, kind } => {
                let models = kind_models(kind);
                let cols = if models.len() > 5 { 2 } else { 1 };
                let rows = models.len().div_ceil(cols);
                let cw = body.width() / cols as f32;
                let rh = (body.height() / rows.max(1) as f32).min(px(34.0));
                for (n, (i, m)) in models.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(
                            body.min.x + cw * (n / rows) as f32,
                            body.min.y + rh * (n % rows) as f32,
                        ),
                        vec2(cw, rh),
                    )
                    .shrink2(vec2(px(2.0), px(1.0)));
                    let resp = interact(ui, r, (tag, "model", *i), Sense::click());
                    if resp.hovered() {
                        p.rect_filled(r, 8.0, th.screen_hi);
                        ctx.set_cursor_icon(CursorIcon::PointingHand);
                        self.tip = Some((
                            m.blurb.to_string(),
                            String::new(),
                            pos2(r.center().x, r.max.y + 2.0),
                            Align2::CENTER_TOP,
                        ));
                    }
                    let sw = Rect::from_min_size(
                        pos2(r.min.x + px(4.0), r.center().y - px(10.0)),
                        vec2(px(14.0), px(20.0)),
                    );
                    p.rect_filled(sw, 3.0, faces::rgb(m.colour));
                    p.rect_stroke(
                        sw,
                        3.0,
                        Stroke::new(1.0, Color32::from_black_alpha(120)),
                        egui::StrokeKind::Inside,
                    );
                    text::draw(
                        &p,
                        m.name,
                        pos2(sw.max.x + px(8.0), r.center().y - px(6.0)),
                        Align2::LEFT_CENTER,
                        Style::new(12.0, 800, th.text),
                        Some(r.max.x - sw.max.x - px(10.0)),
                    );
                    text::draw(
                        &p,
                        &format!("after {}", m.after),
                        pos2(sw.max.x + px(8.0), r.center().y + px(7.0)),
                        Align2::LEFT_CENTER,
                        Style::new(9.5, 600, th.caption),
                        Some(r.max.x - sw.max.x - px(10.0)),
                    );
                    if resp.clicked() {
                        self.add_block(st, at, *i);
                    }
                }
            }
        }
    }

    // -- the big view -----------------------------------------------------------------------------------------------

    pub(crate) fn draw_board_view(&mut self, ui: &Ui, st: &mut AppState, area: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        self.begin_frame(&ctx);
        paint::dim(&p, area, Some(215));
        // the editor needs the editing block (an add goes back to the board, where the browser is)
        if self.board.picker.is_some() {
            self.board.editing = false;
        }
        let editing = self.board.editing && {
            let b = self.board_now(st);
            self.board.sel.is_some_and(|u| b.index_of(u).is_some())
        };
        let (wmax, hmax) = if editing { (1320.0, 860.0) } else { (1260.0, 800.0) };
        let w = (area.width() - px(60.0)).min(px(wmax));
        let h = (area.height() - px(40.0)).min(px(hmax));
        let panel = Rect::from_center_size(area.center(), vec2(w, h));
        // a click outside closes it
        let outside = interact(ui, area, "board-view-back", Sense::click());
        if outside.clicked()
            && let Some(pos) = outside.interact_pointer_pos()
            && !panel.contains(pos)
        {
            self.board_commit(st);
            self.board.open = false;
            return;
        }
        paint::floating(&p, panel, Some(26.0));
        let inner = panel.shrink(px(20.0));
        let foot_h = px(40.0);
        if editing {
            self.editor_view(ui, st, inner, foot_h);
            self.view_footer(ui, inner, foot_h);
            return;
        }
        // header
        let hdr = Rect::from_min_size(inner.min, vec2(inner.width(), px(52.0)));
        let sub = match &self.board.rig {
            Some(_) => format!(
                "RIG \u{b7} {} \u{b7} {}",
                self.song.title.to_uppercase(),
                self.song.part_name.to_uppercase()
            ),
            None => "RIG".into(),
        };
        caps(
            &p,
            &sub,
            pos2(hdr.min.x, hdr.min.y + px(9.0)),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        let title = match &self.board.naming {
            Some(n) => format!("Save as: {n}\u{2581}"),
            None => self.board_title(st),
        };
        text::draw(
            &p,
            &title,
            pos2(hdr.min.x, hdr.min.y + px(34.0)),
            Align2::LEFT_CENTER,
            Style::new(
                22.0,
                800,
                if self.board.naming.is_some() {
                    th.accent_ink
                } else {
                    th.text
                },
            ),
            Some(px(420.0)),
        );
        // the tones, follow
        let mut x = hdr.min.x + px(440.0);
        if let Some(rig) = &self.board.rig {
            let mut labels: Vec<String> = rig.tones.iter().map(|t| t.label.to_uppercase()).collect();
            labels.push(
                if pb::always_mine(&st.config) {
                    "ALWAYS YOURS"
                } else {
                    "YOURS"
                }
                .into(),
            );
            let n = rig.tones.len();
            let sel = match self.source(st) {
                Source::Song(i) => i,
                Source::Mine => n,
            };
            let tw = (labels
                .iter()
                .map(|l| text::width(&ctx, l, Style::caps(10.5, th.caption)) + px(18.0))
                .sum::<f32>()
                + px(6.0))
            .min(px(470.0));
            let tr = Rect::from_min_size(pos2(x, hdr.min.y + px(10.0)), vec2(tw, px(32.0)));
            let rects = pills(&p, tr, &labels, Some(sel), 10.5);
            for (i, r) in rects.iter().enumerate() {
                let resp = interact(ui, *r, ("board-view-tone", i), Sense::click());
                if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    self.pick_tone(st, if i < n { Pick::Tone(i) } else { Pick::Mine });
                }
            }
            x = tr.max.x + px(10.0);
            let fr = Rect::from_min_size(pos2(x, hdr.min.y + px(10.0)), vec2(px(122.0), px(32.0)));
            let on = pb::follow(&st.config) && self.board.pick.is_none();
            let resp = interact(ui, fr, "board-view-follow", Sense::click());
            lamp_key(&p, fr, "FOLLOW SONG", on, resp.hovered());
            if resp.clicked() {
                self.set_follow(st, !on);
            }
            x = fr.max.x + px(10.0);
        }
        let _ = x;
        // the keys at the right: presets, save as, reset / undo, close
        let mut kx = hdr.max.x;
        let close = Rect::from_min_size(pos2(kx - px(34.0), hdr.min.y + px(9.0)), Vec2::splat(px(34.0)));
        if widgets::round_button(
            ui,
            close,
            "board-view-close",
            Icon::XBold,
            13.0,
            RoundKind::Ivory,
            false,
        )
        .clicked()
        {
            self.board_commit(st);
            self.board.open = false;
            self.board.naming = None;
            return;
        }
        kx = close.min.x - px(10.0);
        let key_btn = |ui: &Ui, kx: &mut f32, id: &str, label: &str, primary: bool| -> bool {
            let bw = widgets::button_width(ui.ctx(), label, "", 13.0, false) + px(8.0);
            let r = Rect::from_min_size(pos2(*kx - bw, hdr.min.y + px(9.0)), vec2(bw, px(34.0)));
            *kx = r.min.x - px(8.0);
            let st_ = if primary {
                widgets::ButtonStyle::primary()
            } else {
                widgets::ButtonStyle::default()
            };
            widgets::button(ui, r, id, label, "", st_).clicked()
        };
        let edited_song = matches!(self.source(st), Source::Song(i) if self.board.rig.as_ref().is_some_and(|r| r.tones.get(i).is_some_and(|t| t.edited)));
        if edited_song && key_btn(ui, &mut kx, "board-reset", "Reset to song", false) {
            if let (Source::Song(i), Some(rig)) = (self.source(st), self.board.rig.as_mut()) {
                rig.reset(i, &self.song.arr, &mut st.userdata);
            }
            self.board.sel = None;
            self.say("Back to the song's own tone", "");
        }
        if self.board.undo.is_some()
            && key_btn(ui, &mut kx, "board-undo", "Undo", false)
            && let Some((b, label)) = self.board.undo.take()
        {
            self.board_edit(st, |cur| *cur = b);
            self.board_commit(st);
            self.say(format!("Back to {label}"), "");
        }
        // (P37.6c, D142: one editor. Saving a sound and picking a starting board happen in the amp room (its Sounds
        // drawer and Save); this view sends you there instead of keeping a second Presets and Save as)
        if key_btn(
            ui,
            &mut kx,
            "board-room",
            "Save & sounds: amp room \u{203a}",
            true,
        ) {
            self.board_commit(st);
            self.board.open = false;
            self.board.naming = None;
            self.board.menu = false;
            self.reqs.push(super::PlayerReq::Room);
            return;
        }
        let presets_anchor = pos2(kx + px(8.0), hdr.max.y + px(4.0));
        // the board itself, and under it the shown block (or the browser, adding one)
        let lower_h = px(150.0);
        let board_r = Rect::from_min_max(
            pos2(inner.min.x, hdr.max.y + px(10.0)),
            pos2(inner.max.x, inner.max.y - foot_h - px(12.0) - lower_h - px(12.0)),
        );
        self.board_area(ui, st, board_r);
        let lower = Rect::from_min_max(
            pos2(inner.min.x, board_r.max.y + px(12.0)),
            pos2(inner.max.x, inner.max.y - foot_h - px(12.0)),
        );
        let b = self.board_now(st);
        let blk = self.board.sel.and_then(|u| b.index_of(u)).map(|i| b.blocks[i]);
        match (self.board.picker, blk) {
            (Some(picker), _) => self.picker_ui(ui, st, lower, picker, "view"),
            (None, Some(blk)) => self.block_bar(ui, st, lower, blk),
            (None, None) => {
                paint::display(&p, lower, Some(18.0), 6.0);
                text::draw(
                    &p,
                    "Click a block on the board to edit it, or + to add one.",
                    lower.center(),
                    Align2::CENTER_CENTER,
                    Style::new(13.0, 600, th.caption),
                    Some(lower.width() - px(20.0)),
                );
            }
        }
        self.view_footer(ui, inner, foot_h);
        if self.board.menu {
            self.presets_menu(ui, st, presets_anchor);
        }
    }

    /// The big view's footer: the input level and its check, the output, the route.
    fn view_footer(&mut self, ui: &Ui, inner: Rect, foot_h: f32) {
        let p = ui.painter().clone();
        let th = t();
        let fy = inner.max.y - foot_h;
        let (in_peak, out_peak, route) = match self.engine() {
            Some(e) => {
                let m = e.meters();
                let s = e.status();
                let name = s
                    .input
                    .as_ref()
                    .map(|i| short_device(&i.device_name))
                    .unwrap_or_else(|| "NO INPUT".into());
                let route = match &s.input {
                    Some(i) => format!(
                        "{} \u{b7} {} \u{b7} {:.0} kHz \u{b7} {} samples{}",
                        i.device_name,
                        i.mode,
                        i.rate as f64 / 1000.0,
                        i.block,
                        if s.duplex {
                            " \u{b7} one device both ways"
                        } else {
                            ""
                        }
                    ),
                    None => "no input".into(),
                };
                (m.peak, m.tone_peak, (name, route))
            }
            None => (0.0, 0.0, ("NO INPUT".into(), "audio off".into())),
        };
        meter(
            &p,
            Rect::from_min_size(pos2(inner.min.x, fy + px(4.0)), vec2(px(280.0), px(30.0))),
            &format!("INPUT \u{b7} {}", route.0),
            in_peak,
            true,
            self.clipping(),
        );
        let cr = Rect::from_min_size(
            pos2(inner.min.x + px(292.0), fy + px(6.0)),
            vec2(px(150.0), px(30.0)),
        );
        let checking = self.board.level.is_some();
        if widgets::button(
            ui,
            cr,
            "board-level",
            if checking {
                "Play hard\u{2026}"
            } else {
                "Check my level"
            },
            "",
            widgets::ButtonStyle::active(checking),
        )
        .clicked()
            && !checking
        {
            self.board.level = Some((Instant::now() + Duration::from_secs(5), 0.0));
            self.board.level_note = None;
        }
        meter(
            &p,
            Rect::from_min_size(pos2(cr.max.x + px(16.0), fy + px(4.0)), vec2(px(200.0), px(30.0))),
            "OUTPUT",
            out_peak,
            false,
            false,
        );
        let note = self.board.level_note.clone().unwrap_or(route.1);
        text::draw(
            &p,
            &note,
            pos2(inner.max.x, fy + px(20.0)),
            Align2::RIGHT_CENTER,
            Style::new(
                11.0,
                600,
                if self.board.level_note.is_some() {
                    th.accent_ink
                } else {
                    th.caption
                },
            ),
            Some(inner.max.x - cr.max.x - px(240.0)),
        );
    }

    /// The board itself: every block standing on a pedalboard, cables between, + between them.
    pub(crate) fn board_area(&mut self, ui: &Ui, st: &mut AppState, r: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        // the board: dark aluminium with slats (the amp room draws its own stage under it)
        if !self.board.stage {
            p.rect_filled(r, 18.0, Color32::from_rgb(30, 34, 39));
            let slat = r.height() / 7.0;
            for i in 1..7 {
                let y = r.min.y + slat * i as f32;
                p.line_segment(
                    [pos2(r.min.x + 10.0, y), pos2(r.max.x - 10.0, y)],
                    Stroke::new(1.0, Color32::from_white_alpha(8)),
                );
            }
            paint::inset(&p, r, Some(18.0), 3.0, Fill::Color(Color32::TRANSPARENT));
        }
        let b = self.board_now(st);
        // (on the amp room's stage the units stand on the floor, their names under them, and an amp straight before
        // a cab stands on it, the two the same width: `stack[i]`, block i is that amp)
        let stage = self.board.stage;
        let nb = b.blocks.len();
        let stack: Vec<bool> = (0..nb)
            .map(|i| {
                stage
                    && b.blocks[i].def().kind == Kind::Amp
                    && faces::face_of(&b.blocks[i]) == Face::Amp
                    && b.blocks
                        .get(i + 1)
                        .is_some_and(|c| c.def().kind == Kind::Cab && faces::face_of(c) == Face::Cab)
            })
            .collect();
        let under = |i: usize| i > 0 && stack[i - 1];
        let mut sizes: Vec<Vec2> = b.blocks.iter().map(faces::board_size).collect();
        for i in 0..nb {
            if stack[i] {
                let (a, c) = (sizes[i], sizes[i + 1]);
                let w = a.x.max(c.x);
                sizes[i] = a * (w / a.x.max(1.0));
                sizes[i + 1] = c * (w / c.x.max(1.0));
            }
        }
        let gap = if stage { 28.0 } else { 40.0 };
        let cols = (0..nb).filter(|&i| !under(i)).count();
        let total: f32 =
            (0..nb).filter(|&i| !under(i)).map(|i| sizes[i].x).sum::<f32>() + gap * (cols as f32 + 1.0);
        let label_h = if stage { px(44.0) } else { 0.0 };
        let tall = if stage {
            (0..nb)
                .map(|i| {
                    if stack[i] {
                        sizes[i].y + sizes[i + 1].y
                    } else {
                        sizes[i].y
                    }
                })
                .fold(200.0f32, f32::max)
        } else {
            200.0
        };
        let k = ((r.width() - 60.0) / total)
            .min((r.height() - 50.0 - label_h) / tall)
            .min(1.7);
        let mut x = r.min.x + 30.0 + gap * k;
        if stage {
            x += ((r.width() - 60.0) - total * k).max(0.0) / 2.0; // (centred on the stage)
        }
        let base = r.max.y - label_h;
        let lows = (0..nb)
            .filter(|&i| !stack[i] && !under(i))
            .map(|i| sizes[i].y)
            .fold(f32::MAX, f32::min);
        let low = if lows == f32::MAX {
            sizes.iter().map(|s| s.y).fold(f32::MAX, f32::min)
        } else {
            lows
        };
        let cy = if stage && !sizes.is_empty() {
            base - low * k * 0.55
        } else {
            r.center().y
        };
        let mut rects: Vec<(u32, Rect)> = Vec::new();
        for (i, (blk, s)) in b.blocks.iter().zip(&sizes).enumerate() {
            let sz = *s * k;
            if under(i) {
                // the cab under the amp before it: the amp goes up onto it
                let ar = rects[i - 1].1;
                rects.push((blk.uid, Rect::from_min_size(pos2(ar.min.x, base - sz.y), sz)));
                rects[i - 1].1 = Rect::from_min_size(pos2(ar.min.x, base - sz.y - ar.height()), ar.size());
                continue;
            }
            let y = if stage { base - sz.y } else { cy - sz.y / 2.0 };
            rects.push((blk.uid, Rect::from_min_size(pos2(x, y), sz)));
            x += sz.x + gap * k;
        }
        if stage {
            for (i, (_, rr)) in rects.iter().enumerate() {
                if under(i) {
                    continue;
                }
                let d = b.blocks[i].def();
                let on = b.blocks[i].on;
                // (a capture by its own name: "Hot Rod DeVille on Hot Rod Deluxe 1x12", P33)
                let named = |x: &Block| Rack::file_name(x).unwrap_or_else(|| x.def().name.to_string());
                let (num, name, after) = if stack[i] {
                    let c = b.blocks[i + 1].def();
                    let after = if d.file || c.file {
                        "NAM capture \u{b7} TONE3000".to_string()
                    } else {
                        format!("after {} \u{b7} {}", d.after, c.after)
                    };
                    (
                        format!("{} + {}  ", i + 1, i + 2),
                        format!("{} on {}", named(&b.blocks[i]), named(&b.blocks[i + 1])),
                        after,
                    )
                } else {
                    (
                        format!("{}  ", i + 1),
                        named(&b.blocks[i]),
                        if on {
                            after_line(&b.blocks[i])
                        } else {
                            "off".into()
                        },
                    )
                };
                // (the stage is dark in every theme: its words are light)
                let (lit, dim) = (
                    Color32::from_rgb(0xe8, 0xea, 0xee),
                    Color32::from_rgb(0x8a, 0x90, 0x99),
                );
                let room_w = rr.width() + gap * k - px(6.0);
                let ns = Style::new(12.5, 800, if on { lit } else { dim });
                let ctx = p.ctx();
                if text::width(ctx, &num, ns) + text::width(ctx, &name, ns) <= room_w {
                    text::draw_runs(
                        &p,
                        &[(&num, Style::new(12.5, 800, dim)), (&name, ns)],
                        pos2(rr.center().x, base + px(16.0)),
                        Align2::CENTER_CENTER,
                    );
                } else {
                    // (too long: the name shortened, its number kept, B18)
                    text::draw(
                        &p,
                        &format!("{num}{name}"),
                        pos2(rr.center().x, base + px(16.0)),
                        Align2::CENTER_CENTER,
                        ns,
                        Some(room_w),
                    );
                }
                text::draw(
                    &p,
                    &after,
                    pos2(rr.center().x, base + px(32.0)),
                    Align2::CENTER_CENTER,
                    Style::new(10.0, 500, dim),
                    Some(room_w),
                );
            }
        }
        // cables first, under everything: coloured patch leads drooping between the jacks
        let colours = [
            Color32::from_rgb(210, 59, 59),
            Color32::from_rgb(232, 185, 49),
            Color32::from_rgb(47, 127, 209),
            Color32::from_rgb(63, 179, 107),
            Color32::from_rgb(200, 200, 200),
            Color32::from_rgb(176, 79, 198),
        ];
        let mut prev = pos2(r.min.x + 8.0, cy);
        let points: Vec<(Pos2, Pos2)> = rects
            .iter()
            .map(|(_, rr)| (pos2(rr.min.x, cy + 6.0), pos2(rr.max.x, cy + 6.0)))
            .collect();
        let n = points.len();
        for i in 0..=n {
            if i < n && under(i) {
                // (an amp on its cab: no lead between them showing)
                prev = points[i].1;
                continue;
            }
            let to = if i < n {
                points[i].0
            } else {
                pos2(r.max.x - 8.0, cy)
            };
            let sag = 28.0 + (to.x - prev.x).abs() * 0.08;
            let c = CubicBezierShape::from_points_stroke(
                [prev, prev + vec2(18.0, sag), to + vec2(-18.0, sag), to],
                false,
                Color32::TRANSPARENT,
                Stroke::new(5.0, Color32::from_rgb(10, 12, 14)),
            );
            p.add(c);
            let c2 = CubicBezierShape::from_points_stroke(
                [prev, prev + vec2(18.0, sag), to + vec2(-18.0, sag), to],
                false,
                Color32::TRANSPARENT,
                Stroke::new(3.2, colours[i % colours.len()]),
            );
            p.add(c2);
            if i < n {
                prev = points[i].1;
            }
        }
        caps(
            &p,
            "IN",
            pos2(r.min.x + 14.0, cy - 14.0),
            9.0,
            if self.board.stage {
                Color32::from_rgb(0x8a, 0x90, 0x99)
            } else {
                th.caption
            },
            Align2::LEFT_CENTER,
        );
        caps(
            &p,
            "OUT",
            pos2(r.max.x - 14.0, cy - 14.0),
            9.0,
            if self.board.stage {
                Color32::from_rgb(0x8a, 0x90, 0x99)
            } else {
                th.caption
            },
            Align2::RIGHT_CENTER,
        );
        let moving = self.board.moving;
        for (i, (uid, br)) in rects.iter().enumerate() {
            let blk = b.blocks[i];
            let sel = self.board.sel == Some(*uid);
            // the block's body: click to select, drag to move
            let body = interact(ui, *br, ("board-body", *uid), Sense::click_and_drag());
            if body.hovered() {
                self.tip = Some((
                    format!("{} \u{b7} {}", blk.def().shown_name(), after_line(&blk)),
                    "click: show \u{b7} double-click: edit \u{b7} drag: move".into(),
                    pos2(br.center().x, r.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if body.double_clicked() {
                self.board.sel = Some(*uid);
                self.board.picker = None;
                self.board.editing = true;
            } else if body.clicked() {
                self.board.sel = Some(*uid);
                self.board.picker = None;
            }
            if body.drag_started()
                && let Some(pos) = body.interact_pointer_pos()
            {
                self.board.moving = Some(MoveDrag {
                    tag: "board",
                    uid: *uid,
                    grab: pos.x - br.min.x,
                    x: br.min.x,
                    moved: false,
                    pass: ctx.cumulative_pass_nr(),
                });
            }
            let mut draw_r = *br;
            if let Some(m) = self.board.moving.as_mut()
                && m.uid == *uid
                && m.tag == "board"
            {
                m.pass = ctx.cumulative_pass_nr();
                if ctx.input(|i| i.pointer.any_down()) {
                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        m.x = pos.x - m.grab;
                        m.moved = m.moved || body.dragged();
                    }
                    if m.moved {
                        draw_r = Rect::from_min_size(pos2(m.x, br.min.y - 8.0), br.size());
                        ctx.set_cursor_icon(CursorIcon::Grabbing);
                        drop_marker(
                            &p,
                            &rects,
                            *uid,
                            draw_r.center().x,
                            gap * k,
                            (br.min.y - 4.0, br.max.y + 4.0),
                        );
                    }
                } else {
                    let centre = m.x + br.width() / 2.0;
                    let to = drop_index(&rects, *uid, centre);
                    let moved = m.moved;
                    self.board.moving = None;
                    if moved && b.index_of(*uid) != Some(to) {
                        self.move_block(st, *uid, to);
                        self.board_commit(st);
                    }
                }
            }
            let _ = moving;
            if sel {
                p.rect_stroke(
                    draw_r.expand(6.0),
                    14.0,
                    Stroke::new(2.0, th.accent),
                    egui::StrokeKind::Outside,
                );
            }
            self.face_ui(ui, st, blk, draw_r, Detail::Board, "board");
            if !blk.on {
                paint::wash(&p, draw_r, Color32::BLACK, 0.35);
            }
        }
        // + between the blocks and at the end
        let mut plus_at: Vec<(usize, f32)> = (0..rects.len())
            .filter(|&i| !under(i))
            .map(|i| (i, rects[i].1.min.x - gap * k / 2.0))
            .collect();
        plus_at.push((
            rects.len(),
            rects
                .last()
                .map(|r| r.1.max.x + gap * k / 2.0)
                .unwrap_or(r.center().x),
        ));
        for (at, px_) in plus_at {
            let py = if stage && !sizes.is_empty() {
                base - low * k - px(20.0)
            } else {
                r.max.y - 22.0
            };
            let pr = Rect::from_center_size(pos2(px_, py), Vec2::splat(22.0));
            if widgets::round_button(
                ui,
                pr,
                ("board-plus", at),
                Icon::Plus,
                9.0,
                RoundKind::Ivory,
                false,
            )
            .clicked()
            {
                self.board.picker = Some(Picker::Kinds { at });
                self.board.browse = None;
            }
        }
        if b.blocks.is_empty() {
            text::draw(
                &p,
                "Nothing on the board yet: + adds a block",
                r.center(),
                Align2::CENTER_CENTER,
                Style::new(14.0, 600, th.caption),
                None,
            );
        }
    }

    /// What the block does, drawn.
    fn picture_ui(&self, p: &Painter, r: Rect, blk: &Block) {
        let th = t();
        let (title, note) = picture::caption(blk);
        caps(
            p,
            title,
            pos2(r.min.x + px(14.0), r.min.y + px(16.0)),
            11.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        text::draw(
            p,
            note,
            pos2(r.max.x - px(14.0), r.min.y + px(16.0)),
            Align2::RIGHT_CENTER,
            Style::new(10.5, 600, th.caption),
            Some(r.width() * 0.6),
        );
        let g = Rect::from_min_max(
            pos2(r.min.x + px(40.0), r.min.y + px(34.0)),
            pos2(r.max.x - px(16.0), r.max.y - px(26.0)),
        );
        let grid = Stroke::new(1.0, th.divider);
        let line = Stroke::new(2.2, th.accent);
        let label = |s: &str, pos: Pos2, a: Align2| {
            text::draw(p, s, pos, a, Style::new(9.0, 700, th.caption), None);
        };
        let poly = |pts: Vec<Pos2>, fill: bool| {
            if fill && pts.len() > 1 {
                let mut area = pts.clone();
                area.push(pos2(pts[pts.len() - 1].x, g.max.y));
                area.push(pos2(pts[0].x, g.max.y));
                p.add(egui::Shape::convex_polygon(
                    area,
                    Color32::TRANSPARENT,
                    Stroke::NONE,
                ));
            }
            p.add(egui::Shape::line(pts, line));
        };
        match picture::picture(blk) {
            Picture::Response(pts) => {
                let fx = |f: f32| g.min.x + (f / 20.0).log10() / 3.0 * g.width();
                let (top, bot) = (12.0, -24.0);
                let fy = |d: f32| g.min.y + (top - d.clamp(bot, top)) / (top - bot) * g.height();
                for f in [100.0, 1000.0, 10000.0] {
                    p.line_segment([pos2(fx(f), g.min.y), pos2(fx(f), g.max.y)], grid);
                    label(
                        if f >= 1000.0 {
                            if f >= 10000.0 { "10k" } else { "1k" }
                        } else {
                            "100"
                        },
                        pos2(fx(f), g.max.y + px(10.0)),
                        Align2::CENTER_CENTER,
                    );
                }
                for d in [12.0, 0.0, -12.0, -24.0] {
                    p.line_segment([pos2(g.min.x, fy(d)), pos2(g.max.x, fy(d))], grid);
                    label(
                        &format!("{d:+.0}"),
                        pos2(g.min.x - px(6.0), fy(d)),
                        Align2::RIGHT_CENTER,
                    );
                }
                let pts: Vec<Pos2> = pts.iter().map(|&(f, d)| pos2(fx(f), fy(d))).collect();
                poly(pts, false);
            }
            Picture::Transfer(pts) => {
                for v in [-1.0, 0.0, 1.0] {
                    let x = g.center().x + v * g.width() / 2.0;
                    let y = g.center().y - v * g.height() / 2.0;
                    p.line_segment([pos2(x, g.min.y), pos2(x, g.max.y)], grid);
                    p.line_segment([pos2(g.min.x, y), pos2(g.max.x, y)], grid);
                }
                label(
                    "in \u{2192}",
                    pos2(g.max.x, g.max.y + px(10.0)),
                    Align2::RIGHT_CENTER,
                );
                label("out", pos2(g.min.x - px(6.0), g.min.y), Align2::RIGHT_CENTER);
                let pts: Vec<Pos2> = pts
                    .iter()
                    .map(|&(a, b)| {
                        pos2(
                            g.center().x + a * g.width() / 2.0,
                            g.center().y - b * g.height() / 2.0,
                        )
                    })
                    .collect();
                poly(pts, false);
            }
            Picture::Dynamics(pts) => {
                let fx = |d: f32| g.min.x + (d + 60.0) / 60.0 * g.width();
                let fy = |d: f32| g.max.y - (d + 60.0) / 60.0 * g.height();
                for d in [-60.0, -40.0, -20.0, 0.0] {
                    p.line_segment([pos2(fx(d), g.min.y), pos2(fx(d), g.max.y)], grid);
                    p.line_segment([pos2(g.min.x, fy(d)), pos2(g.max.x, fy(d))], grid);
                    label(
                        &format!("{d:.0}"),
                        pos2(fx(d), g.max.y + px(10.0)),
                        Align2::CENTER_CENTER,
                    );
                }
                p.line_segment(
                    [pos2(fx(-60.0), fy(-60.0)), pos2(fx(0.0), fy(0.0))],
                    Stroke::new(1.0, th.tick),
                );
                let pts: Vec<Pos2> = pts.iter().map(|&(a, b)| pos2(fx(a), fy(b))).collect();
                poly(pts, false);
            }
            Picture::Echoes(taps, span) => {
                for s in 0..=(span.floor() as i32) {
                    let x = g.min.x + s as f32 / span * g.width();
                    p.line_segment([pos2(x, g.min.y), pos2(x, g.max.y)], grid);
                    label(
                        &format!("{s} s"),
                        pos2(x, g.max.y + px(10.0)),
                        Align2::CENTER_CENTER,
                    );
                }
                for (i, (t_, lvl)) in taps.iter().enumerate() {
                    let x = g.min.x + t_ / span * g.width();
                    let y = g.max.y - lvl * g.height();
                    let c = if i == 0 { th.text_2 } else { th.accent };
                    p.line_segment(
                        [pos2(x, g.max.y), pos2(x, y)],
                        Stroke::new(if i == 0 { 3.0 } else { 2.4 }, c),
                    );
                    p.circle_filled(pos2(x, y), 3.0, c);
                }
            }
            Picture::Decay(pts, span) => {
                for s in 0..=(span.floor() as i32) {
                    let x = g.min.x + s as f32 / span * g.width();
                    p.line_segment([pos2(x, g.min.y), pos2(x, g.max.y)], grid);
                    label(
                        &format!("{s} s"),
                        pos2(x, g.max.y + px(10.0)),
                        Align2::CENTER_CENTER,
                    );
                }
                let pts: Vec<Pos2> = pts
                    .iter()
                    .map(|&(t_, lvl)| pos2(g.min.x + t_ / span * g.width(), g.max.y - lvl * g.height()))
                    .collect();
                poly(pts, false);
            }
            Picture::Movement(pts) => {
                for s in [0.5, 1.0, 1.5] {
                    let x = g.min.x + s / 2.0 * g.width();
                    p.line_segment([pos2(x, g.min.y), pos2(x, g.max.y)], grid);
                    label(
                        &format!("{s} s"),
                        pos2(x, g.max.y + px(10.0)),
                        Align2::CENTER_CENTER,
                    );
                }
                let pts: Vec<Pos2> = pts
                    .iter()
                    .map(|&(t_, v)| pos2(g.min.x + t_ / 2.0 * g.width(), g.max.y - v * g.height()))
                    .collect();
                poly(pts, false);
            }
        }
    }

    /// The model browser: kind tabs, then the kind's models. With a block shown, a click changes it to that model;
    /// with + pressed, it adds one there.
    fn browser_ui(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Option<Block>) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let adding = match self.board.picker {
            Some(Picker::Kinds { at }) | Some(Picker::Models { at, .. }) => Some(at),
            None => None,
        };
        let kind = self
            .board
            .browse
            .or(blk.map(|b| b.def().kind))
            .unwrap_or(Kind::Drive);
        caps(
            &p,
            if adding.is_some() {
                "ADD A BLOCK HERE"
            } else {
                "CHANGE MODEL"
            },
            pos2(r.min.x + px(14.0), r.min.y + px(16.0)),
            11.0,
            if adding.is_some() {
                th.accent_ink
            } else {
                th.caption
            },
            Align2::LEFT_CENTER,
        );
        if adding.is_some() {
            let cr = Rect::from_min_size(
                pos2(r.max.x - px(70.0), r.min.y + px(8.0)),
                vec2(px(56.0), px(16.0)),
            );
            let resp = interact(ui, cr, "board-browse-cancel", Sense::click());
            text::draw(
                &p,
                "Cancel",
                cr.right_center(),
                Align2::RIGHT_CENTER,
                Style::new(11.0, 700, if resp.hovered() { th.accent_ink } else { th.caption }),
                None,
            );
            if resp.clicked() {
                self.board.picker = None;
            }
        }
        // kind tabs, wrapping
        let mut x = r.min.x + px(12.0);
        let mut y = r.min.y + px(32.0);
        for k in Kind::ALL {
            let label = k.label();
            let w = text::width(&ctx, label, Style::new(10.0, 800, th.text)) + px(26.0);
            if x + w > r.max.x - px(12.0) {
                x = r.min.x + px(12.0);
                y += px(30.0);
            }
            let tr = Rect::from_min_size(pos2(x, y), vec2(w, px(24.0)));
            let resp = interact(ui, tr, ("board-kind", label), Sense::click());
            let on = k == kind;
            if on {
                keys::key_face(&p, tr, Some(8.0), false, false);
            } else if resp.hovered() {
                p.rect_filled(tr, 8.0, th.screen_hi);
            }
            p.rect_filled(
                Rect::from_center_size(pos2(tr.min.x + px(11.0), tr.center().y), vec2(px(8.0), px(8.0))),
                2.0,
                faces::kind_colour(k),
            );
            text::draw(
                &p,
                label,
                pos2(tr.min.x + px(19.0), tr.center().y),
                Align2::LEFT_CENTER,
                Style::new(10.0, 800, if on { keys::key_ink(false) } else { th.text_2 }),
                None,
            );
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.board.browse = Some(k);
            }
            x = tr.max.x + px(6.0);
        }
        // the kind's models
        let list = Rect::from_min_max(
            pos2(r.min.x + px(8.0), y + px(34.0)),
            pos2(r.max.x - px(8.0), r.max.y - px(8.0)),
        );
        let models = kind_models(kind);
        let rh = (list.height() / models.len().max(1) as f32).min(px(40.0));
        // (the name over its "after", where there's the height; else side by side)
        let two_lines = rh >= px(30.0);
        for (n, (i, m)) in models.iter().enumerate() {
            let rr = Rect::from_min_size(
                pos2(list.min.x, list.min.y + rh * n as f32),
                vec2(list.width(), rh),
            )
            .shrink2(vec2(0.0, px(1.0)));
            let resp = interact(ui, rr, ("board-model", *i), Sense::click());
            let current = blk.is_some_and(|b| b.model == *i) && adding.is_none();
            if current {
                p.rect_filled(rr, 9.0, th.accent.gamma_multiply(0.14));
            } else if resp.hovered() {
                p.rect_filled(rr, 9.0, th.screen_hi);
            }
            let sh = (rr.height() - px(4.0)).clamp(px(8.0), px(22.0));
            let sw = Rect::from_min_size(
                pos2(rr.min.x + px(6.0), rr.center().y - sh / 2.0),
                vec2(px(16.0), sh),
            );
            p.rect_filled(sw, 3.0, faces::rgb(m.colour));
            p.rect_stroke(
                sw,
                3.0,
                Stroke::new(1.0, Color32::from_black_alpha(120)),
                egui::StrokeKind::Inside,
            );
            let name = Style::new(12.0, 800, if current { th.accent_ink } else { th.text });
            let after = Style::new(10.0, 600, th.caption);
            let tx = sw.max.x + px(10.0);
            if two_lines {
                text::draw(
                    &p,
                    m.name,
                    pos2(tx, rr.center().y - px(7.0)),
                    Align2::LEFT_CENTER,
                    name,
                    Some(rr.max.x - tx - px(4.0)),
                );
                text::draw(
                    &p,
                    &format!("after {}", m.after),
                    pos2(tx, rr.center().y + px(8.0)),
                    Align2::LEFT_CENTER,
                    after,
                    Some(rr.max.x - tx - px(4.0)),
                );
            } else {
                let nr = text::draw(
                    &p,
                    m.name,
                    pos2(tx, rr.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    Some(rr.max.x - tx - px(4.0)),
                );
                let ax = nr.max.x + px(8.0);
                if rr.max.x - ax > px(40.0) {
                    text::draw(
                        &p,
                        &format!("after {}", m.after),
                        pos2(ax, rr.center().y + px(1.0)),
                        Align2::LEFT_CENTER,
                        after,
                        Some(rr.max.x - ax - px(6.0)),
                    );
                }
            }
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                self.tip = Some((
                    m.blurb.to_string(),
                    String::new(),
                    pos2(rr.min.x, rr.center().y),
                    Align2::RIGHT_CENTER,
                ));
            }
            if resp.clicked() {
                match (adding, blk) {
                    (Some(at), _) => self.add_block(st, at, *i),
                    (None, Some(b)) => {
                        self.change_model(st, b.uid, *i);
                    }
                    (None, None) => {
                        let at = self.board_now(st).blocks.len();
                        self.add_block(st, at, *i);
                    }
                }
            }
        }
    }

    /// The starting boards and yours, under the Presets key.
    fn presets_menu(&mut self, ui: &Ui, st: &mut AppState, anchor: Pos2) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let starters = board::presets();
        let saved = pb::saved(&st.config);
        let row_h = px(28.0);
        let n = starters.len() + saved.len() + 2;
        let w = px(260.0);
        let r = Rect::from_min_size(
            pos2(anchor.x - w + px(120.0), anchor.y),
            vec2(w, row_h * n as f32 + px(16.0)),
        );
        paint::floating(&p, r, Some(14.0));
        let mut y = r.min.y + px(8.0);
        caps(
            &p,
            "STARTING BOARDS",
            pos2(r.min.x + px(14.0), y + row_h / 2.0),
            10.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        y += row_h;
        let mut chosen: Option<(String, Board)> = None;
        for (name, b) in &starters {
            let rr = Rect::from_min_size(pos2(r.min.x + px(6.0), y), vec2(w - px(12.0), row_h));
            let resp = interact(ui, rr, ("board-preset", *name), Sense::click());
            if resp.hovered() {
                p.rect_filled(rr, 8.0, th.screen_hi);
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            let ids: Vec<&str> = b.blocks.iter().map(|x| x.def().short).collect();
            text::draw(
                &p,
                name,
                pos2(rr.min.x + px(10.0), rr.center().y),
                Align2::LEFT_CENTER,
                Style::new(13.0, 700, th.text),
                None,
            );
            text::draw(
                &p,
                &ids.join(" \u{203a} "),
                pos2(rr.max.x - px(8.0), rr.center().y),
                Align2::RIGHT_CENTER,
                Style::new(9.5, 700, th.caption),
                Some(w * 0.5),
            );
            if resp.clicked() {
                chosen = Some((name.to_string(), b.clone()));
            }
            y += row_h;
        }
        caps(
            &p,
            "YOUR BOARDS",
            pos2(r.min.x + px(14.0), y + row_h / 2.0),
            10.0,
            th.caption,
            Align2::LEFT_CENTER,
        );
        if saved.is_empty() {
            text::draw(
                &p,
                "none yet: Save as\u{2026}",
                pos2(r.max.x - px(14.0), y + row_h / 2.0),
                Align2::RIGHT_CENTER,
                Style::new(10.0, 600, th.caption),
                None,
            );
        }
        y += row_h;
        let mut delete: Option<String> = None;
        for (name, b) in &saved {
            let rr = Rect::from_min_size(pos2(r.min.x + px(6.0), y), vec2(w - px(12.0), row_h));
            let xr = Rect::from_center_size(pos2(rr.max.x - px(14.0), rr.center().y), Vec2::splat(px(18.0)));
            let resp = interact(ui, rr, ("board-saved", name.as_str()), Sense::click());
            if resp.hovered() {
                p.rect_filled(rr, 8.0, th.screen_hi);
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            text::draw(
                &p,
                name,
                pos2(rr.min.x + px(10.0), rr.center().y),
                Align2::LEFT_CENTER,
                Style::new(13.0, 700, th.text),
                Some(w - px(60.0)),
            );
            if widgets::round_button(
                ui,
                xr,
                ("board-saved-x", name.as_str()),
                Icon::X,
                8.0,
                RoundKind::Ivory,
                false,
            )
            .clicked()
            {
                delete = Some(name.clone());
            } else if resp.clicked() {
                chosen = Some((name.clone(), b.clone()));
            }
            y += row_h;
        }
        if let Some(n) = delete {
            pb::delete_saved(&mut st.config, &n);
            st.save_config();
            self.say(format!("Deleted \u{201c}{n}\u{201d}"), "");
        }
        if let Some((name, b)) = chosen {
            self.board.menu = false;
            self.load_board(st, &name, b);
        }
    }
}

/// A chain unit's remove key (top right) and "i" dot (bottom right): apart, so neither covers the other (owner,
/// P37.6c).
pub fn chip_keys(r: Rect) -> (Rect, Rect) {
    let d = px(18.0);
    (
        Rect::from_center_size(pos2(r.max.x - px(3.0), r.min.y + px(3.0)), Vec2::splat(d)),
        Rect::from_center_size(pos2(r.max.x - px(3.0), r.max.y - px(3.0)), Vec2::splat(d)),
    )
}

#[cfg(test)]
mod drag_tests {
    use super::*;

    #[test]
    fn the_remove_key_and_the_info_dot_are_apart() {
        for (w, h) in [(40.0, 40.0), (80.0, 60.0), (24.0, 30.0)] {
            let r = Rect::from_min_size(pos2(100.0, 50.0), vec2(w, h));
            let (x, i) = chip_keys(r);
            assert!(!x.intersects(i), "{w}x{h}: {x:?} {i:?}");
            assert!(x.center().y < i.center().y);
        }
    }

    #[test]
    fn a_click_on_the_remove_key_takes_the_unit_off() {
        let ctx = egui::Context::default();
        qrs_ui::text::install(&ctx);
        let dir = std::env::temp_dir().join(format!("qrs-chain-x-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = AppState::for_test(&ctx, dir.clone(), Default::default());
        let mut rack = Rack::new(None);
        rack.board.pick = Some(Pick::Mine);
        rack.board_edit(&mut st, |b| {
            *b = Board::new(
                ["green", "amp_crunch", "cab_4x12"]
                    .iter()
                    .map(|id| Block::of(id).unwrap())
                    .collect(),
            );
        });
        let target = rack.board_now(&st).blocks[1].uid;
        let well = Rect::from_min_size(pos2(20.0, 20.0), vec2(600.0, 120.0));
        let frame = |rack: &mut Rack, st: &mut AppState, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                events,
                ..Default::default()
            };
            ctx.begin_pass(input);
            egui::Area::new(egui::Id::new("chain-test"))
                .fixed_pos(Pos2::ZERO)
                .show(&ctx, |ui| {
                    ui.set_min_size(vec2(800.0, 400.0));
                    rack.chain_ui(ui, st, well, "test", true);
                });
            ctx.end_pass().textures_delta.clear();
        };
        frame(&mut rack, &mut st, Vec::new());
        let r = rack.board.chips.iter().find(|c| c.0 == target).unwrap().1;
        let x = chip_keys(r).0.center();
        let press = |pressed| egui::Event::PointerButton {
            pos: x,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut rack, &mut st, vec![egui::Event::PointerMoved(x)]);
        frame(&mut rack, &mut st, vec![press(true)]);
        frame(&mut rack, &mut st, vec![press(false)]);
        frame(&mut rack, &mut st, Vec::new());
        let b = rack.board_now(&st);
        assert_eq!(b.blocks.len(), 2, "the amp is off the chain");
        assert!(b.index_of(target).is_none());
        assert!(rack.board.moving.is_none(), "no drag started");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Drives a real egui drag on the chain strip: press on a unit, move past its neighbour, release (P38.1).
    pub(crate) fn drag_on_chain(
        tag: &'static str,
        names: bool,
        from: usize,
        past: usize,
    ) -> (Vec<u32>, Vec<u32>) {
        let ctx = egui::Context::default();
        qrs_ui::text::install(&ctx);
        let dir = std::env::temp_dir().join(format!("qrs-chain-drag-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = AppState::for_test(&ctx, dir.clone(), Default::default());
        let mut rack = Rack::new(None);
        // (as the player does each frame: board_update runs before the strip draws, and once first for the song)
        rack.board_update(&mut st);
        rack.board.pick = Some(Pick::Mine);
        rack.board_edit(&mut st, |b| {
            *b = Board::new(
                ["green", "amp_crunch", "cab_4x12"]
                    .iter()
                    .map(|id| Block::of(id).unwrap())
                    .collect(),
            );
        });
        let before: Vec<u32> = rack.board_now(&st).blocks.iter().map(|b| b.uid).collect();
        let well = Rect::from_min_size(pos2(20.0, 20.0), vec2(600.0, 120.0));
        let mut time = 0.0;
        let mut frame = |rack: &mut Rack, st: &mut AppState, events: Vec<egui::Event>| {
            time += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            ctx.begin_pass(input);
            rack.board_update(st);
            egui::Area::new(egui::Id::new("chain-test"))
                .fixed_pos(Pos2::ZERO)
                .show(&ctx, |ui| {
                    ui.set_min_size(vec2(800.0, 400.0));
                    rack.chain_ui(ui, st, well, tag, names);
                });
            ctx.end_pass().textures_delta.clear();
        };
        frame(&mut rack, &mut st, Vec::new());
        let rect_of = |rack: &Rack, uid: u32| rack.board.chips.iter().find(|c| c.0 == uid).unwrap().1;
        let a = rect_of(&rack, before[from]).center();
        let b = rect_of(&rack, before[past]);
        let to = pos2(if past > from { b.max.x + 6.0 } else { b.min.x - 6.0 }, a.y);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut rack, &mut st, vec![egui::Event::PointerMoved(a)]);
        frame(&mut rack, &mut st, vec![button(a, true)]);
        for k in 1..=12 {
            let q = a + (to - a) * (k as f32 / 12.0);
            frame(&mut rack, &mut st, vec![egui::Event::PointerMoved(q)]);
        }
        frame(&mut rack, &mut st, vec![button(to, false)]);
        frame(&mut rack, &mut st, Vec::new());
        let after: Vec<u32> = rack.board_now(&st).blocks.iter().map(|b| b.uid).collect();
        let _ = std::fs::remove_dir_all(&dir);
        (before, after)
    }

    #[test]
    fn dragging_a_unit_past_its_neighbour_reorders_the_chain() {
        for (tag, names) in [("room-chain", true), ("tile", false)] {
            let (b, a) = drag_on_chain(tag, names, 0, 1);
            assert_eq!(a, vec![b[1], b[0], b[2]], "{tag}: the pedal moved after the amp");
            let (b, a) = drag_on_chain(tag, names, 2, 0);
            assert_eq!(a, vec![b[2], b[0], b[1]], "{tag}: the cab moved to the front");
        }
    }

    fn held(tag: &'static str) -> KnobDrag {
        KnobDrag {
            tag,
            uid: 7,
            knob: 0,
            start: pos2(100.0, 100.0),
            u0: 0.5,
            across: false,
            len: 160.0,
        }
    }

    #[test]
    fn a_second_face_with_another_tag_does_not_cancel_the_drag() {
        // the editor holds the drag; the tile's copy of the same knob, drawn first, isn't pressed
        let d = held("editor");
        let tile = drag_step(Some(&d), "tile", 7, 0, false, Some(pos2(100.0, 60.0)), true);
        assert_eq!(tile, DragAct::Idle);
        // the editor's own copy turns it by how far the pointer went up
        let ed = drag_step(Some(&d), "editor", 7, 0, true, Some(pos2(100.0, 60.0)), true);
        assert_eq!(ed, DragAct::Turn(40.0));
        // let go: only the owner ends it
        assert_eq!(
            drag_step(Some(&d), "tile", 7, 0, false, None, false),
            DragAct::Idle
        );
        assert_eq!(
            drag_step(Some(&d), "editor", 7, 0, false, None, false),
            DragAct::End
        );
    }

    #[test]
    fn a_press_starts_a_drag_and_other_knobs_stay_idle() {
        assert_eq!(
            drag_step(None, "room", 7, 2, true, Some(pos2(5.0, 5.0)), true),
            DragAct::Start
        );
        assert_eq!(
            drag_step(None, "room", 7, 2, false, Some(pos2(5.0, 5.0)), true),
            DragAct::Idle
        );
        let d = held("room");
        assert_eq!(
            drag_step(Some(&d), "room", 7, 1, false, Some(pos2(5.0, 5.0)), true),
            DragAct::Idle
        );
        // the pointer outside the knob's rect still turns the knob that holds it
        assert_eq!(
            drag_step(Some(&d), "room", 7, 0, false, Some(pos2(400.0, 0.0)), true),
            DragAct::Turn(100.0)
        );
    }

    #[test]
    fn the_drop_index_uses_only_the_dragging_strips_rects() {
        let r = |x: f32| Rect::from_min_size(pos2(x, 0.0), vec2(40.0, 40.0));
        // the view's strip: blocks 1, 2, 3 at x 100, 200, 300; block 1 dropped at x 260 (centre 280)
        let view = [(1, r(100.0)), (2, r(200.0)), (3, r(300.0))];
        assert_eq!(drop_index(&view, 1, 280.0), 1);
        assert_eq!(drop_index(&view, 1, 400.0), 2);
        assert_eq!(drop_index(&view, 3, 0.0), 0);
        // (the tile's strip far to the right would have said 0 for the same drop: it isn't asked any more)
        let tile = [(1, r(1100.0)), (2, r(1200.0)), (3, r(1300.0))];
        assert_eq!(drop_index(&tile, 1, 280.0), 0);
    }

    #[test]
    fn the_wheel_list_is_cleared_once_a_frame() {
        let ctx = egui::Context::default();
        let mut rack = Rack::new(None);
        for frame in 0..5 {
            ctx.begin_pass(Default::default());
            rack.begin_frame(&ctx);
            for k in 0..30 {
                rack.board.wheel.push(WheelSpot {
                    uid: 1,
                    knob: k,
                    rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0)),
                });
            }
            // (a second caller in the same frame keeps what the first pushed)
            rack.begin_frame(&ctx);
            assert_eq!(rack.wheel_len(), 30, "frame {frame}");
            ctx.end_pass().textures_delta.clear();
        }
    }
}
