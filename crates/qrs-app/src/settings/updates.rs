//! The About page (P20.1, D36; the Updates page until P35.3, D92; new in quickRS): this version, the check on start,
//! Check for updates, Update now (the work is `crate::update`'s; this page draws its phase and queues `update:<what>`
//! actions), Report a bug and the log file, the first-time setup again and Reset all settings… (asks first).

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle};

use super::{SettingsView, caption_lines, snap};
use crate::state::AppState;
use crate::update::{self, Phase, Version};

const BUG: &str = "Something wrong, or an idea? Report a bug opens a new issue on quickRS's GitHub page in your browser, with this version and your system filled in: say what happened and what you did just before. If quickRS crashed or showed an error, attach the log file (Show the log file). Nothing is sent without you.";

const ABOUT: &str = "Update now downloads the new version from quickRS's GitHub releases, checks it against the \
    release's SHA256SUMS.txt (a download that doesn't match is refused), puts the new program files in place of the \
    old ones and starts quickRS again. Your keys, settings, scores, licks and song folders are never touched. If \
    quickRS's folder can't be written to, the release page opens in your browser instead.";

impl SettingsView {
    pub(super) fn draw_about(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (gap, pad) = (px(24.0), px(22.0));
        let colw = snap((body.width() - gap) / 2.0);
        let wd = colw - 2.0 * pad;
        let hh = Self::h_heading(&ctx);
        let phase = st.updater.phase();
        if phase.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        let status = match &phase {
            Phase::Idle => "Not checked since quickRS started".to_string(),
            other => update::describe(other),
        };
        let cap = Style::new(13.0, 500, th.caption);
        let lines = text::wrap(&ctx, &status, cap, wd);
        let bh = px(46.0);
        let card = Rect::from_min_size(
            body.min,
            vec2(
                colw,
                2.0 * pad
                    + hh
                    + px(34.0)
                    + Self::h_switch("md", true)
                    + px(20.0)
                    + bh
                    + px(14.0)
                    + lines.len().max(1) as f32 * px(19.0),
            ),
        );
        paint::card(&p, card, 24.0, false);
        let x = card.min.x + pad;
        let mut y = self.heading(&p, "QUICKRS", x, card.min.y + pad, None, "", "");
        text::draw(
            &p,
            &format!("Version {}", Version::current()),
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(17.0, 800, th.text),
            Some(wd),
        );
        y += px(34.0);
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "Check for updates on start",
            "at most once a day, from quickRS's GitHub releases",
            st.config.bool(update::KEY_CHECK, true),
            &format!("toggle:{}", update::KEY_CHECK),
            "md",
            "",
            700,
        ) + px(20.0);
        // the buttons, left to right: Check for updates, then Update now and its notes when there's one
        let style = |primary: bool| ButtonStyle {
            size: 14.0,
            r: Some(14.0),
            primary,
            enabled: !phase.busy(),
            ..Default::default()
        };
        let mut bx = x;
        let mut buttons = vec![("Check for updates", "update:check", false)];
        if matches!(phase, Phase::Available(_)) {
            buttons.push(("Update now", "update:install", true));
            buttons.push(("Release notes", "update:notes", false));
        } else if matches!(phase, Phase::Failed(_) | Phase::Browser) {
            buttons.push(("Release page", "update:page", false));
        }
        for (label, action, primary) in buttons {
            let bw = widgets::button_width(&ctx, label, "", 14.0, false);
            if bx + bw > x + wd + px(1.0) {
                break;
            }
            self.button(
                ui,
                Rect::from_min_size(pos2(bx, y), vec2(bw, bh)),
                label,
                "",
                action,
                style(primary),
            );
            bx += bw + px(10.0);
        }
        y += bh + px(14.0);
        let failed = matches!(phase, Phase::Failed(_));
        for line in &lines {
            text::draw(
                &p,
                line,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(
                    13.0,
                    if failed { 700 } else { 500 },
                    if failed { th.text } else { th.caption },
                ),
                None,
            );
            y += px(19.0);
        }
        // the right card: what an update does, and a skipped version
        let skipped = st.config.str(update::KEY_SKIP, "");
        let x2 = body.min.x + colw + gap;
        let about = text::wrap(&ctx, ABOUT, cap, wd);
        let mut h2 = 2.0 * pad + hh + about.len() as f32 * px(19.0);
        if !skipped.is_empty() {
            h2 += px(18.0) + px(19.0) + px(12.0) + bh;
        }
        let card2 = Rect::from_min_size(pos2(x2, body.min.y), vec2(colw, h2));
        paint::card(&p, card2, 24.0, false);
        let x2 = x2 + pad;
        let y2 = self.heading(&p, "WHAT AN UPDATE CHANGES", x2, card2.min.y + pad, None, "", "");
        let mut y2 = caption_lines(&p, ABOUT, x2, y2, wd, 13.0, about.len());
        if !skipped.is_empty() {
            y2 += px(18.0);
            text::draw(
                &p,
                &format!("Version {skipped} is skipped: it isn't offered on start."),
                pos2(x2, y2),
                Align2::LEFT_TOP,
                cap,
                Some(wd),
            );
            y2 += px(19.0) + px(12.0);
            let label = "Offer it again";
            let bw = widgets::button_width(&ctx, label, "", 14.0, false);
            self.button(
                ui,
                Rect::from_min_size(pos2(x2, y2), vec2(bw, bh)),
                label,
                "",
                "update:unskip",
                style(false),
            );
        }
        // under both: report a bug (a new GitHub issue, filled in) and the log file to attach
        let top = card.max.y.max(card2.max.y) + gap;
        let bw_full = 2.0 * colw + gap;
        let bug_lines = text::wrap(&ctx, BUG, cap, bw_full - 2.0 * pad);
        let card3 = Rect::from_min_size(
            pos2(body.min.x, top),
            vec2(
                bw_full,
                2.0 * pad + hh + bug_lines.len() as f32 * px(19.0) + px(14.0) + bh,
            ),
        );
        paint::card(&p, card3, 24.0, false);
        let y3 = self.heading(&p, "REPORT A BUG", x, card3.min.y + pad, None, "", "");
        let y3 = caption_lines(&p, BUG, x, y3, bw_full - 2.0 * pad, 13.0, bug_lines.len()) + px(14.0);
        let mut bx = x;
        for (label, action, primary) in [
            ("Report a bug", "update:bug", true),
            ("Show the log file", "update:log", false),
        ] {
            let bw = widgets::button_width(&ctx, label, "", 14.0, false);
            self.button(
                ui,
                Rect::from_min_size(pos2(bx, y3), vec2(bw, bh)),
                label,
                "",
                action,
                ButtonStyle {
                    size: 14.0,
                    r: Some(14.0),
                    primary,
                    ..Default::default()
                },
            );
            bx += bw + px(10.0);
        }
        // the first-time setup again (P23.14; at the panel's top before P35.3) and Reset all (P35.7), side by side
        let top = card3.max.y + gap;
        let setup_words = "The welcome's steps again: the look, your guitar, low-latency output, the note widget, \
                           your folders and keys. Nothing changes until you change it.";
        let reset_words = "Every page but your tone back to its defaults. Kept: your song folders, RS's keys, \
                           scores, your tones and each input's level check. It asks first.";
        let sl = text::wrap(&ctx, setup_words, cap, wd);
        let rl = text::wrap(&ctx, reset_words, cap, wd);
        let h = 2.0 * pad + hh + sl.len().max(rl.len()) as f32 * px(19.0) + px(14.0) + bh;
        for (i, (title, words, n, label, action, danger)) in [
            (
                "FIRST-TIME SETUP",
                setup_words,
                sl.len(),
                "Run the first-time setup again",
                "firstsetup",
                false,
            ),
            (
                "RESET",
                reset_words,
                rl.len(),
                "Reset all settings\u{2026}",
                "reset:all",
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let card = Rect::from_min_size(pos2(body.min.x + i as f32 * (colw + gap), top), vec2(colw, h));
            paint::card(&p, card, 24.0, false);
            let cx = card.min.x + pad;
            let cy = self.heading(&p, title, cx, card.min.y + pad, None, "", "");
            caption_lines(&p, words, cx, cy, wd, 13.0, n);
            let bw = widgets::button_width(&ctx, label, "", 14.0, false);
            self.button(
                ui,
                Rect::from_min_size(pos2(cx, card.max.y - pad - bh), vec2(bw, bh)),
                label,
                "",
                action,
                ButtonStyle {
                    size: 14.0,
                    r: Some(14.0),
                    danger,
                    ..Default::default()
                },
            );
        }
        top + h
    }
}
