//! The Sounds tab (PLAN.md P22.3, Room-Library): your sounds (favourite slots first), the starting rigs, and every
//! song's own tone (the gear index: search by song, artist, part or tone). Selecting one previews it (you play
//! through it; your rig is kept), Esc goes back to yours, Enter loads it, Shift 1-4 loads it into a scene.

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use qrs_audio::board::{self, Board};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::draw::{self, KeyLook};
use super::rigs::{self, COLOURS};
use super::save::tag_colour;
use super::{RoomView, Typing};
use crate::ampbutton::{rgb, rgba};
use crate::gearindex::{self, SongTone};
use crate::pedalfaces as faces;
use crate::player::board::Rack;
use crate::state::AppState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    Mine,
    Start,
    Songs,
}

/// A sound in the list.
#[derive(Clone, Debug)]
pub enum Item {
    Song(SongTone),
    /// One of your sounds (its index in the room's list).
    Mine(usize),
    /// A starting rig (its index in `board::presets()`).
    Start(usize),
}

pub struct Sounds {
    pub query: String,
    filter: Filter,
    tag: Option<&'static str>,
    items: Vec<Item>,
    /// (song tones found, your sounds, starting rigs) before the filter.
    counts: (usize, usize, usize),
    built: Option<(String, Filter, Option<&'static str>, u64, u64)>,
    sel: Option<usize>,
    /// What you're hearing instead of your rig: (name, its board, a line about it).
    preview: Option<(String, Board, String)>,
    scroll: f32,
    list: Rect,
    /// Rows' rects this frame (to keep the selection in view).
    rows: Vec<(usize, Rect)>,
    /// Delete was clicked once on this sound (a second click deletes it).
    confirm: Option<String>,
    /// The first visit (P41.3, D171): the starters come first, as big cards over the list and first in it.
    pub starters_first: bool,
}

impl Default for Sounds {
    fn default() -> Self {
        Sounds {
            query: String::new(),
            filter: Filter::All,
            tag: None,
            items: Vec::new(),
            counts: (0, 0, 0),
            built: None,
            sel: None,
            preview: None,
            scroll: 0.0,
            list: Rect::NOTHING,
            rows: Vec::new(),
            confirm: None,
            starters_first: false,
        }
    }
}

const CHARACTER: [&str; 5] = ["Clean", "Crunch", "Lead", "Ambient", "Bass"];

fn chars_of(label: &str) -> Vec<&'static str> {
    let l = label.to_lowercase();
    let mut v: Vec<&'static str> = Vec::new();
    for (w, tag) in [
        ("clean", "Clean"),
        ("crunch", "Crunch"),
        ("drive", "Crunch"),
        ("dist", "Lead"),
        ("lead", "Lead"),
        ("solo", "Lead"),
        ("ambient", "Ambient"),
        ("verb", "Ambient"),
        ("delay", "Ambient"),
        ("bass", "Bass"),
    ] {
        if l.contains(w) && !v.contains(&tag) {
            v.push(tag);
        }
    }
    v
}

impl Sounds {
    /// The search or a filter changed: build the list again.
    pub fn moved(&mut self) {
        self.built = None;
        self.scroll = 0.0;
    }

    /// The list's first sound (tests).
    #[cfg(test)]
    pub fn first_item(&self) -> Option<&Item> {
        self.items.first()
    }

    pub fn previewing(&self) -> bool {
        self.preview.is_some()
    }

    pub fn preview_name(&self) -> String {
        self.preview.as_ref().map(|p| p.0.clone()).unwrap_or_default()
    }

    /// Back to your rig.
    pub fn stop(&mut self, rack: &mut Rack, st: &AppState) {
        self.preview = None;
        self.sel = None;
        rack.board.audition = None;
        rack.board_send(st);
    }

    pub fn poll(&mut self, _st: &AppState) {}

    pub fn wheel(&mut self, pos: Pos2, notches: f32) {
        if self.list.contains(pos) {
            self.scroll = (self.scroll - notches * px(52.0)).max(0.0);
        }
    }

    pub(super) fn build(&mut self, room_saved: &[rigs::Rig], st: &AppState) {
        let key = (
            self.query.clone(),
            self.filter,
            self.tag,
            gearindex::version(),
            st.userdata.version,
        );
        if self.built.as_ref() == Some(&key) {
            return;
        }
        self.built = Some(key);
        let q = self.query.trim().to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let tag_ok = |labels: &[&str]| {
            self.tag
                .is_none_or(|t| labels.iter().any(|l| l.eq_ignore_ascii_case(t)))
        };
        // song tones: the search's, else the songs you played last
        let songs: Vec<SongTone> = if q.is_empty() {
            let mut recent: Vec<(&String, &f64)> = st.userdata.recent.iter().collect();
            recent.sort_by(|a, b| b.1.total_cmp(a.1));
            let sids: Vec<String> = recent.into_iter().take(40).map(|(s, _)| s.clone()).collect();
            let mut v = gearindex::tones_of(&sids, 24);
            if v.is_empty() {
                v = gearindex::find_tones("", 24).0;
            }
            v
        } else {
            gearindex::find_tones(&q, 200).0
        };
        let songs: Vec<SongTone> = songs
            .into_iter()
            .filter(|s| tag_ok(&chars_of(&s.label)))
            .collect();
        let mine: Vec<usize> = (0..room_saved.len())
            .filter(|&i| {
                let r = &room_saved[i];
                rigs::matches(r, &words) && tag_ok(&r.tags.iter().map(String::as_str).collect::<Vec<_>>())
            })
            .collect();
        let presets = board::presets();
        let start: Vec<usize> = (0..presets.len())
            .filter(|&i| {
                let (n, b) = &presets[i];
                let hay = format!("{n} {}", rigs::chain_text(b)).to_lowercase();
                words.iter().all(|w| hay.contains(w)) && tag_ok(&chars_of(n))
            })
            .collect();
        self.counts = (songs.len(), mine.len(), start.len());
        self.items.clear();
        // (the first visit, or no sounds of your own: the starting rigs first, P41.3)
        let start_first = self.starters_first || room_saved.is_empty();
        if start_first && matches!(self.filter, Filter::All | Filter::Start) {
            self.items.extend(start.iter().copied().map(Item::Start));
        }
        if matches!(self.filter, Filter::All | Filter::Songs) {
            self.items.extend(songs.into_iter().map(Item::Song));
        }
        if matches!(self.filter, Filter::All | Filter::Mine) {
            self.items.extend(mine.into_iter().map(Item::Mine));
        }
        if !start_first && matches!(self.filter, Filter::All | Filter::Start) {
            self.items.extend(start.into_iter().map(Item::Start));
        }
        if self.sel.is_some_and(|s| s >= self.items.len()) {
            self.sel = None;
        }
    }
}

/// An item's name, board and a line about it.
fn about(room: &RoomView, it: &Item) -> (String, Board, String) {
    match it {
        Item::Song(s) => (
            format!("{} \u{b7} {}", s.title, s.part),
            s.board.clone(),
            format!("{} \u{b7} the song's own tone from RS ({})", s.artist, s.label),
        ),
        Item::Mine(i) => {
            let r = &room.saved[*i];
            (
                r.name.clone(),
                r.board.clone(),
                if r.based_on.is_empty() {
                    "one of your sounds".to_string()
                } else {
                    format!("one of your sounds \u{b7} based on {}", r.based_on)
                },
            )
        }
        Item::Start(i) => {
            let (n, b) = board::presets().swap_remove(*i);
            (
                n.to_string(),
                b,
                format!(
                    "a starting rig: {}",
                    starter_line(n).unwrap_or("a good place to begin")
                ),
            )
        }
    }
}

/// A starter's one line, for a newcomer (P41.3).
pub fn starter_line(name: &str) -> Option<&'static str> {
    Some(match name {
        "Clean" => "bright and clean, for chords and picking",
        "Crunch" => "a classic rock crunch, for riffs",
        "Lead" => "smooth and singing, for solos",
        "Bass" => "warm and round, under the band",
        "Metal" => "tight and heavy, for chugging riffs",
        "Blues" => "warm, a little hairy, for bends",
        "Ambient" => "clean with long echoes, for swells",
        _ => return None,
    })
}

/// The starters as cards (Clean, Crunch, Lead, Bass; Bass first on a bass): the name, its line, Hear. A click on one
/// previews it as a row of the list does (Enter loads it).
fn starter_cards(room: &mut RoomView, ui: &Ui, st: &AppState, x0: f32, x1: f32, y: f32) {
    let p = ui.painter().clone();
    let th = t();
    draw::caps(
        &p,
        "START HERE",
        pos2(x0, y),
        10.5,
        th.caption,
        Align2::LEFT_CENTER,
    );
    draw::label(
        &p,
        "four sounds to begin with \u{b7} Hear one, then Load (Enter)",
        pos2(x0 + px(96.0), y),
        11.0,
        600,
        th.caption,
        Align2::LEFT_CENTER,
    );
    let mut names = ["Clean", "Crunch", "Lead", "Bass"];
    if st.instrument().is_bass() {
        names.rotate_right(1);
    }
    let presets = board::presets();
    let y = y + px(14.0);
    let sw = (x1 - x0 - px(3.0) * px(10.0)) / 4.0;
    for (n, name) in names.iter().enumerate() {
        let Some(k) = presets.iter().position(|(x, _)| x == name) else {
            continue;
        };
        let cr = Rect::from_min_size(pos2(x0 + n as f32 * (sw + px(10.0)), y), vec2(sw, px(58.0)));
        let at = room
            .sounds
            .items
            .iter()
            .position(|it| matches!(it, Item::Start(j) if *j == k));
        let hearing = room.sounds.preview.as_ref().is_some_and(|pv| pv.0 == *name);
        let resp = interact(ui, cr, ("room-starter", n), Sense::click());
        let col = tag_colour(name);
        p.rect_filled(cr, px(14.0), col.gamma_multiply(if hearing { 0.2 } else { 0.1 }));
        p.rect_stroke(
            cr,
            px(14.0),
            Stroke::new(
                if hearing { 2.0 } else { 1.0 },
                if hearing {
                    th.accent
                } else {
                    col.gamma_multiply(0.6)
                },
            ),
            egui::StrokeKind::Inside,
        );
        draw::label_w(
            &p,
            name,
            pos2(cr.min.x + px(14.0), cr.min.y + px(17.0)),
            15.0,
            800,
            th.text,
            Align2::LEFT_CENTER,
            cr.width() - px(70.0),
        );
        draw::label_w(
            &p,
            starter_line(name).unwrap_or(""),
            pos2(cr.min.x + px(14.0), cr.min.y + px(40.0)),
            11.0,
            600,
            th.text_2,
            Align2::LEFT_CENTER,
            cr.width() - px(22.0),
        );
        draw::label(
            &p,
            if hearing { "Hearing" } else { "Hear \u{203a}" },
            pos2(cr.max.x - px(12.0), cr.min.y + px(17.0)),
            11.5,
            800,
            if hearing || resp.hovered() {
                th.accent_ink
            } else {
                th.caption
            },
            Align2::RIGHT_CENTER,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            match at {
                Some(i) => select(room, st, i),
                None => {
                    // (a search or a filter has it off the list: heard all the same)
                    let b = presets[k].1.clone();
                    room.rack.board.audition = Some(b.renewed());
                    room.rack.board_send(st);
                    room.sounds.sel = None;
                    room.sounds.preview = Some((
                        name.to_string(),
                        b,
                        format!("a starting rig: {}", starter_line(name).unwrap_or("")),
                    ));
                }
            }
        }
    }
}

pub(super) fn select(room: &mut RoomView, st: &AppState, i: usize) {
    let Some(it) = room.sounds.items.get(i).cloned() else {
        return;
    };
    room.sounds.sel = Some(i);
    let (name, b, line) = about(room, &it);
    room.rack.board.audition = Some(b.renewed());
    room.rack.board_send(st);
    room.sounds.preview = Some((name, b, line));
    room.ab = false;
}

/// Load what's previewed (or selected) as the rig.
fn load_sel(room: &mut RoomView, st: &mut AppState) {
    let Some(it) = room.sounds.sel.and_then(|i| room.sounds.items.get(i).cloned()) else {
        return;
    };
    room.sounds.preview = None;
    room.rack.board.audition = None;
    match it {
        Item::Mine(i) => {
            let r = room.saved[i].clone();
            room.load_saved(st, &r);
        }
        Item::Start(i) => {
            let (n, b) = board::presets().swap_remove(i);
            room.load(st, b, Default::default(), None, n.to_string());
        }
        Item::Song(s) => {
            let based = format!("{} \u{b7} {} {}", s.title, s.part, s.label);
            room.load(st, s.board, Default::default(), None, based);
        }
    }
    room.sounds.sel = None;
}

/// The previewed sound into scene `n` of your rig (it plays at once if that's the scene you're on).
pub fn load_into_scene(room: &mut RoomView, st: &mut AppState, n: usize) {
    let Some((name, b, _)) = room.sounds.preview.clone() else {
        return;
    };
    room.rack.board_commit(st);
    room.keep_scene(st);
    room.state.scenes[n] = Some(b.clone());
    room.sounds.stop(&mut room.rack, st);
    if n == room.state.scene {
        let cur = room.rack.board_now(st);
        let out = rigs::scene_board(&cur, &b);
        room.rack.board_edit(st, |x| *x = out);
        room.rack.board_commit(st);
        room.rack.said.clear();
    }
    room.state.save(&mut st.config);
    st.save_config();
    room.say(
        format!("{name} in {}", rigs::scene_label(&room.state.names, n)),
        "  \u{b7}  save to keep it with the sound",
    );
}

pub fn key(room: &mut RoomView, st: &mut AppState, key: Key, _m: Modifiers) {
    let n = room.sounds.items.len();
    if n == 0 {
        return;
    }
    match key {
        Key::ArrowDown => {
            let i = room.sounds.sel.map(|i| (i + 1).min(n - 1)).unwrap_or(0);
            select(room, st, i);
            keep_in_view(room, i);
        }
        Key::ArrowUp => {
            let i = room.sounds.sel.map(|i| i.saturating_sub(1)).unwrap_or(0);
            select(room, st, i);
            keep_in_view(room, i);
        }
        Key::Enter => load_sel(room, st),
        _ => {}
    }
}

fn keep_in_view(room: &mut RoomView, i: usize) {
    if let Some((_, r)) = room.sounds.rows.iter().find(|(j, _)| *j == i) {
        let l = room.sounds.list;
        if r.max.y > l.max.y {
            room.sounds.scroll += r.max.y - l.max.y + px(8.0);
        } else if r.min.y < l.min.y {
            room.sounds.scroll = (room.sounds.scroll - (l.min.y - r.min.y) - px(8.0)).max(0.0);
        }
    }
}

pub fn page(room: &mut RoomView, ui: &Ui, st: &mut AppState, body: Rect) {
    let saved = room.saved.clone();
    room.sounds.build(&saved, st);
    let split = body.min.x + (body.width() * 0.628).round();
    let left = Rect::from_min_max(body.min, pos2(split - px(6.0), body.max.y));
    let right = Rect::from_min_max(pos2(split + px(6.0), body.min.y), body.max);
    list_panel(room, ui, st, left);
    preview_panel(room, ui, st, right);
}

fn list_panel(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    draw::card(&p, r, px(20.0));
    let x0 = r.min.x + px(16.0);
    let x1 = r.max.x - px(16.0);
    // the search
    let sr = Rect::from_min_size(pos2(x0, r.min.y + px(14.0)), vec2(x1 - x0, px(50.0)));
    let typing = room.typing == Some(Typing::Sounds);
    let resp = interact(ui, sr, "room-sounds-find", Sense::click());
    paint::inset(&p, sr, Some(px(14.0)), 2.0, Fill::Base);
    if typing {
        p.rect_stroke(
            sr,
            px(14.0),
            Stroke::new(2.0, th.accent),
            egui::StrokeKind::Inside,
        );
    }
    if resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::Text);
    }
    if resp.clicked() {
        room.typing = Some(Typing::Sounds);
    }
    draw::icon_search(
        &p,
        pos2(sr.min.x + px(26.0), sr.center().y),
        px(18.0),
        if typing { th.accent } else { th.caption },
    );
    let qr = draw::label(
        &p,
        &room.sounds.query,
        pos2(sr.min.x + px(48.0), sr.center().y),
        18.0,
        800,
        th.text,
        Align2::LEFT_CENTER,
    );
    if typing {
        draw::caret(
            &p,
            if room.sounds.query.is_empty() {
                sr.min.x + px(48.0)
            } else {
                qr.max.x
            },
            sr.center().y,
            px(22.0),
            th.accent,
        );
    }
    let ph_x = if room.sounds.query.is_empty() {
        sr.min.x + px(52.0)
    } else {
        qr.max.x + px(28.0)
    };
    draw::label(
        &p,
        "song, artist, tag or gear",
        pos2(ph_x, sr.center().y + px(1.0)),
        13.0,
        600,
        th.caption,
        Align2::LEFT_CENTER,
    );
    let total = room.sounds.counts.0 + room.sounds.counts.1 + room.sounds.counts.2;
    let reading = gearindex::reading()
        .map(|(a, b)| format!("reading your songs' tones {a} / {b}"))
        .unwrap_or_else(|| format!("searching {}", crate::home::songs(st.groups.len())));
    draw::label(
        &p,
        &format!("{total} sounds \u{b7} {reading}"),
        pos2(sr.max.x - px(18.0), sr.center().y),
        12.0,
        600,
        th.text_2,
        Align2::RIGHT_CENTER,
    );
    // filters
    let fy = sr.max.y + px(12.0);
    let mut x = x0;
    let (cs, cm, cst) = room.sounds.counts;
    for (f, label) in [
        (Filter::All, format!("All  {total}")),
        (Filter::Mine, format!("My sounds  {cm}")),
        (Filter::Start, format!("Starting rigs  {cst}")),
        (Filter::Songs, format!("From your songs  {cs}")),
    ] {
        let w = draw::chip_w(&ctx, &label, false, 12.0);
        let cr = Rect::from_min_size(pos2(x, fy), vec2(w, px(28.0)));
        x = cr.max.x + px(6.0);
        if draw::chip(
            ui,
            cr,
            ("room-sounds-filter", label.as_str()),
            &label,
            None,
            room.sounds.filter == f,
            12.0,
        )
        .clicked()
        {
            room.sounds.filter = f;
            room.sounds.moved();
        }
    }
    let mut tx = x1;
    for tag in CHARACTER.iter().rev() {
        let w = draw::chip_w(&ctx, tag, true, 12.0);
        let cr = Rect::from_min_size(pos2(tx - w, fy), vec2(w, px(28.0)));
        tx = cr.min.x - px(6.0);
        if tx < x {
            break;
        }
        let on = room.sounds.tag == Some(*tag);
        if draw::chip(
            ui,
            cr,
            ("room-sounds-tag", *tag),
            tag,
            Some(tag_colour(tag)),
            on,
            12.0,
        )
        .clicked()
        {
            room.sounds.tag = if on { None } else { Some(*tag) };
            room.sounds.moved();
        }
    }
    // the favourite slots; with none set yet, the four starters as big cards (P41.3, D171)
    let mut y = fy + px(44.0);
    if !room.saved.iter().any(|r| r.slot > 0) {
        starter_cards(room, ui, st, x0, x1, y);
        y += px(14.0);
    } else {
        draw::caps(
            &p,
            "FAVOURITE SLOTS",
            pos2(x0, y),
            10.5,
            th.caption,
            Align2::LEFT_CENTER,
        );
        draw::label(
            &p,
            "your four go-to sounds \u{b7} set one when you save",
            pos2(x0 + px(124.0), y),
            11.0,
            600,
            th.caption,
            Align2::LEFT_CENTER,
        );
        y += px(14.0);
        let sw = (x1 - x0 - px(3.0) * px(10.0)) / 4.0;
        for slot in 1..=4u8 {
            let cr = Rect::from_min_size(
                pos2(x0 + (slot - 1) as f32 * (sw + px(10.0)), y),
                vec2(sw, px(58.0)),
            );
            let rig = room.saved.iter().position(|r| r.slot == slot);
            let resp = interact(ui, cr, ("room-slot", slot), Sense::click());
            match rig {
                Some(i) => {
                    let rg = &room.saved[i];
                    let col = rgb(COLOURS[rg.color.min(COLOURS.len() - 1)]);
                    let current = room
                        .state
                        .rig
                        .as_ref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&rg.name));
                    p.rect_filled(cr, px(14.0), col.gamma_multiply(0.12));
                    p.rect_stroke(
                        cr,
                        px(14.0),
                        Stroke::new(
                            if current { 2.0 } else { 1.0 },
                            if current {
                                th.accent
                            } else {
                                col.gamma_multiply(0.6)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    p.rect_filled(
                        Rect::from_min_size(
                            pos2(cr.min.x + px(3.0), cr.min.y + px(12.0)),
                            vec2(px(3.0), px(34.0)),
                        ),
                        px(1.5),
                        col,
                    );
                    draw::label_w(
                        &p,
                        &rg.name,
                        pos2(cr.min.x + px(16.0), cr.min.y + px(17.0)),
                        13.0,
                        800,
                        th.text,
                        Align2::LEFT_CENTER,
                        cr.width() - px(40.0),
                    );
                    draw::label(
                        &p,
                        &slot.to_string(),
                        pos2(cr.max.x - px(12.0), cr.min.y + px(14.0)),
                        11.0,
                        800,
                        col,
                        Align2::RIGHT_CENTER,
                    );
                    draw::chain_icons(
                        &p,
                        &rg.board,
                        cr.min.x + px(16.0),
                        cr.min.y + px(40.0),
                        px(18.0),
                        8,
                    );
                    if current {
                        text::caps(
                            &p,
                            "CURRENT",
                            pos2(cr.max.x - px(10.0), cr.max.y - px(12.0)),
                            8.5,
                            th.accent_ink,
                            Align2::RIGHT_CENTER,
                            None,
                        );
                    }
                    if resp.hovered() {
                        ctx.set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        if let Some(j) = room
                            .sounds
                            .items
                            .iter()
                            .position(|it| matches!(it, Item::Mine(k) if *k == i))
                        {
                            select(room, st, j);
                        } else {
                            let r = room.saved[i].clone();
                            room.load_saved(st, &r);
                        }
                    }
                }
                None => {
                    paint::inset(&p, cr, Some(px(14.0)), 1.5, Fill::Base);
                    draw::label(
                        &p,
                        &format!("{slot}  \u{b7}  empty"),
                        cr.center(),
                        12.0,
                        700,
                        th.caption,
                        Align2::CENTER_CENTER,
                    );
                }
            }
        }
    }
    // the list
    let list = Rect::from_min_max(
        pos2(r.min.x + px(6.0), y + px(70.0)),
        pos2(r.max.x - px(6.0), r.max.y - px(34.0)),
    );
    room.sounds.list = list;
    let clip = list;
    let lp = p.with_clip_rect(clip);
    let mut yy = list.min.y - room.sounds.scroll;
    room.sounds.rows.clear();
    let items = room.sounds.items.clone();
    let mut last_sec: Option<u8> = None;
    let sec_of = |it: &Item| match it {
        Item::Song(_) => 0u8,
        Item::Mine(_) => 1,
        Item::Start(_) => 2,
    };
    if items.is_empty() {
        draw::label_w(
            &p,
            if gearindex::reading().is_some() {
                "Nothing yet: your songs' tones are still being read"
            } else {
                "No sound matches"
            },
            list.center(),
            13.0,
            600,
            th.caption,
            Align2::CENTER_CENTER,
            list.width() - px(40.0),
        );
    }
    let mut add_load: Option<usize> = None;
    for (i, it) in items.iter().enumerate() {
        let sec = sec_of(it);
        if last_sec != Some(sec) {
            last_sec = Some(sec);
            let (head, note, n) = match sec {
                0 => (
                    "FROM YOUR SONGS",
                    "each song's own tone from RS, on our amps",
                    room.sounds.counts.0,
                ),
                1 => (
                    "MY SOUNDS",
                    "saved here, with your playlists",
                    room.sounds.counts.1,
                ),
                _ => (
                    "STARTING RIGS",
                    "built in \u{b7} a good place to begin",
                    room.sounds.counts.2,
                ),
            };
            yy += px(8.0);
            let hr = draw::caps(
                &lp,
                head,
                pos2(x0, yy + px(10.0)),
                10.5,
                th.caption,
                Align2::LEFT_CENTER,
            );
            draw::label(
                &lp,
                &n.to_string(),
                pos2(hr.max.x + px(10.0), yy + px(10.0)),
                10.5,
                800,
                th.caption,
                Align2::LEFT_CENTER,
            );
            draw::label(
                &lp,
                note,
                pos2(x1, yy + px(10.0)),
                11.0,
                600,
                th.caption,
                Align2::RIGHT_CENTER,
            );
            yy += px(26.0);
        }
        let row = Rect::from_min_size(pos2(r.min.x + px(14.0), yy), vec2(r.width() - px(28.0), px(48.0)));
        yy += px(52.0);
        room.sounds.rows.push((i, row));
        if row.max.y < clip.min.y || row.min.y > clip.max.y {
            continue;
        }
        let vis = row.intersect(clip);
        let resp = interact(ui, vis, ("room-sound", i), Sense::click());
        let sel = room.sounds.sel == Some(i);
        if sel {
            lp.rect_filled(row, px(14.0), th.accent.gamma_multiply(0.08));
            lp.rect_stroke(
                row,
                px(14.0),
                Stroke::new(1.5, th.accent),
                egui::StrokeKind::Inside,
            );
        } else if resp.hovered() {
            lp.rect_filled(row, px(14.0), th.base_hi.gamma_multiply(0.5));
        }
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        // the icon well
        let ir = Rect::from_center_size(
            pos2(row.min.x + px(28.0), row.center().y),
            vec2(px(34.0), px(34.0)),
        );
        paint::inset(&lp, ir, Some(px(9.0)), 1.5, Fill::Base);
        let (title, sub, tags, b): (String, String, Vec<&'static str>, Board) = match it {
            Item::Song(s) => {
                text::draw(
                    &lp,
                    "\u{266b}",
                    ir.center(),
                    Align2::CENTER_CENTER,
                    Style::new(15.0, 700, th.text_2),
                    None,
                );
                (
                    format!("{} - {} \u{b7} {}", s.title, s.artist, s.part),
                    format!("{} \u{b7} {}", s.label, super::plain::chain_words(&s.board)),
                    chars_of(&s.label),
                    s.board.clone(),
                )
            }
            Item::Mine(k) => {
                let rg = &room.saved[*k];
                let col = rgb(COLOURS[rg.color.min(COLOURS.len() - 1)]);
                lp.rect_stroke(ir, px(9.0), Stroke::new(1.5, col), egui::StrokeKind::Inside);
                text::draw(
                    &lp,
                    "\u{2605}",
                    ir.center(),
                    Align2::CENTER_CENTER,
                    Style::new(14.0, 700, col),
                    None,
                );
                let mut sub = if rg.based_on.is_empty() {
                    "your sound".to_string()
                } else {
                    format!("based on {}", rg.based_on)
                };
                if !rg.tags.is_empty() {
                    sub.push_str(&format!(" \u{b7} {}", rg.tags.join(", ").to_lowercase()));
                }
                let tags: Vec<&'static str> = CHARACTER
                    .iter()
                    .copied()
                    .filter(|c| rg.tags.iter().any(|t| t.eq_ignore_ascii_case(c)))
                    .collect();
                (rg.name.clone(), sub, tags, rg.board.clone())
            }
            Item::Start(k) => {
                let (n, b) = board::presets().swap_remove(*k);
                lp.rect_filled(
                    Rect::from_center_size(ir.center(), vec2(px(14.0), px(10.0))),
                    px(2.0),
                    th.text_2,
                );
                (
                    n.to_string(),
                    starter_line(n).map_or_else(|| super::plain::chain_words(&b), str::to_string),
                    chars_of(n),
                    b,
                )
            }
        };
        let tx = ir.max.x + px(14.0);
        let mid = row.min.x + row.width() * 0.47;
        draw::label_w(
            &lp,
            &title,
            pos2(tx, row.center().y - px(9.0)),
            14.0,
            800,
            th.text,
            Align2::LEFT_CENTER,
            mid - tx - px(10.0),
        );
        draw::label_w(
            &lp,
            &sub,
            pos2(tx, row.center().y + px(10.0)),
            11.5,
            600,
            th.caption,
            Align2::LEFT_CENTER,
            mid - tx - px(10.0),
        );
        let mut cx = mid;
        for tg in tags.iter().take(2) {
            let col = tag_colour(tg);
            let w = text::width(&ctx, &tg.to_uppercase(), Style::caps(9.5, col)) + px(14.0);
            let cr = Rect::from_min_size(pos2(cx, row.center().y - px(10.0)), vec2(w, px(20.0)));
            lp.rect_filled(cr, px(6.0), col.gamma_multiply(0.12));
            text::caps(
                &lp,
                &tg.to_uppercase(),
                cr.center(),
                9.5,
                col,
                Align2::CENTER_CENTER,
                None,
            );
            cx = cr.max.x + px(6.0);
        }
        let mx = mid + px(120.0);
        draw::chain_icons(&lp, &b, mx, row.center().y, px(24.0), 8);
        if sel {
            let lr = Rect::from_min_size(
                pos2(row.max.x - px(78.0), row.center().y - px(16.0)),
                vec2(px(70.0), px(32.0)),
            );
            if draw::key(ui, lr, ("room-sound-load", i), KeyLook::Ivory, "Load", "", None).clicked() {
                add_load = Some(i);
            }
        }
        if resp.clicked() && !sel {
            select(room, st, i);
        } else if resp.double_clicked() {
            add_load = Some(i);
        }
    }
    // (the list's end, for the wheel)
    let content = yy + room.sounds.scroll - list.min.y;
    room.sounds.scroll = room.sounds.scroll.min((content - list.height()).max(0.0));
    if let Some(i) = add_load {
        room.sounds.sel = Some(i);
        load_sel(room, st);
    }
    draw::label(
        &p,
        "\u{2191}\u{2193} move, you hear it \u{b7} Enter loads \u{b7} Shift 1-4 loads into a scene \u{b7} Esc back to yours",
        pos2(x0, r.max.y - px(17.0)),
        11.0,
        600,
        th.caption,
        Align2::LEFT_CENTER,
    );
}

fn preview_panel(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    draw::card(&p, r, px(20.0));
    let x0 = r.min.x + px(16.0);
    let x1 = r.max.x - px(16.0);
    let Some((name, b, line)) = room.sounds.preview.clone() else {
        draw::caps(
            &p,
            "PREVIEW",
            pos2(x0, r.min.y + px(24.0)),
            10.5,
            th.caption,
            Align2::LEFT_CENTER,
        );
        draw::label_w(
            &p,
            "Select a sound to hear it: you play through it at once, and your rig is kept until you load one.",
            pos2(r.center().x, r.center().y),
            14.0,
            600,
            th.caption,
            Align2::CENTER_CENTER,
            r.width() - px(60.0),
        );
        return;
    };
    let green = Color32::from_rgb(0x3e, 0xc4, 0x6d);
    // the level bars beside PREVIEWING
    for i in 0..4 {
        let h = px(4.0 + 3.0 * i as f32);
        p.rect_filled(
            Rect::from_min_size(
                pos2(x0 + i as f32 * px(4.0), r.min.y + px(28.0) - h),
                vec2(px(2.5), h),
            ),
            1.0,
            green,
        );
    }
    draw::caps(
        &p,
        "PREVIEWING  \u{b7}  YOU'RE PLAYING THROUGH IT",
        pos2(x0 + px(22.0), r.min.y + px(22.0)),
        10.5,
        green,
        Align2::LEFT_CENTER,
    );
    // one of your sounds can be forgotten (click twice)
    let mine = room
        .sounds
        .sel
        .and_then(|i| room.sounds.items.get(i))
        .and_then(|it| match it {
            Item::Mine(k) => room.saved.get(*k).map(|r| r.name.clone()),
            _ => None,
        });
    let mut name_w = x1 - x0;
    if let Some(n) = mine {
        let asking = room.sounds.confirm.as_deref() == Some(n.as_str());
        let label = if asking { "Delete it?" } else { "Delete\u{2026}" };
        let w = draw::key_w(&ui.ctx().clone(), label, "", false);
        let dr = Rect::from_min_size(pos2(x1 - w, r.min.y + px(38.0)), vec2(w, px(30.0)));
        name_w -= w + px(10.0);
        if draw::key(
            ui,
            dr,
            "room-sound-delete",
            if asking { KeyLook::Lit } else { KeyLook::Dark },
            label,
            "",
            None,
        )
        .clicked()
        {
            if asking {
                rigs::delete(&mut st.userdata, &n);
                if room
                    .state
                    .rig
                    .as_ref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(&n))
                {
                    room.state.based_on = n.clone();
                    room.state.rig = None;
                }
                room.sounds.confirm = None;
                room.sounds.stop(&mut room.rack, st);
                room.sounds.moved();
                room.say(format!("Deleted \u{201c}{n}\u{201d}"), "");
                return;
            }
            room.sounds.confirm = Some(n);
        }
    }
    draw::label_w(
        &p,
        &name,
        pos2(x0, r.min.y + px(52.0)),
        24.0,
        800,
        th.text,
        Align2::LEFT_CENTER,
        name_w,
    );
    draw::label_w(
        &p,
        &line,
        pos2(x0, r.min.y + px(76.0)),
        12.0,
        600,
        th.caption,
        Align2::LEFT_CENTER,
        x1 - x0,
    );
    // the rig, on a small stage
    let pic = Rect::from_min_max(
        pos2(x0, r.min.y + px(94.0)),
        pos2(x1, r.min.y + px(94.0) + px(264.0)),
    );
    crate::ampbutton::rounded_vgrad(
        &p,
        pic,
        px(16.0),
        rgb([0x1b, 0x1c, 0x20]),
        rgb([0x0e, 0x0f, 0x11]),
    );
    let lv = text::caps(
        &p,
        "LIVE",
        pos2(pic.min.x + px(18.0), pic.min.y + px(22.0)),
        10.0,
        green,
        Align2::LEFT_CENTER,
        None,
    );
    // (P41.3: the rig in plain words over its picture)
    text::draw(
        &p,
        &super::plain::chain_words(&b),
        pos2(lv.max.x + px(12.0), lv.center().y),
        Align2::LEFT_CENTER,
        Style::new(11.5, 700, Color32::from_rgb(0xd8, 0xd6, 0xd0)),
        Some(pic.max.x - lv.max.x - px(30.0)),
    );
    let sizes: Vec<egui::Vec2> = b.blocks.iter().map(faces::board_size).collect();
    let tw: f32 = sizes.iter().map(|s| s.x).sum::<f32>() + px(14.0) * sizes.len() as f32;
    let k = ((pic.width() - px(40.0)) / tw.max(1.0))
        .min((pic.height() - px(70.0)) / 200.0)
        .min(1.0);
    let mut fx = pic.center().x - tw * k / 2.0;
    let base = pic.max.y - px(26.0);
    for (blk, s) in b.blocks.iter().zip(&sizes) {
        let sz = *s * k;
        let fr = Rect::from_min_size(pos2(fx, base - sz.y), sz);
        p.rect_filled(fr.translate(vec2(0.0, px(4.0))), px(6.0), rgba(0, 0, 0, 0.4));
        faces::miniature(&p, fr, blk, 0.0);
        if !blk.on {
            paint::wash(&p, fr, Color32::BLACK, 0.45);
        }
        fx = fr.max.x + px(14.0) * k;
    }
    // what each unit is set to
    let mut y = pic.max.y + px(22.0);
    for blk in b.blocks.iter().take(6) {
        let def = blk.def();
        p.rect_filled(
            Rect::from_center_size(pos2(x0 + px(4.0), y), vec2(px(8.0), px(8.0))),
            px(2.0),
            faces::kind_colour(def.kind),
        );
        // (a capture by its own name, and the gear it stands in for: B20)
        let name = match crate::player::board::Rack::file_name(blk) {
            Some(n) if def.file => super::rig::short_capture_name(&n),
            _ => def.name.to_string(),
        };
        let skin = def
            .file
            .then(|| crate::player::board::captures::skin_of(blk.asset))
            .flatten();
        // (P41.3: what it is in plain words, then its name; the knobs are the rig's once it's loaded)
        let what = super::plain::unit_words(blk);
        let what = {
            let mut c = what.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        };
        let wr = draw::label_w(
            &p,
            &what,
            pos2(x0 + px(18.0), y),
            13.0,
            800,
            if blk.on { th.text } else { th.caption },
            Align2::LEFT_CENTER,
            (x1 - x0) * 0.5,
        );
        let like = skin
            .as_deref()
            .and_then(qrs_audio::board::catalog::find)
            .map(|m| format!(" \u{b7} sounds like our {}", qrs_audio::board::MODELS[m].name))
            .unwrap_or_default();
        draw::label_w(
            &p,
            &if blk.on {
                format!("{name}{like}")
            } else {
                format!("{name} \u{b7} off")
            },
            pos2(wr.max.x + px(12.0), y),
            12.0,
            600,
            th.text_2,
            Align2::LEFT_CENTER,
            (x1 - wr.max.x - px(12.0)).max(px(20.0)),
        );
        p.line_segment(
            [pos2(x0, y + px(16.0)), pos2(x1, y + px(16.0))],
            Stroke::new(1.0, th.divider),
        );
        y += px(32.0);
    }
    if b.blocks.len() > 6 {
        draw::label(
            &p,
            &format!("+{} more", b.blocks.len() - 6),
            pos2(x0, y),
            11.0,
            700,
            th.caption,
            Align2::LEFT_CENTER,
        );
    }
    // your rig is kept
    let kr = Rect::from_min_max(pos2(x0, r.max.y - px(150.0)), pos2(x1, r.max.y - px(86.0)));
    paint::inset(&p, kr, Some(px(14.0)), 1.5, Fill::Base);
    let edited = if room.state.rig.is_some() {
        ""
    } else {
        " (not saved)"
    };
    text::draw_runs(
        &p,
        &[
            ("Your rig is kept: ", Style::new(12.5, 700, th.text_2)),
            (
                &format!("{}{edited}", room.title()),
                Style::new(12.5, 800, th.text),
            ),
        ],
        pos2(kr.min.x + px(18.0), kr.min.y + px(22.0)),
        Align2::LEFT_CENTER,
    );
    draw::label(
        &p,
        "Esc, or moving off the list, puts it straight back",
        pos2(kr.min.x + px(18.0), kr.min.y + px(43.0)),
        11.0,
        600,
        th.caption,
        Align2::LEFT_CENTER,
    );
    // the keys
    let ky = r.max.y - px(16.0) - px(48.0);
    let ctx = ui.ctx().clone();
    let lw = draw::key_w(&ctx, "Load", "Enter", false) + px(24.0);
    let lr = Rect::from_min_size(pos2(x0, ky), vec2(lw, px(48.0)));
    if draw::key(ui, lr, "room-preview-load", KeyLook::Ivory, "Load", "Enter", None).clicked() {
        load_sel(room, st);
        return;
    }
    let n = room.state.scene;
    let sl = format!("Load into scene {}", n + 1);
    let sw = draw::key_w(&ctx, &sl, "", false) + px(24.0);
    let srr = Rect::from_min_size(pos2(lr.max.x + px(10.0), ky), vec2(sw, px(48.0)));
    if draw::key(ui, srr, "room-preview-scene", KeyLook::Dark, &sl, "", None).clicked() {
        load_into_scene(room, st, n);
        return;
    }
    let bw = (x1 - srr.max.x - px(10.0)).max(px(60.0));
    let br = Rect::from_min_size(pos2(x1 - bw, ky), vec2(bw, px(48.0)));
    if draw::key(ui, br, "room-preview-back", KeyLook::Dark, "Yours", "Esc", None).clicked() {
        room.sounds.stop(&mut room.rack, st);
    }
}
