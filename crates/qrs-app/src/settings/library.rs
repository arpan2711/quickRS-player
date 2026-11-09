//! The Library page (port of settingsview.py `draw_library`, made easier to set up from the 0.1.0 feedback, P20.2):
//!
//!     SONG FOLDERS     every folder searched, where it came from, Remove; Add folder... (the system's picker) and
//!                      Rescan; "Search RS's dlc folder" (off: feedpaks only)
//!     SONG LIST        expand & preview; which copy plays when a song is there as both a psarc and a feedpak
//!     RS KEYS   whether keys were found and where it looked; the two keys typed or pasted, or a keys file
//!                      loaded, saved as keys.json (songs needing them are read again straight away)
//!     RECORDINGS       where R's recordings of what you hear and Ctrl R's song takes go: each folder with Change,
//!                      Open and Default (config.json `recordings_dir`, `takes_dir`; P23.9)
//!
//! A folder dropped on the window is still added too.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_library::Prefer;
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle};

use super::pages::seg_opts;
use super::{SettingsView, caption_lines, centre_y, line_h, snap};
use crate::home::songs;
use crate::keyfile::{self, KeyState};
use crate::state::AppState;

/// The two key boxes' ids (Settings' own keys are off while one has the focus).
pub fn key_box_id(which: &str) -> egui::Id {
    egui::Id::new(("set-keybox", which))
}

/// Where a folder in the list came from.
fn origin(st: &AppState, dlc: &[PathBuf], d: &Path) -> &'static str {
    use qrs_library::config::same_dir;
    if st.config.added_dirs().iter().any(|a| same_dir(a, d)) {
        "added by you"
    } else if dlc.iter().any(|a| same_dir(a, d)) {
        "RS's dlc folder, found by itself"
    } else {
        "feedpaks next to RS's dlc folder, found by itself"
    }
}

/// A one-line text box sunk in the card (the keys).
pub(super) fn text_box(
    ui: &mut Ui,
    rect: Rect,
    which: &str,
    value: &mut String,
    hint: &str,
) -> egui::Response {
    let th = t();
    paint::inset(ui.painter(), rect, Some(12.0), 3.0, paint::Fill::Base);
    let edit = egui::TextEdit::singleline(value)
        .id(key_box_id(which))
        .frame(egui::Frame::NONE)
        .font(egui::FontId::new(14.0, text::family(500)))
        .text_color(th.text)
        .hint_text(egui::RichText::new(hint).color(th.caption))
        .desired_width(rect.width() - px(28.0));
    let inner = Rect::from_min_size(
        pos2(rect.min.x + px(14.0), centre_y(rect) - px(10.0)),
        vec2(rect.width() - px(28.0), px(20.0)),
    );
    ui.put(inner, edit)
}

impl SettingsView {
    pub(super) fn draw_library(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let bottom = self.draw_folders(ui, st, body);
        let keys = Rect::from_min_size(
            pos2(body.min.x, bottom + px(24.0)),
            vec2(body.width(), body.height()),
        );
        // the keys, folded to one line once they're found (P35.5): open, the whole card
        let found = matches!(st.key_state, KeyState::Found { .. });
        let bottom = if found && !qrs_ui::theme::classic() {
            let from = match &st.key_state {
                KeyState::Found { path: Some(p), .. } => format!(
                    "read from {}",
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
                _ => "from the environment".into(),
            };
            let w = snap(keys.width().min(760.0));
            let (h, open) = self.fold(
                ui,
                keys.min.x,
                keys.min.y,
                w,
                "keys",
                "RS keys: found",
                &format!("psarc songs play  \u{b7}  {from}  \u{b7}  open to change them"),
            );
            if open {
                let card = Rect::from_min_size(pos2(keys.min.x, keys.min.y + h + px(12.0)), keys.size());
                self.draw_keys(ui, st, card)
            } else {
                keys.min.y + h
            }
        } else {
            self.draw_keys(ui, st, keys)
        };
        let rec = Rect::from_min_size(
            pos2(body.min.x, bottom + px(24.0)),
            vec2(body.width(), body.height()),
        );
        self.draw_rec_folders(ui, st, rec)
    }

    /// RECORDINGS: the two folders, each with Change, Open and Default.
    fn draw_rec_folders(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        use qrs_library::config::{RECORDINGS_DIR, TAKES_DIR};
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let wd_all = snap(body.width().min(760.0));
        let l14 = line_h(&ctx, 14.0);
        let l12 = line_h(&ctx, 12.0);
        let row_h = l14 + px(4.0) + l12 + px(10.0) + px(52.0);
        let card = Rect::from_min_size(
            body.min,
            vec2(wd_all, pad + hh + 2.0 * row_h + px(18.0) + px(8.0) + pad),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let mut y = self.heading(
            &p,
            "RECORDINGS",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            "named by the date and time; nothing to choose when you record",
            "",
        );
        let root = st.data.root.clone();
        let rows = [
            (
                RECORDINGS_DIR,
                "What you hear",
                "R",
                "in a song or the amp room: the song, you through the rig, as a WAV",
                st.config.recordings_dir(&root),
                st.config.folder(RECORDINGS_DIR).is_none(),
            ),
            (
                TAKES_DIR,
                "Song takes",
                "Ctrl R",
                "a loop recorded and scored, with its analysis; the last 10 of a loop are kept",
                st.config.takes_dir(&root),
                st.config.folder(TAKES_DIR).is_none(),
            ),
        ];
        for (i, (key, title, cap, about, dir, default)) in rows.into_iter().enumerate() {
            if i > 0 {
                y += px(18.0);
            }
            let tr = text::draw(
                &p,
                title,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(14.0, 800, th.text),
                None,
            );
            qrs_ui::keys::keycap(
                &p,
                cap,
                pos2(tr.max.x + px(8.0), tr.center().y),
                Align2::LEFT_CENTER,
                10.0,
                qrs_ui::keys::CapOn::Material,
            );
            y += l14 + px(4.0);
            text::draw(
                &p,
                about,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(12.0, 500, th.caption),
                Some(wd),
            );
            y += l12 + px(10.0);
            let bx = Rect::from_min_size(pos2(x, y), vec2(wd, px(52.0)));
            paint::display(&p, bx, Some(14.0), 4.0);
            let bs = ButtonStyle {
                size: 12.0,
                r: Some(10.0),
                ..Default::default()
            };
            let mut bxr = bx.max.x - px(10.0);
            let mut place = |label: &str| {
                let w = widgets::button_width(&ctx, label, "", 12.0, false);
                let r = Rect::from_min_size(pos2(bxr - w, centre_y(bx) - px(15.0)), vec2(w, px(30.0)));
                bxr = r.min.x - px(8.0);
                r
            };
            let reset = place("Default");
            let open = place("Open");
            let change = place("Change\u{2026}");
            self.button(
                ui,
                change,
                "Change\u{2026}",
                "",
                &format!("recdir_change:{key}"),
                bs,
            );
            self.button(ui, open, "Open", "", &format!("recdir_open:{key}"), bs);
            self.button(
                ui,
                reset,
                "Default",
                "",
                &format!("recdir_reset:{key}"),
                ButtonStyle {
                    enabled: !default,
                    ..bs
                },
            );
            let tw = change.min.x - px(12.0) - (bx.min.x + px(14.0));
            text::draw(
                &p,
                &dir.display().to_string(),
                pos2(bx.min.x + px(14.0), centre_y(bx) - px(8.0)),
                Align2::LEFT_CENTER,
                Style::new(13.0, 600, th.text),
                Some(tw),
            );
            text::draw(
                &p,
                if default { "the default" } else { "chosen by you" },
                pos2(bx.min.x + px(14.0), centre_y(bx) + px(10.0)),
                Align2::LEFT_CENTER,
                Style::new(11.0, 500, th.caption),
                Some(tw),
            );
            y = bx.max.y;
        }
        y += px(8.0);
        let _ = y;
        card.max.y
    }

    /// Change, open or reset a recordings folder (`key`: config.json's `recordings_dir` or `takes_dir`).
    pub(super) fn rec_folder(&mut self, st: &mut AppState, verb: &str, key: &str) {
        use qrs_library::config::{RECORDINGS_DIR, TAKES_DIR};
        if key != RECORDINGS_DIR && key != TAKES_DIR {
            return;
        }
        let root = st.data.root.clone();
        let now = if key == TAKES_DIR {
            st.config.takes_dir(&root)
        } else {
            st.config.recordings_dir(&root)
        };
        let what = if key == TAKES_DIR {
            "Song takes"
        } else {
            "Recordings"
        };
        match verb {
            "recdir_change" => {
                let title = format!("Where {} go", what.to_lowercase());
                if let Some(dir) = crate::dialogs::pick_folder_titled(&title, Some(&now)) {
                    st.config.set_folder(key, Some(&dir));
                    st.save_config();
                    self.say(format!("{what} go to {}", dir.display()));
                } else if crate::dialogs::unavailable() {
                    self.say(format!(
                        "No file picker on this desktop: set it by hand in config.json ({key}), or install                          xdg-desktop-portal-gtk"
                    ));
                }
            }
            "recdir_open" => crate::recording::open_dir(&now),
            "recdir_reset" => {
                st.config.set_folder(key, None);
                st.save_config();
                let dir = if key == TAKES_DIR {
                    st.config.takes_dir(&root)
                } else {
                    st.config.recordings_dir(&root)
                };
                self.say(format!("{what} go to {} again", dir.display()));
            }
            _ => {}
        }
    }

    /// SONG FOLDERS and SONG LIST.
    fn draw_folders(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let dlc = self
            .dlc_found
            .get_or_insert_with(qrs_library::config::rs_dlc_dirs)
            .clone();
        let row_h = px(46.0);
        let rows = st.dirs.len().max(1) as f32;
        let wd_all = snap(body.width().min(760.0));
        let box_h = 2.0 * px(8.0) + rows * row_h;
        let both = st
            .groups
            .iter()
            .any(|g| g.has_psarc(&st.songs) && g.has_feedpak(&st.songs));
        let seg_h = px(44.0) + 2.0 * px(6.0);
        let prefer_h = if both {
            px(26.0) + hh + seg_h + px(10.0) + line_h(&ctx, 13.0)
        } else {
            0.0
        };
        let card = Rect::from_min_size(
            body.min,
            vec2(
                wd_all,
                pad + hh
                    + box_h
                    + px(16.0)
                    + px(46.0)
                    + px(10.0)
                    + line_h(&ctx, 13.0)
                    + px(16.0)
                    + Self::h_switch("md", true)
                    + px(26.0)
                    + hh
                    + Self::h_switch("md", true)
                    + prefer_h
                    + pad,
            ),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let found = format!("{} found", songs(st.groups.len()));
        let mut y = self.heading(
            &p,
            "SONG FOLDERS",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &found,
            "",
        );
        let bx = Rect::from_min_size(pos2(x, y), vec2(wd, box_h));
        paint::display(&p, bx, Some(14.0), 4.0);
        if st.dirs.is_empty() {
            text::draw(
                &p,
                "No folders yet: add the folder your songs are in.",
                pos2(bx.min.x + px(14.0), bx.min.y + px(8.0) + snap(row_h / 2.0)),
                Align2::LEFT_CENTER,
                Style::new(13.0, 500, th.caption),
                Some(bx.width() - px(28.0)),
            );
        }
        for (i, d) in st.dirs.clone().iter().enumerate() {
            let row = Rect::from_min_size(
                pos2(bx.min.x + px(14.0), bx.min.y + px(8.0) + i as f32 * row_h),
                vec2(bx.width() - px(22.0), row_h),
            );
            let rw = widgets::button_width(&ctx, "Remove", "", 12.0, false);
            let rb = Rect::from_min_size(pos2(row.max.x - rw, centre_y(row) - px(15.0)), vec2(rw, px(30.0)));
            self.button(
                ui,
                rb,
                "Remove",
                "",
                &format!("rmfolder:{i}"),
                ButtonStyle {
                    size: 12.0,
                    r: Some(10.0),
                    ..Default::default()
                },
            );
            let tw = rb.min.x - px(12.0) - row.min.x;
            text::draw(
                &p,
                &d.display().to_string(),
                pos2(row.min.x, centre_y(row) - px(8.0)),
                Align2::LEFT_CENTER,
                Style::new(13.0, 600, th.text),
                Some(tw),
            );
            text::draw(
                &p,
                origin(st, &dlc, d),
                pos2(row.min.x, centre_y(row) + px(10.0)),
                Align2::LEFT_CENTER,
                Style::new(11.0, 500, th.caption),
                Some(tw),
            );
        }
        y = bx.max.y + px(16.0);
        let bs = ButtonStyle {
            size: 14.0,
            r: Some(14.0),
            ..Default::default()
        };
        let aw = widgets::button_width(&ctx, "Add folder…", "", 14.0, false);
        let add = Rect::from_min_size(pos2(x, y), vec2(aw, px(46.0)));
        self.button(
            ui,
            add,
            "Add folder…",
            "",
            "addfolder",
            ButtonStyle {
                primary: st.dirs.is_empty(),
                ..bs
            },
        );
        let bw = widgets::button_width(&ctx, "Rescan folders", "F5", 14.0, false);
        let rescan = Rect::from_min_size(pos2(add.max.x + px(12.0), y), vec2(bw, px(46.0)));
        self.button(ui, rescan, "Rescan folders", "F5", "rescan", bs);
        y += px(46.0) + px(10.0);
        let note = if st.scanning() {
            let done = st.library.done.load(std::sync::atomic::Ordering::Relaxed);
            let total = st.library.total.load(std::sync::atomic::Ordering::Relaxed);
            format!("Reading songs… {done} of {total}")
        } else {
            "A folder dropped on the window is added too.".into()
        };
        y = caption_lines(&p, &note, x, y, wd, 13.0, 1) + px(16.0);
        // (the hint says what the switch does now: it said "off: ..." while on, P37.4)
        let hint = if dlc.is_empty() {
            "RS2014 isn't found on this computer"
        } else if st.config.search_dlc() {
            "on: RS's own songs folder too (its psarcs need keys)"
        } else {
            "off: only the folders you added (feedpaks play without keys)"
        };
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "Search RS's dlc folder",
            hint,
            st.config.search_dlc(),
            "searchdlc",
            "md",
            "",
            700,
        );
        y += px(26.0);
        y = self.heading(&p, "SONG LIST", x, y, None, "", "");
        y += self.switch_row(
            ui,
            x,
            y,
            wd,
            "Expand & preview the selected song",
            "bigger cover and the song's preview clip",
            st.config.bool("select_preview", true),
            "toggle:select_preview",
            "md",
            "",
            700,
        );
        if both {
            y += px(26.0);
            y = self.heading(
                &p,
                "SONGS IN BOTH FORMATS",
                x,
                y,
                Some(card.max.x - pad),
                "which copy plays",
                "",
            );
            let seg = Rect::from_min_size(pos2(x, y), vec2(wd.min(px(420.0)), seg_h));
            let sel = usize::from(st.prefer() == Prefer::Feedpak);
            let (_, clicked) = widgets::segmented(
                ui,
                seg,
                "set-prefer",
                &seg_opts(&["The psarc", "The feedpak"], sel),
            );
            if let Some(i) = clicked {
                self.acts.push(format!("prefer:{}", ["psarc", "feedpak"][i]));
            }
            caption_lines(
                &p,
                "Your scores are shared by both copies; the part picker can still open the other one.",
                x,
                seg.max.y + px(10.0),
                wd,
                13.0,
                1,
            );
        }
        card.max.y
    }

    /// RS KEYS: what was found, the two boxes, Save and Load.
    fn draw_keys(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(22.0);
        let hh = Self::h_heading(&ctx);
        let wd_all = snap(body.width().min(760.0));
        let l13 = line_h(&ctx, 13.0);
        let l12 = line_h(&ctx, 12.0);
        let box_h = px(42.0);
        let card = Rect::from_min_size(
            body.min,
            vec2(
                wd_all,
                pad + hh
                    + line_h(&ctx, 15.0)
                    + px(6.0)
                    + 2.0 * l12
                    + px(18.0)
                    + 3.0 * (l13 + px(6.0) + box_h + px(4.0) + l12 + px(10.0))
                    + 2.0 * l12
                    + px(14.0)
                    + px(46.0)
                    + px(10.0)
                    + l12
                    + pad,
            ),
        );
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let mut y = self.heading(
            &p,
            "RS KEYS",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            "only for .psarc files; feedpaks don't need them",
            "",
        );
        let waiting = st.songs.iter().filter(|s| s.needs_keys).count();
        // what was found, and (a line of its own: paths are long) where
        let name_of = |p: &Path| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let (status, color, detail) = match &st.key_state {
            KeyState::Found { path, misnamed } => (
                if st.keys.sng_mac.is_some() {
                    "Keys found: psarc songs play (the Mac key too)".to_string()
                } else {
                    "Keys found: psarc songs play".to_string()
                },
                th.hit,
                match path {
                    Some(p) if *misnamed => (
                        format!(
                            "Read from {} (keys.json is the usual name; this works too):",
                            name_of(p)
                        ),
                        p.display().to_string(),
                    ),
                    Some(p) => ("Read from:".into(), p.display().to_string()),
                    None => (
                        "From the environment variables".into(),
                        "RSPLAY_PSARC_KEY and RSPLAY_SNG_KEY".into(),
                    ),
                },
            ),
            KeyState::Partial { path } => (
                "Only one of the two keys".to_string(),
                th.miss,
                (
                    "It needs both psarc_key and sng_key:".into(),
                    path.display().to_string(),
                ),
            ),
            KeyState::Invalid { path, why } => (
                "Your keys file doesn't read".to_string(),
                th.miss,
                (
                    why.replace(&path.display().to_string(), &name_of(path)),
                    path.display().to_string(),
                ),
            ),
            KeyState::Missing => (
                if waiting > 0 {
                    format!("No keys yet: {waiting} psarc songs wait for them")
                } else {
                    "No keys (not needed for feedpaks)".to_string()
                },
                th.caption,
                (
                    "Looked for keys.json (or keys.json.txt) in:".into(),
                    looked_in(&st.key_files),
                ),
            ),
        };
        let tr = text::draw(
            &p,
            &status,
            pos2(x, y),
            Align2::LEFT_TOP,
            Style::new(15.0, 800, color),
            Some(wd),
        );
        y = tr.max.y + px(6.0);
        let cs = Style::new(12.0, 500, th.caption);
        let ly = text::draw(&p, &detail.0, pos2(x, y), Align2::LEFT_TOP, cs, Some(wd))
            .max
            .y;
        text::draw(&p, &detail.1, pos2(x, ly), Align2::LEFT_TOP, cs, Some(wd));
        y += 2.0 * l12 + px(18.0);
        // the boxes; a whole keys.json pasted into any fills them all
        let mut pasted = None;
        for (which, label, note) in [
            ("psarc", "PSARC key", ""),
            ("sng", "SNG key", ""),
            (
                "mac",
                "Mac SNG key",
                "optional: only RS's own songs from its Mac edition need it",
            ),
        ] {
            let lr = text::draw(
                &p,
                label,
                pos2(x, y),
                Align2::LEFT_TOP,
                Style::new(13.0, 700, th.text),
                None,
            );
            if !note.is_empty() {
                text::draw(
                    &p,
                    note,
                    pos2(lr.max.x + px(10.0), lr.center().y),
                    Align2::LEFT_CENTER,
                    Style::new(12.0, 500, th.caption),
                    Some(x + wd - lr.max.x - px(10.0)),
                );
            }
            y += l13 + px(6.0);
            let r = Rect::from_min_size(pos2(x, y), vec2(wd, box_h));
            let value = match which {
                "psarc" => &mut self.psarc_in,
                "sng" => &mut self.sng_in,
                _ => &mut self.mac_in,
            };
            let resp = text_box(ui, r, which, value, "64 hex characters");
            if resp.changed()
                && let Some(all) = keyfile::split_pasted(value)
            {
                pasted = Some(all);
            }
            let (msg, ok) = match keyfile::check_key(value) {
                Ok(None) => (String::new(), true),
                Ok(Some(_)) => ("looks right".to_string(), true),
                Err(e) => (e, false),
            };
            y += box_h + px(4.0);
            text::draw(
                &p,
                &msg,
                pos2(x + px(4.0), y),
                Align2::LEFT_TOP,
                Style::new(12.0, 600, if ok { th.hit } else { th.miss }),
                Some(wd),
            );
            y += l12 + px(10.0);
        }
        if let Some((a, b, m)) = pasted {
            self.psarc_in = a;
            self.sng_in = b;
            self.mac_in = m;
        }
        let save_path = st.data.keys();
        let hy = text::draw(
            &p,
            "quickRS doesn't come with the keys. A whole keys.json pasted into any box fills them all.",
            pos2(x, y),
            Align2::LEFT_TOP,
            cs,
            Some(wd),
        )
        .max
        .y;
        text::draw(
            &p,
            &format!("Saved as {}", save_path.display()),
            pos2(x, hy),
            Align2::LEFT_TOP,
            cs,
            Some(wd),
        );
        y += 2.0 * l12 + px(14.0);
        let ready = self.keys_to_save(st).is_ok();
        let sw = widgets::button_width(&ctx, "Save keys", "", 14.0, false);
        let save = Rect::from_min_size(pos2(x, y), vec2(sw, px(46.0)));
        self.button(
            ui,
            save,
            "Save keys",
            "",
            "savekeys",
            ButtonStyle {
                primary: ready,
                enabled: ready,
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        let lw = widgets::button_width(&ctx, "Load keys file…", "", 14.0, false);
        let load = Rect::from_min_size(pos2(save.max.x + px(12.0), y), vec2(lw, px(46.0)));
        self.button(
            ui,
            load,
            "Load keys file…",
            "",
            "loadkeys",
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        if let Some((msg, ok)) = &self.key_note {
            text::draw(
                &p,
                msg,
                pos2(load.max.x + px(16.0), centre_y(load)),
                Align2::LEFT_CENTER,
                Style::new(13.0, 700, if *ok { th.hit } else { th.miss }),
                Some(x + wd - load.max.x - px(16.0)),
            );
        }
        card.max.y
    }

    /// The keys the boxes would save: the two typed in (and a Mac key if typed), or a Mac key alone added to the
    /// keys in use (P51.2: someone whose keys work adds only the third).
    fn keys_to_save(
        &self,
        st: &AppState,
    ) -> Result<
        (
            qrs_formats::keys::Key,
            qrs_formats::keys::Key,
            Option<qrs_formats::keys::Key>,
        ),
        String,
    > {
        let bad = || "Both keys are needed, 64 hex characters each".to_string();
        let mac = keyfile::check_key(&self.mac_in).map_err(|e| format!("Mac SNG key: {e}"))?;
        match (
            keyfile::check_key(&self.psarc_in),
            keyfile::check_key(&self.sng_in),
        ) {
            (Ok(Some(a)), Ok(Some(b))) => Ok((a, b, mac)),
            (Ok(None), Ok(None)) => match (mac, st.keys.psarc, st.keys.sng) {
                (Some(m), Some(a), Some(b)) => Ok((a, b, Some(m))),
                _ => Err(bad()),
            },
            _ => Err(bad()),
        }
    }

    /// Save the keys typed in the boxes as keys.json, and use them.
    pub(super) fn save_keys(&mut self, st: &mut AppState) {
        match self.keys_to_save(st) {
            Ok((a, b, m)) => self.write_keys(st, a, b, m),
            Err(e) => self.key_note = Some((e, false)),
        }
    }

    /// Load a keys file picked in the system's dialog (keys.json, keys.json.txt...) and save it as keys.json.
    pub(super) fn load_keys_file(&mut self, st: &mut AppState) {
        let Some(path) = crate::dialogs::pick_keys_file() else {
            if crate::dialogs::unavailable() {
                self.key_note = Some((
                    "No file picker on this desktop: paste the two keys into the boxes instead".into(),
                    false,
                ));
            }
            return;
        };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match qrs_formats::keys_from_text(&text, &name) {
            Ok(k) => match (k.psarc, k.sng) {
                (Some(a), Some(b)) => self.write_keys(st, a, b, k.sng_mac),
                _ => self.key_note = Some((format!("{name} needs both psarc_key and sng_key"), false)),
            },
            Err(e) => self.key_note = Some((e.to_string(), false)),
        }
    }

    fn write_keys(
        &mut self,
        st: &mut AppState,
        a: qrs_formats::keys::Key,
        b: qrs_formats::keys::Key,
        mac: Option<qrs_formats::keys::Key>,
    ) {
        let path = st.data.keys();
        match keyfile::write(&path, a, b, mac) {
            Ok(()) => {
                self.psarc_in.clear();
                self.sng_in.clear();
                self.mac_in.clear();
                st.reload_keys();
                let ok = matches!(st.key_state, KeyState::Found { .. });
                self.key_note = Some(if ok {
                    ("Saved, and in use".into(), true)
                } else {
                    ("Saved, but another keys file is read first".into(), false)
                });
            }
            Err(e) => self.key_note = Some((format!("Couldn't save {}: {e}", path.display()), false)),
        }
    }
}

/// The folders the keys files were looked for in, in short ("A, B and 2 more").
fn looked_in(files: &[PathBuf]) -> String {
    let mut dirs: Vec<String> = Vec::new();
    for f in files {
        if let Some(d) = f.parent().map(|d| d.display().to_string())
            && !dirs.contains(&d)
        {
            dirs.push(d);
        }
    }
    match dirs.len() {
        0..=2 => dirs.join("; "),
        n => format!("{}; {} and {} more", dirs[0], dirs[1], n - 2),
    }
}
