//! The player (port of rsplay/app.py `PlayerView`): the song's header with the part keys, the speed knob and the
//! guitar's status strip; the tab (tab.rs); the transport, seek groove and volume knobs in the footer; the
//! console on the right with the tiles (tiles.rs) or the controls (controls.rs); the count-in, toasts, the
//! slow-down card, the sections list and the results card. Every key of the Python player works the same.
//! New here: ear mode (H), for a lesson's riff the coach in the console (coach.rs), and note by note (W,
//! notebynote.rs: the song waits at each note until you play it).

mod arcade;
pub mod board;
pub mod coach;
mod continuous;
pub(crate) mod controls;
mod footer_keys;
pub(crate) mod gearinfo;
mod highway;
pub use highway::{Backdrop, GEM_SIZES, NoteColours, gem_from};
/// The highway's note sizes, for Settings > Display (D143).
pub mod highway_size {
    pub use super::highway::{SIZES, size_from, size_label, size_pct};
}
pub(crate) mod history;
mod hud;
mod notebynote;
mod practise;
mod review;
mod rules;
mod runstats;
#[cfg(test)]
mod scenarios;
mod songaudio;
mod songsynth;
mod tab;
mod take;
mod theory;
pub mod tiles;
mod transport_art;
mod transport_room;
mod tuning;

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
use runstats::{STATS_H, draw_run_stats, summary_line};
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
/// How far the tab's bars can be widened or narrowed, and its strings spread or closed up, on top of the zoom.
const WIDTH_K: (f32, f32) = (0.5, 2.5);
const HEIGHT_K: (f32, f32) = (0.6, 1.8);

/// What the zoom HUD shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ZoomWhat {
    Zoom,
    Width,
    Height,
}
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

/// R's action: record what you hear, pause, resume (the lamp row's rec key too); Shift R stops and saves; Ctrl R
/// records a take of the loop, scored (D30, D53).
fn r_key(ctrl: bool, shift: bool) -> &'static str {
    if ctrl {
        "take:record"
    } else if shift {
        "rec:stop"
    } else {
        hud::REC_ACT
    }
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
    /// The part's own key, from its notes (`keyguess::song_key`): the chord names as numbers go by it.
    pub key: Option<qrs_analysis::keyguess::Key>,
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
        let btimes: Vec<f64> = beats.iter().map(|b| b.time).collect();
        // the delays' Sync plays at the song's tempo (P37.2)
        qrs_audio::board::set_tempo(qrs_audio::board::tempo_of_beats(&btimes));
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
            key: qrs_analysis::keyguess::song_key(&arr),
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
    /// Back to the Lessons page (the coach's).
    Lessons,
    /// On to a lesson's riff (its song, with the coach).
    Riff {
        lesson: String,
        riff: String,
    },
    /// Another song (by its id).
    Song {
        id: String,
    },
    /// The amp room in this song's context (the Pedalboard tile's expand key, P37.1).
    Room,
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
    /// Ear mode (H, P13.1): the tab's notes hidden until you find them.
    pub ear: bool,
    /// For each of `chart.glyphs`: shown in ear mode (played right, or found in a lesson).
    pub revealed: Vec<bool>,
    /// A lesson's riff being learned (P13.5): the coach in the console.
    pub coach: Option<coach::Coach>,
    /// A take of a stretch (P17.11): recording, analysing, its report and "your tab".
    take: Option<take::Session>,
    /// The last run's review (P29.1): its spots, and the one being practised.
    review: Option<review::Review>,
    /// This part's saved runs (newest first), as last read or written; and where they live.
    runs: Vec<history::SavedRun>,
    /// Whose runs `runs` are (the history key they were read for; the review key reads them once per part).
    runs_for: String,
    /// The footer's restart key and the review key's hit rect (P37.8), and whether the review key's menu is up.
    restart_rect: Rect,
    review_rect: Rect,
    review_menu: bool,
    reviews_path: std::path::PathBuf,
    /// Continuous mode (O, P29.2): the loop runs straight on, every pass scored.
    continuous: bool,
    /// Continuous mode's game (P29.3): multiplier, score, confetti.
    arcade: arcade::Arcade,
    /// Each loop's pass accuracies while the song is open (by `continuous::loop_key`).
    pass_hist: std::collections::HashMap<(i64, i64), Vec<f64>>,
    /// Note by note (W, P15.3): the song stops at each note still to be played until you play it.
    pub nbn: bool,
    /// The note the song is stopped at, waiting for you.
    waiting: Option<notebynote::Waiting>,
    /// The jewel lamp's light as drawn (0 off, 1 on, up to 2 breathing).
    nbn_lit: f32,
    pub tab: TabView,
    /// The 3D highway (D32), shown instead of the tab while `hw_on` (V).
    pub highway: highway::Highway,
    pub hw_on: bool,
    pub synth: SynthPlayer,
    pub sel_loop: bool,
    pub sel_slow: f64,
    pub sel_track: bool,
    pub loop_a: Option<f64>,
    pub loop_b: Option<f64>,
    loop_passes: u32,
    loop_acc: Option<f64>,
    /// Chart sync (`av_offset`): +: notes later than audio (the picture and the scoring both).
    pub offset: f64,
    /// Your hits (`input_offset`): +: count your playing later (a pluck heard at `st` counts at `st - offset +
    /// input_offset`, `timing_cal::chart_pluck`).
    pub input_offset: f64,
    /// Screen (`display_offset`, P36.1): +: draw the notes that much further on, for a screen that lags; the picture
    /// only, never the scoring, seeking or loops (`draw_now`).
    pub display_offset: f64,
    toast: Option<(String, String, Instant)>,
    /// The keys of the press that made the toast (a keycap flash at the tab's foot), or none (a toast at its top).
    toast_keys: Vec<String>,
    /// The keys being handled, for the toast their action makes.
    pressed: Option<Vec<String>>,
    /// The Shortcuts sheet is up.
    shortcuts: bool,
    countdown_until: Option<Instant>,
    /// Opened paused and not played yet: the first Play counts in (as opening a song does).
    fresh: bool,
    /// The count-in before playing (config `countdown`, D121): off, everything plays at once.
    countdown_on: bool,
    /// Where the practice range came from (rules.rs, D119).
    range_from: Option<rules::RangeFrom>,
    /// The review page's return: where the song was and whether it played (rules.rs).
    review_return: Option<rules::ReviewReturn>,
    /// Your range before a review spot replaced it (rules.rs).
    range_before: Option<rules::RangeBefore>,
    /// Chart time at the last frame (the loop goes round only when the song plays out of its end); none after a seek.
    loop_prev: Option<f64>,
    pub lyrics: Vec<Line>,
    lyric_starts: Vec<f64>,
    pub lyrics_next: bool,
    pub show_controls: bool,
    ctrl_scroll: f32,
    ctrl_max: f32,
    /// The controls' difficulty slider while it's dragged: where it will go.
    diff_preview: Option<f64>,
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
    /// What the input plays (P32): followed each frame; the scorer, the tuner and the pitch reading use it.
    pub(super) instrument: qrs_analysis::instrument::Instrument,
    inst_hinted: bool,
    pub tuner_text: String,
    /// The tuner's tuning card (D136) hidden for this song (its ×).
    retune_hidden: bool,
    tuner_at: Instant,
    zoom_at: Option<Instant>,
    /// Which of the tab's sizes the HUD shows (the zoom, the bars' width or the strings' spacing).
    zoom_what: ZoomWhat,
    /// The tab fills the screen (owner request): no header, transport or console; F or Esc to leave.
    pub tab_full: bool,
    /// Chord names as numbers in the part's key (config "chord_numerals"; the tab and the highway).
    pub chord_numerals: bool,
    /// The transport's baked art (loaded on the first frame that draws it), the gem's light, and the last frame.
    art: Option<transport_art::Art>,
    gem_glow: transport_art::Glow,
    art_at: Option<Instant>,
    /// Where the tab's toolbar is (as last drawn: open, rolled up or between).
    tools_rect: Rect,
    tip: Option<(String, String, Pos2, Align2)>,
    sec_menu: Option<SecMenu>,
    sec_jump: Option<usize>,
    sec_label: Option<Rect>,
    sec_panel: Option<Rect>,
    pending_speed: Option<f64>,
    speed_at: Instant,
    stretch: Option<StretchJob>,
    /// The song's stem mixer and guitar-input transposition (P20.3).
    mixer: songaudio::Mixer,
    /// The synth guitar playing the part in place of the song's (P23.11).
    song_synth: songsynth::SongSynth,
    pub nota: Nota,
    pub auto_level: bool,
    run_scored: bool,
    recorded: bool,
    new_best: bool,
    /// The results card was closed (its x or Esc, P23.12): the song page stays as it is, any recording running,
    /// until the song plays again or ends again.
    summary_dismissed: bool,
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
    /// Screenshot mode: the streak shown on the score tile (QUICKRS_SHOT_STREAK or a "streak_<n>" shot, P37.7).
    shot_streak: Option<u32>,
    /// Screenshot mode: the part as loaded while a shot shows it with techniques added (`shot_techs`).
    shot_arr: Option<Arrangement>,
    /// The score tile's streak effect (P37.7).
    streak_fx: qrs_ui::streak::StreakFx,
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
    /// The Pedalboard tile and its big view (P14.3-P14.4).
    pub rack: board::Rack,
}

/// A bass part on a guitar, or a guitar part on a bass, says so once (P32.1; config.json `instrument_hints` keeps
/// which were said).
fn instrument_hint(st: &mut AppState, inst: qrs_analysis::instrument::Instrument, part_is_bass: bool) {
    let (key, msg) = match (inst.is_bass(), part_is_bass) {
        (false, true) => (
            "guitar_on_bass",
            "A bass part: play it on your guitar's low strings (scored an octave up too). Playing a bass? \
             Settings > Sound > Instrument",
        ),
        (true, false) => (
            "bass_on_guitar",
            "A guitar part on your bass: notes count at pitch or an octave down. The bass part opens first when \
             there is one",
        ),
        _ => return,
    };
    let mut said: Vec<serde_json::Value> = st
        .config
        .get("instrument_hints")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if said.iter().any(|v| v.as_str() == Some(key)) {
        return;
    }
    said.push(serde_json::json!(key));
    st.config.set("instrument_hints", serde_json::Value::Array(said));
    st.save_config();
    st.say(msg);
}

impl PlayerView {
    /// The tab's (or highway's) well as last drawn: notices over the song centre on it, not the window (P37.4b).
    pub fn well_rect(&self) -> Option<Rect> {
        self.well.is_positive().then_some(self.well)
    }

    pub fn new(st: &mut AppState, loaded: crate::loading::Loaded) -> PlayerView {
        let crate::loading::Loaded {
            g,
            entry,
            part,
            arr,
            lyrics,
            song,
            stems,
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
        let saved_audio = st.userdata.song_audio(&sid);
        st.userdata.alias(&canonical, &sid);
        st.userdata.started(&sid, &part_name);
        let mut transport = transport;
        transport.set_volume(st.config.f64("volume", 0.8) as f32);
        let lyric_starts = lyrics.iter().map(|l| l.start()).collect();
        let revealed = vec![false; chart.glyphs.len()];
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
            ear: false,
            revealed,
            coach: None,
            take: None,
            nbn: false,
            waiting: None,
            nbn_lit: 0.0,
            tab: TabView::shaped(
                (st.config.f64("tab_width", 1.0) as f32).clamp(WIDTH_K.0, WIDTH_K.1),
                (st.config.f64("tab_height", 1.0) as f32).clamp(HEIGHT_K.0, HEIGHT_K.1),
                st.config.bool("tab_compact", false),
            ),
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
            display_offset: st.config.f64("display_offset", 0.0),
            toast: None,
            toast_keys: Vec::new(),
            pressed: None,
            shortcuts: false,
            countdown_until: None,
            fresh: false,
            countdown_on: st.config.bool("countdown", true),
            range_from: None,
            review_return: None,
            range_before: None,
            loop_prev: None,
            lyrics,
            lyric_starts,
            lyrics_next: st.config.bool("lyrics_next", true),
            show_controls: false,
            ctrl_scroll: 0.0,
            ctrl_max: 0.0,
            diff_preview: None,
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
            instrument: qrs_analysis::instrument::Instrument::Guitar,
            inst_hinted: false,
            tuner_text: String::new(),
            retune_hidden: false,
            tuner_at: Instant::now(),
            zoom_at: None,
            zoom_what: ZoomWhat::Zoom,
            tab_full: st.config.bool("tab_full", false),
            chord_numerals: st.config.bool("chord_numerals", false),
            art: None,
            gem_glow: transport_art::Glow::default(),
            art_at: None,
            highway: highway::Highway::new(
                st.config.bool("hw_invert", false),
                st.config.f64("hw_speed", highway::SPEED_DEFAULT as f64).max(0.0) as usize,
                st.config
                    .f64("hw_camera", highway::CAMERA_DEFAULT as f64)
                    .max(0.0) as usize,
                highway::Backdrop::parse(&st.config.str("hw_backdrop", "live")),
            )
            .with_size(highway::size_from(&st.config)),
            hw_on: st.config.bool("view_highway", false),
            tools_rect: Rect::NOTHING,
            tip: None,
            sec_menu: None,
            sec_jump: None,
            sec_label: None,
            sec_panel: None,
            pending_speed: None,
            speed_at: Instant::now(),
            stretch: None,
            mixer: songaudio::Mixer::new(stems, &saved_audio),
            song_synth: songsynth::SongSynth::from_config(&st.config),
            nota,
            auto_level: st.config.bool("auto_level", true),
            run_scored: true,
            recorded: false,
            new_best: false,
            summary_dismissed: false,
            review: None,
            runs: Vec::new(),
            runs_for: String::new(),
            restart_rect: Rect::NOTHING,
            review_rect: Rect::NOTHING,
            review_menu: false,
            reviews_path: st.data.root.join("reviews.json"),
            continuous: false,
            arcade: Default::default(),
            pass_hist: Default::default(),
            result_scored: false,
            result_rank: None,
            tile_drag: None,
            tile_rects: Default::default(),
            tile_h: Default::default(),
            back_rect: Rect::NOTHING,
            countdown_hold: None,
            shot_stretch: None,
            shot_runs: false,
            shot_streak: None,
            shot_arr: None,
            streak_fx: Default::default(),
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
            rack: board::Rack::new(None),
        };
        p.relink_glyphs();
        if let Some(e) = p.transport.engine() {
            e.set_input_transpose(p.mixer.pitch as f32);
            let _ = e.take_onsets(); // plucks before the song started don't count
        }
        if p.mixer.pitch != 0 {
            // the guitar input's saved pitch shift; the backing track stays at its original pitch
            p.scorer.transpose(-p.mixer.pitch);
        }
        p.count_in(); // (a song opens counting in, or playing at once with the count-in off)
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
        self.toast_keys = self.pressed.take().unwrap_or_default();
        self.toast = Some((
            text.into(),
            tail.into(),
            Instant::now() + Duration::from_secs_f32(secs),
        ));
    }

    /// Chart time at the now-line (what's heard: the scorer, seeking and loops go by it).
    pub fn now(&self) -> f64 {
        self.transport.position() - self.offset
    }

    /// Chart time the picture shows at the now-line: `now` plus the screen's lag (`display_offset`, P36.1), so on a
    /// slow screen a note reaches the line as it's heard.
    pub fn draw_now(&self) -> f64 {
        crate::timing_cal::draw_now(self.transport.position(), self.offset, self.display_offset)
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
        // (a count-in running goes on from the new place: rules.rs)
        self.sec_jump = None;
        self.loop_prev = None;
        self.take_listen_stop(); // (listening back is over the take's own place)
        let time = time.clamp(0.0, self.transport.duration());
        self.transport.seek(time);
        self.nbn_release(true); // waiting at a note: play on from there
        self.tab.follow = true; // the cursor jumped: bring the tab page to it
        self.scorer.reset_from(time - self.offset);
        if time - self.offset < self.chart.first_note - 0.5 {
            // back to the start: a fresh run
            self.restart_counters();
            self.run_scored = self.at_max_settings();
        }
    }

    fn restart_counters(&mut self) {
        self.scorer = self.new_scorer();
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

    /// A high score only counts at full difficulty and full speed (and not note by note).
    /// The highway's streak readout (D144): the score tile's streak and heat (the same `streak_fx`, so the two agree
    /// and cool together), the multiplier only when the run is scored.
    fn streak_hud(&mut self, ctx: &egui::Context) -> highway::Hud {
        let streak = self.shot_streak.unwrap_or(self.scorer.streak);
        let clock = crate::shotclock::shot_now().unwrap_or_else(|| ctx.input(|i| i.time));
        if self.shot_streak.is_some() {
            self.streak_fx.snap(streak);
        }
        let heat = self.streak_fx.update(clock, streak);
        let scored = self.run_scored && self.at_max_settings() && self.has_input();
        highway::Hud {
            streak,
            mult: if scored {
                qrs_analysis::detect::multiplier(streak)
            } else {
                0
            },
            heat: if self.has_input() { heat } else { 0.0 },
        }
    }

    fn at_max_settings(&self) -> bool {
        self.mastery >= 0.999 && (self.speed() - 1.0).abs() < 1e-6 && !self.nbn
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
        self.nbn_release(true); // the note waited at was the old chart's
        let started = self.started_playing();
        self.chart = Chart::new(arr, self.transport.duration(), &self.nota);
        self.revealed = vec![false; self.chart.glyphs.len()];
        if let Some(c) = self.coach.as_mut() {
            // the item's notes as this part has them; the ones found stay found
            c.refit(&self.chart);
            let found: Vec<(f64, i32)> = c.phrase[..c.progress.found as usize]
                .iter()
                .map(|n| (n.t, n.string))
                .collect();
            for (t_, s_) in found {
                self.reveal_note(t_, s_);
            }
        }
        self.scorer = self.new_scorer();
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
        self.set_speed_to(new);
    }

    /// The speed straight to `new` (one of `SPEEDS`: the controls' presets).
    fn set_speed_to(&mut self, new: f64) {
        if self.engine().is_none() {
            self.say("Speed control needs the audio engine", "");
            return;
        }
        // applied after a short pause, so tapping three times builds one version
        self.pending_speed = Some(new);
        self.speed_at = Instant::now();
        self.say(format!("Speed {}%", (new * 100.0).round() as i64), "");
    }

    fn apply_speed(&mut self, speed: f64) {
        // (with the guitar input's pitch adjustment, if there is one)
        self.restretch(speed, true);
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
        self.range_follows_section(i);
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
        self.nbn_release(false); // the song stays stopped under the selection
        self.tab.follow = true; // the page goes with it, as with the song
        let speed = self.selection_speed();
        if self.sel_track && self.track_ok() {
            // the song itself: any passage, notes or not
            self.countdown_until = None;
            if self.transport.playing() {
                self.transport.pause();
            }
            if let Some(song) = self.transport.song().cloned() {
                self.synth
                    .start_track(song, a, b, self.offset, speed, 0.0, self.sel_loop);
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
        let opens = self.sounding_opens();
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

    // -- saved licks ------------------------------------------------------------------------------------------

    /// The stretch a lick would be saved from: the selection ("sel"), the loop ("loop"), or (None) the selection
    /// if there is one, else the loop.
    fn lick_range(&self, from: Option<&str>) -> Option<(f64, f64)> {
        let sel = self.tab.sel.filter(|(a, b)| b > a);
        let lp = self.loop_a.zip(self.loop_b).filter(|(a, b)| b > a);
        match from {
            Some("sel") => sel,
            Some("loop") => lp,
            _ => sel.or(lp),
        }
    }

    /// The lick for a stretch of this part: the song, the part, and the sections and bars it falls in.
    pub fn lick_for(&self, a: f64, b: f64) -> qrs_library::Lick {
        let mut sections: Vec<String> = Vec::new();
        for s in &self.chart.sections {
            if s.start < b - 0.01 && s.end > a + 0.01 {
                let label = section_label(s);
                if sections.last() != Some(&label) {
                    sections.push(label);
                }
            }
        }
        let ms = TabView::measures_of(&self.chart);
        let bar = |t: f64| {
            ms.iter()
                .rev()
                .find(|m| m.start <= t)
                .or(ms.first())
                .map(|m| m.number)
        };
        let bars = bar(a + 1e-3).zip(bar(b - 1e-3)).map(|(x, y)| (x, y.max(x)));
        qrs_library::Lick {
            song: self.sid.clone(),
            canonical: String::new(), // (the row's: see save_lick)
            part: self.part_name.clone(),
            part_key: self.part_key.clone(),
            start: a,
            end: b,
            title: self.entry.display_title(),
            artist: self.entry.artist.clone(),
            tuning: self.chart.arr.tuning_name(),
            name: qrs_library::Lick::auto_name(&sections, bars, a),
            sections,
            bars,
            saved: 0.0,
            other: Default::default(),
        }
    }

    /// Save the selection or the loop as a lick (see `lick_range`).
    fn save_lick(&mut self, st: &mut AppState, from: Option<&str>) {
        let Some((a, b)) = self.lick_range(from) else {
            self.say_for(
                "Nothing to save: ",
                "select bars (Ctrl-drag over the tab) or set a loop with [ and ]",
                3.0,
            );
            return;
        };
        let mut lick = self.lick_for(a, b);
        lick.canonical = st
            .row_of(&self.entry)
            .map(|g| st.groups[g].canonical.clone())
            .unwrap_or_default();
        let name = lick.name.clone();
        st.userdata.add_lick(lick);
        self.say_for("Saved lick: ", name, 2.5);
    }

    /// Open on a saved lick: its stretch looped, the playhead at its start (the count-in still runs).
    pub fn set_lick_loop(&mut self, a: f64, b: f64, name: &str) {
        self.seek(a);
        self.set_range(a, b, rules::RangeFrom::Lick);
        self.say_for(
            "Lick: ",
            format!("{name}  ·  looping {} – {}", fmt_time(a), fmt_time(b)),
            3.0,
        );
    }

    // -- ear mode (P13.1) -------------------------------------------------------------------------------------

    /// Hide the tab's notes, or show them again. Notes already found stay found while the part is open.
    pub fn set_ear(&mut self, on: bool) {
        self.ear = on;
        if self.revealed.len() != self.chart.glyphs.len() {
            self.revealed = vec![false; self.chart.glyphs.len()];
        }
    }

    /// Show the note at chart time `t` on `string` (one found in a lesson).
    pub fn reveal_note(&mut self, t: f64, string: i32) {
        let i0 = self.chart.gtimes.partition_point(|&x| x < t - 0.002);
        for i in i0..self.chart.glyphs.len() {
            let gl = &self.chart.glyphs[i];
            if gl.t > t + 0.002 {
                break;
            }
            if gl.string == string
                && let Some(shown) = self.revealed.get_mut(i)
            {
                *shown = true;
            }
        }
    }

    /// Ear mode: a note judged as played (in time or not) shows from then on.
    fn reveal_played(&mut self) {
        if !self.ear {
            return;
        }
        for (gl, shown) in self.chart.glyphs.iter().zip(self.revealed.iter_mut()) {
            if !*shown
                && let Some(g) = gl.group
                && matches!(
                    self.states.get(g),
                    Some(Some(Judgement::Hit | Judgement::Late | Judgement::Early))
                )
            {
                *shown = true;
            }
        }
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
        self.countdown_on = st.config.bool("countdown", true);
        self.auto_level = st.config.bool("auto_level", true);
        self.offset = st.config.f64("av_offset", 0.0);
        self.input_offset = st.config.f64("input_offset", 0.0);
        self.highway.backdrop = highway::Backdrop::parse(&st.config.str("hw_backdrop", "live"));
        self.highway.size = highway::size_from(&st.config);
        self.highway.colours = highway::NoteColours::parse(&st.config.str("hw_colours", "vivid"));
        self.highway.gem = highway::gem_from(&st.config);
        self.display_offset = st.config.f64("display_offset", 0.0);
        let old = self.nota;
        self.nota = Nota::from_config(st);
        if old.rests != self.nota.rests || old.triplets != self.nota.triplets || old.snap != self.nota.snap {
            let nota = self.nota;
            self.chart.build_rhythm(&nota);
        }
    }

    /// Skip the count-in and play (--play).
    /// Open paused at the start (no count-in); the first Play counts in.
    pub fn open_paused(&mut self) {
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause(); // (opened with the count-in off: it had started)
        }
        self.fresh = true;
    }

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
        self.nbn_release(false);
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
        if name.starts_with("review") && self.review_act(name) {
            return;
        }
        // the tuner's tuning card: the guitar input shift to apply
        if let Some(to) = name.strip_prefix("pitch_to:").and_then(|v| v.parse::<i32>().ok()) {
            if to == 0 {
                self.change_pitch(st, 0);
            } else if to != self.mixer.pitch {
                self.change_pitch(st, to - self.mixer.pitch);
            }
            return;
        }
        match name {
            "library" => self.reqs.push(PlayerReq::Library),
            // the results card closed: the song page as it was, stopped at the end (a recording keeps running)
            "summary_close" => {
                self.summary_dismissed = true;
                self.backdrop.clear();
            }
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
                if let Some(c) = self.coach.as_mut() {
                    c.show = false; // (the console's controls, not the coach)
                }
                // (the header's SPEED knob: the Practise tab)
                if controls::ctrl_tab(&st.config) != controls::CtrlTab::Practise {
                    controls::set_ctrl_tab(&mut st.config, controls::CtrlTab::Practise);
                    self.ctrl_scroll = 0.0;
                }
            }
            "settings:practice" => self.reqs.push(PlayerReq::Settings("practice")),
            "settings:display" => self.reqs.push(PlayerReq::Settings("display")),
            "sel_play" => self.play_selection(true),
            "save_lick" => self.save_lick(st, None),
            "lick_sel" => self.save_lick(st, Some("sel")),
            "lick_loop" => self.save_lick(st, Some("loop")),
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
                // (the selection is the practice range: both go)
                self.stop_synth();
                if self.range_from == Some(rules::RangeFrom::Selection) {
                    self.clear_range();
                } else {
                    self.tab.sel = None;
                }
            }
            "play" => {
                self.stop_synth();
                if std::mem::take(&mut self.fresh) && !self.transport.playing() && self.waiting.is_none() {
                    self.count_in();
                } else if self.countdown_until.is_some() {
                    self.countdown_until = None;
                    self.transport.play();
                } else if self.transport.finished() {
                    self.seek(0.0); // play again: a fresh run from the top
                    self.transport.play();
                } else if self.waiting.is_some() {
                    self.nbn_release(false); // a pause (the song has already stopped)
                } else {
                    self.transport.toggle();
                }
            }
            "shortcuts" => {
                self.shortcuts = !self.shortcuts;
                self.pressed = None;
            }
            "nbn" => self.set_note_by_note(!self.nbn),
            "nbn:skip" => self.nbn_skip(),
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
            "start" => self.restart(),
            "countdown" => self.toggle_countdown(st),
            "zoom_in" => {
                self.tab.zoom = (self.tab.zoom * 1.15).min(2.5);
                self.zoom_at = Some(Instant::now());
                self.zoom_what = ZoomWhat::Zoom;
            }
            "zoom_out" => {
                self.tab.zoom = (self.tab.zoom / 1.15).max(0.5);
                self.zoom_at = Some(Instant::now());
                self.zoom_what = ZoomWhat::Zoom;
            }
            "tab_wider" | "tab_narrower" | "tab_taller" | "tab_shorter" => {
                let k = if matches!(name, "tab_wider" | "tab_taller") {
                    1.1
                } else {
                    1.0 / 1.1
                };
                if matches!(name, "tab_wider" | "tab_narrower") {
                    self.tab.width_k = (self.tab.width_k * k).clamp(WIDTH_K.0, WIDTH_K.1);
                    st.config.set("tab_width", self.tab.width_k as f64);
                    self.zoom_what = ZoomWhat::Width;
                } else {
                    self.tab.height_k = (self.tab.height_k * k).clamp(HEIGHT_K.0, HEIGHT_K.1);
                    st.config.set("tab_height", self.tab.height_k as f64);
                    self.zoom_what = ZoomWhat::Height;
                }
                self.zoom_at = Some(Instant::now());
            }
            "zoom_reset" => {
                self.tab.zoom = 1.0;
                self.zoom_at = Some(Instant::now());
                self.zoom_what = ZoomWhat::Zoom;
            }
            "tab_width_reset" | "tab_height_reset" => {
                if name == "tab_width_reset" {
                    self.tab.width_k = 1.0;
                    st.config.set("tab_width", 1.0);
                    self.zoom_what = ZoomWhat::Width;
                } else {
                    self.tab.height_k = 1.0;
                    st.config.set("tab_height", 1.0);
                    self.zoom_what = ZoomWhat::Height;
                }
                self.zoom_at = Some(Instant::now());
            }
            "tab_shape_reset" => {
                self.tab.width_k = 1.0;
                self.tab.height_k = 1.0;
                st.config.set("tab_width", 1.0);
                st.config.set("tab_height", 1.0);
                self.say("Tab: bar width and string spacing reset", "");
            }
            "tab_compact" => {
                self.tab.compact = !self.tab.compact;
                st.config.set("tab_compact", self.tab.compact);
                self.tab.jump = true;
                self.say(
                    format!("Compact rows: {}", if self.tab.compact { "on" } else { "off" }),
                    "C",
                );
            }
            "view" => {
                self.hw_on = !self.hw_on;
                st.config.set("view_highway", self.hw_on);
                self.tab.jump = true;
                if self.hw_on {
                    let moves = self.highway.moves(&self.chart);
                    self.say(
                        format!("Highway: the camera moves {moves} times in this part"),
                        "V",
                    );
                } else {
                    self.say("Tab", "V");
                }
            }
            "hw_speed_reset" | "hw_camera_reset" => {
                let camera = name == "hw_camera_reset";
                self.highway.reset(camera);
                if camera {
                    st.config.set("hw_camera", self.highway.camera as f64);
                    self.say(format!("Camera: {}", highway::CAMERAS[self.highway.camera]), "");
                } else {
                    st.config.set("hw_speed", self.highway.speed as f64);
                    self.say(format!("Note speed {}%", self.highway.speed_pct()), "");
                }
            }
            "hw_smaller" | "hw_bigger" | "hw_size_reset" => {
                if name == "hw_size_reset" {
                    self.highway.reset_size();
                } else {
                    self.highway.step_size(if name == "hw_bigger" { 1 } else { -1 });
                }
                st.config
                    .set("hw_size", highway::size_pct(self.highway.size) as f64);
                let what = if self.highway.size == highway::SIZE_DEFAULT {
                    " (RS's)"
                } else {
                    ""
                };
                self.say(
                    format!("Note size {}{what}", highway::size_label(self.highway.size)),
                    "",
                );
            }
            "hw_slower" | "hw_faster" => {
                if self.highway.step_speed(if name == "hw_faster" { 1 } else { -1 }) {
                    st.config.set("hw_speed", self.highway.speed as f64);
                }
                self.say(format!("Note speed {}%", self.highway.speed_pct()), "");
            }
            "hw_steadier" | "hw_livelier" => {
                if self
                    .highway
                    .step_camera(if name == "hw_livelier" { 1 } else { -1 })
                {
                    st.config.set("hw_camera", self.highway.camera as f64);
                }
                let moves = self.highway.moves(&self.chart);
                self.say(
                    format!(
                        "Camera: {} ({moves} moves in this part)",
                        highway::CAMERAS[self.highway.camera]
                    ),
                    "",
                );
            }
            "hw_invert" => {
                self.highway.invert = !self.highway.invert;
                st.config.set("hw_invert", self.highway.invert);
                self.say(
                    if self.highway.invert {
                        "Strings inverted: low E at the bottom"
                    } else {
                        "Low E on top"
                    },
                    "I",
                );
            }
            "hw_backdrop" => {
                let b = self.highway.cycle_backdrop();
                st.config.set("hw_backdrop", b.key());
                self.say(
                    match b {
                        highway::Backdrop::Live => "Background: Live",
                        highway::Backdrop::Still => "Background: Still (one wallpaper)",
                        highway::Backdrop::Plain => "Background: Plain (dark)",
                    },
                    "D",
                );
            }
            "tab_full" => {
                self.tab_full = !self.tab_full;
                st.config.set("tab_full", self.tab_full);
                self.tab.jump = true;
                if self.tab_full {
                    self.say(
                        if self.hw_on {
                            "Full screen"
                        } else {
                            "Full-screen tab"
                        },
                        "F",
                    );
                }
            }
            "chord_numerals" => {
                self.chord_numerals = !self.chord_numerals;
                st.config.set("chord_numerals", self.chord_numerals);
                match (self.chord_numerals, self.chart.key) {
                    (false, _) => self.say("Chord names as charted", "K"),
                    (true, Some(key)) => self.say(
                        format!("Chords as numbers in {}", qrs_analysis::keyguess::key_label(key)),
                        "K",
                    ),
                    (true, None) => self.say("No key found for this part: chord names as charted", "K"),
                }
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
                self.tab.sel = None;
                self.range_from = Some(rules::RangeFrom::Marks);
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
                    self.set_range(a, end, rules::RangeFrom::Marks);
                    self.say(format!("Looping {} – {}", fmt_time(a), fmt_time(end)), "");
                    self.seek(a - LOOP_LEAD_IN);
                }
            }
            "continuous" => self.toggle_continuous(),
            "clear_loop" => {
                self.clear_range();
                self.say("Loop cleared", "");
            }
            "loop_section" => {
                let i = self.section_here();
                if i >= 0 {
                    let sec = self.chart.sections[i as usize].clone();
                    self.set_range(sec.start, sec.end, rules::RangeFrom::Section(i as usize));
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
            "ear" => {
                self.set_ear(!self.ear);
                if self.ear {
                    self.say(
                        "Ear mode: ",
                        "the notes are hidden until you play them  ·  H shows them",
                    );
                } else {
                    self.say("Ear mode off", "");
                }
            }
            "lyrics" => {
                // Y: the lyrics tile folds or opens (a tile is never hidden, P35.2)
                let open = !tiles::tile_open(st, "lyrics");
                tiles::set_tile_collapsed(st, "lyrics", !open);
                let extra = if open && self.lyrics.is_empty() {
                    "  ·  this song has no lyrics"
                } else {
                    ""
                };
                self.say(
                    format!("Lyrics tile {}{extra}", if open { "open" } else { "folded" }),
                    "",
                );
            }
            "slower" => self.change_speed(-1),
            "faster" => self.change_speed(1),
            n if n.starts_with("gtime_set:") => {
                // (the results card's offer: Your hits set outright)
                if let Ok(v) = n["gtime_set:".len()..].parse::<f64>() {
                    self.input_offset = v.clamp(-0.25, 0.25);
                    st.config.set("input_offset", self.input_offset);
                    st.save_config();
                    self.say(
                        format!(
                            "{} delay {}",
                            st.instrument().noun_cap(),
                            crate::settings::delay_label(self.input_offset)
                        ),
                        "",
                    );
                }
            }
            "gtime-" | "gtime+" | "gtime-1" | "gtime+1" => {
                // (Ctrl+. a longer delay: the notes count earlier, `input_offset` down; 5 ms as everywhere, D132)
                let step = if name.ends_with('1') {
                    0.001
                } else {
                    crate::settings::NUDGE_MS as f64 / 1000.0
                };
                self.input_offset += if name.starts_with("gtime+") { -step } else { step };
                st.config
                    .set("input_offset", (self.input_offset * 1000.0).round() / 1000.0);
                self.say(
                    format!(
                        "{} delay {}",
                        st.instrument().noun_cap(),
                        crate::settings::delay_label(self.input_offset)
                    ),
                    "",
                );
            }
            "sync-" | "sync+" | "sync-1" | "sync+1" => {
                let step = if name.ends_with('1') {
                    0.001
                } else {
                    crate::settings::NUDGE_MS as f64 / 1000.0
                };
                self.offset += if name.starts_with("sync+") { step } else { -step };
                st.config
                    .set("av_offset", (self.offset * 1000.0).round() / 1000.0);
                self.say(format!("Sync offset {:+.0} ms", self.offset * 1000.0), "");
            }
            "song_synth" => self.toggle_song_synth(st),
            "monitor" => {
                if let Some(e) = self.engine().cloned() {
                    let on = !e.monitor();
                    e.set_monitor(on);
                    st.config.set(st.monitor_key().0, on);
                    let inst = st.instrument();
                    self.say(
                        if on {
                            format!("Hearing your {}", inst.noun())
                        } else {
                            format!("{} sound off", inst.noun_cap())
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
                let spec = crate::tones::spec_for(&st.config, next);
                crate::tones::store(&mut st.config, &spec);
                if let Some(e) = self.engine() {
                    e.set_tone(spec);
                }
                self.say(format!("Tone: {}", crate::tones::label(next)), "");
            }
            "sounds" => {
                // your saved sounds on the Pedalboard tile (it steps aside on an amp modeller, D33)
                if !crate::calibrate::board_tile_wanted(st) {
                    self.say(
                        "Your amp modeller makes the tone",
                        "  \u{b7}  your sounds play on the rig",
                    );
                } else {
                    if tiles::tile_collapsed(st, "board") {
                        tiles::set_tile_collapsed(st, "board", false);
                    }
                    self.show_controls = false;
                    self.rack.sounds_toggle();
                }
            }
            "gvol-" | "gvol+" => {
                if let Some(e) = self.engine().cloned() {
                    let v = (e.guitar_gain() + if name == "gvol+" { 0.1 } else { -0.1 }).clamp(0.0, 2.0);
                    self.set_volume(st, "gvol", v);
                    self.say(
                        format!(
                            "{} volume {}%",
                            st.instrument().noun_cap(),
                            (v * 100.0).round() as i64
                        ),
                        "",
                    );
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
            "pitch+" => self.change_pitch(st, 1),
            "pitch-" => self.change_pitch(st, -1),
            "pitch0" => self.change_pitch(st, 0),
            _ => {
                if let Some(what) = name.strip_prefix("stem:") {
                    self.stem_action(st, what);
                } else if let Some(what) = name.strip_prefix("coach:") {
                    self.coach_act(st, what);
                } else if let Some(what) = name.strip_prefix("take:") {
                    self.take_act(what);
                } else if let Some(what) = name.strip_prefix("rec:") {
                    // R: record what you hear, pause, resume; Shift R or a click on the badge: stop and save (P23.9)
                    let said = match what {
                        "stop" => crate::recording::finish(st),
                        _ => crate::recording::press(
                            st,
                            crate::recording::Page::Player,
                            &self.entry.display_title(),
                        ),
                    };
                    if let Some((a, b)) = said {
                        self.say_for(a, b, 2.5);
                    }
                } else if let Some(rest) = name.strip_prefix("setvol:") {
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
                } else if let Some(act) = controls::fastout_act(name) {
                    // (the same as Settings' switch: the output reopens, the song keeps its place and plays on)
                    match crate::settings::fastout::apply(st, act) {
                        Ok(m) | Err(m) => self.say(m, ""),
                    }
                } else if let Some(tab) = name
                    .strip_prefix("ctrl_tab:")
                    .and_then(controls::CtrlTab::from_key)
                {
                    controls::set_ctrl_tab(&mut st.config, tab);
                    self.ctrl_scroll = 0.0;
                } else if let Some(v) = name
                    .strip_prefix("speed_set:")
                    .and_then(|v| v.parse::<f64>().ok())
                {
                    self.set_speed_to(controls::snap_speed(v));
                } else if let Some(v) = name.strip_prefix("diff_set:").and_then(|v| v.parse::<f64>().ok()) {
                    let step = controls::diff_step(self.mastery, v);
                    if step.abs() > 1e-9 {
                        self.change_mastery(st, step);
                    }
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
            // (5 ms a press, with Shift 1 ms: P36.6, P37.6a)
            let name = if ctrl {
                if up { "gtime+" } else { "gtime-" }
            } else if up {
                "sync+"
            } else {
                "sync-"
            };
            return Some(if shift { format!("{name}1") } else { name.into() });
        }
        if matches!(key, Key::OpenBracket | Key::CloseBracket) && ctrl {
            return Some("clear_loop".into());
        }
        if key == Key::S && ctrl {
            return Some("save_lick".into());
        }
        if key == Key::Enter && !ctrl && self.waiting.is_some() {
            return Some("nbn:skip".into());
        }
        if let Some(c) = self.coach.as_ref() {
            // the coach's keys: on, hear it again, a hint, the tuner; playing along, the speed keys step through
            // the item's speeds
            if matches!(
                c.step,
                coach::Step::Play | coach::Step::Practise | coach::Step::Record
            ) && matches!(key, Key::Equals | Key::Plus)
            {
                return Some(if ctrl { "coach:slower" } else { "coach:faster" }.into());
            }
            if !ctrl {
                match key {
                    Key::Enter => return Some("coach:next".into()),
                    // (a theory page or a take: back a page, back to Practise)
                    Key::Backspace if matches!(c.step, coach::Step::Page(_) | coach::Step::Record) => {
                        return Some("coach:back".into());
                    }
                    Key::R => return Some("coach:hear".into()),
                    Key::Slash | Key::Questionmark => return Some("coach:hint".into()),
                    Key::U => return Some("coach:tuner".into()),
                    _ => {}
                }
            }
        }
        if key == Key::F1 || (matches!(key, Key::Questionmark | Key::Slash) && self.coach.is_none()) {
            return Some("shortcuts".into());
        }
        // R: record what you hear, pause, resume (Shift R stops and saves); Ctrl R: a take of the loop, scored (D30)
        if key == Key::R && self.coach.is_none() {
            return Some(r_key(ctrl, shift).into());
        }
        if !ctrl && key == Key::B {
            return Some("take:listen".into());
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
        if key == Key::T && shift && !ctrl {
            return Some("sounds".into());
        }
        if let Some(d) = crate::library::digit(key)
            && (1..=6).contains(&d)
            && !ctrl
        {
            return Some(format!("partn:{}", d - 1));
        }
        if matches!(key, Key::ArrowUp | Key::ArrowDown) && shift && !m.alt {
            // the guitar input's pitch, a semitone at a time (P20.3)
            return Some(if key == Key::ArrowUp { "pitch+" } else { "pitch-" }.into());
        }
        if m.alt {
            // the tab's shape: Alt+Left/Right the bars' width, Alt+Up/Down the strings' spacing, Alt+0 both back
            let a = match key {
                Key::ArrowRight => Some("tab_wider"),
                Key::ArrowLeft => Some("tab_narrower"),
                Key::ArrowUp => Some("tab_taller"),
                Key::ArrowDown => Some("tab_shorter"),
                Key::Num0 => Some("tab_shape_reset"),
                _ => None,
            };
            if let Some(a) = a {
                return Some(a.into());
            }
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
            Key::Escape if self.tab_full => "tab_full",
            Key::Escape | Key::Backspace => "library",
            Key::F if !ctrl => "tab_full",
            Key::V if !ctrl => "view",
            Key::I if !ctrl && self.hw_on => "hw_invert",
            Key::D if !ctrl && self.hw_on => "hw_backdrop",
            Key::C if shift && !ctrl => "countdown",
            Key::C if !ctrl => "tab_compact",
            Key::K if !ctrl => "chord_numerals",
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
            Key::H => "ear",
            Key::E if !ctrl => "review",
            Key::O if !ctrl => "continuous",
            Key::W => "nbn",
            Key::N => "nota:show",
            Key::M => "monitor",
            Key::G if !ctrl => "song_synth",
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
        return self.mg_view.open || self.rack.board.open;
        #[cfg(not(feature = "nux"))]
        self.rack.board.open
    }

    /// Keys and the wheel.
    pub fn handle(&mut self, st: &mut AppState, ev: &Event) {
        if self.board_key(st, ev) {
            return;
        }
        if let Event::MouseWheel {
            unit,
            delta,
            modifiers,
            ..
        } = ev
        {
            let notches = crate::library::wheel_notches(*unit, delta.y);
            let pos = st.ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
            if self.board_wheel(st, pos, notches, modifiers.shift) || self.rack.board.open {
                return;
            }
        }
        if self.rack.board.open {
            // the big view is modal: Esc closes it; Space still plays and pauses
            match ev {
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => {
                    self.rack.board.open = false;
                    self.rack.board.picker = None;
                }
                Event::Key {
                    key: Key::Space,
                    pressed: true,
                    ..
                } => {}
                _ => return,
            }
            if !matches!(ev, Event::Key { key: Key::Space, .. }) {
                return;
            }
        }
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
        if self.review_event(ev) {
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
                if self.shortcuts && matches!(key, Key::Escape) {
                    self.shortcuts = false;
                    return;
                }
                if let Some(a) = summary_key(*key, self.summary_up()) {
                    // (the card's own Esc, before Esc's "back to the library" can see it)
                    self.act(st, a);
                    return;
                }
                if let Some(name) = self.key_action(*key, *modifiers) {
                    // the toast this press makes flashes its keycaps (Space shows on the play key already)
                    if *key != Key::Space && !name.starts_with("coach:") {
                        self.pressed = Some(hud::key_labels(*key, *modifiers));
                    }
                    self.act(st, &name);
                    if self.pressed.is_some() {
                        self.flash_for(&name);
                    }
                    self.pressed = None;
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
        self.board_update(st);
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
            self.speed_applied();
            if self.started_playing() {
                self.run_scored = false;
            }
        }
        if self.stretch.as_ref().is_some_and(|j| j.done()) && self.engine().is_some_and(|e| !e.buffering()) {
            // keep the job (its buffer is the engine's) but stop showing progress
        }
        self.update_song_audio();
        self.update_song_synth();
        if self.countdown_until.is_some_and(|t0| Instant::now() >= t0) {
            self.countdown_until = None;
            self.transport.play();
        }
        let playing = self.transport.playing();
        let prev = self.loop_prev.take();
        if playing
            && let Some((a, _)) = self.range()
            && self.loop_due(prev, self.now())
        {
            self.loop_pass_done(st);
            if self.continuous {
                // straight on from the loop's start, as if the song went round
                self.seek(a + self.offset);
            } else {
                self.seek((a - LOOP_LEAD_IN).max(0.0));
            }
            self.countdown_until = None;
        }
        self.loop_prev = Some(self.now());
        if playing && self.transport.finished() {
            self.transport.pause(); // pausing steps back to the heard position: pin it to the end
            let d = self.transport.duration();
            self.transport.seek(d);
        }
        let ended_now = !self.transport.playing() && self.transport.finished() && !self.recorded;
        // (a closed results card comes back when a new run ends)
        self.summary_dismissed = keep_dismissed(self.summary_dismissed, self.transport.playing(), ended_now);
        if self.transport.playing() {
            self.recorded = false;
            self.new_best = false; // a new run: the last one's results card no longer applies
            self.result_rank = None;
        } else if ended_now {
            self.recorded = true;
            self.record_completion(st);
            if self.loop_a.zip(self.loop_b).is_none() {
                // (a run, not a loop reaching the end: saved with the last ones, its review ready for the card's
                // Review)
                self.record_review_run();
            }
        }
        // what the input plays (P32): a bass's notes are read as a bass's
        let inst = st.instrument();
        if inst != self.instrument || !self.inst_hinted {
            self.inst_hinted = true;
            instrument_hint(st, inst, self.chart.arr.is_bass);
        }
        self.instrument = inst;
        if self.scorer.instrument() != inst {
            self.scorer.set_instrument(inst);
        }
        // the guitar: plucks to the scorer, the pitch to the tuner
        let e = self.engine().cloned();
        if let Some(e) = &e {
            e.set_onset_bass(inst.is_bass());
        }
        if let Some(e) = e
            && e.status().input.is_some()
            && !crate::timing_cal::onsets_held()
        {
            let wall = Instant::now();
            let onsets = e.take_onsets();
            // the coach reads each pluck's note, playing or not
            self.coach_plucks(&onsets, e.capture().rate());
            self.coach_listen(e.capture());
            self.take_frame(&e, &onsets); // (a take's guitar off the ring)
            if self.transport.playing() {
                let (av, io) = (self.offset, self.input_offset);
                self.scorer.feed(onsets.iter().filter_map(|o| {
                    o.song_time.map(|st| Pluck {
                        index: o.index,
                        song_time: crate::timing_cal::chart_pluck(st, av, io),
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
            } else if self.waiting.is_some() {
                self.nbn_listen(&e, &onsets);
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
        self.nbn_watch();
        let before = std::mem::take(&mut self.states);
        self.states = self.scorer.groups.iter().map(|g| g.state).collect();
        self.arcade_notes(&before);
        self.reveal_played();
        self.coach_tick(st);
        self.take_tick(st);
    }

    /// Feed the tuner (and the note tile's "you:") from the guitar input.
    fn read_pitch(&mut self, e: &qrs_audio::Engine) {
        let level = e.meters().level;
        let rate = e.capture().rate();
        if level > 0.002 && e.capture().latest(&mut self.pitch_buf).is_some() {
            let (hz, clarity) =
                qrs_analysis::instrument::pitch_for(self.instrument, &mut self.sp, &self.pitch_buf, rate);
            if hz > 0.0 && clarity > 0.85 {
                self.tuner_hist
                    .push_back(69.0 + 12.0 * (hz / 440.0).log2() + self.mixer.pitch as f64);
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

    /// Riff Repeater-style: after a loop pass at 90%+ accuracy, add notes (raise difficulty 10%). In a lesson, the
    /// coach hears about every pass.
    fn loop_pass_done(&mut self, st: &mut AppState) {
        self.loop_passes += 1;
        let acc = self.loop_pass_accuracy();
        self.review_pass(acc);
        self.continuous_pass(acc);
        if !self.continuous && !self.take.as_ref().is_some_and(take::Session::going) {
            self.loop_pass_level(st, acc);
        }
        self.coach_pass(st, acc);
    }

    /// The loop pass's accuracy (hits, and half for late or early), None without notes in it or a guitar.
    fn loop_pass_accuracy(&mut self) -> Option<f64> {
        let (Some(a), Some(b)) = (self.loop_a, self.loop_b) else {
            return None;
        };
        // (a note at the loop's end still in its late window when the loop goes round isn't counted: it can't be
        // judged once the song is back at the start, and isn't yet missed)
        let now = self.now();
        let groups: Vec<&qrs_analysis::detect::NoteGroup> = self
            .scorer
            .groups
            .iter()
            .filter(|g| a <= g.t && g.t < b && (g.state.is_some() || g.t < now - qrs_analysis::detect::LATE))
            .collect();
        if groups.is_empty() || !self.has_input() {
            return None;
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
        Some(acc)
    }

    fn loop_pass_level(&mut self, st: &AppState, acc: Option<f64>) {
        let Some(acc) = acc else { return };
        if self.auto_level && !self.nbn && self.mastery < 1.0 && acc >= LEVEL_UP_ACCURACY {
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
    /// A screenshot sequence's frame (`shotclock`): the song held at `t` (song audio seconds), and with
    /// QUICKRS_SHOT_JUDGE (hit, late, early, miss) each note not yet judged judged so as it reaches the now-line, as
    /// if played (without it, notes pass unplayed and are missed as they would be).
    pub fn shot_seq_frame(&mut self, t: f64) {
        self.transport.hold(Some(t));
        let judge = match std::env::var("QUICKRS_SHOT_JUDGE").ok().as_deref().map(str::trim) {
            Some("hit") => Some(Judgement::Hit),
            Some("late") => Some(Judgement::Late),
            Some("early") => Some(Judgement::Early),
            Some("miss") => Some(Judgement::Miss),
            _ => None,
        };
        if let Some(j) = judge {
            let now = t - self.offset;
            for g in &mut self.scorer.groups {
                if g.state.is_none() && g.t <= now {
                    g.state = Some(j);
                }
            }
        }
    }

    pub fn prepare_shot(&mut self, st: &mut AppState, page: Option<crate::shots::Page>) {
        use crate::shots::Page;
        self.art_at = None; // (the gem's light as it ends, not easing from the shot before)
        // (QUICKRS_SHOT_AT: another moment of the song, in seconds; the 10 s before it are judged as below)
        let at: f64 = std::env::var("QUICKRS_SHOT_AT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| crate::shotclock::seq_env().and_then(|s| s.first().copied())) // (a sequence starts where it starts)
            .unwrap_or(61.0);
        // what an earlier shot staged goes
        self.loop_a = None;
        self.loop_b = None;
        self.range_from = None;
        self.coach = None;
        self.take = None;
        // (the review page and its history menu stayed up from 36_review_history into every player shot after it:
        // 13h-13l, 18, 19, 70-72 showed the review, P37.4)
        self.review = None;
        self.continuous = false;
        // (and the tab, not the highway an earlier shot switched to: 70_board_tile came out on the highway)
        self.hw_on = false;
        self.highway.invert = false;
        self.nbn = false;
        self.waiting = None;
        self.set_ear(false);
        self.sel_slow = 1.0;
        self.sel_track = false;
        self.tab.zoom = 1.0;
        self.tab.width_k = 1.0;
        self.tab.height_k = 1.0;
        self.tab.compact = false;
        self.tab_full = false;
        self.zoom_at = None;
        self.zoom_what = ZoomWhat::Zoom;
        self.sec_menu = None;
        self.pending_speed = None;
        self.shot_stretch = None;
        self.panel_scroll = 0.0;
        self.toast = None;
        self.toast_keys.clear();
        self.shortcuts = false;
        self.mixer.pitch = 0;
        if let Some(e) = self.engine() {
            e.set_input_transpose(0.0);
        }
        self.summary_dismissed = false;
        self.shot_streak = std::env::var("QUICKRS_SHOT_STREAK")
            .ok()
            .and_then(|v| v.parse().ok());
        self.input_offset = st.config.f64("input_offset", 0.0);
        if let Some(e) = self.engine() {
            e.set_monitor(true);
        }
        self.countdown_hold = None;
        self.countdown_on = st.config.bool("countdown", true);
        if self.audio_error == "its Vorbis stream ended early (a damaged file?)" {
            self.audio_error.clear();
        }
        self.countdown_until = None;
        self.show_controls = matches!(
            page,
            Some(Page::PlayerControls | Page::PlayerAt("controls_sound" | "controls_view"))
        );
        self.ctrl_scroll = 0.0; // (the shot's tab is staged in config: controls_tab)
        #[cfg(feature = "nux")]
        {
            self.mg_view.open = page == Some(Page::Mg300View);
        }
        let board_page = matches!(
            page,
            Some(Page::PlayerBoard | Page::BoardView | Page::PlayerAt("board_add" | "board_sounds"))
        );
        self.rack.board.open = page == Some(Page::BoardView);
        self.rack.board.picker =
            (page == Some(Page::PlayerAt("board_add"))).then_some(board::Picker::Kinds { at: 2 });
        if board_page {
            self.board_use(st);
        }
        self.tab.sel = None;
        self.tab.follow = true;
        self.tab.jump = true;
        // the highway's note size as configured (a shot below may change it), and the chart as loaded unless the shot
        // gives the notes ahead techniques (13m: bends, slides, mutes, harmonics, tremolo, slap / pop, taps)
        self.highway.size = highway::size_from(&st.config);
        let techs = match page {
            Some(Page::PlayerAt(w)) => w.strip_prefix("hwtech_"),
            _ => None,
        };
        self.shot_techs(techs, at - self.offset);
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
        self.transport.seek(at - 1.0);
        self.transport.hold(Some(at));
        let forced_to = if std::env::var("QUICKRS_SHOT_JUDGE").is_ok() {
            at + 0.02
        } else {
            at - 1.0
        };
        for g in &mut self.scorer.groups {
            if at - 11.0 < g.t && g.t < forced_to {
                // (QUICKRS_SHOT_JUDGE=hit|late|miss: all of them so, to check the highway's effects)
                let forced = std::env::var("QUICKRS_SHOT_JUDGE").ok();
                g.state = Some(
                    match forced.as_deref().map_or((g.t * 7.0) as i64 % 5, |f| match f {
                        "late" => 3,
                        "miss" => 4,
                        _ => 0,
                    }) {
                        3 => Judgement::Late,
                        4 => Judgement::Miss,
                        _ => Judgement::Hit,
                    },
                );
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
        // (the judgements above were set from outside the scorer: its streak and best streak follow them, or the results
        // card read "best streak 0" with 27 hits, P37.6d)
        self.scorer.recount_streak();
        if board_page {
            // (after that frame, which takes up the song's tone at 61 s: QUICKRS_BOARD, a starting board on "mine";
            // QUICKRS_BOARD_SEL, the block shown, by model id)
            self.rack.board.picker =
                (page == Some(Page::PlayerAt("board_add"))).then_some(board::Picker::Kinds { at: 2 });
            if let Ok(name) = std::env::var("QUICKRS_BOARD") {
                self.board_shot(
                    st,
                    Some(&name),
                    std::env::var("QUICKRS_BOARD_SEL").ok().as_deref(),
                );
            } else if std::env::var("QUICKRS_BOARD_SEL").is_ok()
                || std::env::var("QUICKRS_BOARD_CHAIN").is_ok()
            {
                // (QUICKRS_BOARD_CHAIN: a board of exactly these models, see `board_shot`)
                let sel = std::env::var("QUICKRS_BOARD_SEL").ok();
                self.board_shot(st, None, sel.as_deref());
            }
        }
        // (your sounds listed on the tile, one of them on: P23.3)
        self.board_sounds_shot(st, page == Some(Page::PlayerAt("board_sounds")));
        let now = at - self.offset;
        // the bar at now + dt: the tab's, else from the chart's bar lines (the tab lays its bars out when it's first
        // drawn, so a player shot taken first had none and lost its selection)
        let bar_after = |p: &Self, dt: f64| {
            p.tab.measure_at(now + dt).map(|m| (m.start, m.end)).or_else(|| {
                let starts: Vec<f64> = p
                    .chart
                    .beats
                    .iter()
                    .filter(|b| b.measure >= 0)
                    .map(|b| b.time)
                    .collect();
                let k = starts.partition_point(|&s| s <= now + dt);
                (k > 0 && k < starts.len()).then(|| (starts[k - 1], starts[k]))
            })
        };
        match page {
            Some(Page::PlayerSelection) | Some(Page::PlayerAt("sel_slow")) => {
                // a Ctrl-drag over the bar ahead: the selection is the loop (D119: one range). The shot used to set a
                // loop of its own as well (bars 28-35 shaded round bar 31 selected, the veteran's review, P37.6d)
                self.tab.sel = bar_after(self, 2.0);
                if let Some((a, b)) = self.tab.sel {
                    self.loop_a = Some(a);
                    self.loop_b = Some(b);
                    self.range_from = Some(rules::RangeFrom::Selection);
                }
                if page == Some(Page::PlayerAt("sel_slow")) {
                    self.sel_slow = 0.7;
                }
            }
            Some(Page::PlayerAt(what)) => match what {
                "tiles_scrolled" => self.panel_scroll = qrs_ui::units::pixels(200.0), // (Python: 200 px)
                "zoomed_out" => self.tab.zoom = 0.5,
                "tab_full" => {
                    self.tab_full = true;
                    self.tab.compact = true;
                    self.tab.jump = true;
                }
                // (the toolbar has its own row and no longer rolls up, D135: "rolled" is the same shot)
                "tab_tools" | "tab_tools_hover" | "tab_tools_rolled" => self.tab.jump = true,
                "highway" | "highway_invert" => {
                    self.hw_on = true;
                    self.highway.invert = what == "highway_invert";
                }
                "highway_full" => {
                    self.hw_on = true;
                    self.tab_full = true;
                }
                // the note size (D143): further, and two steps closer
                w if w.starts_with("highway_size") => {
                    self.hw_on = true;
                    let pct: f64 = w["highway_size".len()..].parse().unwrap_or(100.0);
                    let off = |i: usize| (highway::SIZES[i] as f64 * 100.0 - pct).abs();
                    self.highway.size = (0..highway::SIZES.len())
                        .min_by(|&a, &b| off(a).total_cmp(&off(b)))
                        .unwrap_or(highway::SIZE_DEFAULT);
                }
                w if w.starts_with("hwtech_") => self.hw_on = true,
                "highway_streak" => {
                    self.hw_on = true;
                    self.shot_streak = Some(60);
                }
                "tab_compact" => {
                    self.tab.compact = true;
                    self.tab.jump = true;
                }
                // narrower bars, closer strings, the HUD showing the spacing
                "tab_shape" => {
                    self.tab.width_k = 0.75;
                    self.tab.height_k = 0.8;
                    self.zoom_what = ZoomWhat::Height;
                    self.zoom_at = Some(Instant::now());
                    self.tab.jump = true;
                }
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
                // the lamps' row (P37.11): a loop and ear mode lit, the count-in lamp on, or off
                "lamps" | "countin_off" => {
                    self.loop_a = Some(now - 4.0);
                    self.loop_b = Some(now + 14.0);
                    self.set_ear(true);
                    self.countdown_on = what == "lamps";
                }
                "indicators" => {
                    // everything the shortcuts light at once: muted, transposed, a loop, the lamps, a key's flash
                    if let Some(e) = self.engine() {
                        e.set_monitor(false);
                    }
                    self.mixer.pitch = 1;
                    if let Some(e) = self.engine() {
                        e.set_input_transpose(1.0);
                    }
                    self.scorer.transpose(-1);
                    self.input_offset = 0.02;
                    self.loop_a = Some(now - 4.0);
                    self.loop_b = Some(now + 14.0);
                    self.loop_passes = 3;
                    self.nbn = true;
                    self.set_ear(true);
                    self.pressed = Some(vec!["Shift".into(), "↑".into()]);
                    self.say_for("Input ", "+1 semitone", 30.0);
                }
                "shortcuts" => self.shortcuts = true,
                // the synth guitar's key on, and being prepared (P23.11)
                "song_synth" => self.song_synth.shot = Some(1.0),
                "song_synth_prep" => self.song_synth.shot = Some(0.4),
                "recording" | "rec_paused" | "rec_saved" => {
                    let dir = st.config.recordings_dir(&st.data.root);
                    let what = what.strip_prefix("rec_").unwrap_or(what);
                    st.rec.stage(&dir, what, &self.entry.display_title());
                }
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
                // the run's review (P29.1): a run with a few rough stretches, its page or a spot being practised
                "review" | "review_spot" | "review_continuous" | "review_history" => {
                    self.shot_song_end(st, "results");
                    self.stage_review(what);
                }
                // the footer's review key (P37.8): three saved runs and its menu up, or none (dimmed, its tip)
                "review_key" | "review_key_none" => {
                    self.transport.pause();
                    self.review = None;
                    self.runs = if what == "review_key" {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0.0, |d| d.as_secs_f64());
                        [(600.0, 0.93), (3600.0 * 20.0, 0.81), (3600.0 * 50.0, 0.57)]
                            .map(|(ago, acc)| history::SavedRun {
                                at: now - ago,
                                acc,
                                notes: Vec::new(),
                                gist: None,
                            })
                            .to_vec()
                    } else {
                        Vec::new()
                    };
                    self.runs_for = self.history_key();
                }
                "restart_hover" => self.transport.pause(),
                // the results card closed (its x or Esc, P23.12): the page under it, stopped at the end
                "results_closed" => {
                    self.shot_song_end(st, "results");
                    self.act(st, "summary_close");
                }
                // the header's transpose stepper moved: the guitar input shift and track tuning are shown
                "transpose2" => {
                    self.mixer.pitch = 2;
                    if let Some(e) = self.engine() {
                        e.set_input_transpose(2.0);
                    }
                    self.scorer.transpose(-2);
                }
                // the score tile on a streak of n (P37.7)
                // the score tile on a streak of n (P37.7), about three notes in four hit so the arc is long
                w if w.starts_with("streak_") => {
                    self.shot_streak = w["streak_".len()..].parse().ok();
                    let judged = self.scorer.groups.iter_mut().filter(|g| g.state.is_some());
                    for (k, g) in judged.enumerate() {
                        g.state = Some(if k % 4 == 3 {
                            Judgement::Miss
                        } else {
                            Judgement::Hit
                        });
                    }
                    self.scorer.recount_streak();
                }
                "ear" => self.set_ear(true),
                "nbn" | "nbn_wrong" => self.nbn_shot(what == "nbn_wrong"),
                "lesson" => {
                    // lesson 1's riff for this song, started as the Lessons page starts it
                    let title = self.entry.display_title();
                    let lesson = st.lessons.iter().find_map(|l| {
                        let r = l.riffs.iter().find(|r| r.is_song(&self.entry.artist, &title))?;
                        Some((l.clone(), r.id.clone()))
                    });
                    if let Some((l, r)) = lesson {
                        self.start_lesson(st, l, &r);
                    }
                }
                w if w.starts_with("coach_") => {
                    self.transport.pause();
                    self.stage_coach(st, &w["coach_".len()..]);
                }
                w if w.starts_with("take_") => {
                    self.transport.pause();
                    self.stage_take(w);
                }
                _ => {}
            },
            Some(Page::Results) => self.shot_song_end(st, "results"),
            Some(Page::Settings(_)) => self.transport.pause(), // opening the settings pauses, as in the Python app
            _ => {}
        }
    }

    /// Screenshot mode: the notes from just before `t` (chart seconds) to the highway's far end given one family of
    /// techniques, so a shot shows how the highway draws them ("bends", "slides", "mutes", "harmonics", "tremolo",
    /// "slap", "taps"); None puts the part back as loaded. The notes keep their times, strings and frets.
    fn shot_techs(&mut self, kind: Option<&str>, t: f64) {
        if kind.is_none() && self.shot_arr.is_none() {
            return;
        }
        let base = self.shot_arr.take().unwrap_or_else(|| self.chart.arr.clone());
        let mut arr = base.clone();
        if let Some(kind) = kind {
            techify(&mut arr, kind, t - 0.4, t + 3.2);
            self.shot_arr = Some(base);
        }
        self.chart = Chart::new(arr, self.transport.duration(), &self.nota);
        self.revealed = vec![false; self.chart.glyphs.len()];
        self.scorer = self.new_scorer();
        self.relink_glyphs();
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
                self.audio_error = "its Vorbis stream ended early (a damaged file?)".into();
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
            Page::PlayerAt("tab_tools_hover") => (self.tools_rect != Rect::NOTHING)
                .then(|| pos2(self.tools_rect.max.x - px(48.0), self.tools_rect.center().y)),
            Page::PlayerAt("back_tip") => (self.back_rect != Rect::NOTHING).then(|| self.back_rect.center()),
            Page::PlayerAt("restart_hover") => {
                (self.restart_rect != Rect::NOTHING).then(|| self.restart_rect.center())
            }
            Page::PlayerAt("review_key" | "review_key_none") => (self.review_rect != Rect::NOTHING)
                .then(|| pos2(self.review_rect.max.x - px(10.0), self.review_rect.center().y)),
            _ => None,
        }
    }

    pub fn close(&mut self) {
        let e = self.engine().cloned();
        self.synth.stop_maybe(e.as_deref());
        if let Some(job) = self.stretch.take() {
            job.cancel();
        }
        self.mixer.cancel();
        if let Some(e) = e {
            e.set_input_transpose(0.0);
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
        if self.overlay_up() {
            if self.backdrop.ready(&ctx) {
                // the song has ended (or is held under the review), so what's under the card doesn't change
                self.backdrop.draw(ui.painter(), area);
                if self.review_open() {
                    self.draw_review(ui, area);
                } else {
                    self.draw_summary(ui, st, area);
                }
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
        // (the tab and the highway draw ahead by the screen's lag; the clock and tiles show what's heard)
        let draw_now = self.draw_now();
        let (main, aside) = Self::layout(area);
        let foot = px(76.0);
        let well = if self.tab_full {
            // the full-screen tab: the whole window but a margin and the scrollbar's groove
            let pad = px(18.0);
            Rect::from_min_size(
                ipos(area.min.x + pad, area.min.y + pad),
                vec2(
                    snap(area.width() - 2.0 * pad - px(24.0)),
                    snap(area.height() - 2.0 * pad),
                ),
            )
        } else {
            let top = self.draw_header(ui, st, main);
            Rect::from_min_size(
                ipos(main.min.x, top + px(18.0)),
                vec2(
                    snap(main.width() - px(24.0)),
                    snap(main.max.y - foot - px(18.0) - top - px(18.0)),
                ),
            )
        };
        // the tab's toolbar on a row of its own over the tab (D135): nothing of the tab under it
        let (tools_row, well) = if !theme::classic() && !self.hw_on {
            let (row, tab) = Self::tools_row(well, px(10.0));
            (Some(row), tab)
        } else {
            (None, well)
        };
        let groove = Rect::from_min_size(
            ipos(well.max.x + px(14.0), well.min.y),
            vec2(px(10.0), well.height()),
        );
        self.well = well;
        let synth_t = self
            .engine()
            .and_then(|e| self.synth.position(e))
            .or_else(|| self.take_listen_pos());
        let numerals = self.chart.key.filter(|_| self.chord_numerals);
        let tctx = TabCtx {
            chart: &self.chart,
            states: &self.states,
            now: draw_now,
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
            ear: self.ear.then_some(self.revealed.as_slice()),
            held: self.nbn_tab(),
            numerals,
        };
        let tab_acts = if self.hw_on {
            self.highway.full = self.tab_full;
            self.highway.numerals = numerals;
            self.highway.hud = self.streak_hud(&ctx);
            let out = self.highway.ui(ui, &self.chart, &self.states, draw_now, well);
            self.acts.extend(out.acts.into_iter().map(String::from));
            if out.tip.is_some() {
                self.tip = out.tip;
            }
            Vec::new()
        } else {
            self.tab.ui(ui, &tctx, well, groove)
        };
        for a in tab_acts {
            self.tab_act(a);
        }
        self.draw_coach_well(ui, well); // (a theory page over the tab, or a lesson's callouts on it)
        if theme::classic() {
            self.draw_zoom_hud(ui, well); // (the Python app's look: no toolbar, the zoom's HUD)
        } else if let Some(row) = tools_row {
            self.draw_tab_tools(ui, row); // (the highway has its own keys: back to the tab, invert, full screen)
        }
        if !self.tab_full {
            self.draw_footer(
                ui,
                st,
                Rect::from_min_size(ipos(main.min.x, main.max.y - foot), vec2(main.width(), foot)),
                now,
            );
            self.draw_aside(ui, st, aside, now);
        }
        self.draw_states(ui, well);
        let strip = self.draw_review_strip(ui, well);
        let plate = self.draw_continuous(ui, well, strip.map(|r| r.min.y));
        // continuous mode's score panel, beside the strip (or the passes' plate)
        if let Some(r) = strip.or(plate) {
            self.draw_arcade(ui, r.max.x + px(10.0), r.max.y, area);
        }
        if self.sec_menu.is_some() {
            if self.pedal_view_open() || self.overlay_up() {
                self.sec_menu = None; // (it closes under the pedal's view and the results, as in the Python app)
            } else {
                self.draw_section_menu(ui);
            }
        }
        if self.rack.board.open {
            self.draw_board_view(ui, st, area);
        }
        if self.tab_full || self.overlay_up() {
            self.review_rect = Rect::NOTHING;
        }
        self.draw_review_menu(ui);
        if self.shortcuts {
            // every key, over the player (a click outside closes it)
            let card = hud::draw_shortcuts(ui.painter(), area);
            let resp = interact(ui, area, "shortcuts-sheet", Sense::click());
            if resp.clicked() && !resp.interact_pointer_pos().is_some_and(|p| card.contains(p)) {
                self.shortcuts = false;
            }
        } else if let Some((text, key, pos, anchor)) = self.tip.clone() {
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

    /// What the tab asked for: a click seeks, a Ctrl-drag selected bars (the practice range, rules.rs), a key.
    fn tab_act(&mut self, a: TabAct) {
        match a {
            TabAct::Seek(tm) => {
                let tm = tm + self.offset;
                self.seek(tm);
            }
            TabAct::Selected => {
                // a Ctrl-drag: the bars selected are the practice range (D119)
                self.stop_synth();
                if let Some((a, b)) = self.tab.sel.filter(|(a, b)| b > a) {
                    self.set_range(a, b, rules::RangeFrom::Selection);
                }
            }
            TabAct::Act(name) => self.acts.push(name.into()),
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
        // the chosen part is a state, not an action: on the glass look its name and light, no key (P34.5)
        seg.plain = true;
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
            slowed.then_some(hud::Lit::Amber),
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
        // the favourite star right after the title (owner request, 2026-11-02): room kept for it, so a long title
        // is cut short (…) before the star goes
        let star_r = px(12.0);
        let star_room = snap(2.0 * star_r + px(14.0));
        let title_max = snap(tw - star_room).max(pixels(40.0));
        let tr = txt(
            &p,
            &self.entry.display_title(),
            ipos(tx, main.min.y - px(4.0)),
            30.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(title_max),
        );
        // (P37.6b: a line cut short ("Here Without…") says itself in full on hover)
        let cut_tip = |ui: &Ui, r: Rect, full: &str, st: Style, max: f32, id: &str| {
            if text::width(&ctx, full, st) > max + 0.5 {
                interact(ui, r, ("hdr-cut", id), Sense::hover()).on_hover_text(full.trim());
            }
        };
        cut_tip(
            ui,
            tr,
            &self.entry.display_title(),
            Style::new(30.0, 800, th.text),
            title_max,
            "title",
        );
        {
            let fav = st.userdata.is_favorite(&self.ids);
            let c = ipos(tr.max.x + px(12.0) + star_r, mid_y(tr) + px(1.0));
            let hit = Rect::from_center_size(c, Vec2::splat(2.0 * star_r + px(10.0)));
            let resp = interact(ui, hit, "player-fav", Sense::click());
            crate::library::draw_star(&p, c, if resp.hovered() { star_r * 1.12 } else { star_r }, fav);
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                self.tip = Some((
                    if fav {
                        "In your favourites (click to remove)"
                    } else {
                        "Add to favourites"
                    }
                    .into(),
                    String::new(),
                    ipos(hit.min.x, hit.max.y + px(8.0)),
                    Align2::LEFT_TOP,
                ));
            }
            if resp.clicked() {
                let on = st.userdata.toggle_favorite(&self.ids);
                if let (Some(first), Some(grp)) = (self.ids.first(), st.groups.get(self.g)) {
                    let canon = grp.canonical.clone();
                    st.userdata.alias(&canon, first);
                }
                let title = self.entry.display_title();
                st.say(format!(
                    "{} {title} {} favourites",
                    if on { "Added" } else { "Removed" },
                    if on { "to" } else { "from" }
                ));
            }
        }
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
        cut_tip(
            ui,
            ar,
            &about.join("  ·  "),
            Style::new(15.0, 600, th.text),
            tw,
            "about",
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
        // the tuning is cut short before the transpose keys are (P37.9): room kept for "− +12 +" after it
        let keep = snap(
            px(10.0) + 2.0 * px(22.0) + text::width(&ctx, "+12", Style::new(13.0, 800, th.text)) + px(4.0),
        );
        let more_max = snap(tx + tw - x - keep).max(pixels(10.0));
        let mr = txt(
            &p,
            &more,
            pos2(x, y),
            14.0,
            th.text_2,
            600,
            Align2::LEFT_TOP,
            Some(more_max),
        );
        let strings = self.transposed_input_string_names().join(" ");
        cut_tip(
            ui,
            mr,
            &format!("{}  ({strings})", more.trim_start_matches("  ·  ")),
            Style::new(14.0, 600, th.text),
            0.0,
            "tuning",
        );
        x = snap(mr.max.x);
        // transpose: a sunken "TRANSPOSE − 0 +" beside the tuning, shifting the guitar input while the backing
        // track and tab stay as written. With no room, the signed shift stays visible.
        let pitch = self.mixer.pitch;
        let cy = y + text::line_height(&ctx, Style::new(14.0, 700, th.text)) / 2.0;
        let follow = if pitch != 0 {
            text::width(
                &ctx,
                &format!("  →  {}", self.transposed_input_tuning_name()),
                Style::new(14.0, 800, th.text),
            )
        } else {
            0.0
        };
        let stepper = self.draw_transpose(ui, pos2(x + px(10.0), cy), snap(tx + tw - x - px(10.0)), follow);
        if let Some(r) = stepper {
            x = snap(r.max.x);
        }
        if pitch != 0 && ipx(tx + tw - x) > ipx(px(60.0)) {
            if stepper.is_none() {
                let r = hud::chip(
                    &p,
                    &songaudio::pitch_short(pitch),
                    pos2(x + px(8.0), cy),
                    hud::Lit::Amber,
                );
                x = snap(r.max.x);
            }
            let now = format!("  →  {}", self.transposed_input_tuning_name());
            let now_max = snap(tx + tw - x).max(pixels(10.0));
            let nr = txt(
                &p,
                &now,
                pos2(x, y),
                14.0,
                th.text,
                800,
                Align2::LEFT_TOP,
                Some(now_max),
            );
            cut_tip(
                ui,
                nr,
                &format!(
                    "Guitar input {}: sounds in {}; backing track stays in {}; tab stays as written",
                    songaudio::pitch_long(pitch),
                    self.transposed_input_tuning_name(),
                    self.sounding_tuning_name()
                ),
                Style::new(14.0, 800, th.text),
                0.0,
                "now",
            );
            x = snap(nr.max.x);
        }
        if ipx(tx + tw - x) > ipx(px(60.0)) {
            let names = format!("   {}", self.transposed_input_string_names().join(" "));
            txt(
                &p,
                &names,
                pos2(x, y),
                14.0,
                th.caption,
                500,
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

    /// The header's transpose stepper, its left middle at `at`, in `room` across: a sunken pill, "TRANSPOSE" (when it
    /// fits), − the semitones +. The keys shift the guitar input; a click on the value puts it back to 0. `follow`:
    /// what must fit after it (the track's tuning). None: no room,
    /// nothing drawn.
    fn draw_transpose(&mut self, ui: &Ui, at: Pos2, room: f32, follow: f32) -> Option<Rect> {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let pitch = self.mixer.pitch;
        let (key, h) = (px(22.0), px(24.0));
        let label = "TRANSPOSE";
        let lab_w = snap(text::width(&ctx, label, Style::caps(10.0, th.caption)) + px(12.0));
        let val_w = snap(text::width(&ctx, "+12", Style::new(13.0, 800, th.text)) + px(4.0));
        let compact = snap(2.0 * key + val_w);
        let w = match transpose_form(room, compact + lab_w, compact, follow) {
            TransposeForm::Full => compact + lab_w,
            TransposeForm::Compact => compact,
            TransposeForm::Hidden => return None,
        };
        let r = Rect::from_min_size(ipos(at.x, at.y - snap(h / 2.0)), vec2(w, h));
        paint::inset(&p, r, Some(h / 2.0), 3.0, Fill::Base);
        let lit = hud::Lit::Amber.color();
        if pitch != 0 {
            paint::ring(&p, r, Some(h / 2.0), 1.5, lit, true);
        }
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let mut x = r.min.x;
        if w > compact {
            caps(
                &p,
                label,
                ipos(x + px(10.0), mid_y(r)),
                10.0,
                Align2::LEFT_CENTER,
                None,
            );
            let lr = Rect::from_min_size(r.min, vec2(lab_w, h));
            if hover.is_some_and(|q| lr.contains(q)) {
                self.tip = Some((
                    "Transpose the guitar input (the backing track and tab stay as written)".into(),
                    String::new(),
                    ipos(lr.min.x, r.max.y + px(8.0)),
                    Align2::LEFT_TOP,
                ));
            }
            x = snap(x + lab_w - px(4.0));
        }
        let minus = Rect::from_min_size(pos2(x, r.min.y), Vec2::splat(key));
        let plus = Rect::from_min_size(ipos(r.max.x - key, r.min.y), Vec2::splat(key));
        for (b, ic, tip, k, act) in [
            (
                minus,
                Icon::Minus,
                "Shift guitar input down a semitone",
                "Shift ↓",
                "pitch-",
            ),
            (
                plus,
                Icon::Plus,
                "Shift guitar input up a semitone",
                "Shift ↑",
                "pitch+",
            ),
        ] {
            let resp = interact(ui, b, ("hdr-transpose", act), Sense::click());
            let ink = if resp.is_pointer_button_down_on() {
                th.accent
            } else if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                p.circle_filled(mid(b), snap(key / 2.0 - px(3.0)), th.line);
                th.text
            } else {
                th.caption
            };
            icons::icon(&p, ic, mid(b), 11.0, ink);
            if resp.hovered() {
                self.tip = Some((
                    tip.into(),
                    k.into(),
                    ipos(mid_x(b), r.max.y + px(8.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.clicked() {
                self.acts.push(act.into());
            }
        }
        let mid_r = Rect::from_min_max(pos2(minus.max.x, r.min.y), pos2(plus.min.x, r.max.y));
        let resp = interact(ui, mid_r, "hdr-transpose-value", Sense::click());
        txt(
            &p,
            &songaudio::pitch_short(pitch),
            mid(mid_r),
            13.0,
            if resp.hovered() {
                th.text
            } else if pitch != 0 {
                lit
            } else {
                th.text_2
            },
            800,
            Align2::CENTER_CENTER,
            None,
        );
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            self.tip = Some((
                if pitch != 0 {
                    format!(
                        "Guitar input shifted {}  ·  click: no shift",
                        songaudio::pitch_long(pitch)
                    )
                } else {
                    "Guitar input: original pitch".into()
                },
                String::new(),
                ipos(mid_x(mid_r), r.max.y + px(8.0)),
                Align2::CENTER_TOP,
            ));
        }
        if resp.clicked() && pitch != 0 {
            self.acts.push("pitch0".into());
        }
        Some(r)
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
        lit: Option<hud::Lit>,
        enabled: bool,
    ) -> Rect {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let bx = px(56.0);
        let lab_w = text::width(&ctx, label, Style::caps(11.0, th.caption)).max(text::width(
            &ctx,
            if value.len() > 4 { value } else { "200%" },
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
        // lit: a glow round the knob (red also frames the unit: the guitar muted)
        if let Some(l) = lit.filter(|_| enabled) {
            if l == hud::Lit::Red {
                hud::frame(&p, r.expand2(vec2(px(8.0), px(6.0))), l);
            }
            hud::knob_glow(&p, c, bx - px(10.0), l);
        }
        if enabled {
            let hot = self.dial_interact(ui, r, kind);
            // (the dark themes' baked knobs show it: held while dragged, else hovered)
            let held = self.dial_drag.as_ref().is_some_and(|d| d.kind == kind);
            knobs::knob_in(
                &p,
                c,
                34.0,
                self.dial_value(base),
                Scale {
                    home: Self::dial_home(base),
                    ..Default::default()
                },
                style,
                qrs_ui::glass::State::of(hot, held),
            );
        } else {
            knobs::knob(&p, c, 34.0, 0.0, Scale::default(), style);
        }
        let tx = snap(r.min.x + bx + px(10.0));
        caps(&p, label, ipos(tx, cy - px(1.0)), 11.0, Align2::LEFT_BOTTOM, None);
        let col = if let Some(l) = lit.filter(|_| enabled) {
            l.color()
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
            /// A setting away from normal (amber) or the guitar muted (red).
            Chip(String, hud::Lit),
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
                        items.push(It::Text(
                            format!("{}: {why}", st.instrument().noun_cap()),
                            th.text_2,
                            700,
                        ));
                        items.push(It::Key("F2"));
                    }
                    Some(inp) => {
                        led = th.hit;
                        items.push(It::Text(inp.device_name.clone(), th.text, 700));
                        items.push(It::Meter);
                        if self.show_controls || !tiles::tile_open(st, "note") {
                            items.push(It::You);
                        }
                        if e.monitor() {
                            // T's tones are chips; as is and the pedalboard are the usual sound
                            let tone = e.tone();
                            let picked = matches!(
                                tone,
                                qrs_audio::tone::ToneSpec::Preset { .. }
                                    | qrs_audio::tone::ToneSpec::Custom { .. }
                            );
                            if picked {
                                items.push(It::Text(
                                    format!("hearing {}", st.instrument().noun()),
                                    th.caption,
                                    500,
                                ));
                                items.push(It::Chip(format!("tone: {}", tone_label(&tone)), hud::Lit::Amber));
                            } else {
                                items.push(It::Text(
                                    format!(
                                        "hearing {}  ·  {}",
                                        st.instrument().noun(),
                                        tone_label(&tone).to_lowercase()
                                    ),
                                    th.caption,
                                    500,
                                ));
                            }
                        } else {
                            items.push(It::Chip(
                                format!("{} muted  ·  M", st.instrument().noun()),
                                hud::Lit::Red,
                            ));
                        }
                    }
                }
            }
        }
        // what's away from normal: easier, the sync and the guitar's timing
        if self.mastery < 0.999 {
            items.push(It::Chip(
                format!("easier: {}%", (self.mastery * 100.0).round() as i64),
                hud::Lit::Amber,
            ));
        }
        if self.offset.abs() >= 0.0005 {
            items.push(It::Chip(
                format!("sync {:+.0} ms", self.offset * 1000.0),
                hud::Lit::Amber,
            ));
        }
        if self.input_offset.abs() >= 0.0005 {
            items.push(It::Chip(
                format!(
                    "{} delay {}",
                    st.instrument().noun(),
                    crate::settings::delay_label(self.input_offset)
                ),
                hud::Lit::Amber,
            ));
        }
        if self.display_offset.abs() >= 0.0005 {
            items.push(It::Chip(
                format!("screen {:+.0} ms", self.display_offset * 1000.0),
                hud::Lit::Amber,
            ));
        }
        let (gap, h) = (px(12.0), px(34.0));
        let widths: Vec<f32> = items
            .iter()
            .map(|it| match it {
                It::Text(s, _, w) => text::width(&ctx, s, Style::new(13.0, *w, th.text)),
                It::Key(k) => cap_w(&ctx, k),
                It::Meter => px(70.0),
                It::You => text::width(&ctx, "you: C#3 −50¢", Style::new(13.0, 700, th.text)),
                It::Chip(s, _) => hud::chip_width(&ctx, s),
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
                It::Chip(s, lit) => {
                    if ipx(room) >= ipx(*wd) {
                        hud::chip(&p, s, pos2(cx, cy), *lit);
                    }
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

    /// The RS Highway key's width: the first key of the tab's and the highway's floating toolbars.
    pub(super) fn rs_highway_w(ctx: &egui::Context) -> f32 {
        let tw_ = text::width(ctx, "RS Highway", Style::new(13.0, 800, Color32::WHITE));
        snap(
            px(12.0)
                + px(20.0)
                + px(8.0)
                + tw_
                + px(8.0)
                + qrs_ui::keys::keycap_width(ctx, "V", 11.0)
                + px(10.0),
        )
    }

    /// The RS Highway key (owner request, 2026-10-29) in `r`, on its toolbar's painter (so it fades, shows through
    /// and rolls away with it): a pill with a soft glow, emerald in the dark themes, orange in Daylight; outlined
    /// while the tab shows, filled while the highway does. V does the same.
    pub(super) fn draw_rs_highway(p: &egui::Painter, r: Rect, on: bool, hot: bool) {
        let th = t();
        let (col, ink) = if theme::is_dark() {
            (Color32::from_rgb(52, 199, 123), Color32::from_rgb(6, 34, 20))
        } else {
            (Color32::from_rgb(232, 112, 30), Color32::from_rgb(255, 248, 240))
        };
        let (h, cy) = (r.height(), r.center().y);
        paint::glow(
            p,
            r,
            col,
            if hot { 7.0 } else { 5.0 },
            Some(if on || hot { 0.55 } else { 0.35 }),
            Some(h / 2.0),
        );
        let fg = if on {
            p.rect_filled(r, h / 2.0, col);
            ink
        } else {
            p.rect_filled(r, h / 2.0, th.base);
            p.rect_filled(r, h / 2.0, col.gamma_multiply(if hot { 0.28 } else { 0.18 }));
            p.rect_stroke(r, h / 2.0, egui::Stroke::new(1.5, col), egui::StrokeKind::Inside);
            col
        };
        icons::icon(p, Icon::Highway, pos2(r.min.x + px(22.0), cy), 15.0, fg);
        text::draw(
            p,
            "RS Highway",
            pos2(r.min.x + px(40.0), cy),
            Align2::LEFT_CENTER,
            Style::new(13.0, 800, fg),
            None,
        );
        qrs_ui::keys::keycap(
            p,
            "V",
            pos2(r.max.x - px(10.0), cy),
            Align2::RIGHT_CENTER,
            11.0,
            if on {
                qrs_ui::keys::CapOn::Fill
            } else {
                qrs_ui::keys::CapOn::Material
            },
        );
    }

    /// The tab's toolbar (owner request, D31; on its own row since P37.6b, D135): the RS Highway key, zoom, the bars'
    /// width and the strings' spacing as sunken pills (− amount +, cut in like the search field), each named before
    /// it, then compact rows, chord numbers and full screen as sunken round keys. It sits in `row`, a strip of its own
    /// over the tab's well, right-aligned: nothing of the tab is under it (it used to float over the first row,
    /// see-through, and roll up into a key; the reviews found the notes under it hidden). In a narrow window the
    /// names shorten, then become icons (named on hover).
    fn draw_tab_tools(&mut self, ui: &mut Ui, row: Rect) {
        let ctx = ui.ctx().clone();
        let th = t();
        let (key, core, gap, pad) = (px(26.0), px(91.0), px(6.0), px(5.0));
        let div_gap = px(8.0);
        let rs_w = Self::rs_highway_w(&ctx);
        let lab_st = Style::new(12.0, 700, th.caption);
        let names: [[&str; 3]; 3] = [
            ["Zoom", "Zoom", ""],
            ["Bar width", "Width", ""],
            ["String spacing", "Spacing", ""],
        ];
        let width_with = |v: usize| -> (f32, [f32; 3]) {
            let labs = names.map(|n| {
                if n[v].is_empty() {
                    px(18.0)
                } else {
                    snap(text::width(&ctx, n[v], lab_st) + px(6.0))
                }
            });
            let w = 2.0 * pad
                + rs_w
                + 3.0 * gap
                + labs.iter().sum::<f32>()
                + 3.0 * (px(3.0) + core)
                + 2.0 * div_gap
                + px(1.0)
                + 3.0 * key
                + px(12.0);
            (snap(w), labs)
        };
        // the longest names that fit
        let (w, labs, v) = (0..3)
            .map(|v| {
                let (w, l) = width_with(v);
                (w, l, v)
            })
            .find(|(w, ..)| *w <= row.width())
            .unwrap_or_else(|| {
                let (w, l) = width_with(2);
                (w, l, 2)
            });
        let h = snap(key + 2.0 * pad);
        let plate = Rect::from_min_size(ipos(row.max.x - w, mid_y(row) - snap(h / 2.0)), vec2(w, h));
        self.tools_rect = plate;
        let tu = ui.new_child(egui::UiBuilder::new().max_rect(plate));
        let p = tu.painter().clone();
        paint::floating(&p, plate, Some(12.0));
        let cy = mid_y(plate);
        let hit = |r: Rect, id: (&str, &str), sense: Sense| -> (bool, bool, bool) {
            let resp = interact(&tu, r, id, sense);
            (resp.hovered(), resp.is_pointer_button_down_on(), resp.clicked())
        };
        let mut tip: Option<(&str, &str, Rect)> = None;
        let mut x = plate.min.x + pad;
        // the RS Highway key first
        {
            let r = Rect::from_min_size(ipos(x, cy - snap(key / 2.0)), vec2(rs_w, key));
            let (hovered, _, clicked) = hit(r, ("tab-tool", "rs-highway"), Sense::click());
            Self::draw_rs_highway(&p, r, false, hovered);
            if hovered {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                tip = Some(("RS-style highway", "V", r));
            }
            if clicked {
                self.acts.push("view".into());
            }
            x = r.max.x + gap;
        }
        let steppers: [(Icon, f32, &str, [(&str, &str, &str); 2], &str); 3] = [
            (
                Icon::Search,
                self.tab.zoom,
                "Zoom",
                [("Zoom out", "↓", "zoom_out"), ("Zoom in", "↑", "zoom_in")],
                "zoom_reset",
            ),
            (
                Icon::Width,
                self.tab.width_k,
                "Bar width",
                [
                    ("Narrower bars", "Alt ←", "tab_narrower"),
                    ("Wider bars", "Alt →", "tab_wider"),
                ],
                "tab_width_reset",
            ),
            (
                Icon::Height,
                self.tab.height_k,
                "String spacing",
                [
                    ("Strings closer", "Alt ↓", "tab_shorter"),
                    ("Strings apart", "Alt ↑", "tab_taller"),
                ],
                "tab_height_reset",
            ),
        ];
        for (i, &(icon, val, what, keys, reset)) in steppers.iter().enumerate() {
            // what it sizes, named before the pill (an icon when the window is too narrow for the words)
            let lab = Rect::from_min_size(ipos(x, cy - snap(key / 2.0)), vec2(labs[i], key));
            if names[i][v].is_empty() {
                icons::icon(&p, icon, mid(lab), 14.0, th.caption);
            } else {
                text::draw(
                    &p,
                    names[i][v],
                    pos2(lab.min.x + px(2.0), cy),
                    Align2::LEFT_CENTER,
                    lab_st,
                    None,
                );
            }
            if hit(lab, ("tab-tool-label", what), Sense::hover()).0 {
                tip = Some((what, "", lab));
            }
            let r = Rect::from_min_size(pos2(snap(lab.max.x + px(3.0)), lab.min.y), vec2(core, key));
            paint::inset(&p, r, None, 3.0, Fill::Base);
            // − and + at the pill's ends, the amount between (a click on it puts it back to 100%)
            let minus = Rect::from_min_size(r.min, Vec2::splat(key));
            let plus = Rect::from_min_size(ipos(r.max.x - key, r.min.y), Vec2::splat(key));
            for (b, ic, (label, k, act)) in [(minus, Icon::Minus, keys[0]), (plus, Icon::Plus, keys[1])] {
                let (hovered, down, clicked) = hit(b, ("tab-tool", act), Sense::click());
                let ink = if down {
                    th.accent
                } else if hovered {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                    p.circle_filled(mid(b), snap(key / 2.0 - px(3.0)), th.line);
                    th.text
                } else {
                    th.caption
                };
                icons::icon(&p, ic, mid(b), 12.0, ink);
                if clicked {
                    self.acts.push(act.into());
                }
                if hovered {
                    tip = Some((label, k, b));
                }
            }
            let mid_r = Rect::from_min_max(pos2(minus.max.x, r.min.y), pos2(plus.min.x, r.max.y));
            let (hovered, _, clicked) = hit(mid_r, ("tab-tool-read", what), Sense::click());
            txt(
                &p,
                &format!("{}%", (val * 100.0).round() as i64),
                mid(mid_r),
                12.0,
                if hovered { th.text } else { th.accent_ink },
                800,
                Align2::CENTER_CENTER,
                None,
            );
            if hovered {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                tip = Some((what, "click: 100%", mid_r));
            }
            if clicked {
                self.acts.push(reset.into());
            }
            x = r.max.x + if i < 2 { gap } else { 0.0 };
        }
        // a groove between the steppers and the switches
        x = snap(x + div_gap);
        p.rect_filled(
            Rect::from_min_size(ipos(x, plate.min.y + px(8.0)), vec2(px(1.0), snap(h - px(16.0)))),
            0.0,
            th.line,
        );
        x = snap(x + px(1.0) + div_gap);
        let toggles = [
            (Icon::Rows, self.tab.compact, "Compact rows", "C", "tab_compact"),
            (
                Icon::Music,
                self.chord_numerals,
                if self.chord_numerals {
                    "Chord names: numbers in the song's key (click: as charted)"
                } else {
                    "Chord names as charted (click: numbers in the song's key)"
                },
                "K",
                "chord_numerals",
            ),
            (
                if self.tab_full { Icon::Shrink } else { Icon::Expand },
                self.tab_full,
                if self.tab_full {
                    "Leave full screen"
                } else {
                    "Full-screen tab"
                },
                if self.tab_full { "F / Esc" } else { "F" },
                "tab_full",
            ),
        ];
        for (icon, on, label, keycap, act) in toggles {
            let r = Rect::from_min_size(ipos(x, cy - snap(key / 2.0)), Vec2::splat(key));
            let (hovered, down, clicked) = hit(r, ("tab-tool", act), Sense::click());
            paint::inset(&p, r, None, if down { 4.0 } else { 3.0 }, Fill::Base);
            if on {
                paint::ring(&p, r, None, 1.5, th.accent, true);
            }
            let ink = if on {
                th.accent_ink
            } else if hovered {
                th.text
            } else {
                th.caption
            };
            icons::icon(&p, icon, mid(r), 13.0, ink);
            if hovered {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                tip = Some((label, keycap, r));
            }
            if clicked {
                self.acts.push(act.into());
            }
            x = snap(r.max.x + px(4.0));
        }
        if let Some((label, k, r)) = tip {
            self.tip = Some((
                label.into(),
                k.into(),
                ipos(r.max.x, plate.max.y + px(8.0)),
                Align2::RIGHT_TOP,
            ));
        }
    }

    /// The tab's toolbar row and the tab's well under it (D135): the row takes the top of `well` (reaching `lift`
    /// up into the margin above it), the tab the rest, a gap between. Nothing of one is in the other.
    pub(super) fn tools_row(well: Rect, lift: f32) -> (Rect, Rect) {
        let h = snap(px(26.0) + 2.0 * px(5.0));
        let row = Rect::from_min_size(ipos(well.min.x, well.min.y - lift), vec2(well.width(), h));
        let tab = Rect::from_min_max(pos2(well.min.x, snap(row.max.y + px(8.0))), well.max);
        (row, tab)
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
        // the zoom, or the bars' width or the strings' spacing on top of it, with its range
        let (z, lo, hi, label) = match self.zoom_what {
            ZoomWhat::Zoom => (self.tab.zoom, 0.5, 2.5, ""),
            ZoomWhat::Width => (self.tab.width_k, WIDTH_K.0, WIDTH_K.1, "Width "),
            ZoomWhat::Height => (self.tab.height_k, HEIGHT_K.0, HEIGHT_K.1, "Spacing "),
        };
        let extra = if label.is_empty() {
            0.0
        } else {
            text::width(ui.ctx(), label, Style::new(16.0, 800, th.text))
        };
        // panel.topright = (well.right - int(18 * s), well.y + int(18 * s))
        let (pw, ph) = (snap(px(196.0) + extra), px(44.0));
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
            &format!("{label}{}%", (z * 100.0).round() as i64),
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
        let u = ((z.ln() - lo.ln()) / (hi.ln() - lo.ln())).clamp(0.0, 1.0);
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
        draw_tip(p, label, key, pos, anchor, area);
    }

    /// The transport (back, play or pause, forward) as round keys in a sunken cradle; the section, bar and time
    /// over the seek groove; the guitar's and the song's volume as knobs.
    fn draw_footer(&mut self, ui: &Ui, st: &mut AppState, r: Rect, now: f64) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let cy = mid_y(r);
        let playing = self.transport.playing()
            || self.countdown_until.is_some()
            || self.waiting.is_some()
            || self.synth.state != SynthState::Idle;
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
        // the baked transport (P34.3), made for the dark panels: the gem's light eases on a switch (a first frame
        // starts where it ends); the classic look and Daylight keep the painted keys
        let art = !theme::classic() && th.dark;
        if art && self.art.is_none() {
            self.art = Some(transport_art::Art::load(&ctx));
        }
        let dt = self.art_at.map_or(10.0, |t| t.elapsed().as_secs_f32().min(0.1));
        self.art_at = Some(Instant::now());
        if self.gem_glow.step(playing, dt) {
            ctx.request_repaint();
        }
        let gem_at = play.center();
        if art {
            let a = self.art.as_ref().expect("loaded");
            a.spill(&p, gem_at, &self.gem_glow);
            for (r, id, act, icon, flip) in [
                (back, "t-back", "back", Icon::Back, false),
                (fwd, "t-fwd", "fwd", Icon::Fwd, true),
            ] {
                let resp = interact(ui, r, id, Sense::click());
                if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                let state = transport_art::State::of(&resp);
                a.key(&p, r.center(), state, flip, &self.gem_glow);
                // « » inked into the stone: a light lip under the dark ink
                let c = r.center() + vec2(0.0, px(state.lift()));
                icons::icon(
                    &p,
                    icon,
                    c + vec2(0.0, px(0.9)),
                    20.0,
                    Color32::from_white_alpha(120),
                );
                icons::icon(&p, icon, c, 20.0, Color32::from_rgb(44, 46, 50));
                if resp.clicked() {
                    self.acts.push(act.into());
                }
            }
            let resp = interact(ui, play, "t-play", Sense::click());
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            a.gem(&p, gem_at, transport_art::State::of(&resp), &self.gem_glow);
            if resp.clicked() {
                self.acts.push("play".into());
            }
        } else {
            if widgets::round_button(ui, back, "t-back", Icon::Back, 20.0, RoundKind::Stone, false).clicked()
            {
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
            if widgets::round_button(ui, fwd, "t-fwd", Icon::Fwd, 20.0, RoundKind::Stone, false).clicked() {
                self.acts.push("fwd".into());
            }
        }
        self.draw_restart_key(ui, cradle);
        // the lamps' row, centred under the tab (P37.11); recording what you hear (R) adds its badge to it (P23.9)
        let well = self.well;
        self.draw_lamps_row(ui, well, cradle, r, st.rec.status());
        // the volumes: your guitar, then the song
        let song = self.draw_knob_unit(
            ui,
            "vol",
            "SONG",
            &format!("{}%", (self.transport.volume() * 100.0).round() as i64),
            pos2(r.max.x, cy),
            None,
            true,
        );
        // M mutes the guitar: the knob glows red and says so (its level stays under it)
        let muted = self.engine().is_some_and(|e| !e.monitor());
        let gtxt = match self.engine() {
            Some(_) if muted => "MUTED".to_string(),
            Some(e) => format!("{}%", (e.guitar_gain() * 100.0).round() as i64),
            None => "–".into(),
        };
        let has_engine = self.engine().is_some();
        let gtr = self.draw_knob_unit(
            ui,
            "gvol",
            st.instrument().noun_upper(),
            &gtxt,
            ipos(song.min.x - px(24.0), cy),
            muted.then_some(hud::Lit::Red),
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
        let m = px(2.0).max(pixels(1.0));
        let pos = self.seek_drag.unwrap_or(now);
        let xp = bx(pos);
        // the baked bar: the thumb's centre travels 13 points in from either end, the resin filled to it
        let thumb_x = {
            let i = px(transport_art::THUMB_IN);
            let f = (pos / len).clamp(0.0, 1.0) as f32;
            bar.min.x + i + f * (bar.width() - 2.0 * i).max(0.0)
        };
        if art {
            let ticks: Vec<f32> = self.chart.sections.iter().skip(1).map(|s| bx(s.start)).collect();
            let a = self.art.as_ref().expect("loaded");
            a.bar(
                &p,
                bar.min.x,
                bar.max.x,
                mid_y(bar),
                thumb_x,
                &ticks,
                gem_at,
                &self.gem_glow,
            );
        } else {
            paint::inset(&p, bar, None, 3.0, Fill::Base);
        }
        if !art && xp - bar.min.x > m {
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
        for sec in self.chart.sections.iter().skip(1).filter(|_| !art) {
            let xx = snap(bx(sec.start));
            p.rect_filled(
                Rect::from_min_size(ipos(xx - one, bar.min.y + m), vec2(two, bar.height() - 2.0 * m)),
                0.0,
                th.track,
            );
        }
        if let (Some(la), Some(lb)) = (self.loop_a, self.loop_b) {
            let (la, lb) = (bx(la), bx(lb));
            // the looped stretch glows over the groove
            let span = Rect::from_min_max(
                ipos(la, bar.min.y - px(5.0)),
                ipos(lb.max(la + px(4.0)), bar.max.y + px(5.0)),
            );
            paint::glow(&p, span, th.accent, 10.0, Some(0.45), Some(6.0));
            p.rect_filled(span, px(6.0), th.accent.gamma_multiply(0.18));
            paint::ring(&p, span, Some(6.0), 1.5, th.accent, false);
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
        if art {
            // (the owner: no pop on hover; lifted a little only while it's held and dragged)
            let state = if self.seek_drag.is_some() {
                transport_art::State::Hover
            } else {
                transport_art::State::Normal
            };
            let a = self.art.as_ref().expect("loaded");
            a.thumb(&p, pos2(thumb_x, mid_y(bar)), state, gem_at, &self.gem_glow);
        } else {
            qrs_ui::keys::thumb(&p, pos2(snap(xp), mid_y(bar)), 26.0);
        }
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
        let right = txt(
            &p,
            &fmt_time(pos),
            ipos(tr.min.x, top),
            13.0,
            th.text_2,
            700,
            Align2::RIGHT_TOP,
            None,
        );
        // (the sync and the guitar's timing, when set, are chips in the status strip)
        let room = snap(right.min.x - left.max.x - px(24.0));
        if let (Some(la), Some(lb)) = (self.loop_a, self.loop_b)
            && ipx(room) > ipx(px(60.0))
        {
            let mut s = format!("LOOP  {}–{}", fmt_time(la), fmt_time(lb));
            if self.loop_passes > 0 {
                s += &format!("  ·  pass {}", self.loop_passes);
            }
            if let Some(acc) = self.loop_acc {
                s += &format!("  ·  {}%", (acc * 100.0).round() as i64);
            }
            // a lit chip, between the bar number and the time
            let w = hud::chip_width(&ctx, &s);
            let cx = snap(left.max.x + right.min.x) / 2.0;
            let cy = top + text::line_height(&ctx, Style::new(13.0, 700, th.text)) / 2.0;
            if ipx(room) >= ipx(w) {
                hud::chip(&p, &s, pos2(snap(cx - w / 2.0), cy), hud::Lit::Amber);
            } else {
                txt(
                    &p,
                    &s,
                    ipos(cx, top),
                    13.0,
                    th.accent_ink,
                    700,
                    Align2::CENTER_TOP,
                    Some(room),
                );
            }
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
    /// that swaps them at the bottom; in a lesson, the coach too (and Lesson / Tiles / Controls at the bottom).
    fn draw_aside(&mut self, ui: &mut Ui, st: &mut AppState, aside: Rect, now: f64) {
        use qrs_ui::units::px;
        let p = ui.painter().clone();
        paint::raised(&p, aside, Some(28.0), 8.0, Fill::Base, 0.0);
        let pad = px(16.0);
        let inner = aside.shrink(pad);
        let bh = px(48.0);
        let foot = Rect::from_min_size(pos2(inner.min.x, inner.max.y - bh), vec2(inner.width(), bh));
        let area = Rect::from_min_max(inner.min, pos2(inner.max.x, foot.min.y - px(12.0)));
        self.panel_rect = area;
        // the Shortcuts key at the foot's left (every key on a sheet), the console's swap key beside it
        let keys = Rect::from_min_size(foot.min, vec2(px(132.0).min(foot.width() * 0.4), bh));
        let btn = Rect::from_min_max(pos2(keys.max.x + px(10.0), foot.min.y), foot.max);
        let kr = button(
            ui,
            keys,
            "aside-keys",
            "Shortcuts",
            "",
            ButtonStyle {
                icon: Some(Icon::Rows),
                size: 14.0,
                r: Some(16.0),
                active: self.shortcuts,
                // used rarely: a text key on the glass look (P34.5)
                plain: true,
                ..Default::default()
            },
        );
        if kr.hovered() {
            self.tip = Some((
                "Every keyboard shortcut".into(),
                "F1".into(),
                pos2(keys.min.x, keys.min.y - px(10.0)),
                Align2::LEFT_BOTTOM,
            ));
        }
        if kr.clicked() {
            self.acts.push("shortcuts".into());
        }
        if self.take_panel_up() {
            // a take outside a lesson: its card (Close goes back to the tiles)
            self.tile_rects.clear();
            let full = Rect::from_min_max(area.min, btn.max);
            self.draw_take_panel(ui, full);
            return;
        }
        if let Some(showing) = self.coach.as_ref().map(|c| c.show) {
            let sel = if showing {
                0
            } else if self.show_controls {
                2
            } else {
                1
            };
            if showing {
                self.tile_rects.clear();
                self.draw_coach(ui, area);
            } else if self.show_controls {
                self.tile_rects.clear();
                self.draw_controls(ui, st, area);
            } else {
                self.draw_tile_column(ui, st, area, now, aside);
            }
            let labels = ["Lesson", "Tiles", "Controls"];
            let mut seg = Segmented::new(&labels, Some(sel));
            // each option as wide as its label needs and the slack shared (equal thirds cut "Controls" to "Contr…",
            // P37.4); a smaller size first if they don't fit
            let avail = btn.width() - 2.0 * px(seg.pad) - 2.0 * px(seg.gap);
            for size in [seg.size, 13.0, 12.0] {
                let nat = widgets::segment_widths(ui.ctx(), &labels, size, seg.pad);
                let sum: f32 = nat.iter().sum();
                seg.size = size;
                if sum <= avail || size == 12.0 {
                    let k = if sum > avail { avail / sum } else { 1.0 };
                    let slack = ((avail - sum * k) / nat.len() as f32).max(0.0);
                    seg.widths = Some(nat.iter().map(|w| w * k + slack).collect());
                    break;
                }
            }
            let (_, hit) = widgets::segmented(ui, btn, "aside-coach", &seg);
            if let Some(i) = hit
                && i != sel
            {
                self.acts.push(
                    match i {
                        0 => "coach:show:lesson",
                        1 => "coach:show:tiles",
                        _ => "coach:show:controls",
                    }
                    .into(),
                );
            }
            return;
        }
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
                format!("No song audio: {}.", plain_audio_error(&self.audio_error)),
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
            vec2(button_width(&ctx, "Sound settings", "F2", 13.0, false), px(40.0)),
        );
        if button(
            ui,
            b,
            "noaudio-settings",
            "Sound settings",
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
        } else if self.overlay_up() {
            summary = true;
            if !self.backdrop.capturing() {
                // the next frame, the player alone like this one, is captured: blurred under the card from then on
                self.backdrop.capture(ui.ctx());
            }
        }
        let _ = summary;
        let prog = self
            .shot_stretch
            .or_else(|| self.mixer.pending_progress())
            .or_else(|| self.stretch.as_ref().filter(|j| !j.done()).map(|j| j.progress()));
        let buffering = self.engine().is_some_and(|e| e.buffering());
        if self.pending_speed.is_some() || prog.is_some() || buffering {
            self.draw_slowing(&p, well, prog);
        }
        if let Some((text_, tail, until)) = &self.toast
            && Instant::now() < *until
        {
            if self.toast_keys.is_empty() {
                widgets::toast(&p, text_, ipos(mid_x(well), well.min.y + px(44.0)), tail);
            } else {
                // a key's flash: its caps and what it did, at the tab's foot
                hud::flash(
                    &p,
                    &self.toast_keys,
                    text_,
                    tail,
                    ipos(mid_x(well), well.max.y - px(48.0)),
                );
            }
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

    /// The results card or the review is up, over the blurred player.
    fn overlay_up(&self) -> bool {
        self.review_open() || self.summary_up()
    }

    /// The results are up: the song ran to its end with something played.
    fn summary_up(&self) -> bool {
        summary_shows(
            self.summary_dismissed,
            self.countdown_until.is_some() || self.transport.playing(),
            self.transport.finished(),
            self.scorer.groups.iter().any(|g| g.state.is_some()),
        )
    }

    /// The results card over the blurred, dimmed player: accuracy as a ring, the score, the counts, your best runs
    /// (this one lit), Play again / Library.
    fn draw_summary(&mut self, ui: &Ui, st: &AppState, area: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        // modal: only its buttons (and keys) do anything; a click on the haze around it closes it, as its x does
        // (owner request, 2026-11-02)
        let modal = interact(ui, area, "summary-modal", Sense::click());
        let c = self.scorer.counts();
        let scored = self.result_scored;
        let top = st.userdata.top(&self.ids, &self.part_name);
        // the best runs with repeats folded into one row (P37.6d)
        let best = runstats::collapse_runs(&top);
        let stats = self.run_stats();
        let (w, h) = (snap(area.width()), snap(area.height()));
        let pw = pixels(to_px(620.0).min(ipx(w) - to_px(40.0)).trunc() as f32);
        let (px_, py_) = (px(34.0), px(30.0));
        let runs_h = if best.is_empty() {
            0.0
        } else {
            snap(px(14.0) + px(16.0) + px(10.0) + best.len() as f32 * px(26.0) + px(10.0))
        };
        let ph0 = snap(
            py_ + px(44.0)
                + px(20.0)
                + px(150.0)
                + px(20.0)
                + px(70.0)
                + px(20.0)
                + if best.is_empty() { 0.0 } else { runs_h + px(20.0) }
                + px(54.0)
                + py_,
        );
        // the run by section and technique, when there's a section to show and the window has the room
        let stats_h = snap(STATS_H.iter().map(|&v| px(v)).sum::<f32>());
        let show_stats = !stats.sections.is_empty() && ph0 + stats_h + px(20.0) <= h - px(40.0);
        let ph = if show_stats {
            snap(ph0 + stats_h + px(20.0))
        } else {
            ph0
        };
        // ((w - pw) // 2, max(int(20 * s), (h - ph) // 2))
        let panel = Rect::from_min_size(
            ipos(
                area.min.x + snap((w - pw) / 2.0),
                area.min.y + px(20.0).max(pixels(((ipx(h) - ipx(ph)) / 2.0).floor() as f32)),
            ),
            vec2(pw, ph),
        );
        paint::floating(&p, panel, Some(32.0));
        if modal.clicked() && modal.interact_pointer_pos().is_some_and(|at| !panel.contains(at)) {
            self.acts.push("summary_close".into());
        }
        let (lx, rx) = (snap(panel.min.x + px_), snap(panel.max.x - px_));
        let mut y = snap(panel.min.y + py_);
        let (chip_text, chip_col, chip_icon) = if self.new_best {
            ("New best score", th.hit, Some(Icon::Up))
        } else if !scored {
            ("Not saved", th.late, None)
        } else {
            ("", th.text, None)
        };
        // the close key at the top right (P23.12): the card goes, the song page stays as it was (a recording keeps
        // running), with Esc's cap beside it
        let close = Rect::from_min_size(ipos(rx - px(34.0), y + px(4.0)), Vec2::splat(px(34.0)));
        // a plain grey well that sinks when pressed (owner: the ruby was too much here)
        let close_resp = interact(ui, close, "sum-close", Sense::click());
        paint::inset(
            &p,
            close,
            None,
            if close_resp.is_pointer_button_down_on() {
                4.5
            } else {
                3.0
            },
            Fill::Base,
        );
        icons::icon(
            &p,
            Icon::X,
            mid(close),
            13.0,
            if close_resp.hovered() { th.text } else { th.text_2 },
        );
        if close_resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        let esc = keycap(
            &p,
            "Esc",
            ipos(close.min.x - px(8.0), mid_y(close)),
            Align2::RIGHT_CENTER,
            11.0,
            CapOn::Material,
        );
        let close_hover = close_resp.hovered()
            || ctx
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|h| esc.contains(h));
        if close_resp.clicked() {
            self.acts.push("summary_close".into());
        }
        // the header: what this was, and a chip for a new best (or why it wasn't saved)
        let mut chip_x = snap(esc.min.x - px(14.0));
        if !chip_text.is_empty() {
            let cw = snap(
                text::width(&ctx, chip_text, Style::new(13.0, 800, chip_col))
                    + px(24.0)
                    + if chip_icon.is_some() { px(20.0) } else { 0.0 },
            );
            let chip = Rect::from_min_size(ipos(chip_x - cw, y + px(4.0)), vec2(cw, px(34.0)));
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
        let line_w = snap(rx - sx);
        if !scored {
            for line in text::wrap(
                &ctx,
                "Only a full run at 100% difficulty and speed counts for the high score.",
                Style::new(13.0, 700, th.late),
                line_w,
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
            // the best streak and the multiplier it reached (x2 at 10, x3 at 20, x4 at 30)
            let bs = self.scorer.best_streak;
            let mut parts: Vec<(String, Color32, u16)> = vec![
                ("best streak ".into(), th.text_2, 500),
                (bs.to_string(), th.text, 800),
            ];
            let m = qrs_analysis::detect::multiplier(bs);
            if m > 1 {
                parts.push(("  ·  reached ".into(), th.text_2, 500));
                parts.push((format!("×{m}"), th.accent_ink, 800));
            }
            summary_line(&p, &parts, pos2(sx, ly), line_w);
            ly = snap(ly + px(19.0));
            if self.new_best && best.len() > 1 {
                summary_line(
                    &p,
                    &[
                        ("up ".into(), th.text_2, 500),
                        (
                            group_digits((best[0].run.score - best[1].run.score).max(0) as usize),
                            th.hit,
                            800,
                        ),
                        (" on your old best".into(), th.text_2, 500),
                    ],
                    pos2(sx, ly),
                    line_w,
                );
                ly = snap(ly + px(19.0));
            }
        }
        // the change since the last run (any run saved, scored or practice: the same notes, so it compares)
        if let Some(last) = stats.last_acc {
            let d = ((stats.acc - last) * 100.0).round() as i64;
            let (num, col) = match d {
                0 => ("same accuracy".to_string(), th.text),
                d if d > 0 => (format!("+{d}%"), th.hit),
                d => (format!("\u{2212}{}%", -d), th.miss),
            };
            let tail = if d == 0 {
                " as your last run"
            } else {
                " on your last run"
            };
            if ly <= y + px(146.0) {
                summary_line(
                    &p,
                    &[(num, col, 800), (tail.into(), th.text_2, 500)],
                    pos2(sx, ly),
                    line_w,
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
        if show_stats {
            let bx = Rect::from_min_size(pos2(lx, y), vec2(snap(rx - lx), stats_h));
            draw_run_stats(&p, &ctx, bx, &stats);
            y = snap(bx.max.y + px(20.0));
        }
        if !best.is_empty() {
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
            for (i, row) in best.iter().enumerate() {
                // the run you just finished (it may be one of a row's repeats)
                let mine = scored && self.result_rank.is_some_and(|k| row.of.contains(&k));
                let run = &row.run;
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
                let r = txt(
                    &p,
                    &format!("{}.  {}{acc}", i + 1, group_digits(run.score as usize)),
                    ipos(x0 + px(20.0), cy),
                    14.0,
                    if mine { th.accent_ink } else { th.text_2 },
                    if mine { 800 } else { 600 },
                    Align2::LEFT_CENTER,
                    None,
                );
                if row.times > 1 {
                    txt(
                        &p,
                        &format!("\u{d7}{} runs", row.times),
                        ipos(r.max.x + px(10.0), cy),
                        12.0,
                        th.caption,
                        700,
                        Align2::LEFT_CENTER,
                        None,
                    );
                }
                let tag_w = if mine {
                    text::width(&ctx, "this run", Style::new(13.0, 800, th.accent_ink))
                } else {
                    0.0
                };
                let when = if row.times > 1 {
                    format!("last {}", fmt_when(run.at, true))
                } else {
                    fmt_when(run.at, true)
                };
                txt(
                    &p,
                    &when,
                    ipos(bx.min.x + px(260.0), cy),
                    13.0,
                    th.caption,
                    500,
                    Align2::LEFT_CENTER,
                    Some(snap(
                        bx.max.x - px(18.0) - tag_w - px(12.0) - bx.min.x - px(260.0),
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
        let lib_w = snap(button_w(&ctx, "Library", "⌫", 15.0, false) + px(20.0));
        let lib = Rect::from_min_size(ipos(rx - lib_w, y), vec2(lib_w, px(54.0)));
        // the run's review (P29.1): where the notes went, to practise
        let rev_w = if self.review.is_some() {
            snap(button_w(&ctx, "Review", "E", 15.0, true) + px(20.0))
        } else {
            0.0
        };
        let rev = Rect::from_min_size(ipos(lib.min.x - px(14.0) - rev_w, y), vec2(rev_w, px(54.0)));
        let again_end = if rev_w > 0.0 { rev.min.x } else { lib.min.x };
        let again = Rect::from_min_size(pos2(lx, y), vec2(snap(again_end - px(14.0) - lx), px(54.0)));
        if rev_w > 0.0
            && button(
                ui,
                rev,
                "sum-review",
                "Review",
                "E",
                ButtonStyle {
                    icon: Some(Icon::Search),
                    accent: self.review.as_ref().is_some_and(|r| !r.list.is_empty()),
                    size: 15.0,
                    r: Some(17.0),
                    ..Default::default()
                },
            )
            .clicked()
        {
            self.acts.push("review".into());
        }
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
            "⌫", // (Backspace: Esc closes the card)
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
        // (P36.6, the review's P2.4) the hits sat one way all run: offer the delay that centres them
        let dts: Vec<f64> = self
            .scorer
            .groups
            .iter()
            .filter(|g| g.state.is_some_and(|j| j != Judgement::Miss))
            .map(|g| g.dt)
            .collect();
        if let Some(v) = crate::timing_cal::offer(&dts) {
            let target = crate::timing_cal::ms_round(self.input_offset + v);
            let words = format!(
                "Your notes sat {:.0} ms {} all run.",
                (v * 1000.0).abs(),
                if v < 0.0 { "late" } else { "early" }
            );
            let label = format!(
                "Set {} delay to {}",
                st.instrument().noun(),
                crate::settings::delay_label(target)
            );
            let bw = widgets::button_width(&ctx, &label, "", 13.0, false);
            let plate = Rect::from_min_size(
                pos2(panel.min.x, panel.max.y + px(14.0)),
                vec2(panel.width(), px(52.0)),
            );
            if plate.max.y <= area.max.y {
                paint::floating(&p, plate, Some(18.0));
                text::draw(
                    &p,
                    &words,
                    pos2(plate.min.x + px(20.0), mid_y(plate)),
                    Align2::LEFT_CENTER,
                    Style::new(13.0, 700, th.text),
                    Some(plate.width() - bw - px(50.0)),
                );
                if widgets::button(
                    ui,
                    Rect::from_min_size(
                        pos2(plate.max.x - px(14.0) - bw, plate.min.y + px(9.0)),
                        vec2(bw, px(34.0)),
                    ),
                    "summary-timing-offer",
                    &label,
                    "",
                    ButtonStyle {
                        size: 13.0,
                        r: Some(11.0),
                        ..Default::default()
                    },
                )
                .clicked()
                {
                    self.acts.push(format!("gtime_set:{target}"));
                }
            }
        }
        if close_hover {
            draw_tip(
                &p,
                "Close: stay on the song",
                "Esc",
                pos2(close.max.x, snap(close.max.y + px(8.0))),
                Align2::RIGHT_TOP,
                area,
            );
        }
    }
}

pub fn tone_label(t: &qrs_audio::tone::ToneSpec) -> String {
    match t {
        qrs_audio::tone::ToneSpec::AsIs => "As is".into(),
        qrs_audio::tone::ToneSpec::Preset { preset } => preset.label().into(),
        qrs_audio::tone::ToneSpec::Custom { .. } => "Custom".into(),
        qrs_audio::tone::ToneSpec::Board { .. } => "Rig".into(),
    }
}

/// The results card is up: the song ran to its end (not playing or counting in) with something played, and the
/// card wasn't closed.
fn summary_shows(dismissed: bool, running: bool, finished: bool, judged: bool) -> bool {
    !dismissed && !running && finished && judged
}

/// Whether a closed results card stays closed this frame: until the song plays again (a restart, a seek and
/// play) or a run ends (P23.12).
fn keep_dismissed(dismissed: bool, playing: bool, ended_now: bool) -> bool {
    dismissed && !playing && !ended_now
}

/// How much of the header's transpose stepper fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransposeForm {
    /// "TRANSPOSE − 0 +"
    Full,
    /// "− 0 +"
    Compact,
    /// none (the "+1" chip shows a shift instead)
    Hidden,
}

/// The stepper's form in `room` across: whole if there's room for it and `after` (what follows it: the tuning it
/// sounds in, when transposed), else without its label (P37.9: − and + stay, whatever follows is cut instead, so a
/// transposed song can always be put back), none only when not even that fits.
fn transpose_form(room: f32, full_w: f32, compact_w: f32, after: f32) -> TransposeForm {
    if room >= full_w + after {
        TransposeForm::Full
    } else if room >= compact_w {
        TransposeForm::Compact
    } else {
        TransposeForm::Hidden
    }
}

/// A key while the results card may be up: Esc closes the card (only the card: not Esc's "back to the library",
/// P23.12). None: the key does what it always does.
fn summary_key(key: Key, summary_up: bool) -> Option<&'static str> {
    (summary_up && key == Key::Escape).then_some("summary_close")
}

/// A tooltip: the text (and a key cap) on a small raised plate, drawn over everything (the player's, the amp
/// room's).
pub(crate) fn draw_tip(p: &Painter, label: &str, key: &str, pos: Pos2, anchor: Align2, area: Rect) {
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

/// Give the single notes in [t0, t1) one family of techniques, in turn (screenshot staging, `shot_techs`): bends of a
/// half, whole and step and a half; slides up and down and an unpitched one; palm and fret-hand mutes; natural and
/// pinch harmonics; tremolo and vibrato; slap and pop; tap, hammer-on and pull-off. Sustains (bends, slides,
/// tremolo, vibrato) run to just before the string's next note, at most 0.7 s.
fn techify(arr: &mut Arrangement, kind: &str, t0: f64, t1: f64) {
    let n = arr.notes.len();
    let idx: Vec<usize> = (0..n)
        .filter(|&i| arr.notes[i].time >= t0 && arr.notes[i].time < t1)
        .collect();
    for (k, &i) in idx.iter().enumerate() {
        let next = arr.notes[i + 1..]
            .iter()
            .find(|m| m.string == arr.notes[i].string)
            .map_or(f64::INFINITY, |m| m.time);
        let room = (next - arr.notes[i].time - 0.08).clamp(0.0, 0.7);
        let nt = &mut arr.notes[i];
        let fret = nt.fret;
        let mut sus = false;
        match kind {
            "bends" => {
                nt.techniques.insert(Techs::BEND);
                nt.bend = [1.0, 2.0, 3.0][k % 3];
                nt.fret = fret.max(3);
                sus = true;
            }
            "slides" => {
                if k % 3 == 2 {
                    nt.techniques.insert(Techs::UNPITCHED_SLIDE);
                    nt.slide_unpitch_to = (fret - 3).max(1);
                } else {
                    nt.techniques.insert(Techs::SLIDE);
                    nt.slide_to = if k % 3 == 0 { fret + 2 } else { (fret - 2).max(1) };
                }
                nt.fret = fret.max(1);
                sus = true;
            }
            "mutes" => nt.techniques.insert(if k % 2 == 0 {
                Techs::PALM_MUTE
            } else {
                Techs::FRET_HAND_MUTE
            }),
            "harmonics" => {
                if k % 2 == 0 {
                    nt.techniques.insert(Techs::HARMONIC);
                    nt.fret = [12, 7, 5][(k / 2) % 3];
                } else {
                    nt.techniques.insert(Techs::PINCH_HARMONIC);
                }
            }
            "tremolo" => {
                nt.techniques.insert(if k % 2 == 0 {
                    Techs::TREMOLO
                } else {
                    Techs::VIBRATO
                });
                sus = true;
            }
            "slap" => nt
                .techniques
                .insert(if k % 2 == 0 { Techs::SLAP } else { Techs::POP }),
            "taps" => nt
                .techniques
                .insert([Techs::TAP, Techs::HAMMER_ON, Techs::PULL_OFF][k % 3]),
            _ => {}
        }
        if sus && room > 0.15 {
            nt.sustain = nt.sustain.max(room);
            nt.techniques.insert(Techs::SUSTAIN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::snap_at;

    #[test]
    fn a_closed_results_card_stays_closed_until_the_next_run() {
        use super::{keep_dismissed, summary_shows};
        // the song over, something judged: the card
        assert!(summary_shows(false, false, true, true));
        // closed: the page stays as it was, stopped at the end
        assert!(!summary_shows(true, false, true, true));
        // still closed while paused at the end, frame after frame (nothing ended anew)
        let mut d = true;
        for _ in 0..3 {
            d = keep_dismissed(d, false, false);
        }
        assert!(d && !summary_shows(d, false, true, true));
        // played again (Play again, Space, or a seek and play): the closing is forgotten, and the run's end shows it
        d = keep_dismissed(d, true, false);
        assert!(!d);
        assert!(summary_shows(d, false, true, true));
        // a run ending also shows it again (a recorded end, even without a frame of playing)
        assert!(!keep_dismissed(true, false, true));
        // never while playing, counting in, short of the end, or with nothing played
        assert!(!summary_shows(false, true, true, true));
        assert!(!summary_shows(false, false, false, true));
        assert!(!summary_shows(false, false, true, false));
    }

    #[test]
    fn esc_closes_the_results_card_first() {
        use super::{Key, summary_key};
        // the card up: Esc is the card's (not "library"), Space and Backspace keep their meaning
        assert_eq!(summary_key(Key::Escape, true), Some("summary_close"));
        assert_eq!(summary_key(Key::Space, true), None);
        assert_eq!(summary_key(Key::Backspace, true), None);
        // no card (or closed): Esc is back to the library again
        assert_eq!(summary_key(Key::Escape, false), None);
    }

    #[test]
    fn the_transpose_stepper_loses_its_label_before_it_goes() {
        use super::{TransposeForm, transpose_form};
        // (60 across kept for what follows it)
        let f = |room| transpose_form(room, 150.0, 80.0, 60.0);
        assert_eq!(f(400.0), TransposeForm::Full);
        assert_eq!(f(210.0), TransposeForm::Full);
        assert_eq!(f(209.0), TransposeForm::Compact);
        assert_eq!(f(140.0), TransposeForm::Compact);
        // − and + stay when what follows doesn't fit (P37.9)
        assert_eq!(f(80.0), TransposeForm::Compact);
        assert_eq!(f(79.0), TransposeForm::Hidden);
        assert_eq!(f(0.0), TransposeForm::Hidden);
    }

    #[test]
    fn the_rec_key_is_r() {
        assert_eq!(super::r_key(false, false), super::hud::REC_ACT);
        assert_eq!(super::hud::REC_ACT, "rec:press");
        assert_eq!(super::r_key(false, true), "rec:stop");
        assert_eq!(super::r_key(true, false), "take:record");
    }

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

/// The no-audio card's reason in plain words (P37.6b: "its Vorbis stream ended early" meant nothing to a newcomer);
/// the decoder's own words stay in the log.
pub(crate) fn plain_audio_error(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("no audio") {
        "the song file has no audio in it".into()
    } else if [
        "vorbis",
        "ogg",
        "wem",
        "ended early",
        "decode",
        "damaged",
        "corrupt",
        "codebook",
    ]
    .iter()
    .any(|w| l.contains(w))
    {
        "the file's audio looks damaged, so it can't be played".into()
    } else if l.contains("key") {
        "the file needs the RS keys to play its audio".into()
    } else {
        "the file's audio couldn't be read".into()
    }
}

#[cfg(test)]
mod plain_words_tests {
    #[test]
    fn the_no_audio_reason_is_in_plain_words() {
        let p = super::plain_audio_error;
        assert_eq!(
            p("its Vorbis stream ended early (a damaged file?)"),
            "the file's audio looks damaged, so it can't be played"
        );
        assert_eq!(p("the song has no audio"), "the song file has no audio in it");
        assert!(!p("wem: setup data ended early").contains("wem"));
    }
}
