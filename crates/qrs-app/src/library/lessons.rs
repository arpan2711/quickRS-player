//! The Lessons page (new in quickRS, P13.4, v2 P16.5, v3 P17.5; no Python source): the selected lesson's notes (what
//! it teaches, what it builds on, the steps of the selected item: a theory item's pages, tips) beside every lesson's
//! items, grouped by course (courses by `course_order`, lessons numbered within theirs), each with its kind and how
//! far you've got. Enter (or a double click) opens the item's song with the coach in the player's
//! console (player/coach.rs).

use eframe::egui;
use egui::{Align2, Color32, Key, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_library::lessons::{Activity, Kind};
use qrs_library::{Lesson, Riff, RiffProgress};
use qrs_ui::icons::{self, Icon};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::{pixels, px};
use qrs_ui::widgets::{self, ButtonStyle, RoundKind, button, button_width, interact};

use super::{LibView, ROW_H, Req, View, caps, draw_fade, inflate, ipx, last_part, line, line_w, txt};
use crate::state::AppState;

/// Where a riff is at: not started, under way, or done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    New,
    Going,
    Done,
}

/// An item's progress in words: "Step 3 of 5  ·  5 of 12 notes found", "Done  ·  94% at full speed".
pub fn riff_status(p: &RiffProgress, r: &Riff) -> (String, Stage) {
    if p.done {
        let s = match p.best_at(1.0) {
            Some(acc) => format!("Done  ·  {}% at full speed", (acc * 100.0).round() as i64),
            None => "Done".into(),
        };
        return (s, Stage::Done);
    }
    if !p.started() {
        return ("Not started".into(), Stage::New);
    }
    let steps = r.step_list();
    let i = r.step_index(&p.step).min(steps.len().saturating_sub(1));
    let Some(step) = steps.get(i) else {
        return ("Started".into(), Stage::Going);
    };
    if r.kind == Kind::Theory {
        return (
            format!("Page {} of {}  ·  {}", i + 1, steps.len(), step.title),
            Stage::Going,
        );
    }
    let what = match step.key.as_str() {
        "listen" => format!("listened {} of {}", p.listens.min(r.listens()), r.listens()),
        "first" => "finding the first note".to_string(),
        "notes" if r.phrase > 0 => format!("{} of {} notes found", p.found.min(r.phrase as u32), r.phrase),
        "notes" => format!("{} notes found", p.found),
        "play" => match p.best.iter().max_by_key(|b| b.0) {
            Some((pct, acc)) => format!("playing along  ·  {}% at {pct}%", (acc * 100.0).round() as i64),
            None => "playing along".to_string(),
        },
        "practise" => match p.best_at(1.0) {
            Some(acc) => format!(
                "practising  ·  best {}% at full speed",
                (acc * 100.0).round() as i64
            ),
            None => "practising".to_string(),
        },
        "record" if p.takes > 0 => format!(
            "recording  ·  {} take{}",
            p.takes,
            if p.takes == 1 { "" } else { "s" }
        ),
        "record" => "recording a take".to_string(),
        "see" => "seeing the shape".to_string(),
        "run" if p.runs > 0 => format!(
            "up and down  ·  {} run{}, {} clean",
            p.runs,
            if p.runs == 1 { "" } else { "s" },
            p.clean
        ),
        "run" => "up and down".to_string(),
        "jam" => "over the song".to_string(),
        "check" => "check the tab".to_string(),
        _ => "done".to_string(),
    };
    (
        format!("Step {} of {}  ·  {what}", i + 1, steps.len()),
        Stage::Going,
    )
}

/// What the page says the selected item gives you: "the string is given  ·  listen 5 times  ·  play along from
/// 100% (slower: 85%, 70%, 55%)".
pub fn riff_hint(r: &Riff) -> String {
    let ladder = r.ladder();
    let along = match ladder.split_first() {
        Some((top, rest)) if !rest.is_empty() => format!(
            "play along from {}% (slower: {})",
            (top * 100.0).round() as i64,
            rest.iter()
                .map(|s| format!("{}%", (s * 100.0).round() as i64))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some((top, _)) => format!("play along at {}%", (top * 100.0).round() as i64),
        None => String::new(),
    };
    match r.kind {
        Kind::Ear => format!(
            "{}  ·  listen {} times  ·  {along}",
            if r.string_hint {
                "the string is given"
            } else {
                "no string given"
            },
            r.listens()
        ),
        Kind::Learn => format!(
            "listen {} with the tab  ·  practise it whole, {}  ·  record a take: {}% at full speed",
            match r.listens() {
                1 => "once".to_string(),
                n => format!("{n} times"),
            },
            along.replacen("play along ", "", 1),
            (r.pass * 100.0).round() as i64
        ),
        Kind::Theory => {
            let count = |a: Activity, one: &str, many: &str| {
                let n = r.pages.iter().filter(|p| p.act == a).count();
                match n {
                    0 => None,
                    1 => Some(format!("1 {one}")),
                    n => Some(format!("{n} {many}")),
                }
            };
            let mut parts = vec![format!(
                "{} page{}",
                r.pages.len(),
                if r.pages.len() == 1 { "" } else { "s" }
            )];
            parts.extend(
                [
                    count(Activity::Run, "scale to run", "scales to run"),
                    count(Activity::Find, "set of notes to find", "sets of notes to find"),
                    count(Activity::Jam, "jam", "jams"),
                ]
                .into_iter()
                .flatten(),
            );
            parts.join("  ·  ")
        }
        Kind::Scale => match &r.scale {
            Some(sc) => format!("{}  ·  {} notes in the shape", sc.name, sc.shape.len()),
            None => "a scale".to_string(),
        },
    }
}

/// The heading over a kind's steps in the lesson's notes.
fn steps_head(kind: Kind) -> &'static str {
    match kind {
        Kind::Ear => "BY EAR, STEP BY STEP",
        Kind::Learn => "LEARNING IT, STEP BY STEP",
        Kind::Scale => "THE SCALE, STEP BY STEP",
        Kind::Theory => "THE PAGES",
    }
}

/// The order the page lists lessons in: by course (`course_order`), then by number within it.
pub fn lesson_order(lessons: &[std::sync::Arc<Lesson>]) -> Vec<usize> {
    qrs_library::lessons::lesson_order(lessons)
}

impl LibView {
    /// Every lesson's riffs, in order, and the row each one's song is on.
    pub(super) fn refresh_lessons(&mut self, st: &AppState) {
        self.lesson_items = lesson_order(&st.lessons)
            .into_iter()
            .flat_map(|li| (0..st.lessons[li].riffs.len()).map(move |ri| (li, ri)))
            .collect();
        self.lesson_rows = self
            .lesson_items
            .iter()
            .map(|&(li, ri)| st.riff_row(&st.lessons[li].riffs[ri]))
            .collect();
        self.sel = self.sel.min(self.lesson_items.len().saturating_sub(1));
    }

    /// The first riff not done yet (where the page opens), else the first.
    pub(super) fn first_unfinished(&self, st: &AppState) -> usize {
        let mut i = 0;
        for li in lesson_order(&st.lessons) {
            let l = &st.lessons[li];
            for r in &l.riffs {
                if !st.userdata.riff_progress(&l.id, &r.id).done {
                    return i;
                }
                i += 1;
            }
        }
        0
    }

    /// "1 lesson  ·  1 of 4 done".
    pub(super) fn lessons_count(&self, st: &AppState) -> String {
        let n = st.lessons.len();
        let (mut done, mut all) = (0, 0);
        for l in &st.lessons {
            for r in &l.riffs {
                all += 1;
                if st.userdata.riff_progress(&l.id, &r.id).done {
                    done += 1;
                }
            }
        }
        format!(
            "{n} lesson{}  ·  {done} of {all} done",
            if n == 1 { "" } else { "s" }
        )
    }

    /// The selected riff's lesson and riff.
    fn selected_riff<'a>(&self, st: &'a AppState) -> Option<(&'a qrs_library::Lesson, &'a Riff)> {
        let &(li, ri) = self.lesson_items.get(self.sel)?;
        let l = st.lessons.get(li)?;
        Some((l, l.riffs.get(ri)?))
    }

    /// What Enter does to the selected riff: start it, carry on, or play it again.
    pub(super) fn lesson_verb(&self, st: &AppState) -> &'static str {
        match self.selected_riff(st) {
            Some((l, r)) => {
                let p = st.userdata.riff_progress(&l.id, &r.id);
                if p.done {
                    "Play it again"
                } else if p.started() {
                    "Carry on"
                } else {
                    "Start it"
                }
            }
            None => "Start it",
        }
    }

    /// Open the selected riff: its song on its part, with the coach.
    pub(super) fn open_riff(&mut self, st: &mut AppState) {
        let Some((l, r)) = self.selected_riff(st).map(|(l, r)| (l.id.clone(), r.clone())) else {
            return;
        };
        let Some(g) = self.lesson_rows.get(self.sel).copied().flatten() else {
            st.say(format!("{} by {} isn't in your library", r.title, r.artist));
            return;
        };
        let e = st.song(g);
        if e.needs_keys {
            st.say(format!(
                "{} needs RS's keys: add them in Settings > Directories",
                r.title
            ));
            return;
        }
        let part = r
            .part_of(&e.arrangements)
            .map(|i| &e.arrangements[i])
            .or_else(|| last_part(st, e))
            .map(|a| a.key.clone());
        let Some(part) = part else { return };
        self.reqs.push(Req::Lesson {
            g,
            part,
            lesson: l,
            riff: r.id,
        });
    }

    /// Show the Lessons page with a lesson's riff selected (and scrolled to).
    pub fn select_riff(&mut self, st: &mut AppState, lesson: &str, riff: &str) {
        if self.view != View::Lessons {
            self.go(st, View::Lessons);
        }
        self.refresh(st);
        if let Some(i) = self.lesson_items.iter().position(|&(li, ri)| {
            st.lessons
                .get(li)
                .is_some_and(|l| l.id == lesson && l.riffs.get(ri).is_some_and(|r| r.id == riff))
        }) {
            self.sel = i;
            self.lesson_follow = true;
        }
    }

    /// Forget how far the selected item got, so it starts again from its first step.
    pub(super) fn restart_riff(&mut self, st: &mut AppState) {
        let Some((l, r)) = self.selected_riff(st).map(|(l, r)| (l.id.clone(), r.clone())) else {
            return;
        };
        if !st.userdata.riff_progress(&l, &r.id).started() {
            st.say(format!("{}: not started yet", r.title));
            return;
        }
        st.userdata.forget_riff(&l, &r.id);
        st.say(format!(
            "{} ({}) starts over next time",
            r.title,
            r.name.to_lowercase()
        ));
    }

    /// The page's keys: the arrows, Page Up / Down, Ctrl+Home / End, Enter opens the riff, F5 reads the lessons
    /// again, Esc (or Backspace) goes Home.
    pub(super) fn handle_lessons_key(
        &mut self,
        st: &mut AppState,
        area: Rect,
        key: Key,
        m: egui::Modifiers,
    ) -> bool {
        let n = self.lesson_items.len();
        let last = n.saturating_sub(1);
        match key {
            Key::Escape | Key::Backspace => self.go(st, View::Home),
            Key::ArrowDown => self.sel = (self.sel + 1).min(last),
            Key::ArrowUp => self.sel = self.sel.saturating_sub(1),
            Key::PageDown => self.sel = (self.sel + 4).min(last),
            Key::PageUp => self.sel = self.sel.saturating_sub(4),
            Key::Home if m.command => self.sel = 0,
            Key::End if m.command => self.sel = last,
            Key::Enter => self.open_riff(st),
            Key::F5 => self.act(st, area, "rescan"),
            _ => return false,
        }
        self.lesson_follow = true;
        true
    }

    /// The lesson's notes on the left, every lesson's riffs on the right.
    pub(super) fn draw_lessons(&mut self, ui: &Ui, st: &mut AppState, area: Rect, well: Rect) {
        let p = ui.painter().clone();
        let th = t();
        paint::display(&p, well, Some(24.0), 6.0);
        let inner = Rect::from_min_max(
            pos2(well.min.x + px(12.0), well.min.y + px(10.0)),
            pos2(well.max.x - px(12.0), well.max.y - px(10.0)),
        );
        if st.lessons.is_empty() {
            txt(
                &p,
                "No lessons: put lesson files in the data folder's lessons folder, then press F5.",
                pos2(inner.center().x, inner.min.y + px(40.0)),
                16.0,
                th.caption,
                600,
                Align2::CENTER_TOP,
                Some(inner.width() - px(40.0)),
            );
            return;
        }
        let notes_w = (inner.width() * 0.42).min(px(470.0));
        let notes = Rect::from_min_size(inner.min, vec2(notes_w, inner.height()));
        let div_x = notes.max.x + px(14.0);
        line(
            &p,
            pos2(div_x, inner.min.y + px(16.0)),
            pos2(div_x, inner.max.y - px(16.0)),
            line_w(),
            th.line,
        );
        let list = Rect::from_min_max(pos2(div_x + px(14.0), inner.min.y), inner.max);
        self.draw_lesson_notes(&p, st, notes);
        self.draw_riff_list(ui, st, area, list, well);
    }

    /// What the selected riff's lesson teaches: its course and title, the summary and introduction, the steps of
    /// each riff, the tips and where the method comes from. Scrolls with the wheel when it doesn't fit.
    fn draw_lesson_notes(&mut self, p: &egui::Painter, st: &AppState, r: Rect) {
        let th = t();
        let ctx = st.ctx.clone();
        self.notes_rect = r;
        let (li, ri) = self.lesson_items.get(self.sel).copied().unwrap_or((0, 0));
        let Some(l) = st.lessons.get(li).cloned() else {
            return;
        };
        let Some(item) = l.riffs.get(ri).cloned() else {
            return;
        };
        let kind = item.kind;
        let lp = p.with_clip_rect(r);
        let w = r.width() - px(12.0);
        let x = r.min.x + px(6.0);
        let top = r.min.y + px(14.0) - self.notes_scroll;
        let mut y = top;
        let para = |y: &mut f32, s: &str, size: f32, color: Color32, weight: u16, indent: f32, step: f32| {
            for line_ in text::wrap(&ctx, s, Style::new(size, weight, color), w - indent) {
                txt(
                    &lp,
                    &line_,
                    pos2(x + indent, *y),
                    size,
                    color,
                    weight,
                    Align2::LEFT_TOP,
                    None,
                );
                *y += px(step);
            }
        };
        let head = if l.course.is_empty() {
            l.label().to_uppercase()
        } else {
            format!("{}  ·  {}", l.label(), l.course).to_uppercase()
        };
        caps(&lp, &head, pos2(x, y), 11.0, Align2::LEFT_TOP, Some(w));
        y += px(20.0);
        para(&mut y, &l.title, 22.0, th.text, 800, 0.0, 30.0);
        y += px(2.0);
        if !l.summary.is_empty() {
            para(&mut y, &l.summary, 14.0, th.text, 600, 0.0, 20.0);
            y += px(10.0);
        }
        for intro in &l.intro {
            para(&mut y, intro, 13.0, th.text_2, 500, 0.0, 19.0);
            y += px(8.0);
        }
        if !l.teaches.is_empty() {
            y += px(6.0);
            caps(&lp, "YOU'LL LEARN", pos2(x, y), 11.0, Align2::LEFT_TOP, None);
            y += px(22.0);
            for what in &l.teaches {
                paint::dot(&lp, pos2(x + px(5.0), y + px(9.0)), 2.5, th.hit);
                para(&mut y, what, 13.0, th.text, 600, 18.0, 19.0);
                y += px(3.0);
            }
            y += px(4.0);
        }
        if !l.builds_on.is_empty() {
            y += px(6.0);
            let refs: Vec<String> = l
                .builds_on
                .iter()
                .map(|id| {
                    st.lessons
                        .iter()
                        .find(|o| o.id == *id)
                        .map(|o| o.reference())
                        .unwrap_or_else(|| id.clone())
                })
                .collect();
            para(
                &mut y,
                &format!("Builds on: {}", refs.join("; ")),
                13.0,
                th.caption,
                600,
                0.0,
                19.0,
            );
            y += px(6.0);
        }
        {
            y += px(6.0);
            let head = if kind == Kind::Theory {
                format!("{}: THE PAGES", item.name.to_uppercase())
            } else {
                steps_head(kind).to_string()
            };
            caps(&lp, &head, pos2(x, y), 11.0, Align2::LEFT_TOP, Some(w));
            y += px(22.0);
            for (i, step) in item.step_list().iter().enumerate() {
                let (name, what) = (step.title.as_str(), step.what.as_str());
                let c = pos2(x + px(11.0), y + px(9.0));
                paint::inset(
                    &lp,
                    Rect::from_center_size(c, Vec2::splat(px(22.0))),
                    Some(11.0),
                    2.0,
                    Fill::Base,
                );
                txt(
                    &lp,
                    &(i + 1).to_string(),
                    c,
                    12.0,
                    th.accent_ink,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
                txt(
                    &lp,
                    name,
                    pos2(x + px(32.0), y),
                    14.0,
                    th.text,
                    800,
                    Align2::LEFT_TOP,
                    Some(w - px(32.0)),
                );
                y += px(20.0);
                para(&mut y, what, 13.0, th.caption, 500, 32.0, 18.0);
                y += px(8.0);
            }
        }
        if !l.tips.is_empty() {
            y += px(6.0);
            caps(&lp, "TIPS", pos2(x, y), 11.0, Align2::LEFT_TOP, None);
            y += px(22.0);
            for tip in &l.tips {
                paint::dot(&lp, pos2(x + px(5.0), y + px(9.0)), 2.5, th.accent);
                para(&mut y, tip, 13.0, th.text_2, 500, 18.0, 19.0);
                y += px(6.0);
            }
        }
        if !l.credit.is_empty() {
            y += px(10.0);
            para(&mut y, &l.credit, 12.0, th.caption, 500, 0.0, 17.0);
        }
        let content = y - top + px(14.0);
        self.notes_max = (content - r.height()).max(0.0);
        self.notes_scroll = self.notes_scroll.clamp(0.0, self.notes_max);
        if self.notes_scroll < self.notes_max - 0.5 {
            draw_fade(
                p,
                Rect::from_min_size(pos2(r.min.x, r.max.y - px(40.0)), vec2(r.width(), px(40.0))),
                th.screen,
            );
            icons::icon(
                p,
                Icon::Down,
                pos2(r.center().x, r.max.y - px(10.0)),
                16.0,
                th.caption,
            );
        }
    }

    /// Every lesson's riffs: a heading for each lesson (its title and how many are done), then a row per riff
    /// (its number, or a tick when done; the cover; its name over the song; how far you've got). The selected row
    /// opens up with a word about the riff, a key to start over and the one to play it.
    fn draw_riff_list(&mut self, ui: &Ui, st: &mut AppState, area: Rect, r: Rect, well: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = st.ctx.clone();
        let clip = inflate(r, px(8.0), 0.0).intersect(inflate(well, -px(6.0), -px(6.0)));
        let lp = p.with_clip_rect(clip);
        let rh = px(ROW_H);
        let top = r.min.y + px(4.0) - self.lesson_scroll;
        let mut y = top;
        let mut sel_span: Option<(f32, f32)> = None;
        let mut clicked: Option<(usize, bool)> = None;
        let mut acts: Vec<&'static str> = Vec::new();
        let mut last_lesson: Option<usize> = None;
        let mut last_course: Option<String> = None;
        for i in 0..self.lesson_items.len() {
            let (li, ri) = self.lesson_items[i];
            let Some(l) = st.lessons.get(li).cloned() else {
                break;
            };
            let Some(riff) = l.riffs.get(ri).cloned() else {
                continue;
            };
            if last_course.as_deref() != Some(l.course.as_str()) {
                // the course's heading, over its lessons
                last_course = Some(l.course.clone());
                if i > 0 {
                    y += px(18.0);
                }
                if !l.course.is_empty() {
                    let cy = y + px(10.0);
                    let cr = txt(
                        &lp,
                        &l.course,
                        pos2(r.min.x + px(12.0), cy),
                        13.0,
                        th.accent_ink,
                        800,
                        Align2::LEFT_CENTER,
                        Some(r.width() - px(24.0)),
                    );
                    line(
                        &lp,
                        pos2(cr.max.x + px(12.0), cy),
                        pos2(r.max.x - px(12.0), cy),
                        line_w(),
                        th.line,
                    );
                    y += px(26.0);
                }
            }
            if last_lesson != Some(li) {
                // the lesson's heading
                if last_lesson.is_some() {
                    y += px(14.0);
                }
                last_lesson = Some(li);
                let done = l
                    .riffs
                    .iter()
                    .filter(|rf| st.userdata.riff_progress(&l.id, &rf.id).done)
                    .count();
                caps(
                    &lp,
                    &l.label().to_uppercase(),
                    pos2(r.min.x + px(12.0), y + px(4.0)),
                    11.0,
                    Align2::LEFT_TOP,
                    None,
                );
                let dr = txt(
                    &lp,
                    &format!("{done} of {} done", l.riffs.len()),
                    pos2(r.max.x - px(12.0), y + px(30.0)),
                    13.0,
                    if done == l.riffs.len() { th.hit } else { th.caption },
                    700,
                    Align2::RIGHT_CENTER,
                    None,
                );
                txt(
                    &lp,
                    &l.title,
                    pos2(r.min.x + px(12.0), y + px(30.0)),
                    17.0,
                    th.text,
                    800,
                    Align2::LEFT_CENTER,
                    Some(dr.min.x - px(16.0) - r.min.x - px(12.0)),
                );
                y += px(52.0);
            }
            let chosen = i == self.sel;
            let g = self.lesson_rows.get(i).copied().flatten();
            let prog = st.userdata.riff_progress(&l.id, &riff.id);
            let (status, stage) = riff_status(&prog, &riff);
            let about_lines: Vec<String> = if chosen && !riff.about.is_empty() {
                text::wrap(
                    &ctx,
                    &riff.about,
                    Style::new(13.0, 500, th.text_2),
                    r.width() - px(104.0),
                )
            } else {
                Vec::new()
            };
            let h = if chosen {
                rh + px(78.0) + about_lines.len() as f32 * px(19.0)
            } else {
                rh
            };
            let row = Rect::from_min_size(pos2(r.min.x, y), vec2(r.width(), h));
            if chosen {
                sel_span = Some((y - top, h));
            }
            let resp = interact(ui, row.intersect(clip), ("lesson-row", li, ri), Sense::click());
            if resp.clicked() {
                clicked = Some((i, false));
            }
            if resp.double_clicked() {
                clicked = Some((i, true));
            }
            let hot = resp.hovered();
            if chosen {
                paint::raised(&lp, inflate(row, 0.0, -px(4.0)), Some(12.0), 3.0, Fill::Base, 0.0);
            } else if hot {
                paint::rrect(&lp, inflate(row, 0.0, -px(4.0)), px(12.0), th.screen_hi);
            }
            if hot {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let next_chosen = i + 1 == self.sel;
            if !chosen && !next_chosen {
                let ly = row.max.y - pixels(1.0);
                line(
                    &lp,
                    pos2(row.min.x + px(12.0), ly),
                    pos2(row.max.x - px(12.0), ly),
                    line_w(),
                    th.line,
                );
            }
            let cy = row.min.y + rh / 2.0;
            // the riff's number in a small well, or a tick when it's done
            let badge = Rect::from_center_size(pos2(row.min.x + px(26.0), cy), Vec2::splat(px(26.0)));
            paint::inset(&lp, badge, Some(13.0), 2.0, Fill::Base);
            if stage == Stage::Done {
                icons::icon(&lp, Icon::Check, badge.center(), 15.0, th.hit);
            } else {
                txt(
                    &lp,
                    &(ri + 1).to_string(),
                    badge.center(),
                    13.0,
                    if stage == Stage::Going {
                        th.accent_ink
                    } else {
                        th.text_2
                    },
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
            let art = Rect::from_min_size(pos2(row.min.x + px(50.0), cy - px(20.0)), Vec2::splat(px(40.0)));
            let drawn = g.is_some_and(|g| {
                let e = st.song(g);
                crate::covers::draw(&lp, &st.covers, e, ipx(art.width()), art, 9.0, false)
            });
            if !drawn {
                paint::rrect(&lp, art, px(9.0), th.screen_hi);
                icons::icon(&lp, Icon::Note, art.center(), 18.0, th.caption);
            }
            // the play key (and start over) on the selected row, at its right
            let mut right = row.max.x - px(12.0);
            if chosen {
                let verb = if prog.done {
                    "Again"
                } else if prog.started() {
                    "Carry on"
                } else {
                    "Start"
                };
                let pw = button_width(&ctx, verb, "Enter", 13.0, true);
                let play = Rect::from_min_size(pos2(right - pw, cy - px(18.0)), vec2(pw, px(36.0)));
                if button(
                    ui,
                    play,
                    ("lesson-play", li, ri),
                    verb,
                    "Enter",
                    ButtonStyle {
                        primary: true,
                        icon: Some(Icon::Play),
                        size: 13.0,
                        r: Some(12.0),
                        enabled: g.is_some(),
                        ..Default::default()
                    },
                )
                .clicked()
                {
                    acts.push("lesson_open");
                }
                right = play.min.x - px(10.0);
                if prog.started() {
                    let again =
                        Rect::from_min_size(pos2(right - px(30.0), cy - px(15.0)), Vec2::splat(px(30.0)));
                    let aresp = widgets::round_button(
                        ui,
                        again,
                        ("lesson-restart", li, ri),
                        Icon::Restart,
                        13.0,
                        RoundKind::Ivory,
                        false,
                    );
                    if aresp.clicked() {
                        acts.push("lesson_restart");
                    }
                    if aresp.hovered() {
                        txt(
                            &p,
                            "Start the riff over",
                            pos2(again.center().x, again.min.y - px(6.0)),
                            12.0,
                            th.text_2,
                            700,
                            Align2::CENTER_BOTTOM,
                            None,
                        );
                    }
                    right = again.min.x - px(10.0);
                }
            }
            // its status at the right of the row (the selected one's goes under it, with more room)
            let status_col = match stage {
                Stage::Done => th.hit,
                Stage::Going => th.accent_ink,
                Stage::New => th.caption,
            };
            let name_x = art.max.x + px(12.0);
            let tw = if chosen {
                right - px(6.0) - name_x
            } else {
                let sw = (right - name_x) * 0.48;
                let sr = txt(
                    &lp,
                    &status,
                    pos2(right, cy),
                    13.0,
                    status_col,
                    700,
                    Align2::RIGHT_CENTER,
                    Some(sw),
                );
                sr.min.x - px(16.0) - name_x
            };
            // its name, and its kind in a small well after it
            let badge = riff.kind.label().to_uppercase();
            let badge_col = match riff.kind {
                Kind::Ear => th.text_2,
                Kind::Learn => th.accent_ink,
                Kind::Scale => th.hit,
                Kind::Theory => th.late,
            };
            let bw = text::width(&ctx, &badge, Style::new(9.0, 800, badge_col)) + px(14.0);
            let nr = txt(
                &lp,
                &riff.name,
                pos2(name_x, cy - px(9.0)),
                14.0,
                th.text,
                800,
                Align2::LEFT_CENTER,
                Some((tw - bw - px(8.0)).max(px(24.0))),
            );
            let pill = Rect::from_min_size(pos2(nr.max.x + px(8.0), cy - px(17.0)), vec2(bw, px(16.0)));
            paint::inset(&lp, pill, Some(8.0), 1.5, Fill::Base);
            txt(
                &lp,
                &badge,
                pill.center(),
                9.0,
                badge_col,
                800,
                Align2::CENTER_CENTER,
                None,
            );
            let (sub, sub_col) = if g.is_some() {
                (
                    format!("{}  ·  {}  ·  {}", riff.title, riff.artist, riff.part),
                    th.caption,
                )
            } else {
                (
                    format!("{}  ·  {}  ·  not in your library", riff.title, riff.artist),
                    th.miss,
                )
            };
            txt(
                &lp,
                &sub,
                pos2(name_x, cy + px(10.0)),
                12.0,
                sub_col,
                500,
                Align2::LEFT_CENTER,
                Some(tw),
            );
            if chosen {
                // how far you've got, a word about the riff, and what the lesson gives you
                let mut ay = row.min.y + rh + px(2.0);
                txt(
                    &lp,
                    &status,
                    pos2(name_x, ay),
                    13.0,
                    status_col,
                    700,
                    Align2::LEFT_TOP,
                    Some(r.max.x - px(12.0) - name_x),
                );
                ay += px(22.0);
                for line_ in &about_lines {
                    txt(
                        &lp,
                        line_,
                        pos2(name_x, ay),
                        13.0,
                        th.text_2,
                        500,
                        Align2::LEFT_TOP,
                        None,
                    );
                    ay += px(19.0);
                }
                let hint = riff_hint(&riff);
                txt(
                    &lp,
                    &hint,
                    pos2(name_x, ay + px(8.0)),
                    12.0,
                    th.caption,
                    600,
                    Align2::LEFT_TOP,
                    Some(r.max.x - px(12.0) - name_x),
                );
            }
            y += h;
        }
        // scrolling: the most, and keep the selected row in view after a key
        let content = y - top + px(10.0);
        self.lesson_max = (content - r.height()).max(0.0);
        if std::mem::take(&mut self.lesson_follow)
            && let Some((sy, sh)) = sel_span
        {
            if sy < self.lesson_scroll {
                self.lesson_scroll = (sy - px(60.0)).max(0.0);
            } else if sy + sh > self.lesson_scroll + r.height() {
                self.lesson_scroll = sy + sh - r.height() + px(10.0);
            }
        }
        self.lesson_scroll = self.lesson_scroll.clamp(0.0, self.lesson_max);
        if self.lesson_scroll < self.lesson_max - 0.5 {
            draw_fade(
                &p,
                Rect::from_min_size(
                    pos2(well.min.x + px(6.0), well.max.y - px(46.0)),
                    vec2(well.width() - px(12.0), px(40.0)),
                ),
                th.screen,
            );
        }
        if let Some((i, double)) = clicked {
            self.sel = i;
            if double {
                self.open_riff(st);
            }
        }
        for a in acts {
            self.act(st, area, a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_riffs_status_in_words() {
        let r = Riff {
            phrase: 12,
            ..Default::default()
        };
        let mut p = RiffProgress::default();
        assert_eq!(riff_status(&p, &r), ("Not started".into(), Stage::New));
        // v1's tune step reads as the first
        p.step = "tune".into();
        assert_eq!(riff_status(&p, &r).0, "Step 1 of 5  ·  listened 0 of 5");
        p.step = "listen".into();
        p.listens = 2;
        assert_eq!(riff_status(&p, &r).0, "Step 1 of 5  ·  listened 2 of 5");
        p.step = "notes".into();
        p.found = 5;
        assert_eq!(riff_status(&p, &r).0, "Step 3 of 5  ·  5 of 12 notes found");
        p.step = "play".into();
        p.record(0.7, 0.92);
        p.record(0.85, 0.6);
        assert_eq!(
            riff_status(&p, &r).0,
            "Step 4 of 5  ·  playing along  ·  60% at 85%"
        );
        p.done = true;
        assert_eq!(riff_status(&p, &r), ("Done".into(), Stage::Done));
        p.record(1.0, 0.944);
        assert_eq!(riff_status(&p, &r).0, "Done  ·  94% at full speed");
        // the other kinds' steps
        let learn = Riff {
            kind: Kind::Learn,
            ..Default::default()
        };
        // v2's Learn it reads as Practise
        let mut p = RiffProgress {
            step: "learn".into(),
            ..Default::default()
        };
        assert_eq!(riff_status(&p, &learn).0, "Step 2 of 4  ·  practising");
        p.record(1.0, 0.81);
        assert_eq!(
            riff_status(&p, &learn).0,
            "Step 2 of 4  ·  practising  ·  best 81% at full speed"
        );
        p.step = "record".into();
        p.takes = 2;
        assert_eq!(riff_status(&p, &learn).0, "Step 3 of 4  ·  recording  ·  2 takes");
        // a theory item: its pages
        let page = |t: &str| qrs_library::lessons::Page {
            title: t.into(),
            ..Default::default()
        };
        let theory = Riff {
            kind: Kind::Theory,
            pages: vec![page("The key: Bb"), page("Three chords"), page("The scale")],
            ..Default::default()
        };
        let p = RiffProgress {
            step: "p1".into(),
            ..Default::default()
        };
        assert_eq!(riff_status(&p, &theory).0, "Page 2 of 3  ·  Three chords");
        let scale = Riff {
            kind: Kind::Scale,
            ..Default::default()
        };
        let p = RiffProgress {
            step: "run".into(),
            runs: 3,
            clean: 1,
            ..Default::default()
        };
        assert_eq!(
            riff_status(&p, &scale).0,
            "Step 2 of 4  ·  up and down  ·  3 runs, 1 clean"
        );
    }

    #[test]
    fn what_an_item_gives_you() {
        let ear = Riff {
            string_hint: true,
            ..Default::default()
        };
        assert_eq!(
            riff_hint(&ear),
            "the string is given  ·  listen 5 times  ·  play along from 100% (slower: 85%, 70%, 55%)"
        );
        let learn = Riff {
            kind: Kind::Learn,
            speeds: vec![1.0],
            ..Default::default()
        };
        assert_eq!(
            riff_hint(&learn),
            "listen once with the tab  ·  practise it whole, at 100%  ·  record a take: 90% at full speed"
        );
        let learn = Riff {
            kind: Kind::Learn,
            pass: 0.8,
            listens_set: Some(2),
            ..Default::default()
        };
        assert_eq!(
            riff_hint(&learn),
            concat!(
                "listen 2 times with the tab  ·  practise it whole, from 100% (slower: 85%, 70%, 55%)  ·  ",
                "record a take: 80% at full speed"
            )
        );
        let page = |act: Activity| qrs_library::lessons::Page {
            title: "x".into(),
            act,
            ..Default::default()
        };
        let theory = Riff {
            kind: Kind::Theory,
            pages: vec![
                page(Activity::Read),
                page(Activity::Run),
                page(Activity::Find),
                page(Activity::Find),
                page(Activity::Jam),
            ],
            ..Default::default()
        };
        assert_eq!(
            riff_hint(&theory),
            "5 pages  ·  1 scale to run  ·  2 sets of notes to find  ·  1 jam"
        );
    }

    #[test]
    fn lessons_are_listed_by_course_then_number() {
        let l = |id: &str, number: u32, course: &str, order: u32| {
            std::sync::Arc::new(Lesson {
                id: id.into(),
                number,
                course: course.into(),
                course_order: order,
                ..Default::default()
            })
        };
        let ls = vec![
            l("s2", 2, "Songs", 1),
            l("e1", 1, "Ear training", 2),
            l("e2", 2, "Ear training", 2),
            l("s1", 1, "Songs", 1),
            l("f1", 1, "Rock and blues foundations", 3),
            l("mine", 1, "My own", qrs_library::lessons::COURSE_ORDER),
        ];
        let order: Vec<&str> = lesson_order(&ls).into_iter().map(|i| ls[i].id.as_str()).collect();
        assert_eq!(order, ["s1", "s2", "e1", "e2", "f1", "mine"]);
    }
}
