//! The Catalogue (P37.13, D127; owner: "a catalog of all my equipment... all the pedals, all the cabinets"): one
//! full page of everything the room has, our models and your NAM captures and IRs, grouped (Amps, Bass amps, Cabs,
//! Drive, Fuzz, Dynamics, EQ, Filter, Modulation, Pitch, Delay, Reverb, Captures, IRs), searched as you type, filtered
//! (Models | Captures | Both, All | Guitar | Bass). Each card: the unit's face, its name, what it's inspired by (the
//! brand only in the `brands` build, D42/D58), its character; Add to rig, Replace selected, Info. Arrows move, Enter
//! adds, Shift+Enter replaces, Ctrl+I opens its info card, Esc goes back. The info card (`info_card`) is the "i" dot's
//! everywhere in the room.

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Event, Key, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use qrs_audio::board::{Block, Kind, MODELS, catalog};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::RoomView;
use super::draw::{self, KeyLook};
use super::rig::{ShelfMode, is_bass_model};
use super::stage::Entry;
use crate::pedalfaces as faces;
use crate::player::gearinfo;
use crate::state::AppState;

/// Which instruments the page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inst {
    All,
    Guitar,
    Bass,
}

/// The page's own state.
pub struct Cat {
    pub query: String,
    pub mode: ShelfMode,
    pub inst: Inst,
    /// The group shown (None: every group, one after another).
    pub group: Option<usize>,
    /// The selected card (an index into what's shown).
    pub sel: usize,
    /// How far the cards are scrolled (points).
    pub scroll: f32,
    /// Where the cards were last laid out: (entry, rect), for the keys and the wheel.
    pub(super) cards: Vec<(Entry, Rect)>,
    cols: usize,
    grid: Rect,
    content_h: f32,
}

impl Cat {
    pub fn new(bass: bool) -> Cat {
        Cat {
            query: String::new(),
            mode: ShelfMode::Both,
            inst: if bass { Inst::Bass } else { Inst::All },
            group: None,
            sel: 0,
            scroll: 0.0,
            cards: Vec::new(),
            cols: 1,
            grid: Rect::NOTHING,
            content_h: 0.0,
        }
    }
}

/// The groups, by index (the order they're shown in is `ORDER`; P37.6c added the bass cabs and bass drives, D141).
pub const GROUPS: [&str; 16] = [
    "Amps",
    "Bass amps",
    "Cabs",
    "Drive",
    "Fuzz",
    "Dynamics",
    "EQ",
    "Filter",
    "Modulation",
    "Pitch",
    "Delay",
    "Reverb",
    "NAM captures",
    "Bass cabs",
    "Bass drives",
    "IRs",
];
/// The capture groups' indices.
const G_NAM: usize = 12;
const G_IR: usize = 15;
/// The order the groups are listed in: each bass group after its guitar one.
pub const ORDER: [usize; 16] = [0, 1, 2, 13, 3, 14, 4, 5, 6, 7, 8, 9, 10, 11, 12, 15];

/// Everything the catalogue holds, as its "Everything" counts it with no filter (a gain ladder one card): the Sounds
/// tab says the same.
pub fn gear_total() -> usize {
    total_of(&entries(&Cat::new(false)))
}

/// A pitch unit (octaver, pitch shifter): its own group, whatever its kind.
fn is_pitch(id: &str) -> bool {
    id.contains("octav") || id.contains("pitch") || id.contains("whammy") || id.contains("harmon")
}

/// The group a model is in.
fn model_group(m: usize) -> usize {
    let d = &MODELS[m];
    if is_pitch(d.id) {
        return 9;
    }
    match d.family() {
        Kind::Amp if is_bass_model(d.id) => 1,
        Kind::Amp => 0,
        Kind::Cab if is_bass_model(d.id) => 13,
        Kind::Cab => 2,
        Kind::Drive | Kind::Fuzz if is_bass_model(d.id) => 14,
        Kind::Drive => 3,
        Kind::Fuzz => 4,
        Kind::Dynamics => 5,
        Kind::Eq => 6,
        Kind::Filter => 7,
        Kind::Modulation => 8,
        Kind::Delay => 10,
        Kind::Reverb => 11,
    }
}

/// Everything, grouped: (group, its entries) for the groups with any, filtered by the page's search and filters.
pub fn entries(c: &Cat) -> Vec<(usize, Vec<Entry>)> {
    let q = c.query.trim().to_lowercase();
    let mut groups: Vec<Vec<Entry>> = vec![Vec::new(); GROUPS.len()];
    if c.mode != ShelfMode::Captures {
        for (m, d) in MODELS.iter().enumerate() {
            if d.file {
                continue;
            }
            let bass = is_bass_model(d.id);
            // (an amp or cab is for its instrument; effects work on both)
            let for_both = !matches!(d.kind, Kind::Amp | Kind::Cab);
            let ok = match c.inst {
                Inst::All => true,
                Inst::Guitar => !bass,
                Inst::Bass => bass || for_both,
            };
            if !ok {
                continue;
            }
            if !q.is_empty() && !model_matches(d, &q) {
                continue;
            }
            groups[model_group(m)].push(Entry::Model(m));
        }
    }
    if c.mode != ShelfMode::Models && crate::captures::ON {
        crate::captures::with(|l| {
            let mut v: Vec<&crate::captures::Item> = l.items.iter().collect();
            v.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then(a.gain.unwrap_or(0.0).total_cmp(&b.gain.unwrap_or(0.0)))
            });
            // (P37.6c: a unit's gain ladder is one card, its lowest step standing for it; the card lists the steps)
            let mut ladders_seen: Vec<(u64, u8)> = Vec::new();
            for i in v {
                if let Some(key) = ladder_key(i)
                    && ladder_steps(l, i).len() > 1
                {
                    if ladders_seen.contains(&key) {
                        continue;
                    }
                    ladders_seen.push(key);
                    // (the card is the ladder's lowest step)
                    let first = ladder_steps(l, i)[0];
                    if first != i.id {
                        if let Some(f) = l.get(first) {
                            let ok = match c.inst {
                                Inst::All => true,
                                Inst::Guitar => !f.bass,
                                Inst::Bass => f.bass,
                            };
                            let hay =
                                format!("{} {}", f.name, f.gear.as_deref().unwrap_or("")).to_lowercase();
                            if ok && (q.is_empty() || q.split_whitespace().all(|w| hay.contains(w))) {
                                groups[if f.kind == crate::captures::Kind::Ir {
                                    G_IR
                                } else {
                                    G_NAM
                                }]
                                .push(Entry::Capture(f.id));
                            }
                        }
                        continue;
                    }
                }
                let ok = match c.inst {
                    Inst::All => true,
                    Inst::Guitar => !i.bass,
                    Inst::Bass => i.bass,
                };
                let hay = format!("{} {}", i.name, i.gear.as_deref().unwrap_or("")).to_lowercase();
                if !ok || (!q.is_empty() && !q.split_whitespace().all(|w| hay.contains(w))) {
                    continue;
                }
                let g = if i.kind == crate::captures::Kind::Ir {
                    G_IR
                } else {
                    G_NAM
                };
                groups[g].push(Entry::Capture(i.id));
            }
        });
        // (the empty file units, to load your own .nam or .wav)
        if q.is_empty() {
            for (m, d) in catalog::files() {
                groups[if d.kind == Kind::Cab { G_IR } else { G_NAM }].push(Entry::Model(m));
            }
        }
    }
    let mut groups: Vec<Option<Vec<Entry>>> = groups.into_iter().map(Some).collect();
    let mut out: Vec<(usize, Vec<Entry>)> = ORDER
        .iter()
        .filter_map(|&g| groups[g].take().map(|v| (g, v)))
        .filter(|(g, v)| !v.is_empty() && c.group.is_none_or(|x| x == *g))
        .collect();
    // (a search naming a family puts that group first, P41.3: "fuzz" shows the fuzzes before a drive that says
    // "fuzz" in its words)
    if let Some(g) = wanted_group(&q) {
        out.sort_by_key(|(x, _)| *x != g);
    }
    out
}

/// What a model is searched by (P41.3, D172): its name, family, brand (the `brands` build), what it's inspired by,
/// its sound, the players known for it and the records it's heard on ("jack white", "gilmour", "seven nation army"
/// find the Big Muff), every word of the search somewhere in it.
pub fn model_matches(d: &qrs_audio::board::ModelDef, q: &str) -> bool {
    let info = gearinfo::info(d.id);
    let about = gearinfo::about(d.id);
    let heard = about
        .map(|a| {
            a.heard
                .iter()
                .map(|h| format!("{} {}", h.song, h.artist))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let hay = format!(
        "{} {} {} {} {} {} {}",
        d.name,
        d.family().label(),
        if catalog::SHOW_BRANDS {
            catalog::brand(d.id)
        } else {
            ""
        },
        info.map_or("", |i| i.inspired_by),
        info.map_or("", |i| i.character),
        info.map_or(String::new(), |i| i.players.join(" ")),
        heard,
    )
    .to_lowercase();
    let flat = |s: &str| s.replace(['\'', '"', '.', ','], "");
    let hay = flat(&hay);
    q.split_whitespace().all(|w| hay.contains(&flat(w)))
}

/// The group a search names ("fuzz", "delay", "cab"...), to show first.
pub fn wanted_group(q: &str) -> Option<usize> {
    q.split_whitespace().find_map(|w| {
        Some(match w.trim_end_matches('s') {
            "amp" | "head" => 0,
            "cab" | "cabinet" | "speaker" => 2,
            "drive" | "overdrive" | "distortion" | "dist" | "boost" => 3,
            "fuzz" | "fuzze" => 4,
            "comp" | "compressor" | "gate" | "dynamic" => 5,
            "eq" => 6,
            "wah" | "filter" => 7,
            "chorus" | "phaser" | "flanger" | "tremolo" | "trem" | "vibe" | "modulation" => 8,
            "octave" | "pitch" => 9,
            "delay" | "echo" => 10,
            "reverb" | "verb" => 11,
            "capture" | "nam" => G_NAM,
            "ir" => G_IR,
            _ => return None,
        })
    })
}

/// A capture's card line in plain words (P41.3): what it is, whether it needs a cab, who made it.
fn capture_line(i: &crate::captures::Item) -> String {
    let what = match i.kind {
        crate::captures::Kind::Nam => match i.cab {
            Some(false) => "A real amp, captured: it plays into a cab",
            Some(true) => "A real amp and its cab, captured",
            None => "A real amp, captured",
        },
        crate::captures::Kind::Pedal => "A real pedal, captured",
        crate::captures::Kind::Ir => "A real speaker box and mic, recorded",
    };
    match &i.creator {
        Some(c) => format!("{what} \u{b7} by {c}"),
        None => what.to_string(),
    }
}

/// A capture's gain ladder: (its TONE3000 tone, its kind), when it has a gain.
fn ladder_key(i: &crate::captures::Item) -> Option<(u64, u8)> {
    let k = match i.kind {
        crate::captures::Kind::Nam => 0,
        crate::captures::Kind::Pedal => 1,
        crate::captures::Kind::Ir => return None,
    };
    i.gain?;
    Some((i.tone_id?, k))
}

/// The captures of `i`'s gain ladder, by gain (just `i` when it has none).
fn ladder_steps(l: &crate::captures::Library, i: &crate::captures::Item) -> Vec<u32> {
    let Some(key) = ladder_key(i) else {
        return vec![i.id];
    };
    let mut v: Vec<&crate::captures::Item> = l.items.iter().filter(|x| ladder_key(x) == Some(key)).collect();
    v.sort_by(|a, b| {
        a.gain
            .unwrap_or(0.0)
            .total_cmp(&b.gain.unwrap_or(0.0))
            .then(a.id.cmp(&b.id))
    });
    v.into_iter().map(|x| x.id).collect()
}

/// A capture's name split at its setting: ("2015 Fender Hot Rod Deluxe III", "edge Funky Clean").
fn name_parts(n: &str) -> (&str, &str) {
    for sep in [" \u{b7} ", " - ", " | "] {
        if let Some(at) = n.find(sep) {
            return (n[..at].trim(), n[at + sep.len()..].trim());
        }
    }
    (n, "")
}

/// An entry's name, its "inspired by" line and its character line.
pub fn words(e: Entry) -> (String, String, String) {
    match e {
        Entry::Model(m) => {
            let d = &MODELS[m];
            let info = gearinfo::info(d.id);
            let insp = info.map(|i| i.inspired_line()).unwrap_or_else(|| {
                if d.file {
                    "load your own file into it".into()
                } else {
                    format!("after {}", d.after)
                }
            });
            let ch = info
                .map(|i| i.character.to_string())
                .unwrap_or_else(|| d.blurb.to_string());
            (d.name.to_string(), insp, ch)
        }
        Entry::Capture(id) => crate::captures::with(|l| {
            l.get(id)
                .map(|i| {
                    let like = i
                        .skin
                        .as_deref()
                        .and_then(catalog::find)
                        .map(|s| format!("sounds like our {}", MODELS[s].name))
                        .unwrap_or_else(|| "a capture of real gear".into());
                    // (the full name, P37.6c: the card wraps it on two lines; a gain ladder is named once and its
                    // steps listed)
                    let steps = ladder_steps(l, i);
                    if steps.len() > 1 {
                        // (P41.3: each step in plain words, "clean · edge of breakup · crunch"; the settings' codes
                        // are in its info card)
                        let mut words: Vec<String> = Vec::new();
                        for st in steps.iter().filter_map(|s| l.get(*s)) {
                            let (_, set) = name_parts(&st.name);
                            let w = match (super::rig::setting_words(set), st.gain) {
                                ("", Some(g)) => format!("gain {g:.1}"),
                                ("", None) => continue,
                                (w, _) => w.to_string(),
                            };
                            if !words.contains(&w) {
                                words.push(w);
                            }
                        }
                        (
                            super::rig::short_capture_name(name_parts(&i.name).0),
                            like,
                            format!(
                                "{} gain settings on its Gain knob: {}",
                                steps.len(),
                                words.join(" \u{b7} ")
                            ),
                        )
                    } else {
                        // (P41.3: the short name and a plain line; the full name and the file's facts are in Info)
                        (super::rig::short_capture_name(&i.name), like, capture_line(i))
                    }
                })
                .unwrap_or_default()
        }),
    }
}

/// The small "i" dot (shown where a unit is hovered): a click opens its info card.
pub fn info_dot(ui: &Ui, c: Pos2, id: impl egui::AsId) -> egui::Response {
    let th = t();
    let r = Rect::from_center_size(c, vec2(px(18.0), px(18.0)));
    let resp = interact(ui, r.expand(px(2.0)), id, Sense::click());
    let p = ui.painter();
    let hot = resp.hovered();
    p.circle_filled(
        c,
        px(8.5),
        if hot {
            th.accent
        } else {
            Color32::from_black_alpha(170)
        },
    );
    p.circle_stroke(
        c,
        px(8.5),
        Stroke::new(1.0, Color32::from_white_alpha(if hot { 0 } else { 70 })),
    );
    text::draw(
        p,
        "i",
        c + vec2(0.0, px(0.5)),
        Align2::CENTER_CENTER,
        Style::new(11.0, 900, if hot { th.on_accent } else { Color32::WHITE }),
        None,
    );
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

/// The page, in the room's body.
pub fn page(room: &mut RoomView, ui: &Ui, st: &mut AppState, body: Rect) {
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    draw::card(&p, body, px(18.0));
    let Some(mut c) = room.cat.take() else { return };
    let groups = entries(&c);
    let total: usize = groups.iter().map(|g| g.1.len()).sum();
    // the header: the title, the search, the filters, back
    let hy = body.min.y + px(26.0);
    let x0 = body.min.x + px(22.0);
    text::caps(
        &p,
        &format!("CATALOGUE  \u{b7}  {total} UNITS"),
        pos2(x0, hy),
        11.0,
        th.caption,
        Align2::LEFT_CENTER,
        None,
    );
    let back_w = draw::key_w(&ctx, "Back to the rig", "Esc", false);
    let back = Rect::from_min_size(
        pos2(body.max.x - px(18.0) - back_w, hy - px(16.0)),
        vec2(back_w, px(32.0)),
    );
    let mut close = draw::key(
        ui,
        back,
        "cat-back",
        KeyLook::Dark,
        "Back to the rig",
        "Esc",
        None,
    )
    .clicked();
    let mut x = back.min.x - px(20.0);
    let chips = |ui: &Ui, labels: &[&str], on: usize, tag: &str, x: &mut f32| -> Option<usize> {
        let mut hit = None;
        for (k, l) in labels.iter().enumerate().rev() {
            let w = draw::chip_w(ui.ctx(), l, false, 11.0);
            let r = Rect::from_min_size(pos2(*x - w, hy - px(13.0)), vec2(w, px(26.0)));
            *x = r.min.x - px(5.0);
            if draw::chip(ui, r, (tag, k), l, None, k == on, 11.0).clicked() {
                hit = Some(k);
            }
        }
        *x -= px(14.0);
        hit
    };
    let inst_on = match c.inst {
        Inst::All => 0,
        Inst::Guitar => 1,
        Inst::Bass => 2,
    };
    if let Some(k) = chips(ui, &["All", "Guitar", "Bass"], inst_on, "cat-inst", &mut x) {
        c.inst = [Inst::All, Inst::Guitar, Inst::Bass][k];
        c.sel = 0;
        c.scroll = 0.0;
    }
    let mode_on = match c.mode {
        ShelfMode::Models => 0,
        ShelfMode::Captures => 1,
        ShelfMode::Both => 2,
    };
    if let Some(k) = chips(ui, &["Models", "Captures", "Both"], mode_on, "cat-mode", &mut x) {
        c.mode = [ShelfMode::Models, ShelfMode::Captures, ShelfMode::Both][k];
        c.sel = 0;
        c.scroll = 0.0;
    }
    // the search: always listening (type to find)
    let sx0 =
        x0 + text::width(
            &ctx,
            &format!("CATALOGUE  \u{b7}  {total} UNITS"),
            Style::caps(11.0, th.caption),
        ) + px(24.0);
    let sr = Rect::from_min_max(
        pos2(sx0, hy - px(15.0)),
        pos2(x.max(sx0 + px(120.0)), hy + px(15.0)),
    );
    paint::inset(&p, sr, Some(px(15.0)), 1.5, paint::Fill::Base);
    draw::icon_search(&p, pos2(sr.min.x + px(16.0), hy), px(14.0), th.caption);
    let (shown, ink) = if c.query.is_empty() {
        (
            "Type to find any unit: a name, a brand, a sound (\u{201c}fuzz\u{201d}, \u{201c}bass\u{201d})"
                .to_string(),
            th.caption,
        )
    } else {
        (format!("{}\u{2581}", c.query), th.text)
    };
    text::draw(
        &p,
        &shown,
        pos2(sr.min.x + px(32.0), hy),
        Align2::LEFT_CENTER,
        Style::new(12.0, 700, ink),
        Some(sr.width() - px(44.0)),
    );
    // the groups, down the left
    let side = Rect::from_min_max(
        pos2(x0, body.min.y + px(52.0)),
        pos2(x0 + px(170.0), body.max.y - px(40.0)),
    );
    let all_groups = {
        let mut probe = Cat::new(false);
        probe.query = c.query.clone();
        probe.mode = c.mode;
        probe.inst = c.inst;
        entries(&probe)
    };
    let mut y = side.min.y;
    let rows: Vec<(Option<usize>, String, usize)> =
        std::iter::once((None, "Everything".to_string(), total_of(&all_groups)))
            .chain(
                all_groups
                    .iter()
                    .map(|(g, v)| (Some(*g), GROUPS[*g].to_string(), v.len())),
            )
            .collect();
    let rh = ((side.height()) / rows.len().max(1) as f32).min(px(30.0));
    for (g, label, n) in rows {
        let rr = Rect::from_min_size(pos2(side.min.x, y), vec2(side.width(), rh - px(2.0)));
        let resp = interact(ui, rr, ("cat-group", g.unwrap_or(99)), Sense::click());
        let on = c.group == g;
        if on {
            p.rect_filled(rr, px(8.0), th.accent.gamma_multiply(0.16));
        } else if resp.hovered() {
            p.rect_filled(rr, px(8.0), th.screen_hi);
        }
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        let ink = if on { th.accent_ink } else { th.text_2 };
        text::draw(
            &p,
            &label,
            pos2(rr.min.x + px(10.0), rr.center().y),
            Align2::LEFT_CENTER,
            Style::new(12.5, 700, ink),
            Some(rr.width() - px(44.0)),
        );
        text::draw(
            &p,
            &n.to_string(),
            pos2(rr.max.x - px(10.0), rr.center().y),
            Align2::RIGHT_CENTER,
            Style::new(11.0, 700, th.caption),
            None,
        );
        if resp.clicked() {
            c.group = g;
            c.sel = 0;
            c.scroll = 0.0;
        }
        y += rh;
    }
    // the cards
    let grid = Rect::from_min_max(
        pos2(side.max.x + px(22.0), body.min.y + px(52.0)),
        pos2(body.max.x - px(18.0), body.max.y - px(40.0)),
    );
    let card = vec2(px(214.0), px(232.0));
    let gap = px(14.0);
    let cols = (((grid.width() + gap) / (card.x + gap)).floor() as usize).max(1);
    let cw = (grid.width() - gap * (cols - 1) as f32) / cols as f32;
    let clip = p.with_clip_rect(grid);
    let mut cards: Vec<(Entry, Rect)> = Vec::new();
    let mut yy = grid.min.y - c.scroll;
    let flat: Vec<Entry> = groups.iter().flat_map(|g| g.1.iter().copied()).collect();
    c.sel = c.sel.min(flat.len().saturating_sub(1));
    let mut idx = 0usize;
    let b = room.rack.board_now(st);
    let sel_unit = room
        .rack
        .board
        .sel
        .and_then(|u| b.index_of(u))
        .map(|i| b.blocks[i]);
    let mut act: Option<(Entry, &str)> = None;
    let info_none = room.info.is_none();
    for (g, list) in &groups {
        if c.group.is_none() {
            text::caps(
                &clip,
                &format!("{}  \u{b7}  {}", GROUPS[*g].to_uppercase(), list.len()),
                pos2(grid.min.x, yy + px(12.0)),
                10.5,
                th.caption,
                Align2::LEFT_CENTER,
                None,
            );
            yy += px(28.0);
        }
        for (j, &e) in list.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(
                    grid.min.x + (j % cols) as f32 * (cw + gap),
                    yy + (j / cols) as f32 * (card.y + gap),
                ),
                vec2(cw, card.y),
            );
            if r.max.y > grid.min.y && r.min.y < grid.max.y {
                let vis = r.intersect(grid);
                let resp = interact(ui, vis, ("cat-card", e.key()), Sense::click());
                let sel = idx == c.sel;
                if resp.clicked() {
                    c.sel = idx;
                }
                if resp.double_clicked() {
                    act = Some((e, "add"));
                }
                // (P38.1: hot while the pointer is anywhere on the card, its own keys included. egui's `hovered()`
                // goes false once a key on top has the pointer, so the keys came and went each frame (the card
                // "shivered") and a first click on Add / Replace / i landed on the card behind and only selected it)
                let hot = resp.hovered() || (info_none && ui.rect_contains_pointer(vis));
                if let Some(a) = card_ui(ui, &clip, r, vis, e, sel, hot, sel_unit.as_ref()) {
                    c.sel = idx;
                    act = Some((e, a));
                }
            }
            cards.push((e, r));
            idx += 1;
        }
        yy += list.len().div_ceil(cols) as f32 * (card.y + gap) + px(6.0);
    }
    if flat.is_empty() {
        text::draw(
            &p,
            "Nothing matches. Try fewer words, or Both and All above.",
            grid.center(),
            Align2::CENTER_CENTER,
            Style::new(13.0, 700, th.caption),
            None,
        );
    }
    c.content_h = yy + c.scroll - grid.min.y;
    c.cards = cards;
    c.cols = cols;
    c.grid = grid;
    // the foot: the keys
    text::draw(
        &p,
        "Arrows move  \u{b7}  Enter adds to the rig  \u{b7}  Shift+Enter replaces the open unit  \u{b7}  Ctrl+I its info  \u{b7}  type to search  \u{b7}  Esc back",
        pos2(x0, body.max.y - px(20.0)),
        Align2::LEFT_CENTER,
        Style::new(11.0, 600, th.caption),
        Some(body.width() - px(44.0)),
    );
    if close {
        room.cat = None;
        return;
    }
    room.cat = Some(c);
    if let Some((e, a)) = act {
        close = do_act(room, st, e, a);
        if close {
            room.cat = None;
        }
    }
}

fn total_of(g: &[(usize, Vec<Entry>)]) -> usize {
    g.iter().map(|x| x.1.len()).sum()
}

/// How many units the catalogue lists with no search or filter (the top bar's key says it, P41.5), counted again at
/// most every 2 s, so a capture that just came in shows up without counting every frame.
pub fn unit_count() -> usize {
    use std::cell::Cell;
    use std::time::{Duration, Instant};
    thread_local!(static LAST: Cell<Option<(Instant, usize)>> = const { Cell::new(None) });
    LAST.with(|last| match last.get() {
        Some((at, n)) if at.elapsed() < Duration::from_secs(2) => n,
        _ => {
            let n = total_of(&entries(&Cat::new(false)));
            last.set(Some((Instant::now(), n)));
            n
        }
    })
}

/// A card: the face, the name, inspired by, its character; on hover or selected its keys and its "i" dot. Returns
/// a key pressed ("add", "replace", "info").
#[allow(clippy::too_many_arguments)]
fn card_ui(
    ui: &Ui,
    p: &Painter,
    r: Rect,
    vis: Rect,
    e: Entry,
    sel: bool,
    hot: bool,
    open: Option<&Block>,
) -> Option<&'static str> {
    let th = t();
    if sel {
        paint::inset(p, r, Some(px(14.0)), 1.5, paint::Fill::Base);
        p.rect_stroke(
            r.shrink(1.0),
            px(14.0),
            Stroke::new(1.5, th.accent),
            egui::StrokeKind::Inside,
        );
    } else {
        paint::raised(p, r, Some(px(14.0)), 2.0, paint::Fill::Base, 0.0);
        if hot {
            p.rect_stroke(
                r.shrink(1.0),
                px(14.0),
                Stroke::new(1.0, th.accent.gamma_multiply(0.5)),
                egui::StrokeKind::Inside,
            );
        }
    }
    // the face in a quiet well
    let bed = Rect::from_min_size(
        r.min + vec2(px(10.0), px(10.0)),
        vec2(r.width() - px(20.0), px(112.0)),
    );
    paint::inset(p, bed, Some(px(9.0)), 1.5, paint::Fill::Base);
    let blk = e.block();
    // (P37.6c: a capture is drawn as the gear it stands for (its skin's face, with the NAM sticker), not one more
    // silver brick, under a strip naming its maker; captures with no skin keep the silver unit)
    let (art, maker) = match e {
        Entry::Capture(id) => crate::captures::with(|l| {
            l.get(id)
                .map(|i| {
                    let skin = i.skin.as_deref().and_then(Block::of).filter(|s| !s.def().file);
                    let maker = i
                        .gear
                        .as_deref()
                        .unwrap_or(&i.name)
                        .split_whitespace()
                        .find(|w| w.chars().any(|c| c.is_alphabetic()) && w.len() > 2)
                        .unwrap_or("")
                        .to_uppercase();
                    (skin, maker)
                })
                .unwrap_or((None, String::new()))
        }),
        Entry::Model(_) => (None, String::new()),
    };
    let shown = art.unwrap_or(blk);
    let strip_h = if maker.is_empty() { 0.0 } else { px(16.0) };
    let well = Rect::from_min_max(pos2(bed.min.x, bed.min.y + strip_h), bed.max);
    let fs = faces::board_size(&shown);
    let k = ((well.width() - px(16.0)) / fs.x).min((well.height() - px(14.0)) / fs.y);
    let fr = Rect::from_center_size(well.center(), fs * k);
    faces::miniature(p, fr, &shown, 0.0);
    if art.is_some() {
        faces::neural_sticker(p, fr);
    }
    if !maker.is_empty() {
        let sr = Rect::from_min_size(bed.min, vec2(bed.width(), strip_h));
        let h = maker
            .bytes()
            .fold(7u32, |a, c| a.wrapping_mul(31).wrapping_add(c as u32));
        let hue = (h % 360) as f32 / 360.0;
        let col: Color32 = egui::ecolor::Hsva::new(hue, 0.45, 0.42, 1.0).into();
        p.rect_filled(
            sr,
            egui::CornerRadius {
                nw: px(9.0) as u8,
                ne: px(9.0) as u8,
                sw: 0,
                se: 0,
            },
            col,
        );
        text::caps(
            p,
            &maker,
            pos2(sr.min.x + px(10.0), sr.center().y),
            9.0,
            Color32::from_rgb(0xf4, 0xf2, 0xec),
            Align2::LEFT_CENTER,
            Some(sr.width() - px(36.0)),
        );
    }
    let (name, insp, ch) = words(e);
    let tx = r.min.x + px(12.0);
    let tw = r.width() - px(24.0);
    // (the full name: two lines when it needs them, P37.6c)
    let ns = Style::new(
        if matches!(e, Entry::Capture(_)) {
            12.5
        } else {
            14.0
        },
        800,
        th.text,
    );
    let nl = text::wrap(ui.ctx(), &name, ns, tw);
    let two = nl.len() > 1;
    for (n, l) in nl.iter().take(2).enumerate() {
        text::draw(
            p,
            l,
            pos2(
                tx,
                bed.max.y + px(if two { 12.0 } else { 16.0 }) + px(15.0) * n as f32,
            ),
            Align2::LEFT_CENTER,
            ns,
            Some(tw),
        );
    }
    let dy = if two { px(12.0) } else { 0.0 };
    text::draw(
        p,
        &insp,
        pos2(tx, bed.max.y + px(35.0) + dy),
        Align2::LEFT_CENTER,
        Style::new(10.5, 700, th.accent_ink),
        Some(tw),
    );
    let mut out = None;
    if sel || hot {
        // its keys
        let ky = r.max.y - px(26.0);
        let mut kx = tx;
        let keys: [(&str, &'static str, bool); 3] = [
            ("Add", "add", true),
            ("Replace", "replace", open.is_some()),
            ("Info", "info", true),
        ];
        for (label, a, live) in keys {
            let w = draw::key_w(ui.ctx(), label, "", false) - px(6.0);
            let kr = Rect::from_min_size(pos2(kx, ky - px(13.0)), vec2(w, px(26.0)));
            kx = kr.max.x + px(6.0);
            if !vis.contains_rect(kr) {
                continue;
            }
            let resp = draw::key(ui, kr, ("cat-key", e.key(), a), KeyLook::Dark, label, "", None);
            if !live {
                paint::wash(p, kr, th.base, 0.55);
            } else if resp.clicked() {
                out = Some(a);
            }
        }
    } else {
        let lines = text::wrap(ui.ctx(), &ch, Style::new(10.5, 600, th.caption), tw);
        for (i, l) in lines.iter().take(if two { 2 } else { 3 }).enumerate() {
            text::draw(
                p,
                l,
                pos2(tx, bed.max.y + px(56.0) + dy + i as f32 * px(15.0)),
                Align2::LEFT_CENTER,
                Style::new(10.5, 600, th.caption),
                Some(tw),
            );
        }
    }
    // the "i" dot (the face carries its own NAM / IR stamp: no second one here, P37.6c)
    if (sel || hot) && vis.contains(pos2(bed.max.x - px(12.0), bed.min.y + px(12.0))) {
        let c = pos2(bed.max.x - px(12.0), bed.min.y + px(12.0));
        if info_dot(ui, c, ("cat-i", e.key())).clicked() {
            out = Some("info");
        }
    }
    out
}

/// Do a card's key: add it after the open unit, put it in the open unit's place, or open its info. Returns whether
/// the page closes (it does after an add or a replace, on the rig with the new unit open).
fn do_act(room: &mut RoomView, st: &mut AppState, e: Entry, a: &str) -> bool {
    let b = room.rack.board_now(st);
    match a {
        "info" => {
            room.info = Some(e);
            false
        }
        "replace" => {
            let Some(uid) = room.rack.board.sel else {
                return false;
            };
            room.rig_ui.change = Some(uid);
            super::rig::place_entry(room, st, e, &b);
            room.say("Replaced the open unit", "  \u{b7}  Ctrl Z undoes");
            true
        }
        _ => {
            room.rig_ui.change = None;
            super::rig::place_entry(room, st, e, &b);
            // (the new unit open)
            let now = room.rack.board_now(st);
            let at = room.rack.board.sel.and_then(|u| now.index_of(u)).map(|i| i + 1);
            if let Some(i) = at.filter(|&i| i < now.blocks.len()) {
                room.rack.board.sel = Some(now.blocks[i].uid);
            }
            let (name, _, _) = words(e);
            room.say(format!("Added {name}"), "  \u{b7}  Ctrl Z undoes");
            true
        }
    }
}

/// The page's keys and wheel.
pub fn handle(room: &mut RoomView, st: &mut AppState, ev: &Event) {
    let Some(c) = room.cat.as_mut() else { return };
    match ev {
        Event::Text(s) => {
            for ch in s.chars().filter(|ch| !ch.is_control()) {
                if c.query.chars().count() < 40 {
                    c.query.push(ch);
                }
            }
            c.sel = 0;
            c.scroll = 0.0;
        }
        Event::MouseWheel { unit, delta, .. } => {
            let n = crate::library::wheel_notches(*unit, delta.y);
            let max = (c.content_h - c.grid.height()).max(0.0);
            c.scroll = (c.scroll - n * px(60.0)).clamp(0.0, max);
        }
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            let n = c.cards.len();
            let cols = c.cols.max(1);
            match key {
                Key::Escape => {
                    if !c.query.is_empty() {
                        c.query.clear();
                    } else {
                        room.cat = None;
                    }
                    return;
                }
                Key::Backspace => {
                    c.query.pop();
                    c.sel = 0;
                }
                Key::ArrowRight => c.sel = (c.sel + 1).min(n.saturating_sub(1)),
                Key::ArrowLeft => c.sel = c.sel.saturating_sub(1),
                Key::ArrowDown => c.sel = (c.sel + cols).min(n.saturating_sub(1)),
                Key::ArrowUp => c.sel = c.sel.saturating_sub(cols),
                Key::Enter | Key::I => {
                    let Some(&(e, _)) = c.cards.get(c.sel) else { return };
                    let a = if *key == Key::I {
                        if !modifiers.command {
                            return;
                        }
                        "info"
                    } else if modifiers.shift {
                        "replace"
                    } else {
                        "add"
                    };
                    if do_act(room, st, e, a) {
                        room.cat = None;
                    }
                    return;
                }
                _ => {}
            }
            // (the selected card kept in view)
            if let Some(&(_, r)) = c.cards.get(c.sel) {
                let top = r.min.y + c.scroll - c.grid.min.y;
                if top < c.scroll {
                    c.scroll = (top - px(30.0)).max(0.0);
                } else if top + r.height() > c.scroll + c.grid.height() {
                    c.scroll = top + r.height() - c.grid.height() + px(10.0);
                }
            }
        }
        _ => {}
    }
}

// -- the info card ----------------------------------------------------------------------------------------------------

/// The "i" dot's card over the room: "Inspired by ...", the real unit's era, what it is, its character, players and
/// records, what each control of ours does with its sweet spot, the songs in your library that use it. A capture: what
/// was captured and the model it sounds like. Esc or a click outside closes it.
pub fn info_card(room: &mut RoomView, ui: &Ui, area: Rect) {
    let Some(e) = room.info else { return };
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    paint::dim(&p, area, Some(140));
    let w = (area.width() * 0.56).clamp(px(520.0), px(760.0));
    // (a capture has less to say: a shorter card, not one 80% empty, P37.6c)
    let h = (area.height() * 0.8).min(match e {
        Entry::Capture(_) => px(440.0),
        Entry::Model(_) => px(700.0),
    });
    let r = Rect::from_center_size(area.center(), vec2(w, h));
    let outside = interact(ui, area, "room-info-dim", Sense::click());
    // (solid under the panel's light: the room mustn't show through the words)
    p.rect_filled(r, px(20.0), th.base);
    paint::floating(&p, r, Some(px(20.0)));
    let x0 = r.min.x + px(28.0);
    let tw = r.width() - px(56.0);
    let mut y = r.min.y + px(30.0);
    // the face, then the words
    let blk = e.block();
    let fs = faces::board_size(&blk);
    let bed = Rect::from_min_size(pos2(x0, y), vec2(px(170.0), px(130.0)));
    paint::inset(&p, bed, Some(px(10.0)), 1.5, paint::Fill::Base);
    let k = ((bed.width() - px(20.0)) / fs.x).min((bed.height() - px(18.0)) / fs.y);
    let fr = Rect::from_center_size(bed.center(), fs * k);
    faces::miniature(&p, fr, &blk, 0.0);
    let hx = bed.max.x + px(20.0);
    let hw = r.max.x - px(28.0) - hx;
    let (name, insp, ch) = words(e);
    let kind = match e {
        Entry::Model(m) => MODELS[m].family().label().to_uppercase(),
        Entry::Capture(_) => "CAPTURE".into(),
    };
    text::caps(
        &p,
        &kind,
        pos2(hx, y + px(8.0)),
        10.5,
        th.caption,
        Align2::LEFT_CENTER,
        Some(hw),
    );
    // (a capture's full name, on two lines if it needs them: the short one cut off "Hot Rod Deluxe III · Dr…")
    let full = match e {
        Entry::Capture(id) => {
            crate::captures::with(|l| l.get(id).map(|i| i.name.clone())).unwrap_or(name.clone())
        }
        Entry::Model(_) => name.clone(),
    };
    let ns = Style::new(
        if matches!(e, Entry::Capture(_)) {
            17.0
        } else {
            22.0
        },
        800,
        th.text,
    );
    let lines = text::wrap(&ctx, &full, ns, hw);
    let two = lines.len() > 1;
    for (n, l) in lines.iter().take(2).enumerate() {
        text::draw(
            &p,
            l,
            pos2(hx, y + px(if two { 26.0 } else { 32.0 }) + px(21.0) * n as f32),
            Align2::LEFT_CENTER,
            ns,
            Some(hw),
        );
    }
    let head = match e {
        Entry::Model(m) => gearinfo::info(MODELS[m].id)
            .map(|i| format!("Inspired by {}", i.inspired_by))
            .unwrap_or_else(|| insp.clone()),
        Entry::Capture(_) => insp.clone(),
    };
    let mut hy = y + px(if two { 76.0 } else { 60.0 });
    for l in text::wrap(&ctx, &head, Style::new(14.0, 700, th.accent_ink), hw)
        .iter()
        .take(2)
    {
        text::draw(
            &p,
            l,
            pos2(hx, hy),
            Align2::LEFT_CENTER,
            Style::new(14.0, 700, th.accent_ink),
            None,
        );
        hy += px(19.0);
    }
    if let Entry::Model(m) = e
        && let Some(i) = gearinfo::info(MODELS[m].id)
        && !i.era.is_empty()
    {
        text::draw(
            &p,
            i.era,
            pos2(hx, hy + px(4.0)),
            Align2::LEFT_CENTER,
            Style::new(11.5, 700, th.caption),
            Some(hw),
        );
    }
    // (P41.3: what this kind of unit is, in one line, before its story; the word itself hoverable)
    let kr = Rect::from_min_size(
        pos2(hx, y),
        vec2(text::width(&ctx, &kind, Style::caps(10.5, th.caption)), px(16.0)),
    );
    let word = match e {
        Entry::Capture(_) => "capture",
        Entry::Model(m) => match MODELS[m].id {
            "cab_ir" => "ir",
            "amp_nam" | "pedal_nam" => "capture",
            _ => MODELS[m].family().label(),
        },
    };
    super::plain::hover(&mut room.rack.tip, ui, kr, word);
    y = bed.max.y + px(22.0);
    let bottom = r.max.y - px(54.0);
    let what = super::plain::what_is(e);
    if !what.is_empty() {
        for l in text::wrap(&ctx, what, Style::new(13.0, 700, th.text), tw)
            .iter()
            .take(2)
        {
            text::draw(
                &p,
                l,
                pos2(x0, y),
                Align2::LEFT_CENTER,
                Style::new(13.0, 700, th.text),
                None,
            );
            y += px(18.0);
        }
        y += px(10.0);
    }
    let para = |p: &Painter, title: &str, body: &str, y: &mut f32| {
        if body.trim().is_empty() || *y > bottom - px(30.0) {
            return;
        }
        text::caps(
            p,
            title,
            pos2(x0, *y),
            10.0,
            th.caption,
            Align2::LEFT_CENTER,
            None,
        );
        *y += px(18.0);
        // (each line of the body its own paragraph: a control a line)
        for (n, part) in body.split('\n').enumerate() {
            if n > 0 {
                *y += px(4.0);
            }
            for l in text::wrap(&ctx, part, Style::new(12.5, 600, th.text_2), tw) {
                if *y > bottom {
                    break;
                }
                text::draw(
                    p,
                    &l,
                    pos2(x0, *y),
                    Align2::LEFT_CENTER,
                    Style::new(12.5, 600, th.text_2),
                    None,
                );
                *y += px(17.0);
            }
        }
        *y += px(10.0);
    };
    match e {
        Entry::Model(m) => {
            let d = &MODELS[m];
            if let Some(i) = gearinfo::info(d.id) {
                para(&p, "THE REAL ONE", i.about, &mut y);
                para(
                    &p,
                    "ITS SOUND",
                    if i.character.is_empty() { &ch } else { i.character },
                    &mut y,
                );
                if !i.players.is_empty() {
                    para(&p, "PLAYERS AND RECORDS", &i.players.join("  \u{b7}  "), &mut y);
                }
                let ctl: Vec<String> = d
                    .front_knobs()
                    .iter()
                    .filter_map(|kd| {
                        let line = i.control(kd.key)?;
                        let sweet = gearinfo::tip(d.id, kd.key)
                            .filter(|t| !t.note.is_empty())
                            .map(|t| format!(" (sweet spot: {})", t.note))
                            .unwrap_or_default();
                        let added = if catalog::is_added(d.id, kd.key) {
                            " (added: not on the real one)"
                        } else {
                            ""
                        };
                        Some(format!("{}{added}: {line}{sweet}", kd.label))
                    })
                    .collect();
                if !ctl.is_empty() {
                    para(&p, "ITS CONTROLS", &ctl.join("\n"), &mut y);
                }
            } else {
                para(&p, "WHAT IT IS", d.blurb, &mut y);
            }
            let songs: Vec<String> = crate::gearindex::songs_with(d.id)
                .into_iter()
                .take(6)
                .map(|(_, title, artist)| format!("{title} ({artist})"))
                .collect();
            if !songs.is_empty() {
                para(
                    &p,
                    "SONGS IN YOUR LIBRARY THAT USE IT",
                    &songs.join("  \u{b7}  "),
                    &mut y,
                );
            }
        }
        Entry::Capture(id) => {
            let (what, from) = crate::captures::with(|l| {
                l.get(id)
                    .map(|i| {
                        let mut what = format!("{}.", i.name);
                        if let Some(g) = &i.gear {
                            what.push_str(&format!(" Captured from: {g}."));
                        }
                        if let Some(c) = &i.creator {
                            what.push_str(&format!(" By {c}."));
                        }
                        (what, i.detail())
                    })
                    .unwrap_or_default()
            });
            para(&p, "WHAT WAS CAPTURED", &what, &mut y);
            para(&p, "THE FILE", &from, &mut y);
            // (the line under the name says "sounds like our X"; under its title, just the model)
            let like = insp.strip_prefix("sounds like ").unwrap_or(&insp);
            para(&p, "SOUNDS LIKE", like, &mut y);
        }
    }
    let close_w = draw::key_w(&ctx, "Close", "Esc", false);
    let cr = Rect::from_min_size(
        pos2(r.max.x - px(24.0) - close_w, r.max.y - px(46.0)),
        vec2(close_w, px(32.0)),
    );
    let closed = draw::key(ui, cr, "room-info-close", KeyLook::Dark, "Close", "Esc", None).clicked();
    let card_hit = ui.rect_contains_pointer(r);
    if closed || (outside.clicked() && !card_hit) {
        room.info = None;
    }
}

/// The info card's keys: Esc closes it (it takes every key while open).
pub fn info_handle(room: &mut RoomView, ev: &Event) {
    if let Event::Key {
        key: Key::Escape,
        pressed: true,
        ..
    } = ev
    {
        room.info = None;
    }
}
