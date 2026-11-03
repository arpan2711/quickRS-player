//! Continuous mode (new in quickRS, P29.2, owner request; no Python source): the loop plays on without stopping
//! (its end runs straight into its start, no lead-in, no count-in) and every pass is scored as you go. A plate at
//! the tab's foot holds the passes: the last one (and against the one before), the pass under way so far, the best
//! three, every pass as a column. O, or the review strip's key, turns it on for the loop set (a review spot, L,
//! [ ], a lick); clearing the loop turns it off. The level doesn't go up by itself in it (the passes stay
//! comparable). Each loop's passes are kept while the song is open, so a spot comes back with its own.

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_analysis::Judgement;
use qrs_ui::paint::{self, Fill};
use qrs_ui::theme::t;
use qrs_ui::units::px;

use super::review::CLEAN;
use super::{LOOP_LEAD_IN, PlayerView, ipos, snap};
use crate::library::{caps, txt};

/// The passes drawn as columns (the latest).
const COLUMNS: usize = 24;

/// A loop's key in the passes kept: its ends in milliseconds.
pub(super) fn loop_key(a: f64, b: f64) -> (i64, i64) {
    ((a * 1000.0).round() as i64, (b * 1000.0).round() as i64)
}

/// The best three passes: (pass number from 1, accuracy), best first (the earlier of equals first).
pub(super) fn top3(passes: &[f64]) -> Vec<(usize, f64)> {
    let mut v: Vec<(usize, f64)> = passes.iter().enumerate().map(|(i, &a)| (i + 1, a)).collect();
    v.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    v.truncate(3);
    v
}

/// Clean passes (90%+) in a row, up to the last.
pub(super) fn clean_streak(passes: &[f64]) -> usize {
    passes.iter().rev().take_while(|&&a| a >= CLEAN).count()
}

/// What the toast says of the pass just played (the last of `passes`): a new best, its place in the top three, or
/// how it went against the one before.
pub(super) fn pass_word(passes: &[f64]) -> String {
    let Some((&last, before)) = passes.split_last() else {
        return String::new();
    };
    if before.is_empty() {
        return "the first".into();
    }
    if before.iter().all(|&b| last > b) {
        return "new best".into();
    }
    let n = passes.len();
    if let Some(place) = top3(passes).iter().position(|&(p, _)| p == n) {
        return ["", "2nd best", "3rd best"][place].into();
    }
    let d = ((last - before[before.len() - 1]) * 100.0).round() as i64;
    match d {
        0 => "same as the last".into(),
        d => format!("{d:+} on the last"),
    }
}

fn pct(a: f64) -> String {
    format!("{}%", (a * 100.0).round() as i64)
}

impl PlayerView {
    /// The passes of the loop set (empty without one).
    pub(super) fn passes(&self) -> &[f64] {
        self.loop_a
            .zip(self.loop_b)
            .and_then(|(a, b)| self.pass_hist.get(&loop_key(a, b)))
            .map_or(&[], |v| v.as_slice())
    }

    /// A pass was scored: kept with its loop's; in continuous mode, said.
    pub(super) fn continuous_pass(&mut self, acc: Option<f64>) {
        let (Some(acc), Some((a, b))) = (acc, self.loop_a.zip(self.loop_b)) else {
            return;
        };
        let list = self.pass_hist.entry(loop_key(a, b)).or_default();
        let best_before = list.iter().copied().reduce(f64::max);
        list.push(acc);
        if self.continuous {
            // (the score panel's chip says how the pass went: no toast over the tab)
            self.arcade_pass(acc, best_before);
        }
    }

    /// O: continuous mode on or off, for the loop set.
    pub(super) fn toggle_continuous(&mut self) {
        if self.continuous {
            self.continuous = false;
            self.say("Continuous off: ", "the loop goes back to its lead-in");
            return;
        }
        let Some((a, _)) = self.loop_a.zip(self.loop_b) else {
            self.say_for(
                "Continuous needs a loop: ",
                "a review spot (E), L for this section, or [ and ]",
                3.0,
            );
            return;
        };
        self.continuous = true;
        self.say_for("Continuous: ", "the loop plays on, every pass scored", 2.5);
        if !self.transport.playing() && self.countdown_until.is_none() {
            self.seek((a - LOOP_LEAD_IN).max(0.0));
            self.countdown_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs_f32(super::COUNTDOWN));
        }
    }

    /// The pass under way: its accuracy over the notes judged so far, and how many those are.
    fn pass_so_far(&self) -> Option<(f64, usize)> {
        let (a, b) = self.loop_a.zip(self.loop_b)?;
        let (mut n, mut got) = (0usize, 0.0);
        for g in self.scorer.groups.iter().filter(|g| a <= g.t && g.t < b) {
            match g.state {
                Some(Judgement::Hit) => got += 1.0,
                Some(Judgement::Late | Judgement::Early) => got += 0.5,
                Some(Judgement::Miss) => {}
                None => continue,
            }
            n += 1;
        }
        (n > 0).then(|| (got / n as f64, n))
    }

    /// The passes' plate at the tab's foot (over the review strip when that's up).
    pub(super) fn draw_continuous(&mut self, ui: &Ui, well: Rect, above: Option<f32>) -> Option<Rect> {
        if !self.continuous || self.loop_a.zip(self.loop_b).is_none() {
            return None;
        }
        let p = ui.painter().clone();
        let th = t();
        let passes = self.passes().to_vec();
        let (w, h) = (snap(px(430.0).min(well.width() - px(28.0))), px(146.0));
        let bottom = above.unwrap_or(well.max.y - px(12.0)) - px(8.0);
        let plate = Rect::from_min_size(ipos(well.min.x + px(14.0), bottom - h), vec2(w, h));
        paint::raised(&p, plate, Some(18.0), 6.0, Fill::Base, 0.0);
        let (lx, rx) = (snap(plate.min.x + px(16.0)), snap(plate.max.x - px(16.0)));
        let mut y = snap(plate.min.y + px(14.0));
        // the head: what this is, the pass under way, clean in a row
        caps(&p, "CONTINUOUS", pos2(lx, y), 11.0, Align2::LEFT_TOP, None);
        let streak = clean_streak(&passes);
        let head = match self.pass_so_far() {
            Some((acc, _)) => format!("pass {}  ·  {} so far", passes.len() + 1, pct(acc)),
            None => format!("pass {}", passes.len() + 1),
        };
        txt(
            &p,
            &head,
            ipos(rx, y - px(1.0)),
            12.0,
            th.text_2,
            700,
            Align2::RIGHT_TOP,
            None,
        );
        if streak >= 2 {
            txt(
                &p,
                &format!("{streak} clean in a row"),
                ipos(lx + px(98.0), y - px(1.0)),
                12.0,
                th.hit,
                800,
                Align2::LEFT_TOP,
                None,
            );
        }
        y = snap(y + px(22.0));
        // the last pass, big, and the best three beside it
        let cy = snap(y + px(20.0));
        match passes.last() {
            Some(&last) => {
                let col = if last >= CLEAN {
                    th.hit
                } else if last >= 0.6 {
                    th.late
                } else {
                    th.miss
                };
                let r = txt(
                    &p,
                    &pct(last),
                    pos2(lx, cy),
                    30.0,
                    col,
                    800,
                    Align2::LEFT_CENTER,
                    None,
                );
                txt(
                    &p,
                    "last pass",
                    pos2(lx, r.max.y + px(1.0)),
                    11.0,
                    th.caption,
                    700,
                    Align2::LEFT_TOP,
                    None,
                );
                let word = pass_word(&passes);
                txt(
                    &p,
                    &word,
                    pos2(r.max.x + px(10.0), cy),
                    12.0,
                    th.text_2,
                    700,
                    Align2::LEFT_CENTER,
                    None,
                );
            }
            None => {
                txt(
                    &p,
                    "Play along: each pass is scored when it ends",
                    pos2(lx, cy),
                    13.0,
                    th.text_2,
                    600,
                    Align2::LEFT_CENTER,
                    Some(snap(rx - lx)),
                );
            }
        }
        let top = top3(&passes);
        if !top.is_empty() {
            let tx = snap(lx + px(200.0));
            caps(
                &p,
                "BEST 3",
                pos2(tx, cy - px(17.0)),
                10.0,
                Align2::LEFT_TOP,
                None,
            );
            let mut x = tx;
            for (k, (n, a)) in top.iter().enumerate() {
                let r = txt(
                    &p,
                    &pct(*a),
                    pos2(x, cy + px(4.0)),
                    if k == 0 { 16.0 } else { 14.0 },
                    if k == 0 { th.accent_ink } else { th.text },
                    800,
                    Align2::LEFT_CENTER,
                    None,
                );
                txt(
                    &p,
                    &format!("#{n}"),
                    pos2(x, r.max.y + px(1.0)),
                    10.0,
                    th.caption,
                    700,
                    Align2::LEFT_TOP,
                    None,
                );
                x = snap(r.max.x + px(16.0));
            }
        }
        // every pass a column (the latest COLUMNS), the clean line across, the best lit
        let chart = Rect::from_min_max(
            pos2(lx, snap(plate.max.y - px(52.0))),
            pos2(rx, snap(plate.max.y - px(12.0))),
        );
        paint::inset(&p, chart, Some(6.0), 2.0, Fill::Base);
        let inner = chart.shrink2(vec2(px(6.0), px(4.0)));
        let line_y = inner.max.y - inner.height() * CLEAN as f32;
        p.hline(
            inner.x_range(),
            line_y,
            egui::Stroke::new(px(1.0), th.hit.gamma_multiply(0.45)),
        );
        let shown = &passes[passes.len().saturating_sub(COLUMNS)..];
        let first_no = passes.len() - shown.len();
        let best_no = top.first().map(|t| t.0);
        let cw = inner.width() / COLUMNS as f32;
        for (k, &a) in shown.iter().enumerate() {
            let x = inner.min.x + k as f32 * cw;
            let hh = (inner.height() * a as f32).max(px(2.0));
            let col = if a >= CLEAN {
                th.hit
            } else if a >= 0.6 {
                th.late
            } else {
                th.miss
            };
            let best = best_no == Some(first_no + k + 1);
            p.rect_filled(
                Rect::from_min_size(
                    pos2(x + px(1.0), inner.max.y - hh),
                    vec2((cw - px(2.0)).max(px(1.0)), hh),
                ),
                px(1.5),
                if best { col } else { col.gamma_multiply(0.7) },
            );
        }
        Some(plate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_best_three_best_first() {
        assert_eq!(
            top3(&[0.5, 0.9, 0.7, 0.9, 0.6]),
            vec![(2, 0.9), (4, 0.9), (3, 0.7)]
        );
        assert_eq!(top3(&[0.4]), vec![(1, 0.4)]);
        assert!(top3(&[]).is_empty());
    }

    #[test]
    fn clean_in_a_row_counts_back_from_the_last() {
        assert_eq!(clean_streak(&[0.95, 0.5, 0.9, 0.92]), 2);
        assert_eq!(clean_streak(&[0.95, 0.5]), 0);
    }

    #[test]
    fn what_a_pass_is_called() {
        assert_eq!(pass_word(&[0.6]), "the first");
        assert_eq!(pass_word(&[0.6, 0.7]), "new best");
        assert_eq!(pass_word(&[0.6, 0.8, 0.7]), "2nd best");
        assert_eq!(pass_word(&[0.9, 0.8, 0.5, 0.7]), "3rd best");
        assert_eq!(pass_word(&[0.9, 0.8, 0.7, 0.65]), "-5 on the last");
        assert_eq!(pass_word(&[0.9, 0.8, 0.7, 0.5, 0.6]), "+10 on the last");
        assert_eq!(pass_word(&[0.9, 0.8, 0.7, 0.6, 0.6]), "same as the last");
    }

    #[test]
    fn a_loop_key_is_its_ends_in_ms() {
        assert_eq!(loop_key(1.0004, 2.5), (1000, 2500));
    }
}
