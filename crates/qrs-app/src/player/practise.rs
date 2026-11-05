//! A learn item's Practise and Record steps, and its callouts (new in quickRS, P17.2-P17.3, D29; no Python source).
//! - Practise: the whole stretch loops at the speed you pick (full speed first), the tab shown, every pass scored
//!   (the last one, the best at each speed); note by note (W, Phase 15's mode) for a tricky spot, its passes not
//!   counted; "Ready to record" when you are.
//! - Record: a count-in and one take of the stretch. A take at full speed scoring the item's `pass` finishes it (Done,
//!   saved as a lick); one below it offers another go or Practise; one at a slower speed is kept but doesn't finish.
//!   For now the take is the existing scorer's pass; Phase 18's take recorder (`player/take.rs`) plugs in at the one
//!   seam, `take_result`: it starts recording in `take_start`, shows `Take::Analysing` while its thread works, and
//!   hands back a `TakeOutcome`.
//! - Callouts (any item): a line on the card from a moment before the play head reaches one until a few seconds after
//!   it (`lessons::callout_at`), and a flag on the tab at its time.

use std::time::Instant;

use eframe::egui;
use egui::{Align2, Rect, Stroke, Ui, pos2, vec2};
use qrs_library::lessons::{Callout, callout_at};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;

use super::PlayerView;
use super::coach::{Coach, Step};
use crate::library::{caps, txt};
use crate::state::AppState;

/// Full speed, give or take the rounding of a speed set.
const FULL: f64 = 0.995;

/// What a take came to: how accurate (None: no guitar to judge it), at what speed, and whether it finishes the item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TakeOutcome {
    pub accuracy: Option<f64>,
    pub at_speed: f64,
    pub passed: bool,
}

/// The pass rule: a take finishes the item when it's at full speed and at least the item's `pass`.
pub fn take_outcome(accuracy: Option<f64>, at_speed: f64, pass: f64) -> TakeOutcome {
    TakeOutcome {
        accuracy,
        at_speed,
        passed: at_speed >= FULL && accuracy.is_some_and(|a| a >= pass),
    }
}

/// Where the Record step's take is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Take {
    /// Not started: Enter counts in.
    #[default]
    Ready,
    /// Counting in, then playing the stretch once (since when).
    Recording(Instant),
    /// The take is being analysed (Phase 18's recorder sets it; the placeholder's scorer is done at once).
    Analysing,
    /// Its result.
    Done(TakeOutcome),
}

impl Take {
    /// Whether a take is under way (the speed and the way back wait for it).
    pub fn going(&self) -> bool {
        matches!(self, Take::Recording(_) | Take::Analysing)
    }

    /// The take key's label, and whether it's the primary key.
    pub fn key_label(&self) -> (&'static str, bool) {
        match self {
            Take::Ready => ("Record", true),
            Take::Recording(_) => ("Stop the take", false),
            Take::Analysing => ("Stop the analysis", false),
            Take::Done(o) => ("Record again", !o.passed),
        }
    }
}

impl PlayerView {
    /// Practise: a pass ended. Counted (the best at its speed kept) unless note by note was on during it.
    pub(super) fn practise_pass(&mut self, acc: Option<f64>) {
        let Some(c) = self.coach.as_mut() else { return };
        let practice_only = std::mem::replace(&mut c.nbn_pass, c.nbn);
        if practice_only {
            c.last_pass = None;
            self.say("Note by note: ", "a practice pass, not counted");
            return;
        }
        c.last_pass = Some(acc);
        let Some(acc) = acc else { return };
        let speed = c.speed();
        let best = c.progress.record(speed, acc);
        c.dirty = true;
        let tail = if acc < c.riff.pass {
            if best { "best yet at this speed" } else { "" }
        } else if c.speed_i > 0 {
            "try it faster (=)"
        } else {
            "ready to record (Enter)"
        };
        let head = format!(
            "Pass: {}% at {}%{}",
            (acc * 100.0).round() as i64,
            (speed * 100.0).round() as i64,
            if tail.is_empty() { "" } else { "  ·  " }
        );
        self.say(head, tail);
    }

    /// Practise: note by note on or off (W).
    pub(super) fn practise_nbn(&mut self) {
        let Some(c) = self.coach.as_mut() else { return };
        if c.step != Step::Practise || c.tuner {
            self.say("Note by note is for Practise: ", "the song waits at each note");
            return;
        }
        c.nbn = !c.nbn;
        let on = c.nbn;
        if on {
            c.nbn_pass = true; // (this pass isn't counted)
            self.say(
                "Note by note: ",
                "the song waits for each note  ·  W turns it off",
            );
        } else {
            self.nbn_release(true);
            self.say("Note by note off: ", "play it out");
        }
    }

    /// Record: the take key (Enter): start a take, stop one under way, or record again.
    pub(super) fn take_key(&mut self, _st: &mut AppState) {
        let Some(c) = self.coach.as_ref() else { return };
        match c.take {
            Take::Ready | Take::Done(_) => self.take_start(),
            Take::Recording(_) => self.take_abort("Take stopped"),
            Take::Analysing => self.take_abort("Analysis stopped"),
        }
    }

    /// A take: the count-in, then the stretch once, recorded and analysed (take.rs), at the speed set here (the
    /// song plays at it from the next frame, before the count-in ends).
    fn take_start(&mut self) {
        let Some(c) = self.coach.as_mut() else { return };
        c.nbn = false;
        c.last_pass = None;
        let (speed, a, b) = (c.speed(), c.riff.start, c.riff.end);
        self.set_ear(false);
        self.coach_speed(speed);
        if self.take_begin(a, b, true)
            && let Some(c) = self.coach.as_mut()
        {
            c.take = Take::Recording(Instant::now());
        }
    }

    /// Record: the loop came round. A take under way ends there (take.rs notices, keeps the tail, analyses it):
    /// the song stops. Passes played outside a take don't count.
    pub(super) fn take_pass(&mut self, _st: &mut AppState, _acc: Option<f64>) {
        if self.take.as_ref().is_some_and(super::take::Session::going) {
            self.transport.pause();
        }
    }

    /// The seam: a take's result (take.rs hands it the report's accuracy, `take_outcome`'s rule). Counted (the best at its speed kept), shown,
    /// and a pass finishes the item.
    pub(super) fn take_result(&mut self, st: &mut AppState, outcome: TakeOutcome) {
        let Some(c) = self.coach.as_mut() else { return };
        c.progress.takes += 1;
        if let Some(a) = outcome.accuracy {
            c.progress.record(outcome.at_speed, a);
        }
        c.dirty = true;
        c.take = Take::Done(outcome);
        let pass = c.riff.pass;
        let pc = |v: f64| (v * 100.0).round() as i64;
        match outcome.accuracy {
            None => self.say("Nothing here could be checked: ", "the take isn't counted"),
            Some(a) if outcome.passed => {
                self.coach_enter(st, Step::Done);
                if let Some(c) = self.coach.as_mut() {
                    c.take = Take::Done(outcome);
                }
                self.say_for(format!("Take: {}%  ·  learned!  ", pc(a)), "Saved as a lick", 3.0);
            }
            Some(a) if outcome.at_speed < FULL && a >= pass => self.say(
                format!("Take: {}% at {}%  ·  ", pc(a), pc(outcome.at_speed)),
                "now try it at full speed",
            ),
            Some(a) => self.say(
                format!("Take: {}%  ·  ", pc(a)),
                format!("{}% finishes it: again, or back to Practise", pc(pass)),
            ),
        }
    }

    /// Record: the take's panel (ready, under way, analysing), or its result card down to `bottom`. Returns the y
    /// under it and a bar of the result clicked (its start).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_take(
        &self,
        ui: &Ui,
        p: &egui::Painter,
        c: &Coach,
        x: f32,
        y: f32,
        w: f32,
        bottom: f32,
    ) -> (f32, Option<f64>) {
        let th = t();
        if matches!(c.take, Take::Done(_))
            && let Some(r) = self.take.as_ref().and_then(|s| s.report.clone())
        {
            let area = Rect::from_min_max(pos2(x, y), pos2(x + w, bottom.max(y + px(60.0))));
            let bar = self.draw_take_report(ui, p, &r, area);
            return (bottom, bar);
        }
        let r = Rect::from_min_size(pos2(x, y), vec2(w, px(96.0)));
        paint::inset(p, r, Some(14.0), 2.0, Fill::Base);
        let pc = |v: f64| (v * 100.0).round() as i64;
        let speed = c.speed();
        let (label, head, sub, col) = match c.take {
            Take::Ready => (
                "THE TAKE",
                format!("Ready at {}%", pc(speed)),
                if speed >= FULL {
                    "Enter: a count-in, then the whole part once".to_string()
                } else {
                    "a slower take is kept; full speed finishes it".to_string()
                },
                th.text,
            ),
            Take::Recording(_) if self.countdown_until.is_some() => (
                "RECORDING",
                "Count-in…".into(),
                format!("then the whole part at {}%", pc(speed)),
                th.miss,
            ),
            Take::Recording(_) => {
                let (a, b) = (c.riff.start, c.riff.end);
                let k = ((self.now() - a) / (b - a).max(0.1)).clamp(0.0, 1.0) as f32;
                let bar = Rect::from_min_size(
                    pos2(r.min.x + px(14.0), r.max.y - px(18.0)),
                    vec2(r.width() - px(28.0), px(6.0)),
                );
                paint::inset(p, bar, None, 1.0, Fill::Base);
                p.rect_filled(
                    Rect::from_min_size(bar.min, vec2(bar.width() * k, bar.height())),
                    px(3.0),
                    th.miss,
                );
                let secs = |s: f64| format!("{}:{:02}", (s / 60.0) as i64, (s % 60.0) as i64);
                (
                    "RECORDING",
                    "Recording".into(),
                    format!(
                        "{} of {}",
                        secs((self.now() - a).clamp(0.0, b - a) / speed),
                        secs((b - a) / speed)
                    ),
                    th.miss,
                )
            }
            Take::Analysing => {
                // how far the analysis has got
                let k = self.take.as_ref().and_then(|s| s.progress()).unwrap_or(0.0);
                let bar = Rect::from_min_size(
                    pos2(r.min.x + px(14.0), r.max.y - px(18.0)),
                    vec2(r.width() - px(28.0), px(6.0)),
                );
                paint::inset(p, bar, None, 1.0, Fill::Base);
                p.rect_filled(
                    Rect::from_min_size(bar.min, vec2(bar.width() * k.clamp(0.0, 1.0), bar.height())),
                    px(3.0),
                    th.accent,
                );
                p.ctx().request_repaint();
                (
                    "THE TAKE",
                    "Analysing…".into(),
                    "every note checked against the tab".into(),
                    th.text_2,
                )
            }
            Take::Done(o) => match o.accuracy {
                None => (
                    "THE TAKE",
                    "Nothing heard".into(),
                    "no input to judge it: Settings (F2) > Audio input / output".into(),
                    th.miss,
                ),
                Some(a) => (
                    "LAST TAKE",
                    format!("{}% at {}%", pc(a), pc(o.at_speed)),
                    if o.passed {
                        "learned!".to_string()
                    } else if o.at_speed < FULL && a >= c.riff.pass {
                        "good: now try it at full speed".to_string()
                    } else {
                        format!("{}% finishes it: again, or back to Practise", pc(c.riff.pass))
                    },
                    if o.passed || a >= c.riff.pass {
                        th.hit
                    } else {
                        th.text
                    },
                ),
            },
        };
        caps(
            p,
            label,
            pos2(r.min.x + px(14.0), r.min.y + px(10.0)),
            10.0,
            Align2::LEFT_TOP,
            None,
        );
        let hy = r.min.y + px(42.0);
        if matches!(c.take, Take::Recording(_)) {
            // the red light
            let pulse = 0.6 + 0.4 * (p.ctx().input(|i| i.time) as f32 * 4.0).sin();
            p.ctx().request_repaint();
            let hw = text::width(p.ctx(), &head, Style::new(22.0, 800, col));
            paint::led(
                p,
                pos2(r.center().x - hw / 2.0 - px(16.0), hy),
                12.0,
                col.gamma_multiply(pulse),
            );
        }
        txt(
            p,
            &head,
            pos2(r.center().x, hy),
            22.0,
            col,
            800,
            Align2::CENTER_CENTER,
            Some(r.width() - px(40.0)),
        );
        txt(
            p,
            &sub,
            pos2(r.center().x, hy + px(22.0)),
            12.0,
            th.text_2,
            700,
            Align2::CENTER_CENTER,
            Some(r.width() - px(20.0)),
        );
        let mut yy = r.max.y + px(8.0);
        if c.progress.takes > 0 {
            txt(
                p,
                &format!(
                    "{} take{} so far  ·  best at full speed: {}",
                    c.progress.takes,
                    if c.progress.takes == 1 { "" } else { "s" },
                    c.progress
                        .best_at(1.0)
                        .map(|a| format!("{}%", pc(a)))
                        .unwrap_or_else(|| "–".into())
                ),
                pos2(x + w / 2.0, yy),
                12.0,
                th.caption,
                700,
                Align2::CENTER_TOP,
                Some(w),
            );
            yy += px(18.0);
        }
        (yy, None)
    }

    /// The callout line on the card (Listen, Practise, Record): the one up now, else the next one dimmed. Its room is
    /// kept either way. Returns the y under it.
    pub(super) fn draw_callout(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let r = Rect::from_min_size(pos2(x, y), vec2(w, px(62.0)));
        let now = self.now();
        let live = callout_at(&c.riff.callouts, now);
        let (head, body, lit) = match live {
            Some(co) => (
                if now < co.at {
                    format!("COMING UP  ·  {}", clock(co.at))
                } else {
                    format!("HERE  ·  {}", clock(co.at))
                },
                co.text.clone(),
                true,
            ),
            None => match next_callout(&c.riff.callouts, now, c.riff.start) {
                Some(co) => (format!("NEXT  ·  {}", clock(co.at)), co.text.clone(), false),
                None => ("CALLOUTS".into(), String::new(), false),
            },
        };
        if lit {
            paint::raised(p, r, Some(12.0), 2.0, Fill::Base, 0.0);
            paint::ring(p, r, Some(12.0), 1.5, th.accent, false);
        } else {
            paint::inset(p, r, Some(12.0), 2.0, Fill::Base);
        }
        flag(
            p,
            pos2(r.min.x + px(14.0), r.min.y + px(9.0)),
            if lit { th.accent } else { th.caption },
        );
        text::caps(
            p,
            &head,
            pos2(r.min.x + px(26.0), r.min.y + px(8.0)),
            10.0,
            if lit { th.accent_ink } else { th.caption },
            Align2::LEFT_TOP,
            Some(w - px(36.0)),
        );
        let col = if lit { th.text } else { th.caption };
        let st = Style::new(12.0, 700, col);
        let lines = text::wrap(p.ctx(), &body, st, w - px(24.0));
        for (i, line) in lines.iter().take(2).enumerate() {
            let cut = i == 1 && lines.len() > 2;
            let shown = if cut {
                format!("{}…", line.trim_end())
            } else {
                line.clone()
            };
            txt(
                p,
                &shown,
                pos2(r.min.x + px(12.0), r.min.y + px(25.0) + i as f32 * px(16.0)),
                12.0,
                col,
                700,
                Align2::LEFT_TOP,
                Some(w - px(24.0)),
            );
        }
        if live.is_some() {
            p.ctx().request_repaint();
        }
        r.max.y + px(10.0)
    }

    /// Over the tab well: a theory page's sheet, or an item's callouts as flags on the tab (not while the notes are
    /// hidden by ear).
    pub(super) fn draw_coach_well(&mut self, ui: &Ui, well: Rect) {
        self.draw_take_overlay(ui);
        let Some(c) = self.coach.as_ref() else { return };
        if matches!(c.step, Step::Page(_)) && !c.tuner {
            self.draw_sheet(ui, well);
            return;
        }
        if self.ear || !matches!(c.step, Step::Listen | Step::Practise | Step::Record) {
            return;
        }
        self.draw_novel(ui);
        if c.riff.callouts.is_empty() {
            return;
        }
        let th = t();
        let p = ui.painter().with_clip_rect(self.tab.view_rect());
        let live = callout_at(&c.riff.callouts, self.now()).map(|co| co.at);
        for (i, co) in c.riff.callouts.iter().enumerate() {
            let Some((cx, top, bottom)) = self.tab.screen_at(co.at) else {
                continue;
            };
            // (a row scrolled part way out: its flag stays in view)
            let top = top.max(self.tab.view_rect().min.y + px(2.0));
            let lit = live == Some(co.at);
            let col = if lit {
                th.accent
            } else {
                th.accent.gamma_multiply(0.55)
            };
            p.rect_filled(
                Rect::from_min_max(pos2(cx - px(1.0), top + px(4.0)), pos2(cx + px(1.0), bottom)),
                0.0,
                col.gamma_multiply(if lit { 0.9 } else { 0.5 }),
            );
            flag(&p, pos2(cx, top + px(4.0)), col);
            txt(
                &p,
                &(i + 1).to_string(),
                pos2(cx + px(13.0), top + px(9.0)),
                9.0,
                th.accent_ink,
                800,
                Align2::LEFT_CENTER,
                None,
            );
        }
    }
}

impl PlayerView {
    /// A whole-song item: the bars no earlier learn item of the lesson (same song, same part) covers, from Phase 18's
    /// bar similarity (`similar::novel_bars`), for the tab.
    pub(super) fn coach_novel(&mut self) {
        let title = self.entry.display_title();
        let artist = self.entry.artist.clone();
        let here = self
            .entry
            .arrangements
            .iter()
            .position(|a| a.key == self.part_key);
        let arrangements = self.entry.arrangements.clone();
        let Some(c) = self.coach.as_ref() else { return };
        let same = |r: &qrs_library::Riff| r.is_song(&artist, &title) && r.part_of(&arrangements) == here;
        let novel = match learned_spans(&c.lesson.riffs, c.index, &same) {
            Some(spans) => {
                qrs_analysis::similar::novel_bars(&self.chart.arr, &spans, (c.riff.start, c.riff.end))
            }
            None => Vec::new(),
        };
        if !novel.is_empty() {
            let bars: Vec<String> = novel
                .iter()
                .map(|b| format!("{} ({:.1})", b.number, b.t0))
                .collect();
            crate::applog::write(&format!(
                "lesson {} / {}: new here: bars {}",
                c.lesson.id,
                c.riff.id,
                bars.join(", ")
            ));
        }
        if let Some(c) = self.coach.as_mut() {
            c.novel = novel;
        }
    }

    /// "New here": the whole-song item's novel bars tinted on the tab, each with a small label.
    fn draw_novel(&self, ui: &Ui) {
        let Some(c) = self.coach.as_ref() else { return };
        if c.novel.is_empty() {
            return;
        }
        let th = t();
        let p = ui.painter().with_clip_rect(self.tab.view_rect());
        for nb in &c.novel {
            for r in self.tab.bar_bands(nb.t0, nb.t1) {
                p.rect_filled(r, px(10.0), th.accent.gamma_multiply(0.10));
                p.rect_stroke(
                    r.shrink(px(1.0)),
                    px(10.0),
                    Stroke::new(px(1.0), th.accent.gamma_multiply(0.45)),
                    egui::StrokeKind::Inside,
                );
                let tag = Rect::from_min_size(
                    pos2(r.max.x - px(40.0), r.min.y + px(3.0)),
                    vec2(px(34.0), px(14.0)),
                );
                p.rect_filled(tag, px(5.0), th.accent.gamma_multiply(0.9));
                text::caps(
                    &p,
                    "NEW",
                    tag.center(),
                    9.0,
                    th.on_accent,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
        }
    }
}

/// The spans learned before a whole-song item (`index` in `riffs`): every earlier learn item on its song and part
/// (`same`), when the item is a whole-song one: it takes in at least one of them (90% of it inside) and is at least
/// twice as long as the longest it takes in. None for any other item.
pub fn learned_spans(
    riffs: &[qrs_library::Riff],
    index: usize,
    same: &dyn Fn(&qrs_library::Riff) -> bool,
) -> Option<Vec<(f64, f64)>> {
    let me = riffs.get(index)?;
    if me.kind != qrs_library::lessons::Kind::Learn {
        return None;
    }
    let earlier: Vec<(f64, f64)> = riffs[..index]
        .iter()
        .filter(|r| r.kind == qrs_library::lessons::Kind::Learn && same(r))
        .map(|r| (r.start, r.end))
        .collect();
    let inside = |&(a, b): &(f64, f64)| (b.min(me.end) - a.max(me.start)).max(0.0) >= 0.9 * (b - a);
    let longest = earlier
        .iter()
        .filter(|s| inside(s))
        .map(|(a, b)| b - a)
        .fold(0.0, f64::max);
    (longest > 0.0 && me.end - me.start >= 2.0 * longest).then_some(earlier)
}

/// The first callout after `now` (or from the stretch's start when nothing's played yet).
pub fn next_callout(callouts: &[Callout], now: f64, start: f64) -> Option<&Callout> {
    let from = now.max(start - 1.0);
    callouts
        .iter()
        .filter(|c| c.at > from)
        .min_by(|a, b| a.at.total_cmp(&b.at))
}

/// "1:23" for chart seconds.
fn clock(s: f64) -> String {
    let s = s.max(0.0);
    format!("{}:{:02}", (s / 60.0) as i64, (s % 60.0) as i64)
}

/// A small pennant: the top of its pole at `at`, the flag to the right.
fn flag(p: &egui::Painter, at: egui::Pos2, col: egui::Color32) {
    let h = px(10.0);
    p.line_segment([at, pos2(at.x, at.y + h + px(3.0))], Stroke::new(px(1.5), col));
    p.add(egui::Shape::convex_polygon(
        vec![
            at,
            pos2(at.x + px(9.0), at.y + h * 0.35),
            pos2(at.x, at.y + h * 0.7),
        ],
        col,
        Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_finishes_the_item_only_at_full_speed_and_its_pass() {
        // at full speed: the pass mark finishes it, a hair under doesn't
        assert!(take_outcome(Some(0.8), 1.0, 0.8).passed);
        assert!(take_outcome(Some(0.95), 1.0, 0.8).passed);
        assert!(!take_outcome(Some(0.799), 1.0, 0.8).passed);
        // a slower take is kept but never finishes it, however good
        let slow = take_outcome(Some(1.0), 0.85, 0.8);
        assert!(!slow.passed);
        assert_eq!((slow.accuracy, slow.at_speed), (Some(1.0), 0.85));
        // a speed set's "1.0" read back as 0.999...
        assert!(take_outcome(Some(0.9), 0.9999, 0.9).passed);
        // no guitar: nothing to judge
        assert!(!take_outcome(None, 1.0, 0.8).passed);
    }

    #[test]
    fn the_take_key_says_what_it_does() {
        assert_eq!(Take::Ready.key_label(), ("Record", true));
        assert!(Take::Recording(Instant::now()).going());
        assert!(Take::Analysing.going());
        assert!(!Take::Ready.going());
        let missed = Take::Done(take_outcome(Some(0.6), 1.0, 0.8));
        assert_eq!(missed.key_label(), ("Record again", true));
        assert!(!missed.going());
        let passed = Take::Done(take_outcome(Some(0.9), 1.0, 0.8));
        assert_eq!(passed.key_label(), ("Record again", false));
    }

    /// A part in 4/4 at a beat a second: `bars` bars of beats, with the notes (time, string, fret) given.
    fn part(bars: usize, notes: &[(f64, i32, i32)]) -> qrs_formats::Arrangement {
        use qrs_formats::model::Beat;
        let mut a = qrs_formats::Arrangement::new("Rhythm");
        a.song_length = bars as f64 * 4.0 + 1.0;
        a.beats = (0..=bars * 4)
            .map(|i| Beat {
                time: i as f64,
                measure: if i % 4 == 0 { (i / 4) as i32 + 1 } else { -1 },
            })
            .collect();
        for &(t, s, f) in notes {
            a.notes.push(qrs_formats::Note::new(t, s, f));
        }
        a
    }

    #[test]
    fn stop_time_is_new_here_though_the_shuffle_covers_it() {
        // bars 1-2: the shuffle (eight hits a bar, root and fifth); bar 3: the same; bar 4: stop time (one hit);
        // bar 5: a lick never learned
        let mut notes: Vec<(f64, i32, i32)> = Vec::new();
        for bar in [0, 1, 2] {
            for k in 0..8 {
                let t = bar as f64 * 4.0 + k as f64 * 0.5;
                notes.push((t, 0, 6));
                notes.push((t, 1, 8));
            }
        }
        notes.push((12.0, 0, 6));
        notes.push((12.0, 1, 8));
        for k in 0..4 {
            notes.push((16.0 + k as f64, 3, 3 + k));
        }
        let arr = part(5, &notes);
        // learned: bars 1-2; the whole song: bars 1-5
        // (Phase 18's novel bars, as the coach takes them)
        let marked: Vec<i32> = qrs_analysis::similar::novel_bars(&arr, &[(0.0, 8.0)], (0.0, 20.0))
            .iter()
            .map(|b| b.number)
            .collect();
        assert_eq!(
            marked,
            vec![4, 5],
            "stop time and the new lick; the repeated shuffle isn't new"
        );
        // nothing learned: every bar with notes is new
        assert_eq!(qrs_analysis::similar::novel_bars(&arr, &[], (0.0, 20.0)).len(), 5);
    }

    #[test]
    fn only_a_whole_song_item_is_marked_new_here() {
        use qrs_library::Riff;
        use qrs_library::lessons::Kind;
        let item = |id: &str, kind: Kind, part: &str, start: f64, end: f64| Riff {
            id: id.into(),
            kind,
            part: part.into(),
            start,
            end,
            ..Default::default()
        };
        // Johnny B. Goode's shape: the intro, the rhythm, the stabs, the solo, theory between, then the whole song
        let riffs = vec![
            item("intro", Kind::Learn, "Lead", 10.83, 27.35),
            item("intro-theory", Kind::Theory, "Lead", 10.83, 27.35),
            item("verse-chorus-rhythm", Kind::Learn, "Rhythm", 28.19, 62.22),
            item("chorus-stabs", Kind::Learn, "Lead", 45.29, 59.4),
            item("solo", Kind::Learn, "Lead", 96.0, 130.1),
            item("whole-song", Kind::Learn, "Lead", 10.83, 165.9),
            item("whole-song-rhythm", Kind::Learn, "Rhythm", 17.0, 165.9),
        ];
        let lead = |r: &Riff| r.part == "Lead";
        let rhythm = |r: &Riff| r.part == "Rhythm";
        // the whole song, lead: against the intro, the stabs and the solo (not the theory item, not the rhythm)
        assert_eq!(
            learned_spans(&riffs, 5, &lead),
            Some(vec![(10.83, 27.35), (45.29, 59.4), (96.0, 130.1)])
        );
        assert_eq!(learned_spans(&riffs, 6, &rhythm), Some(vec![(28.19, 62.22)]));
        // the parts themselves aren't whole-song items (nothing learned inside them, or not twice as long)
        assert_eq!(learned_spans(&riffs, 0, &lead), None);
        assert_eq!(
            learned_spans(&riffs, 3, &lead),
            None,
            "the stabs: the intro isn't inside them"
        );
        assert_eq!(learned_spans(&riffs, 4, &lead), None);
        assert_eq!(learned_spans(&riffs, 1, &lead), None, "a theory item");
        // a slightly longer re-take of a part isn't a whole-song item either
        let mut again = riffs.clone();
        again.push(item("intro-again", Kind::Learn, "Lead", 10.5, 28.0));
        assert_eq!(learned_spans(&again, 7, &lead), None);
        assert_eq!(learned_spans(&riffs, 99, &lead), None);
    }

    #[test]
    fn the_next_callout_after_the_play_head() {
        let cs = vec![
            Callout {
                at: 50.0,
                text: "b".into(),
            },
            Callout {
                at: 20.0,
                text: "a".into(),
            },
        ];
        assert_eq!(next_callout(&cs, 0.0, 10.0).map(|c| c.text.as_str()), Some("a"));
        assert_eq!(next_callout(&cs, 25.0, 10.0).map(|c| c.text.as_str()), Some("b"));
        assert_eq!(next_callout(&cs, 50.0, 10.0), None);
        assert_eq!(clock(79.33), "1:19");
        assert_eq!(clock(5.0), "0:05");
    }
}
