//! Library screen (port of rsplay/libview.py): a sidebar of views (all songs, favourites, recently played, tunings,
//! playlists) under the app's pick (click it: dark or light), a searchable song list on one screen (the selected
//! song raised out of it, opening up with its cover and preview), a toolbar under it, and dialogs for choosing a
//! part and managing playlists. One row per song: a psarc and its converted feedpak share a row (PLAN.md section 5).
//!
//! Every layout number is the Python app's, in its whole pixels (`px(n)` = `int(n * s)`, `snap(v)` = `int(v)`), so
//! each edge lands on the pixel it has there.

use std::collections::HashMap;
use std::time::Instant;

use eframe::egui;
use egui::{Align2, Color32, Event, Key, Painter, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap, keycap_width};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{self, fmt_time, t};
use qrs_ui::units::{self, pixels, px, to_px};
use qrs_ui::widgets::{self, ButtonStyle, RoundKind, button, button_width, interact};

use crate::state::AppState;

// -- the Python app's whole pixels ----------------------------------------------------------------------------------

/// The Python app's scale (1 until the app sets one).
pub fn scale() -> f64 {
    let s = units::scale();
    if s > 0.0 { s } else { 1.0 }
}

/// `int(v)` of a position or length in points: the whole pixel the Python app has there (rounded toward zero). As
/// `units::snap`, but a number that is a whole pixel to within f32's error stays on that pixel: an f32 sum of
/// layout numbers is off by up to ~1e-4 px at window size, more than `units::snap`'s 1e-6 px allowance, so it would
/// drop such a number a pixel about half the time.
pub fn snap(v: f32) -> f32 {
    let s = units::scale();
    if s <= 0.0 {
        return v;
    }
    let x = v as f64 * s;
    let near = x.round();
    let k = if (x - near).abs() < 1e-3 { near } else { x.trunc() };
    (k / s) as f32
}

/// A whole-pixel position or length (points) as the Python app's integer, for its integer arithmetic
/// (`main.width * 0.34`, `(w - pw) // 2`, comparisons).
pub fn ipx(v: f32) -> f64 {
    to_px(v).round()
}

/// Python pixels (a float, e.g. `min(380 * s, main.width * 0.34)`) cut to a whole one, `int(...)`, as points.
pub fn trunc_px(v: f64) -> f32 {
    pixels(v.trunc() as f32)
}

/// pygame's `r.centerx`: `r.x + r.w // 2`.
pub fn centerx(r: Rect) -> f32 {
    r.min.x + snap(r.width() / 2.0)
}

/// pygame's `r.centery`: `r.y + r.h // 2`.
pub fn centery(r: Rect) -> f32 {
    r.min.y + snap(r.height() / 2.0)
}

/// pygame's `r.inflate(dx, dy)` (whole pixels): `dx` wider and `dy` taller, its left and top moving out by half of
/// them (C's integer halves, toward zero).
pub fn inflate(r: Rect, dx: f32, dy: f32) -> Rect {
    let half = |d: f32| pixels(((ipx(d) as i64) / 2) as f32);
    Rect::from_min_size(
        pos2(r.min.x - half(dx), r.min.y - half(dy)),
        vec2(r.width() + dx, r.height() + dy),
    )
}

/// The Python app's thin lines: `max(1, int(s))` pixels.
pub fn line_w() -> i64 {
    (scale().trunc() as i64).max(1)
}

/// `pygame.draw.line` across (y fixed) or down (x fixed), `w` pixels thick: every pixel from `a` to `b` inclusive,
/// thickened as pygame does (`w // 2` either side, one fewer before it when `w` is even).
pub fn line(p: &Painter, a: Pos2, b: Pos2, w: i64, color: Color32) {
    let one = pixels(1.0);
    let (before, after) = (pixels((w / 2 - (1 - w % 2)) as f32), pixels((w / 2) as f32));
    let r = if a.y == b.y {
        Rect::from_min_max(
            pos2(a.x.min(b.x), a.y - before),
            pos2(a.x.max(b.x) + one, a.y + after + one),
        )
    } else {
        Rect::from_min_max(
            pos2(a.x - before, a.y.min(b.y)),
            pos2(a.x + after + one, a.y.max(b.y) + one),
        )
    };
    paint::rrect(p, r, 0.0, color);
}

pub const SORTS: [&str; 6] = [
    "artist",
    "title",
    "recently added",
    "recently played",
    "best score",
    "most played",
];
const EXPAND: f32 = 100.0;
const ROW_H: f32 = 52.0;
const PREVIEW_DELAY: f32 = 0.5;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Home,
    All,
    Fav,
    Recent,
    Played,
    Unplayed,
    Tuning(String),
    Playlist(String),
}

impl View {
    pub fn title(&self) -> String {
        match self {
            View::Home => "Home".into(),
            View::All => "All songs".into(),
            View::Fav => "Favourites".into(),
            View::Recent => "Recently played".into(),
            View::Played => "Played".into(),
            View::Unplayed => "Not played yet".into(),
            View::Tuning(t) => t.clone(),
            View::Playlist(n) => n.clone(),
        }
    }
}

/// What the screen asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Req {
    /// Open the player on row g's part `part` (its key).
    Play {
        g: usize,
        part: String,
    },
    Settings(&'static str),
    Fullscreen,
}

pub struct TextBox {
    pub title: String,
    pub text: String,
    pub verb: String,
    pub skip: Option<char>,
    pub then: TextThen,
}

#[derive(Clone, Debug)]
pub enum TextThen {
    NewPlaylist { add: Option<usize> },
    Rename(String),
}

/// The main part of a song's tuning (its first guitar part's).
pub fn main_tuning(e: &qrs_library::SongEntry) -> String {
    e.arrangements
        .iter()
        .find(|a| a.name.to_lowercase() != "bass")
        .or(e.arrangements.first())
        .map(|a| a.tuning.clone())
        .unwrap_or_else(|| "?".into())
}

/// The part you'd play by default: the kind you played last (lead, rhythm, bass...), else the first that isn't a
/// bonus.
pub fn last_part<'a>(st: &AppState, e: &'a qrs_library::SongEntry) -> Option<&'a qrs_library::PartEntry> {
    let want = st.config.str("last_arrangement", "lead");
    e.arrangements
        .iter()
        .find(|a| a.name.to_lowercase() == want)
        .or_else(|| e.arrangements.iter().find(|a| !a.bonus))
        .or(e.arrangements.first())
}

/// The sidebar well and the main column, for Home and the library pages.
pub fn page_rects(area: Rect) -> (Rect, Rect) {
    let pad = px(28.0);
    let side = Rect::from_min_size(
        area.min + vec2(pad, pad),
        vec2(px(250.0), area.height() - 2.0 * pad),
    );
    let main = Rect::from_min_max(
        pos2(side.max.x + px(32.0), area.min.y + pad),
        pos2(area.max.x - pad, area.max.y - pad),
    );
    (side, main)
}

/// A text anchor at its whole pixel (`int`, as pygame's `Rect` takes it), a hundredth of a pixel inside it (away
/// from zero): `text::draw` truncates the anchor with the window's `pixels_per_point`, which can sit a hair under
/// the Python scale, and would put a point exactly on a pixel boundary a whole pixel up or left.
fn anchor_at(at: Pos2) -> Pos2 {
    let nudge = |v: f32| {
        let v = snap(v);
        v + pixels(0.01).copysign(v)
    };
    pos2(nudge(at.x), nudge(at.y))
}

/// A text's room with the same allowance: `blit_text` cuts a line only when it is wider than `max_w`, and a room
/// of whole pixels can come out a hair short of them in f32.
fn room(max_w: Option<f32>) -> Option<f32> {
    max_w.map(|m| m + pixels(0.01))
}

/// `blit_text`: the anchor point taken to its whole pixel, as pygame's `Rect` does.
pub fn txt(
    p: &Painter,
    s: &str,
    at: Pos2,
    size: f32,
    color: Color32,
    weight: u16,
    anchor: Align2,
    max_w: Option<f32>,
) -> Rect {
    text::draw(
        p,
        s,
        anchor_at(at),
        anchor,
        Style::new(size, weight, color),
        room(max_w),
    )
}

pub fn caps(p: &Painter, s: &str, at: Pos2, size: f32, anchor: Align2, max_w: Option<f32>) -> Rect {
    text::caps(p, s, anchor_at(at), size, t().caption, anchor, room(max_w))
}

/// A favourite star: the accent when on, a dim outline when off. r in points; its rect (for clicks) is
/// `Rect(cx - r, cy - r, 2r, 2r)` in whole pixels, as the Python app's.
pub fn draw_star(p: &Painter, c: Pos2, r: f32, on: bool) -> Rect {
    let th = t();
    icons::icon(
        p,
        if on { Icon::Star } else { Icon::StarOff },
        c,
        2.0 * r,
        if on { th.accent } else { th.star_off },
    );
    Rect::from_min_size(pos2(snap(c.x - r), snap(c.y - r)), Vec2::splat(snap(2.0 * r)))
}

fn fmt_int(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

pub fn best_text(best: Option<(i64, f64, f64)>) -> String {
    match best {
        Some((score, acc, _)) => format!("{}  ·  {}%", fmt_int(score), (acc * 100.0).round() as i64),
        None => "–".into(),
    }
}

pub struct LibView {
    pub query: String,
    pub skip_text: Option<char>,
    pub sort: usize,
    pub view: View,
    pub items: Vec<usize>,
    key: Option<(u64, String, usize, View)>,
    pub sel: usize,
    pub top: usize,
    pub picker: Option<usize>,
    pub picker_sel: usize,
    pub menu: Option<usize>,
    /// The `AppState::regroups` the row numbers here belong to.
    rows_of: u64,
    pub text_box: Option<TextBox>,
    grow: HashMap<String, f32>,
    sel_song: Option<usize>,
    sel_since: Instant,
    quiet: Option<String>,
    pub side_scroll: f32,
    side_max: f32,
    side_rect: Rect,
    list: Option<Rect>,
    counts: Option<(u64, Counts)>,
    tunings: Option<(u64, Vec<(String, usize)>)>,
    pub reqs: Vec<Req>,
}

#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub all: usize,
    pub fav: usize,
    pub recent: usize,
    pub played: usize,
    pub unplayed: usize,
    pub playlists: Vec<(String, usize)>,
}

impl Default for LibView {
    fn default() -> Self {
        LibView {
            query: String::new(),
            skip_text: None,
            sort: 0,
            view: View::Home,
            items: Vec::new(),
            key: None,
            sel: 0,
            top: 0,
            picker: None,
            picker_sel: 0,
            menu: None,
            rows_of: 0,
            text_box: None,
            grow: HashMap::new(),
            sel_song: None,
            sel_since: Instant::now(),
            quiet: None,
            side_scroll: 0.0,
            side_max: 0.0,
            side_rect: Rect::NOTHING,
            list: None,
            counts: None,
            tunings: None,
            reqs: Vec::new(),
        }
    }
}

impl LibView {
    // -- data ---------------------------------------------------------------------------------------------------

    fn rows(st: &AppState) -> impl Iterator<Item = usize> + '_ {
        (0..st.groups.len()).filter(|&g| st.listed(g))
    }

    pub fn tuning_counts(&mut self, st: &AppState) -> Vec<(String, usize)> {
        if let Some((v, tc)) = &self.tunings
            && *v == st.version
        {
            return tc.clone();
        }
        let mut m: HashMap<String, usize> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for g in Self::rows(st) {
            let tn = main_tuning(st.song(g));
            if !m.contains_key(&tn) {
                order.push(tn.clone());
            }
            *m.entry(tn).or_default() += 1;
        }
        let mut tc: Vec<(String, usize)> = order.into_iter().map(|k| (k.clone(), m[&k])).collect();
        tc.sort_by_key(|t| std::cmp::Reverse(t.1)); // Counter.most_common: by count, ties in first-seen order
        self.tunings = Some((st.version, tc.clone()));
        tc
    }

    pub fn counts(&mut self, st: &AppState) -> Counts {
        let key = st.version.wrapping_mul(1_000_003) ^ st.userdata.version;
        if let Some((k, c)) = &self.counts
            && *k == key
        {
            return c.clone();
        }
        let ud = &st.userdata;
        let mut c = Counts::default();
        let mut row_ids: Vec<Vec<String>> = Vec::new();
        for g in Self::rows(st) {
            let ids = st.ids(g);
            c.all += 1;
            if ud.is_favorite(&ids) {
                c.fav += 1;
            }
            if ud.last_played(&ids).is_some() {
                c.recent += 1;
            } else {
                c.unplayed += 1;
            }
            if ud.best(&ids, None).is_some() {
                c.played += 1;
            }
            row_ids.push(ids);
        }
        for (name, members) in &ud.playlists {
            let n = row_ids
                .iter()
                .filter(|ids| ids.iter().any(|i| members.contains(i)))
                .count();
            c.playlists.push((name.clone(), n));
        }
        self.counts = Some((key, c.clone()));
        c
    }

    pub fn refresh(&mut self, st: &AppState) {
        // the rows were rebuilt (the library read again): the songs held here, on their rows now
        let mut current = self.items.get(self.sel).copied();
        if self.rows_of != st.regroups {
            let since = self.rows_of;
            let follow = |g: Option<usize>| g.and_then(|g| st.follow(g, since));
            current = follow(current);
            self.picker = follow(self.picker);
            self.menu = follow(self.menu);
            self.sel_song = follow(self.sel_song);
            self.rows_of = st.regroups;
        }
        let key = (
            st.version.wrapping_mul(1_000_003) ^ st.userdata.version,
            self.query.clone(),
            self.sort,
            self.view.clone(),
        );
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        let ud = &st.userdata;
        let top_tunings: Vec<String> = self.tuning_counts(st).into_iter().take(6).map(|t| t.0).collect();
        let mut items: Vec<usize> = Self::rows(st)
            .filter(|&g| {
                let ids = st.ids(g);
                match &self.view {
                    View::Fav => ud.is_favorite(&ids),
                    View::Recent => ud.last_played(&ids).is_some(),
                    View::Played => ud.best(&ids, None).is_some(),
                    View::Unplayed => ud.last_played(&ids).is_none(),
                    View::Tuning(tn) => {
                        let mt = main_tuning(st.song(g));
                        mt == *tn || (tn == "Other" && !top_tunings.contains(&mt))
                    }
                    View::Playlist(name) => ud.in_playlist(name, &ids),
                    _ => true,
                }
            })
            .collect();
        let words: Vec<String> = self
            .query
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        if !words.is_empty() {
            items.retain(|&g| {
                let st_text = st.song(g).search_text();
                words.iter().all(|w| st_text.contains(w.as_str()))
            });
        }
        let mode = SORTS[self.sort];
        let recent = |g: usize| ud.last_played(&st.ids(g)).unwrap_or(0.0);
        if let View::Playlist(name) = &self.view
            && self.query.is_empty()
        {
            let order: Vec<String> = ud
                .playlists
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let pos = |g: usize| {
                st.ids(g)
                    .iter()
                    .filter_map(|i| order.iter().position(|o| o == i))
                    .min()
                    .unwrap_or(usize::MAX)
            };
            items.sort_by_key(|&g| pos(g));
        } else if self.view == View::Recent || mode == "recently played" {
            items.sort_by(|&a, &b| recent(b).total_cmp(&recent(a)));
        } else if mode == "artist" {
            let k = |g: usize| {
                let s = st.song(g);
                let a = s.artist.to_lowercase();
                (
                    a.strip_prefix("the ").map(String::from).unwrap_or(a),
                    s.display_title().to_lowercase(),
                )
            };
            items.sort_by_cached_key(|&g| k(g));
        } else if mode == "title" {
            items.sort_by_cached_key(|&g| st.song(g).display_title().to_lowercase());
        } else if mode == "recently added" {
            items.sort_by(|&a, &b| st.song(b).mtime.total_cmp(&st.song(a).mtime));
        } else if mode == "best score" {
            let acc = |g: usize| ud.best(&st.ids(g), None).map(|b| b.1).unwrap_or(-1.0);
            items.sort_by(|&a, &b| acc(b).total_cmp(&acc(a)));
        } else if mode == "most played" {
            items.sort_by_key(|&g| -ud.plays(&st.ids(g)));
        }
        self.items = items;
        if let Some(c) = current
            && let Some(i) = self.items.iter().position(|&g| g == c)
        {
            self.sel = i;
        }
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
    }

    pub fn selected(&self) -> Option<usize> {
        self.items.get(self.sel).copied()
    }

    fn expanding(st: &AppState) -> bool {
        st.config.bool("select_preview", true)
    }

    /// Where the rows are (inside the song list's well, under its column header).
    fn list_rect(&self, area: Rect) -> Rect {
        if let Some(r) = self.list {
            return r;
        }
        let (_, main) = page_rects(area);
        Rect::from_min_size(
            main.min + vec2(px(12.0), px(114.0)),
            vec2(main.width() - px(24.0), main.height() - px(190.0)),
        )
    }

    /// How much taller a row is while it opens up (smoothstep of its grow), in whole pixels.
    fn extra(&self, st: &AppState, g: usize) -> f32 {
        let gr = self.grow.get(&st.song(g).id()).copied().unwrap_or(0.0) as f64;
        trunc_px(EXPAND as f64 * scale() * (gr * gr * (3.0 - 2.0 * gr)))
    }

    /// (index, y, height) of the rows on screen, starting at self.top; the selected row is taller.
    fn row_layout(&self, st: &AppState, r: Rect) -> Vec<(usize, f32, f32)> {
        let rh = px(ROW_H);
        let mut out = Vec::new();
        let (mut y, mut i) = (r.min.y, self.top);
        while i < self.items.len() && ipx(y) < ipx(r.max.y) {
            let h = rh + self.extra(st, self.items[i]);
            out.push((i, y, h));
            y = snap(y + h);
            i += 1;
        }
        out
    }

    fn visible_rows(&self, st: &AppState, area: Rect) -> usize {
        let extra = if Self::expanding(st) { ipx(px(EXPAND)) } else { 0.0 };
        let n = ((ipx(self.list_rect(area).height()) - extra) / ipx(px(ROW_H))).floor();
        (n as i64).max(1) as usize
    }

    fn ensure_visible(&mut self, st: &AppState, area: Rect) {
        let n = self.visible_rows(st, area);
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + n {
            self.top = self.sel + 1 - n;
        }
        self.top = self.top.min(self.items.len().saturating_sub(n));
    }

    // -- actions ------------------------------------------------------------------------------------------------

    pub fn open_picker(&mut self, st: &AppState, g: usize) {
        self.picker = Some(g);
        let last = st.config.str("last_arrangement", "lead");
        self.picker_sel = st
            .song(g)
            .arrangements
            .iter()
            .position(|a| a.name.to_lowercase() == last)
            .unwrap_or(0);
    }

    fn choose(&mut self, st: &mut AppState, i: usize) {
        let Some(g) = self.picker.take() else { return };
        let Some(a) = st.song(g).arrangements.get(i).cloned() else {
            return;
        };
        st.config.set("last_arrangement", a.name.to_lowercase());
        self.reqs.push(Req::Play { g, part: a.key });
    }

    fn toggle_favorite(&mut self, st: &mut AppState, g: Option<usize>) {
        let Some(g) = g else { return };
        let ids = st.ids(g);
        let canon = st.groups[g].canonical.clone();
        let on = st.userdata.toggle_favorite(&ids);
        if let Some(first) = ids.first() {
            st.userdata.alias(&canon, first);
        }
        let title = st.song(g).display_title();
        st.say(format!(
            "{} {title} {} favourites",
            if on { "Added" } else { "Removed" },
            if on { "to" } else { "from" }
        ));
    }

    fn new_playlist(&mut self, add: Option<usize>, skip: Option<char>) {
        self.text_box = Some(TextBox {
            title: "New playlist name".into(),
            text: String::new(),
            verb: "Create".into(),
            skip,
            then: TextThen::NewPlaylist { add },
        });
    }

    fn text_ok(&mut self, st: &mut AppState, tb: TextBox) {
        let text = tb.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        match tb.then {
            TextThen::NewPlaylist { add } => {
                let name = st.userdata.create_playlist(&text);
                if let Some(g) = add {
                    let ids = st.ids(g);
                    st.userdata.toggle_in_playlist(&name, &ids);
                    let title = st.song(g).display_title();
                    st.say(format!("Added {title} to {name}"));
                }
            }
            TextThen::Rename(old) => {
                let new = st.userdata.rename_playlist(&old, &text);
                self.view = View::Playlist(new);
            }
        }
    }

    pub fn act(&mut self, st: &mut AppState, area: Rect, name: &str) {
        let song = self.selected();
        match name {
            "play" => {
                if let Some(g) = song {
                    self.open_picker(st, g);
                }
            }
            "play_now" => {
                if let Some(g) = song
                    && let Some(a) = last_part(st, st.song(g)).cloned()
                {
                    st.config.set("last_arrangement", a.name.to_lowercase());
                    self.reqs.push(Req::Play { g, part: a.key });
                }
            }
            "preview" => {
                if let Some(g) = song {
                    let e = st.song(g).clone();
                    if st.previewer.want().as_deref() == Some(e.id().as_str()) {
                        st.previewer.stop();
                        self.quiet = Some(e.id());
                    } else {
                        self.quiet = None;
                        st.previewer.request(&e);
                    }
                }
            }
            "sort" => self.sort = (self.sort + 1) % SORTS.len(),
            "fav" => self.toggle_favorite(st, song),
            "addto" => self.menu = song,
            "remove" => {
                if let (Some(g), View::Playlist(name)) = (song, self.view.clone()) {
                    let ids = st.ids(g);
                    st.userdata.toggle_in_playlist(&name, &ids);
                    let title = st.song(g).display_title();
                    st.say(format!("Removed {title} from {name}"));
                }
            }
            "rename_pl" => {
                if let View::Playlist(name) = &self.view {
                    self.text_box = Some(TextBox {
                        title: "Rename playlist".into(),
                        text: name.clone(),
                        verb: "Rename".into(),
                        skip: None,
                        then: TextThen::Rename(name.clone()),
                    });
                }
            }
            "delete_pl" => {
                if let View::Playlist(name) = self.view.clone() {
                    st.userdata.delete_playlist(&name);
                    st.say(format!("Deleted playlist {name}"));
                    self.view = View::All;
                }
            }
            "new_pl" => self.new_playlist(None, None),
            "rescan" => {
                st.library
                    .rescan(st.keys.clone(), st.dirs.clone(), st.data.index());
            }
            "select_preview" => {
                let on = !Self::expanding(st);
                st.config.set("select_preview", on);
                st.save_config();
                if !on {
                    st.previewer.stop();
                }
                self.ensure_visible(st, area);
            }
            "settings" => self.reqs.push(Req::Settings("audio")),
            "fullscreen" => self.reqs.push(Req::Fullscreen),
            _ => {}
        }
    }

    /// Open a sidebar page (Home included).
    pub fn go(&mut self, st: &mut AppState, v: View) {
        if v == View::Home {
            self.query.clear();
            st.previewer.stop();
        }
        self.view = v;
        self.sel = 0;
        self.top = 0;
    }

    // -- keys ---------------------------------------------------------------------------------------------------

    /// Keyboard and wheel input for the library page (and its dialogs). Returns true if it was used.
    pub fn handle(&mut self, st: &mut AppState, area: Rect, ev: &Event) -> bool {
        if self.text_box.is_some() {
            self.handle_text(st, ev);
            return true;
        }
        if self.menu.is_some() {
            self.handle_menu(st, ev);
            return true;
        }
        if self.picker.is_some() {
            self.handle_picker(st, ev);
            return true;
        }
        match ev {
            Event::Text(s) => {
                let skip = self.skip_text.take();
                for c in s.chars() {
                    if skip.is_some_and(|k| c.to_ascii_lowercase() == k) {
                        continue; // the letter of the key that opened the library (L on Home)
                    }
                    self.query.push(c);
                }
                self.sel = 0;
                self.top = 0;
                true
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let ctrl = modifiers.command;
                self.skip_text = None;
                let n = self.items.len();
                match key {
                    Key::Backspace if self.query.is_empty() => self.view = View::All,
                    Key::Backspace => {
                        if ctrl {
                            self.query.clear()
                        } else {
                            self.query.pop();
                        }
                        self.sel = 0;
                        self.top = 0;
                    }
                    Key::Escape => {
                        if !self.query.is_empty() {
                            self.query.clear();
                        } else if self.view != View::All {
                            self.view = View::All;
                        } else {
                            st.previewer.stop();
                            self.view = View::Home;
                        }
                    }
                    Key::D if ctrl => self.act(st, area, "fav"),
                    Key::P if ctrl => self.act(st, area, "addto"),
                    Key::Delete => self.act(st, area, "remove"),
                    Key::ArrowDown => self.sel = (self.sel + 1).min(n.saturating_sub(1)),
                    Key::ArrowUp => self.sel = self.sel.saturating_sub(1),
                    Key::PageDown => {
                        self.sel = (self.sel + self.visible_rows(st, area)).min(n.saturating_sub(1))
                    }
                    Key::PageUp => self.sel = self.sel.saturating_sub(self.visible_rows(st, area)),
                    Key::Home if ctrl => self.sel = 0,
                    Key::End if ctrl => self.sel = n.saturating_sub(1),
                    Key::Enter => self.act(st, area, if modifiers.shift { "play" } else { "play_now" }),
                    Key::Tab => self.act(st, area, "sort"),
                    Key::F5 => self.act(st, area, "rescan"),
                    _ => return false,
                }
                self.sel = self.sel.min(self.items.len().saturating_sub(1));
                self.ensure_visible(st, area);
                true
            }
            Event::MouseWheel { unit, delta, .. } => {
                let notches = wheel_notches(*unit, delta.y);
                let pos = st.ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
                if self.side_rect.contains(pos) {
                    self.side_scroll = (self.side_scroll - notches * 40.0).clamp(0.0, self.side_max);
                } else {
                    let max_top = self.items.len().saturating_sub(self.visible_rows(st, area)) as i64;
                    self.top = (self.top as i64 - (notches * 3.0) as i64).clamp(0, max_top.max(0)) as usize;
                }
                true
            }
            _ => false,
        }
    }

    fn handle_picker(&mut self, st: &mut AppState, ev: &Event) {
        let Some(g) = self.picker else { return };
        let n = st.song(g).arrangements.len().max(1);
        if let Event::Key {
            key, pressed: true, ..
        } = ev
        {
            match key {
                Key::Escape | Key::Backspace => self.picker = None,
                Key::ArrowDown | Key::ArrowRight => self.picker_sel = (self.picker_sel + 1) % n,
                Key::ArrowUp | Key::ArrowLeft => self.picker_sel = (self.picker_sel + n - 1) % n,
                Key::Enter | Key::Space => self.choose(st, self.picker_sel),
                Key::F => self.toggle_favorite(st, Some(g)),
                Key::P => self.menu = Some(g),
                k => {
                    if let Some(d) = digit(*k)
                        && d >= 1
                        && d <= n
                    {
                        self.choose(st, d - 1);
                    }
                }
            }
        }
    }

    fn handle_menu(&mut self, st: &mut AppState, ev: &Event) {
        let Some(g) = self.menu else { return };
        if let Event::Key {
            key, pressed: true, ..
        } = ev
        {
            match key {
                Key::Escape | Key::Enter | Key::Backspace => self.menu = None,
                Key::N => {
                    self.menu = None;
                    self.new_playlist(Some(g), Some('n'));
                }
                k => {
                    let names: Vec<String> = st.userdata.playlists.iter().map(|p| p.0.clone()).collect();
                    if let Some(d) = digit(*k)
                        && d >= 1
                        && d <= names.len().min(9)
                    {
                        let ids = st.ids(g);
                        st.userdata.toggle_in_playlist(&names[d - 1], &ids);
                    }
                }
            }
        }
    }

    fn handle_text(&mut self, st: &mut AppState, ev: &Event) {
        let Some(tb) = &mut self.text_box else { return };
        match ev {
            Event::Text(s) => {
                let skip = tb.skip.take();
                for c in s.chars() {
                    if skip.is_some_and(|k| c.to_ascii_lowercase() == k) {
                        continue;
                    }
                    tb.text.push(c);
                }
            }
            Event::Key {
                key, pressed: true, ..
            } => match key {
                Key::Backspace => {
                    tb.text.pop();
                }
                Key::Escape => self.text_box = None,
                Key::Enter => {
                    if let Some(tb) = self.text_box.take() {
                        self.text_ok(st, tb);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    // -- drawing ------------------------------------------------------------------------------------------------

    /// Animate the selected row open, and play its preview once it has stayed selected for a moment (unless its
    /// preview key stopped it).
    fn update_selection(&mut self, st: &mut AppState) {
        let on = Self::expanding(st);
        let dt = st.ctx.input(|i| i.stable_dt).min(0.1);
        let song = if on && self.menu.is_none() && self.text_box.is_none() {
            self.selected()
        } else {
            None
        };
        let target = song.map(|g| st.song(g).id());
        let mut keys: Vec<String> = self.grow.keys().cloned().collect();
        if let Some(tg) = &target
            && !keys.contains(tg)
        {
            keys.push(tg.clone());
        }
        for sid in keys {
            let g = self.grow.get(&sid).copied().unwrap_or(0.0);
            let g = if Some(&sid) == target.as_ref() {
                (g + dt * 5.0).min(1.0)
            } else {
                (g - dt * 6.0).max(0.0)
            };
            if g <= 0.0 {
                self.grow.remove(&sid);
            } else {
                if g < 1.0 {
                    st.ctx.request_repaint();
                }
                self.grow.insert(sid, g);
            }
        }
        if let Some(pg) = self.picker {
            if on {
                let e = st.song(pg).clone();
                st.previewer.request(&e);
            }
            return;
        }
        if song != self.sel_song {
            self.sel_song = song;
            self.sel_since = Instant::now();
            self.quiet = None;
            if let Some(w) = st.previewer.want()
                && Some(&w) != target.as_ref()
            {
                st.previewer.stop();
            }
        } else if let Some(g) = song {
            let e = st.song(g).clone();
            if self.sel_since.elapsed().as_secs_f32() >= PREVIEW_DELAY
                && self.quiet.as_ref() != target.as_ref()
            {
                st.previewer.request(&e);
            } else if self.sel_since.elapsed().as_secs_f32() < PREVIEW_DELAY {
                st.ctx
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        self.refresh(st);
        self.update_selection(st);
        let p = ui.painter().clone();
        let th = t();
        let (side, main) = page_rects(area);
        self.draw_sidebar(ui, st, side);
        // header: title and count, the search box, the sort
        let hy = main.min.y + px(26.0);
        let title = self.view.title();
        let tr = txt(
            &p,
            &title,
            pos2(main.min.x, hy),
            28.0,
            th.text,
            800,
            Align2::LEFT_CENTER,
            Some(snap(main.width() / 3.0)),
        );
        let total = self.counts(st).all;
        let mut count = if self.items.len() != total {
            format!(
                "{} of {}",
                fmt_int(self.items.len() as i64),
                fmt_int(total as i64)
            )
        } else {
            format!("{} songs", fmt_int(total as i64))
        };
        if st.scanning() {
            use std::sync::atomic::Ordering::Relaxed;
            count += &format!(
                "  ·  scanning {}/{}",
                fmt_int(st.library.done.load(Relaxed) as i64),
                fmt_int(st.library.total.load(Relaxed) as i64)
            );
            st.ctx
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        let sort_label = format!("Sort: {}", SORTS[self.sort]);
        let sw = button_width(&st.ctx, &sort_label, "Tab", 14.0, false);
        let sort = Rect::from_min_size(pos2(main.max.x - sw, hy - px(24.0)), vec2(sw, px(48.0)));
        if button(
            ui,
            sort,
            "lib-sort",
            &sort_label,
            "Tab",
            ButtonStyle {
                size: 14.0,
                r: Some(16.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            self.act(st, area, "sort");
        }
        let bw = trunc_px((380.0 * scale()).min(ipx(main.width()) * 0.34));
        let boxr = Rect::from_min_size(
            pos2(sort.min.x - px(18.0) - bw, hy - px(26.0)),
            vec2(bw, px(52.0)),
        );
        draw_search(
            &p,
            boxr,
            &self.query,
            &format!("Type to search {} songs", fmt_int(total as i64)),
            &[],
        );
        txt(
            &p,
            &count,
            pos2(tr.max.x + px(14.0), hy + px(2.0)),
            15.0,
            th.caption,
            600,
            Align2::LEFT_CENTER,
            Some(boxr.min.x - tr.max.x - px(28.0)),
        );
        // the toolbar along the bottom, then the list in a screen between
        let bar = Rect::from_min_size(
            pos2(main.min.x, main.max.y - px(64.0)),
            vec2(main.width(), px(64.0)),
        );
        self.draw_toolbar(ui, st, area, bar);
        let top = hy + px(26.0) + px(20.0);
        let well = Rect::from_min_max(pos2(main.min.x, top), pos2(main.max.x, bar.min.y - px(20.0)));
        self.draw_list(ui, st, area, well);
        if let Some((msg, at)) = &st.toast {
            if at.elapsed().as_secs_f32() < 5.0 {
                widgets::toast(&p, msg, pos2(centerx(well), well.max.y - px(36.0)), "");
                st.ctx
                    .request_repaint_after(std::time::Duration::from_millis(250));
            } else {
                st.toast = None;
            }
        }
        if self.picker.is_some() {
            self.draw_picker(ui, st, area);
        }
        if self.menu.is_some() {
            self.draw_menu(ui, st, area);
        }
        if self.text_box.is_some() {
            self.draw_text_box(ui, st, area);
        }
    }

    fn draw_toolbar(&mut self, ui: &Ui, st: &mut AppState, area: Rect, bar: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = st.ctx.clone();
        paint::raised(&p, bar, Some(22.0), 6.0, Fill::Base, 0.0);
        let fav_on = self
            .selected()
            .is_some_and(|g| st.userdata.is_favorite(&st.ids(g)));
        let mut left: Vec<(&str, Vec<&str>, &str, &str)> = vec![
            (
                "Favourite",
                vec!["Ctrl", "D"],
                "fav",
                if fav_on { "fav" } else { "fav_off" },
            ),
            ("Add to playlist", vec!["Ctrl", "P"], "addto", ""),
        ];
        if matches!(self.view, View::Playlist(_)) {
            left.push(("Remove", vec!["Del"], "remove", ""));
            left.push(("Rename", vec![], "rename_pl", ""));
            left.push(("Delete playlist", vec![], "delete_pl", "danger"));
        }
        left.push(("Preview the selected song", vec![], "select_preview", "switch"));
        let right: Vec<(&str, Vec<&str>, &str, &str)> = vec![
            ("Rescan", vec!["F5"], "rescan", "dim"),
            ("Fullscreen", vec!["F11"], "fullscreen", "dim"),
            ("Settings", vec!["F2"], "settings", "dim"),
        ];
        let h = px(46.0);
        let cy = centery(bar);
        let width = |label: &str, keys: &[&str], kind: &str| -> f32 {
            let mut w = text::width(&ctx, label, Style::new(14.0, 700, th.text)) + px(28.0);
            w += keys
                .iter()
                .map(|k| keycap_width(&ctx, k, 11.0) + px(3.0))
                .sum::<f32>()
                + if keys.is_empty() { 0.0 } else { px(5.0) };
            if kind.starts_with("fav") {
                w += px(22.0);
            }
            if kind == "switch" {
                w += px(48.0) + px(12.0);
            }
            w
        };
        let mut acts: Vec<&str> = Vec::new();
        let tool = |x: f32,
                    label: &str,
                    keys: &[&str],
                    action: &'static str,
                    kind: &str,
                    acts: &mut Vec<&str>|
         -> f32 {
            let r = Rect::from_min_size(pos2(x, cy - snap(h / 2.0)), vec2(width(label, keys, kind), h));
            let resp = interact(ui, r, ("lib-tool", action), Sense::click());
            let hot = resp.hovered();
            if hot && kind != "switch" {
                paint::raised(&p, r, Some(14.0), 3.0, Fill::Base, 0.0);
            }
            if hot {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let mut tx = r.min.x + px(14.0);
            if kind.starts_with("fav") {
                draw_star(&p, pos2(tx + px(7.0), cy), 7.0, kind == "fav");
                tx += px(22.0);
            }
            let col = match kind {
                "danger" => th.miss,
                "dim" => th.text_2,
                _ => th.text,
            };
            let tr = txt(
                &p,
                label,
                pos2(tx, cy),
                14.0,
                col,
                if kind == "dim" { 600 } else { 700 },
                Align2::LEFT_CENTER,
                None,
            );
            let mut kx = tr.max.x + px(8.0);
            for k in keys {
                kx = keycap(&p, k, pos2(kx, cy), Align2::LEFT_CENTER, 11.0, CapOn::Material)
                    .max
                    .x
                    + px(3.0);
            }
            if kind == "switch" {
                // sw.midleft = (t.right + 12, cy)
                let (sw_w, sw_h) = (px(48.0), px(26.0));
                let sw =
                    Rect::from_min_size(pos2(tr.max.x + px(12.0), cy - snap(sw_h / 2.0)), vec2(sw_w, sw_h));
                widgets::draw_switch(&p, sw, egui::Id::new("libpreview"), LibView::expanding(st));
            }
            if resp.clicked() {
                acts.push(action);
            }
            r.max.x
        };
        let mut x = bar.min.x + px(10.0);
        for (label, keys, action, kind) in &left {
            if *kind == "switch" {
                // a thin line before the switch
                let lx = x + px(6.0);
                line(
                    &p,
                    pos2(lx, cy - px(14.0)),
                    pos2(lx, cy + px(14.0)),
                    line_w(),
                    th.line,
                );
                x += px(12.0);
            }
            x = tool(x, label, keys, static_str(action), kind, &mut acts) + px(4.0);
        }
        let total: f32 = right
            .iter()
            .map(|(l, k, _, kind)| width(l, k, kind) + px(4.0))
            .sum();
        let mut x = bar.max.x - px(10.0) - total + px(4.0);
        for (label, keys, action, kind) in &right {
            x = tool(x, label, keys, static_str(action), kind, &mut acts) + px(4.0);
        }
        for a in acts {
            self.act(st, area, a);
        }
    }

    /// x of each column in the song list: star, cover, song, tuning, best, length (at its right edge).
    fn columns(r: Rect) -> [f32; 6] {
        let s = scale();
        let x = r.min.x + px(12.0);
        let inner = r.width() - px(24.0);
        let (tun_w, best_w, len_w) = (
            trunc_px((160.0 * s).min(ipx(inner) * 0.18)),
            trunc_px((150.0 * s).min(ipx(inner) * 0.17)),
            px(70.0),
        );
        let length = x + inner;
        let best = length - len_w - best_w;
        let tuning = best - tun_w;
        let art = x + px(30.0);
        [x, art, art + px(56.0), tuning, best, length] // star, art, song, tuning, best, length
    }

    /// The songs on a screen: a header of columns, then a row each (a star for favourites, the cover, title and
    /// artist, tuning, your best, length); the selected song raised out of it, opened up with its cover, preview and
    /// Play when expanding is on.
    fn draw_list(&mut self, ui: &Ui, st: &mut AppState, area: Rect, well: Rect) {
        let p = ui.painter().clone();
        let th = t();
        paint::display(&p, well, Some(24.0), 6.0);
        let r = Rect::from_min_size(
            pos2(well.min.x + px(12.0), well.min.y + px(10.0) + px(34.0)),
            vec2(well.width() - px(24.0), well.height() - px(10.0) - px(34.0)),
        );
        self.list = Some(r);
        let [c_star, c_art, c_song, c_tun, c_best, c_len] = Self::columns(r);
        let hy = well.min.y + px(10.0) + px(17.0);
        caps(&p, "SONG", pos2(c_song, hy), 11.0, Align2::LEFT_CENTER, None);
        caps(&p, "TUNING", pos2(c_tun, hy), 11.0, Align2::LEFT_CENTER, None);
        caps(&p, "BEST", pos2(c_best, hy), 11.0, Align2::LEFT_CENTER, None);
        caps(&p, "LENGTH", pos2(c_len, hy), 11.0, Align2::RIGHT_CENTER, None);
        let rh = px(ROW_H);
        let clip = inflate(r, px(8.0), 0.0).intersect(inflate(well, -px(6.0), -px(6.0)));
        let lp = p.with_clip_rect(clip);
        let layout = self.row_layout(st, r);
        let mut acts: Vec<String> = Vec::new();
        let mut clicked_row: Option<(usize, bool)> = None;
        let mut star_hit: Option<usize> = None;
        for (n, &(i, ry, row_height)) in layout.iter().enumerate() {
            let g = self.items[i];
            let e = st.song(g).clone();
            let ids = st.ids(g);
            let extra = row_height - rh;
            let row = Rect::from_min_size(pos2(r.min.x, ry), vec2(r.width(), row_height));
            let chosen = i == self.sel;
            // (hover and clicks: the row inside the list, as row_at and `row and r collide`)
            let resp = interact(ui, row.intersect(r), ("lib-row", g), Sense::click());
            if resp.clicked() {
                clicked_row = Some((i, false));
            }
            if resp.double_clicked() {
                clicked_row = Some((i, true));
            }
            if chosen && ipx(extra) > ipx(px(8.0)) {
                let a = self.draw_open_row(
                    ui,
                    &lp,
                    st,
                    inflate(row, 0.0, -px(12.0)),
                    g,
                    extra / EXPAND,
                    &mut star_hit,
                );
                acts.extend(a);
                continue;
            }
            let next_chosen = layout.get(n + 1).is_some_and(|l| l.0 == self.sel);
            let hot = resp.hovered();
            if chosen {
                paint::raised(&lp, inflate(row, 0.0, -px(4.0)), Some(12.0), 3.0, Fill::Base, 0.0);
            } else if hot {
                paint::rrect(&lp, inflate(row, 0.0, -px(4.0)), px(12.0), th.screen_hi);
            }
            if !chosen && !next_chosen {
                let y = row.max.y - pixels(1.0);
                line(
                    &lp,
                    pos2(row.min.x + px(12.0), y),
                    pos2(row.max.x - px(12.0), y),
                    line_w(),
                    th.line,
                );
            }
            let cy = centery(row);
            let fav = st.userdata.is_favorite(&ids);
            let star = Rect::from_min_size(pos2(c_star, cy - px(9.0)), Vec2::splat(px(18.0)));
            let star_c = pos2(centerx(star), centery(star));
            if fav {
                paint::glow(&lp, inflate(star, -px(6.0), -px(6.0)), th.accent, 6.0, None, None);
                draw_star(&lp, star_c, 7.0, true);
            } else if hot {
                draw_star(&lp, star_c, 7.0, false);
            }
            if interact(
                ui,
                inflate(star, pixels(8.0), pixels(8.0)).intersect(clip),
                ("lib-star", g),
                Sense::click(),
            )
            .clicked()
            {
                star_hit = Some(g);
            }
            let art_r = Rect::from_min_size(pos2(c_art, cy - px(20.0)), Vec2::splat(px(40.0)));
            if !crate::covers::draw(&lp, &st.covers, &e, ipx(art_r.width()), art_r, 9.0, false) {
                paint::rrect(&lp, art_r, px(9.0), th.screen_hi);
            }
            let tw = c_tun - px(16.0) - c_song;
            txt(
                &lp,
                &e.display_title(),
                pos2(c_song, cy - px(9.0)),
                14.0,
                th.text,
                800,
                Align2::LEFT_CENTER,
                Some(tw),
            );
            let sub = if e.needs_keys {
                "needs keys".to_string()
            } else {
                e.artist.clone()
            };
            txt(
                &lp,
                &sub,
                pos2(c_song, cy + px(10.0)),
                12.0,
                if e.needs_keys { th.miss } else { th.caption },
                500,
                Align2::LEFT_CENTER,
                Some(tw),
            );
            txt(
                &lp,
                &main_tuning(&e),
                pos2(c_tun, cy),
                13.0,
                th.text_2,
                500,
                Align2::LEFT_CENTER,
                Some(c_best - c_tun - px(14.0)),
            );
            let best = st.userdata.best(&ids, None);
            let btxt = best_text(best);
            let bcol = if best.is_some() { th.text } else { th.caption };
            if chosen {
                // the selected row (when it doesn't open up): Play at its end
                let pw = button_width(&st.ctx, "Play", "Enter", 13.0, true);
                let play =
                    Rect::from_min_size(pos2(row.max.x - px(8.0) - pw, cy - px(18.0)), vec2(pw, px(36.0)));
                if button(
                    ui,
                    play,
                    ("lib-play", g),
                    "Play",
                    "Enter",
                    ButtonStyle {
                        primary: true,
                        icon: Some(Icon::Play),
                        size: 13.0,
                        r: Some(12.0),
                        ..Default::default()
                    },
                )
                .clicked()
                {
                    acts.push("play_now".into());
                }
                txt(
                    &lp,
                    &btxt,
                    pos2(c_best, cy),
                    13.0,
                    bcol,
                    700,
                    Align2::LEFT_CENTER,
                    Some(play.min.x - px(10.0) - c_best),
                );
            } else {
                txt(
                    &lp,
                    &btxt,
                    pos2(c_best, cy),
                    13.0,
                    bcol,
                    700,
                    Align2::LEFT_CENTER,
                    Some(c_len - px(70.0) - c_best),
                );
                txt(
                    &lp,
                    &fmt_time(e.length),
                    pos2(c_len, cy),
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
        }
        // the list fades out at the bottom of the screen
        let fade = Rect::from_min_size(
            pos2(well.min.x + px(6.0), well.max.y - px(46.0)),
            vec2(well.width() - px(12.0), px(40.0)),
        );
        draw_fade(&p, fade, th.screen);
        if self.items.is_empty() {
            let msg = if st.scanning() && st.songs.is_empty() {
                "Reading your library…".to_string()
            } else if !self.query.is_empty() {
                "No songs match your search.".to_string()
            } else {
                match self.view {
                    View::Fav => "No favourites yet: click a song's star (or Ctrl+D) to add it.".into(),
                    View::Playlist(_) => {
                        "This playlist is empty: select a song and press Ctrl+P (or Add to playlist).".into()
                    }
                    View::Recent => "Nothing played yet.".into(),
                    View::Played => "No finished songs yet.".into(),
                    _ => "No songs.".into(),
                }
            };
            txt(
                &p,
                &msg,
                pos2(centerx(well), r.min.y + px(40.0)),
                16.0,
                th.caption,
                600,
                Align2::CENTER_TOP,
                None,
            );
        }
        if st.dirs.is_empty() {
            txt(
                &p,
                "No Rocksmith dlc folder found. Add it in Settings > Library.",
                pos2(r.min.x + px(16.0), r.min.y + px(80.0)),
                15.0,
                th.miss,
                700,
                Align2::LEFT_TOP,
                None,
            );
        } else if !st.scanning() && st.songs.is_empty() {
            let dirs: Vec<String> = st.dirs.iter().map(|d| d.display().to_string()).collect();
            txt(
                &p,
                &format!(
                    "No songs yet: put your .psarc or .feedpak files in {}",
                    dirs.join("  or  ")
                ),
                pos2(r.min.x + px(16.0), r.min.y + px(80.0)),
                15.0,
                th.accent_ink,
                500,
                Align2::LEFT_TOP,
                Some(r.width() - px(32.0)),
            );
        }
        if let Some(g) = star_hit {
            self.toggle_favorite(st, Some(g));
        } else if let Some((i, double)) = clicked_row {
            self.sel = i;
            if double && let Some(g) = self.items.get(i).copied() {
                self.open_picker(st, g);
            }
        }
        for a in acts {
            self.act(st, area, &a);
        }
    }

    /// The selected song opened up: raised out of the screen, a big cover, its details, the preview with a key to
    /// stop it, and Play (the part you played last) and Pick a part. Returns the actions clicked.
    fn draw_open_row(
        &self,
        ui: &Ui,
        p: &Painter,
        st: &AppState,
        block: Rect,
        g: usize,
        grow: f32,
        star_hit: &mut Option<usize>,
    ) -> Vec<String> {
        let th = t();
        let ctx = &st.ctx;
        let e = st.song(g);
        let ids = st.ids(g);
        let mut acts = Vec::new();
        paint::raised(p, block, Some(20.0), 4.0, Fill::Base, 0.0);
        let pad = px(14.0);
        let size = (block.height() - 2.0 * pad).min(px(124.0));
        let bcy = centery(block);
        let art_r = Rect::from_min_size(pos2(block.min.x + pad, bcy - snap(size / 2.0)), Vec2::splat(size));
        crate::covers::draw(p, &st.covers, e, ipx(size), art_r, 16.0, true);
        let part = last_part(st, e).map(|a| a.name.clone()).unwrap_or_default();
        let play_label = format!("Play {part}");
        let bw = button_width(ctx, &play_label, "Enter", 15.0, true).max(button_width(
            ctx,
            "Pick a part",
            "Shift+Enter",
            14.0,
            false,
        ));
        let play = Rect::from_min_size(
            pos2(block.max.x - pad - px(6.0) - bw, bcy - px(48.0) - px(5.0)),
            vec2(bw, px(48.0)),
        );
        let pick = Rect::from_min_size(pos2(play.min.x, bcy + px(5.0)), vec2(bw, px(44.0)));
        if grow > 0.6 && !e.needs_keys {
            if button(
                ui,
                play,
                ("lib-open-play", g),
                &play_label,
                "Enter",
                ButtonStyle {
                    primary: true,
                    icon: Some(Icon::Play),
                    size: 15.0,
                    r: Some(15.0),
                    ..Default::default()
                },
            )
            .clicked()
            {
                acts.push("play_now".into());
            }
            if button(
                ui,
                pick,
                ("lib-open-pick", g),
                "Pick a part",
                "Shift+Enter",
                ButtonStyle {
                    size: 14.0,
                    r: Some(14.0),
                    ..Default::default()
                },
            )
            .clicked()
            {
                acts.push("play".into());
            }
        }
        let x = art_r.max.x + px(20.0);
        let tw = play.min.x - px(20.0) - x;
        let y = block.min.y + pad + px(4.0);
        let fav = st.userdata.is_favorite(&ids);
        let tr = txt(
            p,
            &e.display_title(),
            pos2(x, y),
            22.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(tw - px(28.0)),
        );
        let star = draw_star(p, pos2(tr.max.x + px(16.0), centery(tr)), 8.0, fav);
        if interact(
            ui,
            inflate(star, pixels(8.0), pixels(8.0)),
            ("lib-open-star", g),
            Sense::click(),
        )
        .clicked()
        {
            *star_hit = Some(g);
        }
        if grow < 0.6 {
            return acts;
        }
        let year = if e.year != 0 {
            e.year.to_string()
        } else {
            String::new()
        };
        let details: Vec<String> = [e.artist.clone(), e.album.clone(), year, fmt_time(e.length)]
            .into_iter()
            .filter(|v| !v.is_empty())
            .collect();
        txt(
            p,
            &details.join("  ·  "),
            pos2(x, tr.max.y + px(4.0)),
            14.0,
            th.text_2,
            600,
            Align2::LEFT_TOP,
            Some(tw),
        );
        let parts: Vec<&str> = e.arrangements.iter().map(|a| a.name.as_str()).collect();
        let mut bits = vec![parts.join(", "), main_tuning(e)];
        if let Some((score, acc, _)) = st.userdata.best(&ids, None) {
            bits.push(format!(
                "best {} ({}%)",
                fmt_int(score),
                (acc * 100.0).round() as i64
            ));
        }
        let plays = st.userdata.plays(&ids);
        bits.push(if plays > 0 {
            format!("played {plays}×")
        } else {
            "not played yet".into()
        });
        let src = st.groups[g].sources_label(&st.songs);
        if src.contains('·') {
            bits.push(src);
        }
        txt(
            p,
            &bits.join("  ·  "),
            pos2(x, tr.max.y + px(26.0)),
            13.0,
            th.caption,
            500,
            Align2::LEFT_TOP,
            Some(tw),
        );
        if e.needs_keys {
            txt(
                p,
                "This psarc needs Rocksmith's keys (Settings > Library).",
                pos2(x, tr.max.y + px(56.0)),
                13.0,
                th.miss,
                700,
                Align2::LEFT_TOP,
                Some(tw),
            );
            return acts;
        }
        if !st.previewer.has_engine() {
            return acts;
        }
        let cy = tr.max.y + px(66.0);
        let key = Rect::from_min_size(pos2(x, cy - px(15.0)), Vec2::splat(px(30.0)));
        let playing = st.previewer.want().as_deref() == Some(e.id().as_str());
        if widgets::round_button(
            ui,
            key,
            ("lib-pv", g),
            if playing { Icon::Pause } else { Icon::Play },
            12.0,
            RoundKind::Ivory,
            false,
        )
        .clicked()
        {
            acts.push("preview".into());
        }
        let groove = Rect::from_min_size(
            pos2(key.max.x + px(12.0), cy - px(5.0)),
            vec2(trunc_px((220.0 * scale()).min(ipx(tw) * 0.4)), px(10.0)),
        );
        paint::inset(p, groove, None, 2.0, Fill::Base);
        let pos = if playing { st.previewer.position() } else { None };
        let label = if playing && st.previewer.loading.load(std::sync::atomic::Ordering::Acquire) {
            "loading the preview…".to_string()
        } else if let Some((at, len)) = pos.filter(|(_, l)| *l > 0.0) {
            let u = (at / len).min(1.0);
            let m = px(2.0).max(pixels(1.0));
            let w = trunc_px(ipx(groove.width() - 2.0 * m) * u).max(groove.height() - 2.0 * m);
            paint::pill(
                p,
                Rect::from_min_size(groove.min + vec2(m, m), vec2(w, groove.height() - 2.0 * m)),
                th.accent,
            );
            st.ctx
                .request_repaint_after(std::time::Duration::from_millis(100));
            format!("preview {} / {}", fmt_time(at), fmt_time(len))
        } else if st.previewer.none.lock().ok().and_then(|n| n.clone()).as_deref() == Some(e.id().as_str()) {
            "no preview".to_string()
        } else {
            "preview".to_string()
        };
        txt(
            p,
            &label,
            pos2(groove.max.x + px(12.0), cy),
            12.0,
            th.caption,
            500,
            Align2::LEFT_CENTER,
            Some(play.min.x - groove.max.x - px(30.0)),
        );
        acts
    }

    // -- sidebar ------------------------------------------------------------------------------------------------

    /// The sunken sidebar: the pick (click it: dark or light) and the app's name, Home, the library views, tunings
    /// and playlists, each with its count; the page you're on is raised, in the accent.
    pub fn draw_sidebar(&mut self, ui: &Ui, st: &mut AppState, rect: Rect) {
        let p = ui.painter().clone();
        let th = t();
        self.side_rect = rect;
        paint::well(&p, rect, 26.0);
        let counts = self.counts(st);
        let tc = self.tuning_counts(st);
        let (x0, wd) = (rect.min.x + px(12.0), rect.width() - px(24.0));
        let clip = inflate(rect, -px(6.0), -px(6.0));
        let sp = p.with_clip_rect(clip);
        let mut y = rect.min.y + px(14.0) - snap(self.side_scroll);
        let top_y = y;
        let mark = Rect::from_min_size(pos2(x0 + px(6.0), y + px(5.0)), Vec2::splat(px(46.0)));
        draw_pick(&sp, st, mark);
        let pick_resp = interact(ui, mark.intersect(clip), "side-pick", Sense::click());
        let hot = pick_resp.hovered();
        if hot {
            st.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        txt(
            &sp,
            "quickRS",
            pos2(mark.max.x + px(12.0), centery(mark) - px(9.0)),
            17.0,
            th.text,
            800,
            Align2::LEFT_CENTER,
            None,
        );
        let sub = if hot {
            if theme::is_dark() {
                "switch to light"
            } else {
                "switch to dark"
            }
        } else {
            "Bristol 2026"
        };
        txt(
            &sp,
            sub,
            pos2(mark.max.x + px(12.0), centery(mark) + px(11.0)),
            12.0,
            if hot { th.accent_ink } else { th.caption },
            if hot { 700 } else { 500 },
            Align2::LEFT_CENTER,
            None,
        );
        y = mark.max.y + px(15.0);
        let mut go: Option<View> = None;
        let mut new_pl = false;
        let header = |y: &mut f32, label: &str| {
            *y += px(10.0);
            caps(&sp, label, pos2(x0 + px(14.0), *y), 11.0, Align2::LEFT_TOP, None);
            *y += px(20.0);
        };
        // h: the row's height (scale 1); the next row starts int((h + 2) * s) below it
        let mut item = |y: &mut f32,
                        label: &str,
                        view: Option<View>,
                        count: Option<usize>,
                        icon: Option<Icon>,
                        h: f32| {
            let r = Rect::from_min_size(pos2(x0, *y), vec2(wd, px(h)));
            let active = view.as_ref() == Some(&self.view);
            let resp = interact(
                ui,
                r.intersect(clip),
                ("side", label, view.is_none()),
                Sense::click(),
            );
            let hot = resp.hovered();
            if active {
                paint::raised(&sp, r, Some(14.0), 3.0, Fill::Base, 0.0);
            } else if hot {
                paint::raised(&sp, r, Some(12.0), 2.0, Fill::Base, 0.0);
            }
            if hot {
                st.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let col = if active {
                th.accent_ink
            } else if hot {
                th.text
            } else {
                th.text_2
            };
            let rcy = centery(r);
            let mut x = r.min.x + px(14.0);
            match icon {
                Some(Icon::Star) => {
                    draw_star(&sp, pos2(x + px(7.0), rcy), 7.0, true);
                    x += px(22.0);
                }
                Some(ic) => {
                    let icol = if ic == Icon::Plus { th.accent_ink } else { col };
                    icons::icon(&sp, ic, pos2(x + px(8.0), rcy), 16.0, icol);
                    x += px(24.0);
                }
                None => {}
            }
            txt(
                &sp,
                label,
                pos2(x, rcy),
                14.0,
                col,
                if active { 800 } else { 600 },
                Align2::LEFT_CENTER,
                Some(r.max.x - px(56.0) - x),
            );
            if let Some(n) = count {
                txt(
                    &sp,
                    &fmt_int(n as i64),
                    pos2(r.max.x - px(14.0), rcy),
                    12.0,
                    th.caption,
                    700,
                    Align2::RIGHT_CENTER,
                    None,
                );
            }
            if resp.clicked() {
                match view {
                    Some(v) => go = Some(v),
                    None => new_pl = true,
                }
            }
            *y += px(h + 2.0);
        };
        item(&mut y, "Home", Some(View::Home), None, Some(Icon::Home), 44.0);
        y += px(6.0);
        header(&mut y, "LIBRARY");
        item(&mut y, "All songs", Some(View::All), Some(counts.all), None, 36.0);
        item(
            &mut y,
            "Favourites",
            Some(View::Fav),
            Some(counts.fav),
            Some(Icon::Star),
            36.0,
        );
        item(
            &mut y,
            "Recently played",
            Some(View::Recent),
            Some(counts.recent),
            None,
            36.0,
        );
        item(
            &mut y,
            "Played (has a score)",
            Some(View::Played),
            Some(counts.played),
            None,
            36.0,
        );
        item(
            &mut y,
            "Not played yet",
            Some(View::Unplayed),
            Some(counts.unplayed),
            None,
            36.0,
        );
        header(&mut y, "TUNING");
        for (tn, n) in tc.iter().take(6) {
            item(&mut y, tn, Some(View::Tuning(tn.clone())), Some(*n), None, 36.0);
        }
        let other: usize = tc.iter().skip(6).map(|t| t.1).sum();
        if other > 0 {
            item(
                &mut y,
                "Other tunings",
                Some(View::Tuning("Other".into())),
                Some(other),
                None,
                36.0,
            );
        }
        header(&mut y, "PLAYLISTS");
        for (name, n) in &counts.playlists {
            item(
                &mut y,
                name,
                Some(View::Playlist(name.clone())),
                Some(*n),
                None,
                36.0,
            );
        }
        item(&mut y, "New playlist", None, None, Some(Icon::Plus), 36.0);
        // (side_scroll and side_max in points; the Python app's are pixels, hence its 0.5 and 1 px margins)
        let content = y - top_y + px(12.0);
        self.side_max = snap(content - rect.height()).max(0.0);
        self.side_scroll = self.side_scroll.clamp(0.0, self.side_max);
        if self.side_max > 0.0 {
            paint::fade_edges(
                &p,
                clip,
                self.side_scroll > pixels(0.5),
                self.side_scroll < self.side_max - pixels(0.5),
                20.0,
                paint::FaceKind::Base { rect, sunk: true },
            );
            // it scrolls: show which way there's more
            if self.side_scroll < self.side_max - pixels(1.0) {
                icons::icon(
                    &p,
                    Icon::Down,
                    pos2(centerx(rect), rect.max.y - px(12.0)),
                    16.0,
                    th.caption,
                );
            }
            if self.side_scroll > pixels(1.0) {
                icons::icon(
                    &p,
                    Icon::Up,
                    pos2(centerx(rect), rect.min.y + px(12.0)),
                    16.0,
                    th.caption,
                );
            }
        }
        if pick_resp.clicked() {
            st.toggle_theme();
        }
        if let Some(v) = go {
            self.go(st, v);
        }
        if new_pl {
            if self.view == View::Home {
                self.view = View::All; // the name box is drawn by the library page
            }
            self.new_playlist(None, None);
        }
    }

    // -- dialogs ------------------------------------------------------------------------------------------------

    /// A full-window catch for clicks outside a dialog; true when one landed outside `panel`.
    fn modal(ui: &Ui, area: Rect, panel: Rect, id: &str) -> bool {
        let p = ui.painter();
        paint::dim(p, area, None);
        let resp = interact(ui, area, ("modal", id), Sense::click());
        resp.clicked()
            && resp
                .interact_pointer_pos()
                .is_some_and(|pos| !panel.contains(pos))
    }

    /// Pick a part: the cover and title, every part with its tuning, best score and plays; Play, favourite, add to
    /// a playlist, back.
    fn draw_picker(&mut self, ui: &Ui, st: &mut AppState, area: Rect) {
        let Some(g) = self.picker else { return };
        let th = t();
        let ctx = st.ctx.clone();
        let e = st.song(g).clone();
        let ids = st.ids(g);
        let rows = e.arrangements.clone();
        let s = scale();
        let (w, h) = (ipx(area.width()), ipx(area.height()));
        let (pad, rh, gap) = (px(28.0), px(62.0), px(6.0));
        let pw = trunc_px((560.0 * s).min(w - 40.0 * s));
        let n = rows.len() as f32;
        let list_h = 2.0 * px(8.0) + n * rh + (n - 1.0) * gap;
        let ph = pad + px(96.0) + px(18.0) + px(16.0) + px(14.0) + list_h + px(20.0) + px(52.0) + pad;
        let panel = Rect::from_min_size(
            area.min
                + vec2(
                    pixels(((w - ipx(pw)) / 2.0).floor() as f32),
                    pixels(ipx(px(16.0)).max(((h - ipx(ph)) / 2.0).floor()) as f32),
                ),
            vec2(pw, ph),
        );
        let outside = Self::modal(ui, area, panel, "picker");
        let p = ui.painter().clone();
        paint::floating(&p, panel, Some(28.0));
        let art_r = Rect::from_min_size(panel.min + vec2(pad, pad), Vec2::splat(px(96.0)));
        crate::covers::draw(&p, &st.covers, &e, ipx(art_r.width()), art_r, 18.0, true);
        let tx = art_r.max.x + px(18.0);
        let tw = panel.max.x - pad - tx;
        txt(
            &p,
            &e.display_title(),
            pos2(tx, centery(art_r) - px(4.0)),
            24.0,
            th.text,
            800,
            Align2::LEFT_BOTTOM,
            Some(tw),
        );
        let sub: Vec<String> = [e.artist.clone(), e.album.clone(), fmt_time(e.length)]
            .into_iter()
            .filter(|v| !v.is_empty())
            .collect();
        txt(
            &p,
            &sub.join("  ·  "),
            pos2(tx, centery(art_r) + px(2.0)),
            15.0,
            th.text_2,
            600,
            Align2::LEFT_TOP,
            Some(tw),
        );
        let mut y = art_r.max.y + px(18.0);
        caps(
            &p,
            "CHOOSE A PART",
            pos2(panel.min.x + pad, y),
            12.0,
            Align2::LEFT_TOP,
            None,
        );
        y += px(16.0) + px(14.0);
        let well = Rect::from_min_size(pos2(panel.min.x + pad, y), vec2(pw - 2.0 * pad, list_h));
        paint::well(&p, well, 20.0);
        let mut ry = well.min.y + px(8.0);
        let mut chose: Option<usize> = None;
        for (i, a) in rows.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(well.min.x + px(8.0), ry), vec2(well.width() - px(16.0), rh));
            let resp = interact(ui, rect, ("picker-row", i), Sense::click());
            let on = i == self.picker_sel;
            let hot = resp.hovered();
            if on {
                paint::raised(&p, rect, Some(14.0), 4.0, Fill::Base, 0.0);
            } else if hot {
                paint::raised(&p, rect, Some(14.0), 2.0, Fill::Base, 0.0);
            }
            if hot {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                chose = Some(i);
            }
            let mid = centery(rect) + px(1.0);
            keycap(
                &p,
                &(i + 1).to_string(),
                pos2(rect.min.x + px(14.0), centery(rect)),
                Align2::LEFT_CENTER,
                11.0,
                CapOn::Material,
            );
            let label = format!("{}{}", a.name, if a.bonus { "  (bonus)" } else { "" });
            txt(
                &p,
                &label,
                pos2(rect.min.x + px(52.0), mid),
                17.0,
                if on { th.accent_ink } else { th.text },
                800,
                Align2::LEFT_BOTTOM,
                Some(rect.width() * 0.5),
            );
            txt(
                &p,
                &a.tuning,
                pos2(rect.min.x + px(52.0), mid),
                13.0,
                th.caption,
                500,
                Align2::LEFT_TOP,
                Some(rect.width() * 0.5),
            );
            let best = st.userdata.best(&ids, Some(&a.name));
            let plays = st.userdata.part_plays(&ids, &a.name);
            let rx = rect.max.x - px(16.0);
            if let Some((score, acc, sp)) = best {
                let mut best_s = format!("best {}  ·  {}%", fmt_int(score), (acc * 100.0).round() as i64);
                if sp != 1.0 {
                    best_s += &format!(" at {}%", (sp * 100.0).round() as i64);
                }
                txt(
                    &p,
                    &best_s,
                    pos2(rx, mid),
                    14.0,
                    th.accent_ink,
                    700,
                    Align2::RIGHT_BOTTOM,
                    None,
                );
            } else {
                txt(
                    &p,
                    if plays > 0 {
                        "no score yet"
                    } else {
                        "not played yet"
                    },
                    pos2(rx, mid),
                    14.0,
                    th.caption,
                    700,
                    Align2::RIGHT_BOTTOM,
                    None,
                );
            }
            if plays > 0 {
                txt(
                    &p,
                    &format!("played {plays}×"),
                    pos2(rx, mid),
                    12.0,
                    th.caption,
                    500,
                    Align2::RIGHT_TOP,
                    None,
                );
            }
            ry += rh + gap;
        }
        let fav = st.userdata.is_favorite(&ids);
        let by = well.max.y + px(20.0);
        let bh = px(52.0);
        let others: [(&str, &str, &str, Option<Icon>); 3] = [
            ("", "F", "fav", Some(if fav { Icon::Star } else { Icon::StarOff })),
            ("Playlist", "P", "addto", None),
            ("Back", "Esc", "back", None),
        ];
        let ow: Vec<f32> = others
            .iter()
            .map(|o| button_width(&ctx, o.0, o.1, 14.0, o.3.is_some()))
            .collect();
        let play_w = pw - 2.0 * pad - ow.iter().sum::<f32>() - px(12.0) * others.len() as f32;
        let play = Rect::from_min_size(pos2(panel.min.x + pad, by), vec2(play_w, bh));
        let part_name = rows
            .get(self.picker_sel)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let mut act: Option<&str> = None;
        if button(
            ui,
            play,
            "picker-play",
            &format!("Play {part_name}"),
            "Enter",
            ButtonStyle {
                primary: true,
                icon: Some(Icon::Play),
                size: 16.0,
                r: Some(16.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            act = Some("play");
        }
        let mut x = play.max.x + px(12.0);
        for (o, wd) in others.iter().zip(&ow) {
            let r = Rect::from_min_size(pos2(x, by), vec2(*wd, bh));
            let stl = ButtonStyle {
                active: o.2 == "fav" && fav,
                icon: o.3,
                size: 14.0,
                r: Some(16.0),
                ..Default::default()
            };
            if button(ui, r, ("picker-btn", o.2), o.0, o.1, stl).clicked() {
                act = Some(o.2);
            }
            x += wd + px(12.0);
        }
        match act {
            Some("play") => self.choose(st, self.picker_sel),
            Some("fav") => self.toggle_favorite(st, Some(g)),
            Some("addto") => self.menu = Some(g),
            Some(_) => self.picker = None,
            None => {
                if let Some(i) = chose {
                    self.choose(st, i);
                } else if outside {
                    self.picker = None;
                }
            }
        }
    }

    /// Add to playlist: every playlist with a tick where the song already is (1-9 toggle), and a new one.
    fn draw_menu(&mut self, ui: &Ui, st: &mut AppState, area: Rect) {
        let Some(g) = self.menu else { return };
        let th = t();
        let ctx = st.ctx.clone();
        let ids = st.ids(g);
        let names: Vec<String> = st.userdata.playlists.iter().map(|p| p.0.clone()).collect();
        let s = scale();
        let (w, h) = (ipx(area.width()), ipx(area.height()));
        let (pad, rh, gap) = (px(22.0), px(44.0), px(4.0));
        let n_rows = (names.len() + 1) as f32;
        let list_h = 2.0 * px(8.0) + n_rows * rh + (n_rows - 1.0) * gap;
        let pw = trunc_px((420.0 * s).min(w - 40.0 * s));
        let ph = pad + px(26.0) + px(12.0) + px(18.0) + px(12.0) + list_h + px(16.0) + px(48.0) + pad;
        let panel = Rect::from_min_size(
            area.min
                + vec2(
                    pixels(((w - ipx(pw)) / 2.0).floor() as f32),
                    pixels(ipx(px(16.0)).max(((h - ipx(ph)) / 2.0).floor()) as f32),
                ),
            vec2(pw, ph),
        );
        let outside = Self::modal(ui, area, panel, "menu");
        let p = ui.painter().clone();
        paint::floating(&p, panel, Some(26.0));
        txt(
            &p,
            "Add to playlist",
            panel.min + vec2(pad, pad),
            18.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            None,
        );
        txt(
            &p,
            &st.song(g).display_title(),
            panel.min + vec2(pad, pad + px(34.0)),
            13.0,
            th.caption,
            500,
            Align2::LEFT_TOP,
            Some(pw - 2.0 * pad),
        );
        let well = Rect::from_min_size(
            panel.min + vec2(pad, pad + px(68.0)),
            vec2(pw - 2.0 * pad, list_h),
        );
        paint::well(&p, well, 18.0);
        let mut y = well.min.y + px(8.0);
        let mut toggle: Option<String> = None;
        for (i, name) in names.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(well.min.x + px(8.0), y), vec2(well.width() - px(16.0), rh));
            let resp = interact(ui, rect, ("menu-row", i), Sense::click());
            if resp.hovered() {
                paint::raised(&p, rect, Some(12.0), 3.0, Fill::Base, 0.0);
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                toggle = Some(name.clone());
            }
            let inside = st.userdata.in_playlist(name, &ids);
            let rcy = centery(rect);
            let mut x = rect.min.x + px(14.0);
            if i < 9 {
                x = keycap(
                    &p,
                    &(i + 1).to_string(),
                    pos2(x, rcy),
                    Align2::LEFT_CENTER,
                    11.0,
                    CapOn::Material,
                )
                .max
                .x + px(12.0);
            }
            txt(
                &p,
                name,
                pos2(x, rcy),
                14.0,
                th.text,
                700,
                Align2::LEFT_CENTER,
                Some(rect.max.x - x - px(60.0)),
            );
            if inside {
                icons::icon(
                    &p,
                    Icon::Check,
                    pos2(rect.max.x - px(24.0), rcy),
                    18.0,
                    th.accent_ink,
                );
            } else {
                txt(
                    &p,
                    "add",
                    pos2(rect.max.x - px(14.0), rcy),
                    14.0,
                    th.caption,
                    700,
                    Align2::RIGHT_CENTER,
                    None,
                );
            }
            y += rh + gap;
        }
        let new = Rect::from_min_size(pos2(well.min.x + px(8.0), y), vec2(well.width() - px(16.0), rh));
        let new_resp = interact(ui, new, "menu-new", Sense::click());
        if new_resp.hovered() {
            paint::raised(&p, new, Some(12.0), 3.0, Fill::Base, 0.0);
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let ncy = centery(new);
        icons::icon(
            &p,
            Icon::Plus,
            pos2(new.min.x + px(22.0), ncy),
            16.0,
            th.accent_ink,
        );
        let tr = txt(
            &p,
            "New playlist…",
            pos2(new.min.x + px(38.0), ncy),
            14.0,
            th.text,
            700,
            Align2::LEFT_CENTER,
            None,
        );
        keycap(
            &p,
            "N",
            pos2(tr.max.x + px(10.0), ncy),
            Align2::LEFT_CENTER,
            11.0,
            CapOn::Material,
        );
        let dw = button_width(&ctx, "Done", "Esc", 14.0, false);
        let done = Rect::from_min_size(
            pos2(panel.max.x - pad - dw, well.max.y + px(16.0)),
            vec2(dw, px(48.0)),
        );
        let done_click = button(
            ui,
            done,
            "menu-done",
            "Done",
            "Esc",
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        )
        .clicked();
        if let Some(name) = toggle {
            st.userdata.toggle_in_playlist(&name, &ids);
        } else if new_resp.clicked() {
            self.menu = None;
            self.new_playlist(Some(g), None);
        } else if done_click || outside {
            self.menu = None;
        }
    }

    /// A name box: the text in a sunken field ringed in the accent; Cancel / Create.
    fn draw_text_box(&mut self, ui: &Ui, st: &mut AppState, area: Rect) {
        let th = t();
        let ctx = st.ctx.clone();
        let s = scale();
        let (w, h) = (ipx(area.width()), ipx(area.height()));
        let pad = px(24.0);
        let pw = trunc_px((520.0 * s).min(w - 40.0 * s));
        let ph = pad + px(28.0) + px(16.0) + px(52.0) + px(16.0) + px(48.0) + pad;
        let panel = Rect::from_min_size(
            area.min
                + vec2(
                    pixels(((w - ipx(pw)) / 2.0).floor() as f32),
                    pixels(((h - ipx(ph)) / 2.0).floor() as f32),
                ),
            vec2(pw, ph),
        );
        let _ = Self::modal(ui, area, panel, "text");
        let p = ui.painter().clone();
        paint::floating(&p, panel, Some(26.0));
        let Some(tb) = &self.text_box else { return };
        txt(
            &p,
            &tb.title,
            panel.min + vec2(pad, pad),
            20.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            None,
        );
        let boxr = Rect::from_min_size(
            panel.min + vec2(pad, pad + px(44.0)),
            vec2(pw - 2.0 * pad, px(52.0)),
        );
        paint::inset(&p, boxr, Some(16.0), 3.0, Fill::Base);
        paint::ring(&p, boxr, Some(16.0), 2.0, th.accent, true);
        let caret = if (ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0 {
            "|"
        } else {
            ""
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        txt(
            &p,
            &format!("{}{caret}", tb.text),
            pos2(boxr.min.x + px(18.0), centery(boxr)),
            17.0,
            th.text,
            600,
            Align2::LEFT_CENTER,
            Some(boxr.width() - px(36.0)),
        );
        let verb = tb.verb.clone();
        let ow = button_width(&ctx, &verb, "Enter", 14.0, false);
        let cw = button_width(&ctx, "Cancel", "Esc", 14.0, false);
        let by = boxr.max.y + px(16.0);
        let ok = Rect::from_min_size(pos2(panel.max.x - pad - ow, by), vec2(ow, px(48.0)));
        let cancel = Rect::from_min_size(pos2(ok.min.x - px(12.0) - cw, by), vec2(cw, px(48.0)));
        let c = button(
            ui,
            cancel,
            "text-cancel",
            "Cancel",
            "Esc",
            ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        )
        .clicked();
        let o = button(
            ui,
            ok,
            "text-ok",
            &verb,
            "Enter",
            ButtonStyle {
                primary: true,
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        )
        .clicked();
        if c {
            self.text_box = None;
        } else if o && let Some(tb) = self.text_box.take() {
            self.text_ok(st, tb);
        }
    }
}

fn static_str(s: &str) -> &'static str {
    match s {
        "fav" => "fav",
        "addto" => "addto",
        "remove" => "remove",
        "rename_pl" => "rename_pl",
        "delete_pl" => "delete_pl",
        "select_preview" => "select_preview",
        "rescan" => "rescan",
        "fullscreen" => "fullscreen",
        "settings" => "settings",
        _ => "",
    }
}

/// A wheel movement in notches of a mouse wheel (trackpads send points, some mice pages).
pub fn wheel_notches(unit: egui::MouseWheelUnit, dy: f32) -> f32 {
    match unit {
        egui::MouseWheelUnit::Line => dy,
        egui::MouseWheelUnit::Point => dy / 40.0,
        egui::MouseWheelUnit::Page => dy * 10.0,
    }
}

pub fn digit(k: Key) -> Option<usize> {
    Some(match k {
        Key::Num0 => 0,
        Key::Num1 => 1,
        Key::Num2 => 2,
        Key::Num3 => 3,
        Key::Num4 => 4,
        Key::Num5 => 5,
        Key::Num6 => 6,
        Key::Num7 => 7,
        Key::Num8 => 8,
        Key::Num9 => 9,
        _ => return None,
    })
}

/// A sunken search field with a magnifier (and the keys that open it at the right); a 2 px accent ring while
/// there's a search.
pub fn draw_search(p: &Painter, boxr: Rect, query: &str, hint: &str, keys: &[&str]) {
    let th = t();
    paint::inset(p, boxr, None, 6.0, Fill::Base);
    if !query.is_empty() {
        paint::ring(p, boxr, None, 2.0, th.accent, true);
    }
    let bcy = centery(boxr);
    icons::icon(
        p,
        Icon::Search,
        pos2(boxr.min.x + px(29.0), bcy),
        18.0,
        th.caption,
    );
    let x = boxr.min.x + px(48.0);
    let mut right = boxr.max.x - px(18.0);
    if !keys.is_empty() && query.is_empty() {
        for k in keys.iter().rev() {
            right = keycap(
                p,
                k,
                pos2(right, bcy),
                Align2::RIGHT_CENTER,
                11.0,
                CapOn::Material,
            )
            .min
            .x - px(3.0);
        }
    }
    let room = right - px(8.0) - x;
    if !query.is_empty() {
        let caret = if (p.ctx().input(|i| i.time) * 2.0) as i64 % 2 == 0 {
            "|"
        } else {
            ""
        };
        p.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
        txt(
            p,
            &format!("{query}{caret}"),
            pos2(x, bcy),
            15.0,
            th.text,
            600,
            Align2::LEFT_CENTER,
            Some(room),
        );
    } else {
        txt(
            p,
            hint,
            pos2(x, bcy),
            15.0,
            th.caption,
            600,
            Align2::LEFT_CENTER,
            Some(room),
        );
    }
}

/// The screen's colour from clear to solid, top to bottom, over `rect` (whole pixels): rows slide out under it. Each
/// pixel row has its own alpha, `(k / (h - 1)) ** 1.6`, as the Python app's fade surface.
fn draw_fade(p: &Painter, rect: Rect, color: Color32) {
    let h = ipx(rect.height()).max(0.0) as usize;
    if h == 0 || rect.width() <= 0.0 {
        return;
    }
    let one = pixels(1.0);
    let mut mesh = egui::Mesh::default();
    for k in 0..h {
        let f = if h > 1 {
            (k as f64 / (h - 1) as f64) as f32
        } else {
            0.0
        };
        let a = (f.powf(1.6) * 255.0) as u8;
        let y = rect.min.y + k as f32 * one;
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.max.x, y + one)),
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a),
        );
    }
    p.add(egui::Shape::mesh(mesh));
}

/// The pick: gold in the dark themes, graphite in Daylight.
fn draw_pick(p: &Painter, st: &AppState, rect: Rect) {
    let dark = theme::is_dark();
    let name = if dark { "pick-gold" } else { "pick-graphite" };
    let tex = crate::picks::pick(&st.ctx, name);
    match tex {
        Some(t) => paint::image(p, t, rect, Color32::WHITE),
        None => {
            let c = pos2(centerx(rect), centery(rect));
            icons::icon(p, Icon::Music, c, 20.0, t().accent_ink);
        }
    }
}
