//! The amp room's one screen (P37.1, D102; the audit's mock 1, "simple first, depth on demand"): the chain on its
//! stage, six quick controls naming the real knob each turns, the open unit (its face, where every control turns,
//! and the same controls as a list you can drag), and the Sounds drawer (This song | My sounds | Starters | All
//! song tones | Gear & captures). The song pedalboard is this screen too: the player's tile opens the room in the
//! song's context. Song tone explained (mock 2) is an overlay from a song tone's "RS gear" link.

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};
use qrs_audio::board::{self, Block, Board, Kind, MODELS, Unit, catalog};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, interact};

use super::stage::{self, Entry};
use super::{RoomView, Tab, Typing};
use crate::pedalfaces as faces;
use crate::player::board::Pick;
use crate::state::AppState;

/// The drawer's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Song,
    Mine,
    Starters,
    AllSongs,
    Gear,
}

/// What the shelf shows of a family: our models, your captures, or both (replacing the hidden `nam_songs` filter
/// that took our models off the shelf).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShelfMode {
    Models,
    Captures,
    Both,
}

/// The one screen's own state.
pub struct RigUi {
    pub seg: Seg,
    pub shelf: ShelfMode,
    /// A quick control being dragged: (its slot, where it started, its turn there).
    qdrag: Option<(usize, f32, f32)>,
    /// Where the wheel turns a quick control this frame.
    qspots: Vec<(Rect, usize)>,
    /// Song tone explained: which tone.
    pub explain: Option<usize>,
    /// "Change model": the unit the shelf replaces (else the shelf adds).
    pub change: Option<u32>,
    /// What "Hear" is playing (a row's key), to stop it with a second click.
    hearing: Option<String>,
    /// The suggested captures list is open.
    pub suggest: bool,
    /// The open unit's controls as a list beside its face (P37.6c: closed by default, the face shows them all).
    pub list_open: bool,
    /// What the goal row last moved, said under it (P41.3).
    pub goal_said: Option<String>,
}

impl Default for RigUi {
    fn default() -> Self {
        RigUi {
            seg: Seg::Mine,
            shelf: if crate::captures::ON {
                ShelfMode::Both
            } else {
                ShelfMode::Models
            },
            qdrag: None,
            qspots: Vec::new(),
            explain: None,
            change: None,
            hearing: None,
            suggest: false,
            list_open: false,
            goal_said: None,
        }
    }
}

// -- quick controls ---------------------------------------------------------------------------------------------------

/// What a quick control turns.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// These knobs (block, knob), all set to the same turn; the first is what it shows.
    Knobs(Vec<(u32, usize)>),
    /// The sound's level (the board's).
    Level,
    /// Nothing in this rig has it.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quick {
    pub label: &'static str,
    pub target: Target,
    /// The real knob it turns, in words ("Perspex 100 gain").
    pub says: String,
    /// The unit it turns ("Perspex 100"; empty for Level and nothing), and its knob alone ("gain"): the card names
    /// the main unit once and each caption only its knob (P37.6c).
    pub unit: String,
    pub knob: String,
}

/// The six quick controls for a board (pure, tested): Gain, Bass, Mid, Treble on the amp that plays (a capture's
/// Input and its post EQ when it has one; else the first drive's or EQ's), Effects the mix of every delay and
/// reverb that's on, Level the sound's level.
pub fn quick_map(b: &Board) -> Vec<Quick> {
    let on = |k: Kind| b.blocks.iter().filter(move |x| x.on && x.def().kind == k);
    // (one Gain on a capture, P41.3: its Gain knob when it steps through a gain ladder (the captures of the real
    // unit at several gains), else its Input, the drive into it, named "Drive")
    let ladder = |x: &Block| x.def().file && qrs_audio::board::nam::ladder(x.asset).len() > 1;
    let knob_of = |x: &Block, keys: &[&str]| {
        if x.def().file && keys.first() == Some(&"gain") {
            return if ladder(x) {
                x.def().knob_index("gain")
            } else {
                x.def().knob_index("input")
            };
        }
        keys.iter().find_map(|k| x.def().knob_index(k))
    };
    let amp = on(Kind::Amp).next().copied();
    let pick = |keys: &[&str], also: &[Kind]| -> Option<(Block, usize)> {
        if let Some(a) = amp
            && let Some(k) = knob_of(&a, keys)
        {
            return Some((a, k));
        }
        also.iter()
            .flat_map(|&kind| on(kind))
            .find_map(|x| knob_of(x, keys).map(|k| (*x, k)))
    };
    // (an empty capture unit is named by what it is: "No file chosen yet input" read as nonsense)
    let name = |x: &Block| match crate::player::board::Rack::file_name(x) {
        Some(n) if x.def().file && x.asset != 0 => short_capture_name(&n),
        _ => x.def().name.to_string(),
    };
    let one = |label: &'static str, keys: &[&str], also: &[Kind]| match pick(keys, also) {
        Some((x, k)) => {
            let key = x.def().knobs[k].key;
            // (a capture's Input as its Gain reads "Drive": the drive into the capture, not a gain knob of its own)
            let (label, knob) = match (x.def().file, key) {
                (true, "input") if label == "Gain" => ("Drive", "input drive".to_string()),
                (true, "gain") => (label, "gain (its captures)".to_string()),
                _ => (label, x.def().knobs[k].label.to_lowercase()),
            };
            Quick {
                label,
                target: Target::Knobs(vec![(x.uid, k)]),
                says: format!("{} {knob}", name(&x)),
                unit: name(&x),
                knob,
            }
        }
        None => Quick {
            label,
            target: Target::None,
            says: format!("no {} in this rig", label.to_lowercase()),
            unit: String::new(),
            knob: format!("no {}", label.to_lowercase()),
        },
    };
    let fx = effect_knobs(b);
    let effects = if fx.is_empty() {
        Quick {
            label: "Effects",
            target: Target::None,
            says: "no delay or reverb on".into(),
            unit: String::new(),
            knob: "no delay or reverb on".into(),
        }
    } else {
        let blk = |uid: u32| b.index_of(uid).map(|i| b.blocks[i]);
        let what = |&(uid, k): &(u32, usize)| {
            blk(uid).map_or(String::new(), |x| {
                format!("{} {}", x.def().name, x.def().knobs[k].label.to_lowercase())
            })
        };
        let (unit, knob) = match fx.as_slice() {
            [one] => {
                let x = blk(one.0).expect("on the board");
                let kd = x.def().knobs[one.1];
                (x.def().name.to_string(), kd.label.to_lowercase())
            }
            _ => (String::new(), format!("mix of {} effects", fx.len())),
        };
        Quick {
            label: "Effects",
            says: fx.iter().map(what).collect::<Vec<_>>().join(" + "),
            target: Target::Knobs(fx),
            unit,
            knob,
        }
    };
    vec![
        one(
            "Gain",
            &["gain", "volume", "input", "drive"],
            &[Kind::Drive, Kind::Fuzz],
        ),
        one("Bass", &["bass"], &[Kind::Eq]),
        one("Mid", &["mid", "middle"], &[Kind::Eq]),
        one("Treble", &["treble"], &[Kind::Eq]),
        effects,
        Quick {
            label: "Level",
            target: Target::Level,
            says: "sound level".into(),
            unit: String::new(),
            knob: "sound level".into(),
        },
    ]
}

/// The unit most quick controls turn (named once in the card's header), if any.
pub fn quick_main_unit(qs: &[Quick]) -> Option<String> {
    let mut best: Option<(&str, usize)> = None;
    for q in qs.iter().filter(|q| !q.unit.is_empty()) {
        let n = qs.iter().filter(|o| o.unit == q.unit).count();
        if best.is_none_or(|(_, m)| n > m) {
            best = Some((&q.unit, n));
        }
    }
    best.map(|(u, _)| u.to_string())
}

/// A quick control's caption under its knob: the knob alone on the main unit, else the unit and its knob.
pub fn quick_caption(q: &Quick, main: Option<&str>) -> String {
    if q.unit.is_empty() || Some(q.unit.as_str()) == main {
        q.knob.clone()
    } else {
        format!("{} {}", q.unit, q.knob)
    }
}

/// A capture's name, short (P41.3, D172): the gear without its year, and the setting in plain words when it has one
/// ("1976 Fender Deluxe Reverb · edge SFDR Sweet Spot V4 T5 B5" → "Fender Deluxe Reverb · edge of breakup"). The
/// rest (knob settings, size, licence) is in its info card.
pub fn short_capture_name(n: &str) -> String {
    let at = [" \u{b7} ", " - ", " | "]
        .iter()
        .filter_map(|s| n.find(s).map(|i| (i, s.len())))
        .min();
    let (gear, set) = match at {
        Some((i, l)) => (&n[..i], &n[i + l..]),
        None => (n, ""),
    };
    // (no bracketed tail, no leading year)
    let gear = gear[..[" (", " [", ", "]
        .iter()
        .filter_map(|s| gear.find(s))
        .min()
        .unwrap_or(gear.len())]
        .trim();
    let gear = match gear.split_once(' ') {
        Some((y, rest)) if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() => rest,
        _ => gear,
    };
    let set = setting_words(set);
    let s = if set.is_empty() {
        gear.to_string()
    } else {
        format!("{gear} \u{b7} {set}")
    };
    let s = if s.chars().count() > 44 {
        gear.to_string()
    } else {
        s
    };
    if s.chars().count() > 40 {
        format!("{}\u{2026}", s.chars().take(39).collect::<String>())
    } else {
        s
    }
}

/// A capture's setting in a player's words: the first character word it has ("edge" → "edge of breakup", "crunch",
/// "clean"...), else nothing (a setting like "V4 T5 B5" or a pack's name says nothing to a newcomer).
pub(crate) fn setting_words(s: &str) -> &'static str {
    let l = s.to_lowercase();
    [
        ("edge", "edge of breakup"),
        ("breakup", "edge of breakup"),
        ("crunch", "crunch"),
        ("clean", "clean"),
        ("high gain", "high gain"),
        ("hi gain", "high gain"),
        ("hi-gain", "high gain"),
        ("lead", "lead"),
        ("dirty", "dirty"),
        ("drive", "driven"),
        ("boost", "boosted"),
        ("fuzz", "fuzz"),
    ]
    .iter()
    .filter_map(|(w, say)| l.find(w).map(|i| (i, *say)))
    .min_by_key(|x| x.0)
    .map_or("", |x| x.1)
}

/// What the Effects quick knob turns (P41.3): the mix of every delay and reverb that's on, an amp's own reverb, and
/// its tremolo's intensity while the trem is on (the Twin's spring and trem are effects to a player).
pub fn effect_knobs(b: &Board) -> Vec<(u32, usize)> {
    let mut v = Vec::new();
    for x in b.blocks.iter().filter(|x| x.on) {
        let d = x.def();
        match d.kind {
            Kind::Delay | Kind::Reverb => {
                if let Some(k) = d.knob_index("mix") {
                    v.push((x.uid, k));
                }
            }
            Kind::Amp => {
                if let Some(k) = d.knob_index("reverb") {
                    v.push((x.uid, k));
                }
                if let Some(k) = d.knob_index("intensity")
                    && x.v[k] > d.knobs[k].lo + 0.05
                {
                    v.push((x.uid, k));
                }
            }
            _ => {}
        }
    }
    v
}

// -- the goal row (P41.3, D173) -----------------------------------------------------------------------------------------

/// What a player wants of a sound, in their words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    MoreCrunch,
    LessHarsh,
    Bigger,
    Cleaner,
    MoreSpace,
}

impl Goal {
    pub const ALL: [Goal; 5] = [
        Goal::MoreCrunch,
        Goal::LessHarsh,
        Goal::Bigger,
        Goal::Cleaner,
        Goal::MoreSpace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Goal::MoreCrunch => "More crunch",
            Goal::LessHarsh => "Less harsh",
            Goal::Bigger => "Bigger",
            Goal::Cleaner => "Cleaner",
            Goal::MoreSpace => "More space",
        }
    }
}

/// One knob a goal turns: the unit, its knob, its value before and after.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nudge {
    pub uid: u32,
    pub knob: usize,
    pub from: f32,
    pub to: f32,
}

/// A step of the dial (a tenth of its turn; a capture's gain ladder by one capture).
const STEP: f32 = 0.1;

/// The knobs a goal turns on `b`, a step each way it says (pure, tested): More crunch the Gain (a capture's gain
/// ladder up a capture, or its drive) and, with no amp, a drive pedal's drive; Cleaner the same down, with every drive
/// pedal's drive; Less harsh the Treble and Presence down (a cab's high cut with neither); Bigger the Bass up and the
/// cab's mic closer; More space the mix of every delay and reverb (the amp's own reverb too). Empty: nothing on this
/// rig does it (More space then adds a reverb, `apply_goal`).
pub fn goal_nudges(b: &Board, g: Goal) -> Vec<Nudge> {
    let qs = quick_map(b);
    let mut out: Vec<Nudge> = Vec::new();
    let turn = |out: &mut Vec<Nudge>, uid: u32, k: usize, du: f32| {
        let Some(x) = b.index_of(uid).map(|i| b.blocks[i]) else {
            return;
        };
        if out.iter().any(|n| n.uid == uid && n.knob == k) {
            return;
        }
        let kd = x.def().knobs[k];
        let from = x.v[k];
        let to = if x.def().file && kd.key == "gain" {
            // (a capture's gain ladder: the next capture up or down)
            let steps = qrs_audio::board::nam::ladder(x.asset);
            let next = if du > 0.0 {
                steps
                    .iter()
                    .map(|s| s.1)
                    .filter(|&v| v > from + 1e-3)
                    .reduce(f32::min)
            } else {
                steps
                    .iter()
                    .map(|s| s.1)
                    .filter(|&v| v < from - 1e-3)
                    .reduce(f32::max)
            };
            match next {
                Some(v) => v,
                None => return,
            }
        } else {
            kd.from_u((kd.to_u(from) + du).clamp(0.0, 1.0))
        };
        if (to - from).abs() > 1e-4 {
            out.push(Nudge {
                uid,
                knob: k,
                from,
                to,
            });
        }
    };
    let knob = |q: &Quick| match &q.target {
        Target::Knobs(v) => v.first().copied(),
        _ => None,
    };
    let find = |kind: Kind, keys: &[&str]| -> Vec<(u32, usize)> {
        b.blocks
            .iter()
            .filter(|x| x.on && x.def().kind == kind)
            .filter_map(|x| {
                keys.iter()
                    .find_map(|k| x.def().knob_index(k))
                    .map(|k| (x.uid, k))
            })
            .collect()
    };
    let has_amp = b.blocks.iter().any(|x| x.on && x.def().kind == Kind::Amp);
    match g {
        Goal::MoreCrunch | Goal::Cleaner => {
            let du = if g == Goal::MoreCrunch { STEP } else { -STEP };
            if let Some((uid, k)) = knob(&qs[0]) {
                turn(&mut out, uid, k, du);
            }
            // (no amp: the drive pedal is the crunch; cleaner: every drive pedal down a step too)
            if !has_amp || g == Goal::Cleaner {
                for kind in [Kind::Drive, Kind::Fuzz] {
                    for (uid, k) in find(kind, &["drive", "gain", "fuzz", "sustain"]) {
                        turn(&mut out, uid, k, du);
                    }
                }
            }
        }
        Goal::LessHarsh => {
            if let Some((uid, k)) = knob(&qs[3]) {
                turn(&mut out, uid, k, -STEP);
            }
            for (uid, k) in find(Kind::Amp, &["presence"]) {
                turn(&mut out, uid, k, -STEP);
            }
            if out.is_empty() {
                for (uid, k) in find(Kind::Cab, &["high_cut"]) {
                    turn(&mut out, uid, k, -STEP);
                }
            }
        }
        Goal::Bigger => {
            if let Some((uid, k)) = knob(&qs[1]) {
                turn(&mut out, uid, k, STEP);
            }
            for (uid, k) in find(Kind::Cab, &["distance"]) {
                turn(&mut out, uid, k, -STEP);
            }
        }
        Goal::MoreSpace => {
            for (uid, k) in effect_knobs(b) {
                turn(&mut out, uid, k, STEP * 0.8);
            }
        }
    }
    out
}

/// What a goal moved, in words: "Perspex 100 gain 6.0 → 7.0".
pub fn nudge_words(b: &Board, n: &Nudge) -> String {
    let Some(x) = b.index_of(n.uid).map(|i| b.blocks[i]) else {
        return String::new();
    };
    let (mut a, mut c) = (x, x);
    a.v[n.knob] = n.from;
    c.v[n.knob] = n.to;
    let name = match crate::player::board::Rack::file_name(&x) {
        Some(f) if x.def().file && x.asset != 0 => short_capture_name(&f),
        _ => x.def().name.to_string(),
    };
    format!(
        "{name} {} {} \u{2192} {}",
        x.def().knobs[n.knob].label.to_lowercase(),
        faces::silver::knob_text(&a, n.knob),
        faces::silver::knob_text(&c, n.knob)
    )
}

/// A goal on the room's rig: its knobs turned (one undo step), and what moved said. More space with no delay or
/// reverb adds a Room reverb at the end.
fn apply_goal(room: &mut RoomView, st: &mut AppState, g: Goal) {
    let b = room.rack.board_now(st);
    let nudges = goal_nudges(&b, g);
    if nudges.is_empty() {
        if g == Goal::MoreSpace
            && let Some(m) = catalog::find("room")
        {
            room.rack.add_block(st, b.blocks.len(), m);
            room.rig_ui.goal_said = Some("More space: added a Room reverb at the end".into());
            room.say(
                "More space: added a Room reverb",
                "  \u{b7}  Ctrl Z takes it back",
            );
        } else {
            let why = match g {
                Goal::LessHarsh => "no treble, presence or cab to soften here",
                Goal::Bigger => "no bass knob or cab mic here",
                _ => "nothing on this rig turns that way any further",
            };
            room.rig_ui.goal_said = Some(format!("{}: {why}", g.label()));
            room.say(format!("{}: {why}", g.label()), "");
        }
        return;
    }
    let words: Vec<String> = nudges.iter().map(|n| nudge_words(&b, n)).collect();
    for n in &nudges {
        room.rack.set_knob(st, n.uid, n.knob, n.to);
    }
    room.rack.board_commit(st);
    let line = words.join(", ");
    room.say(format!("{}: {line}", g.label()), "  \u{b7}  Ctrl Z takes it back");
    room.rig_ui.goal_said = Some(format!("{}: {line}", g.label()));
}

const LEVEL: (f32, f32) = (-24.0, 12.0);

fn quick_u(b: &Board, q: &Quick) -> Option<f32> {
    match &q.target {
        Target::Knobs(v) => {
            let (uid, k) = *v.first()?;
            let x = b.blocks[b.index_of(uid)?];
            Some(x.def().knobs[k].to_u(x.v[k]))
        }
        Target::Level => Some((b.level_db - LEVEL.0) / (LEVEL.1 - LEVEL.0)),
        Target::None => None,
    }
}

fn quick_text(b: &Board, q: &Quick) -> String {
    match &q.target {
        Target::Knobs(v) => v
            .first()
            .and_then(|&(uid, k)| {
                b.index_of(uid).map(|i| {
                    // (a capture's gain ladder: "4 of 4", the step it's on; its gain is on the face)
                    let t = faces::silver::knob_text(&b.blocks[i], k);
                    match t.split_once(" \u{b7} ") {
                        Some((_, step)) if b.blocks[i].def().file => step.to_string(),
                        _ => t,
                    }
                })
            })
            .unwrap_or_default(),
        Target::Level => {
            if b.level_db.abs() < 0.05 {
                "0 dB".into()
            } else {
                format!("{:+.1} dB", b.level_db)
            }
        }
        Target::None => "\u{2013}".into(),
    }
}

fn set_quick(room: &mut RoomView, st: &mut AppState, q: &Quick, u: f32) {
    match &q.target {
        Target::Knobs(v) => {
            let b = room.rack.board_now(st);
            for &(uid, k) in v {
                if let Some(i) = b.index_of(uid) {
                    let kd = b.blocks[i].def().knobs[k];
                    room.rack.set_knob(st, uid, k, kd.from_u(u));
                }
            }
        }
        Target::Level => {
            let db = ((LEVEL.0 + u.clamp(0.0, 1.0) * (LEVEL.1 - LEVEL.0)) * 2.0).round() / 2.0;
            room.rack.board_edit(st, |x| x.level_db = db);
        }
        Target::None => {}
    }
}

// -- small parts ------------------------------------------------------------------------------------------------------

/// A text link (secondary actions: quiet, no button), accent ink, underlined under the mouse.
fn link(ui: &Ui, pos: Pos2, id: impl egui::AsId, s: &str, anchor: Align2, size: f32) -> Response {
    let th = t();
    let st = Style::new(size, 700, th.accent_ink);
    let w = text::width(ui.ctx(), s, st);
    let r = anchor.anchor_size(pos, vec2(w, px(size + 6.0)));
    let resp = interact(ui, r, id, Sense::click());
    let p = ui.painter();
    text::draw(p, s, r.left_center(), Align2::LEFT_CENTER, st, None);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        p.line_segment(
            [pos2(r.min.x, r.max.y - px(2.0)), pos2(r.max.x, r.max.y - px(2.0))],
            Stroke::new(1.0, th.accent_ink),
        );
    }
    resp
}

/// A quiet text link (the open unit's actions): the second text colour, amber and underlined on hover.
fn quiet_link(ui: &Ui, pos: Pos2, id: impl egui::AsId, s: &str, size: f32) -> Response {
    let th = t();
    let w = text::width(ui.ctx(), s, Style::new(size, 700, th.text_2));
    let r = Align2::LEFT_CENTER.anchor_size(pos, vec2(w, px(size + 6.0)));
    let resp = interact(ui, r, id, Sense::click());
    let p = ui.painter();
    let ink = if resp.hovered() { th.accent_ink } else { th.text_2 };
    text::draw(
        p,
        s,
        r.left_center(),
        Align2::LEFT_CENTER,
        Style::new(size, 700, ink),
        None,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        p.line_segment(
            [pos2(r.min.x, r.max.y - px(2.0)), pos2(r.max.x, r.max.y - px(2.0))],
            Stroke::new(1.0, th.accent_ink),
        );
    }
    resp
}

fn card(p: &egui::Painter, r: Rect) {
    super::draw::card(p, r, px(18.0));
}

fn header(p: &egui::Painter, r: Rect, s: &str) -> Rect {
    super::draw::caps(
        p,
        s,
        pos2(r.min.x + px(20.0), r.min.y + px(20.0)),
        10.5,
        t().caption,
        Align2::LEFT_CENTER,
    )
}

// -- the page ---------------------------------------------------------------------------------------------------------

/// The one screen's panels in `body` (P37.13): the chain, the quick controls and the open unit side by side, the
/// drawer under them. The open unit gets the height (it is where the work is); the drawer keeps at least a shelf's row.
pub struct PageRects {
    pub chain: Rect,
    pub quick: Rect,
    pub unit: Rect,
    pub drawer: Rect,
}

pub fn page_rects(body: Rect) -> PageRects {
    let gap = px(12.0);
    // (a short window keeps the open unit tall enough for a face at its own size: the chain and drawer give way)
    let short = body.height() < px(600.0);
    let chain_h = (body.height() * 0.22).clamp(px(if short { 118.0 } else { 140.0 }), px(200.0));
    let drawer_h = (body.height() * 0.27).clamp(px(if short { 104.0 } else { 150.0 }), px(300.0));
    let chain = Rect::from_min_size(body.min, vec2(body.width(), chain_h));
    let mid = Rect::from_min_max(
        pos2(body.min.x, chain.max.y + gap),
        pos2(
            body.max.x,
            (body.max.y - drawer_h - gap).max(chain.max.y + gap + px(200.0)),
        ),
    );
    let drawer = Rect::from_min_max(pos2(body.min.x, mid.max.y + gap), body.max);
    // (the quick controls want about 3 knobs across; the unit the rest)
    let qw = (mid.width() * 0.3).clamp(px(300.0), px(460.0));
    let split = mid.min.x + qw.round();
    PageRects {
        chain,
        quick: Rect::from_min_max(mid.min, pos2(split - gap / 2.0, mid.max.y)),
        unit: Rect::from_min_max(pos2(split + gap / 2.0, mid.min.y), mid.max),
        drawer,
    }
}

pub fn page(room: &mut RoomView, ui: &Ui, st: &mut AppState, body: Rect) {
    room.spots.clear();
    room.rig_ui.qspots.clear();
    let PageRects {
        chain: chain_r,
        quick: quick_r,
        unit: unit_r,
        drawer: drawer_r,
    } = page_rects(body);
    chain_row(room, ui, st, chain_r);
    // a + in the chain: the drawer's gear, adding there
    if let Some(
        crate::player::board::Picker::Kinds { at } | crate::player::board::Picker::Models { at, .. },
    ) = room.rack.board.picker.take()
    {
        room.shelf_at = Some(at);
        room.rig_ui.seg = Seg::Gear;
        room.rig_ui.change = None;
    }
    quick(room, ui, st, quick_r);
    open_unit(room, ui, st, unit_r);
    if drawer_r.height() > px(60.0) {
        drawer(room, ui, st, drawer_r);
    }
    if let Some(i) = room.rig_ui.explain {
        explain(room, ui, st, body, i);
    }
}

/// The chain: every unit small and in order, named (click opens it, double-click turns it off, drag reorders,
/// + adds), what's live, and (B17) a modeller in front said with its fix.
fn chain_row(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    card(&p, r);
    let hr = header(
        &p,
        r,
        "THE CHAIN  \u{b7}  CLICK A UNIT TO OPEN IT  \u{b7}  DRAG TO REORDER  \u{b7}  + ADDS",
    );
    let has_input = room.rack.engine().is_some_and(|e| e.status().input.is_some());
    let (dot, word) = if room.ab || room.rack.board.audition.is_some() {
        (
            th.accent,
            "HEARING A COMPARISON  \u{b7}  THE KEY LEFT OF 1 FLIPS BACK",
        )
    } else if has_input || room.shot {
        (
            Color32::from_rgb(0x3e, 0xc4, 0x6d),
            if st.instrument().is_bass() {
                "LIVE  \u{b7}  YOUR BASS PLAYS THROUGH THIS"
            } else {
                "LIVE  \u{b7}  YOUR GUITAR PLAYS THROUGH THIS"
            },
        )
    } else {
        (
            th.caption,
            "NO INPUT  \u{b7}  SET UP YOUR INSTRUMENT IN SETTINGS > SOUND",
        )
    };
    let b = room.rack.board_now(st);
    // (recording: its badge at the header's right, the words left of it)
    let right = match st.rec.status() {
        Some(s) => {
            let w = crate::recording::badge_width(ui, s);
            room.badge_at = Some(pos2(r.max.x - px(16.0), hr.center().y));
            r.max.x - px(16.0) - w - px(14.0)
        }
        None => r.max.x - px(20.0),
    };
    let wr = super::draw::caps(
        &p,
        &format!(
            "{word}  \u{b7}  {} UNITS, {} ON",
            b.blocks.len(),
            b.blocks.iter().filter(|x| x.on).count()
        ),
        pos2(right, hr.center().y),
        10.0,
        dot,
        Align2::RIGHT_CENTER,
    );
    p.circle_filled(pos2(wr.min.x - px(10.0), wr.center().y), px(3.5), dot);
    let warn = modeller_warning(room, ui, st, r);
    let well = Rect::from_min_max(
        pos2(r.min.x + px(14.0), r.min.y + px(34.0)),
        pos2(
            r.max.x - px(14.0),
            r.max.y - if warn { px(44.0) } else { px(10.0) },
        ),
    );
    paint::inset(&p, well, Some(px(12.0)), 2.0, paint::Fill::Base);
    room.rack.chain_ui(ui, st, well, "room-chain", true);
    // the hovered unit's "i" dot at its top right
    let b = room.rack.board_now(st);
    if !room.rack.board.inert
        && let Some(&(uid, cr)) = room
            .rack
            .board
            .chips
            .iter()
            .find(|(_, cr)| ui.rect_contains_pointer(cr.expand(px(10.0))))
        && let Some(i) = b.index_of(uid)
    {
        let blk = b.blocks[i];
        let e = if blk.def().file && blk.asset != 0 {
            Entry::Capture(blk.asset)
        } else {
            Entry::Model(blk.model)
        };
        // (at its bottom right: the remove key has the top right, owner P37.6c)
        let c = crate::player::board::chip_keys(cr).1.center();
        if super::catalogue::info_dot(ui, c, ("room-chain-i", uid)).clicked() {
            room.info = Some(e);
        }
    }
}

/// The modeller banner's answers, per input (P41.3, D170): config.json `modeller_banner` {input id: "yes" | "no" |
/// "hidden"}, additive.
pub const KEY_BANNER: &str = "modeller_banner";

/// What the modeller banner was told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// "Yes, use quickRS's amps": the input is taken as dry (the interface set to a clean preset).
    Yes,
    /// "No, keep my interface's sound": the input is an amp modeller for good (songs play it as it is).
    No,
    /// The ×: asked enough, nothing changed.
    Hide,
}

/// Whether the banner asks about this input: an amp modeller (by its kind, or Auto by its name), not a microphone,
/// and not answered yet.
pub fn banner_wanted(cfg: &qrs_library::Config, id: &str, name: &str) -> bool {
    let answered = cfg.get(KEY_BANNER).and_then(|m| m.get(id)).is_some();
    !answered
        && crate::calibrate::load(cfg, id).kind != crate::calibrate::Kind::Mic
        && crate::calibrate::is_modeller_input(cfg, id, name)
}

/// The banner answered for this input: Yes takes it as dry, No as a modeller; each (and the ×) is kept, so it never
/// asks again.
pub fn answer_banner(cfg: &mut qrs_library::Config, id: &str, a: Answer) {
    let mut cal = crate::calibrate::load(cfg, id);
    match a {
        Answer::Yes => cal.kind = crate::calibrate::Kind::Dry,
        Answer::No => cal.kind = crate::calibrate::Kind::Modeller,
        Answer::Hide => {}
    }
    if a != Answer::Hide {
        crate::calibrate::save(cfg, id, &cal);
    }
    let mut m = cfg
        .get(KEY_BANNER)
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let word = match a {
        Answer::Yes => "yes",
        Answer::No => "no",
        Answer::Hide => "hidden",
    };
    m.insert(id.to_string(), serde_json::json!(word));
    cfg.set(KEY_BANNER, serde_json::Value::Object(m));
}

/// B17: an amp modeller in front of the room's amps (the owner's NUX MG-300 is one): asked once, two answers and a ×
/// (P41.3, D170: it was a question with one answer that never went away).
fn modeller_warning(room: &mut RoomView, ui: &Ui, st: &mut AppState, chain: Rect) -> bool {
    let Some((id, name)) = crate::calibrate::current_input(st) else {
        return false;
    };
    if !banner_wanted(&st.config, &id, &name) || room.rack.board_now(st).blocks.is_empty() {
        return false;
    }
    let th = t();
    let p = ui.painter().clone();
    let short: String = name
        .split('(')
        .next()
        .unwrap_or(&name)
        .trim()
        .chars()
        .take(24)
        .collect();
    let r = Rect::from_min_size(
        pos2(chain.min.x + px(14.0), chain.max.y - px(38.0)),
        vec2(chain.width() - px(28.0), px(30.0)),
    );
    // (dark themes: a dark amber plate, light words; Daylight: a pale amber plate, dark words)
    let (plate, ink) = if th.dark {
        (
            Color32::from_rgba_unmultiplied(0x3a, 0x2a, 0x10, 230),
            Color32::from_rgb(0xf6, 0xe2, 0xb8),
        )
    } else {
        (
            Color32::from_rgb(0xfb, 0xec, 0xc8),
            Color32::from_rgb(0x5a, 0x3b, 0x08),
        )
    };
    p.rect_filled(r, px(8.0), plate);
    p.rect_stroke(
        r,
        px(8.0),
        Stroke::new(1.0, Color32::from_rgb(0xf2, 0xb5, 0x3c)),
        egui::StrokeKind::Inside,
    );
    // (the answers at the right, the question and what Yes means in what's left: the longest words that fit)
    let ls = Style::new(11.5, 700, th.accent_ink);
    let yes = "Yes, use quickRS's amps";
    let no = "No, keep my interface's sound";
    let answers_w =
        text::width(ui.ctx(), yes, ls) + text::width(ui.ctx(), no, ls) + px(14.0 + 18.0 + 18.0 + 24.0);
    let room_w = r.width() - px(24.0) - answers_w;
    let qs = Style::new(11.5, 700, ink);
    let words = [
        format!(
            "Your interface ({short}) already adds an amp sound. Use quickRS's amps instead? Then set it to a clean \
             preset."
        ),
        format!("{short} already adds an amp sound: use quickRS's amps (then set it to a clean preset)?"),
        format!("{short} already adds an amp sound: use quickRS's amps?"),
    ];
    let q = words
        .iter()
        .find(|w| text::width(ui.ctx(), w, qs) <= room_w)
        .unwrap_or(&words[2]);
    let tr = text::draw(
        &p,
        q,
        pos2(r.min.x + px(12.0), r.center().y),
        Align2::LEFT_CENTER,
        qs,
        Some(room_w.max(px(80.0))),
    );
    if ui.rect_contains_pointer(tr) {
        room.rack.tip = Some((
            format!(
                "Yes: quickRS's amps and cabs make the sound; set your {short} to a clean preset (its amp and cab off) \
                 so there aren't two amps. No: songs play your interface's own sound."
            ),
            "asked once".into(),
            pos2(tr.center().x, tr.min.y - px(4.0)),
            Align2::CENTER_BOTTOM,
        ));
    }
    let yr = link(
        ui,
        pos2(tr.max.x + px(14.0), r.center().y),
        "room-dry-input",
        yes,
        Align2::LEFT_CENTER,
        11.5,
    );
    let nr = link(
        ui,
        pos2(yr.rect.max.x + px(18.0), r.center().y),
        "room-keep-input",
        no,
        Align2::LEFT_CENTER,
        11.5,
    );
    let xr = quiet_link(
        ui,
        pos2(r.max.x - px(22.0), r.center().y),
        "room-banner-hide",
        "\u{d7}",
        14.0,
    );
    if xr.hovered() {
        room.rack.tip = Some((
            "Hide this for good (Settings > Sound says what your input is)".into(),
            String::new(),
            pos2(xr.rect.center().x, xr.rect.min.y - px(4.0)),
            Align2::RIGHT_BOTTOM,
        ));
    }
    let answer = if yr.clicked() {
        Some(Answer::Yes)
    } else if nr.clicked() {
        Some(Answer::No)
    } else if xr.clicked() {
        Some(Answer::Hide)
    } else {
        None
    };
    if let Some(a) = answer {
        answer_banner(&mut st.config, &id, a);
        st.save_config();
        match a {
            Answer::Yes => room.say(
                "quickRS's amps make the sound now",
                format!("  \u{b7}  set your {short} to a clean preset (its amp and cab off)"),
            ),
            Answer::No => room.say(
                "Your interface's sound stays",
                "  \u{b7}  songs play it as it is; here, Blank hears it alone",
            ),
            Answer::Hide => room.say(
                "Hidden for good",
                "  \u{b7}  Settings > Sound says what your input is",
            ),
        }
    }
    true
}

/// The six quick controls, each naming the knob it turns.
fn quick(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    card(&p, r);
    let b = room.rack.board_now(st);
    let qs = quick_map(&b);
    let main = quick_main_unit(&qs);
    // (the unit most of them turn, named once: each caption then names only its knob, P37.6c; never cut, P41.3: the
    // longest of these words that fits)
    let hw = r.width() - px(40.0);
    let fits = |s: &str| text::width(&ctx, s, Style::caps(10.5, th.caption)) <= hw;
    let head = match &main {
        Some(u) => {
            let gear = u.split(" \u{b7} ").next().unwrap_or(u);
            [
                format!("QUICK CONTROLS  \u{b7}  {}", u.to_uppercase()),
                format!("QUICK  \u{b7}  {}", u.to_uppercase()),
                u.to_uppercase(),
                format!("QUICK  \u{b7}  {}", gear.to_uppercase()),
                gear.to_uppercase(),
            ]
            .into_iter()
            .find(|s| fits(s))
            .unwrap_or_else(|| gear.to_uppercase())
        }
        None => "QUICK CONTROLS".to_string(),
    };
    if fits(&head) {
        header(&p, r, &head);
    } else {
        // (a name longer than the card: on two lines in smaller caps)
        for (n, l) in text::wrap(&ctx, &head, Style::caps(9.0, th.caption), hw)
            .iter()
            .take(2)
            .enumerate()
        {
            text::caps(
                &p,
                l,
                pos2(r.min.x + px(20.0), r.min.y + px(14.0) + px(12.0) * n as f32),
                9.0,
                th.caption,
                Align2::LEFT_CENTER,
                None,
            );
        }
    }
    let cols = 3;
    // the goal row along the foot: what you want, in words; each turns the right knobs and says which
    let gs = 10.5;
    let gap_g = px(6.0);
    let gw: Vec<f32> = Goal::ALL
        .iter()
        .map(|g| super::draw::chip_w(&ctx, g.label(), false, gs))
        .collect();
    let avail = r.width() - px(24.0);
    let mut goal_rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut run = 0.0;
    for (i, w) in gw.iter().enumerate() {
        if run > 0.0 && run + w > avail {
            goal_rows.push(Vec::new());
            run = 0.0;
        }
        run += w + gap_g;
        goal_rows.last_mut().expect("a row").push(i);
    }
    let said_h = px(16.0);
    let goal_h = goal_rows.len() as f32 * px(28.0) + said_h + px(6.0);
    let gy0 = r.max.y - px(8.0) - goal_h;
    for (row, ids) in goal_rows.iter().enumerate() {
        let mut gx = r.min.x + px(12.0);
        for &i in ids {
            let g = Goal::ALL[i];
            let cr = Rect::from_min_size(pos2(gx, gy0 + row as f32 * px(28.0)), vec2(gw[i], px(24.0)));
            gx = cr.max.x + gap_g;
            let resp = super::draw::chip(ui, cr, ("room-goal", i), g.label(), None, false, gs);
            if resp.hovered() {
                let n = goal_nudges(&b, g);
                room.rack.tip = Some((
                    if n.is_empty() {
                        match g {
                            Goal::MoreSpace => {
                                "Adds a Room reverb (nothing here has an echo or a room yet)".into()
                            }
                            _ => "Nothing on this rig turns that way any further".into(),
                        }
                    } else {
                        n.iter()
                            .map(|x| nudge_words(&b, x))
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    "a click turns it \u{b7} Ctrl Z takes it back".into(),
                    pos2(cr.center().x, cr.min.y - px(4.0)),
                    Align2::CENTER_BOTTOM,
                ));
            }
            if resp.clicked() {
                apply_goal(room, st, g);
            }
        }
    }
    if let Some(s) = &room.rig_ui.goal_said {
        text::draw(
            &p,
            s,
            pos2(r.min.x + px(16.0), r.max.y - px(8.0) - said_h / 2.0),
            Align2::LEFT_CENTER,
            Style::new(10.0, 600, th.caption),
            Some(r.width() - px(32.0)),
        );
    }
    let b = room.rack.board_now(st);
    let inner = Rect::from_min_max(
        pos2(r.min.x + px(12.0), r.min.y + px(36.0)),
        pos2(r.max.x - px(12.0), gy0 - px(4.0)),
    );
    let cw = inner.width() / cols as f32;
    let rh = inner.height() / 2.0;
    let kr = (rh * 0.17).min(cw * 0.2).clamp(px(12.0), px(26.0));
    let shift = ctx.input(|i| i.modifiers.shift);
    let any_down = ctx.input(|i| i.pointer.any_down());
    for (n, q) in qs.iter().enumerate() {
        let c = pos2(
            inner.min.x + cw * ((n % cols) as f32 + 0.5),
            inner.min.y + rh * ((n / cols) as f32 + 0.5) - px(4.0),
        );
        let hit = Rect::from_center_size(c, Vec2::splat(kr * 2.0 + px(22.0)));
        let u = quick_u(&b, q);
        let live = u.is_some();
        let resp = interact(ui, hit, ("room-quick", n), Sense::click_and_drag());
        if live {
            room.rig_ui.qspots.push((hit, n));
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(CursorIcon::ResizeVertical);
                room.rack.tip = Some((
                    format!("{}: {}", q.label, q.says),
                    "drag \u{b7} wheel \u{b7} double-click: its default".into(),
                    pos2(hit.center().x, hit.min.y - px(4.0)),
                    Align2::CENTER_BOTTOM,
                ));
            }
            if resp.double_clicked() {
                let d = match &q.target {
                    Target::Knobs(v) => v.first().and_then(|&(uid, k)| {
                        b.index_of(uid).map(|i| {
                            let kd = b.blocks[i].def().knobs[k];
                            kd.to_u(kd.def)
                        })
                    }),
                    Target::Level => Some((0.0 - LEVEL.0) / (LEVEL.1 - LEVEL.0)),
                    Target::None => None,
                };
                if let Some(d) = d {
                    set_quick(room, st, q, d);
                    room.rack.board_commit(st);
                }
            } else if resp.is_pointer_button_down_on()
                && room.rig_ui.qdrag.is_none()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                room.rig_ui.qdrag = Some((n, pos.y, u.unwrap_or(0.0)));
            }
            if let Some((qn, y0, u0)) = room.rig_ui.qdrag
                && qn == n
            {
                if any_down {
                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        let span = if shift { px(900.0) } else { px(180.0) };
                        let nu = (u0 + (y0 - pos.y) / span).clamp(0.0, 1.0);
                        if (nu - u.unwrap_or(0.0)).abs() > 1e-5 {
                            set_quick(room, st, q, nu);
                        }
                    }
                } else {
                    room.rig_ui.qdrag = None;
                    room.rack.board_commit(st);
                }
            }
        }
        let b2 = room.rack.board_now(st);
        let shown_u = quick_u(&b2, q).unwrap_or(0.0);
        stage::big_knob(
            &p,
            c,
            kr,
            shown_u,
            &q.label.to_uppercase(),
            &quick_text(&b2, q),
            live && (resp.hovered() || resp.dragged()),
            room.rig_ui.qdrag.is_some_and(|d| d.0 == n),
        );
        if !live {
            // (a knob with nothing to turn: dimmed in a disc, not a box)
            p.circle_filled(c, kr + px(9.0), th.base.gamma_multiply(0.6));
        }
        // (never cut, P41.3: two lines, smaller words when it needs them)
        let cap = quick_caption(q, main.as_deref());
        let mut cs = Style::new(10.0, 600, th.caption);
        let mut lines = text::wrap(&ctx, &cap, cs, cw - px(8.0));
        if lines.len() > 2 {
            cs = Style::new(9.0, 600, th.caption);
            lines = text::wrap(&ctx, &cap, cs, cw - px(4.0));
        }
        for (j, l) in lines.iter().take(3).enumerate() {
            text::draw(
                &p,
                l,
                pos2(c.x, c.y + kr + px(38.0) + px(12.0) * j as f32),
                Align2::CENTER_CENTER,
                cs,
                None,
            );
        }
    }
}

/// The open unit's face and its list in the unit's card `r` (P37.13): the face at its own proportions
/// (`faces::unit_size`, which counts every control and label), as big as the card allows up to 2.4 times its size;
/// the list beside it only when it's asked for (`list`, P37.6c: every control is on the face, so the list is a
/// second way to turn them, closed by default), the face has its room and the list at least 230 points.
pub fn unit_rects(r: Rect, blk: &Block, list: bool) -> (Rect, Option<Rect>) {
    let inner = Rect::from_min_max(
        pos2(r.min.x + px(20.0), r.min.y + px(42.0)),
        pos2(r.max.x - px(20.0), r.max.y - px(40.0)),
    );
    let fs = faces::unit_size(blk);
    let list_w = (inner.width() * 0.38).clamp(px(230.0), px(420.0));
    let gap = px(28.0);
    let k_all = (inner.width() / fs.x).min(inner.height() / fs.y).min(2.4);
    let k_side = ((inner.width() - list_w - gap) / fs.x)
        .min(inner.height() / fs.y)
        .min(2.4);
    // (the list goes when it would squeeze the face under 90% of what it could be alone, or under its size)
    let side = list && (k_side >= 1.0 && k_side >= k_all * 0.9 || k_side >= 1.4);
    if side {
        let face_box = Rect::from_min_max(inner.min, pos2(inner.max.x - list_w - gap, inner.max.y));
        let fr = Rect::from_center_size(face_box.center(), fs * k_side);
        let list = Rect::from_min_max(pos2(face_box.max.x + gap, inner.min.y), inner.max);
        (fr, Some(list))
    } else {
        (Rect::from_center_size(inner.center(), fs * k_all), None)
    }
}

/// The open unit's face for a window `w` x `h` (points), as the room lays it out (the layout tests).
#[cfg(test)]
pub fn unit_face_rect(b: &Block, w: f32, h: f32) -> Rect {
    let (_, body, _) = super::room_rects(Rect::from_min_size(Pos2::ZERO, vec2(w, h)));
    unit_rects(page_rects(body).unit, b, false).0
}

/// The open unit: its face (every control turns), its controls as a list, the ways deeper, its actions.
fn open_unit(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    card(&p, r);
    let b = room.rack.board_now(st);
    let Some(i) = room.rack.board.sel.and_then(|u| b.index_of(u)) else {
        header(&p, r, "OPEN UNIT");
        super::draw::label_w(
            &p,
            "Click a unit in the chain to open it, or + to add one.",
            r.center(),
            13.0,
            600,
            th.caption,
            Align2::CENTER_CENTER,
            r.width() - px(40.0),
        );
        return;
    };
    let blk = b.blocks[i];
    let def = blk.def();
    let what = match def.id {
        "amp_nam" => "AMP (NAM CAPTURE)".to_string(),
        "pedal_nam" => "PEDAL (NAM CAPTURE)".to_string(),
        "cab_ir" => "CAB (IMPULSE RESPONSE)".to_string(),
        _ => format!("{} (MODELLED)", def.family().label().to_uppercase()),
    };
    let name = match crate::player::board::Rack::file_name(&blk) {
        Some(n) if def.file => short_capture_name(&n),
        _ => def.name.to_string(),
    };
    let hr = header(
        &p,
        r,
        &format!(
            "OPEN UNIT  \u{b7}  {} OF {}  \u{b7}  {}",
            i + 1,
            b.blocks.len(),
            what
        ),
    );
    // (P41.3: the kind in its caps is a word you can hover)
    let hw = match def.id {
        "amp_nam" | "pedal_nam" => "capture",
        "cab_ir" => "ir",
        _ => def.family().label(),
    };
    super::plain::hover(&mut room.rack.tip, ui, hr, hw);
    // its "i" dot: what it's inspired by, its story, its controls
    let entry = match crate::player::board::Rack::file_name(&blk) {
        Some(_) if def.file && blk.asset != 0 => Entry::Capture(blk.asset),
        _ => Entry::Model(blk.model),
    };
    if super::catalogue::info_dot(ui, pos2(hr.max.x + px(16.0), hr.center().y), "room-unit-i").clicked() {
        room.info = Some(entry);
    }
    // MODELLED / NAM: an explicit switch that says what it turns on and off (amps, when captures are on)
    let sw = Rect::from_min_size(
        pos2(r.max.x - px(20.0) - px(220.0), r.min.y + px(10.0)),
        vec2(px(220.0), px(26.0)),
    );
    if room.rack.amp_switch(ui, st, sw, blk) {
        let b = room.rack.board_now(st);
        let ours = b
            .blocks
            .iter()
            .find(|x| x.def().kind == Kind::Amp && !x.def().file)
            .map(|x| x.def().name);
        let nam = b
            .blocks
            .iter()
            .find(|x| x.def().id == "amp_nam")
            .and_then(crate::player::board::Rack::file_name)
            .map(|n| short_capture_name(&n));
        let says = if def.file {
            format!(
                "MODELLED plays {} and turns this capture off",
                ours.unwrap_or("our first amp")
            )
        } else {
            format!(
                "NAM plays {} and turns {} off",
                nam.as_deref().unwrap_or("a capture (you choose one)"),
                def.name
            )
        };
        // (what the switch does, said on hover: under it, it ran into the list)
        if ui.rect_contains_pointer(sw) {
            room.rack.tip = Some((
                says,
                String::new(),
                pos2(sw.center().x, sw.max.y + px(6.0)),
                Align2::CENTER_TOP,
            ));
        }
    }
    // the face, left (sized from its controls); the list, right when there's room for it
    let (fr, list) = unit_rects(r, &blk, room.rig_ui.list_open);
    let hot = room.rack.unit_face(ui, st, blk, fr, "room-unit");
    if !blk.on {
        paint::wash(&p, fr, Color32::BLACK, 0.35);
    }
    if let Some(list) = list {
        knob_list(room, ui, st, list, blk, hot);
    }
    // what turns it, the ways deeper
    // one row along the foot (P37.13): its actions at the left, quiet; the ways deeper at the right, dropped from
    // the end when the row is short; a capture with no file says so above it
    let ay = r.max.y - px(20.0);
    let x0 = r.min.x + px(20.0);
    if faces::silver::missing(&blk) {
        text::draw(
            &p,
            "No capture loaded: you hear yourself dry. Choose one below.",
            pos2(x0, ay - px(24.0)),
            Align2::LEFT_CENTER,
            Style::new(11.0, 700, th.accent_ink),
            Some(r.width() - px(40.0)),
        );
    }
    let mut acts: Vec<(&str, &str)> = vec![(if blk.on { "On \u{25cf}" } else { "Off \u{25cb}" }, "on")];
    if def.file {
        acts.push((
            if def.kind == Kind::Cab {
                "Choose IR\u{2026}"
            } else {
                "Choose capture\u{2026}"
            },
            "choose",
        ));
    } else {
        acts.push(("Defaults", "defaults"));
    }
    acts.push(("Remove", "remove"));
    acts.push(("Change model \u{25be}", "change"));
    let mut ax = x0;
    let mut hit = None;
    for (s, id) in &acts {
        let resp = quiet_link(ui, pos2(ax, ay), ("room-unit-act", *id), s, 12.0);
        ax = resp.rect.max.x + px(18.0);
        if resp.clicked() {
            hit = Some(*id);
        }
    }
    match hit {
        Some("on") => room.rack.toggle_block(st, blk.uid),
        Some("choose") => room.rack.choose_file(blk),
        Some("defaults") => room.rack.block_defaults(st, blk),
        Some("remove") => room.rack.remove_block(st, blk.uid),
        Some("change") => {
            room.rig_ui.seg = Seg::Gear;
            room.rig_ui.change = Some(blk.uid);
            room.shelf_kind = def.family();
            room.shelf_query.clear();
            room.shelf_page = 0;
        }
        _ => {}
    }
    let mut goes: Vec<(&str, &str, bool)> = vec![(
        if room.rig_ui.list_open {
            "Hide the list"
        } else {
            "Controls as a list \u{203a}"
        },
        "list",
        false,
    )];
    if def.has_back() {
        goes.push(("Behind the panel \u{203a}", "back", true));
    }
    goes.push(("Info and sweet spots \u{203a}", "info", false));
    if !def.file {
        goes.push(("Starting points \u{203a}", "starts", false));
    }
    let stl = Style::new(11.5, 700, th.accent_ink);
    let room_for = r.max.x - px(20.0) - (ax + px(12.0));
    while !goes.is_empty()
        && goes
            .iter()
            .map(|g| text::width(ui.ctx(), g.0, stl) + px(16.0))
            .sum::<f32>()
            > room_for
    {
        goes.pop();
    }
    let mut lx = r.max.x - px(20.0);
    for (s, id, back) in goes.iter().rev() {
        let resp = link(
            ui,
            pos2(lx, ay),
            ("room-unit-go", *id),
            s,
            Align2::RIGHT_CENTER,
            11.5,
        );
        lx = resp.rect.min.x - px(16.0);
        if resp.clicked() && *id == "list" {
            room.rig_ui.list_open = !room.rig_ui.list_open;
        } else if resp.clicked() {
            room.rack.board.back = *back;
            room.rack.board.editing = true;
        }
    }
    let _ = name;
}

/// Whether a choice's options all fit a row of segments `w` wide (else the list shows it as a stepper).
fn choice_fits(ctx: &egui::Context, opts: &[&str], w: f32) -> bool {
    let seg = w / opts.len().max(1) as f32 - px(5.0);
    opts.iter()
        .all(|o| text::width(ctx, o, Style::new(9.5, 800, Color32::WHITE)) <= seg)
}

/// The open unit's controls as rows you drag (the same knobs as its face, plus a capture's facts).
fn knob_list(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect, blk: Block, hot: Option<usize>) {
    let p = ui.painter().clone();
    let th = t();
    let def = blk.def();
    let n = def.front_len().max(1);
    let rh = (r.height() / n as f32).min(px(30.0));
    let label_w = r.width() * 0.3;
    let value_w = r.width() * 0.24;
    let (x0, x1) = (r.min.x + label_w + value_w, r.max.x - px(6.0));
    for k in 0..def.front_len() {
        let kd = def.knobs[k];
        let y = r.min.y + rh * (k as f32 + 0.5);
        let rr = Rect::from_min_max(pos2(r.min.x, y - rh / 2.0), pos2(r.max.x, y + rh / 2.0));
        room.spots.push(stage::Spot {
            rect: rr,
            uid: blk.uid,
            knob: k,
        });
        let resp = interact(ui, rr, ("room-row", blk.uid, k), Sense::click_and_drag());
        let lit = hot == Some(k) || resp.hovered() || resp.dragged();
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if resp.double_clicked() {
            room.rack.set_knob(st, blk.uid, k, kd.def);
            room.rack.board_commit(st);
        } else {
            match kd.unit {
                Unit::Choice(opts) => {
                    if resp.clicked() {
                        // (as a stepper, its left half steps back)
                        let back = !choice_fits(ui.ctx(), opts, x1 - x0)
                            && resp.interact_pointer_pos().is_some_and(|p| p.x < (x0 + x1) / 2.0);
                        let step = if back { -1.0 } else { 1.0 };
                        let v = (blk.v[k].round() + step).rem_euclid(opts.len().max(1) as f32);
                        room.rack.set_knob(st, blk.uid, k, v);
                        room.rack.board_commit(st);
                    }
                }
                _ => {
                    if (resp.dragged() || resp.is_pointer_button_down_on())
                        && let Some(pos) = resp.interact_pointer_pos()
                    {
                        let u = ((pos.x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0);
                        let v = kd.from_u(u);
                        if (v - blk.v[k]).abs() > 1e-6 {
                            room.rack.set_knob(st, blk.uid, k, v);
                        }
                    }
                    if resp.drag_stopped() || resp.clicked() {
                        room.rack.board_commit(st);
                    }
                }
            }
        }
        let now = room.rack.board_now(st);
        let b = now.index_of(blk.uid).map(|i| now.blocks[i]).unwrap_or(blk);
        // (a control the real unit doesn't have says so, P37.6c)
        let shown = if catalog::is_added(def.id, kd.key) {
            format!("{} \u{b7} added", kd.label)
        } else {
            kd.label.to_string()
        };
        text::draw(
            &p,
            &shown,
            pos2(r.min.x, y),
            Align2::LEFT_CENTER,
            Style::new(12.0, 700, if lit { th.text } else { th.text_2 }),
            Some(label_w - px(4.0)),
        );
        // (a choice shows its value in its own segments or stepper)
        let value = if matches!(kd.unit, Unit::Choice(_)) {
            String::new()
        } else {
            faces::silver::knob_text(&b, k)
        };
        text::draw(
            &p,
            &value,
            pos2(x0 - px(10.0), y),
            Align2::RIGHT_CENTER,
            Style::new(12.0, 800, th.text),
            Some(value_w - px(6.0)),
        );
        if let Unit::Choice(opts) = kd.unit
            && !choice_fits(ui.ctx(), opts, x1 - x0)
        {
            // too many words for a row of segments: a stepper, the value between two arrows
            let tr = Rect::from_min_max(pos2(x0, y - px(9.0)), pos2(x1, y + px(9.0)));
            p.rect_filled(tr, px(9.0), th.screen_hi);
            let ink = if lit { th.accent_ink } else { th.text_2 };
            let cur = opts.get(b.v[k].round() as usize).copied().unwrap_or("");
            let arrow = Style::new(13.0, 800, ink);
            text::draw(
                &p,
                "\u{2039}",
                pos2(tr.min.x + px(9.0), y),
                Align2::CENTER_CENTER,
                arrow,
                None,
            );
            text::draw(
                &p,
                "\u{203a}",
                pos2(tr.max.x - px(9.0), y),
                Align2::CENTER_CENTER,
                arrow,
                None,
            );
            text::draw(
                &p,
                cur,
                tr.center(),
                Align2::CENTER_CENTER,
                Style::new(10.0, 800, th.text),
                Some(tr.width() - px(36.0)),
            );
        } else if let Unit::Choice(opts) = kd.unit {
            let w = (x1 - x0) / opts.len().max(1) as f32;
            for (j, o) in opts.iter().enumerate() {
                let or =
                    Rect::from_min_size(pos2(x0 + w * j as f32, y - px(9.0)), vec2(w - px(3.0), px(18.0)));
                let on = b.v[k].round() as usize == j;
                p.rect_filled(
                    or,
                    px(5.0),
                    if on {
                        th.accent.gamma_multiply(0.25)
                    } else {
                        th.screen_hi
                    },
                );
                text::draw(
                    &p,
                    o,
                    or.center(),
                    Align2::CENTER_CENTER,
                    Style::new(9.5, 800, if on { th.accent_ink } else { th.text_2 }),
                    Some(or.width() - px(2.0)),
                );
            }
        } else {
            let g = Rect::from_min_max(pos2(x0, y - px(3.0)), pos2(x1, y + px(3.0)));
            p.rect_filled(g, px(3.0), th.track);
            let u = kd.to_u(b.v[k]);
            p.rect_filled(
                Rect::from_min_max(g.min, pos2(g.min.x + g.width() * u, g.max.y)),
                px(3.0),
                th.accent.gamma_multiply(if lit { 1.0 } else { 0.8 }),
            );
            let c = pos2(g.min.x + g.width() * u, g.center().y);
            p.circle_filled(c, px(6.0), Color32::from_rgb(240, 240, 236));
            p.circle_stroke(c, px(6.0), Stroke::new(1.0, Color32::from_black_alpha(120)));
        }
    }
}

// -- the Sounds drawer ------------------------------------------------------------------------------------------------

fn drawer(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    card(&p, r);
    let hr = header(&p, r, "SOUNDS");
    let n_song = room.rack.board.rig.as_ref().map(|x| x.tones.len()).unwrap_or(0);
    let segs: Vec<(Seg, String)> = [
        (n_song > 0).then(|| (Seg::Song, format!("This song ({n_song})"))),
        Some((Seg::Mine, format!("My sounds ({})", room.saved.len()))),
        Some((Seg::Starters, format!("Starters ({})", board::presets().len()))),
        Some((Seg::AllSongs, "All song tones".to_string())),
        Some((
            Seg::Gear,
            if crate::captures::ON {
                format!("Gear & captures ({})", super::catalogue::gear_total())
            } else {
                format!("Gear ({})", catalog::ours())
            },
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    if room.rig_ui.seg == Seg::Song && n_song == 0 {
        room.rig_ui.seg = Seg::Mine;
    }
    let mut x = hr.max.x + px(20.0);
    let cy = r.min.y + px(20.0);
    for (seg, label) in &segs {
        let w = super::draw::chip_w(&ctx, label, false, 11.5) + px(8.0);
        let cr = Rect::from_min_size(pos2(x, cy - px(13.0)), vec2(w, px(26.0)));
        x = cr.max.x + px(6.0);
        if super::draw::chip(
            ui,
            cr,
            ("room-seg", label.as_str()),
            label,
            None,
            room.rig_ui.seg == *seg,
            11.5,
        )
        .clicked()
        {
            room.rig_ui.seg = *seg;
            if *seg != Seg::Gear {
                room.rig_ui.change = None;
                room.shelf_at = None;
            }
        }
    }
    let inner = Rect::from_min_max(
        pos2(r.min.x + px(16.0), r.min.y + px(42.0)),
        r.max - vec2(px(16.0), px(10.0)),
    );
    match room.rig_ui.seg {
        Seg::Song => song_rows(room, ui, st, inner),
        Seg::Mine => {
            let rows: Vec<(String, Board, Option<super::rigs::Rig>)> = room
                .saved
                .iter()
                .map(|r| (r.name.clone(), r.board.clone(), Some(r.clone())))
                .collect();
            if rows.is_empty() {
                text::draw(
                    &p,
                    "No sounds saved yet: Save (Ctrl S) keeps the rig as one.",
                    inner.left_center() + vec2(px(4.0), 0.0),
                    Align2::LEFT_CENTER,
                    Style::new(12.0, 600, th.caption),
                    None,
                );
            }
            board_rows(room, ui, st, inner, rows, "mine");
        }
        Seg::Starters => {
            if link(
                ui,
                pos2(r.max.x - px(20.0), cy),
                "room-shuffle",
                "Shuffle a rig",
                Align2::RIGHT_CENTER,
                11.0,
            )
            .clicked()
            {
                room.shuffle(st);
            }
            // (bass boards first on a bass input, P37.2)
            let bass = st.instrument().is_bass();
            let mut presets = board::presets();
            presets.sort_by_key(|(n, _)| board::preset_is_bass(n) != bass);
            let rows = presets
                .into_iter()
                .map(|(n, b)| (n.to_string(), b, None))
                .collect();
            board_rows(room, ui, st, inner, rows, "start");
        }
        Seg::AllSongs => {
            let (tones, total) = crate::gearindex::find_tones(&room.shelf_query, 40);
            let rows = tones
                .into_iter()
                .map(|t| {
                    (
                        format!("{} \u{b7} {} \u{b7} {}", t.title, t.part, t.label),
                        t.board,
                        None,
                    )
                })
                .collect();
            board_rows(room, ui, st, inner, rows, "songs");
            if link(
                ui,
                pos2(r.max.x - px(20.0), cy),
                "room-all-sounds",
                &format!("All {total} in the Sounds page (L) \u{203a}"),
                Align2::RIGHT_CENTER,
                11.0,
            )
            .clicked()
            {
                room.go(st, Tab::Sounds);
            }
        }
        Seg::Gear => shelf(room, ui, st, inner),
    }
}

/// Rows of boards (your sounds, starters, song tones from the library): name, chain, Hear and Load.
fn board_rows(
    room: &mut RoomView,
    ui: &Ui,
    st: &mut AppState,
    r: Rect,
    rows: Vec<(String, Board, Option<super::rigs::Rig>)>,
    tag: &'static str,
) {
    let p = ui.painter().clone();
    let th = t();
    let rh = px(34.0);
    let fits = ((r.height() + px(4.0)) / (rh + px(4.0))).floor().max(1.0) as usize;
    let cols = if r.width() > px(1100.0) && rows.len() > fits {
        2
    } else {
        1
    };
    let cw = (r.width() - px(10.0) * (cols as f32 - 1.0)) / cols as f32;
    for (n, (name, b, saved)) in rows.into_iter().take(fits * cols).enumerate() {
        let (c, row) = (n / fits, n % fits);
        let rr = Rect::from_min_size(
            pos2(
                r.min.x + c as f32 * (cw + px(10.0)),
                r.min.y + row as f32 * (rh + px(4.0)),
            ),
            vec2(cw, rh),
        );
        let key = format!("{tag}:{name}");
        let hearing =
            room.rig_ui.hearing.as_deref() == Some(key.as_str()) && room.rack.board.audition.is_some();
        paint::inset(&p, rr, Some(px(9.0)), 1.0, paint::Fill::Base);
        if hearing {
            p.rect_stroke(rr, px(9.0), Stroke::new(1.5, th.accent), egui::StrokeKind::Inside);
        }
        let nr = text::draw(
            &p,
            &name,
            pos2(rr.min.x + px(14.0), rr.center().y),
            Align2::LEFT_CENTER,
            Style::new(12.5, 800, th.text),
            Some(cw * 0.34),
        );
        // (P41.3: the rig in plain words, not its units' short marks)
        let chain = match (tag, super::sounds::starter_line(&name)) {
            ("start", Some(l)) => format!("{l} \u{b7} {}", super::plain::chain_words(&b)),
            _ => super::plain::chain_words(&b),
        };
        text::draw(
            &p,
            &chain,
            pos2(nr.max.x + px(14.0), rr.center().y),
            Align2::LEFT_CENTER,
            Style::new(10.5, 600, th.caption),
            Some((rr.max.x - px(130.0) - nr.max.x - px(14.0)).max(0.0)),
        );
        let lr = link(
            ui,
            pos2(rr.max.x - px(14.0), rr.center().y),
            ("row-load", tag, n),
            "Load",
            Align2::RIGHT_CENTER,
            11.5,
        );
        let hr = link(
            ui,
            pos2(lr.rect.min.x - px(14.0), rr.center().y),
            ("row-hear", tag, n),
            if hearing { "Stop" } else { "Hear" },
            Align2::RIGHT_CENTER,
            11.5,
        );
        if hr.clicked() {
            if hearing {
                room.rack.board.audition = None;
                room.rig_ui.hearing = None;
            } else {
                room.rack.board.audition = Some(b.renewed());
                room.rig_ui.hearing = Some(key.clone());
            }
            room.rack.board_send(st);
        }
        if lr.clicked() {
            room.rig_ui.hearing = None;
            match saved {
                Some(rig) => room.load_saved(st, &rig),
                None => room.load(st, b, Default::default(), None, name.clone()),
            }
        }
    }
}

/// What a song tone plays on, for its row (P38.2, NAM builds): " · NAM" (a capture on), " · MODELLED", and a
/// capture whose file is missing (it plays the model of ours instead).
pub(crate) fn tone_source(b: &Board) -> String {
    if !crate::captures::ON {
        return String::new();
    }
    let nam = b.blocks.iter().filter(|x| x.on && x.def().file).count();
    match (nam, crate::captures::missing_on(b)) {
        (_, 1) => " \u{b7} a capture missing \u{b7} modelled".into(),
        (_, m) if m > 1 => format!(" \u{b7} {m} captures missing \u{b7} modelled"),
        (0, _) => " \u{b7} MODELLED".into(),
        _ => " \u{b7} NAM".into(),
    }
}

/// This song's tones: where each plays, Hear, Load (play it), RS gear (song tone explained).
fn song_rows(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    let Some(rig) = room.rack.board.rig.clone() else {
        return;
    };
    let rh = px(34.0);
    let playing = match room.rack.board.pick {
        Some(Pick::Tone(i)) => Some(i),
        Some(Pick::Mine) => None,
        None => Some(room.rack.board.song_tone),
    };
    for (i, tone) in rig.tones.iter().enumerate() {
        let rr = Rect::from_min_size(
            pos2(r.min.x, r.min.y + i as f32 * (rh + px(4.0))),
            vec2(r.width(), rh),
        );
        if rr.max.y > r.max.y + px(2.0) {
            break;
        }
        paint::inset(&p, rr, Some(px(9.0)), 1.0, paint::Fill::Base);
        if playing == Some(i) {
            p.rect_stroke(rr, px(9.0), Stroke::new(1.5, th.accent), egui::StrokeKind::Inside);
        }
        // (P38.2: what plays it: our models, or NAM captures where the library has a match)
        let label = format!(
            "{}{}{}{}",
            tone.label.to_uppercase(),
            if tone.edited { " \u{b7} yours" } else { "" },
            tone_source(&tone.board),
            if playing == Some(i) {
                "  \u{b7}  playing"
            } else {
                ""
            }
        );
        let nr = text::draw(
            &p,
            &label,
            pos2(rr.min.x + px(14.0), rr.center().y),
            Align2::LEFT_CENTER,
            Style::new(12.5, 800, th.text),
            Some(r.width() * 0.25),
        );
        let chain = super::plain::chain_words(&tone.board);
        text::draw(
            &p,
            &chain,
            pos2(nr.max.x + px(14.0), rr.center().y),
            Align2::LEFT_CENTER,
            Style::new(10.5, 600, th.caption),
            Some((r.width() * 0.5).max(0.0)),
        );
        let end = room.rack.song.btimes.last().copied().unwrap_or(0.0);
        let from = tone_spans(&rig, end)
            .iter()
            .find(|c| c.2 == i)
            .map(|c| c.0)
            .unwrap_or(0.0);
        text::draw(
            &p,
            &format!("from {}:{:02}", (from / 60.0) as u32, (from % 60.0) as u32),
            pos2(rr.max.x - px(300.0), rr.center().y),
            Align2::RIGHT_CENTER,
            Style::new(10.5, 600, th.caption),
            None,
        );
        let gr = link(
            ui,
            pos2(rr.max.x - px(14.0), rr.center().y),
            ("song-gear", i),
            "RS gear \u{203a}",
            Align2::RIGHT_CENTER,
            11.5,
        );
        let lr = link(
            ui,
            pos2(gr.rect.min.x - px(14.0), rr.center().y),
            ("song-load", i),
            "Load",
            Align2::RIGHT_CENTER,
            11.5,
        );
        let hr = link(
            ui,
            pos2(lr.rect.min.x - px(14.0), rr.center().y),
            ("song-hear", i),
            "Hear",
            Align2::RIGHT_CENTER,
            11.5,
        );
        if gr.clicked() {
            room.rig_ui.explain = Some(i);
        }
        if lr.clicked() {
            room.rack.board.pick = Some(Pick::Tone(i));
            room.rack.board.sel = None;
        }
        if hr.clicked() {
            room.rack.board.audition = Some(tone.board.renewed());
            room.rig_ui.hearing = Some(format!("song:{i}"));
            room.rack.board_send(st);
        }
    }
}

// -- the shelf --------------------------------------------------------------------------------------------------------

/// The families in the order a player looks for them (bass: the bass rig's order first, BASS-GEAR.md section 4).
pub fn family_order(bass: bool) -> Vec<Kind> {
    if bass {
        vec![
            Kind::Dynamics,
            Kind::Drive,
            Kind::Amp,
            Kind::Cab,
            Kind::Eq,
            Kind::Filter,
            Kind::Modulation,
            Kind::Fuzz,
            Kind::Delay,
            Kind::Reverb,
        ]
    } else {
        Kind::ALL.to_vec()
    }
}

/// A bass unit of ours (its id says so, or it's the bass cabs and the octaver).
pub fn is_bass_model(id: &str) -> bool {
    id.contains("bass")
        || matches!(
            id,
            "amp_b15"
                | "cab_8x10"
                | "cab_1x15"
                | "cab_4x10"
                | "cab_2x10"
                | "cab_4x15"
                | "octaver"
                | "env_filter"
        )
}

/// The shelf's cells for a family (or a search): our models first (bass ones first on a bass), then your captures
/// of the family (`mode`), each family's captures sorted bass-first on a bass, then by name. Every model of ours is
/// in its family (`every_amp_is_two_clicks_away`).
pub fn shelf_cells(kind: Kind, q: &str, mode: ShelfMode, bass: bool) -> Vec<Entry> {
    let mut models: Vec<usize> = if q.is_empty() {
        super::rigs::shelf(kind)
            .into_iter()
            .filter(|&m| !MODELS[m].file)
            .collect()
    } else {
        (0..MODELS.len())
            .filter(|&i| {
                let m = &MODELS[i];
                !m.file
                    && (m.name.to_lowercase().contains(q)
                        || catalog::brand(m.id).to_lowercase().contains(q)
                        || m.after.to_lowercase().contains(q)
                        || m.family().label().to_lowercase().contains(q))
            })
            .collect()
    };
    // (the instrument's own gear first: on a bass the bass gear; on a guitar the guitar gear, so P41.2's ten guitar
    // amps fill the shelf's first page and the bass heads follow)
    models.sort_by_key(|&m| is_bass_model(MODELS[m].id) != bass);
    let mut out: Vec<Entry> = Vec::new();
    if mode != ShelfMode::Captures {
        out.extend(models.into_iter().map(Entry::Model));
    }
    if mode != ShelfMode::Models && crate::captures::ON {
        let caps: Vec<Entry> = crate::captures::with(|l| {
            let mut v: Vec<&crate::captures::Item> = l
                .items
                .iter()
                .filter(|i| {
                    if q.is_empty() {
                        stage::capture_family(i) == kind
                    } else {
                        i.name.to_lowercase().contains(q)
                            || i.gear.as_deref().is_some_and(|g| g.to_lowercase().contains(q))
                    }
                })
                .collect();
            v.sort_by(|a, b| {
                (bass != a.bass)
                    .cmp(&(bass != b.bass))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            v.into_iter().map(|i| Entry::Capture(i.id)).collect()
        });
        out.extend(caps);
        // (an empty file block to load your own .nam / .wav into)
        if q.is_empty() {
            out.extend(
                catalog::files()
                    .filter(|(_, m)| m.kind == kind)
                    .map(|(i, _)| Entry::Model(i)),
            );
        }
    }
    out
}

fn shelf(room: &mut RoomView, ui: &Ui, st: &mut AppState, r: Rect) {
    let p = ui.painter().clone();
    let th = t();
    let ctx = ui.ctx().clone();
    let bass = st.instrument().is_bass();
    // the families, the mode, the search (one line)
    let fy = r.min.y + px(12.0);
    let mut x = r.min.x;
    let q = room.shelf_query.trim().to_lowercase();
    for kind in family_order(bass) {
        let w = super::draw::chip_w(&ctx, kind.label(), true, 11.0);
        let cr = Rect::from_min_size(pos2(x, fy - px(12.0)), vec2(w, px(24.0)));
        x = cr.max.x + px(4.0);
        if super::draw::chip(
            ui,
            cr,
            ("room-fam", kind.label()),
            kind.label(),
            Some(faces::kind_colour(kind)),
            q.is_empty() && room.shelf_kind == kind,
            11.0,
        )
        .clicked()
        {
            room.shelf_kind = kind;
            room.shelf_query.clear();
            room.typing = None;
            room.shelf_page = 0;
        }
    }
    let sr = Rect::from_min_size(
        pos2(r.max.x - px(170.0), fy - px(12.0)),
        vec2(px(170.0), px(24.0)),
    );
    let typing = room.typing == Some(Typing::Shelf);
    let resp = interact(ui, sr, "room-shelf-find", Sense::click());
    paint::inset(&p, sr, Some(px(9.0)), 1.5, paint::Fill::Base);
    if typing {
        p.rect_stroke(sr, px(9.0), Stroke::new(1.5, th.accent), egui::StrokeKind::Inside);
    }
    super::draw::icon_search(&p, pos2(sr.min.x + px(14.0), sr.center().y), px(12.0), th.caption);
    let (qt, qc) = if room.shelf_query.is_empty() && !typing {
        ("Find any unit".to_string(), th.caption)
    } else {
        (room.shelf_query.clone(), th.text)
    };
    let qr = super::draw::label_w(
        &p,
        &qt,
        pos2(sr.min.x + px(27.0), sr.center().y),
        11.5,
        700,
        qc,
        Align2::LEFT_CENTER,
        sr.width() - px(34.0),
    );
    if typing {
        let cx = if room.shelf_query.is_empty() {
            sr.min.x + px(27.0)
        } else {
            qr.max.x
        };
        super::draw::caret(&p, cx, sr.center().y, px(13.0), th.accent);
    }
    if resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::Text);
    }
    if resp.clicked() {
        room.typing = Some(Typing::Shelf);
    }
    let mut mx = sr.min.x - px(12.0);
    if crate::captures::ON {
        let modes = [
            (ShelfMode::Models, "Models"),
            (ShelfMode::Captures, "Captures"),
            (ShelfMode::Both, "Both"),
        ];
        let mw: f32 = modes
            .iter()
            .map(|m| super::draw::chip_w(&ctx, m.1, false, 10.5) + px(4.0))
            .sum();
        let mut sx = mx - mw;
        for (m, l) in modes {
            let w = super::draw::chip_w(&ctx, l, false, 10.5);
            let cr = Rect::from_min_size(pos2(sx, fy - px(12.0)), vec2(w, px(24.0)));
            sx = cr.max.x + px(4.0);
            if super::draw::chip(
                ui,
                cr,
                ("room-shelf-mode", l),
                l,
                None,
                room.rig_ui.shelf == m,
                10.5,
            )
            .clicked()
            {
                room.rig_ui.shelf = m;
                room.shelf_page = 0;
            }
        }
        mx -= mw + px(12.0);
        let sg = link(
            ui,
            pos2(mx, fy),
            "room-suggest",
            "Suggested captures \u{203a}",
            Align2::RIGHT_CENTER,
            11.0,
        );
        if sg.clicked() {
            room.rig_ui.suggest = !room.rig_ui.suggest;
        }
    }
    let what = match (room.rig_ui.change, room.shelf_at) {
        (Some(_), _) => "click one: it replaces the open unit".to_string(),
        (None, Some(at)) => format!("click one: it goes in at {} \u{b7} Esc cancels", at + 1),
        _ => "click one: it goes in after the open unit".into(),
    };
    text::draw(
        &p,
        &what,
        pos2(r.min.x, r.max.y - px(6.0)),
        Align2::LEFT_CENTER,
        Style::new(
            10.5,
            600,
            if room.rig_ui.change.is_some() || room.shelf_at.is_some() {
                th.accent_ink
            } else {
                th.caption
            },
        ),
        Some(r.width() * 0.6),
    );
    let grid = Rect::from_min_max(pos2(r.min.x, fy + px(20.0)), pos2(r.max.x, r.max.y - px(16.0)));
    if room.rig_ui.suggest {
        suggestions(room, ui, grid, st.instrument());
        return;
    }
    let cells = shelf_cells(room.shelf_kind, &q, room.rig_ui.shelf, bass);
    if cells.is_empty() {
        text::draw(
            &p,
            if crate::captures::ON && !crate::captures::is_ready() {
                "Loading your captures\u{2026}"
            } else {
                "Nothing by that name"
            },
            grid.left_center(),
            Align2::LEFT_CENTER,
            Style::new(12.0, 600, th.caption),
            None,
        );
    }
    let cell = vec2(px(84.0), (grid.height()).clamp(px(60.0), px(120.0)));
    let gap = px(6.0);
    let fits = ((grid.width() - px(40.0) + gap) / (cell.x + gap))
        .floor()
        .max(1.0) as usize;
    let pages = cells.len().div_ceil(fits).max(1);
    room.shelf_page = room.shelf_page.min(pages - 1);
    let b = room.rack.board_now(st);
    let mut pick: Option<Entry> = None;
    // (on a bass the bass gear comes first: a line and "GUITAR GEAR" where the rest starts, P37.6c)
    let is_bass_entry = |e: &Entry| match *e {
        Entry::Model(i) => is_bass_model(MODELS[i].id),
        Entry::Capture(id) => crate::captures::with(|l| l.get(id).is_some_and(|i| i.bass)),
    };
    let first_guitar = if bass && q.is_empty() {
        cells
            .iter()
            .position(|e| !is_bass_entry(e))
            .filter(|&n| n > 0 && cells[..n].iter().all(is_bass_entry))
    } else {
        None
    };
    for (j, &m) in cells.iter().skip(room.shelf_page * fits).take(fits).enumerate() {
        let cr = Rect::from_min_size(pos2(grid.min.x + j as f32 * (cell.x + gap), grid.min.y), cell);
        if first_guitar == Some(room.shelf_page * fits + j) {
            let x = cr.min.x - gap * 0.5;
            p.line_segment(
                [pos2(x, grid.min.y + px(4.0)), pos2(x, grid.max.y - px(4.0))],
                Stroke::new(px(1.0), th.caption.gamma_multiply(0.6)),
            );
            text::caps(
                &p,
                "GUITAR GEAR",
                pos2(x + px(6.0), grid.min.y + px(2.0)),
                8.5,
                th.caption,
                Align2::LEFT_TOP,
                None,
            );
        }
        let resp = interact(ui, cr, ("room-cell", m.key()), Sense::click());
        let blk = m.block();
        let in_rig = b
            .blocks
            .iter()
            .any(|x| x.model == blk.model && x.asset == blk.asset);
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            p.rect_filled(cr, px(10.0), th.base_hi.gamma_multiply(0.6));
            room.rack.tip = Some((
                m.about(),
                if room.rig_ui.change.is_some() {
                    "click: put it in place of the open unit"
                } else {
                    "click: add it"
                }
                .into(),
                pos2(cr.center().x, cr.min.y - px(6.0)),
                Align2::CENTER_BOTTOM,
            ));
        }
        let face_h = cell.y - px(34.0);
        let fs = faces::board_size(&blk);
        let k = ((cell.x - px(8.0)) / fs.x).min(face_h / fs.y);
        let fr = Rect::from_min_size(
            pos2(
                cr.center().x - fs.x * k / 2.0,
                cr.min.y + px(2.0) + face_h - fs.y * k,
            ),
            fs * k,
        );
        faces::miniature(&p, fr, &blk, 0.0);
        let (name, chip) = match m {
            Entry::Model(i) => (MODELS[i].name.to_string(), None),
            Entry::Capture(id) => {
                let it = crate::captures::with(|l| l.get(id).cloned());
                let skin = it
                    .as_ref()
                    .and_then(|i| i.skin.as_deref())
                    .and_then(catalog::find)
                    .map(|s| format!("sounds like {}", MODELS[s].name));
                (short_capture_name(&it.map(|i| i.name).unwrap_or_default()), skin)
            }
        };
        text::draw(
            &p,
            &name,
            pos2(cr.center().x, cr.max.y - px(22.0)),
            Align2::CENTER_CENTER,
            Style::new(
                10.0,
                700,
                if in_rig || resp.hovered() {
                    th.text
                } else {
                    th.text_2
                },
            ),
            Some(cell.x),
        );
        if let Some(c) = chip {
            text::draw(
                &p,
                &c,
                pos2(cr.center().x, cr.max.y - px(9.0)),
                Align2::CENTER_CENTER,
                Style::new(8.5, 600, th.caption),
                Some(cell.x),
            );
        }
        if in_rig {
            p.circle_filled(pos2(cr.max.x - px(8.0), cr.min.y + px(8.0)), px(3.5), th.accent);
        }
        if resp.clicked() {
            pick = Some(m);
        }
    }
    if pages > 1 {
        let kx = grid.max.x - px(30.0);
        for (k, (l, d)) in [("\u{2039}", -1i32), ("\u{203a}", 1)].iter().enumerate() {
            let kr = Rect::from_min_size(pos2(kx, grid.min.y + k as f32 * px(34.0)), Vec2::splat(px(28.0)));
            let can = if *d < 0 {
                room.shelf_page > 0
            } else {
                room.shelf_page + 1 < pages
            };
            if widgets::round_button(
                ui,
                kr,
                ("room-page", k),
                if *d < 0 {
                    qrs_ui::icons::Icon::Left
                } else {
                    qrs_ui::icons::Icon::Right
                },
                10.0,
                widgets::RoundKind::Ivory,
                false,
            )
            .clicked()
                && can
            {
                room.shelf_page = (room.shelf_page as i32 + d) as usize;
            }
            let _ = l;
        }
        text::draw(
            &p,
            &format!("{} of {} \u{b7} {} here", room.shelf_page + 1, pages, cells.len()),
            pos2(r.max.x, r.max.y - px(6.0)),
            Align2::RIGHT_CENTER,
            Style::new(10.5, 600, th.caption),
            None,
        );
    }
    if let Some(m) = pick {
        place(room, st, m, &b);
    }
}

/// Put a shelf cell in: in place of the unit "Change model" opened it for, else where the + said, else after the
/// open unit. One edit, one undo step (B14).
/// The catalogue's way in to `place`.
pub(super) fn place_entry(room: &mut RoomView, st: &mut AppState, m: Entry, b: &Board) {
    place(room, st, m, b);
}

fn place(room: &mut RoomView, st: &mut AppState, m: Entry, b: &Board) {
    if let Some(uid) = room.rig_ui.change.take() {
        match m {
            Entry::Model(model) => {
                let moved = room.rack.change_model(st, uid, model);
                if moved.abs() >= 0.25 {
                    room.say(
                        format!("Level matched: {:+.1} dB", moved),
                        format!(
                            "  \u{b7}  {} plays {} at these settings",
                            MODELS[model].name,
                            if moved < 0.0 { "louder" } else { "quieter" }
                        ),
                    );
                }
            }
            Entry::Capture(id) => {
                let kind = crate::captures::with(|l| l.get(id).map(|i| i.kind));
                let same = b.index_of(uid).is_some_and(|i| {
                    b.blocks[i].def().file
                        && Some(crate::player::board::captures::kind_of(&b.blocks[i])) == kind
                });
                if same {
                    room.rack.set_asset(st, uid, id);
                } else if let Some(i) = b.index_of(uid) {
                    room.rack.remove_block(st, uid);
                    room.rack.add_capture(st, i, id);
                }
            }
        }
        room.typing = None;
        return;
    }
    let at = room.shelf_at.take().unwrap_or_else(|| {
        room.rack
            .board
            .sel
            .and_then(|u| b.index_of(u))
            .map(|i| i + 1)
            .unwrap_or(b.blocks.len())
    });
    match m {
        Entry::Capture(id) => room.rack.add_capture(st, at, id),
        Entry::Model(model) => room.rack.add_block(st, at, model),
    }
    room.typing = None;
}

/// Suggested captures: the gear your songs use most, each a link to TONE3000 (your account, your download).
fn suggestions(room: &mut RoomView, ui: &Ui, r: Rect, inst: qrs_analysis::instrument::Instrument) {
    let p = ui.painter().clone();
    let th = t();
    let (list, covered, _) = crate::captures::suggested_captures(12, inst);
    if list.is_empty() {
        text::draw(
            &p,
            if crate::captures::suggest::SHOWN {
                "Your songs' gear is still being read (or none is known yet): try again in a moment."
            } else {
                "Pick any capture on TONE3000: an amp's Captures, Pick on TONE3000\u{2026}"
            },
            r.left_center(),
            Align2::LEFT_CENTER,
            Style::new(12.0, 600, th.caption),
            None,
        );
        return;
    }
    text::draw(
        &p,
        &format!(
            "The gear your songs use most (these cover {covered} of your songs). A click copies its search words and \
             opens TONE3000, where you get it with your own account; quickRS never downloads it for you. Add the file \
             here afterwards (Choose capture\u{2026})."
        ),
        pos2(r.min.x, r.min.y + px(8.0)),
        Align2::LEFT_CENTER,
        Style::new(10.5, 600, th.caption),
        Some(r.width()),
    );
    let cols = 3;
    let cw = r.width() / cols as f32;
    let rh = px(22.0);
    let fits = ((r.height() - px(22.0)) / rh).floor().max(1.0) as usize;
    for (n, s) in list.iter().take(fits * cols).enumerate() {
        let (c, row) = (n / fits, n % fits);
        let y = r.min.y + px(30.0) + row as f32 * rh;
        let x = r.min.x + c as f32 * cw;
        let like = catalog::find(s.unit.skin)
            .map(|m| format!(" (sounds like {})", MODELS[m].name))
            .unwrap_or_default();
        let label = format!(
            "{}{like}  \u{b7}  {} song{}{}",
            s.unit.name,
            s.songs,
            if s.songs == 1 { "" } else { "s" },
            if s.have { "  \u{b7}  in your library" } else { "" }
        );
        if link(ui, pos2(x, y), ("suggest", n), &label, Align2::LEFT_CENTER, 11.0).clicked() {
            ui.ctx().copy_text(s.unit.query.to_string());
            let url = crate::captures::suggest::search_url(s.unit.query);
            if !crate::drive::intercept_url(&url) {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            room.say(
                format!("Opened TONE3000 for {}", s.unit.name),
                "  \u{b7}  its search words are copied",
            );
        }
    }
}

// -- song tone explained (mock 2) ----------------------------------------------------------------------------------------

fn explain(room: &mut RoomView, ui: &Ui, st: &mut AppState, area: Rect, i: usize) {
    let p = ui.painter().clone();
    let th = t();
    let Some(rig) = room.rack.board.rig.clone() else {
        room.rig_ui.explain = None;
        return;
    };
    let Some(slot) = rig.tones.get(i).cloned() else {
        room.rig_ui.explain = None;
        return;
    };
    paint::dim(&p, area, Some(230));
    let panel = area.shrink(px(10.0));
    paint::floating(&p, panel, Some(22.0));
    let close = interact(ui, area, "explain-back", Sense::click());
    let inner = panel.shrink(px(24.0));
    // the head: which tone, where it plays, the other tones, Hear it, Load into my rig
    super::draw::caps(
        &p,
        &format!(
            "SONG TONE  \u{b7}  {}  \u{b7}  {}",
            room.rack.song.title.to_uppercase(),
            room.rack.song.part_name.to_uppercase()
        ),
        pos2(inner.min.x, inner.min.y + px(6.0)),
        10.5,
        th.caption,
        Align2::LEFT_CENTER,
    );
    let end = room.rack.song.btimes.last().copied().unwrap_or(0.0);
    let spans = tone_spans(&rig, end);
    let mmss = |t: f64| format!("{}:{:02}", (t / 60.0) as u32, (t % 60.0) as u32);
    let used: Vec<String> = spans
        .iter()
        .filter(|s| s.2 == i)
        .map(|s| format!("{}-{}", mmss(s.0), mmss(s.1)))
        .collect();
    text::draw(
        &p,
        &format!(
            "{}  \u{b7}  {}",
            slot.label.to_uppercase(),
            if used.is_empty() {
                "the song's base tone".to_string()
            } else {
                format!("used {}", used.join(", "))
            }
        ),
        pos2(inner.min.x, inner.min.y + px(32.0)),
        Align2::LEFT_CENTER,
        Style::new(20.0, 800, th.text),
        Some(inner.width() * 0.55),
    );
    let mut kx = inner.max.x;
    let closer = Rect::from_min_size(pos2(kx - px(30.0), inner.min.y + px(4.0)), Vec2::splat(px(30.0)));
    if widgets::round_button(
        ui,
        closer,
        "explain-close",
        qrs_ui::icons::Icon::XBold,
        11.0,
        widgets::RoundKind::Ivory,
        false,
    )
    .clicked()
    {
        room.rig_ui.explain = None;
        return;
    }
    kx = closer.min.x - px(16.0);
    let lr = link(
        ui,
        pos2(kx, inner.min.y + px(19.0)),
        "explain-load",
        "Load into my rig",
        Align2::RIGHT_CENTER,
        12.0,
    );
    if lr.clicked() {
        let b = slot.board.clone();
        room.rack.board.pick = Some(Pick::Mine);
        room.load(
            st,
            b,
            Default::default(),
            None,
            format!("{} \u{b7} {}", room.rack.song.title, slot.label),
        );
        room.rig_ui.explain = None;
        return;
    }
    let hr = link(
        ui,
        pos2(lr.rect.min.x - px(18.0), inner.min.y + px(19.0)),
        "explain-hear",
        "Hear it",
        Align2::RIGHT_CENTER,
        12.0,
    );
    if hr.clicked() {
        room.rack.board.audition = Some(slot.board.renewed());
        room.rack.board_send(st);
    }
    // RS's chain, slot by slot: what it becomes, what you hear
    let tone = room.rack.song.arr.rig.iter().find(|t| t.key == slot.key).cloned();
    let top = inner.min.y + px(60.0);
    let cols = [
        inner.min.x,
        inner.min.x + inner.width() * 0.36,
        inner.min.x + inner.width() * 0.62,
    ];
    for (c, h) in cols
        .iter()
        .zip(["THE RS CHAIN", "BECOMES", "WHAT YOU HEAR \u{b7} CLICK TO OPEN"])
    {
        super::draw::caps(&p, h, pos2(*c, top), 10.0, th.caption, Align2::LEFT_CENTER);
    }
    let rh = px(46.0);
    let mut y = top + px(14.0);
    let board_now = slot.board.clone();
    if let Some(tone) = &tone {
        for g in &tone.gear {
            if y + rh > inner.max.y - px(120.0) {
                break;
            }
            let rr = Rect::from_min_size(pos2(inner.min.x, y), vec2(inner.width(), rh - px(6.0)));
            paint::inset(&p, rr, Some(px(9.0)), 1.0, paint::Fill::Base);
            let slot_name = slot_words(&g.slot);
            let knobs = g
                .knobs
                .iter()
                .take(4)
                .map(|(k, v)| format!("{k} {v:.0}"))
                .collect::<Vec<_>>()
                .join(" \u{b7} ");
            // (a friendly name, never RS's raw id: the real unit with brands on (D42/D58), else the model of
            // ours it becomes, else what kind of slot it is)
            let ours = qrs_audio::board::rs_gear::blocks_for(g, tone)
                .and_then(|v| v.first().map(|b| b.def().name.to_string()));
            let gear_name = if catalog::SHOW_BRANDS {
                qrs_audio::board::rs_gear::real_gear(&g.key)
                    .map(str::to_string)
                    .or_else(|| ours.clone())
                    .unwrap_or_else(|| slot_name.to_string())
            } else {
                ours.unwrap_or_else(|| slot_name.to_string())
            };
            super::draw::caps(
                &p,
                &slot_name.to_uppercase(),
                pos2(cols[0] + px(12.0), rr.min.y + px(11.0)),
                8.5,
                th.caption,
                Align2::LEFT_CENTER,
            );
            text::draw(
                &p,
                &format!(
                    "{gear_name}{}{knobs}",
                    if knobs.is_empty() { "" } else { " \u{b7} " }
                ),
                pos2(cols[0] + px(12.0), rr.min.y + px(27.0)),
                Align2::LEFT_CENTER,
                Style::new(11.0, 600, th.text),
                Some(cols[1] - cols[0] - px(20.0)),
            );
            let ours = board::rs_gear::blocks_for(g, tone).unwrap_or_default();
            let becomes = if ours.is_empty() {
                // (only an acoustic or bass part's DI and PA land here: an electric one gets an amp and cab, D141)
                "flat on purpose (a DI or PA, as the song has it)".to_string()
            } else {
                ours.iter()
                    .map(|b| b.def().kind.label())
                    .collect::<Vec<_>>()
                    .join(" + ")
            };
            text::draw(
                &p,
                &format!("{becomes} \u{2192}"),
                pos2(cols[1], rr.center().y),
                Align2::LEFT_CENTER,
                Style::new(11.5, 700, th.text_2),
                Some(cols[2] - cols[1] - px(12.0)),
            );
            // what plays: the song board's block for it (a capture when the song uses NAM)
            // (P38.2: a NAM board keeps our unit, off, beside its capture: the one that's on is what you hear)
            let heard = ours.first().and_then(|o| {
                let same = |x: &&Block| x.model == o.model;
                let file = |x: &&Block| x.def().file && x.def().kind == o.def().kind;
                let bs = &board_now.blocks;
                bs.iter()
                    .filter(|x| x.on)
                    .find(same)
                    .or_else(|| bs.iter().filter(|x| x.on).find(file))
                    .or_else(|| bs.iter().find(same))
                    .or_else(|| bs.iter().find(file))
                    .copied()
            });
            if let Some(h) = heard {
                let (nm, how) = if h.def().file {
                    (
                        crate::player::board::Rack::file_name(&h)
                            .map(|n| short_capture_name(&n))
                            .unwrap_or_default(),
                        // (P41.1, D164: why this capture)
                        crate::captures::auto::why_of(&slot.key, h.asset)
                            .map(|w| format!("NAM capture: {w}"))
                            .unwrap_or_else(|| "NAM capture, picked for this gear".to_string()),
                    )
                } else {
                    (
                        h.def().name.to_string(),
                        if g.slot == "Amp" {
                            "model, knobs from the song".into()
                        } else {
                            "model, settings from the song".into()
                        },
                    )
                };
                let hr = Rect::from_min_max(pos2(cols[2], rr.min.y), rr.max);
                let resp = interact(ui, hr, ("explain-open", h.uid), Sense::click());
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    p.rect_stroke(rr, px(9.0), Stroke::new(1.2, th.accent), egui::StrokeKind::Inside);
                }
                text::draw(
                    &p,
                    &nm,
                    pos2(cols[2], rr.min.y + px(13.0)),
                    Align2::LEFT_CENTER,
                    Style::new(12.5, 800, th.text),
                    Some(inner.max.x - cols[2] - px(30.0)),
                );
                text::draw(
                    &p,
                    &how,
                    pos2(cols[2], rr.min.y + px(29.0)),
                    Align2::LEFT_CENTER,
                    Style::new(10.0, 600, th.caption),
                    None,
                );
                text::draw(
                    &p,
                    "\u{203a}",
                    pos2(rr.max.x - px(14.0), rr.center().y),
                    Align2::RIGHT_CENTER,
                    Style::new(14.0, 800, th.accent_ink),
                    None,
                );
                if resp.clicked() {
                    room.rack.board.pick = Some(Pick::Tone(i));
                    room.rack.board.sel = Some(h.uid);
                    room.rig_ui.explain = None;
                    return;
                }
            }
            y += rh;
        }
        text::draw(
            &p,
            &format!(
                "Volume: RS tone level {:+.0} dB \u{2192} sound level {:+.1} dB (matched by loudness). Swap any unit for a model or a capture; Reset to song puts RS's mapping back.",
                tone.volume_db, board_now.level_db
            ),
            pos2(inner.min.x, y + px(10.0)),
            Align2::LEFT_CENTER,
            Style::new(10.5, 600, th.caption),
            Some(inner.width()),
        );
    }
    // where the tones play, and follow / hold / always my sound
    let ty = inner.max.y - px(84.0);
    super::draw::caps(
        &p,
        "WHERE THIS SONG'S TONES PLAY",
        pos2(inner.min.x, ty),
        10.0,
        th.caption,
        Align2::LEFT_CENTER,
    );
    let bar = Rect::from_min_size(pos2(inner.min.x, ty + px(12.0)), vec2(inner.width(), px(26.0)));
    let total = spans.last().map(|s| s.1).unwrap_or(1.0).max(1.0);
    for s in &spans {
        let x0 = bar.min.x + bar.width() * (s.0 / total) as f32;
        let x1 = bar.min.x + bar.width() * (s.1 / total) as f32;
        let sr = Rect::from_min_max(pos2(x0, bar.min.y), pos2(x1.max(x0 + 2.0), bar.max.y));
        let col = Color32::from_rgb(
            super::rigs::SCENE_COLOURS[s.2 % 4][0],
            super::rigs::SCENE_COLOURS[s.2 % 4][1],
            super::rigs::SCENE_COLOURS[s.2 % 4][2],
        );
        p.rect_filled(
            sr.shrink(1.0),
            px(4.0),
            if s.2 == i { col } else { col.gamma_multiply(0.45) },
        );
        if sr.width() > px(50.0) {
            text::caps(
                &p,
                &rig.tones
                    .get(s.2)
                    .map(|t| t.label.to_uppercase())
                    .unwrap_or_default(),
                sr.center(),
                9.0,
                Color32::BLACK,
                Align2::CENTER_CENTER,
                Some(sr.width() - px(4.0)),
            );
        }
    }
    let ky = bar.max.y + px(22.0);
    let follow = crate::pedalboard::follow(&st.config) && room.rack.board.pick.is_none();
    let opts = [
        ("Follow the song".to_string(), follow),
        (
            format!("Hold {}", slot.label.to_uppercase()),
            room.rack.board.pick == Some(Pick::Tone(i)),
        ),
        (
            "Always my sound".to_string(),
            room.rack.board.pick == Some(Pick::Mine),
        ),
    ];
    let mut x = inner.min.x;
    for (n, (l, on)) in opts.iter().enumerate() {
        let w = super::draw::chip_w(ui.ctx(), l, true, 11.5) + px(8.0);
        let cr = Rect::from_min_size(pos2(x, ky - px(13.0)), vec2(w, px(26.0)));
        x = cr.max.x + px(8.0);
        if super::draw::chip(ui, cr, ("explain-follow", n), l, None, *on, 11.5).clicked() {
            match n {
                0 => {
                    crate::pedalboard::set_follow(&mut st.config, true);
                    crate::pedalboard::set_always_mine(&mut st.config, false);
                    st.save_config();
                    room.rack.board.pick = None;
                }
                1 => {
                    crate::pedalboard::set_always_mine(&mut st.config, false);
                    st.save_config();
                    room.rack.board.pick = Some(Pick::Tone(i));
                }
                // (P41.1: on every song, until you follow a song again)
                _ => {
                    crate::pedalboard::set_always_mine(&mut st.config, true);
                    st.save_config();
                    room.rack.board.pick = Some(Pick::Mine);
                }
            }
        }
    }
    text::draw(
        &p,
        "Edits to a song tone are kept for this song only (\"yours\"); Reset to song puts RS's back.",
        pos2(x + px(12.0), ky),
        Align2::LEFT_CENTER,
        Style::new(10.5, 600, th.caption),
        Some(inner.max.x - x - px(12.0)),
    );
    if close.clicked()
        && let Some(pos) = close.interact_pointer_pos()
        && !panel.contains(pos)
    {
        room.rig_ui.explain = None;
    }
}

/// Where each tone plays: (from, to, tone), the song's base tone from the start until its first change.
pub fn tone_spans(rig: &crate::pedalboard::SongRig, end: f64) -> Vec<(f64, f64, usize)> {
    let mut changes = rig.changes.clone();
    if changes.first().is_none_or(|c| c.0 > 0.0) {
        changes.insert(0, (0.0, rig.base));
    }
    changes
        .iter()
        .enumerate()
        .map(|(n, c)| (c.0, changes.get(n + 1).map(|x| x.0).unwrap_or(end.max(c.0)), c.1))
        .collect()
}

fn slot_words(slot: &str) -> &'static str {
    match slot {
        "PrePedal1" => "Pre pedal 1",
        "PrePedal2" => "Pre pedal 2",
        "PrePedal3" => "Pre pedal 3",
        "Amp" => "Amp",
        "PostPedal1" => "Post pedal 1",
        "PostPedal2" => "Post pedal 2",
        "PostPedal3" => "Post pedal 3",
        "Cabinet" => "Cabinet",
        "Rack1" => "Rack 1",
        "Rack2" => "Rack 2",
        "Rack3" => "Rack 3",
        _ => "Gear",
    }
}

/// The wheel over a quick control.
pub fn wheel(room: &mut RoomView, st: &mut AppState, pos: Pos2, notches: f32, shift: bool) -> bool {
    let Some(&(_, n)) = room.rig_ui.qspots.iter().rev().find(|s| s.0.contains(pos)) else {
        return false;
    };
    let b = room.rack.board_now(st);
    let qs = quick_map(&b);
    let Some(q) = qs.get(n) else { return false };
    let Some(u) = quick_u(&b, q) else { return false };
    set_quick(room, st, q, u + notches * if shift { 0.005 } else { 0.025 });
    room.wheel_at = Some(std::time::Instant::now());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(ids: &[&str]) -> Board {
        Board::new(ids.iter().map(|id| Block::of(id).unwrap()).collect())
    }

    #[test]
    fn each_quick_control_names_the_knob_it_turns() {
        let b = board(&["green", "amp_crunch", "cab_4x12", "analog_delay", "spring"]);
        let q = quick_map(&b);
        let labels: Vec<&str> = q.iter().map(|x| x.label).collect();
        assert_eq!(labels, ["Gain", "Bass", "Mid", "Treble", "Effects", "Level"]);
        let amp = b.blocks[1];
        assert_eq!(
            q[0].target,
            Target::Knobs(vec![(amp.uid, amp.def().knob_index("gain").unwrap())])
        );
        assert!(q[0].says.contains(amp.def().name), "{}", q[0].says);
        match &q[4].target {
            Target::Knobs(v) => assert_eq!(v.len(), 2, "both the delay's and the reverb's mix"),
            t => panic!("{t:?}"),
        }
        assert_eq!(q[5].target, Target::Level);
        // the card names the amp once, each caption its knob alone; the effects name theirs
        let main = quick_main_unit(&q);
        assert_eq!(main.as_deref(), Some(amp.def().name));
        assert_eq!(quick_caption(&q[0], main.as_deref()), "gain");
        assert_eq!(quick_caption(&q[4], main.as_deref()), "mix of 2 effects");
        // a capture's gain is its Input; nothing for a tone knob it doesn't have
        let n = board(&["amp_nam", "cab_ir"]);
        let qn = quick_map(&n);
        let input = n.blocks[0].def().knob_index("input").unwrap();
        assert_eq!(qn[0].target, Target::Knobs(vec![(n.blocks[0].uid, input)]));
        if n.blocks[0].def().knob_index("bass").is_none() {
            assert_eq!(qn[1].target, Target::None);
        }
        assert_eq!(qn[4].target, Target::None);
    }

    #[test]
    fn every_amp_is_two_clicks_away() {
        // the drawer's Gear page (one click) opens on a family; every model of ours is a cell of its family on
        // its first page, models before captures (two clicks: the family, the cell)
        for kind in Kind::ALL {
            let cells = shelf_cells(kind, "", ShelfMode::Both, false);
            // (on a guitar its own gear first, P41.2)
            let mut ours: Vec<usize> = catalog::of_family(kind).map(|(i, _)| i).collect();
            ours.sort_by_key(|&m| is_bass_model(MODELS[m].id));
            for (n, m) in ours.iter().enumerate() {
                assert_eq!(
                    cells.get(n),
                    Some(&Entry::Model(*m)),
                    "{:?}: {}",
                    kind,
                    MODELS[*m].id
                );
            }
        }
        // (each instrument's amps on the first shelf page at 1280 wide; the other's follow, P41.2)
        for b in [false, true] {
            let amps = catalog::of_kind(Kind::Amp)
                .filter(|(_, m)| is_bass_model(m.id) == b)
                .count();
            assert!(amps <= 12, "{amps} amps: more than a shelf page at 1280 wide");
        }
        // with Models only, no captures; bass amps first on a bass
        let bass = shelf_cells(Kind::Amp, "", ShelfMode::Models, true);
        assert!(matches!(bass[0], Entry::Model(m) if is_bass_model(MODELS[m].id)));
        assert!(
            bass.iter()
                .all(|e| matches!(e, Entry::Model(m) if !MODELS[*m].file))
        );
    }

    #[test]
    fn the_base_tone_plays_from_the_start() {
        let rig = crate::pedalboard::SongRig {
            sid: String::new(),
            part: String::new(),
            tones: Vec::new(),
            changes: vec![(151.0, 1), (181.0, 0), (234.0, 1)],
            base: 1,
        };
        let s = tone_spans(&rig, 245.0);
        assert_eq!(
            s,
            vec![
                (0.0, 151.0, 1),
                (151.0, 181.0, 1),
                (181.0, 234.0, 0),
                (234.0, 245.0, 1)
            ]
        );
    }

    #[test]
    fn the_modeller_banner_asks_once_and_each_answer_sticks() {
        let dir = std::env::temp_dir().join(format!("qrs-banner-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        let _ = std::fs::write(&path, "{}");
        let mut cfg = qrs_library::Config::load(&path);
        let (id, name) = ("Asio:NUX Audio", "NUX Audio");
        assert!(banner_wanted(&cfg, id, name), "a NUX asks");
        assert!(
            !banner_wanted(&cfg, "Wasapi:Focusrite", "Focusrite USB"),
            "a dry interface doesn't"
        );
        // Yes: taken as dry, never asked again
        let mut yes = cfg.clone();
        answer_banner(&mut yes, id, Answer::Yes);
        assert_eq!(crate::calibrate::load(&yes, id).kind, crate::calibrate::Kind::Dry);
        assert!(!banner_wanted(&yes, id, name));
        // No: a modeller for good (songs play it as it is), never asked again
        let mut no = cfg.clone();
        answer_banner(&mut no, id, Answer::No);
        assert_eq!(
            crate::calibrate::load(&no, id).kind,
            crate::calibrate::Kind::Modeller
        );
        assert!(crate::calibrate::is_modeller_input(&no, id, name));
        assert!(!banner_wanted(&no, id, name));
        // the ×: nothing changed but the asking
        answer_banner(&mut cfg, id, Answer::Hide);
        assert_eq!(
            crate::calibrate::load(&cfg, id).kind,
            crate::calibrate::Kind::Auto
        );
        assert!(!banner_wanted(&cfg, id, name));
        // (another input is asked on its own)
        assert!(banner_wanted(&cfg, "Wasapi:Line (NUX Audio)", "Line (NUX Audio)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_goal_row_turns_the_right_knobs() {
        let b = board(&["green", "amp_crunch", "cab_4x12", "spring"]);
        let (drive, amp, cab, verb) = (b.blocks[0], b.blocks[1], b.blocks[2], b.blocks[3]);
        let k = |x: &Block, key: &str| x.def().knob_index(key).unwrap();
        let moved = |g: Goal| -> Vec<(u32, usize, f32)> {
            goal_nudges(&b, g)
                .iter()
                .map(|n| (n.uid, n.knob, n.to - n.from))
                .collect()
        };
        // more crunch: the amp's gain up (the pedal left alone while there's an amp)
        let m = moved(Goal::MoreCrunch);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!((m[0].0, m[0].1), (amp.uid, k(&amp, "gain")));
        assert!(m[0].2 > 0.0);
        // cleaner: the amp's gain and the pedal's drive down
        let m = moved(Goal::Cleaner);
        assert!(
            m.iter()
                .any(|x| (x.0, x.1) == (amp.uid, k(&amp, "gain")) && x.2 < 0.0),
            "{m:?}"
        );
        assert!(
            m.iter()
                .any(|x| (x.0, x.1) == (drive.uid, k(&drive, "drive")) && x.2 < 0.0),
            "{m:?}"
        );
        // less harsh: the treble (and a presence, if the amp has one) down
        let m = moved(Goal::LessHarsh);
        assert!(
            m.iter()
                .any(|x| (x.0, x.1) == (amp.uid, k(&amp, "treble")) && x.2 < 0.0),
            "{m:?}"
        );
        assert!(m.iter().all(|x| x.2 < 0.0));
        // bigger: the bass up, the cab's mic closer
        let m = moved(Goal::Bigger);
        assert!(
            m.iter()
                .any(|x| (x.0, x.1) == (amp.uid, k(&amp, "bass")) && x.2 > 0.0),
            "{m:?}"
        );
        if let Some(dk) = cab.def().knob_index("distance") {
            assert!(
                m.iter().any(|x| (x.0, x.1) == (cab.uid, dk) && x.2 < 0.0),
                "{m:?}"
            );
        }
        // more space: the reverb's mix up
        let m = moved(Goal::MoreSpace);
        assert_eq!(
            m.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(),
            vec![(verb.uid, k(&verb, "mix"))]
        );
        // nothing to make space with: empty (the room adds a reverb then)
        assert!(goal_nudges(&board(&["green", "amp_crunch", "cab_4x12"]), Goal::MoreSpace).is_empty());
        // at the end of its turn, a knob isn't "moved"
        let maxed = Board::new(vec![Block::of("amp_crunch").unwrap().with("gain", 10.0)]);
        assert!(goal_nudges(&maxed, Goal::MoreCrunch).is_empty());
        // and what moved is said by the knob's name and values
        let n = goal_nudges(&b, Goal::MoreCrunch)[0];
        let w = nudge_words(&b, &n);
        assert!(
            w.starts_with(&format!("{} gain ", amp.def().name)) && w.contains('\u{2192}'),
            "{w}"
        );
    }

    #[test]
    fn effects_count_an_amps_own_reverb_and_trem() {
        // (the Twin's spring is an effect to a player: the Effects knob turns it)
        let b = board(&["amp_clean", "cab_2x12"]);
        let q = quick_map(&b);
        let amp = b.blocks[0];
        assert_eq!(
            q[4].target,
            Target::Knobs(vec![(amp.uid, amp.def().knob_index("reverb").unwrap())])
        );
        // the trem counts while it's on
        let mut b2 = b.clone();
        let ik = amp.def().knob_index("intensity").unwrap();
        b2.blocks[0].v[ik] = 5.0;
        match &quick_map(&b2)[4].target {
            Target::Knobs(v) => assert_eq!(v.len(), 2),
            t => panic!("{t:?}"),
        }
    }

    #[test]
    fn capture_names_are_short() {
        assert_eq!(
            short_capture_name("1976 Fender Deluxe Reverb \u{b7} edge SFDR Sweet Spot V4 T5 B5"),
            "Fender Deluxe Reverb \u{b7} edge of breakup"
        );
        assert_eq!(
            short_capture_name("AMPEG SVT-CL \u{b7} crunch SANS HAIRY DRIVE"),
            "AMPEG SVT-CL \u{b7} crunch"
        );
        assert_eq!(
            short_capture_name("Jekyll & Hyde - TONE3000 pack (v2)"),
            "Jekyll & Hyde"
        );
        assert_eq!(short_capture_name("SVT-CL"), "SVT-CL");
        assert!(
            short_capture_name("A very long capture name without any separators at all")
                .chars()
                .count()
                <= 40
        );
    }
}
