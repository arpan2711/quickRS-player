//! The 3D highway (owner request, D32; rebuilt as Rocksmith 2014's in P30, D78): a true 3D world in string-gap
//! units seen through one pinhole camera (`view`), notes travelling on their own strings straight to their frets,
//! chords as upright glass boxes over the hand's window, the anchor zones scrolling in on the floor, strings in three
//! states with additive halos, hits flashing the fret wires. Measured from Rocksmith footage and the game's own object
//! data (research folder `research/spec/00..05`); our own drawing code, colours as numbers (D35). "Invert strings"
//! puts the low E at the bottom (Rocksmith's own option). Notes take the scorer's judgements (the tab's glyph
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
use qrs_analysis::hwcamera::{self, CamOpts, CamPlan, RsCamera};
use qrs_formats::Techs;
use qrs_ui::icons::{self, Icon};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};

mod gfx;
mod notes;
mod scene;
mod view;
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::{Chart, ipos, mid, mid_y, snap};
use crate::library::txt;

/// Note speeds (relative; `SPEED_DEFAULT` shows as 100%): they spread notes in depth, never change when they show.
pub const SPEEDS: [f32; 7] = [0.4, 0.5, 0.65, 0.8, 1.0, 1.25, 1.6];
pub const SPEED_DEFAULT: usize = 3;
/// The camera's temper: how long it holds a view before moving.
/// The camera: Rocksmith's own (a slow spring after the hand's windows, fitted to the owner's video), or the planned
/// one (holds still, moves only in gaps; D32).
pub const CAMERAS: [&str; 2] = ["Rocksmith", "Steady"];
pub const CAMERA_DEFAULT: usize = 0;

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
    /// The same chord again in the same hand shape: Rocksmith draws it as an empty frame.
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
    plan: CamPlan,
    /// Rocksmith's camera over the part.
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
    /// The song's note speed at this difficulty (Rocksmith's DynamicVisualDensity; 1.3 at full difficulty by default,
    /// lower = faster).
    density: f32,
}

pub struct Highway {
    /// Low E at the bottom (Rocksmith's "invert strings"); off = low E on top as in Rocksmith.
    pub invert: bool,
    /// Index in `SPEEDS`.
    pub speed: usize,
    /// Index in `CAMERAS`.
    pub camera: usize,
    cache: Option<Cache>,
    /// When each scorer group was judged (for the burst / cross), cleared when its judgement goes (a seek back).
    judged: Vec<Option<(Judgement, f64)>>,
    /// The toolbar's last hover or change (it's see-through at rest, solid while in use).
    tools_used: Option<Instant>,
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
    pub fn new(invert: bool, speed: usize, camera: usize) -> Highway {
        Highway {
            invert,
            speed: speed.min(SPEEDS.len() - 1),
            camera: camera.min(CAMERAS.len() - 1),
            cache: None,
            judged: Vec::new(),
            tools_used: None,
        }
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

    /// Back to the default speed or camera.
    pub fn reset(&mut self, camera: bool) {
        self.tools_used = Some(Instant::now());
        if camera {
            self.camera = CAMERA_DEFAULT;
        } else {
            self.speed = SPEED_DEFAULT;
        }
    }

    /// The note speed as a percentage of the default.
    pub fn speed_pct(&self) -> i64 {
        (SPEEDS[self.speed] / SPEEDS[SPEED_DEFAULT] * 100.0).round() as i64
    }

    fn cache(&mut self, chart: &Chart) -> &Cache {
        if self
            .cache
            .as_ref()
            .is_none_or(|c| c.id != chart.id || c.camera != self.camera)
        {
            let shape_at = |t: f64| {
                let i = chart.shape_starts.partition_point(|&s| s <= t + 1e-3);
                (i > 0).then(|| &chart.shapes[i - 1]).filter(|s| t < s.end - 1e-3)
            };
            let any_hd = chart.chords.iter().any(|c| c.high_density);
            let mut chords: Vec<HwChord> = Vec::with_capacity(chart.chords.len());
            for (i, ch) in chart.chords.iter().enumerate() {
                let fretted = ch.notes.iter().map(|n| n.fret).filter(|&f| f > 0);
                // the chart's own flag (Rocksmith's high density); charts without any flags: the same chord again in the
                // same hand shape
                let repeat = if any_hd {
                    ch.high_density
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
                    name: ch.name.clone(),
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
                        name: s.name.clone(),
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
        let invert = self.invert;
        let speed = SPEEDS[self.speed] / SPEEDS[SPEED_DEFAULT];
        let n_str = chart.n_strings.clamp(4, 6);
        let camera = self.camera;
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
        let v = view::View::new(well, cam, now, c_secs, n_str, invert);
        scene::draw(&p, &v, c, &marks);
        let out = self.tools(ui, well);
        ui.ctx().request_repaint();
        out
    }

    /// The highway's toolbar, floating at its top right like the tab's: note speed and the camera's temper as sunken
    /// pills (− amount +), then invert strings as a round key (the RS Highway key, back to the tab, is the player's). See-through at
    /// rest, solid while the pointer is on it or just after a change.
    fn tools(&mut self, ui: &mut Ui, well: Rect) -> HwOut {
        let ctx = ui.ctx().clone();
        let th = t();
        let mut out = HwOut {
            acts: Vec::new(),
            tip: None,
        };
        let (key, pill, gap, pad, div_gap) = (px(26.0), px(112.0), px(6.0), px(5.0), px(8.0));
        let label_w: f32 = ["Speed", "Camera"]
            .iter()
            .map(|l| text::width(&ctx, l, Style::new(11.0, 600, th.caption)).ceil() + px(7.0))
            .sum();
        let rs_w = super::PlayerView::rs_highway_w(&ctx);
        let w = snap(
            2.0 * pad + rs_w + div_gap + label_w + 2.0 * pill + gap + 2.0 * div_gap + px(1.0) + key + px(4.0),
        );
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
        let steppers: [(&str, &str, String, [(&str, &'static str); 2], &'static str); 2] = [
            (
                "Speed",
                "Note speed",
                format!("{}%", self.speed_pct()),
                [("Slower notes", "hw_slower"), ("Faster notes", "hw_faster")],
                "hw_speed_reset",
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
            let r = Rect::from_min_size(ipos(lab.max.x + px(3.0), lab.min.y), vec2(pill, key));
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
                let back = if i == 0 { "click: 100%" } else { "click: Normal" };
                out.tip = Some((what.to_string(), back.into(), tip_at(mid_r), Align2::RIGHT_TOP));
            }
            if resp.clicked() {
                out.acts.push(reset);
            }
            x = r.max.x + if i == 0 { gap } else { 0.0 };
        }
        x = snap(x + div_gap);
        p.rect_filled(
            Rect::from_min_size(ipos(x, plate.min.y + px(8.0)), vec2(px(1.0), snap(h - px(16.0)))),
            0.0,
            th.line,
        );
        x = snap(x + px(1.0) + div_gap);
        let toggles = [(Icon::Invert, self.invert, "Invert strings", "I", "hw_invert")];
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

/// A chord name as Rocksmith spells it: flats as sharps (Gb5 -> F♯5), ♯ for #.
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
