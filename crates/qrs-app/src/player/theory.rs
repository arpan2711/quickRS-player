//! A theory item (new in quickRS, P17.4, D29; no Python source): pages, each a step. The page itself is a sheet over
//! the tab's well (its title, its paragraphs, then the diagram: one neck with the page's scale, the intervals on its
//! dots, how much the item's stretch leans on each note, labelled marks, the notes to find; a row of chord boxes);
//! the coach's card says what to do and keeps count. A page's activity: read it; run the scale up and down (as a
//! scale item's run); find its targets (each lights when its exact pitch is played, anywhere on the neck); or jam
//! over its stretch looping at full speed (your notes land on the neck, in the scale or out). R plays the scale or
//! the chords on the synth guitar.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use qrs_audio::synth::SynthNote;
use qrs_formats::Techs;
use qrs_library::lessons::{Activity, ChordBox, Page, Scale, in_scale, interval_name, pc_name};
use qrs_ui::icons::{self, Icon};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::coach::{Coach, Step};
use super::{COUNTDOWN, Chart, LOOP_LEAD_IN, PlayerView};
use crate::library::{caps, draw_fade, txt};

/// A note's weight in the emphasis: its length (its ring, else the time to the next note) held to these, and the
/// extra a note on a beat gets.
const W_MIN: f64 = 0.1;
const W_MAX: f64 = 1.0;
const ON_BEAT: f64 = 0.5;
/// How near a beat (seconds) a note counts as on it.
const BEAT_NEAR: f64 = 0.05;
/// The notes leaned on, listed (most used first).
const RANKED: usize = 5;
/// How long a find's verdict stays up (seconds).
const SAID_FOR: f32 = 2.5;
/// Seconds between the chords "Hear it" plays.
const CHORD_GAP: f64 = 1.3;

/// A note of the stretch, as the emphasis weighs it: when, where, how long it rings, how far it's bent (half steps,
/// the most) and whether the bend comes back down (bend and release).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ENote {
    pub t: f64,
    pub string: i32,
    pub fret: i32,
    pub sus: f64,
    pub bend: f64,
    pub releases: bool,
}

/// How much a stretch leans on each note: the weight at each place on the neck (string, fret) and on each pitch
/// class, and in all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Emphasis {
    pub spots: Vec<((i32, i32), f64)>,
    pub by_pc: [f64; 12],
    pub total: f64,
}

impl Emphasis {
    /// The weight at a place on the neck.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn at(&self, s: i32, f: i32) -> f64 {
        self.spots
            .iter()
            .find(|x| x.0 == (s, f))
            .map(|x| x.1)
            .unwrap_or(0.0)
    }

    /// The heaviest place's weight.
    pub fn most(&self) -> f64 {
        self.spots.iter().map(|x| x.1).fold(0.0, f64::max)
    }

    /// The pitch classes leaned on most, with their interval from `root` and their share: "Bb (1) 31%".
    pub fn ranked(&self, root: i32, n: usize) -> Vec<(i32, &'static str, f64)> {
        if self.total <= 0.0 {
            return Vec::new();
        }
        let mut v: Vec<(i32, f64)> = (0..12)
            .filter(|&pc| self.by_pc[pc as usize] > 0.0)
            .map(|pc| (pc, self.by_pc[pc as usize]))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter()
            .take(n)
            .map(|(pc, w)| (pc, interval_name(root, pc), w / self.total))
            .collect()
    }
}

/// Weigh a stretch's notes (in time order) on strings tuned to `opens`, with the chart's beats (seconds): each note
/// counts its length, `clamp(ring, or the time to the next note when it doesn't ring, 0.1, 1.0)` seconds, plus 0.5
/// when it's on a beat. A bent note counts at the pitch it's bent to (the fret's pitch plus the bend in half steps,
/// rounded), at the fret where that pitch is on its string; a bend that comes back down counts both pitches.
pub fn weigh(notes: &[ENote], opens: &[i32], beats: &[f64]) -> Emphasis {
    let mut e = Emphasis::default();
    for (i, n) in notes.iter().enumerate() {
        let Some(open) = opens.get(n.string as usize) else {
            continue;
        };
        let next = notes[i + 1..]
            .iter()
            .find(|o| o.t > n.t + 0.005)
            .map(|o| o.t - n.t);
        let len = if n.sus > 0.05 {
            n.sus
        } else {
            next.unwrap_or(W_MAX)
        };
        let k = beats.partition_point(|&b| b < n.t);
        let on_beat = [k.checked_sub(1), Some(k)]
            .into_iter()
            .flatten()
            .filter_map(|j| beats.get(j))
            .any(|&b| (b - n.t).abs() <= BEAT_NEAR);
        let w = len.clamp(W_MIN, W_MAX) + if on_beat { ON_BEAT } else { 0.0 };
        let up = n.bend.round() as i32;
        let mut frets = vec![n.fret + up.max(0)];
        if up > 0 && n.releases {
            frets.push(n.fret);
        }
        for f in frets {
            let m = open + f;
            e.by_pc[m.rem_euclid(12) as usize] += w;
            e.total += w;
            match e.spots.iter_mut().find(|x| x.0 == (n.string, f)) {
                Some(x) => x.1 += w,
                None => e.spots.push(((n.string, f), w)),
            }
        }
    }
    e
}

/// Whether a bend curve (time, half steps) comes back down after its peak (bend and release).
pub fn releases(curve: &[(f64, f64)]) -> bool {
    let Some((pi, peak)) = curve
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .map(|(i, p)| (i, p.1))
    else {
        return false;
    };
    peak >= 0.5 && curve[pi + 1..].iter().any(|p| p.1 <= peak - 0.5)
}

/// The emphasis of a chart's stretch (chart seconds): its notes and chords' notes, muted and ignored ones left out.
pub fn emphasis_of(chart: &Chart, a: f64, b: f64) -> Emphasis {
    let arr = &chart.arr;
    let inside = |t: f64| t >= a - 0.02 && t < b - 0.01;
    let mut notes: Vec<ENote> = Vec::new();
    let mut add = |n: &qrs_formats::Note, techs: Techs, sus: f64| {
        if techs.has(Techs::FRET_HAND_MUTE) || techs.has(Techs::IGNORE) || !inside(n.time) {
            return;
        }
        let peak = n.bend_curve.iter().map(|p| p.1).fold(n.bend, f64::max);
        notes.push(ENote {
            t: n.time,
            string: n.string,
            fret: n.fret,
            sus,
            bend: peak,
            releases: releases(&n.bend_curve),
        });
    };
    for n in &arr.notes {
        add(n, n.techniques, n.sustain);
    }
    for c in &arr.chords {
        for n in &c.notes {
            add(n, n.techniques | c.techniques, n.sustain.max(c.sustain));
        }
    }
    notes.sort_by(|x, y| x.t.total_cmp(&y.t).then(x.string.cmp(&y.string)));
    weigh(&notes, &chart.opens, &chart.btimes)
}

/// A pluck (`played`, MIDI) while finding a page's targets (pitch, found yet): the first target not found yet with
/// that exact pitch, if any.
pub fn find_pluck(find: &[(i32, bool)], played: f64) -> Option<usize> {
    let m = played.round() as i32;
    find.iter().position(|&(t, done)| !done && t == m)
}

/// The frets a page's neck is drawn over (first, last): its places with a fret either side, at least five frets,
/// from the nut when it's that close.
pub fn neck_frets(places: &[(i32, i32)]) -> (i32, i32) {
    let Some(lo) = places.iter().map(|s| s.1).min() else {
        return (0, 5);
    };
    let hi = places.iter().map(|s| s.1).max().unwrap_or(lo);
    let mut a = (lo - 1).max(0);
    if a <= 1 {
        a = 0;
    }
    (a, (hi + 1).max(a + 5).min(24))
}

/// A chord box's frets: the first fret drawn (1: from the nut) and how many are drawn (at least four).
pub fn chord_window(frets: &[i32]) -> (i32, i32) {
    let fretted: Vec<i32> = frets.iter().copied().filter(|&f| f > 0).collect();
    let (Some(lo), Some(hi)) = (fretted.iter().min(), fretted.iter().max()) else {
        return (1, 4);
    };
    let first = if *hi <= 4 { 1 } else { *lo };
    (first, (hi - first + 1).max(4))
}

impl Coach {
    /// Set up page `i` on this chart: its scale's run and notes, its targets' pitches (Some(why) when its scale's
    /// shape doesn't go up in pitch on this tuning).
    /// (Fitted again on the same page, after another part or level, the targets found stay found.)
    pub(super) fn fit_page(&mut self, chart: &Chart, i: usize) -> Option<String> {
        let page = self.riff.pages.get(i)?.clone();
        let find: Vec<(i32, bool)> = page
            .targets
            .iter()
            .filter_map(|&(s, f)| chart.opens.get(s as usize).map(|o| (o + f, false)))
            .collect();
        if self.find_page == Some(i) && self.find.len() == find.len() {
            self.find = find.iter().zip(&self.find).map(|(n, o)| (n.0, o.1)).collect();
        } else {
            self.find = find;
            self.find_said = None;
        }
        self.find_page = Some(i);
        let sc = page.show.scale.as_ref()?;
        self.run_seq = sc.run(&chart.opens);
        self.run_pos = self.run_pos.min(self.run_seq.len().saturating_sub(1));
        self.pcs = sc.pitch_classes(&chart.opens);
        sc.out_of_order(&chart.opens).map(|k| {
            let (s, f) = sc.shape[k];
            format!(
                "{} page {}: the shape's note {} ([{s}, {f}]) isn't higher than the one before on this tuning",
                self.riff.id,
                i + 1,
                k + 1
            )
        })
    }
}

impl PlayerView {
    /// The theory page the coach is on.
    pub(super) fn page(&self) -> Option<&Page> {
        let c = self.coach.as_ref()?;
        match c.step {
            Step::Page(i) => c.riff.pages.get(i),
            _ => None,
        }
    }

    /// Whether the page listens to the guitar (a run, a find, a jam).
    pub(super) fn page_listens(&self) -> bool {
        self.page().is_some_and(|p| p.act != Activity::Read)
    }

    /// On to page `i`: its scale and targets, the song stopped at the item's stretch, or a jam's stretch looping at
    /// full speed (counted in, the tab hidden).
    pub(super) fn page_enter(&mut self, i: usize) {
        let opens_chart = &self.chart;
        let Some(c) = self.coach.as_mut() else { return };
        // (a new visit: its targets to find again, its runs counted afresh)
        c.find_page = None;
        c.page_runs = 0;
        let bad = c.fit_page(opens_chart, i);
        c.jam_in = 0;
        c.jam_all = 0;
        let Some(page) = c.riff.pages.get(i).cloned() else {
            return;
        };
        let (a, b) = match (page.start, page.end) {
            (Some(a), Some(b)) => (a, b),
            _ => (c.riff.start, c.riff.end),
        };
        let (ia, ib) = (c.riff.start, c.riff.end);
        if let Some(why) = bad {
            crate::applog::write(&format!("lesson {}: {why}", c.lesson.id));
            self.say_for(
                "The page's scale shape doesn't go up in pitch on this part's tuning: ",
                "see quickrs.log",
                4.0,
            );
        }
        self.coach_speed(1.0);
        if page.act == Activity::Jam {
            self.loop_a = Some(a);
            self.loop_b = Some(b);
            self.set_ear(true);
            self.seek((a - LOOP_LEAD_IN).max(0.0));
            self.countdown_until = Some(Instant::now() + Duration::from_secs_f32(COUNTDOWN));
            self.run_scored = false;
        } else {
            self.loop_a = Some(ia);
            self.loop_b = Some(ib);
            self.set_ear(false);
            self.seek((ia - LOOP_LEAD_IN).max(0.0));
        }
    }

    /// Find: one pluck's note against the targets not found yet.
    pub(super) fn page_find_note(&mut self, midi: Option<f64>) {
        let Some(c) = self.coach.as_mut() else { return };
        let Some(m) = midi else {
            c.unclear = Some(Instant::now());
            return;
        };
        c.unclear = None;
        let hit = find_pluck(&c.find, m);
        if let Some(k) = hit {
            c.find[k].1 = true;
            if c.find.iter().all(|x| x.1) {
                self.say("All found  ·  ", "Enter: on to the next page");
            }
        }
        if let Some(c) = self.coach.as_mut() {
            c.find_said = Some((m, hit, Instant::now()));
        }
    }

    /// Whether the page's activity is done enough for Next to be the key to press.
    pub(super) fn page_ready(&self, c: &Coach) -> bool {
        match self.page().map(|p| p.act) {
            Some(Activity::Run) => c.page_runs > 0,
            Some(Activity::Find) => !c.find.is_empty() && c.find.iter().all(|x| x.1),
            Some(Activity::Jam) => c.jam_all > 0,
            _ => true,
        }
    }

    /// What the card says to do on the page.
    pub(super) fn page_tell(&self, c: &Coach) -> String {
        let Some(p) = self.page() else {
            return String::new();
        };
        let hear = if p.show.scale.is_some() {
            " R plays the scale."
        } else if !p.show.chords.is_empty() {
            " R plays the chords."
        } else if !p.show.marks.is_empty() {
            " R plays the marked notes."
        } else {
            ""
        };
        let _ = c;
        match p.act {
            Activity::Read => format!("Read the page, then on to the next.{hear}"),
            Activity::Run => {
                "Play the scale from its lowest note to its highest and back down. Each note lights as \
                              you play it; the same note fretted somewhere else counts too."
                    .into()
            }
            Activity::Find => {
                "Find each ringed note and play it: the exact pitch counts, wherever you fret it. Each \
                               one lights as you find it."
                    .into()
            }
            Activity::Jam => {
                "The song loops at full speed with the tab hidden. Make up your own lines with the shape: \
                              your notes land on the neck, green in the scale, red out of it."
                    .into()
            }
        }
    }

    /// The card's part for the page: the run's progress, the notes found, the jam's share in the scale.
    pub(super) fn draw_page_card(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let mid = x + w / 2.0;
        match self.page().map(|pg| pg.act) {
            Some(Activity::Run) => {
                self.draw_run(p, c, x, y, w);
                y + px(80.0)
            }
            Some(Activity::Jam) => {
                self.draw_jam(p, c, x, y, w);
                y + px(84.0)
            }
            Some(Activity::Find) => {
                let found = c.find.iter().filter(|x| x.1).count();
                let all = found == c.find.len();
                txt(
                    p,
                    &format!("{found} of {} found", c.find.len()),
                    pos2(mid, y + px(4.0)),
                    20.0,
                    if all { th.hit } else { th.text },
                    800,
                    Align2::CENTER_TOP,
                    Some(w),
                );
                let fresh = c.find_said.filter(|s| s.2.elapsed().as_secs_f32() < SAID_FOR);
                let unclear = c.unclear.is_some_and(|u| u.elapsed().as_secs_f32() < SAID_FOR);
                let label = |m: f64| qrs_analysis::pitch::note_label(m);
                let (msg, col) = match fresh {
                    Some((m, Some(_), _)) => (format!("Got it: {}", label(m)), th.hit),
                    Some((m, None, _)) if c.find.iter().any(|x| x.1 && x.0 == m.round() as i32) => {
                        (format!("{} again: that one's found", label(m)), th.text_2)
                    }
                    Some((m, None, _)) => (format!("Not one of them: you played {}", label(m)), th.miss),
                    None if unclear => ("Didn't catch that one: pick it clearly".into(), th.caption),
                    None if all => ("Every one found".into(), th.hit),
                    None if !self.has_input() => {
                        ("No input: Settings (F2) > Audio input / output".into(), th.miss)
                    }
                    None => ("Play a ringed note".into(), th.caption),
                };
                txt(
                    p,
                    &msg,
                    pos2(mid, y + px(34.0)),
                    13.0,
                    col,
                    700,
                    Align2::CENTER_TOP,
                    Some(w),
                );
                y + px(60.0)
            }
            _ => y,
        }
    }

    /// "Hear it" on a page: its scale up and down, else its chords one after another, else its marks (or its
    /// targets), on the synth guitar.
    pub(super) fn page_hear(&mut self) {
        let Some(p) = self.page().cloned() else { return };
        let t0 = self.now();
        if let Some(sc) = &p.show.scale {
            let mut notes = sc.shape.clone();
            notes.extend(sc.shape.iter().rev().skip(1));
            self.coach_synth(&notes, t0, 0.32, 0.5);
            return;
        }
        if p.show.chords.is_empty() {
            // its marks, else its targets, one after another
            let notes: Vec<(i32, i32)> = if p.show.marks.is_empty() {
                p.targets.clone()
            } else {
                p.show.marks.iter().map(|m| (m.0, m.1)).collect()
            };
            if notes.is_empty() {
                self.say("Nothing to hear on this page", "");
            } else {
                self.coach_synth(&notes, t0, 0.6, 0.55);
            }
            return;
        }
        let mut ev: Vec<(f64, i32, i32)> = Vec::new();
        for (k, ch) in p.show.chords.iter().enumerate() {
            for (s, &f) in ch.frets.iter().enumerate() {
                if f >= 0 {
                    // (a quick strum, low string first)
                    ev.push((t0 + k as f64 * CHORD_GAP + s as f64 * 0.025, s as i32, f));
                }
            }
        }
        self.synth_events(&ev, CHORD_GAP - 0.15);
    }

    /// Notes on the synth guitar at their own times: (chart time, string, fret), each ringing `ring` seconds.
    fn synth_events(&mut self, ev: &[(f64, i32, i32)], ring: f64) {
        let Some(e) = self.engine().cloned() else {
            self.say("The synth needs the audio engine", "");
            return;
        };
        if ev.is_empty() {
            return;
        }
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
        let notes: Vec<SynthNote> = ev
            .iter()
            .map(|&(t, string, fret)| SynthNote {
                t,
                string,
                fret,
                sus: ring,
                tech: Techs::NONE,
                slide_to: -1,
                bend: 0.0,
                in_chord: true,
            })
            .collect();
        let t0 = ev.iter().map(|x| x.0).fold(f64::MAX, f64::min);
        let end = ev.iter().map(|x| x.0).fold(f64::MIN, f64::max) + ring + 0.4;
        let tone = qrs_audio::synth::synth_tone(&e.tone(), self.chart.arr.is_bass);
        let place: Arc<dyn Fn(f64) -> f64 + Send + Sync> = Arc::new(|x| x);
        self.synth.start(
            notes,
            self.chart.opens.clone(),
            t0 - 0.02,
            end,
            1.0,
            self.chart.arr.is_bass,
            false,
            tone,
            e.rate(),
            place,
        );
    }

    // -- the sheet ----------------------------------------------------------------------------------------------

    /// The page over the tab's well: where it is in the item, its title and paragraphs (the wheel scrolls them when
    /// they don't fit), then its diagram: the neck and the chord boxes, beside the text (or under it when the well
    /// is narrow).
    pub(super) fn draw_sheet(&mut self, ui: &Ui, well: Rect) {
        let Some(c) = self.coach.as_ref() else { return };
        let Step::Page(pi) = c.step else { return };
        let Some(page) = c.riff.pages.get(pi).cloned() else {
            return;
        };
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        paint::display(&p, well, Some(24.0), 6.0);
        // (the tab underneath doesn't take clicks while the page is up)
        let _ = interact(ui, well, "theory-sheet", Sense::click());
        let inner = well.shrink(px(28.0));
        caps(
            &p,
            &format!(
                "{}  ·  {}  ·  PAGE {} OF {}",
                c.lesson.label(),
                c.riff.name,
                pi + 1,
                c.riff.pages.len()
            )
            .to_uppercase(),
            inner.min,
            11.0,
            Align2::LEFT_TOP,
            Some(inner.width()),
        );
        let title_r = txt(
            &p,
            &page.title,
            pos2(inner.min.x, inner.min.y + px(20.0)),
            24.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(inner.width()),
        );
        let body = Rect::from_min_max(pos2(inner.min.x, title_r.max.y + px(16.0)), inner.max);
        let wide = body.width() >= px(700.0);
        let has_diagram = page.show.neck() || !page.show.chords.is_empty();
        let (text_r, diag_r) = if !has_diagram {
            (
                Rect::from_min_size(body.min, vec2(body.width().min(px(640.0)), body.height())),
                Rect::NOTHING,
            )
        } else if wide {
            let tw_ = (body.width() * 0.40).clamp(px(300.0), px(460.0));
            (
                Rect::from_min_size(body.min, vec2(tw_, body.height())),
                Rect::from_min_max(pos2(body.min.x + tw_ + px(32.0), body.min.y), body.max),
            )
        } else {
            let th_ = body.height() * 0.42;
            (
                Rect::from_min_size(body.min, vec2(body.width(), th_)),
                Rect::from_min_max(pos2(body.min.x, body.min.y + th_ + px(16.0)), body.max),
            )
        };
        // the text, scrolled
        let scroll = c.sheet_scroll;
        let tp = p.with_clip_rect(text_r);
        let st = Style::new(14.0, 500, th.text_2);
        let mut y = text_r.min.y - scroll;
        for para in &page.text {
            for line in text::wrap(&ctx, para, st, text_r.width()) {
                txt(
                    &tp,
                    &line,
                    pos2(text_r.min.x, y),
                    14.0,
                    th.text_2,
                    500,
                    Align2::LEFT_TOP,
                    None,
                );
                y += px(21.0);
            }
            y += px(10.0);
        }
        let content = y + scroll - text_r.min.y;
        let max = (content - text_r.height()).max(0.0);
        let hover = ui.rect_contains_pointer(text_r);
        let wheel = if hover {
            ui.input(|i| i.smooth_scroll_delta.y)
        } else {
            0.0
        };
        if scroll < max - 0.5 {
            draw_fade(
                &p,
                Rect::from_min_size(
                    pos2(text_r.min.x, text_r.max.y - px(36.0)),
                    vec2(text_r.width(), px(36.0)),
                ),
                th.screen,
            );
            icons::icon(
                &p,
                Icon::Down,
                pos2(text_r.center().x, text_r.max.y - px(8.0)),
                16.0,
                th.caption,
            );
        }
        // the diagram
        if has_diagram {
            self.draw_diagram(&p, &page, diag_r);
        }
        if let Some(c) = self.coach.as_mut() {
            c.sheet_scroll = (scroll - wheel).clamp(0.0, max);
        }
    }

    /// A page's diagram in `r`: the neck (with its legend and the notes leaned on), then the chord boxes.
    fn draw_diagram(&self, p: &egui::Painter, page: &Page, r: Rect) {
        let th = t();
        let Some(c) = self.coach.as_ref() else { return };
        let sh = &page.show;
        let mut y = r.min.y;
        if sh.neck() {
            let n = self.chart.n_strings.max(1) as f32;
            let neck_h = (px(30.0) * (n - 1.0) + px(44.0)).min(r.height() * 0.62);
            let nr = Rect::from_min_size(pos2(r.min.x, y), vec2(r.width(), neck_h + px(16.0)));
            self.draw_page_neck(p, c, page, nr);
            y = nr.max.y + px(10.0);
            y = self.draw_neck_legend(p, c, page, r.min.x, y, r.width());
        }
        if !sh.chords.is_empty() && y < r.max.y - px(60.0) {
            if sh.neck() {
                y += px(8.0);
                caps(p, "CHORDS", pos2(r.min.x, y), 10.0, Align2::LEFT_TOP, None);
                y += px(18.0);
            }
            let k = sh.chords.len() as f32;
            let gap = px(18.0);
            let room = r.max.y - y;
            let bw = ((r.width() - (k - 1.0) * gap) / k)
                .min(px(150.0))
                .min(room / 1.45);
            let bh = (bw * 1.45).min(room);
            let total = k * bw + (k - 1.0) * gap;
            let x0 = if sh.neck() {
                r.min.x
            } else {
                r.center().x - total / 2.0
            };
            for (i, ch) in sh.chords.iter().enumerate() {
                let br = Rect::from_min_size(pos2(x0 + i as f32 * (bw + gap), y), vec2(bw, bh));
                self.draw_chord_box(p, ch, br);
            }
        }
        let _ = th;
    }

    /// The page's neck: its places (the scale, the marks, the targets) a fret either side, low string at the bottom.
    fn draw_page_neck(&self, p: &egui::Painter, c: &Coach, page: &Page, r: Rect) {
        let th = t();
        let sh = &page.show;
        let opens = &self.chart.opens;
        let n = self.chart.n_strings.max(1) as i32;
        let mut places: Vec<(i32, i32)> = Vec::new();
        if let Some(sc) = &sh.scale {
            places.extend(&sc.shape);
        }
        places.extend(sh.marks.iter().map(|m| (m.0, m.1)));
        places.extend(&page.targets);
        let (lo, hi) = neck_frets(&places);
        let num_h = px(16.0);
        let nut = lo == 0;
        let nut_w = if nut { px(28.0) } else { px(4.0) };
        let first = lo.max(1);
        let cols = (hi - first + 1).max(1);
        // (a few frets in a wide well: no wider than a real neck's look, from the left)
        let fw = ((r.width() - nut_w - px(4.0)) / cols as f32).min(px(70.0));
        let board = Rect::from_min_max(
            pos2(r.min.x, r.min.y + px(14.0)),
            pos2(r.min.x + nut_w + px(4.0) + fw * cols as f32, r.max.y - num_h),
        );
        paint::inset(p, board, Some(10.0), 3.0, Fill::Color(th.neck));
        let x0 = board.min.x + nut_w;
        let m_ = px(12.0);
        let gap = (board.height() - 2.0 * m_) / (n - 1).max(1) as f32;
        let sy = |s: i32| board.min.y + m_ + (n - 1 - s) as f32 * gap;
        let fx = |f: i32| {
            if f == 0 {
                board.min.x + nut_w / 2.0
            } else {
                x0 + ((f - first) as f32 + 0.5) * fw
            }
        };
        let ppp = p.pixels_per_point();
        let whole = |v: f32| (v * ppp).round() / ppp;
        // inlays, frets, the nut, strings, fret numbers
        let mid = board.center().y;
        for f in first..=hi {
            let dots = match f % 12 {
                0 => 2,
                3 | 5 | 7 | 9 => 1,
                _ => 0,
            };
            for j in 0..dots {
                let cy = mid + (j as f32 - (dots - 1) as f32 / 2.0) * gap * 2.0;
                paint::dot(p, pos2(fx(f), cy), 3.0, th.inlay);
            }
        }
        let lw = whole(px(1.5)).max(1.0 / ppp);
        for k in 0..=cols {
            let xx = whole(x0 + k as f32 * fw);
            p.rect_filled(
                Rect::from_min_max(
                    pos2(xx - lw / 2.0, board.min.y + px(4.0)),
                    pos2(xx + lw / 2.0, board.max.y - px(4.0)),
                ),
                0.0,
                th.fret,
            );
        }
        if nut {
            p.rect_filled(
                Rect::from_min_size(
                    pos2(board.min.x + nut_w - px(4.0), board.min.y + px(4.0)),
                    vec2(px(4.0), board.height() - px(8.0)),
                ),
                px(2.0),
                th.nut,
            );
        }
        let widths = [2.2, 1.9, 1.6, 1.3, 1.1, 1.0];
        for s in 0..n {
            let thk = whole(px(widths[(s as usize).min(5)])).max(1.0 / ppp);
            let top = whole(sy(s) - thk / 2.0);
            p.rect_filled(
                Rect::from_min_size(
                    pos2(board.min.x + px(3.0), top),
                    vec2(board.width() - px(6.0), thk),
                ),
                0.0,
                if s < 3 || self.chart.arr.is_bass {
                    th.wound
                } else {
                    th.plain
                },
            );
        }
        for f in first..=hi {
            if f % 2 == 1 || f % 12 == 0 || f == first {
                txt(
                    p,
                    &f.to_string(),
                    pos2(fx(f), board.max.y + px(2.0)),
                    9.0,
                    th.caption,
                    700,
                    Align2::CENTER_TOP,
                    None,
                );
            }
        }
        let on = |s: i32, f: i32| s >= 0 && s < n && f >= lo && f <= hi;
        let rad = (gap * 0.38).clamp(px(7.0), px(11.0));
        let root = sh.scale.as_ref().and_then(Scale::root_pc);
        let pitch = |s: i32, f: i32| opens.get(s as usize).map(|o| o + f);
        // the emphasis: each place shaded by how much the stretch uses it
        if sh.emphasis {
            let most = c.emph.most();
            for &((s, f), wt) in &c.emph.spots {
                if !on(s, f) || most <= 0.0 {
                    continue;
                }
                let k = (wt / most).clamp(0.0, 1.0) as f32;
                let at = pos2(fx(f), sy(s));
                p.circle_filled(
                    at,
                    rad + px(3.0) + k * px(5.0),
                    th.accent.gamma_multiply(0.18 + 0.55 * k),
                );
            }
        }
        // the scale's dots (roots gold, interval labels), lit as they're run
        let run_now = matches!(page.act, Activity::Run) && !c.run_seq.is_empty();
        let up = c.run_seq.len().div_ceil(2);
        let (played, due): (Vec<usize>, Option<usize>) = if run_now {
            let idx = |k: usize| if k < up { k } else { c.run_seq.len() - 1 - k };
            let d = idx(c.run_pos.min(c.run_seq.len() - 1));
            let lit = if c.run_pos < up {
                (0..c.run_pos).collect()
            } else {
                (d + 1..up).collect()
            };
            (lit, Some(d))
        } else {
            (Vec::new(), None)
        };
        let now = p.ctx().input(|i| i.time) as f32;
        let dark = th.key_black;
        if let Some(sc) = &sh.scale {
            for (i, &(s, f)) in sc.shape.iter().enumerate() {
                if !on(s, f) {
                    continue;
                }
                let at = pos2(fx(f), sy(s));
                let Some(m) = pitch(s, f) else { continue };
                let is_root = root.is_some_and(|pc| m.rem_euclid(12) == pc);
                let lit = played.contains(&i);
                let fill = if lit {
                    th.hit
                } else if is_root {
                    th.accent
                } else {
                    th.key_white
                };
                if lit {
                    paint::halo(p, at, 14.0, th.hit, 90);
                }
                p.circle_filled(at, rad, fill);
                if sh.intervals
                    && let Some(r0) = root
                {
                    txt(
                        p,
                        interval_name(r0, m),
                        at,
                        9.5,
                        dark,
                        800,
                        Align2::CENTER_CENTER,
                        None,
                    );
                }
                if due == Some(i) {
                    let pulse = 0.5 + 0.5 * (now * 5.0).sin();
                    p.ctx().request_repaint();
                    p.circle_stroke(
                        at,
                        rad + px(3.0) + pulse * px(2.0),
                        Stroke::new(px(2.0), th.accent_ink),
                    );
                }
            }
        }
        // the marks: a ring and its label above
        for (s, f, label) in &sh.marks {
            if !on(*s, *f) {
                continue;
            }
            let at = pos2(fx(*f), sy(*s));
            let in_shape = sh.scale.as_ref().is_some_and(|sc| sc.shape.contains(&(*s, *f)));
            if !in_shape {
                p.circle_filled(at, rad * 0.8, th.accent_ink);
            }
            p.circle_stroke(at, rad + px(2.5), Stroke::new(px(2.0), th.accent_ink));
            tag(p, label, pos2(at.x, at.y - rad - px(4.0)));
        }
        // the targets: ringed until found, then lit
        if page.act == Activity::Find {
            for (k, &(s, f)) in page.targets.iter().enumerate() {
                if !on(s, f) {
                    continue;
                }
                let at = pos2(fx(f), sy(s));
                let found = c.find.get(k).is_some_and(|x| x.1);
                if found {
                    paint::halo(p, at, 15.0, th.hit, 110);
                    p.circle_filled(at, rad, th.hit);
                    icons::icon(p, Icon::Check, at, 12.0, dark);
                } else {
                    let pulse = 0.5 + 0.5 * (now * 3.0).sin();
                    p.ctx().request_repaint();
                    p.circle_stroke(
                        at,
                        rad + px(3.0) + pulse * px(1.5),
                        Stroke::new(px(2.5), th.accent_ink),
                    );
                }
            }
        }
        // a jam: your notes, fading, green in the scale and red out of it
        if page.act == Activity::Jam {
            let shape: &[(i32, i32)] = sh.scale.as_ref().map(|s| s.shape.as_slice()).unwrap_or(&[]);
            for &(m, at_) in &c.jam {
                let spot = shape
                    .iter()
                    .copied()
                    .find(|&(s, f)| pitch(s, f) == Some(m))
                    .or_else(|| {
                        (0..n).find_map(|s| {
                            let f = m - opens[s as usize];
                            (lo..=hi).contains(&f).then_some((s, f))
                        })
                    });
                let Some((s, f)) = spot else { continue };
                let k = (1.0 - at_.elapsed().as_secs_f32() / 3.0).clamp(0.0, 1.0);
                let col = if in_scale(c.pcs, m) { th.hit } else { th.miss };
                let pos = pos2(fx(f), sy(s));
                p.circle_filled(pos, rad + px(2.0), col.gamma_multiply(0.35 * k));
                p.circle_stroke(pos, rad + px(2.0), Stroke::new(px(2.5), col.gamma_multiply(k)));
            }
            if !c.jam.is_empty() {
                p.ctx().request_repaint();
            }
        }
    }

    /// Under the neck: what its marks are, the scale's name, and (emphasis) the notes the stretch leans on. Returns
    /// the y under it.
    fn draw_neck_legend(&self, p: &egui::Painter, c: &Coach, page: &Page, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let sh = &page.show;
        let mut yy = y;
        if let Some(sc) = &sh.scale {
            let mut xx = x;
            let cy = yy + px(8.0);
            let mut item = |label: &str, draw: &dyn Fn(egui::Pos2)| {
                draw(pos2(xx + px(6.0), cy));
                let r = txt(
                    p,
                    label,
                    pos2(xx + px(16.0), cy),
                    12.0,
                    th.text_2,
                    700,
                    Align2::LEFT_CENTER,
                    None,
                );
                xx = r.max.x + px(14.0);
            };
            item(&sc.name, &|at| {
                p.circle_filled(at, px(5.0), th.key_white);
                p.circle_stroke(at, px(5.0), Stroke::new(qrs_ui::units::px_line(1.0), th.caption));
            });
            item(&format!("root {}", sc.root), &|at| {
                p.circle_filled(at, px(5.0), th.accent);
            });
            if sh.emphasis {
                item("how much the stretch uses it", &|at| {
                    p.circle_filled(at, px(6.0), th.accent.gamma_multiply(0.45));
                });
            }
            yy += px(24.0);
        }
        if sh.emphasis
            && let Some(root) = sh.scale.as_ref().and_then(Scale::root_pc)
        {
            let ranked = c.emph.ranked(root, RANKED);
            caps(
                p,
                "WHAT IT LEANS ON",
                pos2(x, yy + px(2.0)),
                10.0,
                Align2::LEFT_TOP,
                None,
            );
            yy += px(20.0);
            if ranked.is_empty() {
                txt(
                    p,
                    "No notes in the stretch on this part",
                    pos2(x, yy),
                    12.0,
                    th.caption,
                    600,
                    Align2::LEFT_TOP,
                    Some(w),
                );
                return yy + px(20.0);
            }
            let mut xx = x;
            for (pc, iv, share) in ranked {
                let label = format!(
                    "{} ({iv})  {}%",
                    flat_name(pc, &sh.scale),
                    (share * 100.0).round() as i64
                );
                let tw_ = text::width(p.ctx(), &label, Style::new(12.0, 800, th.text)) + px(18.0);
                if xx + tw_ > x + w {
                    break;
                }
                let chip = Rect::from_min_size(pos2(xx, yy), vec2(tw_, px(24.0)));
                paint::inset(p, chip, Some(12.0), 1.5, Fill::Base);
                txt(
                    p,
                    &label,
                    chip.center(),
                    12.0,
                    th.text,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
                xx += tw_ + px(8.0);
            }
            yy += px(30.0);
        }
        yy
    }

    /// A chord box: its name over a grid of its strings (low at the left) and frets, x over a string not played, o
    /// over an open one, the fret it starts at beside it when it's up the neck.
    fn draw_chord_box(&self, p: &egui::Painter, ch: &ChordBox, r: Rect) {
        let th = t();
        txt(
            p,
            &ch.name,
            pos2(r.center().x, r.min.y),
            13.0,
            th.text,
            800,
            Align2::CENTER_TOP,
            Some(r.width()),
        );
        let n = ch.frets.len().max(2) as i32;
        let (first, rows) = chord_window(&ch.frets);
        let side = px(26.0); // (room for the fret number)
        let grid = Rect::from_min_max(
            pos2(r.min.x + side, r.min.y + px(38.0)),
            pos2(r.max.x - px(6.0), r.max.y - px(4.0)),
        );
        let sx = grid.width() / (n - 1) as f32;
        let ry = grid.height() / rows as f32;
        let gx = |s: i32| grid.min.x + s as f32 * sx;
        for k in 0..=rows {
            let y = grid.min.y + k as f32 * ry;
            let wdt = if k == 0 && first == 1 {
                px(4.0)
            } else {
                qrs_ui::units::px_line(1.2)
            };
            p.line_segment(
                [pos2(grid.min.x, y), pos2(grid.max.x, y)],
                Stroke::new(wdt, th.text_2),
            );
        }
        for s in 0..n {
            p.line_segment(
                [pos2(gx(s), grid.min.y), pos2(gx(s), grid.max.y)],
                Stroke::new(qrs_ui::units::px_line(1.2), th.text_2),
            );
        }
        if first > 1 {
            txt(
                p,
                &first.to_string(),
                pos2(
                    grid.min.x - (sx.min(ry) * 0.34).max(px(3.0)) - px(5.0),
                    grid.min.y + ry / 2.0,
                ),
                11.0,
                th.caption,
                800,
                Align2::RIGHT_CENTER,
                None,
            );
        }
        let rad = (sx.min(ry) * 0.34).max(px(3.0));
        for (s, &f) in ch.frets.iter().enumerate() {
            let x = gx(s as i32);
            let top = pos2(x, grid.min.y - px(9.0));
            match f {
                f if f < 0 => {
                    txt(p, "x", top, 11.0, th.caption, 800, Align2::CENTER_CENTER, None);
                }
                0 => {
                    p.circle_stroke(top, px(4.0), Stroke::new(qrs_ui::units::px_line(1.5), th.text_2));
                }
                f => {
                    let row = (f - first) as f32;
                    p.circle_filled(pos2(x, grid.min.y + (row + 0.5) * ry), rad, th.accent);
                }
            }
        }
    }
}

/// A pitch class's name the way the page's scale spells it: flats when its root is a flat ("Bb", "Eb"), else sharps.
fn flat_name(pc: i32, scale: &Option<Scale>) -> String {
    const FLATS: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];
    let flat = scale
        .as_ref()
        .is_some_and(|s| s.root.contains('b') || s.root == "F");
    if flat {
        FLATS[pc.rem_euclid(12) as usize].to_string()
    } else {
        pc_name(pc).to_string()
    }
}

/// A small label on a plate, its bottom centre at `at`.
fn tag(p: &egui::Painter, label: &str, at: egui::Pos2) {
    let th = t();
    let st = Style::new(10.0, 800, th.text);
    let w = text::width(p.ctx(), label, st) + px(10.0);
    let r = Rect::from_min_size(pos2(at.x - w / 2.0, at.y - px(16.0)), vec2(w, px(16.0)));
    p.rect_filled(r, px(5.0), th.screen.gamma_multiply(0.92));
    p.rect_stroke(
        r,
        px(5.0),
        Stroke::new(qrs_ui::units::px_line(1.0), th.accent_ink),
        egui::StrokeKind::Inside,
    );
    txt(
        p,
        label,
        r.center(),
        10.0,
        th.text,
        800,
        Align2::CENTER_CENTER,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDARD: [i32; 6] = [40, 45, 50, 55, 59, 64];

    fn note(t: f64, string: i32, fret: i32, sus: f64) -> ENote {
        ENote {
            t,
            string,
            fret,
            sus,
            bend: 0.0,
            releases: false,
        }
    }

    #[test]
    fn longer_notes_and_notes_on_the_beat_weigh_more() {
        let beats = [1.0, 1.5, 2.0, 2.5];
        // a short note on the beat, a short one off it (0.25 s to the next), a long held one
        let notes = [note(1.0, 3, 3, 0.0), note(1.25, 3, 5, 0.0), note(1.5, 2, 5, 2.0)];
        let e = weigh(&notes, &STANDARD, &beats);
        // 1.0: 0.25 to the next note + 0.5 on the beat; 1.25: 0.25; 1.5: held 2 s, held to 1.0, + 0.5
        assert!((e.at(3, 3) - 0.75).abs() < 1e-9, "{e:?}");
        assert!((e.at(3, 5) - 0.25).abs() < 1e-9);
        assert!((e.at(2, 5) - 1.5).abs() < 1e-9);
        assert!((e.total - 2.5).abs() < 1e-9);
        // a very short note counts at least 0.1, the last note with nothing after it a whole 1.0
        let e = weigh(&[note(0.3, 0, 0, 0.0), note(0.32, 0, 3, 0.0)], &STANDARD, &[]);
        assert!((e.at(0, 0) - 0.1).abs() < 1e-9);
        assert!((e.at(0, 3) - 1.0).abs() < 1e-9);
        // a chord's notes at one time share the time to the next onset, not 0
        let e = weigh(
            &[note(1.0, 0, 6, 0.0), note(1.0, 1, 8, 0.0), note(1.4, 0, 6, 0.0)],
            &STANDARD,
            &[],
        );
        assert!((e.at(1, 8) - 0.4).abs() < 1e-9, "{e:?}");
        // a beat just under 50 ms away still counts
        let e = weigh(&[note(1.04, 0, 0, 0.3)], &STANDARD, &[1.0]);
        assert!((e.total - 0.8).abs() < 1e-9);
    }

    #[test]
    fn a_bent_note_counts_at_the_pitch_its_bent_to() {
        // G string 8th fret (Eb4) bent a whole step: heard as F4, on the neck at the 10th fret
        let bent = ENote {
            bend: 2.0,
            ..note(1.0, 3, 8, 0.5)
        };
        let e = weigh(&[bent], &STANDARD, &[]);
        assert_eq!(e.at(3, 8), 0.0, "not the fretted pitch");
        assert!((e.at(3, 10) - 0.5).abs() < 1e-9);
        assert!((e.by_pc[5] - 0.5).abs() < 1e-9, "F");
        // a half step bend of 0.6 rounds up to a half step; 0.4 is no bend
        let e = weigh(
            &[ENote {
                bend: 0.6,
                ..note(1.0, 3, 8, 0.5)
            }],
            &STANDARD,
            &[],
        );
        assert!(e.at(3, 9) > 0.0);
        let e = weigh(
            &[ENote {
                bend: 0.4,
                ..note(1.0, 3, 8, 0.5)
            }],
            &STANDARD,
            &[],
        );
        assert!(e.at(3, 8) > 0.0);
        // bend and release counts both pitches
        let e = weigh(
            &[ENote {
                bend: 1.0,
                releases: true,
                ..note(1.0, 3, 8, 0.5)
            }],
            &STANDARD,
            &[],
        );
        assert!(e.at(3, 8) > 0.0 && e.at(3, 9) > 0.0);
        assert!((e.total - 1.0).abs() < 1e-9);
        // the curve tells a release
        assert!(releases(&[(0.0, 0.0), (0.2, 2.0), (0.5, 0.0)]));
        assert!(!releases(&[(0.0, 0.0), (0.2, 2.0), (0.5, 2.0)]));
        assert!(
            !releases(&[(0.0, 0.0), (0.2, 0.3), (0.5, 0.0)]),
            "too small to be a bend"
        );
        assert!(!releases(&[]));
    }

    #[test]
    fn the_notes_leaned_on_ranked_with_their_intervals() {
        let bb = 10;
        let notes = [
            note(0.0, 0, 6, 1.0),  // Bb (1)
            note(1.0, 0, 6, 1.0),  // Bb
            note(2.0, 4, 6, 1.0),  // F (5)
            note(3.0, 3, 7, 0.5),  // D (3)
            note(4.0, 3, 6, 0.25), // Db (b3)
        ];
        let e = weigh(&notes, &STANDARD, &[]);
        let r = e.ranked(bb, 5);
        let got: Vec<(i32, &str, i64)> = r
            .iter()
            .map(|x| (x.0, x.1, (x.2 * 100.0).round() as i64))
            .collect();
        assert_eq!(got, vec![(10, "1", 53), (5, "5", 27), (2, "3", 13), (1, "b3", 7)]);
        assert_eq!(e.ranked(bb, 2).len(), 2);
        assert!(Emphasis::default().ranked(bb, 5).is_empty());
        assert_eq!(
            flat_name(
                10,
                &Some(Scale {
                    root: "Bb".into(),
                    ..Default::default()
                })
            ),
            "Bb"
        );
        assert_eq!(flat_name(10, &None), "A#");
    }

    #[test]
    fn finding_each_target_by_its_exact_pitch() {
        // Bb on the low E (46), Bb on the D string (58), Bb on the high E (70)
        let mut find = vec![(46, false), (58, false), (70, false)];
        assert_eq!(
            find_pluck(&find, 58.2),
            Some(1),
            "anywhere on the neck, the exact pitch"
        );
        find[1].1 = true;
        assert_eq!(find_pluck(&find, 58.0), None, "found already");
        assert_eq!(find_pluck(&find, 47.0), None, "a semitone off");
        assert_eq!(find_pluck(&find, 34.0), None, "another octave doesn't count");
        assert_eq!(find_pluck(&find, 45.6), Some(0));
        // two targets with one pitch: one pluck each
        let mut two = vec![(46, false), (46, false)];
        assert_eq!(find_pluck(&two, 46.0), Some(0));
        two[0].1 = true;
        assert_eq!(find_pluck(&two, 46.0), Some(1));
    }

    #[test]
    fn a_charts_stretch_weighed_with_its_bends_and_mutes() {
        use qrs_formats::{Arrangement, Note};
        let mut arr = Arrangement::new("Lead");
        arr.song_length = 30.0;
        let mut bent = Note::new(10.0, 3, 8); // Eb4 bent up a whole step and back
        bent.sustain = 0.6;
        bent.bend = 2.0;
        bent.bend_curve = vec![(10.0, 0.0), (10.2, 2.0), (10.5, 0.0)];
        let mut muted = Note::new(10.7, 3, 8);
        muted.techniques = Techs::FRET_HAND_MUTE;
        arr.notes = vec![
            Note::new(9.0, 3, 3),
            bent,
            muted,
            Note::new(11.0, 0, 6),
            Note::new(20.0, 0, 6),
        ];
        let nota = crate::player::Nota {
            show: true,
            snap: false,
            rests: true,
            beams: true,
            triplets: true,
        };
        let chart = Chart::new(arr, 30.0, &nota);
        let e = emphasis_of(&chart, 10.0, 12.0);
        // the note before the stretch, the muted one and the one after aren't in it
        assert_eq!(e.at(3, 3), 0.0);
        assert!(
            e.at(3, 10) > 0.0 && e.at(3, 8) > 0.0,
            "bent to F and back to Eb: {e:?}"
        );
        assert!(e.at(0, 6) > 0.0);
        assert_eq!(e.spots.len(), 3);
    }

    #[test]
    fn a_refit_keeps_the_notes_found_and_a_new_visit_doesnt() {
        use qrs_library::lessons::{Activity, Kind, Page};
        use qrs_library::{Lesson, Riff, RiffProgress};
        let nota = crate::player::Nota {
            show: true,
            snap: false,
            rests: true,
            beams: true,
            triplets: true,
        };
        let mut arr = qrs_formats::Arrangement::new("Lead");
        arr.song_length = 10.0;
        let chart = Chart::new(arr, 10.0, &nota);
        let page = Page {
            title: "Find the roots".into(),
            act: Activity::Find,
            targets: vec![(0, 6), (2, 8), (5, 6)],
            ..Default::default()
        };
        let riff = Riff {
            kind: Kind::Theory,
            pages: vec![page.clone(), page],
            ..Default::default()
        };
        let mut c = Coach::new(
            std::sync::Arc::new(Lesson::default()),
            0,
            riff,
            RiffProgress::default(),
        );
        c.fit_page(&chart, 0);
        assert_eq!(c.find, vec![(46, false), (58, false), (70, false)]);
        c.find[1].1 = true;
        // another part or difficulty: the same page fitted again keeps what was found
        c.fit_page(&chart, 0);
        assert_eq!(c.find[1], (58, true));
        // another page (or the same one visited again: `page_enter` forgets first) starts afresh
        c.fit_page(&chart, 1);
        assert!(c.find.iter().all(|x| !x.1));
        c.find[0].1 = true;
        c.find_page = None;
        c.fit_page(&chart, 1);
        assert!(c.find.iter().all(|x| !x.1));
    }

    #[test]
    fn a_pages_neck_and_its_chord_boxes() {
        // Bb box 1 at the 6th fret: frets 5 to 10
        assert_eq!(neck_frets(&[(0, 6), (0, 9), (5, 6), (5, 9)]), (5, 10));
        // marks spread over the neck: from the nut
        assert_eq!(neck_frets(&[(0, 6), (1, 1), (1, 13)]), (0, 14));
        assert_eq!(neck_frets(&[]), (0, 5));
        // chord boxes: open chords from the nut; barre chords from their lowest fret; at least four frets
        assert_eq!(chord_window(&[-1, 3, 2, 0, 1, 0]), (1, 4)); // C
        assert_eq!(chord_window(&[6, 8, 6, 7, 6, 6]), (6, 4)); // Bb7
        assert_eq!(chord_window(&[-1, 8, 10, 8, 10, 8]), (8, 4)); // F7
        assert_eq!(chord_window(&[1, 8, 10, -1, -1, -1]), (1, 10)); // a stretch: every fret drawn
        assert_eq!(chord_window(&[0, 0, 0, 0, 0, 0]), (1, 4));
    }
}
