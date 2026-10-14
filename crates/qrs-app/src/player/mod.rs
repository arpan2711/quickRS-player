//! The player (port of rsplay/app.py `PlayerView`): the song's header with the part keys, the speed knob and the
//! guitar's status strip; the tab (tab.rs); the transport, seek groove and volume knobs in the footer; the
//! console on the right with the tiles (tiles.rs) or the controls (controls.rs); the count-in, toasts, the
//! slow-down card, the sections list and the results card. Every key of the Python player works the same.

mod controls;
mod tab;
pub mod tiles;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Event, Key, Painter, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_analysis::{Judgement, Pluck, Scorer, Spectra};
use qrs_audio::stretch::StretchJob;
use qrs_audio::synth::{SynthNote, SynthPlayer, SynthState};
use qrs_formats::ascii_tab::{note_token, section_label};
use qrs_formats::lyrics::Line;
use qrs_formats::model::{Beat, Chord, HandShape, Note, Section};
use qrs_formats::rhythm::{Options, Rhythm, for_arrangement};
use qrs_formats::{Arrangement, Techs};
use qrs_library::SongEntry;
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, Gem, keycap};
use qrs_ui::knobs::{self, KnobStyle, Scale};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{self, fmt_time, t};
use qrs_ui::units::{self, pixels, px, pxr, to_px};
use qrs_ui::widgets::{self, ButtonStyle, RoundKind, Segmented, button, button_width, interact};

use crate::home::{fmt_when, group_digits};
use crate::library::{caps, txt};
use crate::state::AppState;
use crate::transport::{Source, Transport};
pub use tab::{TabAct, TabCtx, TabView};

/// Seconds played before the loop start on each pass.
pub const LOOP_LEAD_IN: f64 = 1.5;
/// A jump to a section (the sections list, PgUp / PgDn) lands this long before it.
pub const SECTION_LEAD_IN: f64 = 1.0;
pub const COUNTDOWN: f32 = 3.0;
pub const LEVEL_UP_ACCURACY: f64 = 0.9;
pub const SPEEDS: [f64; 24] = [
    0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0, 1.05, 1.1, 1.15,
    1.2, 1.25, 1.3, 1.4, 1.5,
];
/// Scale-1 pixels of mouse travel per step of a practice knob, and for a volume knob's whole range.
const DIAL_STEP: f32 = 14.0;
const DIAL_TRAVEL: f32 = 160.0;
const ZOOM_HUD: f32 = 1.4;
const SEC_ROW: f32 = 40.0;
const SEC_ROWS: f32 = 8.5;
/// Seconds into a run of arrow presses, and the step then; 20 s after.
const SCRUB_STEPS: [(f32, i32); 4] = [(0.5, 1), (1.0, 2), (2.0, 5), (3.5, 10)];
const SCRUB_GAP: f32 = 0.5;
const SCRUB_EVERY: f32 = 0.08;

static CHART_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

// -- the Python app's whole pixels ----------------------------------------------------------------------------
// The player's layout numbers as the Python app makes them (PLAN.md P10.2): `px(n)` is its `int(n * s)`, `pxr(n)`
// its `neu.px(n)`, `snap(v)` its `int(v)` of a pixel position, and the helpers below pygame's Rect arithmetic.
// Positions handed to the text and the components are whole pixels, as the Python app's ints are.

/// `int(v)` of a position or length the Python app has in pixels, from points (a sum of `px` numbers, a text box's
/// edge). As `units::snap`, with margins f32 points survive: a point is only within about 1e-4 px of the pixel it
/// stands for (units::snap allows 1e-6, which drops some whole pixels by one), and the result sits 1/1000 px past
/// the pixel, because the text truncates its anchor at the window's pixels-per-point, which can be a hair under
/// the scale (a point exactly on pixel k then lands on k - 1); the components round, which that doesn't change.
fn snap(v: f32) -> f32 {
    snap_at(v, units::scale())
}

/// `snap` at the scale `s` (0: points pass through).
fn snap_at(v: f32, s: f64) -> f32 {
    if s <= 0.0 {
        return v;
    }
    let x = v as f64 * s;
    let k = (x + 5e-3f64.copysign(x)).trunc();
    ((k + 1e-3f64.copysign(k)) / s) as f32
}

/// A whole-pixel length in points as the Python app's int (for its pixel arithmetic, e.g. `w * 0.26`, and its
/// comparisons of whole pixels).
fn ipx(v: f32) -> f64 {
    to_px(snap(v)).round()
}

/// pygame's `r.centerx` (`x + w // 2`).
fn mid_x(r: Rect) -> f32 {
    snap(r.min.x + snap(r.width() / 2.0))
}

/// pygame's `r.centery`.
fn mid_y(r: Rect) -> f32 {
    snap(r.min.y + snap(r.height() / 2.0))
}

/// pygame's `r.center`.
fn mid(r: Rect) -> Pos2 {
    pos2(mid_x(r), mid_y(r))
}

/// pygame's `r.inflate(dx, dy)`: `x - dx / 2` (C division, toward zero), `w + dx`.
fn inflate(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::from_min_size(
        pos2(snap(r.min.x - snap(dx / 2.0)), snap(r.min.y - snap(dy / 2.0))),
        vec2(snap(r.width() + dx), snap(r.height() + dy)),
    )
}

/// A point at whole pixels, as the Python app's int positions.
fn ipos(x: f32, y: f32) -> Pos2 {
    pos2(snap(x), snap(y))
}

/// `neu.keycap_width`: the label's width and `px(12)`.
fn cap_w(ctx: &egui::Context, key: &str) -> f32 {
    if key.is_empty() {
        return 0.0;
    }
    text::width(ctx, key, Style::new(11.0, 700, Color32::WHITE)) + pxr(12.0)
}

/// `neu.button_width`.
fn button_w(ctx: &egui::Context, label: &str, key: &str, size: f32, icon: bool) -> f32 {
    let mut w = text::width(ctx, label, Style::new(size, 700, Color32::WHITE)) + pxr(28.0);
    if !key.is_empty() {
        w += cap_w(ctx, key) + pxr(8.0);
    }
    if icon {
        w += pxr(22.0);
    }
    w
}

/// A note as the tab draws it.
#[derive(Clone, Debug)]
pub struct Glyph {
    pub t: f64,
    pub string: i32,
    pub fret: i32,
    pub sus: f64,
    pub tech: Techs,
    pub slide_to: i32,
    pub bend: f64,
    pub in_chord: bool,
    /// Tab notation, e.g. "h7", "5/", "<12>", "x".
    pub token: String,
    /// Its group in the scorer (for its green / orange / red).
    pub group: Option<usize>,
}

/// One part at one difficulty, laid out for drawing.
pub struct Chart {
    pub id: u64,
    pub rhythm_id: u64,
    pub arr: Arrangement,
    pub glyphs: Vec<Glyph>,
    pub gtimes: Vec<f64>,
    pub chords: Vec<Chord>,
    pub ctimes: Vec<f64>,
    pub beats: Vec<Beat>,
    pub btimes: Vec<f64>,
    pub sections: Vec<Section>,
    pub n_strings: usize,
    pub string_names: Vec<&'static str>,
    pub length: f64,
    pub first_note: f64,
    pub opens: Vec<i32>,
    pub rhythm: Option<Rhythm>,
    pub single_notes: Vec<Note>,
    pub ntimes: Vec<f64>,
    pub shapes: Vec<HandShape>,
    pub shape_starts: Vec<f64>,
    pub shape_new: Vec<bool>,
    pub shape_picked: Vec<bool>,
}

impl Chart {
    pub fn new(arr: Arrangement, audio_len: f64, nota: &Nota) -> Chart {
        let slide = |n: &Note| {
            if n.slide_to >= 0 {
                n.slide_to
            } else {
                n.slide_unpitch_to
            }
        };
        let mut glyphs: Vec<Glyph> = arr
            .notes
            .iter()
            .map(|n| Glyph {
                t: n.time,
                string: n.string,
                fret: n.fret,
                sus: n.sustain,
                tech: n.techniques,
                slide_to: slide(n),
                bend: n.bend,
                in_chord: false,
                token: note_token(n),
                group: None,
            })
            .collect();
        let mut chords = arr.chords.clone();
        chords.sort_by(|a, b| a.time.total_cmp(&b.time));
        for c in &chords {
            for n in &c.notes {
                let techs = n.techniques | c.techniques;
                let mut nn = n.clone();
                nn.techniques = techs;
                glyphs.push(Glyph {
                    t: n.time,
                    string: n.string,
                    fret: n.fret,
                    sus: n.sustain.max(c.sustain),
                    tech: techs,
                    slide_to: slide(n),
                    bend: n.bend,
                    in_chord: true,
                    token: note_token(&nn),
                    group: None,
                });
            }
        }
        glyphs.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.string.cmp(&b.string)));
        let gtimes: Vec<f64> = glyphs.iter().map(|g| g.t).collect();
        let ctimes = chords.iter().map(|c| c.time).collect();
        let beats = arr.beats.clone();
        let btimes = beats.iter().map(|b| b.time).collect();
        let mut sections = arr.sections.clone();
        sections.sort_by(|a, b| a.start.total_cmp(&b.start));
        let length = arr
            .song_length
            .max(audio_len)
            .max(gtimes.last().map(|t| t + 2.0).unwrap_or(0.0));
        let first_note = gtimes.first().copied().unwrap_or(0.0);
        let opens = arr.open_string_midi();
        let mut single_notes: Vec<Note> = arr
            .notes
            .iter()
            .filter(|n| !n.techniques.has(Techs::IGNORE))
            .cloned()
            .collect();
        single_notes.sort_by(|a, b| a.time.total_cmp(&b.time));
        let ntimes: Vec<f64> = single_notes.iter().map(|n| n.time).collect();
        let mut shapes = arr.handshapes.clone();
        shapes.sort_by(|a, b| a.start.total_cmp(&b.start));
        if shapes.is_empty() {
            // charts without hand shapes (XML): one shape per chord
            shapes = chords
                .iter()
                .map(|c| HandShape {
                    start: c.time,
                    end: c.time + c.sustain.max(0.3),
                    name: c.name.clone(),
                    frets: if c.frets.is_empty() {
                        (0..6)
                            .map(|i| {
                                c.notes
                                    .iter()
                                    .find(|n| n.string == i)
                                    .map(|n| n.fret)
                                    .unwrap_or(-1)
                            })
                            .collect()
                    } else {
                        c.frets.clone()
                    },
                    fingers: c.fingers.clone(),
                    arpeggio: false,
                })
                .collect();
        }
        let shape_starts = shapes.iter().map(|h| h.start).collect();
        let mut shape_new = Vec::new();
        let mut shape_picked = Vec::new();
        let mut prev: Option<Vec<i32>> = None;
        for h in &shapes {
            shape_new.push(prev.as_ref() != Some(&h.frets));
            prev = Some(h.frets.clone());
            let i0 = ntimes.partition_point(|&x| x < h.start - 0.01);
            let i1 = ntimes.partition_point(|&x| x < h.end);
            shape_picked.push(h.arpeggio || i1 - i0 >= 2); // single notes inside: picked through
        }
        let mut chart = Chart {
            id: CHART_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            rhythm_id: 0,
            n_strings: arr.string_count(),
            string_names: arr.string_names(),
            arr,
            glyphs,
            gtimes,
            chords,
            ctimes,
            beats,
            btimes,
            sections,
            length,
            first_note,
            opens,
            rhythm: None,
            single_notes,
            ntimes,
            shapes,
            shape_starts,
            shape_new,
            shape_picked,
        };
        chart.build_rhythm(nota);
        chart
    }

    /// Rhythm notation for the tab view, from the chart's onsets and beat grid.
    pub fn build_rhythm(&mut self, nota: &Nota) {
        let opts = Options {
            snap: nota.snap,
            rests: nota.rests,
            beams: nota.beams,
            triplets: nota.triplets,
        };
        self.rhythm = if self.arr.beats.iter().any(|b| b.is_downbeat()) {
            Some(for_arrangement(&self.arr, opts))
        } else {
            None
        };
        self.rhythm_id = CHART_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// The beat nearest t.
    pub fn snap(&self, time: f64) -> f64 {
        if self.btimes.is_empty() {
            return time;
        }
        let i = self.btimes.partition_point(|&b| b < time);
        [i.wrapping_sub(1), i]
            .iter()
            .filter_map(|&j| self.btimes.get(j))
            .copied()
            .min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs()))
            .unwrap_or(time)
    }

    pub fn section_at(&self, time: f64) -> i64 {
        let mut idx = -1;
        for (i, s) in self.sections.iter().enumerate() {
            if s.start <= time + 0.05 {
                idx = i as i64;
            }
        }
        idx
    }

    pub fn synth_notes(&self) -> Vec<SynthNote> {
        self.glyphs
            .iter()
            .map(|g| SynthNote {
                t: g.t,
                string: g.string,
                fret: g.fret,
                sus: g.sus,
                tech: g.tech,
                slide_to: g.slide_to,
                bend: g.bend,
                in_chord: g.in_chord,
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nota {
    pub show: bool,
    pub snap: bool,
    pub rests: bool,
    pub beams: bool,
    pub triplets: bool,
}

impl Nota {
    pub fn from_config(st: &AppState) -> Nota {
        let n = st.config.get("notation").and_then(|v| v.as_object());
        let get = |k: &str| n.and_then(|m| m.get(k)).and_then(|v| v.as_bool()).unwrap_or(true);
        Nota {
            show: get("show"),
            snap: get("snap"),
            rests: get("rests"),
            beams: get("beams"),
            triplets: get("triplets"),
        }
    }

    pub fn to_json(self) -> serde_json::Value {
        serde_json::json!({"show": self.show, "snap": self.snap, "rests": self.rests, "beams": self.beams, "triplets": self.triplets})
    }
}

/// What the player asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerReq {
    Library,
    Settings(&'static str),
}

#[derive(Clone, Debug)]
struct DialDrag {
    kind: &'static str,
    y0: f32,
    steps: i32,
    v0: f32,
    moved: bool,
}

#[derive(Clone, Debug)]
struct SecMenu {
    key: usize,
    scroll: Option<f32>,
    show: bool,
}

#[derive(Clone, Debug)]
struct TileDrag {
    key: &'static str,
    grab: f32,
    y: f32,
}

pub struct PlayerView {
    pub g: usize,
    pub entry: SongEntry,
    /// Every id the song's history is under, the one it's played under, and its canonical id.
    pub ids: Vec<String>,
    pub sid: String,
    pub part_key: String,
    pub part_name: String,
    pub chart: Chart,
    pub mastery: f64,
    pub transport: Transport,
    pub audio_error: String,
    pub scorer: Scorer,
    states: Vec<Option<Judgement>>,
    pub tab: TabView,
    pub synth: SynthPlayer,
    pub sel_loop: bool,
    pub sel_slow: f64,
    pub sel_track: bool,
    pub loop_a: Option<f64>,
    pub loop_b: Option<f64>,
    loop_passes: u32,
    loop_acc: Option<f64>,
    /// +: notes later than audio.
    pub offset: f64,
    /// +: count your playing as earlier.
    pub input_offset: f64,
    toast: Option<(String, String, Instant)>,
    countdown_until: Option<Instant>,
    pub lyrics: Vec<Line>,
    lyric_starts: Vec<f64>,
    pub lyrics_next: bool,
    pub show_controls: bool,
    ctrl_scroll: f32,
    ctrl_target: Option<f32>,
    ctrl_max: f32,
    ctrl_goto: Option<&'static str>,
    group_y: std::collections::HashMap<&'static str, f32>,
    panel_scroll: f32,
    panel_max: f32,
    panel_rect: Rect,
    seek_drag: Option<f64>,
    bar: Rect,
    dial_drag: Option<DialDrag>,
    dials: Vec<(Rect, &'static str)>,
    tuning_choice: usize,
    tuner_hist: VecDeque<f64>,
    pub tuner_midi: Option<f64>,
    tuner_seen: Instant,
    pub tuner_text: String,
    tuner_at: Instant,
    zoom_at: Option<Instant>,
    tip: Option<(String, String, Pos2, Align2)>,
    sec_menu: Option<SecMenu>,
    sec_jump: Option<usize>,
    sec_label: Option<Rect>,
    sec_panel: Option<Rect>,
    pending_speed: Option<f64>,
    speed_at: Instant,
    stretch: Option<StretchJob>,
    pub nota: Nota,
    pub auto_level: bool,
    run_scored: bool,
    recorded: bool,
    new_best: bool,
    result_scored: bool,
    result_rank: Option<usize>,
    tile_drag: Option<TileDrag>,
    tile_rects: std::collections::HashMap<&'static str, Rect>,
    tile_h: std::collections::HashMap<&'static str, f32>,
    /// The back key, as last drawn (screenshot mode points at it for its tooltip).
    back_rect: Rect,
    /// Screenshot mode: the count-in held with this many seconds to go.
    countdown_hold: Option<f32>,
    /// Screenshot mode: a slow-down shown as this far prepared (the Python tool's engine stretch_progress).
    shot_stretch: Option<f64>,
    /// Screenshot mode: the two earlier runs are in the user data; the run recorded by the results shot.
    shot_runs: bool,
    shot_result: Option<(Option<usize>, bool)>,
    tiles_top: f32,
    well: Rect,
    scrub: Option<(Instant, i32, Instant)>,
    sp: Spectra,
    pitch_buf: Vec<f32>,
    pub reqs: Vec<PlayerReq>,
    acts: Vec<String>,
    /// The blurred player under the results card.
    backdrop: crate::backdrop::Backdrop,
    /// The MG-300: its screen (the block shown lives there), the tile's little pedal, the big view.
    #[cfg(feature = "nux")]
    pub mg_lcd: crate::mg300::lcd::LcdView,
    #[cfg(feature = "nux")]
    pub mg_mini: crate::mg300::mini::MiniPedal,
    #[cfg(feature = "nux")]
    pub mg_view: crate::mg300::view::BigView,
}

impl PlayerView {
    pub fn new(st: &mut AppState, loaded: crate::loading::Loaded) -> PlayerView {
        let crate::loading::Loaded {
            g,
            entry,
            part,
            arr,
            lyrics,
            song,
            audio_error,
        } = loaded;
        let nota = Nota::from_config(st);
        let mut audio_error = audio_error;
        let transport = match (st.audio.as_ref().map(|a| a.engine.clone()), song) {
            (Some(e), Some(song)) => {
                e.load_song(song.clone());
                Transport::new(Source::Engine {
                    e,
                    song: Arc::new(song),
                })
            }
            (Some(e), None) => {
                // no song audio: the engine plays silence as long as the chart, so timing works the same
                let len = arr.song_length.max(1.0);
                let silent = qrs_audio::decode::SongBuffer::silent(e.rate(), len);
                e.load_song(silent.clone());
                Transport::new(Source::Engine {
                    e,
                    song: Arc::new(silent),
                })
            }
            (None, _) => {
                if audio_error.is_empty() {
                    audio_error = "the audio engine isn't running".into();
                }
                Transport::new(Source::Silent(crate::transport::SilentClock::new(
                    arr.song_length.max(1.0),
                )))
            }
        };
        if !audio_error.is_empty() {
            crate::applog::write(&format!("{}: no song audio: {audio_error}", entry.path));
        }
        let part_name = entry
            .arrangements
            .iter()
            .find(|a| a.key == part)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| arr.name.clone());
        let chart = Chart::new(arr, transport.duration(), &nota);
        let scorer = Scorer::new(&chart.arr);
        let ids = st.ids(g);
        let sid = entry.id();
        let canonical = st.groups[g].canonical.clone();
        st.userdata.alias(&canonical, &sid);
        st.userdata.started(&sid, &part_name);
        let mut transport = transport;
        transport.set_volume(st.config.f64("volume", 0.8) as f32);
        let lyric_starts = lyrics.iter().map(|l| l.start()).collect();
        let mut p = PlayerView {
            g,
            entry,
            ids,
            sid,
            part_key: part,
            part_name,
            chart,
            mastery: 1.0,
            transport,
            audio_error,
            scorer,
            states: Vec::new(),
            tab: TabView::default(),
            synth: SynthPlayer::default(),
            sel_loop: st.config.bool("sel_loop", false),
            sel_slow: st.config.f64("sel_slow", 1.0),
            sel_track: st.config.str("sel_source", "synth") == "track",
            loop_a: None,
            loop_b: None,
            loop_passes: 0,
            loop_acc: None,
            offset: st.config.f64("av_offset", 0.0),
            input_offset: st.config.f64("input_offset", 0.0),
            toast: None,
            countdown_until: Some(Instant::now() + Duration::from_secs_f32(COUNTDOWN)),
            lyrics,
            lyric_starts,
            lyrics_next: st.config.bool("lyrics_next", true),
            show_controls: false,
            ctrl_scroll: 0.0,
            ctrl_target: None,
            ctrl_max: 0.0,
            ctrl_goto: None,
            group_y: Default::default(),
            panel_scroll: 0.0,
            panel_max: 0.0,
            panel_rect: Rect::NOTHING,
            seek_drag: None,
            bar: Rect::NOTHING,
            dial_drag: None,
            dials: Vec::new(),
            tuning_choice: 0,
            tuner_hist: VecDeque::new(),
            tuner_midi: None,
            tuner_seen: Instant::now(),
            tuner_text: String::new(),
            tuner_at: Instant::now(),
            zoom_at: None,
            tip: None,
            sec_menu: None,
            sec_jump: None,
            sec_label: None,
            sec_panel: None,
            pending_speed: None,
            speed_at: Instant::now(),
            stretch: None,
            nota,
            auto_level: st.config.bool("auto_level", true),
            run_scored: true,
            recorded: false,
            new_best: false,
            result_scored: false,
            result_rank: None,
            tile_drag: None,
            tile_rects: Default::default(),
            tile_h: Default::default(),
            back_rect: Rect::NOTHING,
            countdown_hold: None,
            shot_stretch: None,
            shot_runs: false,
            shot_result: None,
            tiles_top: 0.0,
            well: Rect::NOTHING,
            scrub: None,
            sp: Spectra::new(),
            pitch_buf: vec![0.0; 8192],
            reqs: Vec::new(),
            acts: Vec::new(),
            backdrop: Default::default(),
            #[cfg(feature = "nux")]
            mg_lcd: Default::default(),
            #[cfg(feature = "nux")]
            mg_mini: Default::default(),
            #[cfg(feature = "nux")]
            mg_view: Default::default(),
        };
        p.relink_glyphs();
        if let Some(e) = p.transport.engine() {
            let _ = e.take_onsets(); // plucks before the song started don't count
        }
        p
    }

    fn stop_synth(&mut self) {
        let e = self.engine().cloned();
        self.synth.stop_maybe(e.as_deref());
    }

    pub fn engine(&self) -> Option<&Arc<qrs_audio::Engine>> {
        self.transport.engine()
    }

    fn has_input(&self) -> bool {
        self.engine().is_some_and(|e| e.status().input.is_some())
    }

    pub fn say(&mut self, text: impl Into<String>, tail: impl Into<String>) {
        self.say_for(text, tail, 1.6);
    }

    /// A toast that stays `secs` (the device notices).
    pub fn say_for(&mut self, text: impl Into<String>, tail: impl Into<String>, secs: f32) {
        self.toast = Some((
            text.into(),
            tail.into(),
            Instant::now() + Duration::from_secs_f32(secs),
        ));
    }

    /// Chart time at the now-line.
    pub fn now(&self) -> f64 {
        self.transport.position() - self.offset
    }

    /// The section you're in (-1 before the first): after a jump to one, that one already during the second
    /// before it starts.
    fn section_here(&self) -> i64 {
        let now = self.now();
        if let Some(j) = self.sec_jump
            && let Some(sec) = self.chart.sections.get(j)
            && sec.start - SECTION_LEAD_IN - 0.25 <= now
            && now < sec.start
        {
            return j as i64;
        }
        self.chart.section_at(now)
    }

    pub fn seek(&mut self, time: f64) {
        self.countdown_until = None;
        self.sec_jump = None;
        let time = time.clamp(0.0, self.transport.duration());
        self.transport.seek(time);
        self.tab.follow = true; // the cursor jumped: bring the tab page to it
        self.scorer.reset_from(time - self.offset);
        if time - self.offset < self.chart.first_note - 0.5 {
            // back to the start: a fresh run
            self.restart_counters();
            self.run_scored = self.at_max_settings();
        }
    }

    fn restart_counters(&mut self) {
        self.scorer = Scorer::new(&self.chart.arr);
        if let Some(e) = self.engine() {
            let _ = e.take_onsets();
        }
        self.relink_glyphs();
    }

    /// Point each drawn note at its group in the current scorer.
    fn relink_glyphs(&mut self) {
        let stimes: Vec<f64> = self.scorer.groups.iter().map(|g| g.t).collect();
        for gl in &mut self.chart.glyphs {
            gl.group = None;
            let j = stimes.partition_point(|&x| x < gl.t - 0.016);
            if j < stimes.len() && (stimes[j] - gl.t).abs() <= 0.016 {
                gl.group = Some(j);
            }
        }
    }

    pub fn speed(&self) -> f64 {
        self.pending_speed.unwrap_or_else(|| self.transport.speed())
    }

    /// A high score only counts at full difficulty and full speed.
    fn at_max_settings(&self) -> bool {
        self.mastery >= 0.999 && (self.speed() - 1.0).abs() < 1e-6
    }

    fn started_playing(&self) -> bool {
        self.now() > self.chart.first_note - 0.5
    }

    fn track_ok(&self) -> bool {
        self.engine().is_some() && self.audio_error.is_empty()
    }

    /// How fast the synth plays the selection: the song's speed, or slower with the Slow key.
    fn selection_speed(&self) -> f64 {
        self.speed().min(self.sel_slow)
    }

    /// (Re)load the chart for a part at a difficulty, keeping the song playing. Fresh counters; if the run had
    /// already started it becomes a practice run.
    fn load_chart(&mut self, st: &AppState, key: &str, mastery: f64) -> bool {
        let Some(info) = self.entry.arrangements.iter().find(|a| a.key == key).cloned() else {
            return false;
        };
        let sk = (!self.entry.song_key.is_empty()).then_some(self.entry.song_key.as_str());
        let arr = match qrs_formats::Package::open(&self.entry.path, &st.keys, sk)
            .and_then(|mut p| p.load(Some(key), mastery))
        {
            Ok(a) => a,
            Err(e) => {
                self.say(format!("Couldn't load {}: {e}", info.name), "");
                return false;
            }
        };
        self.part_key = key.to_string();
        self.mastery = mastery;
        self.part_name = info.name.clone();
        let started = self.started_playing();
        self.chart = Chart::new(arr, self.transport.duration(), &self.nota);
        self.scorer = Scorer::new(&self.chart.arr);
        self.relink_glyphs();
        self.stop_synth();
        let now = self.now();
        self.scorer.reset_from(now);
        if let Some(e) = self.engine() {
            let _ = e.take_onsets(); // plucks so far were for the old chart
        }
        self.recorded = false;
        self.new_best = false;
        self.result_rank = None;
        if started {
            self.run_scored = false;
        }
        true
    }

    fn change_mastery(&mut self, st: &AppState, step: f64) {
        let new = ((self.mastery + step).clamp(0.0, 1.0) * 100.0).round() / 100.0;
        if (new - self.mastery).abs() < 1e-9 {
            self.say(
                if step > 0.0 {
                    "Already at full difficulty"
                } else {
                    "Already at the easiest level"
                },
                "",
            );
            return;
        }
        let key = self.part_key.clone();
        if self.load_chart(st, &key, new) {
            self.say(format!("Difficulty {}%", (new * 100.0).round() as i64), "");
        }
    }

    fn change_speed(&mut self, step: i32) {
        if self.engine().is_none() {
            self.say("Speed control needs the audio engine", "");
            return;
        }
        let cur = self.speed();
        let i = (0..SPEEDS.len())
            .min_by(|&a, &b| (SPEEDS[a] - cur).abs().total_cmp(&(SPEEDS[b] - cur).abs()))
            .unwrap_or(15) as i32;
        let new = SPEEDS[(i + step).clamp(0, SPEEDS.len() as i32 - 1) as usize];
        // applied after a short pause, so tapping three times builds one version
        self.pending_speed = Some(new);
        self.speed_at = Instant::now();
        self.say(format!("Speed {}%", (new * 100.0).round() as i64), "");
    }

    fn apply_speed(&mut self, speed: f64) {
        let Source::Engine { e, song } = &self.transport.source else {
            return;
        };
        if let Some(job) = self.stretch.take() {
            job.cancel();
        }
        if (speed - 1.0).abs() < 1e-6 {
            e.set_speed(1.0, None);
        } else {
            let from = e.position();
            let job = StretchJob::start(song.clone(), speed, from);
            e.set_speed(speed, Some(job.buffer.clone()));
            self.stretch = Some(job);
        }
    }

    /// Arrangement names, numbered when a song has two of the same ("Combo", "Combo 2").
    fn part_labels(&self) -> Vec<String> {
        let names: Vec<&str> = self.entry.arrangements.iter().map(|a| a.name.as_str()).collect();
        let mut seen: std::collections::HashMap<&str, usize> = Default::default();
        self.entry
            .arrangements
            .iter()
            .map(|a| {
                let n = seen.entry(a.name.as_str()).or_default();
                *n += 1;
                let label = if names.iter().filter(|x| **x == a.name).count() == 1 {
                    a.name.clone()
                } else {
                    format!("{} {n}", a.name)
                };
                label + if a.bonus { " (bonus)" } else { "" }
            })
            .collect()
    }

    fn part_label(&self, key: &str) -> String {
        let labels = self.part_labels();
        self.entry
            .arrangements
            .iter()
            .position(|a| a.key == key)
            .map(|i| labels[i].clone())
            .unwrap_or_else(|| self.part_name.clone())
    }

    fn set_part(&mut self, st: &mut AppState, key: &str) {
        if key == self.part_key {
            self.say(format!("Already playing {}", self.part_label(key)), "");
            return;
        }
        let mastery = self.mastery;
        if self.load_chart(st, key, mastery) {
            st.config.set("last_arrangement", self.part_name.to_lowercase());
            st.userdata.started(&self.sid, &self.part_name);
            self.say(
                format!("{}  ·  {}", self.part_label(key), self.chart.arr.tuning_name()),
                "",
            );
        }
    }

    fn go_to_section(&mut self, i: usize) {
        let Some(sec) = self.chart.sections.get(i).cloned() else {
            return;
        };
        self.sec_menu = None;
        self.seek(sec.start - SECTION_LEAD_IN);
        self.sec_jump = Some(i);
        self.say(section_label(&sec), "");
    }

    /// Seconds to seek for one press of an arrow (signed), growing the longer you keep pressing or holding it; 0
    /// when this press comes too soon after the last one (key repeat).
    fn scrub_step(&mut self, dir: i32) -> i32 {
        let now = Instant::now();
        let (last, run_dir, mut run_start) = self.scrub.unwrap_or((now - Duration::from_secs(10), 0, now));
        let since = now.duration_since(last).as_secs_f32();
        if dir != run_dir || since > SCRUB_GAP {
            run_start = now;
        } else if since < SCRUB_EVERY {
            return 0;
        }
        self.scrub = Some((now, dir, run_start));
        let held = now.duration_since(run_start).as_secs_f32();
        dir * SCRUB_STEPS
            .iter()
            .find(|(t0, _)| held < *t0)
            .map(|s| s.1)
            .unwrap_or(20)
    }

    // -- the selection ----------------------------------------------------------------------------------------

    fn play_selection(&mut self, toggle: bool) {
        let Some((a, b)) = self.tab.sel else { return };
        if toggle && self.synth.state != SynthState::Idle {
            self.stop_synth();
            return;
        }
        let speed = self.selection_speed();
        if self.sel_track && self.track_ok() {
            // the song itself: any passage, notes or not
            self.countdown_until = None;
            if self.transport.playing() {
                self.transport.pause();
            }
            if let Some(song) = self.transport.song().cloned() {
                self.synth
                    .start_track(song, a, b, self.offset, speed, self.sel_loop);
            }
            return;
        }
        let snap = self.nota.snap;
        let any = self.chart.gtimes.iter().any(|&g| {
            let tt = TabView::tt(&self.chart, snap, g);
            a <= tt && tt < b
        });
        if !any {
            self.stop_synth();
            self.say("No notes in the selection", "");
            return;
        }
        let Some(e) = self.engine().cloned() else {
            self.say("The synth needs the audio engine", "");
            return;
        };
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
        let notes = self.chart.synth_notes();
        let opens = self.chart.opens.clone();
        // where each note is drawn: snapped onto the beat when it's charted just off it
        let snapped: Vec<(f64, f64)> = match &self.chart.rhythm {
            Some(r) if snap => self
                .chart
                .gtimes
                .iter()
                .map(|&g| (g, r.snapped_time(g)))
                .collect(),
            _ => Vec::new(),
        };
        let place: Arc<dyn Fn(f64) -> f64 + Send + Sync> =
            Arc::new(
                move |tm: f64| match snapped.binary_search_by(|p| p.0.total_cmp(&tm)) {
                    Ok(i) => snapped[i].1,
                    Err(_) => tm,
                },
            );
        let tone = qrs_audio::synth::synth_tone(&e.tone(), self.chart.arr.is_bass);
        self.synth.start(
            notes,
            opens,
            a,
            b,
            speed,
            self.chart.arr.is_bass,
            self.sel_loop,
            tone,
            e.rate(),
            place,
        );
    }

    // -- actions ----------------------------------------------------------------------------------------------

    fn set_volume(&mut self, st: &mut AppState, kind: &str, v: f32) {
        if kind == "vol" {
            let v = ((v.clamp(0.0, 1.0)) * 100.0).round() / 100.0;
            self.transport.set_volume(v);
            st.config.set("volume", v as f64);
        } else if let Some(e) = self.engine() {
            let v = ((v.clamp(0.0, 2.0)) * 100.0).round() / 100.0;
            e.set_guitar_gain(v);
            st.config.set("guitar_volume", v as f64);
        }
    }

    /// Pick up changes made on the settings page.
    pub fn reload_settings(&mut self, st: &AppState) {
        self.lyrics_next = st.config.bool("lyrics_next", true);
        self.auto_level = st.config.bool("auto_level", true);
        self.offset = st.config.f64("av_offset", 0.0);
        self.input_offset = st.config.f64("input_offset", 0.0);
        let old = self.nota;
        self.nota = Nota::from_config(st);
        if old.rests != self.nota.rests || old.triplets != self.nota.triplets || old.snap != self.nota.snap {
            let nota = self.nota;
            self.chart.build_rhythm(&nota);
        }
    }

    /// Skip the count-in and play (--play).
    pub fn start_now(&mut self) {
        self.countdown_until = None;
        if !self.transport.playing() {
            self.transport.play();
        }
    }

    /// A panel opens over the player: stop the song (and a count-in, which would start it under the panel) and let
    /// go of anything being dragged.
    pub fn hold(&mut self) {
        self.cancel_drags();
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
    }

    /// Let go of anything being dragged.
    pub fn cancel_drags(&mut self) {
        self.dial_drag = None;
        self.seek_drag = None;
        self.tile_drag = None;
        self.tab.cancel_drags();
    }

    pub fn act(&mut self, st: &mut AppState, name: &str) {
        let pos = self.transport.position();
        match name {
            "library" => self.reqs.push(PlayerReq::Library),
            #[cfg(feature = "nux")]
            "mg:open" => self.mg_view.open = true,
            #[cfg(feature = "nux")]
            "mg:close" => self.mg_view.close(),
            "settings" => self.reqs.push(PlayerReq::Settings("audio")),
            "controls" => {
                self.show_controls = !self.show_controls;
                self.tile_drag = None;
            }
            "practice" => {
                self.show_controls = true;
                let mut opened = controls::panel_open(st);
                opened.insert("PRACTICE".into());
                controls::set_panel_open(st, &opened);
                self.ctrl_goto = Some("PRACTICE");
            }
            "sel_play" => self.play_selection(true),
            "sel_loop" => {
                self.sel_loop = !self.sel_loop;
                st.config.set("sel_loop", self.sel_loop);
                if self.synth.state != SynthState::Idle {
                    self.play_selection(false);
                }
                self.say(
                    if self.sel_loop {
                        "Selection loops"
                    } else {
                        "Selection plays once"
                    },
                    "",
                );
            }
            "sel_source" => {
                if !self.track_ok() {
                    self.say("No song audio to play the selection from", "");
                } else {
                    self.sel_track = !self.sel_track;
                    st.config
                        .set("sel_source", if self.sel_track { "track" } else { "synth" });
                    self.say(
                        "Selection plays: ",
                        if self.sel_track {
                            "the song"
                        } else {
                            "the synth guitar"
                        },
                    );
                    if self.synth.state != SynthState::Idle {
                        self.play_selection(false);
                    }
                }
            }
            "sel_slow" => {
                self.sel_slow = match (self.sel_slow * 100.0).round() as i64 {
                    100 => 0.7,
                    70 => 0.5,
                    _ => 1.0,
                };
                st.config.set("sel_slow", self.sel_slow);
                if self.synth.state != SynthState::Idle {
                    self.play_selection(false);
                }
                self.say(
                    format!(
                        "Selection plays at {}%",
                        (self.selection_speed() * 100.0).round() as i64
                    ),
                    "",
                );
            }
            "sel_clear" => {
                self.stop_synth();
                self.tab.sel = None;
            }
            "play" => {
                self.stop_synth();
                if self.countdown_until.is_some() {
                    self.countdown_until = None;
                    self.transport.play();
                } else if self.transport.finished() {
                    self.seek(0.0); // play again: a fresh run from the top
                    self.transport.play();
                } else {
                    self.transport.toggle();
                }
            }
            "back" | "fwd" => {
                let step = self.scrub_step(if name == "fwd" { 1 } else { -1 });
                if step != 0 {
                    self.seek(pos + step as f64);
                    self.say(format!("{step:+}s"), "");
                }
            }
            "back1" => self.seek(pos - 1.0),
            "fwd1" => self.seek(pos + 1.0),
            "rewind" => {
                self.seek(pos - 5.0);
                self.say("Rewind 5s", "");
            }
            "start" => self.seek(0.0),
            "zoom_in" => {
                self.tab.zoom = (self.tab.zoom * 1.15).min(2.5);
                self.zoom_at = Some(Instant::now());
            }
            "zoom_out" => {
                self.tab.zoom = (self.tab.zoom / 1.15).max(0.5);
                self.zoom_at = Some(Instant::now());
            }
            "theme" => st.toggle_theme(),
            "sections" => {
                if self.sec_menu.is_some() || self.chart.sections.is_empty() {
                    self.sec_menu = None;
                } else {
                    self.sec_menu = Some(SecMenu {
                        key: self.section_here().max(0) as usize,
                        scroll: None,
                        show: false,
                    });
                }
            }
            "next_section" => {
                let i = self.section_here();
                if ((i + 1) as usize) < self.chart.sections.len() {
                    self.go_to_section((i + 1) as usize);
                }
            }
            "prev_section" => {
                let mut i = self.section_here();
                if i >= 0 && self.now() - self.chart.sections[i as usize].start < 2.0 {
                    i -= 1;
                }
                if i >= 0 {
                    self.go_to_section(i as usize);
                } else {
                    self.sec_menu = None;
                    self.seek(0.0);
                }
            }
            "loop_start" => {
                let a = self.chart.snap(self.now());
                self.loop_a = Some(a);
                if self.loop_b.is_some_and(|b| b <= a) {
                    self.loop_b = None;
                }
                self.loop_passes = 0;
                self.loop_acc = None;
                self.say(format!("Loop start {}", fmt_time(a)), "");
            }
            "loop_end" => {
                let end = self.chart.snap(self.now());
                if self.loop_a.is_none() {
                    let i = self.chart.section_at(self.now());
                    self.loop_a = Some(if i >= 0 {
                        self.chart.sections[i as usize].start
                    } else {
                        0.0
                    });
                }
                let a = self.loop_a.unwrap_or(0.0);
                if end > a + 0.2 {
                    self.loop_b = Some(end);
                    self.loop_passes = 0;
                    self.loop_acc = None;
                    self.say(format!("Looping {} – {}", fmt_time(a), fmt_time(end)), "");
                    self.seek(a - LOOP_LEAD_IN);
                }
            }
            "clear_loop" => {
                self.loop_a = None;
                self.loop_b = None;
                self.say("Loop cleared", "");
            }
            "loop_section" => {
                let i = self.section_here();
                if i >= 0 {
                    let sec = self.chart.sections[i as usize].clone();
                    self.loop_a = Some(sec.start);
                    self.loop_b = Some(sec.end);
                    self.loop_passes = 0;
                    self.loop_acc = None;
                    self.say("Looping section: ", section_label(&sec));
                }
            }
            "part" | "part_prev" => {
                let keys: Vec<String> = self.entry.arrangements.iter().map(|a| a.key.clone()).collect();
                if keys.len() < 2 {
                    self.say("This song has only one part", "");
                } else {
                    let step: i64 = if name == "part" { 1 } else { -1 };
                    let i = keys
                        .iter()
                        .position(|k| *k == self.part_key)
                        .map(|i| (i as i64 + step).rem_euclid(keys.len() as i64) as usize)
                        .unwrap_or(0);
                    let k = keys[i].clone();
                    self.set_part(st, &k);
                }
            }
            "noteview" => {
                let guitar = st.config.str("note_view", "piano") != "guitar";
                st.config
                    .set("note_view", if guitar { "guitar" } else { "piano" });
                self.say(
                    format!(
                        "Note tile: {}",
                        if guitar { "24-fret guitar neck" } else { "piano" }
                    ),
                    "",
                );
            }
            "easier" => self.change_mastery(st, -0.1),
            "harder" => self.change_mastery(st, 0.1),
            "auto_level" => {
                self.auto_level = !self.auto_level;
                st.config.set("auto_level", self.auto_level);
                self.say(
                    format!(
                        "Level up in loops: {}",
                        if self.auto_level { "on" } else { "off" }
                    ),
                    "",
                );
            }
            "lyrics_next" => {
                self.lyrics_next = !self.lyrics_next;
                st.config.set("lyrics_next", self.lyrics_next);
            }
            "lyrics" => {
                let on = !tiles::tile_on(st, "lyrics");
                tiles::tile_action(st, "tile:lyrics");
                let extra = if on && self.lyrics.is_empty() {
                    "  ·  this song has no lyrics"
                } else {
                    ""
                };
                self.say(
                    format!("Lyrics tile {}{extra}", if on { "on" } else { "off" }),
                    "",
                );
            }
            "slower" => self.change_speed(-1),
            "faster" => self.change_speed(1),
            "gtime-" | "gtime+" => {
                self.input_offset += if name == "gtime+" { 0.01 } else { -0.01 };
                st.config
                    .set("input_offset", (self.input_offset * 1000.0).round() / 1000.0);
                self.say(format!("Guitar timing {:+.0} ms", self.input_offset * 1000.0), "");
            }
            "sync-" | "sync+" => {
                self.offset += if name == "sync+" { 0.01 } else { -0.01 };
                st.config
                    .set("av_offset", (self.offset * 1000.0).round() / 1000.0);
                self.say(format!("Sync offset {:+.0} ms", self.offset * 1000.0), "");
            }
            "monitor" => {
                if let Some(e) = self.engine().cloned() {
                    let on = !e.monitor();
                    e.set_monitor(on);
                    st.config.set(st.monitor_key().0, on);
                    self.say(
                        if on {
                            "Hearing your guitar"
                        } else {
                            "Guitar sound off"
                        },
                        "",
                    );
                }
            }
            "tone" => {
                // the T key: As is -> Clean -> Crunch -> Lead -> Custom (stored as the Python app's tone_v2)
                let now = match self.engine() {
                    Some(e) => crate::tones::name_of(&e.tone()),
                    None => crate::tones::name_of(&crate::tones::current(&st.config)),
                };
                let next = crate::tones::next(now);
                let spec = crate::tones::spec_of(next, crate::tones::custom(&st.config));
                crate::tones::store(&mut st.config, &spec);
                if let Some(e) = self.engine() {
                    e.set_tone(spec);
                }
                self.say(format!("Tone: {}", crate::tones::label(next)), "");
            }
            "gvol-" | "gvol+" => {
                if let Some(e) = self.engine().cloned() {
                    let v = (e.guitar_gain() + if name == "gvol+" { 0.1 } else { -0.1 }).clamp(0.0, 2.0);
                    self.set_volume(st, "gvol", v);
                    self.say(format!("Guitar volume {}%", (v * 100.0).round() as i64), "");
                }
            }
            "vol-" | "vol+" => {
                let v = self.transport.volume() + if name == "vol+" { 0.1 } else { -0.1 };
                self.set_volume(st, "vol", v);
                self.say(
                    format!(
                        "Song volume {}%",
                        (self.transport.volume() * 100.0).round() as i64
                    ),
                    "",
                );
            }
            _ => {
                if let Some(rest) = name.strip_prefix("setvol:") {
                    let mut it = rest.splitn(2, ':');
                    if let (Some(kind), Some(v)) = (it.next(), it.next().and_then(|v| v.parse::<f32>().ok()))
                    {
                        self.set_volume(st, kind, v);
                    }
                } else if let Some(n) = name
                    .strip_prefix("diffsteps:")
                    .and_then(|v| v.parse::<i32>().ok())
                {
                    self.change_mastery(st, (0.1 * n as f64 * 100.0).round() / 100.0);
                } else if let Some(i) = name.strip_prefix("partn:").and_then(|s| s.parse::<usize>().ok()) {
                    let keys: Vec<String> = self.entry.arrangements.iter().map(|a| a.key.clone()).collect();
                    if i < keys.len() {
                        let k = keys[i].clone();
                        self.set_part(st, &k);
                    } else if keys.len() == 1 {
                        self.say("This song has only one part", "");
                    } else {
                        self.say(
                            format!("This song has {} parts: keys 1–{}", keys.len(), keys.len()),
                            "",
                        );
                    }
                } else if let Some(k) = name.strip_prefix("part:") {
                    let k = k.to_string();
                    self.set_part(st, &k);
                } else if name.starts_with("tile:")
                    || name.starts_with("tileup:")
                    || name.starts_with("tiledown:")
                {
                    tiles::tile_action(st, name);
                } else if let Some(k) = name.strip_prefix("group:") {
                    controls::toggle_group(st, Some(k), None);
                } else if let Some(k) = name.strip_prefix("nota:") {
                    let label = match k {
                        "show" => {
                            self.nota.show = !self.nota.show;
                            ("Rhythm notation", self.nota.show)
                        }
                        "snap" => {
                            self.nota.snap = !self.nota.snap;
                            ("Snap to beat grid", self.nota.snap)
                        }
                        "rests" => {
                            self.nota.rests = !self.nota.rests;
                            ("Rests", self.nota.rests)
                        }
                        "beams" => {
                            self.nota.beams = !self.nota.beams;
                            ("Beams & ties", self.nota.beams)
                        }
                        _ => {
                            self.nota.triplets = !self.nota.triplets;
                            ("Triplets", self.nota.triplets)
                        }
                    };
                    st.config.set("notation", self.nota.to_json());
                    if matches!(k, "rests" | "triplets" | "snap") {
                        let nota = self.nota;
                        self.chart.build_rhythm(&nota);
                    }
                    self.say(format!("{}: {}", label.0, if label.1 { "on" } else { "off" }), "");
                }
            }
        }
    }

    /// The player's keys (port of `key_action`).
    fn key_action(&self, key: Key, m: egui::Modifiers) -> Option<String> {
        let (ctrl, shift) = (m.command, m.shift);
        if matches!(key, Key::Comma | Key::Period) {
            let up = key == Key::Period;
            return Some(
                if ctrl {
                    if up { "gtime+" } else { "gtime-" }
                } else if up {
                    "sync+"
                } else {
                    "sync-"
                }
                .into(),
            );
        }
        if matches!(key, Key::OpenBracket | Key::CloseBracket) && ctrl {
            return Some("clear_loop".into());
        }
        if matches!(key, Key::Equals | Key::Plus) {
            return Some(if ctrl { "slower" } else { "faster" }.into());
        }
        if matches!(key, Key::ArrowUp | Key::ArrowDown) && ctrl {
            return Some(if key == Key::ArrowUp { "harder" } else { "easier" }.into());
        }
        if key == Key::A && shift {
            return Some("part_prev".into());
        }
        if let Some(d) = crate::library::digit(key)
            && (1..=6).contains(&d)
            && !ctrl
        {
            return Some(format!("partn:{}", d - 1));
        }
        if self.tab.sel.is_some() && matches!(key, Key::Escape | Key::P | Key::S) {
            return Some(
                match key {
                    Key::Escape => "sel_clear",
                    Key::P => "sel_play",
                    _ => "sel_slow",
                }
                .into(),
            );
        }
        let a = match key {
            Key::ArrowLeft => {
                if shift {
                    "back1"
                } else {
                    "back"
                }
            }
            Key::ArrowRight => {
                if shift {
                    "fwd1"
                } else {
                    "fwd"
                }
            }
            Key::Escape | Key::Backspace => "library",
            Key::Space => "play",
            Key::Minus => "rewind",
            Key::ArrowUp => "zoom_in",
            Key::ArrowDown => "zoom_out",
            Key::Home => "start",
            Key::PageDown => "next_section",
            Key::PageUp => "prev_section",
            Key::OpenBracket => "loop_start",
            Key::CloseBracket => "loop_end",
            Key::Backslash => "clear_loop",
            Key::L => "loop_section",
            Key::A => "part",
            Key::Y => "lyrics",
            Key::N => "nota:show",
            Key::M => "monitor",
            Key::T => "tone",
            Key::Num7 => "gvol-",
            Key::Num8 => "gvol+",
            Key::Num9 => "vol-",
            Key::Num0 => "vol+",
            _ => return None,
        };
        Some(a.into())
    }

    /// Whether the MG-300's big view is up (never without the NUX support).
    fn pedal_view_open(&self) -> bool {
        #[cfg(feature = "nux")]
        return self.mg_view.open;
        #[cfg(not(feature = "nux"))]
        false
    }

    /// Keys and the wheel.
    pub fn handle(&mut self, st: &mut AppState, ev: &Event) {
        #[cfg(feature = "nux")]
        if self.mg_view.open {
            // the big pedal view is modal: Esc closes it, the wheel plays it
            match ev {
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => self.mg_view.close(),
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    let notches = crate::library::wheel_notches(*unit, delta.y);
                    let pos = st.ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
                    let snap = st.pedal().state();
                    let pedal = st.pedal();
                    self.mg_view
                        .wheel(&snap, pedal, &mut self.mg_lcd, pos, notches, modifiers.shift);
                }
                _ => {}
            }
            return;
        }
        if self.sec_menu.is_some() && self.section_menu_event(ev) {
            return;
        }
        #[cfg(feature = "nux")]
        if let Event::MouseWheel {
            unit,
            delta,
            modifiers,
            ..
        } = ev
            && tiles::tile_on(st, "mg300")
            && !self.show_controls
        {
            let notches = crate::library::wheel_notches(*unit, delta.y);
            let pos = st.ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
            let snap = st.pedal().state();
            let pedal = st.pedal();
            if self
                .mg_mini
                .wheel(&snap, pedal, &mut self.mg_lcd, pos, notches, modifiers.shift)
            {
                return;
            }
        }
        match ev {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if let Some(name) = self.key_action(*key, *modifiers) {
                    self.act(st, &name);
                }
            }
            Event::MouseWheel { unit, delta, .. } => {
                let notches = crate::library::wheel_notches(*unit, delta.y);
                if notches == 0.0 {
                    return;
                }
                let pos = st.ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
                if let Some(&(_, kind)) = self.dials.iter().find(|(r, _)| r.contains(pos)) {
                    let up = notches > 0.0;
                    match kind {
                        "speed" | "speedtop" => self.change_speed(if up { 1 } else { -1 }),
                        "diff" => self.change_mastery(st, if up { 0.1 } else { -0.1 }),
                        "vol" => {
                            let v = self.transport.volume() + if up { 0.05 } else { -0.05 };
                            self.set_volume(st, "vol", v);
                        }
                        _ => {
                            if let Some(e) = self.engine() {
                                let v = e.guitar_gain() + if up { 0.1 } else { -0.1 };
                                self.set_volume(st, "gvol", v);
                            }
                        }
                    }
                } else if self.panel_rect.expand2(vec2(12.0, 0.0)).contains(pos) {
                    if self.show_controls {
                        self.ctrl_target = None;
                        self.ctrl_scroll = (self.ctrl_scroll - notches * 60.0).clamp(0.0, self.ctrl_max);
                    } else {
                        self.panel_scroll = (self.panel_scroll - notches * 60.0).clamp(0.0, self.panel_max);
                    }
                } else {
                    self.tab.wheel(notches);
                }
            }
            _ => {}
        }
    }

    /// The sections list while it's open: the wheel scrolls it, the arrows pick a row, Enter jumps there, Esc
    /// closes it. True if the event was the list's.
    fn section_menu_event(&mut self, ev: &Event) -> bool {
        let n = self.chart.sections.len();
        let Some(menu) = &mut self.sec_menu else {
            return false;
        };
        match ev {
            Event::MouseWheel { unit, delta, .. } => {
                let pos = self.sec_panel.zip(Some(())).map(|_| ()).and(None::<Pos2>);
                let _ = pos;
                let notches = crate::library::wheel_notches(*unit, delta.y);
                menu.scroll = Some(menu.scroll.unwrap_or(0.0) - notches * SEC_ROW);
                true
            }
            Event::Key {
                key, pressed: true, ..
            } => match key {
                Key::Escape => {
                    self.sec_menu = None;
                    true
                }
                Key::Enter => {
                    let k = menu.key;
                    self.go_to_section(k);
                    true
                }
                Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => {
                    let step: i64 = match key {
                        Key::ArrowUp => -1,
                        Key::ArrowDown => 1,
                        Key::Home => -(n as i64),
                        _ => n as i64,
                    };
                    menu.key = (menu.key as i64 + step).clamp(0, n as i64 - 1) as usize;
                    menu.show = true;
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    // -- every frame ------------------------------------------------------------------------------------------

    pub fn update(&mut self, st: &mut AppState) {
        if let Some(r) = self.countdown_hold {
            self.countdown_until = Some(Instant::now() + Duration::from_secs_f32(r));
        }
        if let Some(e) = self.engine().cloned() {
            self.synth.update(&e);
        }
        if !self.synth.error.is_empty() {
            let err = std::mem::take(&mut self.synth.error);
            self.say(format!("Couldn't play the selection: {err}"), "");
        }
        if let Some(sp) = self.pending_speed
            && self.speed_at.elapsed().as_secs_f32() > 0.45
        {
            self.apply_speed(sp);
            self.pending_speed = None;
            if self.started_playing() {
                self.run_scored = false;
            }
        }
        if self.stretch.as_ref().is_some_and(|j| j.done()) && self.engine().is_some_and(|e| !e.buffering()) {
            // keep the job (its buffer is the engine's) but stop showing progress
        }
        if self.countdown_until.is_some_and(|t0| Instant::now() >= t0) {
            self.countdown_until = None;
            self.transport.play();
        }
        let playing = self.transport.playing();
        if playing
            && let (Some(a), Some(b)) = (self.loop_a, self.loop_b)
            && self.now() >= b
        {
            self.loop_pass_done(st);
            self.seek((a - LOOP_LEAD_IN).max(0.0));
            self.countdown_until = None;
        }
        if playing && self.transport.finished() {
            self.transport.pause(); // pausing steps back to the heard position: pin it to the end
            let d = self.transport.duration();
            self.transport.seek(d);
        }
        if self.transport.playing() {
            self.recorded = false;
            self.new_best = false; // a new run: the last one's results card no longer applies
            self.result_rank = None;
        } else if self.transport.finished() && !self.recorded {
            self.recorded = true;
            self.record_completion(st);
        }
        // the guitar: plucks to the scorer, the pitch to the tuner
        let e = self.engine().cloned();
        if let Some(e) = e
            && e.status().input.is_some()
        {
            let wall = Instant::now();
            let onsets = e.take_onsets();
            if self.transport.playing() {
                let shift = self.offset - self.input_offset;
                self.scorer.feed(onsets.iter().filter_map(|o| {
                    o.song_time.map(|st| Pluck {
                        index: o.index,
                        song_time: st - shift,
                    })
                }));
                let meters = e.meters();
                let latency = e.status().input.as_ref().map(|i| i.latency).unwrap_or(0.0);
                let live = qrs_analysis::detect::Live {
                    source: e.capture(),
                    level: meters.level,
                    latency,
                };
                let now = self.now();
                self.scorer
                    .update(now, Some(&live), qrs_audio::clock::now_ns() as f64 / 1e9);
            }
            if wall.duration_since(self.tuner_at).as_secs_f32() > 0.05 {
                self.tuner_at = wall;
                self.read_pitch(&e);
            }
        } else if self.transport.playing() {
            let now = self.now();
            self.scorer
                .update(now, None, qrs_audio::clock::now_ns() as f64 / 1e9);
        }
        self.states = self.scorer.groups.iter().map(|g| g.state).collect();
    }

    /// Feed the tuner (and the note tile's "you:") from the guitar input.
    fn read_pitch(&mut self, e: &qrs_audio::Engine) {
        let level = e.meters().level;
        let rate = e.capture().rate();
        if level > 0.002 && e.capture().latest(&mut self.pitch_buf).is_some() {
            let (hz, clarity) = qrs_analysis::pitch::pitch(&mut self.sp, &self.pitch_buf, rate);
            if hz > 0.0 && clarity > 0.85 {
                self.tuner_hist.push_back(69.0 + 12.0 * (hz / 440.0).log2());
                while self.tuner_hist.len() > 5 {
                    self.tuner_hist.pop_front();
                }
                let mut v: Vec<f64> = self.tuner_hist.iter().copied().collect();
                v.sort_by(|a, b| a.total_cmp(b));
                self.tuner_midi = Some(v[v.len() / 2]); // the median: a steadier needle
                self.tuner_seen = Instant::now();
            }
        }
        if self.tuner_seen.elapsed().as_secs_f32() > 1.5 {
            self.tuner_midi = None;
            self.tuner_hist.clear();
        }
        self.tuner_text = match self.tuner_midi {
            Some(m) => format!(
                "{} {:+.0}¢",
                qrs_analysis::pitch::note_label(m),
                (m - m.round()) * 100.0
            ),
            None => String::new(),
        };
    }

    /// Riff Repeater-style: after a loop pass at 90%+ accuracy, add notes (raise difficulty 10%).
    fn loop_pass_done(&mut self, st: &AppState) {
        self.loop_passes += 1;
        let (Some(a), Some(b)) = (self.loop_a, self.loop_b) else {
            return;
        };
        let groups: Vec<&qrs_analysis::detect::NoteGroup> = self
            .scorer
            .groups
            .iter()
            .filter(|g| a <= g.t && g.t < b)
            .collect();
        if groups.is_empty() || !self.has_input() {
            return;
        }
        let (mut hit, mut le) = (0.0, 0.0);
        for g in &groups {
            match g.state {
                Some(Judgement::Hit) => hit += 1.0,
                Some(Judgement::Late) | Some(Judgement::Early) => le += 1.0,
                _ => {}
            }
        }
        let acc = (hit + 0.5 * le) / groups.len() as f64;
        self.loop_acc = Some(acc);
        if self.auto_level && self.mastery < 1.0 && acc >= LEVEL_UP_ACCURACY {
            let new = ((self.mastery + 0.1).min(1.0) * 100.0).round() / 100.0;
            let key = self.part_key.clone();
            if self.load_chart(st, &key, new) {
                self.say(
                    format!("Loop pass: {}%  ·  ", (acc * 100.0).round() as i64),
                    format!("level up → difficulty {}%", (new * 100.0).round() as i64),
                );
            }
        } else {
            self.say(format!("Loop pass: {}%", (acc * 100.0).round() as i64), "");
        }
    }

    /// Store the run in the user's scores: only a scored run (one part, start to end) with the guitar on.
    fn record_completion(&mut self, st: &mut AppState) {
        self.new_best = false;
        self.result_rank = None;
        self.result_scored = self.run_scored && self.at_max_settings();
        if !self.result_scored || !self.has_input() || !self.scorer.groups.iter().any(|g| g.state.is_some()) {
            return;
        }
        let total = self.scorer.groups.len().max(1) as f64;
        let c = self.scorer.counts();
        let accuracy = (c.hit as f64 + 0.5 * (c.late + c.early) as f64) / total; // notes never judged count as missed
        let speed = self.speed();
        self.result_rank = st.userdata.completed(
            &self.sid,
            &self.part_name,
            self.scorer.score() as i64,
            accuracy,
            speed,
        );
        self.new_best = self.result_rank == Some(0);
    }

    /// Screenshot mode: the moment tools/parity/py_shots.py holds the Python app's song at, staged the same way.
    /// From a clean player: the run restarted, the song playing and held at one minute (61 s), the notes between
    /// 50 and 60 s judged hit, hit, hit, late, miss by their time (the rest before 61 s are missed), and the two
    /// earlier runs the Python tool records (38,900 at 84%, 35,100 at 79%); then what the page adds.
    pub fn prepare_shot(&mut self, st: &mut AppState, page: Option<crate::shots::Page>) {
        use crate::shots::Page;
        const AT: f64 = 61.0;
        // what an earlier shot staged goes
        self.loop_a = None;
        self.loop_b = None;
        self.sel_slow = 1.0;
        self.sel_track = false;
        self.tab.zoom = 1.0;
        self.zoom_at = None;
        self.sec_menu = None;
        self.pending_speed = None;
        self.shot_stretch = None;
        self.panel_scroll = 0.0;
        self.toast = None;
        self.countdown_hold = None;
        if self.audio_error == "the song's audio couldn't be decoded" {
            self.audio_error.clear();
        }
        self.countdown_until = None;
        self.show_controls = matches!(page, Some(Page::PlayerControls | Page::PlayerAt("controls_more")));
        #[cfg(feature = "nux")]
        {
            self.mg_view.open = page == Some(Page::Mg300View);
        }
        self.tab.sel = None;
        self.tab.follow = true;
        self.tab.jump = true;
        // the song
        self.transport.hold(None);
        self.seek(0.0); // a fresh run
        if page == Some(Page::PlayerAt("countin")) {
            // just opened: at the start, counting in (the Python shot is taken 0.05 s into a 2.5 s count-in)
            self.transport.pause();
            self.transport.hold(Some(0.0));
            self.countdown_hold = Some(2.45);
            return;
        }
        self.transport.play();
        self.transport.seek(60.0);
        self.transport.hold(Some(AT));
        for g in &mut self.scorer.groups {
            if 50.0 < g.t && g.t < 60.0 {
                g.state = Some(match (g.t * 7.0) as i64 % 5 {
                    3 => Judgement::Late,
                    4 => Judgement::Miss,
                    _ => Judgement::Hit,
                });
            }
        }
        if !self.shot_runs {
            self.shot_runs = true;
            st.userdata
                .completed(&self.sid, &self.part_name, 38900, 0.84, 1.0);
            st.userdata
                .completed(&self.sid, &self.part_name, 35100, 0.79, 1.0);
        }
        self.update(st); // one frame at 61 s: the notes before it that weren't judged are missed
        let now = AT - self.offset;
        let bar_after = |p: &Self, dt: f64| p.tab.measure_at(now + dt).map(|m| (m.start, m.end));
        match page {
            Some(Page::PlayerSelection) | Some(Page::PlayerAt("sel_slow")) => {
                self.loop_a = Some(now - 4.0);
                self.loop_b = Some(now + 8.0);
                self.tab.sel = bar_after(self, 2.0);
                if page == Some(Page::PlayerAt("sel_slow")) {
                    self.sel_slow = 0.7;
                }
            }
            Some(Page::PlayerAt(what)) => match what {
                "tiles_scrolled" => self.panel_scroll = qrs_ui::units::pixels(200.0), // (Python: 200 px)
                "zoomed_out" => self.tab.zoom = 0.5,
                "sections" => self.acts.push("sections".into()),
                "sel_track" => {
                    self.tab.sel = bar_after(self, 2.0);
                    self.sel_track = true;
                }
                "scrolled_lock" => self.tab.nudge = Some(-qrs_ui::units::pixels(600.0)), // (600 px)
                // zoomed out, the tab settled on its new scroll (the Python tool waits for its glide)
                "zoom" => {
                    self.tab.zoom = (self.tab.zoom / 1.15).max(0.5);
                    self.zoom_at = Some(Instant::now());
                    self.tab.jump = true;
                }
                "paused" => self.transport.pause(),
                "toast" => {
                    self.transport.pause();
                    self.say_for(
                        "Loop pass: 92%",
                        "  \u{b7}  level up \u{2192} difficulty 90%",
                        30.0,
                    );
                }
                "slowing" => self.shot_stretch = Some(0.42),
                "results_practice" | "no_audio" => self.shot_song_end(st, what),
                _ => {}
            },
            Some(Page::Results) => self.shot_song_end(st, "results"),
            Some(Page::Settings(_)) => self.transport.pause(), // opening the settings pauses, as in the Python app
            _ => {}
        }
    }

    /// Screenshot mode: the song over, as the Python tool ends it (paused at the end, the run recorded once), for
    /// the results card ("results"), its practice-run form ("results_practice") or the no-audio card after it, back
    /// at 30 s ("no_audio").
    fn shot_song_end(&mut self, st: &mut AppState, what: &str) {
        self.transport.pause();
        let d = self.transport.duration();
        self.transport.seek(d);
        match self.shot_result {
            Some((rank, best)) => {
                // recorded by an earlier shot: the same results again
                self.recorded = true;
                self.result_scored = true;
                self.result_rank = rank;
                self.new_best = best;
            }
            None => {
                self.run_scored = true;
                self.recorded = false;
                self.update(st); // the song has ended: the run is recorded
                self.shot_result = Some((self.result_rank, self.new_best));
            }
        }
        match what {
            "results_practice" => {
                self.result_scored = false;
                self.new_best = false;
            }
            "no_audio" => {
                self.audio_error = "the song's audio couldn't be decoded".into();
                self.transport.seek(30.0);
            }
            _ => {}
        }
    }

    /// Screenshot mode: where the pointer is for a shot that shows something under it.
    pub fn shot_pointer(&self, page: crate::shots::Page) -> Option<Pos2> {
        use crate::shots::Page;
        match page {
            Page::PlayerAt("tile_hover") => self.tile_rects.get("note").map(|r| r.center()),
            Page::PlayerAt("back_tip") => (self.back_rect != Rect::NOTHING).then(|| self.back_rect.center()),
            _ => None,
        }
    }

    pub fn close(&mut self) {
        let e = self.engine().cloned();
        self.synth.stop_maybe(e.as_deref());
        if let Some(job) = self.stretch.take() {
            job.cancel();
        }
        if let Some(e) = e {
            e.pause();
            e.set_speed(1.0, None);
            e.unload();
        }
    }

    // -- drawing ----------------------------------------------------------------------------------------------

    /// The main column (header, tab, transport) and the console on the right (the tiles or the controls).
    fn layout(area: Rect) -> (Rect, Rect) {
        let (w, h) = (snap(area.width()), snap(area.height()));
        let pad = px(28.0);
        let aside_w = pixels(to_px(330.0).max(to_px(380.0).min(ipx(w) * 0.26)).trunc() as f32);
        let aside = Rect::from_min_size(
            ipos(area.min.x + w - pad - aside_w, area.min.y + pad),
            vec2(aside_w, snap(h - 2.0 * pad)),
        );
        let main = Rect::from_min_size(
            ipos(area.min.x + pad, area.min.y + pad),
            vec2(snap(aside.min.x - area.min.x - px(26.0) - pad), aside.height()),
        );
        (main, aside)
    }

    fn hit(&mut self, ui: &Ui, rect: Rect, action: &str) -> egui::Response {
        let resp = interact(
            ui,
            rect,
            (
                "player-hit",
                action.to_string(),
                (rect.min.x as i32, rect.min.y as i32),
            ),
            Sense::click(),
        );
        if resp.clicked() && !action.is_empty() {
            self.acts.push(action.to_string());
        }
        resp
    }

    pub fn ui(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        self.frame(ui, st, area, true);
    }

    /// Drawn under a panel (settings): no keys or wheel; the panel blocks the clicks.
    pub fn ui_behind(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        self.frame(ui, st, area, false);
    }

    fn frame(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect, input: bool) {
        let ctx = ui.ctx().clone();
        if input {
            let events = ctx.input(|i| i.events.clone());
            for ev in &events {
                self.handle(st, ev);
            }
        }
        self.update(st);
        ctx.request_repaint(); // the tab moves with the song
        if self.summary_up() {
            if self.backdrop.ready(&ctx) {
                // the song has ended, so what's under the results card doesn't change
                self.backdrop.draw(ui.painter(), area);
                self.draw_summary(ui, st, area);
                let acts = std::mem::take(&mut self.acts);
                for a in acts {
                    self.act(st, &a);
                }
                return;
            }
        } else {
            self.backdrop.clear();
        }
        self.dials.clear();
        self.tip = None;
        let now = self.now();
        let (main, aside) = Self::layout(area);
        let top = self.draw_header(ui, st, main);
        let foot = px(76.0);
        let well = Rect::from_min_size(
            ipos(main.min.x, top + px(18.0)),
            vec2(
                snap(main.width() - px(24.0)),
                snap(main.max.y - foot - px(18.0) - top - px(18.0)),
            ),
        );
        let groove = Rect::from_min_size(
            ipos(well.max.x + px(14.0), well.min.y),
            vec2(px(10.0), well.height()),
        );
        self.well = well;
        let synth_t = self.engine().and_then(|e| self.synth.position(e));
        let tctx = TabCtx {
            chart: &self.chart,
            states: &self.states,
            now,
            loop_ab: self.loop_a.zip(self.loop_b),
            synth_t,
            synth_state: self.synth.state,
            playing: self.transport.playing(),
            notation: self.nota.show,
            snap: self.nota.snap,
            beams: self.nota.beams,
            sel_loop: self.sel_loop,
            sel_slow: self.sel_slow,
            sel_track: self.sel_track,
            track_ok: self.track_ok(),
            selection_speed: self.selection_speed(),
        };
        let tab_acts = self.tab.ui(ui, &tctx, well, groove);
        for a in tab_acts {
            match a {
                TabAct::Seek(tm) => {
                    let tm = tm + self.offset;
                    self.seek(tm);
                }
                TabAct::Selected => self.stop_synth(),
                TabAct::Act(name) => self.acts.push(name.into()),
            }
        }
        self.draw_zoom_hud(ui, well);
        self.draw_footer(
            ui,
            st,
            Rect::from_min_size(ipos(main.min.x, main.max.y - foot), vec2(main.width(), foot)),
            now,
        );
        self.draw_aside(ui, st, aside, now);
        self.draw_states(ui, well);
        if self.sec_menu.is_some() {
            if self.pedal_view_open() || self.summary_up() {
                self.sec_menu = None; // (it closes under the pedal's view and the results, as in the Python app)
            } else {
                self.draw_section_menu(ui);
            }
        }
        if let Some((text, key, pos, anchor)) = self.tip.clone() {
            self.draw_tip(ui.painter(), &text, &key, pos, anchor, area);
        }
        #[cfg(feature = "nux")]
        if self.mg_view.open {
            let snap = st.pedal().state();
            let pedal = st.pedal();
            self.mg_view.ui(ui, area, &snap, pedal, &mut self.mg_lcd);
        }
        let acts = std::mem::take(&mut self.acts);
        for a in acts {
            self.act(st, &a);
        }
    }

    /// A key back to the library, the album cover with the song's title, artist, album and year beside it, then
    /// the part and tuning; at the right the speed knob and the part keys, with the guitar's status strip under
    /// them. Returns the header's bottom.
    fn draw_header(&mut self, ui: &Ui, st: &mut AppState, main: Rect) -> f32 {
        let p = ui.painter().clone();
        let th = t();
        let ctx = st.ctx.clone();
        let labels = self.part_labels();
        let lrefs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let keys: Vec<String> = self.entry.arrangements.iter().map(|a| a.key.clone()).collect();
        let psel = keys.iter().position(|k| *k == self.part_key).unwrap_or(0);
        // neu.segment_widths(f, labels, 15): each label, 2 * px(17) and px(15)
        let widths: Vec<f32> = lrefs
            .iter()
            .map(|l| text::width(&ctx, l, Style::new(15.0, 800, th.text)) + 2.0 * pxr(17.0) + pxr(15.0))
            .collect();
        let pw =
            snap(widths.iter().sum::<f32>() + px(4.0) * (widths.len().max(1) - 1) as f32 + 2.0 * px(5.0));
        let pick = Rect::from_min_size(ipos(main.max.x - pw, main.min.y), vec2(pw, px(52.0)));
        let mut seg = Segmented::new(&lrefs, Some(psel));
        seg.widths = Some(widths);
        seg.pad = pxr(5.0);
        seg.gap = pxr(4.0);
        let (_, hit) = widgets::segmented(ui, pick, "partpill", &seg);
        if let Some(i) = hit
            && keys.len() > 1
        {
            self.acts.push(format!("part:{}", keys[i]));
        }
        let slowed = (self.speed() - 1.0).abs() > 1e-6;
        let sp = self.draw_knob_unit(
            ui,
            "speedtop",
            "SPEED",
            &format!("{}%", (self.speed() * 100.0).round() as i64),
            pos2(snap(pick.min.x - px(20.0)), mid_y(pick)),
            slowed,
            true,
        );
        let status = self.draw_status(
            ui,
            st,
            main.max.x,
            snap(pick.max.y + px(10.0)),
            snap(main.width() * 0.55),
        );
        // back to the library, the cover, and the song beside it
        let b = px(44.0);
        let back = Rect::from_min_size(ipos(main.min.x, main.min.y + b - snap(b / 2.0)), Vec2::splat(b));
        self.back_rect = back;
        let br = widgets::round_button(
            ui,
            back,
            "player-back",
            Icon::ArrowLeft,
            18.0,
            RoundKind::Ivory,
            false,
        );
        if br.hovered() {
            self.tip = Some((
                "Back to the library".into(),
                "Esc".into(),
                ipos(back.min.x, back.max.y + px(10.0)),
                Align2::LEFT_TOP,
            ));
        }
        if br.clicked() {
            self.acts.push("library".into());
        }
        let cover = Rect::from_min_size(ipos(back.max.x + px(16.0), main.min.y), Vec2::splat(px(88.0)));
        let size = ipx(cover.width()) - ipx(px(8.0));
        if !crate::covers::draw(&p, &st.covers, &self.entry, size, cover, 16.0, true) {
            icons::icon(&p, Icon::Note, mid(cover), 30.0, th.caption);
        }
        let tx = snap(cover.max.x + px(18.0));
        let tw = snap(sp.min.x.min(status.min.x) - px(24.0) - tx);
        let tr = txt(
            &p,
            &self.entry.display_title(),
            ipos(tx, main.min.y - px(4.0)),
            30.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(tw),
        );
        let year = if self.entry.year != 0 {
            self.entry.year.to_string()
        } else {
            String::new()
        };
        let about: Vec<String> = [self.entry.artist.clone(), self.entry.album.clone(), year]
            .into_iter()
            .filter(|v| !v.is_empty())
            .collect();
        let ar = txt(
            &p,
            &about.join("  ·  "),
            ipos(tx, tr.max.y),
            15.0,
            th.text_2,
            600,
            Align2::LEFT_TOP,
            Some(tw),
        );
        let y = snap(ar.max.y + px(4.0));
        let mut x = snap(
            txt(
                &p,
                &self.part_name,
                pos2(tx, y),
                14.0,
                th.text,
                800,
                Align2::LEFT_TOP,
                Some(tw),
            )
            .max
            .x,
        );
        let capo = if self.chart.arr.capo != 0 {
            format!("  ·  Capo {}", self.chart.arr.capo)
        } else {
            String::new()
        };
        let more = format!("  ·  {}{capo}", self.chart.arr.tuning_name());
        x = snap(
            txt(
                &p,
                &more,
                pos2(x, y),
                14.0,
                th.text_2,
                600,
                Align2::LEFT_TOP,
                Some(snap(tx + tw - x).max(pixels(10.0))),
            )
            .max
            .x,
        );
        if ipx(tx + tw - x) > ipx(px(60.0)) {
            let names = format!("   {}", self.chart.string_names.join(" "));
            x = snap(
                txt(
                    &p,
                    &names,
                    pos2(x, y),
                    14.0,
                    th.caption,
                    500,
                    Align2::LEFT_TOP,
                    Some(snap(tx + tw - x)),
                )
                .max
                .x,
            );
        }
        if self.mastery < 0.999 && ipx(tx + tw - x) > ipx(px(80.0)) {
            txt(
                &p,
                &format!("  ·  difficulty {}%", (self.mastery * 100.0).round() as i64),
                pos2(x, y),
                14.0,
                th.accent_ink,
                700,
                Align2::LEFT_TOP,
                Some(snap(tx + tw - x)),
            );
        }
        snap(
            status
                .max
                .y
                .max(cover.max.y)
                .max(y + text::line_height(&ctx, Style::new(14.0, 500, th.text))),
        )
    }

    /// A knob's value, 0..1 round its scale (while the difficulty knob is dragged, where it will go).
    fn dial_value(&self, kind: &str) -> f32 {
        match kind {
            "speed" | "speedtop" => {
                ((self.speed() - SPEEDS[0]) / (SPEEDS[SPEEDS.len() - 1] - SPEEDS[0])) as f32
            }
            "vol" => self.transport.volume(),
            "gvol" => self.engine().map(|e| e.guitar_gain() / 2.0).unwrap_or(0.0),
            _ => match &self.dial_drag {
                Some(d) if d.kind == "diff" => (self.mastery + 0.1 * d.steps as f64).clamp(0.0, 1.0) as f32,
                _ => self.mastery as f32,
            },
        }
    }

    fn dial_home(kind: &str) -> Option<f32> {
        match kind {
            "speed" | "speedtop" => Some(((1.0 - SPEEDS[0]) / (SPEEDS[SPEEDS.len() - 1] - SPEEDS[0])) as f32),
            "gvol" => Some(0.5),
            "diff" => Some(1.0),
            _ => None,
        }
    }

    /// Drag a knob up or down (speed and volume change as you drag; difficulty when you let go; a click on the
    /// top speed knob opens the practice controls).
    fn dial_interact(&mut self, ui: &Ui, rect: Rect, kind: &'static str) -> bool {
        let resp = interact(ui, rect, ("dial", kind), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }
        self.dials.push((rect, kind));
        if resp.drag_started()
            || (resp.is_pointer_button_down_on() && self.dial_drag.as_ref().is_none_or(|d| d.kind != kind))
        {
            let y = resp
                .interact_pointer_pos()
                .map(|p| p.y)
                .unwrap_or(rect.center().y);
            let v0 = match kind {
                "vol" => self.transport.volume(),
                "gvol" => self.engine().map(|e| e.guitar_gain()).unwrap_or(0.0),
                _ => 0.0,
            };
            self.dial_drag = Some(DialDrag {
                kind,
                y0: y,
                steps: 0,
                v0,
                moved: false,
            });
        }
        if let Some(d) = self.dial_drag.clone()
            && d.kind == kind
        {
            if let Some(pos) = resp.interact_pointer_pos() {
                let dy = d.y0 - pos.y;
                let moved = d.moved || dy.abs() > 3.0;
                if kind == "vol" || kind == "gvol" {
                    let full = if kind == "vol" { 1.0 } else { 2.0 };
                    self.acts
                        .push(format!("setvol:{kind}:{}", d.v0 + dy / DIAL_TRAVEL * full));
                    if let Some(dd) = &mut self.dial_drag {
                        dd.moved = moved;
                    }
                } else {
                    let mut steps = (dy / DIAL_STEP) as i32;
                    if (kind == "speed" || kind == "speedtop") && steps != d.steps {
                        self.change_speed(steps - d.steps);
                    }
                    if kind == "diff" {
                        let lo = -(self.mastery * 10.0).round() as i32;
                        let hi = ((1.0 - self.mastery) * 10.0).round() as i32;
                        steps = steps.clamp(lo, hi);
                    }
                    if let Some(dd) = &mut self.dial_drag {
                        dd.steps = steps;
                        dd.moved = moved;
                    }
                }
            }
            if !resp.is_pointer_button_down_on() && !resp.dragged() {
                let d = self.dial_drag.take().unwrap_or(d);
                if kind == "diff" && d.steps != 0 {
                    self.acts.push(format!("diffsteps:{}", d.steps));
                } else if kind == "speedtop" && !d.moved && resp.clicked() {
                    self.acts.push("practice".into());
                }
            }
            return true;
        }
        resp.hovered()
    }

    /// A small knob with its scale, its name and value beside it, ending at `at` (its right middle): the speed at
    /// the top, the guitar and song volumes at the bottom. Returns its rect.
    fn draw_knob_unit(
        &mut self,
        ui: &Ui,
        kind: &'static str,
        label: &str,
        value: &str,
        at: Pos2,
        accent: bool,
        enabled: bool,
    ) -> Rect {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let bx = px(56.0);
        let lab_w = text::width(&ctx, label, Style::caps(11.0, th.caption)).max(text::width(
            &ctx,
            "200%",
            Style::new(18.0, 800, th.text),
        ));
        let w = snap(bx + px(10.0) + lab_w);
        // r.midright = (int(at[0]), int(at[1]))
        let r = Rect::from_min_size(ipos(snap(at.x) - w, snap(at.y) - snap(bx / 2.0)), vec2(w, bx));
        let cy = mid_y(r);
        let base = if kind == "speedtop" { "speed" } else { kind };
        // the volumes: chicken-head knobs
        let style = if base == "vol" || base == "gvol" {
            Some(KnobStyle::Vintage)
        } else {
            None
        };
        let c = ipos(r.min.x + snap(bx / 2.0), cy);
        if enabled {
            let hot = self.dial_interact(ui, r, kind);
            let _ = hot;
            knobs::knob(
                &p,
                c,
                34.0,
                self.dial_value(base),
                Scale {
                    home: Self::dial_home(base),
                    ..Default::default()
                },
                style,
            );
        } else {
            knobs::knob(&p, c, 34.0, 0.0, Scale::default(), style);
        }
        let tx = snap(r.min.x + bx + px(10.0));
        caps(&p, label, ipos(tx, cy - px(1.0)), 11.0, Align2::LEFT_BOTTOM, None);
        let col = if accent {
            th.accent_ink
        } else if enabled {
            th.text
        } else {
            th.caption
        };
        txt(
            &p,
            value,
            ipos(tx, cy - px(2.0)),
            18.0,
            col,
            800,
            Align2::LEFT_TOP,
            None,
        );
        r
    }

    /// The guitar input in a sunken strip ending at `right`: the pick (click it: dark or light), a light, the
    /// device, its level as LEDs and what you hear. Returns the strip.
    fn draw_status(&mut self, ui: &Ui, st: &AppState, right: f32, y: f32, max_w: f32) -> Rect {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        enum It {
            Text(String, Color32, u16),
            Key(&'static str),
            Meter,
            You,
        }
        let mut items: Vec<It> = Vec::new();
        let led;
        match self.engine() {
            None => {
                led = th.miss;
                items.push(It::Text("Audio engine off".into(), th.text_2, 700));
                items.push(It::Key("F2"));
            }
            Some(e) => {
                let s = e.status();
                match &s.input {
                    None => {
                        led = th.miss;
                        let why = s.input_error.clone().unwrap_or_else(|| "not connected".into());
                        items.push(It::Text(format!("Guitar: {why}"), th.text_2, 700));
                        items.push(It::Key("F2"));
                    }
                    Some(inp) => {
                        led = th.hit;
                        items.push(It::Text(inp.device_name.clone(), th.text, 700));
                        items.push(It::Meter);
                        if self.show_controls || !tiles::tile_on(st, "note") {
                            items.push(It::You);
                        }
                        if e.monitor() {
                            items.push(It::Text(
                                format!(
                                    "hearing guitar  ·  {}  ·  {}%",
                                    tone_label(&e.tone()).to_lowercase(),
                                    (e.guitar_gain() * 100.0).round() as i64
                                ),
                                th.caption,
                                500,
                            ));
                        } else {
                            items.push(It::Text("guitar sound off".into(), th.caption, 500));
                            items.push(It::Key("M"));
                        }
                    }
                }
            }
        }
        let (gap, h) = (px(12.0), px(34.0));
        let widths: Vec<f32> = items
            .iter()
            .map(|it| match it {
                It::Text(s, _, w) => text::width(&ctx, s, Style::new(13.0, *w, th.text)),
                It::Key(k) => cap_w(&ctx, k),
                It::Meter => px(70.0),
                It::You => text::width(&ctx, "you: C#3 −50¢", Style::new(13.0, 700, th.text)),
            })
            .collect();
        let pick_d = px(22.0);
        let inner = pick_d
            + px(12.0)
            + px(8.0)
            + gap
            + widths.iter().sum::<f32>()
            + gap * (widths.len().max(1) - 1) as f32;
        let w = snap(max_w.min(inner + px(26.0)));
        let strip = Rect::from_min_size(ipos(right - w, y), vec2(w, h));
        paint::inset(&p, strip, Some(12.0), 2.0, Fill::Base);
        let cy = mid_y(strip);
        let pick = Rect::from_min_size(
            ipos(strip.min.x + px(10.0), cy - snap(pick_d / 2.0)),
            Vec2::splat(pick_d),
        );
        match crate::picks::pick(
            &ctx,
            if theme::is_dark() {
                "pick-gold"
            } else {
                "pick-graphite"
            },
        ) {
            Some(tex) => paint::image(&p, tex, pick, Color32::WHITE),
            None => {
                icons::icon(&p, Icon::Music, mid(pick), 16.0, th.accent_ink);
            }
        }
        let presp = self.hit(ui, inflate(pick, px(8.0), px(8.0)), "theme");
        if presp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            self.tip = Some((
                if theme::is_dark() {
                    "Light mode"
                } else {
                    "Dark mode"
                }
                .into(),
                String::new(),
                ipos(pick.min.x, strip.max.y + px(10.0)),
                Align2::LEFT_TOP,
            ));
        }
        let mut cx = snap(pick.max.x + px(12.0));
        paint::led(&p, ipos(cx + px(4.0), cy), 8.0, led);
        cx = snap(cx + px(8.0) + gap);
        for (it, wd) in items.iter().zip(&widths) {
            let room = snap(strip.max.x - px(14.0) - cx);
            if ipx(room) < ipx(px(20.0)) {
                break;
            }
            match it {
                It::Text(s, col, w) => {
                    txt(
                        &p,
                        s,
                        pos2(cx, cy),
                        13.0,
                        *col,
                        *w,
                        Align2::LEFT_CENTER,
                        Some(room),
                    );
                }
                It::Key(k) => {
                    keycap(&p, k, pos2(cx, cy), Align2::LEFT_CENTER, 11.0, CapOn::Material);
                }
                It::Meter => {
                    let peak = self.engine().map(|e| e.meters().peak).unwrap_or(0.0);
                    knobs::led_meter(
                        &p,
                        peak,
                        Rect::from_min_size(ipos(cx, cy - px(6.0)), vec2(wd.min(room), px(12.0))),
                        12,
                    );
                }
                It::You => {
                    if self.tuner_text.is_empty() {
                        txt(
                            &p,
                            "you: –",
                            pos2(cx, cy),
                            13.0,
                            th.caption,
                            700,
                            Align2::LEFT_CENTER,
                            None,
                        );
                    } else {
                        txt(
                            &p,
                            &format!("you: {}", self.tuner_text),
                            pos2(cx, cy),
                            13.0,
                            th.text,
                            700,
                            Align2::LEFT_CENTER,
                            None,
                        );
                    }
                }
            }
            cx = snap(cx + wd + gap);
        }
        strip
    }

    /// For a moment after the tab's zoom changes: how far in it is, floating at the tab's top right.
    fn draw_zoom_hud(&self, ui: &Ui, well: Rect) {
        let Some(at) = self.zoom_at else { return };
        let age = at.elapsed().as_secs_f32();
        if age > ZOOM_HUD {
            return;
        }
        let p = ui.painter().clone();
        let th = t();
        let z = self.tab.zoom;
        // panel.topright = (well.right - int(18 * s), well.y + int(18 * s))
        let (pw, ph) = (px(196.0), px(44.0));
        let panel = Rect::from_min_size(
            ipos(well.max.x - px(18.0) - pw, well.min.y + px(18.0)),
            vec2(pw, ph),
        );
        let cy = mid_y(panel);
        let fade = ((ZOOM_HUD - age) / 0.3).min(1.0);
        let k = |c: Color32| c.gamma_multiply(fade);
        if fade >= 0.999 {
            paint::floating(&p, panel, Some(16.0));
        } else {
            p.rect_filled(panel, 16.0, k(th.base));
        }
        icons::icon(
            &p,
            Icon::Search,
            ipos(panel.min.x + px(22.0), cy),
            16.0,
            k(th.text_2),
        );
        let tr = txt(
            &p,
            &format!("{}%", (z * 100.0).round() as i64),
            ipos(panel.min.x + px(40.0), cy),
            16.0,
            k(th.text),
            800,
            Align2::LEFT_CENTER,
            None,
        );
        let x0 = snap(tr.max.x + px(14.0));
        let track = Rect::from_min_size(
            ipos(x0, cy - px(3.0)),
            vec2(snap(panel.max.x - px(18.0) - x0), px(6.0)),
        );
        paint::inset(&p, track, None, 2.0, Fill::Base);
        let u = ((z.ln() - 0.5f32.ln()) / (2.5f32.ln() - 0.5f32.ln())).clamp(0.0, 1.0);
        paint::pill(
            &p,
            Rect::from_min_size(
                track.min,
                vec2(snap(track.width() * u).max(track.height()), track.height()),
            ),
            k(th.accent),
        );
        ui.ctx().request_repaint();
    }

    /// A tooltip: the text (and a key cap) on a small raised plate, drawn over everything.
    fn draw_tip(&self, p: &Painter, label: &str, key: &str, pos: Pos2, anchor: Align2, area: Rect) {
        let th = t();
        let ctx = p.ctx();
        let st = Style::new(13.0, 700, th.text);
        let lw = text::width(ctx, label, st);
        let kw = if key.is_empty() {
            0.0
        } else {
            cap_w(ctx, key) + px(8.0)
        };
        let (w, h) = (snap(lw + kw + px(28.0)), px(34.0));
        // setattr(plate, anchor, (int(pos[0]), int(pos[1]))), then clamped into the window
        let (x, y) = (snap(pos.x), snap(pos.y));
        let x = match anchor.x() {
            egui::Align::Min => x,
            egui::Align::Center => x - snap(w / 2.0),
            egui::Align::Max => x - w,
        };
        let y = match anchor.y() {
            egui::Align::Min => y,
            egui::Align::Center => y - snap(h / 2.0),
            egui::Align::Max => y - h,
        };
        let mut plate = Rect::from_min_size(ipos(x, y), vec2(w, h));
        let dx = (area.min.x - plate.min.x).max(0.0) + (area.max.x - plate.max.x).min(0.0);
        let dy = (area.min.y - plate.min.y).max(0.0) + (area.max.y - plate.max.y).min(0.0);
        plate = Rect::from_min_size(ipos(plate.min.x + dx, plate.min.y + dy), plate.size());
        paint::raised(p, plate, Some(14.0), 4.0, Fill::Base, 0.0);
        let cy = mid_y(plate);
        let tr = text::draw(
            p,
            label,
            ipos(plate.min.x + px(14.0), cy),
            Align2::LEFT_CENTER,
            st,
            None,
        );
        if !key.is_empty() {
            keycap(
                p,
                key,
                ipos(tr.max.x + px(8.0), cy),
                Align2::LEFT_CENTER,
                11.0,
                CapOn::Material,
            );
        }
    }

    /// The transport (back, play or pause, forward) as round keys in a sunken cradle; the section, bar and time
    /// over the seek groove; the guitar's and the song's volume as knobs.
    fn draw_footer(&mut self, ui: &Ui, st: &mut AppState, r: Rect, now: f64) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let cy = mid_y(r);
        let playing = self.transport.playing() || self.countdown_until.is_some();
        let (pad, gap) = (px(6.0), px(12.0));
        let (small, big) = (px(48.0), px(64.0));
        let cradle = Rect::from_min_size(
            ipos(r.min.x, cy - snap(big / 2.0) - pad),
            vec2(
                snap(2.0 * pad + 2.0 * small + big + 2.0 * gap),
                snap(big + 2.0 * pad),
            ),
        );
        paint::inset(&p, cradle, None, 6.0, Fill::Base);
        let back = Rect::from_min_size(
            ipos(cradle.min.x + pad, cy - snap(small / 2.0)),
            Vec2::splat(small),
        );
        let play = Rect::from_min_size(ipos(back.max.x + gap, cy - snap(big / 2.0)), Vec2::splat(big));
        let fwd = Rect::from_min_size(ipos(play.max.x + gap, back.min.y), Vec2::splat(small));
        if widgets::round_button(ui, back, "t-back", Icon::Back, 20.0, RoundKind::Ivory, false).clicked() {
            self.acts.push("back".into());
        }
        // what a press does: red pauses, green plays
        let gem = if playing { Gem::Ruby } else { Gem::Emerald };
        if widgets::round_button(
            ui,
            play,
            "t-play",
            if playing { Icon::Pause } else { Icon::Play },
            26.0,
            RoundKind::Gem(gem),
            false,
        )
        .clicked()
        {
            self.acts.push("play".into());
        }
        if widgets::round_button(ui, fwd, "t-fwd", Icon::Fwd, 20.0, RoundKind::Ivory, false).clicked() {
            self.acts.push("fwd".into());
        }
        // the volumes: your guitar, then the song
        let song = self.draw_knob_unit(
            ui,
            "vol",
            "SONG",
            &format!("{}%", (self.transport.volume() * 100.0).round() as i64),
            pos2(r.max.x, cy),
            false,
            true,
        );
        let gtxt = self
            .engine()
            .map(|e| format!("{}%", (e.guitar_gain() * 100.0).round() as i64))
            .unwrap_or_else(|| "–".into());
        let has_engine = self.engine().is_some();
        let gtr = self.draw_knob_unit(
            ui,
            "gvol",
            "GUITAR",
            &gtxt,
            ipos(song.min.x - px(16.0), cy),
            false,
            has_engine,
        );
        // the seek groove, with the line over it
        let (x0, x1) = (snap(cradle.max.x + px(24.0)), snap(gtr.min.x - px(24.0)));
        let lab_h = text::line_height(&ctx, Style::new(13.0, 800, th.text));
        let top = snap(cy - snap((lab_h + px(10.0) + px(14.0)) / 2.0));
        let bar = Rect::from_min_size(
            ipos(x0, top + lab_h + px(10.0)),
            vec2(snap(x1 - x0).max(pixels(10.0)), px(14.0)),
        );
        self.bar = bar;
        let len = self.chart.length.max(1.0);
        let bx = |tm: f64| bar.min.x + ((tm / len).clamp(0.0, 1.0) as f32) * bar.width();
        let resp = interact(
            ui,
            inflate(bar, px(16.0), px(20.0)),
            "seekbar",
            Sense::click_and_drag(),
        );
        if let Some(pos) = resp.interact_pointer_pos()
            && resp.is_pointer_button_down_on()
        {
            self.seek_drag =
                Some((((pos.x - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0) as f64) * len);
        } else if let Some(tm) = self.seek_drag.take() {
            self.seek(tm); // seeks when the button comes up
        }
        if resp.hovered() || self.seek_drag.is_some() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        paint::inset(&p, bar, None, 3.0, Fill::Base);
        let m = px(2.0).max(pixels(1.0));
        let pos = self.seek_drag.unwrap_or(now);
        let xp = bx(pos);
        if xp - bar.min.x > m {
            paint::pill(
                &p,
                Rect::from_min_size(
                    ipos(bar.min.x + m, bar.min.y + m),
                    vec2(
                        (snap(xp - bar.min.x) - m).max(bar.height() - 2.0 * m),
                        bar.height() - 2.0 * m,
                    ),
                ),
                th.accent,
            );
        }
        // a notch where each section starts, a mark at each end of the loop and a bar between them (whole pixels:
        // pygame draws them square)
        let (one, two) = (px(1.0).max(pixels(1.0)), px(2.0).max(pixels(2.0)));
        for sec in self.chart.sections.iter().skip(1) {
            let xx = snap(bx(sec.start));
            p.rect_filled(
                Rect::from_min_size(ipos(xx - one, bar.min.y + m), vec2(two, bar.height() - 2.0 * m)),
                0.0,
                th.track,
            );
        }
        if let (Some(la), Some(lb)) = (self.loop_a, self.loop_b) {
            let (la, lb) = (bx(la), bx(lb));
            paint::pill(
                &p,
                Rect::from_min_size(
                    ipos(la, bar.min.y - px(8.0)),
                    vec2(px(4.0).max(snap(lb - la)), px(4.0)),
                ),
                th.accent,
            );
            for xx in [la, lb] {
                p.rect_filled(
                    Rect::from_min_size(ipos(snap(xx) - one, bar.min.y - px(9.0)), vec2(two, px(30.0))),
                    0.0,
                    th.accent,
                );
            }
        }
        qrs_ui::keys::thumb(&p, pos2(snap(xp), mid_y(bar)), 26.0);
        // the section (click it: every section) and bar at the left, the loop in the middle, the time at the right
        let third = snap(bar.width() / 3.0);
        let mut left = self.draw_section_name(ui, bar.min.x, top, third);
        if !self.tab.measures.is_empty() {
            let num = self.tab.measure_at(now).map(|m| m.number).unwrap_or(0);
            let last = self.tab.measures.last().map(|m| m.number).unwrap_or(0);
            if num > 0 {
                let x = left.max.x + if left.width() > 0.0 { px(10.0) } else { 0.0 };
                left = txt(
                    &p,
                    &format!("bar {num} of {last}"),
                    ipos(x, top),
                    13.0,
                    th.caption,
                    500,
                    Align2::LEFT_TOP,
                    Some(third),
                );
            }
        }
        let tr = txt(
            &p,
            &format!(" / {}", fmt_time(self.chart.length)),
            ipos(bar.max.x, top),
            13.0,
            th.caption,
            500,
            Align2::RIGHT_TOP,
            None,
        );
        let mut right = txt(
            &p,
            &fmt_time(pos),
            ipos(tr.min.x, top),
            13.0,
            th.text_2,
            700,
            Align2::RIGHT_TOP,
            None,
        );
        let mut offs = Vec::new();
        if self.input_offset != 0.0 {
            offs.push(format!("guitar timing {:+.0} ms", self.input_offset * 1000.0));
        }
        if self.offset != 0.0 {
            offs.push(format!("sync {:+.0} ms", self.offset * 1000.0));
        }
        if !offs.is_empty() {
            right = txt(
                &p,
                &format!("{}  ·  ", offs.join("  ·  ")),
                ipos(right.min.x, top),
                13.0,
                th.caption,
                500,
                Align2::RIGHT_TOP,
                Some(third),
            );
        }
        let room = snap(right.min.x - left.max.x - px(24.0));
        if let (Some(la), Some(lb)) = (self.loop_a, self.loop_b)
            && ipx(room) > ipx(px(60.0))
        {
            let mut s = format!("Loop {}–{}", fmt_time(la), fmt_time(lb));
            if self.loop_passes > 0 {
                s += &format!("  ·  pass {}", self.loop_passes);
            }
            if let Some(acc) = self.loop_acc {
                s += &format!("  ·  {}%", (acc * 100.0).round() as i64);
            }
            // ((left.right + right.x) // 2, top)
            txt(
                &p,
                &s,
                ipos(snap(left.max.x + right.min.x) / 2.0, top),
                13.0,
                th.accent_ink,
                700,
                Align2::CENTER_TOP,
                Some(room),
            );
        }
        let _ = st;
    }

    /// The section you're in, over the seek groove, with a chevron: a click opens the list of every section.
    fn draw_section_name(&mut self, ui: &Ui, x: f32, y: f32, max_w: f32) -> Rect {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        self.sec_label = None;
        if self.chart.sections.is_empty() {
            return Rect::from_min_size(pos2(x, y), Vec2::ZERO);
        }
        let cur = self.section_here();
        let name = if cur >= 0 {
            section_label(&self.chart.sections[cur as usize])
        } else {
            "Sections".into()
        };
        let is_open = self.sec_menu.is_some();
        let st = Style::new(13.0, 800, th.text);
        let h = text::line_height(&ctx, st);
        let chev = px(22.0);
        let tw = text::width(&ctx, &name, st).min(max_w - chev);
        let label = Rect::from_min_size(
            ipos(x - px(8.0), y - px(4.0)),
            vec2(snap(px(8.0) + tw + chev + px(6.0)), snap(h + px(8.0))),
        );
        let resp = self.hit(ui, label, "sections");
        let hot = resp.hovered();
        if hot || is_open {
            paint::inset(&p, label, Some(9.0), 2.0, Fill::Base);
        }
        if hot {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        let tr = txt(
            &p,
            &name,
            ipos(x, y),
            13.0,
            if is_open { th.accent_ink } else { th.text },
            800,
            Align2::LEFT_TOP,
            Some(tw),
        );
        icons::icon(
            &p,
            if is_open { Icon::Down } else { Icon::Up },
            ipos(tr.max.x + snap(chev / 2.0), mid_y(tr)),
            16.0,
            if hot || is_open { th.accent_ink } else { th.text_2 },
        );
        self.sec_label = Some(label);
        Rect::from_min_size(ipos(x, y), vec2(snap(tr.width() + chev), tr.height()))
    }

    /// Every section of the song, in a list floating up from the section's name in the footer: the one you're in
    /// has its light on. Click one (or pick it with the arrows and Enter) to jump to it.
    fn draw_section_menu(&mut self, ui: &Ui) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let (Some(anchor), false) = (self.sec_label, self.chart.sections.is_empty()) else {
            self.sec_menu = None;
            return;
        };
        let n = self.chart.sections.len();
        let cur = self.section_here();
        let (pad, head) = (px(14.0), px(34.0));
        let (rh, inner) = (px(SEC_ROW), px(6.0));
        let full = snap(n as f32 * rh + 2.0 * inner);
        // up to the top of the tab
        let room = snap(anchor.min.y - px(10.0) - (self.well.min.y + px(10.0)) - 2.0 * pad - head);
        let list_h = full
            .min(snap(3.0 * rh + 2.0 * inner))
            .max(full.min(snap(snap(SEC_ROWS * rh) + 2.0 * inner)).min(room));
        let pw = pixels(to_px(380.0).min(ipx(self.well.width()) * 0.6).trunc() as f32);
        let ph = snap(2.0 * pad + head + list_h);
        // panel.bottom = anchor.y - int(8 * s)
        let panel = Rect::from_min_size(
            ipos(anchor.min.x - px(6.0), anchor.min.y - px(8.0) - ph),
            vec2(pw, ph),
        );
        self.sec_panel = Some(panel);
        // a click outside closes it (and does nothing else)
        let area = ctx.content_rect();
        let catch = interact(ui, area, "secmenu-catch", Sense::click());
        if catch.clicked()
            && catch
                .interact_pointer_pos()
                .is_some_and(|pos| !panel.contains(pos))
        {
            self.sec_menu = None;
            return;
        }
        paint::floating(&p, panel, Some(22.0));
        let hy = snap(panel.min.y + pad + snap(head / 2.0) - px(3.0));
        caps(
            &p,
            "SECTIONS",
            ipos(panel.min.x + pad + px(6.0), hy),
            12.0,
            Align2::LEFT_CENTER,
            None,
        );
        let kr = keycap(
            &p,
            "PgDn",
            ipos(panel.max.x - pad - px(4.0), hy),
            Align2::RIGHT_CENTER,
            11.0,
            CapOn::Material,
        );
        let kr = keycap(
            &p,
            "PgUp",
            ipos(kr.min.x - px(6.0), hy),
            Align2::RIGHT_CENTER,
            11.0,
            CapOn::Material,
        );
        txt(
            &p,
            "previous / next",
            ipos(kr.min.x - px(10.0), hy),
            12.0,
            th.caption,
            500,
            Align2::RIGHT_CENTER,
            None,
        );
        let well = Rect::from_min_size(
            ipos(panel.min.x + pad, panel.min.y + pad + head),
            vec2(snap(panel.width() - 2.0 * pad), list_h),
        );
        paint::well(&p, well, 16.0);
        let most = (full - list_h).max(0.0);
        let Some(menu) = &mut self.sec_menu else { return };
        if menu.scroll.is_none() {
            // just opened: the section you're in, in the middle
            menu.scroll = Some(cur.max(0) as f32 * rh + rh / 2.0 + inner - list_h / 2.0);
        }
        if std::mem::take(&mut menu.show) {
            let top_y = menu.key as f32 * rh;
            menu.scroll = Some(
                menu.scroll
                    .unwrap_or(0.0)
                    .max(top_y + rh + 2.0 * inner - list_h)
                    .min(top_y),
            );
        }
        let sc = menu.scroll.unwrap_or(0.0).clamp(0.0, most);
        menu.scroll = Some(sc);
        let key = menu.key;
        let clip = inflate(well, -px(4.0), -px(4.0));
        let lp = p.with_clip_rect(clip);
        let groove = if most > 0.0 { px(14.0) } else { 0.0 };
        let first = ((sc - inner) / rh).floor().max(0.0) as usize;
        let mut jump: Option<usize> = None;
        let mut hover_key: Option<usize> = None;
        for i in first..n {
            let y = snap(well.min.y + inner + i as f32 * rh - snap(sc));
            if ipx(y) > ipx(well.max.y) {
                break;
            }
            let sec = &self.chart.sections[i];
            let row = Rect::from_min_size(
                ipos(well.min.x + inner, y),
                vec2(snap(well.width() - 2.0 * inner - groove), snap(rh - px(2.0))),
            );
            let cy = mid_y(row);
            let vis = row.intersect(clip);
            if vis.height() > 0.0 {
                let resp = interact(ui, vis, ("secrow", i), Sense::click());
                if resp.hovered() {
                    hover_key = Some(i);
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    jump = Some(i);
                }
            }
            if i == key {
                paint::raised(&lp, row, Some(12.0), 3.0, Fill::Base, 0.0);
            }
            let here = i as i64 == cur;
            if here {
                paint::led(&lp, ipos(row.min.x + px(15.0), cy), 7.0, th.accent);
            }
            let x = snap(row.min.x + px(28.0));
            let tr = txt(
                &lp,
                &fmt_time(sec.start),
                ipos(row.max.x - px(14.0), cy),
                13.0,
                th.text_2,
                700,
                Align2::RIGHT_CENTER,
                None,
            );
            let bar_no = self
                .tab
                .measure_at(sec.start + 1e-3)
                .map(|m| m.number)
                .unwrap_or(0);
            let br = txt(
                &lp,
                &if bar_no > 0 {
                    format!("bar {bar_no}")
                } else {
                    String::new()
                },
                ipos(tr.min.x - px(14.0), cy),
                12.0,
                th.caption,
                500,
                Align2::RIGHT_CENTER,
                None,
            );
            txt(
                &lp,
                &section_label(sec),
                pos2(x, cy),
                14.0,
                if here { th.accent_ink } else { th.text },
                if here { 800 } else { 700 },
                Align2::LEFT_CENTER,
                Some(snap(br.min.x - px(12.0) - x)),
            );
        }
        if most > 0.0 {
            paint::fade_edges(
                &p,
                clip,
                sc > 0.5,
                sc < most - 0.5,
                18.0,
                paint::FaceKind::Base {
                    rect: well,
                    sunk: true,
                },
            );
            // where you are in a long list
            let track = Rect::from_min_size(
                ipos(well.max.x - px(12.0), well.min.y + px(10.0)),
                vec2(px(4.0), snap(well.height() - px(20.0))),
            );
            let thh = px(26.0).max(snap(track.height() * list_h / full));
            paint::inset(&p, track, None, 2.0, Fill::Base);
            paint::pill(
                &p,
                Rect::from_min_size(
                    ipos(track.min.x, track.min.y + (track.height() - thh) * sc / most),
                    vec2(track.width(), thh),
                ),
                th.tick,
            );
        }
        if let Some(i) = hover_key
            && let Some(m) = &mut self.sec_menu
        {
            m.key = i;
        }
        if let Some(i) = jump {
            self.go_to_section(i);
        }
    }

    /// The console: one raised panel holding the tiles (each a screen sunk into it) or the controls, with the key
    /// that swaps them at the bottom.
    fn draw_aside(&mut self, ui: &mut Ui, st: &mut AppState, aside: Rect, now: f64) {
        use qrs_ui::units::px;
        let p = ui.painter().clone();
        paint::raised(&p, aside, Some(28.0), 8.0, Fill::Base, 0.0);
        let pad = px(16.0);
        let inner = aside.shrink(pad);
        let bh = px(48.0);
        let btn = Rect::from_min_size(pos2(inner.min.x, inner.max.y - bh), vec2(inner.width(), bh));
        let area = Rect::from_min_max(inner.min, pos2(inner.max.x, btn.min.y - px(12.0)));
        self.panel_rect = area;
        if self.show_controls {
            self.tile_rects.clear();
            self.draw_controls(ui, st, area);
            if button(
                ui,
                btn,
                "aside-swap",
                "Tiles",
                "",
                ButtonStyle {
                    icon: Some(Icon::Grid),
                    size: 15.0,
                    r: Some(16.0),
                    ..Default::default()
                },
            )
            .clicked()
            {
                self.acts.push("controls".into());
            }
        } else {
            self.draw_tile_column(ui, st, area, now, aside);
            if button(
                ui,
                btn,
                "aside-swap",
                "Controls",
                "",
                ButtonStyle {
                    icon: Some(Icon::Sliders),
                    size: 15.0,
                    r: Some(16.0),
                    ..Default::default()
                },
            )
            .clicked()
            {
                self.acts.push("controls".into());
            }
        }
    }

    /// A warning at the top of the tiles when the song plays without sound.
    fn draw_no_audio(&mut self, ui: &Ui, x: f32, y: f32, w: f32) -> f32 {
        use qrs_ui::units::px;
        use tiles::at;
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let pad = px(16.0);
        let (why, then) = if self.engine().is_none() {
            (
                format!(
                    "The audio engine isn't running ({}).",
                    if self.audio_error.is_empty() {
                        "no sound device"
                    } else {
                        &self.audio_error
                    }
                ),
                "The chart plays silently.",
            )
        } else {
            (
                format!("The song's audio couldn't be played: {}.", self.audio_error),
                "The chart plays silently. Ctrl+drag still plays passages on the synth guitar.",
            )
        };
        let lines: Vec<String> = text::wrap(
            &ctx,
            &format!("{why} {then}"),
            Style::new(13.0, 500, th.text_2),
            w - 2.0 * pad,
        )
        .into_iter()
        .take(4)
        .collect();
        let card = Rect::from_min_size(
            pos2(x, y),
            vec2(
                w,
                pad + px(22.0) + lines.len() as f32 * px(19.0) + px(12.0) + px(40.0) + pad,
            ),
        );
        paint::display(&p, card, Some(18.0), 6.0);
        paint::ring(&p, card, Some(18.0), 1.0, th.miss, false);
        text::caps(
            &p,
            "NO SONG AUDIO",
            at(pos2(card.min.x + pad, card.min.y + pad)),
            12.0,
            th.miss,
            Align2::LEFT_TOP,
            None,
        );
        let mut yy = card.min.y + pad + px(24.0);
        for line in &lines {
            txt(
                &p,
                line,
                at(pos2(card.min.x + pad, yy)),
                13.0,
                th.text_2,
                500,
                Align2::LEFT_TOP,
                None,
            );
            yy += px(19.0);
        }
        let b = Rect::from_min_size(
            pos2(card.min.x + pad, card.max.y - pad - px(40.0)),
            vec2(button_width(&ctx, "Audio settings", "F2", 13.0, false), px(40.0)),
        );
        if button(
            ui,
            b,
            "noaudio-settings",
            "Audio settings",
            "F2",
            ButtonStyle {
                size: 13.0,
                r: Some(13.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            self.acts.push("settings".into());
        }
        card.max.y
    }

    // -- states over the tab ----------------------------------------------------------------------------------

    fn draw_states(&mut self, ui: &Ui, well: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let mut summary = false;
        if let Some(until) = self.countdown_until {
            let remaining = until.saturating_duration_since(Instant::now()).as_secs_f32();
            let plate = self.state_plate(ui, well, px(250.0));
            let c = ipos(mid_x(plate), plate.min.y + px(26.0) + px(68.0));
            knobs::ring_gauge(&p, c, 60.0, 8.0, (remaining / COUNTDOWN).clamp(0.0, 1.0), None);
            txt(
                &p,
                &((remaining as i64) + 1).max(1).to_string(),
                c,
                52.0,
                th.accent_ink,
                800,
                Align2::CENTER_CENTER,
                None,
            );
            self.key_line(
                &p,
                &["Get ready  ·  ", "[Space]", " to start now"],
                mid_x(plate),
                snap(plate.max.y - px(34.0)),
                15.0,
                th.text_2,
            );
            if self.hit(ui, plate, "play").clicked() {}
        } else if self.summary_up() {
            summary = true;
            if !self.backdrop.capturing() {
                // the next frame, the player alone like this one, is captured: blurred under the card from then on
                self.backdrop.capture(ui.ctx());
            }
        }
        let _ = summary;
        let prog = self
            .shot_stretch
            .or_else(|| self.stretch.as_ref().filter(|j| !j.done()).map(|j| j.progress()));
        let buffering = self.engine().is_some_and(|e| e.buffering());
        if self.pending_speed.is_some() || prog.is_some() || buffering {
            self.draw_slowing(&p, well, prog);
        }
        if let Some((text_, tail, until)) = &self.toast
            && Instant::now() < *until
        {
            widgets::toast(&p, text_, ipos(mid_x(well), well.min.y + px(44.0)), tail);
        }
    }

    /// The raised card the count-in floats on, in the middle of the tab.
    fn state_plate(&mut self, ui: &Ui, well: Rect, height: f32) -> Rect {
        let w = pixels(to_px(340.0).min(ipx(well.width()) - to_px(40.0)).trunc() as f32);
        // plate.center = well.center
        let plate = Rect::from_min_size(
            ipos(mid_x(well) - snap(w / 2.0), mid_y(well) - snap(height / 2.0)),
            vec2(w, height),
        );
        paint::raised(ui.painter(), plate, Some(26.0), 8.0, Fill::Base, 0.0);
        plate
    }

    /// A centred line of text where "[Key]" parts are drawn as key caps.
    fn key_line(&self, p: &Painter, parts: &[&str], cx: f32, cy: f32, size: f32, color: Color32) {
        let ctx = p.ctx();
        let widths: Vec<f32> = parts
            .iter()
            .map(|s| {
                if s.starts_with('[') {
                    cap_w(ctx, &s[1..s.len() - 1])
                } else {
                    text::width(ctx, s, Style::new(size, 500, color))
                }
            })
            .collect();
        let mut x = snap(cx - snap(widths.iter().sum::<f32>() / 2.0));
        for (s, w) in parts.iter().zip(&widths) {
            if s.starts_with('[') {
                keycap(
                    p,
                    &s[1..s.len() - 1],
                    ipos(x, cy),
                    Align2::LEFT_CENTER,
                    11.0,
                    CapOn::Material,
                );
            } else {
                txt(p, s, ipos(x, cy), size, color, 500, Align2::LEFT_CENTER, None);
            }
            x = snap(x + w);
        }
    }

    /// A card at the bottom of the tab while a new speed is being prepared.
    fn draw_slowing(&self, p: &Painter, well: Rect, prog: Option<f64>) {
        let th = t();
        let pct = (self.speed() * 100.0).round() as i64;
        let (title, tail, u) = match prog {
            Some(pr) if self.pending_speed.is_none() => (
                "Slowing down the rest of the song… ".to_string(),
                format!("{}%", (pr * 100.0).round() as i64),
                Some(pr),
            ),
            _ => (format!("Preparing {pct}% speed…"), String::new(), None),
        };
        // card.midbottom = (well.centerx, well.bottom - int(24 * s))
        let (cw, ch) = (
            pixels(to_px(420.0).min(ipx(well.width()) - to_px(40.0)).trunc() as f32),
            px(108.0),
        );
        let card = Rect::from_min_size(
            ipos(mid_x(well) - snap(cw / 2.0), well.max.y - px(24.0) - ch),
            vec2(cw, ch),
        );
        paint::raised(p, card, Some(22.0), 8.0, Fill::Base, 0.0);
        let x = snap(card.min.x + px(22.0));
        let tr = txt(
            p,
            &title,
            ipos(x, card.min.y + px(18.0)),
            15.0,
            th.text,
            700,
            Align2::LEFT_TOP,
            Some(snap(card.width() - px(80.0))),
        );
        if !tail.is_empty() {
            txt(
                p,
                &tail,
                ipos(tr.max.x, tr.min.y),
                15.0,
                th.accent_ink,
                800,
                Align2::LEFT_TOP,
                None,
            );
        }
        let groove = Rect::from_min_size(
            ipos(x, card.min.y + px(50.0)),
            vec2(snap(card.width() - px(44.0)), px(10.0)),
        );
        paint::inset(p, groove, None, 3.0, Fill::Base);
        if let Some(u) = u {
            let m = px(2.0).max(pixels(1.0));
            paint::pill(
                p,
                Rect::from_min_size(
                    ipos(groove.min.x + m, groove.min.y + m),
                    vec2(
                        snap((groove.width() - 2.0 * m) * u as f32).max(groove.height() - 2.0 * m),
                        groove.height() - 2.0 * m,
                    ),
                ),
                th.accent,
            );
        }
        txt(
            p,
            &format!("{pct}% speed  ·  playback resumes in about a second"),
            ipos(x, card.min.y + px(72.0)),
            13.0,
            th.caption,
            500,
            Align2::LEFT_TOP,
            Some(snap(card.width() - px(44.0))),
        );
    }

    /// The results are up: the song ran to its end with something played.
    fn summary_up(&self) -> bool {
        self.countdown_until.is_none()
            && !self.transport.playing()
            && self.transport.finished()
            && self.scorer.groups.iter().any(|g| g.state.is_some())
    }

    /// The results card over the blurred, dimmed player: accuracy as a ring, the score, the counts, your best runs
    /// (this one lit), Play again / Library.
    fn draw_summary(&mut self, ui: &Ui, st: &AppState, area: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let _ = interact(ui, area, "summary-modal", Sense::click()); // modal: only its buttons (and keys) do anything
        let c = self.scorer.counts();
        let scored = self.result_scored;
        let top = st.userdata.top(&self.ids, &self.part_name);
        let (w, h) = (snap(area.width()), snap(area.height()));
        let pw = pixels(to_px(620.0).min(ipx(w) - to_px(40.0)).trunc() as f32);
        let (px_, py_) = (px(34.0), px(30.0));
        let runs_h = if top.is_empty() {
            0.0
        } else {
            snap(px(14.0) + px(16.0) + px(10.0) + top.len() as f32 * px(26.0) + px(10.0))
        };
        let ph = snap(
            py_ + px(44.0)
                + px(20.0)
                + px(150.0)
                + px(20.0)
                + px(70.0)
                + px(20.0)
                + if top.is_empty() { 0.0 } else { runs_h + px(20.0) }
                + px(54.0)
                + py_,
        );
        // ((w - pw) // 2, max(int(20 * s), (h - ph) // 2))
        let panel = Rect::from_min_size(
            ipos(
                area.min.x + snap((w - pw) / 2.0),
                area.min.y + px(20.0).max(pixels(((ipx(h) - ipx(ph)) / 2.0).floor() as f32)),
            ),
            vec2(pw, ph),
        );
        paint::floating(&p, panel, Some(32.0));
        let (lx, rx) = (snap(panel.min.x + px_), snap(panel.max.x - px_));
        let mut y = snap(panel.min.y + py_);
        let (chip_text, chip_col, chip_icon) = if self.new_best {
            ("New best score", th.hit, Some(Icon::Up))
        } else if !scored {
            ("Not saved", th.late, None)
        } else {
            ("", th.text, None)
        };
        // the header: what this was, and a chip for a new best (or why it wasn't saved)
        let mut chip_x = rx;
        if !chip_text.is_empty() {
            let cw = snap(
                text::width(&ctx, chip_text, Style::new(13.0, 800, chip_col))
                    + px(24.0)
                    + if chip_icon.is_some() { px(20.0) } else { 0.0 },
            );
            let chip = Rect::from_min_size(ipos(rx - cw, y + px(4.0)), vec2(cw, px(34.0)));
            paint::inset(&p, chip, Some(11.0), 2.0, Fill::Base);
            let ccy = mid_y(chip);
            let mut cx = snap(chip.min.x + px(12.0));
            if let Some(ic) = chip_icon {
                icons::icon(&p, ic, ipos(cx + px(7.0), ccy), 14.0, chip_col);
                cx = snap(cx + px(20.0));
            }
            txt(
                &p,
                chip_text,
                pos2(cx, ccy),
                13.0,
                chip_col,
                800,
                Align2::LEFT_CENTER,
                None,
            );
            chip_x = chip.min.x;
        }
        caps(
            &p,
            if scored { "SONG COMPLETE" } else { "PRACTICE RUN" },
            pos2(lx, y),
            12.0,
            Align2::LEFT_TOP,
            None,
        );
        txt(
            &p,
            &format!("{}  ·  {}", self.entry.display_title(), self.part_name),
            ipos(lx, y + px(18.0)),
            20.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(snap(chip_x - lx - px(16.0))),
        );
        y = snap(y + px(44.0) + px(20.0));
        // the accuracy ring, and the score beside it
        let judged = c.judged();
        let acc = self.scorer.accuracy();
        let rc = ipos(lx + px(75.0), y + px(75.0));
        knobs::ring_gauge(
            &p,
            rc,
            64.0,
            12.0,
            if judged > 0 { acc as f32 } else { 0.0 },
            None,
        );
        txt(
            &p,
            &format!("{}%", (acc * 100.0).round() as i64),
            ipos(rc.x, rc.y - px(7.0)),
            34.0,
            th.text,
            800,
            Align2::CENTER_CENTER,
            None,
        );
        txt(
            &p,
            "accuracy",
            ipos(rc.x, rc.y + px(19.0)),
            12.0,
            th.caption,
            700,
            Align2::CENTER_CENTER,
            None,
        );
        let sx = snap(lx + px(150.0) + px(30.0));
        caps(&p, "SCORE", ipos(sx, y + px(26.0)), 12.0, Align2::LEFT_TOP, None);
        txt(
            &p,
            &group_digits(self.scorer.score() as usize),
            ipos(sx - px(2.0), y + px(40.0)),
            54.0,
            if scored { th.text } else { th.text_2 },
            800,
            Align2::LEFT_TOP,
            Some(snap(rx - sx)),
        );
        let mut ly = snap(y + px(108.0));
        if !scored {
            for line in text::wrap(
                &ctx,
                "Only a full run at 100% difficulty and speed counts for the high score.",
                Style::new(13.0, 700, th.late),
                snap(rx - sx),
            )
            .into_iter()
            .take(2)
            {
                txt(
                    &p,
                    &line,
                    pos2(sx, ly),
                    13.0,
                    th.late,
                    700,
                    Align2::LEFT_TOP,
                    None,
                );
                ly = snap(ly + px(18.0));
            }
        } else {
            let r1 = txt(
                &p,
                "best streak ",
                pos2(sx, ly),
                14.0,
                th.text_2,
                500,
                Align2::LEFT_TOP,
                None,
            );
            let r2 = txt(
                &p,
                &self.scorer.best_streak.to_string(),
                ipos(r1.max.x, ly),
                14.0,
                th.text,
                800,
                Align2::LEFT_TOP,
                None,
            );
            if self.new_best && top.len() > 1 {
                let r3 = txt(
                    &p,
                    "  ·  up ",
                    ipos(r2.max.x, ly),
                    14.0,
                    th.text_2,
                    500,
                    Align2::LEFT_TOP,
                    None,
                );
                let r4 = txt(
                    &p,
                    &group_digits((top[0].score - top[1].score).max(0) as usize),
                    ipos(r3.max.x, ly),
                    14.0,
                    th.hit,
                    800,
                    Align2::LEFT_TOP,
                    None,
                );
                txt(
                    &p,
                    " on your old best",
                    ipos(r4.max.x, ly),
                    14.0,
                    th.text_2,
                    500,
                    Align2::LEFT_TOP,
                    Some(snap(rx - snap(r4.max.x))),
                );
            }
        }
        y = snap(y + px(150.0) + px(20.0));
        // the counts, each in a small screen
        let cols = [
            ("hit", c.hit, th.hit),
            ("late", c.late, th.late),
            ("early", c.early, th.late),
            ("missed", c.miss, th.miss),
        ];
        let gap = px(12.0);
        let bw = (rx - lx - 3.0 * gap) / 4.0;
        for (i, (label, val, col)) in cols.iter().enumerate() {
            let r = Rect::from_min_size(ipos(lx + i as f32 * (bw + gap), y), vec2(snap(bw), px(70.0)));
            paint::display(&p, r, Some(14.0), 4.0);
            txt(
                &p,
                &val.to_string(),
                ipos(r.min.x + px(14.0), r.min.y + px(12.0)),
                22.0,
                *col,
                800,
                Align2::LEFT_TOP,
                None,
            );
            txt(
                &p,
                label,
                ipos(r.min.x + px(14.0), r.max.y - px(12.0)),
                12.0,
                th.caption,
                500,
                Align2::LEFT_BOTTOM,
                None,
            );
        }
        y = snap(y + px(70.0) + px(20.0));
        if !top.is_empty() {
            let bx = Rect::from_min_size(pos2(lx, y), vec2(snap(rx - lx), runs_h));
            paint::display(&p, bx, Some(16.0), 4.0);
            let x0 = snap(bx.min.x + px(18.0));
            caps(
                &p,
                "YOUR BEST RUNS",
                ipos(x0, bx.min.y + px(14.0)),
                11.0,
                Align2::LEFT_TOP,
                None,
            );
            let mut ty = snap(bx.min.y + px(14.0) + px(16.0) + px(10.0));
            for (i, run) in top.iter().enumerate() {
                let mine = scored && self.result_rank == Some(i); // the run you just finished
                let cy = snap(ty + px(10.0));
                if mine {
                    paint::led(&p, ipos(x0 + px(4.0), cy), 8.0, th.accent);
                } else {
                    paint::dot(&p, ipos(x0 + px(4.0), cy), 4.0, th.unlit);
                }
                let acc = run
                    .acc
                    .map(|a| format!("  ·  {}%", (a * 100.0).round() as i64))
                    .unwrap_or_default();
                txt(
                    &p,
                    &format!("{}.  {}{acc}", i + 1, group_digits(run.score as usize)),
                    ipos(x0 + px(20.0), cy),
                    14.0,
                    if mine { th.accent_ink } else { th.text_2 },
                    if mine { 800 } else { 600 },
                    Align2::LEFT_CENTER,
                    None,
                );
                let tag_w = if mine {
                    text::width(&ctx, "this run", Style::new(13.0, 800, th.accent_ink))
                } else {
                    0.0
                };
                txt(
                    &p,
                    &fmt_when(run.at, true),
                    ipos(bx.min.x + px(220.0), cy),
                    13.0,
                    th.caption,
                    500,
                    Align2::LEFT_CENTER,
                    Some(snap(
                        bx.max.x - px(18.0) - tag_w - px(12.0) - bx.min.x - px(220.0),
                    )),
                );
                if mine {
                    txt(
                        &p,
                        "this run",
                        ipos(bx.max.x - px(18.0), cy),
                        13.0,
                        th.accent_ink,
                        800,
                        Align2::RIGHT_CENTER,
                        None,
                    );
                }
                ty = snap(ty + px(26.0));
            }
            y = snap(bx.max.y + px(20.0));
        }
        let lib_w = snap(button_w(&ctx, "Library", "Esc", 15.0, false) + px(20.0));
        let lib = Rect::from_min_size(ipos(rx - lib_w, y), vec2(lib_w, px(54.0)));
        let again = Rect::from_min_size(pos2(lx, y), vec2(snap(lib.min.x - px(14.0) - lx), px(54.0)));
        if button(
            ui,
            again,
            "sum-again",
            "Play again",
            "Space",
            ButtonStyle {
                primary: true,
                icon: Some(Icon::Restart),
                size: 16.0,
                r: Some(17.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            self.acts.push("play".into());
        }
        if button(
            ui,
            lib,
            "sum-lib",
            "Library",
            "Esc",
            ButtonStyle {
                size: 15.0,
                r: Some(17.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            self.acts.push("library".into());
        }
    }
}

pub fn tone_label(t: &qrs_audio::tone::ToneSpec) -> String {
    match t {
        qrs_audio::tone::ToneSpec::AsIs => "As is".into(),
        qrs_audio::tone::ToneSpec::Preset { preset } => preset.label().into(),
        qrs_audio::tone::ToneSpec::Custom { .. } => "Custom".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::snap_at;

    #[test]
    fn snap_keeps_the_python_pixels() {
        let s = 1440.0f64 / 1400.0;
        // a sum of whole-pixel points comes back as that whole pixel, for the text (which truncates at a
        // pixels-per-point that can be a hair under the scale) and for the components (which round)
        let ppp = s * (1.0 - 1e-7);
        for a in 0..1100 {
            for b in [0, 1, 7, 33, 64, 150, 333] {
                let v = (a as f64 / s) as f32 + (b as f64 / s) as f32;
                let k = snap_at(v, s) as f64;
                assert_eq!((k * ppp).trunc() as i64, a + b, "{a} + {b}");
                assert_eq!((k * s).round() as i64, a + b, "{a} + {b}");
            }
        }
        // a fractional pixel truncates toward zero, as Python's int()
        assert_eq!((snap_at((36.7 / s) as f32, s) as f64 * s).round(), 36.0);
        assert_eq!((snap_at((-4.5 / s) as f32, s) as f64 * s).round(), -4.0);
        // no scale yet: points pass through
        assert_eq!(snap_at(16.5, 0.0), 16.5);
    }
}
