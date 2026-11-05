//! The Timing page (P35.4, D92; replaces the Python app's Practice page, settingsview.py `draw_practice`): where to
//! go when it feels late. Its cards, top to bottom:
//!
//!     CALIBRATE TIMING   the timing tool's way in (`timing_tool.rs`, P36.2) and its last run
//!     SCREEN             `display_offset`, the picture only (P36.1)
//!     GUITAR DELAY       `input_offset`, shown as the delay (-input_offset, D132): when your playing counts
//!                        (Ctrl+, shorter / Ctrl+. longer; BASS DELAY on a bass)
//!     REDUCE DELAY       low-latency output (Windows' exclusive mode), the buffer, and the latency chip
//!     › Chart sync       `av_offset` (, / .), folded: only for a chart whose notes run ahead of its own music
//!
//! Level up after a clean loop, once on the Practice page, is the song's Controls panel's alone (P35.1).

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle};

use super::{SettingsView, caption_lines, centre_y, fastout, line_h, snap};
use crate::state::AppState;

/// An offset in config.json (seconds) as the page shows it: "+31 ms", "−10 ms", "0 ms".
pub fn ms_label(secs: f64) -> String {
    let ms = (secs * 1000.0).round();
    if ms == 0.0 {
        "0 ms".into()
    } else {
        format!("{ms:+.0} ms").replace('-', "\u{2212}")
    }
}

/// The instrument's delay for `input_offset` (P37.6a, D132): how late your playing reaches quickRS, which is what
/// the scorer takes off, so `input_offset` -18 ms is a delay of "18 ms" (no sign when it's late, the usual way;
/// "−5 ms" for an input that counts later).
pub fn delay_label(input_offset: f64) -> String {
    let ms = (-input_offset * 1000.0).round();
    if ms == 0.0 {
        "0 ms".into()
    } else if ms > 0.0 {
        format!("{ms:.0} ms")
    } else {
        format!("\u{2212}{:.0} ms", -ms)
    }
}

/// Every timing nudge's step (P37.6a, review 3 #5): 5 ms, Shift 1 ms, in the setup, Settings and the player alike.
pub const NUDGE_MS: i32 = 5;

impl SettingsView {
    pub(super) fn draw_timing(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let gap = px(24.0);
        // the page's title and what it's for
        text::draw(
            &p,
            "Timing",
            pos2(body.min.x, body.min.y + px(2.0)),
            Align2::LEFT_TOP,
            Style::new(22.0, 800, th.text),
            None,
        );
        let mut y = caption_lines(
            &p,
            "If the notes feel late, or your notes all score early or late: start at the top.",
            body.min.x,
            body.min.y + px(34.0),
            body.width(),
            13.0,
            2,
        ) + px(16.0);
        y = self.draw_timing_tool_card(ui, st, body, y) + gap;
        y = self.draw_offsets(ui, st, body, y) + gap;
        y = self.draw_reduce_delay(ui, st, body, y) + gap;
        y = self.draw_count_in(ui, st, body, y) + gap;
        // Chart sync, folded: the chart against its own music (not your setup)
        let av = st.config.f64("av_offset", 0.0);
        let (h, open) = self.fold(
            ui,
            body.min.x,
            y,
            body.width(),
            "chartsync",
            "Chart sync (notes against the song)",
            &format!(
                "keys , and .  \u{b7}  only if a chart's notes run ahead of or behind the music itself  \u{b7}  {}",
                ms_label(av)
            ),
        );
        if av.abs() >= 0.0005 {
            Self::changed_dot(&p, pos2(body.max.x - px(18.0), y + snap(h / 2.0)));
        }
        y += h;
        if open {
            y += px(14.0);
            let x = body.min.x + px(42.0);
            let w = (body.width() - px(42.0)).min(px(520.0));
            y += self.stepper(
                ui,
                x,
                y,
                w,
                ("Notes earlier", ",", "ms:av_offset:-5"),
                &ms_label(av),
                ("Notes later", ".", "ms:av_offset:5"),
            );
            y = caption_lines(
                &p,
                &format!(
                    "Moves the chart against the song for every song (5 ms a step). Your playing's timing is {} \
                     delay, above.",
                    st.instrument().noun_cap()
                ),
                x,
                y + px(10.0),
                body.max.x - x,
                12.0,
                2,
            );
        }
        y
    }

    /// CALIBRATE TIMING (P36.2): the timing tool's way in (RS2014 / Guitar Hero style: strum along with a
    /// click, then with gems) and its last run. Returns the card's bottom.
    pub(super) fn draw_timing_tool_card(
        &mut self,
        ui: &mut Ui,
        st: &mut AppState,
        body: Rect,
        y: f32,
    ) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let label = "Calibrate timing\u{2026}";
        let bw = widgets::button_width(&ctx, label, "", 15.0, false).max(px(200.0));
        let tw = body.width() - 2.0 * pad - bw - px(24.0);
        let last = super::timing_tool::summary(st);
        let stale = crate::timing_cal::Record::load(&st.config)
            .is_some_and(|r| crate::timing_cal::timing_stale(&r.route, &super::timing_tool::route(st)));
        let inst = st.instrument();
        // (the play-along, P38.4; the click runs and the fine-tune are its Advanced fold)
        let words = format!(
            "Play a short tune on a tab along with a click: quickRS checks it hears every note and sets {} delay \
             for you. About 30 seconds. Advanced fine-tuning (clicks, the screen, the numbers) is under it.",
            inst.noun_cap()
        );
        let lines = text::wrap(&ctx, &words, Style::new(13.0, 500, th.text_2), tw);
        let last_line = match &last {
            Some(l) => format!("Last run: {l}"),
            None => "Not measured yet.".into(),
        };
        let card = Rect::from_min_size(
            pos2(body.min.x, y),
            vec2(
                body.width(),
                2.0 * pad
                    + hh
                    + line_h(&ctx, 17.0)
                    + px(6.0)
                    + lines.len() as f32 * line_h(&ctx, 13.0)
                    + px(8.0)
                    + line_h(&ctx, 12.0),
            ),
        );
        paint::card(&p, card, 24.0, false);
        let x = card.min.x + pad;
        let yy = self.heading(&p, "CALIBRATE TIMING", x, card.min.y + pad, None, "", "");
        let tr = text::draw(
            &p,
            "Your timing",
            pos2(x, yy),
            Align2::LEFT_TOP,
            Style::new(17.0, 800, th.text),
            Some(tw),
        );
        let mut ly = tr.max.y + px(6.0);
        for line in &lines {
            ly = text::draw(
                &p,
                line,
                pos2(x, ly),
                Align2::LEFT_TOP,
                Style::new(13.0, 500, th.text_2),
                None,
            )
            .max
            .y;
        }
        text::draw(
            &p,
            &last_line,
            pos2(x, ly + px(8.0)),
            Align2::LEFT_TOP,
            Style::new(12.0, 700, if stale { th.late } else { th.caption }),
            Some(tw),
        );
        self.button(
            ui,
            Rect::from_min_size(
                pos2(card.max.x - pad - bw, centre_y(card) - px(23.0)),
                vec2(bw, px(46.0)),
            ),
            label,
            "",
            "setup:timing",
            ButtonStyle {
                size: 15.0,
                r: Some(14.0),
                primary: last.is_none() || stale,
                ..Default::default()
            },
        );
        card.max.y
    }

    /// SCREEN: `display_offset` (P36.1), the left of a pair with the delay: Earlier / the value / Later (5 ms a
    /// step); it moves only the picture, never the scoring. Returns its card's bottom.
    pub(super) fn draw_screen_card(&mut self, ui: &mut Ui, st: &mut AppState, rect: Rect) -> Option<f32> {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let v = st.config.f64("display_offset", 0.0);
        let card = Rect::from_min_size(
            rect.min,
            vec2(
                rect.width(),
                2.0 * pad + hh + px(18.0) + px(46.0) + px(14.0) + line_h(&ctx, 12.0),
            ),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let mut y = self.heading(&p, "SCREEN", x, card.min.y + pad, None, "", "");
        if v.abs() >= 0.0005 {
            let r = text::caps(
                &p,
                "SCREEN",
                pos2(x, card.min.y + pad),
                12.0,
                th.caption,
                Align2::LEFT_TOP,
                None,
            );
            Self::changed_dot(&p, pos2(r.max.x + px(10.0), centre_y(r)));
        }
        text::draw(
            &p,
            "if the notes look ahead of or behind what you hear",
            pos2(x, y - px(4.0)),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, th.caption),
            Some(wd),
        );
        y += px(18.0);
        let h = self.stepper(
            ui,
            x,
            y,
            wd,
            ("Earlier", "", "ms:display_offset:-5"),
            &ms_label(v),
            ("Later", "", "ms:display_offset:5"),
        );
        caption_lines(
            &p,
            "Only the picture: + draws the notes further on, for a screen that lags.",
            x,
            y + h + px(14.0),
            wd,
            12.0,
            1,
        );
        Some(card.max.y)
    }

    /// The offsets: Screen (when it exists) and the delay, side by side; the delay alone takes the whole row.
    fn draw_offsets(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        let gap = px(24.0);
        let colw = snap((body.width() - gap) / 2.0);
        let left = Rect::from_min_size(pos2(body.min.x, y), vec2(colw, body.height()));
        match self.draw_screen_card(ui, st, left) {
            Some(bottom) => {
                let right = Rect::from_min_size(pos2(body.min.x + colw + gap, y), vec2(colw, body.height()));
                self.draw_hits(ui, st, right, false).max(bottom)
            }
            None => self.draw_hits(
                ui,
                st,
                Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), body.height())),
                true,
            ),
        }
    }

    /// GUITAR (BASS) DELAY: `input_offset` shown as the delay (`delay_label`), Shorter / the value / Longer (5 ms a
    /// step; Ctrl+, / Ctrl+. too). `wide`: the whole row, the words beside the stepper. Returns the card's bottom.
    fn draw_hits(&mut self, ui: &mut Ui, st: &mut AppState, rect: Rect, wide: bool) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let v = st.config.f64("input_offset", 0.0);
        let card = Rect::from_min_size(
            rect.min,
            vec2(
                rect.width(),
                2.0 * pad + hh + px(18.0) + px(46.0) + if wide { 0.0 } else { px(14.0) + line_h(&ctx, 12.0) },
            ),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let inst = st.instrument();
        let title = format!("{} DELAY", inst.noun_upper());
        let mut y = self.heading(&p, &title, x, card.min.y + pad, None, "", "");
        if v.abs() >= 0.0005 {
            let r = text::caps(
                &p,
                &title,
                pos2(x, card.min.y + pad),
                12.0,
                th.caption,
                Align2::LEFT_TOP,
                None,
            );
            Self::changed_dot(&p, pos2(r.max.x + px(10.0), centre_y(r)));
        }
        text::draw(
            &p,
            &format!("how late your {} reaches quickRS", inst.noun()),
            pos2(x, y - px(4.0)),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, th.caption),
            Some(wd),
        );
        y += px(18.0);
        let sw = if wide { wd.min(px(520.0)) } else { wd };
        let h = self.stepper(
            ui,
            x,
            y,
            sw,
            ("Shorter", "Ctrl+,", "ms:input_offset:5"),
            &delay_label(v),
            ("Longer", "Ctrl+.", "ms:input_offset:-5"),
        );
        let words =
            "Your notes count this much earlier. 5 ms a step (Shift: 1 ms); Calibrate timing sets it.";
        if wide {
            caption_lines(
                &p,
                words,
                x + sw + px(24.0),
                y + px(2.0),
                wd - sw - px(24.0),
                12.0,
                3,
            );
        } else {
            caption_lines(&p, words, x, y + h + px(14.0), wd, 12.0, 1);
        }
        card.max.y
    }

    /// COUNT-IN (P37.11, D121): the one switch for every 3, 2, 1 before playing (the player's Shift C and its
    /// count-in lamp switch the same). Returns the card's bottom.
    fn draw_count_in(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let hs = Self::h_switch("lg", true);
        let card = Rect::from_min_size(
            pos2(body.min.x, y),
            vec2(body.width(), 2.0 * pad + hh + px(8.0) + hs),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let yy = self.heading(&p, "COUNT-IN", x, card.min.y + pad, None, "", "") + px(8.0);
        let on = st.config.bool("countdown", true);
        self.switch_row(
            ui,
            x,
            yy,
            wd.min(px(620.0)),
            "Count in: 3, 2, 1",
            "before the song, a loop, a review spot or a take starts; off, they play at once (Shift C)",
            on,
            "toggle:countdown",
            "lg",
            "",
            700,
        );
        if !on {
            Self::changed_dot(&p, pos2(card.max.x - px(18.0), card.min.y + pad + px(8.0)));
        }
        card.max.y
    }

    /// REDUCE DELAY: Low-latency output (highlighted: a FAST badge, a tinted plate), the buffer, the latency chip
    /// (`fastout.rs`, shared with the song's Controls panel, P41.4). Returns the card's bottom.
    fn draw_reduce_delay(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        let shown = fastout::Shown::of(st);
        let (card, act) = fastout::draw(
            ui,
            body.min.x,
            y,
            body.width(),
            fastout::Frame::Settings,
            &shown,
            "set",
        );
        if let Some(a) = act {
            self.acts.push(a.action());
        }
        card.max.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_read_in_ms() {
        assert_eq!(ms_label(0.0), "0 ms");
        assert_eq!(ms_label(0.031), "+31 ms");
        assert_eq!(ms_label(-0.01), "\u{2212}10 ms");
        assert_eq!(ms_label(0.0002), "0 ms");
    }

    /// P37.6a (D132): the delay is how late your playing reaches quickRS, the opposite of `input_offset`.
    #[test]
    fn the_delay_reads_as_lateness() {
        assert_eq!(delay_label(-0.018), "18 ms");
        assert_eq!(delay_label(0.0), "0 ms");
        assert_eq!(delay_label(0.005), "\u{2212}5 ms");
        assert_eq!(NUDGE_MS, 5);
    }
}
