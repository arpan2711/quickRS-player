//! TONE3000 downloads you can see (P38.3, new; D157). Every pick (`captures::tone3000`) is kept here, wherever it
//! was started (Settings › Neural Amp Modeler, the first-time setup's Use NAM? page, the board's captures dialog), so
//! it outlives the page that started it; a card over every page says where each is: waiting for the browser (with
//! Cancel and the time left), signing in, the file coming in (a bar, bytes of the size), measuring, Downloaded with
//! Open in the amp room, or why it didn't (Retry). Over a song and in the amp room the card folds to a small badge.
//!
//! Owner, 2026-11-05: "When I click download ... it just takes me to a web page ... in the software itself there's no
//! progress bar or any confirmation or completion, nothing."
//!
//! The jobs are read and changed on the UI thread only; each pick's thread writes its own phase and byte counts.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, CornerRadius, CursorIcon, Rect, Sense, Ui, pos2, vec2};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle, interact};

use crate::captures::suggest::Unit;
use crate::captures::tone3000::{self, Phase, Pick, Progress, Why};
use crate::captures::{Item, Kind};

/// How long a finished download stays on the card (the suggestion row keeps saying it after).
const DONE_FOR: Duration = Duration::from_secs(25);
/// A pick cancelled here goes this soon; one cancelled on TONE3000's page stays a little, with Retry.
const CANCELLED_FOR: Duration = Duration::from_secs(4);
const CANCELLED_THERE_FOR: Duration = Duration::from_secs(15);
/// The most rows on the card (more: "+N more").
const ROWS: usize = 3;

struct Job {
    id: u64,
    kind: Kind,
    /// What's being fetched ("Fender Deluxe Reverb", "a capture").
    what: String,
    unit: Option<&'static Unit>,
    /// The publishable key (for Retry; in memory only).
    key: String,
    pick: Pick,
    ended: Option<Instant>,
}

static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
/// The badge opened into the card (over a song, in the room).
static OPEN: AtomicBool = AtomicBool::new(false);

fn jobs() -> std::sync::MutexGuard<'static, Vec<Job>> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A download as it is now (a copy, for drawing).
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub id: u64,
    pub kind: Kind,
    pub what: String,
    pub unit: Option<&'static str>,
    pub phase: Phase,
    pub title: Option<String>,
    pub progress: Progress,
    pub wait_left: Duration,
}

impl View {
    pub fn busy(&self) -> bool {
        !self.phase.finished()
    }

    /// The name to show: the capture's, the tone's once read, else what was asked for.
    pub fn name(&self) -> String {
        if let Some(it) = self.phase.item() {
            return it.name.clone();
        }
        if let Phase::Downloading { name, .. } = &self.phase {
            return name.clone();
        }
        self.title.clone().unwrap_or_else(|| self.what.clone())
    }

    /// The small caps over the name.
    pub fn stage(&self) -> String {
        match &self.phase {
            Phase::InBrowser => {
                let left = self.wait_left.as_secs();
                format!("WAITING FOR TONE3000 \u{b7} {}:{:02} LEFT", left / 60, left % 60)
            }
            Phase::SigningIn => "SIGNING IN".into(),
            Phase::Reading => "READING THE TONE".into(),
            Phase::Downloading { file, files, .. } if *files > 1 => {
                format!("DOWNLOADING \u{b7} FILE {file} OF {files}")
            }
            Phase::Downloading { .. } => "DOWNLOADING".into(),
            Phase::Measuring(_) => "MEASURING".into(),
            Phase::Done(_) => "DOWNLOADED".into(),
            Phase::Failed(Why::TimedOut, _) => "TIMED OUT".into(),
            Phase::Failed(..) => "DIDN'T DOWNLOAD".into(),
            Phase::Cancelled { .. } => "CANCELLED".into(),
        }
    }

    /// The line under the name.
    pub fn line(&self) -> String {
        match &self.phase {
            Phase::InBrowser => "Pick the tone on the TONE3000 page; quickRS downloads it here.".into(),
            Phase::SigningIn => "Signing in to your TONE3000 account\u{2026}".into(),
            Phase::Reading => "Reading the tone's files\u{2026}".into(),
            Phase::Downloading { .. } => self.progress.words(),
            Phase::Measuring(_) => "In your captures; matching its level to quickRS's models\u{2026}".into(),
            Phase::Done(_) => "In your captures, and the room's catalogue.".into(),
            Phase::Failed(why, e) => match why {
                Why::Network => format!("No connection to TONE3000 ({e})"),
                Why::TimedOut => format!("{e}."),
                Why::Invalid => format!("The file isn't a capture that runs here: {e}"),
                Why::Site | Why::Other => e.clone(),
            },
            Phase::Cancelled { in_browser: true } => {
                "Cancelled on TONE3000's page: nothing downloaded.".into()
            }
            Phase::Cancelled { in_browser: false } => "Cancelled: nothing downloaded.".into(),
        }
    }

    /// The bar: None for none, Some(None) for one that moves without a size, Some(Some(f)) for f of the way.
    pub fn bar(&self) -> Option<Option<f32>> {
        match &self.phase {
            Phase::Downloading { .. } => Some(self.progress.fraction()),
            Phase::SigningIn | Phase::Reading | Phase::Measuring(_) => Some(None),
            _ => None,
        }
    }

    /// Retry is offered (it didn't come, and not because you cancelled it here).
    pub fn can_retry(&self) -> bool {
        matches!(
            self.phase,
            Phase::Failed(..) | Phase::Cancelled { in_browser: true }
        )
    }

    /// A short word for the badge ("42%", "waiting", "done").
    pub fn short(&self) -> String {
        match &self.phase {
            Phase::InBrowser => "waiting for your pick".into(),
            Phase::Downloading { .. } => match self.progress.fraction() {
                Some(f) => format!("{}%", (f * 100.0).round() as u32),
                None => self.progress.words(),
            },
            Phase::SigningIn | Phase::Reading => "starting".into(),
            Phase::Measuring(_) => "measuring".into(),
            Phase::Done(_) => "downloaded \u{2713}".into(),
            Phase::Failed(..) => "didn't download".into(),
            Phase::Cancelled { .. } => "cancelled".into(),
        }
    }
}

fn view_of(j: &Job) -> View {
    View {
        id: j.id,
        kind: j.kind,
        what: j.what.clone(),
        unit: j.unit.map(|u| u.name),
        phase: j.pick.phase(),
        title: j.pick.title(),
        progress: j.pick.progress(),
        wait_left: j.pick.wait_left(),
    }
}

/// What's being fetched, when there's no suggestion to name it.
pub fn what_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Nam => "An amp capture",
        Kind::Pedal => "A pedal capture",
        Kind::Ir => "A cab IR",
    }
}

/// Start a pick: TONE3000's page in the browser, its download here. A pick still waiting for the browser is
/// cancelled (the new page takes its place); downloads already coming keep coming. Returns the job's id.
pub fn start(key: String, kind: Kind, unit: Option<&'static Unit>) -> u64 {
    let pick = tone3000::start(key.clone(), kind, unit);
    push(key, kind, unit, pick)
}

fn push(key: String, kind: Kind, unit: Option<&'static Unit>, pick: Pick) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut js = jobs();
    js.retain(|j| {
        let waiting = j.pick.phase() == Phase::InBrowser;
        if waiting {
            j.pick.cancel();
        }
        !waiting
    });
    js.push(Job {
        id,
        kind,
        what: unit
            .map(|u| u.name.to_string())
            .unwrap_or_else(|| what_of(kind).into()),
        unit,
        key,
        pick,
        ended: None,
    });
    id
}

/// Tests: these downloads, as given (no thread, nothing fetched).
#[cfg(test)]
fn stage(list: Vec<(Kind, Option<&'static Unit>, Phase, Option<&str>, Progress)>) {
    clear();
    for (kind, unit, phase, title, progress) in list {
        push(String::new(), kind, unit, Pick::staged(phase, title, progress));
    }
}

/// Every download forgotten (the ones under way cancelled).
pub fn clear() {
    for j in jobs().drain(..) {
        j.pick.cancel();
    }
    OPEN.store(false, Ordering::Relaxed);
}

/// The downloads a screenshot shows, by its name ("20t_downloads_<what>", any theme); other shots have none.
pub fn stage_shot(name: &str) {
    clear();
    let Some(what) = name.strip_prefix("20t_downloads_") else {
        return;
    };
    let unit = |k: &str| crate::captures::suggest::UNITS.iter().find(|u| u.rs.contains(&k));
    let item = |n: &str| {
        let mut it = crate::captures::blank(Kind::Nam);
        it.name = n.into();
        Box::new(it)
    };
    let none = Progress::default();
    let (at20, ts, plexi) = (
        unit("Amp_AT20"),
        unit("Pedal_TubeScreamer"),
        unit("Amp_MarshallPlexi"),
    );
    let dl = |file, files, name: &str| Phase::Downloading {
        file,
        files,
        name: name.into(),
    };
    let list = match what.split('-').next().unwrap_or("") {
        "waiting" => vec![(Kind::Nam, at20, Phase::InBrowser, None, none)],
        "progress" | "rows" => vec![
            (
                Kind::Nam,
                plexi,
                dl(1, 2, "Plexi 1959 \u{b7} Crunch"),
                Some("Plexi 1959"),
                Progress {
                    got: 1_460_000,
                    total: Some(3_400_000),
                },
            ),
            (
                Kind::Pedal,
                ts,
                dl(1, 1, "TS808 Boost"),
                Some("TS808 Boost"),
                Progress {
                    got: 640_000,
                    total: None,
                },
            ),
            (Kind::Nam, at20, Phase::InBrowser, None, none),
        ],
        "done" => vec![
            (Kind::Pedal, ts, Phase::Measuring(item("TS808 Boost")), None, none),
            (
                Kind::Nam,
                plexi,
                Phase::Done(item("Plexi 1959 \u{b7} Crunch")),
                None,
                none,
            ),
        ],
        "failed" => vec![
            (
                Kind::Nam,
                plexi,
                Phase::Failed(Why::Network, "the download broke off".into()),
                Some("Plexi 1959"),
                none,
            ),
            (
                Kind::Nam,
                at20,
                Phase::Failed(
                    Why::TimedOut,
                    "TONE3000's page didn't come back within 10 minutes".into(),
                ),
                None,
                none,
            ),
            (Kind::Pedal, ts, Phase::Cancelled { in_browser: true }, None, none),
        ],
        "room" | "room_open" => vec![(
            Kind::Nam,
            plexi,
            dl(1, 1, "Plexi 1959 \u{b7} Crunch"),
            Some("Plexi 1959"),
            Progress {
                got: 2_100_000,
                total: Some(3_400_000),
            },
        )],
        _ => Vec::new(),
    };
    for (kind, unit, phase, title, progress) in list {
        push(String::new(), kind, unit, Pick::staged(phase, title, progress));
    }
    // (the room's shot with the badge opened into the card)
    OPEN.store(what.starts_with("room_open"), Ordering::Relaxed);
}

pub fn cancel(id: u64) {
    if let Some(j) = jobs().iter().find(|j| j.id == id) {
        j.pick.cancel();
    }
}

/// The same pick again (TONE3000's page opened again).
pub fn retry(id: u64) {
    let again = {
        let mut js = jobs();
        let Some(i) = js.iter().position(|j| j.id == id) else {
            return;
        };
        let j = js.remove(i);
        (j.key, j.kind, j.unit)
    };
    if !again.0.is_empty() {
        start(again.0, again.1, again.2);
    }
}

pub fn dismiss(id: u64) {
    jobs().retain(|j| j.id != id);
}

/// Every download (finished ones until they've been seen long enough), oldest first.
pub fn views() -> Vec<View> {
    let now = Instant::now();
    let mut js = jobs();
    js.retain_mut(|j| {
        let ph = j.pick.phase();
        if !ph.finished() {
            j.ended = None;
            return true;
        }
        let ended = *j.ended.get_or_insert(now);
        match ph {
            Phase::Done(_) => now.duration_since(ended) < DONE_FOR,
            Phase::Cancelled { in_browser: false } => now.duration_since(ended) < CANCELLED_FOR,
            Phase::Cancelled { in_browser: true } => now.duration_since(ended) < CANCELLED_THERE_FOR,
            // (a failure stays until it's dismissed or tried again)
            _ => true,
        }
    });
    js.iter().map(view_of).collect()
}

pub fn view(id: u64) -> Option<View> {
    views().into_iter().find(|v| v.id == id)
}

/// The latest download for a suggestion (by its unit's name).
pub fn of_unit(name: &str) -> Option<View> {
    views().into_iter().rev().find(|v| v.unit == Some(name))
}

/// Anything still coming.
#[cfg(test)]
fn busy() -> bool {
    views().iter().any(View::busy)
}

// -- drawing --------------------------------------------------------------------------------------------------------

/// What the card asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Open in the amp room: the room's catalogue, found by its name.
    OpenRoom(Box<Item>),
}

/// A bar: the groove, and the part done (or, with no size, a piece that sweeps along).
pub fn bar(p: &egui::Painter, r: Rect, f: Option<f32>, time: f64) {
    let th = t();
    let cr = CornerRadius::same((r.height() / 2.0).round() as u8);
    p.rect_filled(r, cr, th.track);
    match f {
        Some(f) => {
            if f > 0.0 {
                let w = (r.width() * f).max(r.height());
                p.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), cr, th.accent);
            }
        }
        None => {
            let seg = r.width() * 0.28;
            let u = (time * 0.7).fract() as f32;
            let x0 = r.min.x - seg + u * (r.width() + seg);
            let a = x0.max(r.min.x);
            let b = (x0 + seg).min(r.max.x);
            if b > a {
                p.rect_filled(
                    Rect::from_min_max(pos2(a, r.min.y), pos2(b, r.max.y)),
                    cr,
                    th.accent,
                );
            }
        }
    }
}

fn stage_colour(v: &View) -> Color32 {
    let th = t();
    match v.phase {
        Phase::Done(_) => th.hit,
        Phase::Failed(..) => th.miss,
        Phase::Cancelled { .. } => th.caption,
        _ => th.accent_ink,
    }
}

/// The downloads over the page, at its bottom left: the card (or, `compact`, a badge that opens into it). `can_open`:
/// Open in the amp room is offered. `gap`: room left under it (the page's own bottom strip).
pub fn ui(ui: &Ui, area: Rect, compact: bool, can_open: bool, gap: f32) -> Option<Act> {
    let list = views();
    if list.is_empty() {
        OPEN.store(false, Ordering::Relaxed);
        return None;
    }
    let ctx = ui.ctx().clone();
    ctx.request_repaint_after(Duration::from_millis(if list.iter().any(View::busy) {
        100
    } else {
        500
    }));
    let left = area.min.x + px(28.0);
    let bottom = area.max.y - px(gap);
    if compact && !OPEN.load(Ordering::Relaxed) {
        badge(ui, &list, left, bottom);
        return None;
    }
    card(ui, &list, left, bottom, area.width(), compact, can_open)
}

/// The folded badge: a dot and "TONE3000 · 42%"; a click opens the card.
fn badge(ui: &Ui, list: &[View], left: f32, bottom: f32) {
    let th = t();
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let v = list
        .iter()
        .rev()
        .find(|v| v.busy())
        .unwrap_or(&list[list.len() - 1]);
    let more = list.len().saturating_sub(1);
    let words = format!(
        "{}{}",
        v.short(),
        if more > 0 {
            format!("  \u{b7}  +{more}")
        } else {
            String::new()
        }
    );
    let st = Style::new(13.0, 700, th.text);
    let cap = "TONE3000";
    let cw = text::width(&ctx, cap, Style::caps(10.5, th.caption));
    let w = px(18.0) + px(10.0) + cw + px(10.0) + text::width(&ctx, &words, st) + px(18.0);
    let h = px(36.0);
    let r = Rect::from_min_size(pos2(left, bottom - h), vec2(w, h));
    let resp = interact(ui, r, "t3k-badge", Sense::click());
    paint::floating(&p, r, Some(h / 2.0));
    let cy = r.center().y;
    let dot = pos2(r.min.x + px(18.0), cy);
    p.circle_filled(dot, px(4.0), stage_colour(v));
    let cr = text::caps(
        &p,
        cap,
        pos2(dot.x + px(10.0), cy),
        10.5,
        th.caption,
        Align2::LEFT_CENTER,
        None,
    );
    text::draw(
        &p,
        &words,
        pos2(cr.max.x + px(10.0), cy),
        Align2::LEFT_CENTER,
        st,
        None,
    );
    if let Some(f) = v.bar() {
        let br = Rect::from_min_max(
            pos2(r.min.x + px(14.0), r.max.y - px(6.0)),
            pos2(r.max.x - px(14.0), r.max.y - px(3.0)),
        );
        bar(&p, br, f, ctx.input(|i| i.time));
    }
    if resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        OPEN.store(true, Ordering::Relaxed);
    }
}

fn card(
    ui: &Ui,
    list: &[View],
    left: f32,
    bottom: f32,
    area_w: f32,
    compact: bool,
    can_open: bool,
) -> Option<Act> {
    let th = t();
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let time = ctx.input(|i| i.time);
    let pad = px(20.0);
    let w = px(500.0).min(area_w - px(56.0));
    let row_h = px(82.0);
    let shown: Vec<&View> = list
        .iter()
        .rev()
        .take(ROWS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let more = list.len() - shown.len();
    let head_h = px(24.0);
    let h = pad + head_h + shown.len() as f32 * row_h + if more > 0 { px(20.0) } else { 0.0 } + pad - px(8.0);
    let card = Rect::from_min_size(pos2(left, bottom - h), vec2(w, h));
    // (clicks on the card don't reach the page under it)
    let _ = interact(ui, card, "t3k-card", Sense::click());
    paint::floating_opaque(&p, card, Some(22.0));
    let x = card.min.x + pad;
    let busy = list.iter().filter(|v| v.busy()).count();
    text::caps(
        &p,
        &if busy > 0 {
            format!("TONE3000 \u{b7} {busy} UNDER WAY")
        } else {
            "TONE3000".into()
        },
        pos2(x, card.min.y + pad),
        11.0,
        th.caption,
        Align2::LEFT_TOP,
        None,
    );
    let mut act = None;
    // (folded back into the badge: over a song and in the room)
    if compact {
        let fr = Rect::from_min_size(
            pos2(card.max.x - pad - px(60.0), card.min.y + pad - px(4.0)),
            vec2(px(60.0), px(22.0)),
        );
        let r = widgets::button(
            ui,
            fr,
            "t3k-fold",
            "Hide",
            "",
            ButtonStyle {
                size: 11.0,
                r: Some(9.0),
                plain: true,
                ..Default::default()
            },
        );
        if r.clicked() {
            OPEN.store(false, Ordering::Relaxed);
        }
    }
    let mut y = card.min.y + pad + head_h;
    for (n, v) in shown.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, y), vec2(w - 2.0 * pad, row_h));
        if n > 0 {
            p.hline(r.min.x..=r.max.x, r.min.y, (1.0, th.divider));
        }
        if let Some(a) = row(ui, r, v, can_open, time) {
            act = Some(a);
        }
        y += row_h;
    }
    if more > 0 {
        text::draw(
            &p,
            &format!("+{more} more"),
            pos2(x, y + px(2.0)),
            Align2::LEFT_TOP,
            Style::new(12.0, 600, th.caption),
            None,
        );
    }
    act
}

/// One download: its stage, its name, its line or bar, and its keys at the right.
fn row(ui: &Ui, r: Rect, v: &View, can_open: bool, time: f64) -> Option<Act> {
    let th = t();
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    // the keys, from the right
    let mut keys: Vec<(&str, &str, bool)> = Vec::new();
    match &v.phase {
        Phase::Done(_) => {
            if can_open {
                keys.push(("Open in the amp room", "open", true));
            }
            keys.push(("\u{d7}", "dismiss", false));
        }
        _ if v.can_retry() => {
            keys.push(("Retry", "retry", true));
            keys.push(("\u{d7}", "dismiss", false));
        }
        Phase::Cancelled { .. } => keys.push(("\u{d7}", "dismiss", false)),
        Phase::Measuring(_) => {}
        _ => keys.push(("Cancel", "cancel", false)),
    }
    let kh = px(34.0);
    let ky = r.min.y + px(12.0) + (r.height() - px(12.0) - kh) / 2.0 - px(4.0);
    let mut right = r.max.x;
    let mut clicked = None;
    for (label, what, primary) in keys.iter().rev() {
        let kw = if *label == "\u{d7}" {
            kh
        } else {
            widgets::button_width(&ctx, label, "", 13.0, false)
        };
        let kr = Rect::from_min_size(pos2(right - kw, ky), vec2(kw, kh));
        if widgets::button(
            ui,
            kr,
            ("t3k-key", v.id, *what),
            label,
            "",
            ButtonStyle {
                size: 13.0,
                r: Some(12.0),
                primary: *primary,
                plain: !*primary,
                ..Default::default()
            },
        )
        .clicked()
        {
            clicked = Some(*what);
        }
        right = kr.min.x - px(8.0);
    }
    let tw = right - r.min.x - px(10.0);
    let top = r.min.y + px(12.0);
    let cr = text::caps(
        &p,
        &v.stage(),
        pos2(r.min.x, top),
        10.5,
        stage_colour(v),
        Align2::LEFT_TOP,
        Some(tw),
    );
    let name = v.name();
    let nr = text::draw(
        &p,
        &name,
        pos2(r.min.x, cr.max.y + px(4.0)),
        Align2::LEFT_TOP,
        Style::new(15.0, 800, th.text),
        Some(tw),
    );
    let ly = nr.max.y + px(6.0);
    match v.bar() {
        Some(f) => {
            let words = match v.phase {
                Phase::Downloading { .. } => v.line(),
                Phase::Measuring(_) => "matching its level".into(),
                Phase::SigningIn => "signing in".into(),
                _ => "reading the tone".into(),
            };
            let ws = Style::new(12.0, 600, th.text_2);
            let ww = text::width(&ctx, &words, ws);
            let br = Rect::from_min_size(
                pos2(r.min.x, ly + px(4.0)),
                vec2((tw - ww - px(12.0)).max(px(60.0)), px(6.0)),
            );
            bar(&p, br, f, time);
            text::draw(
                &p,
                &words,
                pos2(br.max.x + px(12.0), br.center().y),
                Align2::LEFT_CENTER,
                ws,
                None,
            );
        }
        None => {
            text::draw(
                &p,
                &v.line(),
                pos2(r.min.x, ly),
                Align2::LEFT_TOP,
                Style::new(
                    12.0,
                    500,
                    if matches!(v.phase, Phase::Failed(..)) {
                        th.text
                    } else {
                        th.caption
                    },
                ),
                Some(tw),
            );
        }
    }
    match clicked {
        Some("cancel") => cancel(v.id),
        Some("retry") => retry(v.id),
        Some("dismiss") => dismiss(v.id),
        Some("open") => {
            dismiss(v.id);
            OPEN.store(false, Ordering::Relaxed);
            if let Some(it) = v.phase.item() {
                return Some(Act::OpenRoom(Box::new(it.clone())));
            }
        }
        _ => {}
    }
    None
}

/// What a suggestion row's right end does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAct {
    Find,
    Cancel(u64),
    Retry(u64),
}

/// A suggestion row's right end (Settings › Neural Amp Modeler, the setup's Use NAM?): its song count, then Find on
/// TONE3000; while its download is under way where it is (a small bar) and Cancel; Downloaded ✓ when it's in (or
/// "in your captures" for one you had); Retry when it didn't come. `br`: the key's place; `busy_other`: another
/// suggestion is waiting for the browser (Find stays on: a new pick takes the browser over).
pub fn suggestion_tail(
    ui: &Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    br: Rect,
    unit: &Unit,
    have: bool,
    songs: usize,
) -> Option<RowAct> {
    let th = t();
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let cy = br.center().y;
    let v = of_unit(unit.name);
    let id = egui::Id::new(("t3k-row", id));
    // the words left of the key: the songs, or where the download is
    let (words, colour) = match &v {
        Some(v) if v.busy() || v.can_retry() => (v.short(), stage_colour(v)),
        _ => (
            format!("{songs} song{}", if songs == 1 { "" } else { "s" }),
            th.text_2,
        ),
    };
    let wr = text::draw(
        &p,
        &words,
        pos2(br.min.x - px(20.0), cy),
        Align2::RIGHT_CENTER,
        Style::new(13.0, 700, colour),
        None,
    );
    if let Some(v) = &v
        && let Some(f) = v.bar()
    {
        let r = Rect::from_min_max(
            pos2(wr.min.x, wr.max.y + px(2.0)),
            pos2(wr.max.x, wr.max.y + px(5.0)),
        );
        bar(&p, r, f, ctx.input(|i| i.time));
    }
    let done = v.as_ref().is_some_and(|v| matches!(v.phase, Phase::Done(_)));
    if done || (have && v.as_ref().is_none_or(|v| !v.busy())) {
        text::draw(
            &p,
            if done {
                "Downloaded \u{2713}"
            } else {
                "in your captures"
            },
            br.center(),
            Align2::CENTER_CENTER,
            Style::new(12.0, 700, th.hit),
            None,
        );
        return None;
    }
    let (label, act) = match &v {
        Some(v) if v.busy() && !matches!(v.phase, Phase::Measuring(_)) => ("Cancel", RowAct::Cancel(v.id)),
        Some(v) if v.busy() => ("Measuring\u{2026}", RowAct::Cancel(v.id)),
        Some(v) if v.can_retry() => ("Retry", RowAct::Retry(v.id)),
        _ => ("Find on TONE3000", RowAct::Find),
    };
    let measuring = label.starts_with("Measuring");
    let r = widgets::button(
        ui,
        br,
        id,
        label,
        "",
        ButtonStyle {
            size: 12.0,
            r: Some(11.0),
            enabled: !measuring,
            ..Default::default()
        },
    );
    if r.clicked() && !measuring {
        Some(act)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (the jobs are global: one test walks them all, so tests running at once don't meet)
    #[test]
    fn downloads_come_and_go() {
        let unit = crate::captures::suggest::UNITS
            .iter()
            .find(|u| u.kind == Kind::Nam)
            .unwrap();
        let none = Progress::default();
        stage(vec![(Kind::Nam, Some(unit), Phase::InBrowser, None, none)]);
        let v = of_unit(unit.name).unwrap();
        assert!(
            v.stage().ends_with("10:00 LEFT") || v.stage().ends_with("9:59 LEFT"),
            "{}",
            v.stage()
        );
        assert!(v.stage().starts_with("WAITING FOR TONE3000"));
        assert_eq!(
            v.line(),
            "Pick the tone on the TONE3000 page; quickRS downloads it here."
        );
        assert!(v.busy() && busy());
        assert_eq!(v.name(), unit.name);
        assert_eq!(v.bar(), None);
        // a new pick takes the browser from one still waiting
        let first = v.id;
        push(
            String::new(),
            Kind::Pedal,
            None,
            Pick::staged(Phase::InBrowser, None, none),
        );
        assert!(view(first).is_none());
        let vs = views();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].what, "A pedal capture");
        // a download keeps coming when another pick starts
        let half = Progress {
            got: 500_000,
            total: Some(1_000_000),
        };
        stage(vec![(
            Kind::Nam,
            Some(unit),
            Phase::Downloading {
                file: 1,
                files: 2,
                name: "Plexi \u{b7} Crunch".into(),
            },
            Some("Plexi"),
            half,
        )]);
        push(
            String::new(),
            Kind::Nam,
            None,
            Pick::staged(Phase::InBrowser, None, none),
        );
        let vs = views();
        assert_eq!(vs.len(), 2);
        let d = &vs[0];
        assert_eq!(d.stage(), "DOWNLOADING \u{b7} FILE 1 OF 2");
        assert_eq!(d.name(), "Plexi \u{b7} Crunch");
        assert_eq!(d.line(), "500 KB of 1.0 MB");
        assert_eq!(d.short(), "50%");
        assert_eq!(d.bar(), Some(Some(0.5)));
        // no size: an indeterminate bar
        stage(vec![(
            Kind::Nam,
            None,
            Phase::Downloading {
                file: 1,
                files: 1,
                name: "X".into(),
            },
            None,
            Progress {
                got: 640_000,
                total: None,
            },
        )]);
        assert_eq!(views()[0].bar(), Some(None));
        assert_eq!(views()[0].short(), "640 KB");
        // done: the row says so; Open in the amp room
        let mut it = crate::captures::blank(Kind::Nam);
        it.name = "Plexi Crunch".into();
        stage(vec![(
            Kind::Nam,
            Some(unit),
            Phase::Done(Box::new(it.clone())),
            None,
            none,
        )]);
        let v = of_unit(unit.name).unwrap();
        assert_eq!(v.stage(), "DOWNLOADED");
        assert_eq!(v.name(), "Plexi Crunch");
        assert!(v.line().starts_with("In your captures"));
        assert!(!v.busy() && !busy() && !v.can_retry());
        // failures stay, with Retry; a cancel here doesn't offer one
        stage(vec![
            (
                Kind::Nam,
                None,
                Phase::Failed(Why::Network, "timed out".into()),
                None,
                none,
            ),
            (
                Kind::Nam,
                None,
                Phase::Failed(
                    Why::TimedOut,
                    "TONE3000's page didn't come back within 10 minutes".into(),
                ),
                None,
                none,
            ),
            (Kind::Nam, None, Phase::Cancelled { in_browser: true }, None, none),
            (
                Kind::Nam,
                None,
                Phase::Cancelled { in_browser: false },
                None,
                none,
            ),
        ]);
        let vs = views();
        assert_eq!(vs.len(), 4);
        assert!(vs[0].line().starts_with("No connection to TONE3000"));
        assert_eq!(vs[1].stage(), "TIMED OUT");
        assert!(vs[0].can_retry() && vs[1].can_retry() && vs[2].can_retry() && !vs[3].can_retry());
        dismiss(vs[0].id);
        assert_eq!(views().len(), 3);
        // (a staged job has no key: Retry just takes it away)
        retry(vs[1].id);
        assert_eq!(views().len(), 2);
        clear();
        assert!(views().is_empty());
    }
}
