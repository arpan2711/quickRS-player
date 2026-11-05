//! The Saved licks page (new in quickRS, P11.1; no Python source): the stretches of songs saved from the player
//! (Ctrl+S, or Save lick on the selection's toolbar or under the loop keys), newest first, with what they were in.
//! Enter (or a double click) opens the song on that part with the lick looped; Del forgets it.

use std::collections::HashMap;

use eframe::egui;
use egui::{Align2, Key, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_library::Lick;
use qrs_ui::icons::Icon;
use qrs_ui::paint::{self, Fill};
use qrs_ui::theme::{fmt_time, t};
use qrs_ui::units::{pixels, px};
use qrs_ui::widgets::{self, ButtonStyle, RoundKind, button, button_width, interact};

use super::{
    LibView, ROW_H, Req, View, caps, centerx, centery, draw_fade, inflate, ipx, last_part, line, line_w,
    trunc_px, txt,
};
use crate::state::AppState;

/// The row a lick's song is on now: the row with the id it was saved under, else the same song by its canonical id
/// (a psarc converted to a feedpak since, say).
pub fn lick_row(st: &AppState, l: &Lick) -> Option<usize> {
    let rows = || (0..st.groups.len()).filter(|&g| st.listed(g));
    rows().find(|&g| st.ids(g).contains(&l.song)).or_else(|| {
        (!l.canonical.is_empty()).then(|| rows().find(|&g| st.groups[g].canonical == l.canonical))?
    })
}

/// What a lick is found by when you type on its page.
fn search_text(l: &Lick) -> String {
    format!(
        "{} {} {} {} {} {}",
        l.name,
        l.title,
        l.artist,
        l.part,
        l.tuning,
        l.sections.join(" ")
    )
    .to_lowercase()
}

/// "1:01 – 1:12".
pub fn time_range(l: &Lick) -> String {
    format!("{} – {}", fmt_time(l.start), fmt_time(l.end))
}

impl LibView {
    /// The page's licks (newest first, those matching the search) and their rows.
    pub(super) fn refresh_licks(&mut self, st: &AppState) {
        self.items.clear();
        let ud = &st.userdata;
        let words: Vec<String> = self
            .query
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        self.lick_items = ud
            .licks_newest_first()
            .into_iter()
            .filter(|&i| {
                let text = search_text(&ud.licks[i]);
                words.iter().all(|w| text.contains(w.as_str()))
            })
            .collect();
        // every id and canonical id to its row, once
        let mut by_id: HashMap<String, usize> = HashMap::new();
        let mut by_canon: HashMap<&str, usize> = HashMap::new();
        for g in (0..st.groups.len()).filter(|&g| st.listed(g)) {
            for id in st.ids(g) {
                by_id.entry(id).or_insert(g);
            }
            by_canon.entry(st.groups[g].canonical.as_str()).or_insert(g);
        }
        self.lick_rows = self
            .lick_items
            .iter()
            .map(|&i| {
                let l = &ud.licks[i];
                by_id.get(&l.song).copied().or_else(|| {
                    (!l.canonical.is_empty())
                        .then(|| by_canon.get(l.canonical.as_str()).copied())
                        .flatten()
                })
            })
            .collect();
        self.sel = self.sel.min(self.lick_items.len().saturating_sub(1));
    }

    pub(super) fn lick_visible_rows(&self, area: Rect) -> usize {
        let n = (ipx(self.list_rect(area).height()) / ipx(px(ROW_H))).floor();
        (n as i64).max(1) as usize
    }

    fn lick_ensure_visible(&mut self, area: Rect) {
        let n = self.lick_visible_rows(area);
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + n {
            self.top = self.sel + 1 - n;
        }
        self.top = self.top.min(self.lick_items.len().saturating_sub(n));
    }

    /// Open the selected lick: its song on its part, the stretch looped.
    pub(super) fn open_lick(&mut self, st: &mut AppState) {
        let Some(&i) = self.lick_items.get(self.sel) else {
            return;
        };
        let Some(l) = st.userdata.licks.get(i).cloned() else {
            return;
        };
        let Some(g) = lick_row(st, &l) else {
            st.say(format!("{} isn't in the library any more", l.title));
            return;
        };
        let e = st.song(g);
        if e.needs_keys {
            st.say(format!(
                "{} needs RS's keys: add them in Settings > Directories",
                l.title
            ));
            return;
        }
        let part = e
            .arrangements
            .iter()
            .find(|a| a.key == l.part_key)
            .or_else(|| {
                e.arrangements
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case(&l.part))
            })
            .or_else(|| last_part(st, e));
        let Some(part) = part.map(|a| a.key.clone()) else {
            return;
        };
        self.reqs.push(Req::Lick {
            g,
            part,
            start: l.start,
            end: l.end,
            name: l.name.clone(),
        });
    }

    pub(super) fn delete_lick(&mut self, st: &mut AppState) {
        let Some(&i) = self.lick_items.get(self.sel) else {
            return;
        };
        if let Some(l) = st.userdata.delete_lick(i) {
            st.say(format!("Deleted the lick {} ({})", l.name, l.title));
        }
    }

    /// The page's keys: the arrows, Page Up / Down, Ctrl+Home / End, Enter plays the lick, Del forgets it, Esc (or
    /// Backspace) clears the search, then goes Home.
    pub(super) fn handle_licks_key(
        &mut self,
        st: &mut AppState,
        area: Rect,
        key: Key,
        m: egui::Modifiers,
    ) -> bool {
        let n = self.lick_items.len();
        let ctrl = m.command;
        match key {
            Key::Backspace if !self.query.is_empty() => {
                if ctrl {
                    self.query.clear()
                } else {
                    self.query.pop();
                }
                self.sel = 0;
                self.top = 0;
            }
            Key::Escape if !self.query.is_empty() => self.query.clear(),
            Key::Escape | Key::Backspace => self.go(st, View::Home),
            Key::ArrowDown => self.sel = (self.sel + 1).min(n.saturating_sub(1)),
            Key::ArrowUp => self.sel = self.sel.saturating_sub(1),
            Key::PageDown => self.sel = (self.sel + self.lick_visible_rows(area)).min(n.saturating_sub(1)),
            Key::PageUp => self.sel = self.sel.saturating_sub(self.lick_visible_rows(area)),
            Key::Home if ctrl => self.sel = 0,
            Key::End if ctrl => self.sel = n.saturating_sub(1),
            Key::Enter => self.open_lick(st),
            Key::R if ctrl => self.random(st, area),
            Key::Delete => self.delete_lick(st),
            Key::F5 => self.act(st, area, "rescan"),
            _ => return false,
        }
        self.sel = self.sel.min(self.lick_items.len().saturating_sub(1));
        self.lick_ensure_visible(area);
        true
    }

    /// x of each column: cover, lick (its name over the song), part, time (left) and saved (at its right edge).
    fn lick_columns(r: Rect) -> [f32; 5] {
        let s = super::scale();
        let x = r.min.x + px(12.0);
        let inner = r.width() - px(24.0);
        let (part_w, time_w, saved_w) = (
            trunc_px((210.0 * s).min(ipx(inner) * 0.2)),
            trunc_px((130.0 * s).min(ipx(inner) * 0.14)),
            trunc_px((150.0 * s).min(ipx(inner) * 0.16)),
        );
        let saved = x + inner;
        let time = saved - saved_w - time_w;
        let part = time - part_w;
        [x, x + px(56.0), part, time, saved]
    }

    /// The licks on a screen, as the song list: a header of columns, then a row each (the cover, the lick's name
    /// over its song and artist, the part and tuning, the time, when it was saved); the selected one raised, with
    /// Play and a key to delete it.
    pub(super) fn draw_licks(&mut self, ui: &Ui, st: &mut AppState, area: Rect, well: Rect) {
        let p = ui.painter().clone();
        let th = t();
        paint::display(&p, well, Some(24.0), 6.0);
        let r = Rect::from_min_size(
            pos2(well.min.x + px(12.0), well.min.y + px(10.0) + px(34.0)),
            vec2(well.width() - px(24.0), well.height() - px(10.0) - px(34.0)),
        );
        self.list = Some(r);
        let [c_art, c_name, c_part, c_time, c_saved] = Self::lick_columns(r);
        let hy = well.min.y + px(10.0) + px(17.0);
        caps(&p, "LICK", pos2(c_name, hy), 11.0, Align2::LEFT_CENTER, None);
        // the Random key over the covers, just left of LICK (P37.10)
        let rk = Rect::from_center_size(pos2(c_art + px(20.0), hy), vec2(px(36.0), px(28.0)));
        let random = !self.lick_items.is_empty()
            && super::random_key(ui, rk, "lick-random", &self.random_tip()).clicked();
        caps(&p, "PART", pos2(c_part, hy), 11.0, Align2::LEFT_CENTER, None);
        caps(&p, "TIME", pos2(c_time, hy), 11.0, Align2::LEFT_CENTER, None);
        caps(&p, "SAVED", pos2(c_saved, hy), 11.0, Align2::RIGHT_CENTER, None);
        let rh = px(ROW_H);
        let clip = inflate(r, px(8.0), 0.0).intersect(inflate(well, -px(6.0), -px(6.0)));
        let lp = p.with_clip_rect(clip);
        let mut acts: Vec<&'static str> = Vec::new();
        let mut clicked: Option<(usize, bool)> = None;
        let mut delete: Option<usize> = None;
        let mut y = r.min.y;
        let mut i = self.top;
        while i < self.lick_items.len() && ipx(y) < ipx(r.max.y) {
            let li = self.lick_items[i];
            let Some(l) = st.userdata.licks.get(li).cloned() else {
                break;
            };
            let g = self.lick_rows.get(i).copied().flatten();
            let row = Rect::from_min_size(pos2(r.min.x, y), vec2(r.width(), rh));
            let next_chosen = i + 1 == self.sel;
            let chosen = i == self.sel;
            let resp = interact(ui, row.intersect(r), ("lick-row", li), Sense::click());
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
            let cy = centery(row);
            let art_r = Rect::from_min_size(pos2(c_art, cy - px(20.0)), Vec2::splat(px(40.0)));
            let drawn = g.is_some_and(|g| {
                let e = st.song(g);
                crate::covers::draw(&lp, &st.covers, e, ipx(art_r.width()), art_r, 9.0, false)
            });
            if !drawn {
                paint::rrect(&lp, art_r, px(9.0), th.screen_hi);
            }
            let tw = c_part - px(16.0) - c_name;
            txt(
                &lp,
                &l.name,
                pos2(c_name, cy - px(9.0)),
                14.0,
                th.text,
                800,
                Align2::LEFT_CENTER,
                Some(tw),
            );
            let (sub, sub_col) = if g.is_some() {
                (format!("{}  ·  {}", l.title, l.artist), th.caption)
            } else {
                (format!("{}  ·  not in the library", l.title), th.miss)
            };
            txt(
                &lp,
                &sub,
                pos2(c_name, cy + px(10.0)),
                12.0,
                sub_col,
                500,
                Align2::LEFT_CENTER,
                Some(tw),
            );
            let part = if l.tuning.is_empty() {
                l.part.clone()
            } else {
                format!("{}  ·  {}", l.part, l.tuning)
            };
            txt(
                &lp,
                &part,
                pos2(c_part, cy),
                13.0,
                th.text_2,
                500,
                Align2::LEFT_CENTER,
                Some(c_time - c_part - px(14.0)),
            );
            if chosen {
                // the selected lick: a key to delete it, then Play at its end (over the saved date)
                let pw = button_width(&st.ctx, "Play", "Enter", 13.0, true);
                let play =
                    Rect::from_min_size(pos2(row.max.x - px(8.0) - pw, cy - px(18.0)), vec2(pw, px(36.0)));
                if button(
                    ui,
                    play,
                    ("lick-play", li),
                    "Play",
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
                    acts.push("lick_open");
                }
                let del = Rect::from_min_size(
                    pos2(play.min.x - px(10.0) - px(30.0), cy - px(15.0)),
                    Vec2::splat(px(30.0)),
                );
                let dresp =
                    widgets::round_button(ui, del, ("lick-del", li), Icon::X, 12.0, RoundKind::Ivory, false);
                if dresp.clicked() {
                    delete = Some(i);
                }
                if dresp.hovered() {
                    txt(
                        &p,
                        "Delete  (Del)",
                        pos2(centerx(del), del.min.y - px(6.0)),
                        12.0,
                        th.text_2,
                        700,
                        Align2::CENTER_BOTTOM,
                        None,
                    );
                }
                txt(
                    &lp,
                    &time_range(&l),
                    pos2(c_time, cy),
                    13.0,
                    th.text,
                    700,
                    Align2::LEFT_CENTER,
                    Some(del.min.x - px(10.0) - c_time),
                );
            } else {
                txt(
                    &lp,
                    &time_range(&l),
                    pos2(c_time, cy),
                    13.0,
                    th.text,
                    700,
                    Align2::LEFT_CENTER,
                    Some(c_saved - px(110.0) - c_time),
                );
                txt(
                    &lp,
                    &crate::home::fmt_when(Some(l.saved), false),
                    pos2(c_saved, cy),
                    13.0,
                    th.text_2,
                    500,
                    Align2::RIGHT_CENTER,
                    None,
                );
            }
            if hot {
                st.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            y += rh;
            i += 1;
        }
        let fade = Rect::from_min_size(
            pos2(well.min.x + px(6.0), well.max.y - px(46.0)),
            vec2(well.width() - px(12.0), px(40.0)),
        );
        draw_fade(&p, fade, th.screen);
        if self.lick_items.is_empty() {
            let msg = if !self.query.is_empty() {
                "No licks match your search."
            } else {
                "No saved licks yet: in a song, select some bars (Ctrl-drag over the tab) or set a loop with [ and ], then press Ctrl+S."
            };
            txt(
                &p,
                msg,
                pos2(centerx(well), r.min.y + px(40.0)),
                16.0,
                th.caption,
                600,
                Align2::CENTER_TOP,
                Some(r.width() - px(40.0)),
            );
        }
        if let Some(i) = delete {
            self.sel = i;
            self.delete_lick(st);
        } else if let Some((i, double)) = clicked {
            self.sel = i;
            if double {
                self.open_lick(st);
            }
        }
        if random {
            self.random(st, area);
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
    fn a_lick_is_found_by_its_context() {
        let l = Lick {
            song: "a_p.psarc|".into(),
            part: "Lead".into(),
            start: 61.4,
            end: 72.9,
            title: "Here Without You".into(),
            artist: "3 Doors Down".into(),
            tuning: "Eb Standard".into(),
            sections: vec!["Chorus".into(), "Verse 2".into()],
            name: "Chorus · bars 29-31".into(),
            ..Default::default()
        };
        assert_eq!(time_range(&l), "1:01 – 1:12");
        let s = search_text(&l);
        for w in ["chorus", "verse", "doors", "lead", "eb", "29-31"] {
            assert!(s.contains(w), "{w}");
        }
    }
}
