//! The 3D highway (owner request, D32; rebuilt as RS2014's in P30, D78): a true 3D world in string-gap
//! units seen through one pinhole camera (`view`), notes travelling on their own strings straight to their frets,
//! chords as upright glass boxes over the hand's window, the anchor zones scrolling in on the floor, strings in three
//! states with additive halos, hits flashing the fret wires. Measured from RS footage and the game's own object
//! data (research folder `research/spec/00..05`); our own drawing code, colours as numbers (D35). "Invert strings"
//! puts the low E at the bottom (RS's own option). Notes take the scorer's judgements (the tab's glyph
//! groups). The camera follows the hand (`qrs_analysis::hwcamera::RsCamera`), or the planned "Steady" one (D32).

use std::sync::OnceLock;
use std::time::Instant;

/// The effects' clock (seconds): a frame-sequence render's own time while one runs (so effects play frame by frame),
/// else the wall clock.
fn clock() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    crate::shotclock::shot_now().unwrap_or_else(|| START.get_or_init(Instant::now).elapsed().as_secs_f64())
}

use eframe::egui;
use egui::{Align2, CursorIcon, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_analysis::Judgement;
use qrs_analysis::chordname;
use qrs_analysis::hwcamera::{self, CamOpts, CamPlan, RsCamera};
use qrs_analysis::keyguess::Key;
use qrs_formats::Techs;
use qrs_ui::icons::{self, Icon};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};

mod gfx;
mod notes;
mod scene;
mod skin;
mod view;
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;
pub use skin::{Backdrop, GEM_SIZES, NoteColours};

use super::{Chart, ipos, mid, mid_y, snap};
use crate::library::txt;

/// Note speeds (relative; `SPEED_DEFAULT` shows as 100%): they spread notes in depth, never change when they show.
pub const SPEEDS: [f32; 7] = [0.4, 0.5, 0.65, 0.8, 1.0, 1.25, 1.6];
pub const SPEED_DEFAULT: usize = 3;
/// The camera's temper: how long it holds a view before moving.
/// The camera: RS's own (a slow spring after the hand's windows, fitted to the owner's video), or the planned
/// one (holds still, moves only in gaps; D32).
pub const CAMERAS: [&str; 2] = ["RS", "Steady"];
pub const CAMERA_DEFAULT: usize = 0;
/// Note sizes: how much closer the camera comes than RS's (P37.6d, D143); `SIZE_DEFAULT` (1) is RS's.
/// Config "hw_size" holds the percentage (100).
pub const SIZES: [f32; 5] = [0.85, 1.0, 1.25, 1.5, 1.8];
pub const SIZE_DEFAULT: usize = 1;

/// The note size step in config ("hw_size": its percentage, a number or a string; any other: the nearest step).
/// The note boxes' size from the config (`hw_gem`, a percentage, number or text; P43.3): an index in `GEM_SIZES`.
pub fn gem_from(cfg: &qrs_library::Config) -> usize {
    let pct = match cfg.get("hw_gem") {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.trim().trim_end_matches('%').parse().ok(),
        _ => None,
    };
    let Some(pct) = pct else {
        return skin::GEM_DEFAULT;
    };
    (0..GEM_SIZES.len())
        .min_by(|&a, &b| {
            (GEM_SIZES[a] as f64 * 100.0 - pct)
                .abs()
                .total_cmp(&(GEM_SIZES[b] as f64 * 100.0 - pct).abs())
        })
        .unwrap_or(skin::GEM_DEFAULT)
}

pub fn size_from(cfg: &qrs_library::Config) -> usize {
    let pct = match cfg.get("hw_size") {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.trim().trim_end_matches('%').parse().ok(),
        _ => None,
    };
    let Some(pct) = pct else {
        return SIZE_DEFAULT;
    };
    let off = |i: usize| (SIZES[i] as f64 * 100.0 - pct).abs();
    (0..SIZES.len())
        .min_by(|&a, &b| off(a).total_cmp(&off(b)))
        .unwrap_or(SIZE_DEFAULT)
}

/// A note size as the toolbar and Settings show it.
pub fn size_label(i: usize) -> String {
    format!("{}%", (SIZES[i.min(SIZES.len() - 1)] * 100.0).round() as i64)
}

/// A note size's percentage (for config).
pub fn size_pct(i: usize) -> i64 {
    (SIZES[i.min(SIZES.len() - 1)] * 100.0).round() as i64
}

/// The streak readout at the highway's top left (P37.6d, D144): the multiplier and the streak, in the score tile's
/// heat, fading out with it when the streak breaks. Set by the player each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hud {
    pub streak: u32,
    /// x1..x4; 0 when there's no score (a practice run): the streak shows alone.
    pub mult: u32,
    /// The score tile's streak level (0 below 10 notes, 1..=7 by tier; fractional while it climbs or cools).
    pub heat: f32,
}

/// What the player should do after a frame of the highway: actions, and a tooltip to show.
pub struct HwOut {
    pub acts: Vec<&'static str>,
    pub tip: Option<(String, String, Pos2, Align2)>,
}

/// A note as the highway draws it (from the tab's glyphs, so it shares the scorer's groups).
struct HwNote {
    t: f64,
    string: i32,
    fret: i32,
    sus: f64,
    group: Option<usize>,
    tech: Techs,
    slide_to: i32,
    bend: f64,
    /// Its chord in `Cache::chords`.
    chord: Option<usize>,
    /// The first note on its fret since the hand's window changed: its fret number shows on the floor.
    num: bool,
}

/// A chord's frame (or an arpeggio's shape).
struct HwChord {
    t: f64,
    lo: i32,
    hi: i32,
    name: String,
    /// The same chord again in the same hand shape: RS draws it as an empty frame.
    repeat: bool,
    /// Fingers and frets per string (-1 none).
    fingers: Vec<i32>,
    frets: Vec<i32>,
    /// The hand shape's end: a chord's frame waits at the strings until then.
    hold_end: f64,
}

struct Cache {
    id: u64,
    camera: usize,
    /// The key the chord names were numbered in (None: as charted).
    numerals: Option<Key>,
    plan: CamPlan,
    /// RS's camera over the part.
    rs: RsCamera,
    /// Sorted by time; chord notes flattened.
    notes: Vec<HwNote>,
    chords: Vec<HwChord>,
    arps: Vec<HwChord>,
    /// (start, end, fret, width).
    anchors: Vec<(f64, f64, i32, i32)>,
    /// (time, is a bar line).
    beats: Vec<(f64, bool)>,
    /// Each scorer group's note time.
    group_t: Vec<f64>,
    /// Each chord's notes (indices in `notes`).
    chord_notes: Vec<Vec<usize>>,
    /// Each phrase's start and the strings it uses (a bit per string).
    phrase_masks: Vec<(f64, u8)>,
    /// When the neck's window switches to each anchor: midway between the old anchor's last note and the new one's
    /// first (05 §1.4).
    switch: Vec<f64>,
    /// The song's note speed at this difficulty (RS's DynamicVisualDensity; 1.3 at full difficulty by default,
    /// lower = faster).
    density: f32,
    /// The song's look (skin.rs): its note material, stages and heat.
    look: skin::Look,
}

pub struct Highway {
    /// Low E at the bottom (RS's "invert strings"); off = low E on top as in RS.
    pub invert: bool,
    /// Index in `SPEEDS`.
    pub speed: usize,
    /// Index in `CAMERAS`.
    pub camera: usize,
    /// Index in `SIZES` (the note size: the camera's distance).
    pub size: usize,
    /// The player's full screen (the toolbar's key shows it; the player owns it).
    pub full: bool,
    /// Chord names as numbers in this key (the player's chord-names key); None: as charted.
    pub numerals: Option<Key>,
    /// The background: Live, Still or Plain (config "hw_backdrop", the toolbar's eye key, D).
    pub backdrop: Backdrop,
    /// The note colours (P43.2, config "hw_colours") and the note boxes' size (P43.3, an index in `GEM_SIZES`,
    /// config "hw_gem").
    pub colours: NoteColours,
    pub gem: usize,
    /// The streak readout (the player sets it each frame).
    pub hud: Hud,
    cache: Option<Cache>,
    /// When each scorer group was judged (for the burst / cross), cleared when its judgement goes (a seek back).
    judged: Vec<Option<(Judgement, f64)>>,
    /// The toolbar's last hover or change (it's see-through at rest, solid while in use).
    tools_used: Option<Instant>,
    /// The skin's images (loaded on the first frame) and the last frame's clock (for the look's crossfades).
    tex: Option<skin::Tex>,
    last_frame: Option<f64>,
}

/// The planned camera's options (the "Steady" camera).
fn cam_opts() -> CamOpts {
    CamOpts {
        max_frets: 12.0,
        comfort_frets: 8.0,
        min_hold: 10.0,
        move_cost: 1.6,
        ..CamOpts::default()
    }
}

impl Highway {
    pub fn new(invert: bool, speed: usize, camera: usize, backdrop: Backdrop) -> Highway {
        Highway {
            invert,
            speed: speed.min(SPEEDS.len() - 1),
            camera: camera.min(CAMERAS.len() - 1),
            size: SIZE_DEFAULT,
            full: false,
            numerals: None,
            backdrop,
            colours: NoteColours::default(),
            gem: skin::GEM_DEFAULT,
            hud: Hud::default(),
            cache: None,
            judged: Vec::new(),
            tools_used: None,
            tex: None,
            last_frame: None,
        }
    }

    /// The note size (an index in `SIZES`).
    pub fn with_size(mut self, size: usize) -> Highway {
        self.size = size.min(SIZES.len() - 1);
        self
    }

    /// One step faster (+1) or slower (-1); false at the end of the range.
    pub fn step_speed(&mut self, d: i32) -> bool {
        let n = (self.speed as i32 + d).clamp(0, SPEEDS.len() as i32 - 1) as usize;
        self.tools_used = Some(Instant::now());
        std::mem::replace(&mut self.speed, n) != n
    }

    /// One step livelier (+1) or steadier (-1); false at the end of the range.
    pub fn step_camera(&mut self, d: i32) -> bool {
        let n = (self.camera as i32 + d).clamp(0, CAMERAS.len() as i32 - 1) as usize;
        self.tools_used = Some(Instant::now());
        std::mem::replace(&mut self.camera, n) != n
    }

    /// One step bigger (+1, the camera closer) or smaller (-1); false at the end of the range.
    pub fn step_size(&mut self, d: i32) -> bool {
        let n = (self.size as i32 + d).clamp(0, SIZES.len() as i32 - 1) as usize;
        self.tools_used = Some(Instant::now());
        std::mem::replace(&mut self.size, n) != n
    }

    /// The next background (Live, Still, Plain, Live...).
    pub fn cycle_backdrop(&mut self) -> Backdrop {
        self.tools_used = Some(Instant::now());
        self.backdrop = self.backdrop.next();
        self.backdrop
    }

    /// Back to the default speed or camera.
    pub fn reset(&mut self, camera: bool) {
        self.tools_used = Some(Instant::now());
        if camera {
            self.camera = CAMERA_DEFAULT;
        } else {
            self.speed = SPEED_DEFAULT;
        }
    }

    /// Back to the default note size (RS's camera distance).
    pub fn reset_size(&mut self) {
        self.tools_used = Some(Instant::now());
        self.size = SIZE_DEFAULT;
    }

    /// The note speed as a percentage of the default.
    pub fn speed_pct(&self) -> i64 {
        (SPEEDS[self.speed] / SPEEDS[SPEED_DEFAULT] * 100.0).round() as i64
    }

    fn cache(&mut self, chart: &Chart) -> &Cache {
        if self
            .cache
            .as_ref()
            .is_none_or(|c| c.id != chart.id || c.camera != self.camera || c.numerals != self.numerals)
        {
            let numerals = self.numerals;
            let name_of = |name: &str| match numerals {
                Some(key) if !name.trim().is_empty() => chordname::name_in_key(name.trim(), key),
                _ => name.to_string(),
            };
            let shape_at = |t: f64| {
                let i = chart.shape_starts.partition_point(|&s| s <= t + 1e-3);
                (i > 0).then(|| &chart.shapes[i - 1]).filter(|s| t < s.end - 1e-3)
            };
            let any_hd = chart.chords.iter().any(|c| c.high_density);
            let opens = chart.arr.open_string_midi();
            let mut chords: Vec<HwChord> = Vec::with_capacity(chart.chords.len());
            for (i, ch) in chart.chords.iter().enumerate() {
                let fretted = ch.notes.iter().map(|n| n.fret).filter(|&f| f > 0);
                // the chart's own flag (RS's high density), but only for the same notes again (P43.1: a CDLC flagged
                // a whole verse, C5 to G5 to F5, and the changes were drawn as empty frames); charts without any
                // flags: the same chord again in the same hand shape
                let repeat = if any_hd {
                    ch.high_density && i > 0 && same_notes(&chart.chords[i - 1], ch)
                } else {
                    i > 0 && {
                        let pv = &chart.chords[i - 1];
                        pv.chord_id == ch.chord_id
                            && shape_at(ch.time).is_some_and(|s| pv.time >= s.start - 1e-3)
                    }
                };
                chords.push(HwChord {
                    t: ch.time,
                    lo: fretted.clone().min().unwrap_or(0),
                    hi: fretted.max().unwrap_or(0),
                    name: name_of(&if ch.name.trim().is_empty() {
                        name_from_notes(ch, &opens)
                    } else {
                        ch.name.clone()
                    }),
                    repeat,
                    fingers: ch.fingers.clone(),
                    frets: ch.frets.clone(),
                    hold_end: shape_at(ch.time).map_or(ch.time, |s| s.end),
                });
            }
            chords.sort_by(|a, b| a.t.total_cmp(&b.t));
            let arps = chart
                .shapes
                .iter()
                .filter(|s| s.arpeggio)
                .map(|s| {
                    let fretted = s.frets.iter().copied().filter(|&f| f > 0);
                    HwChord {
                        t: s.start,
                        lo: fretted.clone().min().unwrap_or(0),
                        hi: fretted.max().unwrap_or(0),
                        name: name_of(&s.name),
                        repeat: false,
                        fingers: s.fingers.clone(),
                        frets: s.frets.clone(),
                        hold_end: s.end,
                    }
                })
                .collect();
            let mut notes: Vec<HwNote> = chart
                .glyphs
                .iter()
                .map(|g| HwNote {
                    t: g.t,
                    string: g.string,
                    fret: g.fret,
                    sus: g.sus,
                    group: g.group,
                    tech: g.tech,
                    slide_to: g.slide_to,
                    bend: g.bend,
                    chord: g
                        .in_chord
                        .then(|| chords.partition_point(|c| c.t < g.t - 3e-3))
                        .filter(|&k| chords.get(k).is_some_and(|c| (c.t - g.t).abs() < 3e-3)),
                    num: false,
                })
                .collect();
            notes.sort_by(|a, b| a.t.total_cmp(&b.t));
            let mut an: Vec<_> = chart
                .arr
                .anchors
                .iter()
                .map(|a| (a.start, a.end, a.fret, a.width))
                .collect();
            an.sort_by(|a, b| a.0.total_cmp(&b.0));
            // each window ends where the next starts (charts stitched from difficulty levels can run ends past their
            // successors)
            for i in 0..an.len() {
                let next = an.get(i + 1).map_or(f64::INFINITY, |a| a.0);
                an[i].1 = next.max(an[i].0);
            }
            let ev = hwcamera::events(
                &notes
                    .iter()
                    .map(|n| (n.t, n.fret, n.t + n.sus))
                    .collect::<Vec<_>>(),
                &an.iter().map(|a| (a.0, a.2, a.3)).collect::<Vec<_>>(),
            );
            let plan = CamPlan::new(&ev, chart.length, &cam_opts());
            // (charts without phrases: section starts)
            let phrases: Vec<f64> = if chart.arr.phrases.is_empty() {
                chart.sections.iter().map(|s| s.start).collect()
            } else {
                chart.arr.phrases.clone()
            };
            let rs = RsCamera::new(&an, &phrases, chart.length);
            // floor numbers: the first single note on each fret per anchor
            {
                let mut ai = 0;
                let mut seen: Vec<i32> = Vec::new();
                for n in notes.iter_mut() {
                    while ai + 1 < an.len() && an[ai + 1].0 <= n.t + 1e-3 {
                        ai += 1;
                        seen.clear();
                    }
                    if n.chord.is_none() && n.fret > 0 && !seen.contains(&n.fret) {
                        seen.push(n.fret);
                        n.num = true;
                    }
                }
            }
            let mut chord_notes = vec![Vec::new(); chords.len()];
            for (i, n) in notes.iter().enumerate() {
                if let Some(k) = n.chord {
                    chord_notes[k].push(i);
                }
            }
            let mut phrase_masks: Vec<(f64, u8)> = Vec::with_capacity(phrases.len());
            for (k, &p0) in phrases.iter().enumerate() {
                let p1 = phrases.get(k + 1).copied().unwrap_or(f64::INFINITY);
                let i = notes.partition_point(|n| n.t < p0 - 1e-3);
                let mask = notes[i..]
                    .iter()
                    .take_while(|n| n.t < p1 - 1e-3)
                    .fold(0u8, |m, n| m | (1 << n.string.clamp(0, 5)));
                phrase_masks.push((p0, mask));
            }
            let switch: Vec<f64> = an
                .iter()
                .enumerate()
                .map(|(k, a)| {
                    if k == 0 {
                        return f64::MIN;
                    }
                    let i = notes.partition_point(|n| n.t < a.0 - 1e-3);
                    let first = notes.get(i).map_or(a.0, |n| n.t.min(a.1));
                    let last = if i > 0 {
                        notes[i - 1].t.max(an[k - 1].0)
                    } else {
                        an[k - 1].0
                    };
                    ((first + last) / 2.0).min(a.0)
                })
                .collect();
            self.cache = Some(Cache {
                id: chart.id,
                camera: self.camera,
                numerals,
                plan,
                rs,
                notes,
                chords,
                arps,
                anchors: an,
                beats: chart.beats.iter().map(|b| (b.time, b.measure >= 0)).collect(),
                chord_notes,
                phrase_masks,
                switch,
                density: chart.arr.visual_density_at(chart.arr.mastery),
                look: skin::Look::new(&chart.arr),
                group_t: chart.glyphs.iter().fold(Vec::new(), |mut v, g| {
                    if let Some(k) = g.group {
                        if v.len() <= k {
                            v.resize(k + 1, f64::NAN);
                        }
                        if v[k].is_nan() {
                            v[k] = g.t;
                        }
                    }
                    v
                }),
            });
        }
        self.cache.as_ref().expect("cached")
    }

    /// The camera's planned moves in this part (for the toast).
    pub fn moves(&mut self, chart: &Chart) -> usize {
        self.cache(chart).plan.moves()
    }

    /// Note when each group got its judgement, so the burst or cross plays once from that moment.
    fn track(&mut self, states: &[Option<Judgement>], now: f64) {
        let group_t = self.cache.as_ref().map(|c| c.group_t.clone()).unwrap_or_default();
        self.judged.resize(states.len(), None);
        // many at once is a load or a seek, not playing: those are dated by their notes' times (their effects are over,
        // or as far along as they'd be)
        let fresh = self
            .judged
            .iter()
            .zip(states)
            .filter(|(j, s)| s.is_some() && j.is_none_or(|(k, _)| Some(k) != **s))
            .count();
        let at = |i: usize| {
            if fresh <= 8 {
                return clock();
            }
            let ago = group_t.get(i).map_or(10.0, |t| {
                if t.is_nan() {
                    10.0
                } else {
                    (now - t).clamp(0.0, 10.0)
                }
            });
            clock() - ago
        };
        for (i, (j, s)) in self.judged.iter_mut().zip(states).enumerate() {
            match (*s, *j) {
                (None, _) => *j = None,
                (Some(k), None) => *j = Some((k, at(i))),
                (Some(k), Some((was, _))) if k != was => *j = Some((k, at(i))),
                _ => {}
            }
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        chart: &Chart,
        states: &[Option<Judgement>],
        now: f64,
        well: Rect,
    ) -> HwOut {
        self.cache(chart);
        self.track(states, now);
        let marks: Vec<Option<(Judgement, f32)>> = self
            .judged
            .iter()
            .map(|j| j.map(|(k, at)| (k, (clock() - at).max(0.0) as f32)))
            .collect();
        // the look: the streak so far (notes judged in a row, up to now) heats the stage (only a Live one)
        let live = self.backdrop == Backdrop::Live;
        let streak = if !live {
            0
        } else {
            let c = self.cache.as_ref().expect("cached");
            let mut order: Vec<usize> = (0..states.len().min(c.group_t.len()))
                .filter(|&i| c.group_t[i] <= now && !c.group_t[i].is_nan())
                .collect();
            order.sort_by(|a, b| c.group_t[*a].total_cmp(&c.group_t[*b]));
            order
                .iter()
                .rev()
                .skip_while(|&&i| states[i].is_none())
                .take_while(|&&i| states[i].is_some_and(|j| j != Judgement::Miss))
                .count()
        };
        let t_clock = clock();
        let dt = self.last_frame.map_or(10.0, |l| (t_clock - l).clamp(0.0, 10.0)) as f32;
        self.last_frame = Some(t_clock);
        if self.tex.is_none() {
            self.tex = Some(skin::Tex::load(ui.ctx()));
        }
        if let Some(c) = self.cache.as_mut() {
            c.look.step(now, streak, dt, self.backdrop);
        }
        // the amp breathes with the playing: a pulse on each hit (Live only: Still's amp stays as it is)
        let pulse = if !live {
            0.0
        } else {
            marks
                .iter()
                .flatten()
                .filter(|(j, _)| *j != Judgement::Miss)
                .map(|(_, age)| (1.0 - age / 0.35).max(0.0))
                .fold(0.0f32, f32::max)
        };
        let backdrop = self.backdrop;
        let invert = self.invert;
        let speed = SPEEDS[self.speed] / SPEEDS[SPEED_DEFAULT];
        let n_str = chart.n_strings.clamp(4, 6);
        let camera = self.camera;
        let zoom = SIZES[self.size.min(SIZES.len() - 1)];
        let tex = self.tex.take().expect("loaded");
        let (colours, gem) = (
            self.colours,
            skin::GEM_SIZES[self.gem.min(skin::GEM_SIZES.len() - 1)],
        );
        let c = self.cache(chart);
        let p = ui.painter().with_clip_rect(well);
        // the camera as (fret wire at the screen's middle, frets across)
        let cam = if camera == 0 {
            c.rs.at(now)
        } else {
            let v = c.plan.at(now);
            let (fl, fr) = (hwcamera::fret_at(v.l), hwcamera::fret_at(v.r));
            let across = (fr - fl).clamp(10.55, 14.0);
            (((fl + fr) / 2.0 + (0.5 - 0.459) * across) as f32, across as f32)
        };
        // c: seconds from the strike plane to the camera plane, from the song's DynamicVisualDensity (spec 01);
        // the speed setting divides it (a faster speed spreads notes further apart), the far end stays 4.87 D
        let c_secs = view::C_PER_DVD * c.density.max(0.3) as f64 / speed.max(0.1) as f64;
        let mut v = view::View::new(well, cam, zoom, now, c_secs, n_str, invert);
        v.keep_numbers(cam, zoom);
        let glow = (0.35 + 0.45 * c.look.heat + 0.6 * pulse) * c.look.dim();
        scene::draw(
            &p,
            &v,
            c,
            &marks,
            &scene::Skin {
                tex: &tex,
                glow,
                clock: t_clock,
                backdrop,
                colours,
                gem,
            },
        );
        self.tex = Some(tex);
        self.draw_hud(ui, well);
        let out = self.tools(ui, well);
        ui.ctx().request_repaint();
        out
    }

    /// The streak readout: a small plate at the well's top left, level with the toolbar, over the stage's sky (the
    /// highway runs to the right of the middle, so no note passes there). "×3" in the heat's colour, then the count
    /// and "streak"; it appears with the first tier (10 notes, x2) and fades as the arc cools when the streak breaks.
    fn draw_hud(&self, ui: &mut Ui, well: Rect) {
        let Hud { streak, mult, heat } = self.hud;
        let show = heat.clamp(0.0, 1.0);
        if show <= 0.01 {
            return;
        }
        let ctx = ui.ctx().clone();
        let th = t();
        let dark = qrs_ui::theme::is_dark();
        let hot = |base| qrs_ui::streak::heat_color(heat, base, dark);
        let pad = px(10.0);
        let h = snap(px(26.0) + 2.0 * px(5.0));
        let mult_s = (mult > 1).then(|| format!("×{mult}"));
        let mult_st = Style::new(17.0, 800, th.text);
        let num_st = Style::new(15.0, 800, th.text);
        let cap_st = Style::new(11.0, 600, th.caption);
        let num = streak.to_string();
        let mw = mult_s.as_ref().map_or(0.0, |m| {
            text::width(&ctx, m, mult_st).ceil() + px(9.0) + px(1.0) + px(9.0)
        });
        let w = snap(
            2.0 * pad
                + mw
                + text::width(&ctx, &num, num_st).ceil()
                + px(5.0)
                + text::width(&ctx, "streak", cap_st).ceil(),
        );
        let plate = Rect::from_min_size(ipos(well.min.x + px(16.0), well.min.y + px(12.0)), vec2(w, h));
        let mut hu = ui.new_child(egui::UiBuilder::new().max_rect(plate));
        hu.multiply_opacity(show);
        let p = hu.painter().clone();
        paint::floating(&p, plate, Some(12.0));
        let cy = mid_y(plate);
        let mut x = plate.min.x + pad;
        if let Some(m) = &mult_s {
            let r = txt(
                &p,
                m,
                ipos(x, cy),
                17.0,
                hot(th.accent_ink),
                800,
                Align2::LEFT_CENTER,
                None,
            );
            x = snap(r.max.x + px(9.0));
            p.rect_filled(
                Rect::from_min_size(ipos(x, plate.min.y + px(8.0)), vec2(px(1.0), snap(h - px(16.0)))),
                0.0,
                th.line,
            );
            x = snap(x + px(1.0) + px(9.0));
        }
        let r = txt(
            &p,
            &num,
            ipos(x, cy),
            15.0,
            hot(th.text),
            800,
            Align2::LEFT_CENTER,
            None,
        );
        txt(
            &p,
            "streak",
            ipos(r.max.x + px(5.0), cy),
            11.0,
            hot(th.caption),
            600,
            Align2::LEFT_CENTER,
            None,
        );
    }

    /// The highway's toolbar, floating at its top right like the tab's: note speed, note size (the camera's distance)
    /// and the camera's temper as sunken pills (− amount +), then invert strings, chord names (as charted or numbers in the key), the background (an
    /// eye: Live, Still, Plain) and full screen as round keys (the RS Highway key, back to the tab, is the player's). See-through at rest, solid while the pointer is on it or just after a change.
    fn tools(&mut self, ui: &mut Ui, well: Rect) -> HwOut {
        let ctx = ui.ctx().clone();
        let th = t();
        let mut out = HwOut {
            acts: Vec::new(),
            tip: None,
        };
        let (key, gap, pad, div_gap) = (px(26.0), px(6.0), px(5.0), px(8.0));
        const LABELS: [&str; 3] = ["Speed", "Size", "Camera"];
        let rs_w = super::PlayerView::rs_highway_w(&ctx);
        // (the pills' widths: Speed and Size hold a percentage, Camera its name; a narrow well drops the labels and
        // narrows the pills, the tips still say what each sets)
        let width = |labels: bool, pills: [f32; 3]| {
            let label_w: f32 = if labels {
                LABELS
                    .iter()
                    .map(|l| text::width(&ctx, l, Style::new(11.0, 600, th.caption)).ceil() + px(7.0))
                    .sum()
            } else {
                0.0
            };
            snap(
                2.0 * pad
                    + rs_w
                    + div_gap
                    + label_w
                    + pills.iter().sum::<f32>()
                    + 2.0 * gap
                    + 2.0 * div_gap
                    + px(1.0)
                    + 4.0 * (key + px(4.0)),
            )
        };
        let roomy = [px(96.0), px(96.0), px(112.0)];
        let (labels, pills) = if width(true, roomy) <= well.width() - px(32.0) {
            (true, roomy)
        } else {
            (false, [px(84.0), px(84.0), px(104.0)])
        };
        let w = width(labels, pills);
        let h = snap(key + 2.0 * pad);
        let plate = Rect::from_min_size(ipos(well.max.x - px(16.0) - w, well.min.y + px(12.0)), vec2(w, h));
        let over = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|h| plate.contains(h));
        if over {
            self.tools_used = Some(Instant::now());
        }
        let busy = self.tools_used.is_some_and(|t| t.elapsed().as_secs_f32() < 1.4);
        let solid = ctx.animate_bool_with_time(egui::Id::new("hw-tools"), over || busy, 0.2);
        let mut tu = ui.new_child(egui::UiBuilder::new().max_rect(plate));
        tu.multiply_opacity(0.4 + 0.6 * solid);
        let p = tu.painter().clone();
        paint::floating(&p, plate, Some(12.0));
        let cy = mid_y(plate);
        let tip_at = |r: Rect| ipos(r.max.x, plate.max.y + px(8.0));
        let mut x = plate.min.x + pad;
        // the RS Highway key first, lit (back to the tab)
        {
            let r = Rect::from_min_size(ipos(x, cy - snap(key / 2.0)), vec2(rs_w, key));
            let resp = interact(&tu, r, ("hw-tool", "rs-highway"), Sense::click());
            super::PlayerView::draw_rs_highway(&p, r, true, resp.hovered());
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                out.tip = Some(("Back to the tab".into(), "V".into(), tip_at(r), Align2::RIGHT_TOP));
            }
            if resp.clicked() {
                out.acts.push("view");
            }
            x = r.max.x + div_gap;
        }
        let steppers: [(&str, &str, String, [(&str, &'static str); 2], &'static str); 3] = [
            (
                LABELS[0],
                "Note speed",
                format!("{}%", self.speed_pct()),
                [("Slower notes", "hw_slower"), ("Faster notes", "hw_faster")],
                "hw_speed_reset",
            ),
            (
                LABELS[1],
                "Note size: the camera closer (bigger frets and notes, fewer frets in view) or further",
                size_label(self.size),
                [
                    ("Smaller notes (camera further)", "hw_smaller"),
                    ("Bigger notes (camera closer)", "hw_bigger"),
                ],
                "hw_size_reset",
            ),
            (
                "Camera",
                "Camera: Steady holds a view longest, Lively moves more to stay close",
                CAMERAS[self.camera].to_string(),
                [
                    ("Steadier camera", "hw_steadier"),
                    ("Livelier camera", "hw_livelier"),
                ],
                "hw_camera_reset",
            ),
        ];
        for (i, (label, what, value, keys, reset)) in steppers.iter().enumerate() {
            // what it sets: a plain label before the pill
            let lab = if labels {
                let lw = text::width(&ctx, label, Style::new(11.0, 600, th.caption)).ceil() + px(4.0);
                let lab = Rect::from_min_size(ipos(x, cy - snap(key / 2.0)), vec2(lw, key));
                txt(
                    &p,
                    label,
                    ipos(lab.min.x, cy),
                    11.0,
                    th.caption,
                    600,
                    Align2::LEFT_CENTER,
                    None,
                );
                lab.max.x + px(3.0)
            } else {
                x
            };
            let r = Rect::from_min_size(ipos(lab, cy - snap(key / 2.0)), vec2(pills[i], key));
            paint::inset(&p, r, None, 3.0, Fill::Base);
            // − and + at the pill's ends, the amount between (a click on it puts it back)
            let minus = Rect::from_min_size(r.min, Vec2::splat(key));
            let plus = Rect::from_min_size(ipos(r.max.x - key, r.min.y), Vec2::splat(key));
            for (b, ic, (tip, act)) in [(minus, Icon::Minus, keys[0]), (plus, Icon::Plus, keys[1])] {
                let resp = interact(&tu, b, ("hw-tool", act), Sense::click());
                let ink = if resp.is_pointer_button_down_on() {
                    th.accent
                } else if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                    p.circle_filled(mid(b), snap(key / 2.0 - px(3.0)), th.line);
                    th.text
                } else {
                    th.caption
                };
                icons::icon(&p, ic, mid(b), 12.0, ink);
                if resp.clicked() {
                    out.acts.push(act);
                }
                if resp.hovered() {
                    out.tip = Some((tip.to_string(), String::new(), tip_at(b), Align2::RIGHT_TOP));
                }
            }
            let mid_r = Rect::from_min_max(pos2(minus.max.x, r.min.y), pos2(plus.min.x, r.max.y));
            let resp = interact(&tu, mid_r, ("hw-tool-read", *label), Sense::click());
            let ink = if resp.hovered() { th.text } else { th.accent_ink };
            txt(&p, value, mid(mid_r), 12.0, ink, 800, Align2::CENTER_CENTER, None);
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                let back = match i {
                    0 => "click: 100%",
                    1 => "click: 100% (RS's)",
                    _ => "click: RS",
                };
                out.tip = Some((what.to_string(), back.into(), tip_at(mid_r), Align2::RIGHT_TOP));
            }
            if resp.clicked() {
                out.acts.push(reset);
            }
            x = r.max.x + if i + 1 < steppers.len() { gap } else { 0.0 };
        }
        x = snap(x + div_gap);
        p.rect_filled(
            Rect::from_min_size(ipos(x, plate.min.y + px(8.0)), vec2(px(1.0), snap(h - px(16.0)))),
            0.0,
            th.line,
        );
        x = snap(x + px(1.0) + div_gap);
        let toggles = [
            (Icon::Invert, self.invert, "Invert strings", "I", "hw_invert"),
            (
                Icon::Music,
                self.numerals.is_some(),
                if self.numerals.is_some() {
                    "Chord names: numbers in the song's key (click: as charted)"
                } else {
                    "Chord names as charted (click: numbers in the song's key)"
                },
                "K",
                "chord_numerals",
            ),
            (
                Icon::Eye,
                self.backdrop != Backdrop::Live,
                match self.backdrop {
                    Backdrop::Live => "Background: Live (click: Still, one wallpaper)",
                    Backdrop::Still => "Background: Still (click: Plain, dark)",
                    Backdrop::Plain => "Background: Plain (click: Live)",
                },
                "D",
                "hw_backdrop",
            ),
            (
                if self.full { Icon::Shrink } else { Icon::Expand },
                self.full,
                if self.full {
                    "Leave full screen"
                } else {
                    "Full screen"
                },
                if self.full { "F / Esc" } else { "F" },
                "tab_full",
            ),
        ];
        for (icon, on, label, keycap, act) in toggles {
            let r = Rect::from_min_size(ipos(x, cy - snap(key / 2.0)), Vec2::splat(key));
            let resp = interact(&tu, r, ("hw-tool", act), Sense::click());
            let depth = if resp.is_pointer_button_down_on() {
                4.0
            } else {
                3.0
            };
            paint::inset(&p, r, None, depth, Fill::Base);
            if on {
                paint::ring(&p, r, None, 1.5, th.accent, true);
            }
            let ink = if on {
                th.accent_ink
            } else if resp.hovered() {
                th.text
            } else {
                th.caption
            };
            icons::icon(&p, icon, mid(r), 13.0, ink);
            if resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                out.tip = Some((label.into(), keycap.into(), tip_at(r), Align2::RIGHT_TOP));
            }
            if resp.clicked() {
                out.acts.push(act);
            }
            x = snap(r.max.x + px(4.0));
        }
        out
    }
}

/// A chord name as RS spells it: flats as sharps (Gb5 -> F♯5), ♯ for #.
fn sharp_name(name: &str) -> String {
    let mut out = String::new();
    let cs: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_uppercase() && cs.get(i + 1) == Some(&'b') && "ABCDEFG".contains(c) {
            let (l, sharp) = match c {
                'A' => ('G', true),
                'B' => ('A', true),
                'C' => ('B', false),
                'D' => ('C', true),
                'E' => ('D', true),
                'F' => ('E', false),
                _ => ('F', true),
            };
            out.push(l);
            if sharp {
                out.push('♯');
            }
            i += 2;
            continue;
        }
        out.push(if c == '#' { '♯' } else { c });
        i += 1;
    }
    out
}

/// The hand's window at time t (first fret, width).
fn window_at(c: &Cache, t: f64) -> Option<(i32, i32)> {
    c.anchors[..c.anchors.partition_point(|a| a.0 <= t + 1e-3)]
        .last()
        .map(|a| (a.2, a.3))
}

/// A chord's frame spans its frets and the hand's window at that time.
fn chord_span(c: &Cache, ch: &HwChord) -> (i32, i32) {
    let (mut lo, mut hi) = window_at(c, ch.t).map_or((ch.lo, ch.hi), |(f, w)| (f, f + w - 1));
    if ch.hi > 0 {
        lo = lo.min(ch.lo);
        hi = hi.max(ch.hi);
    }
    (lo.max(1), hi.max(lo.max(1)))
}

impl Cache {
    /// The hand's window shown on the neck at t: it switches midway between anchors (05 §1.4).
    fn window_now(&self, t: f64) -> Option<(i32, i32)> {
        let k = self.switch.partition_point(|&s| s <= t + 1e-3);
        (k > 0).then(|| (self.anchors[k - 1].2, self.anchors[k - 1].3))
    }
}

/// The same strings and frets as the chord before (P43.1): only then is a chart's "repeat" flag drawn as one.
fn same_notes(a: &qrs_formats::model::Chord, b: &qrs_formats::model::Chord) -> bool {
    let key = |c: &qrs_formats::model::Chord| {
        let mut v: Vec<(i32, i32)> = c.notes.iter().map(|n| (n.string, n.fret)).collect();
        v.sort_unstable();
        v
    };
    key(a) == key(b)
}

/// A chart chord with no name, named from its notes (P43.1: a CDLC's unnamed power chords left the highway without a
/// label at each change); empty when they make no chord (one note, or its octaves).
fn name_from_notes(c: &qrs_formats::model::Chord, opens: &[i32]) -> String {
    let heard: Vec<chordname::HeardNote> = c
        .notes
        .iter()
        .filter_map(|n| {
            let open = *opens.get(n.string as usize)?;
            Some(chordname::HeardNote {
                midi: open + n.fret,
                strength: 1.0,
            })
        })
        .collect();
    chordname::name(&heard).map_or_else(String::new, |g| g.name())
}

/// Round the highway's corners (P47.2, owner: the tab's well is rounded, the highway was a hard rectangle): each
/// corner outside a quarter circle of radius `r` painted in the page's colour (the theme's base, what's behind the
/// player), its edge feathered over a pixel, then a faint rim along the edge like the tab's well.
pub fn round_corners(p: &egui::Painter, well: Rect, r: f32) {
    use egui::epaint::Mesh;
    let th = t();
    let base = th.base;
    let feather = 1.0 / p.ctx().pixels_per_point();
    let n = 16;
    for (corner, centre, a0) in [
        (
            well.left_top(),
            well.left_top() + vec2(r, r),
            std::f32::consts::PI,
        ),
        (
            well.right_top(),
            well.right_top() + vec2(-r, r),
            1.5 * std::f32::consts::PI,
        ),
        (well.right_bottom(), well.right_bottom() + vec2(-r, -r), 0.0),
        (
            well.left_bottom(),
            well.left_bottom() + vec2(r, -r),
            0.5 * std::f32::consts::PI,
        ),
    ] {
        let mut m = Mesh::default();
        m.colored_vertex(corner, base);
        for k in 0..=n {
            let a = a0 + std::f32::consts::FRAC_PI_2 * k as f32 / n as f32;
            let d = vec2(a.cos(), a.sin());
            m.colored_vertex(centre + d * r, base);
            m.colored_vertex(centre + d * (r - feather), egui::Color32::TRANSPARENT);
        }
        for k in 0..n as u32 {
            let (o0, o1) = (1 + 2 * k, 3 + 2 * k);
            m.add_triangle(0, o0, o1);
            m.add_triangle(o0, o1, o1 + 1);
            m.add_triangle(o0, o1 + 1, o0 + 1);
        }
        p.add(egui::Shape::mesh(m));
    }
    p.rect_stroke(
        well,
        egui::CornerRadius::same(r.round().clamp(0.0, 255.0) as u8),
        egui::Stroke::new(1.0, gfx::alpha(egui::Color32::BLACK, 0.35)),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_formats::model::{Chord, Note};

    fn chord(notes: &[(i32, i32)], hd: bool) -> Chord {
        Chord {
            time: 0.0,
            chord_id: 0,
            name: String::new(),
            notes: notes.iter().map(|&(s, f)| Note::new(0.0, s, f)).collect(),
            techniques: Techs::default(),
            sustain: 0.0,
            frets: vec![-1; 6],
            fingers: vec![-1; 6],
            high_density: hd,
        }
    }

    /// P43.1 (a tester's All the Small Things): a run flagged as repeats that changes shape, C5 to G5 to F5.
    #[test]
    fn a_changed_shape_is_never_a_repeat_and_gets_a_name() {
        let opens = [40, 45, 50, 55, 59, 64];
        let c5 = chord(&[(1, 3), (2, 5)], true);
        let g5 = chord(&[(0, 3), (1, 5)], true);
        let f5 = chord(&[(0, 1), (1, 3)], true);
        assert!(same_notes(&c5, &c5.clone()));
        assert!(!same_notes(&c5, &g5));
        assert!(!same_notes(&g5, &f5));
        assert_eq!(name_from_notes(&c5, &opens), "C5");
        assert_eq!(name_from_notes(&g5, &opens), "G5");
        assert_eq!(name_from_notes(&f5, &opens), "F5");
        assert_eq!(name_from_notes(&chord(&[(0, 3)], false), &opens), "");
    }

    /// P43.2/P43.3: Vivid unless set otherwise, and the box size read as Settings writes it (text) or as a number.
    #[test]
    fn note_colours_and_box_size_from_the_config() {
        let mut cfg = qrs_library::Config::default();
        assert_eq!(
            NoteColours::parse(&cfg.str("hw_colours", "vivid")),
            NoteColours::Vivid
        );
        assert_eq!(NoteColours::parse("tube"), NoteColours::Tube);
        assert_eq!(NoteColours::parse("nonsense"), NoteColours::Vivid);
        assert_eq!(gem_from(&cfg), skin::GEM_DEFAULT);
        cfg.set("hw_gem", "120");
        assert_eq!(gem_from(&cfg), 2);
        cfg.set("hw_gem", 100);
        assert_eq!(gem_from(&cfg), 0);
    }
}
