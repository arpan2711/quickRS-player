//! The run's review (new in quickRS, P29.1, P29.3, owner request; no Python source): after a run, where the notes
//! were missed, in the song's own parts. The part is cut into ideas (qrs-analysis `parts`: a riff or phrase that
//! repeats, with every place it plays, and the stretches that play once, cut at the charter's marks); each idea's
//! places are scored from the run's judged notes. The page (the results card's Review, or E) lists the ideas that
//! lost a note or more, each once however often it plays ("Riff B, 4 bars, plays 6 times"), with its worst time
//! as a small tab; above, a few lines on the run, and the song's bars as a map with a lane of its parts (a click
//! anywhere opens that part there). An idea opens looped at its worst time (every tool of the player still there)
//! with a strip over the tab: which one, its passes and best, the next one, back to the list. Each run's end is
//! saved with the part's last runs (history.rs, four kept); E on a song just opened shows the newest, and the
//! header's list picks another.

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Event, Key, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_analysis::Judgement;
use qrs_analysis::parts::Idea;
use qrs_formats::ascii_tab::section_label;
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{fmt_time, t};
use qrs_ui::units::{pixels, px, to_px};
use qrs_ui::widgets::{ButtonStyle, button, interact};

use super::rules::RangeFrom;
use super::{LOOP_LEAD_IN, PlayerView, button_w, ipos, ipx, mid, mid_y, snap};
use crate::library::{caps, txt};

/// A part that lost this much in the run (a miss is 1, a late or early note half) is listed.
const BAD_PART: f64 = 1.0;
/// The parts listed: the ones that lost the most.
const MAX_SPOTS: usize = 8;
/// A pass played this well is clean.
pub const CLEAN: f64 = 0.9;
/// A row of the list (points).
const ROW: f32 = 96.0;

/// One bar of the run.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub start: f64,
    pub end: f64,
    pub number: i32,
    pub hit: u32,
    /// Late or early.
    pub half: u32,
    pub miss: u32,
    /// The part it's in, and which of its places.
    pub part: Option<(usize, usize)>,
}

impl Bar {
    pub fn notes(&self) -> u32 {
        self.hit + self.half + self.miss
    }

    /// Notes lost: a miss whole, a late or early note half.
    pub fn loss(&self) -> f64 {
        self.miss as f64 + 0.5 * self.half as f64
    }

    pub fn accuracy(&self) -> Option<f64> {
        let n = self.notes();
        (n > 0).then(|| (self.hit as f64 + 0.5 * self.half as f64) / n as f64)
    }
}

/// A note of a place, for its small tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub t: f64,
    pub string: i32,
    pub fret: i32,
    pub judged: Option<Judgement>,
}

/// One time a part plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// Its bars (indices into `Review::bars`), first and last.
    pub first: usize,
    pub last: usize,
    pub start: f64,
    pub end: f64,
    /// "bars 23–26".
    pub bars: String,
    /// Its notes as the run judged them, in order.
    pub notes: Vec<Judgement>,
    pub loss: f64,
    pub marks: Vec<Mark>,
}

impl Place {
    pub fn accuracy(&self) -> f64 {
        if self.notes.is_empty() {
            return 1.0;
        }
        (self.notes.len() as f64 - self.loss) / self.notes.len() as f64
    }

    pub fn missed(&self) -> usize {
        self.notes.iter().filter(|j| **j == Judgement::Miss).count()
    }
}

/// A part of the song: an idea, every place it plays, and its practice.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// "Riff B", or for one that plays once its section ("Solo 2") or bars.
    pub name: String,
    /// Its lane letter on the map ("B"), or "" for one that plays once.
    pub letter: String,
    /// "first in Verse 1" (repeating), "" otherwise.
    pub hint: String,
    pub len: usize,
    pub places: Vec<Place>,
    /// The place looped and shown: the worst at first.
    pub at: usize,
    /// Lost over every place.
    pub loss: f64,
    /// Practice: loop passes played over it, the last one's and the best one's accuracy.
    pub passes: u32,
    pub last_pass: Option<f64>,
    pub best: Option<f64>,
}

impl Part {
    pub fn repeats(&self) -> bool {
        self.places.len() > 1
    }

    pub fn place(&self) -> &Place {
        &self.places[self.at.min(self.places.len() - 1)]
    }

    /// Over every place.
    pub fn accuracy(&self) -> f64 {
        let n: usize = self.places.iter().map(|p| p.notes.len()).sum();
        if n == 0 {
            return 1.0;
        }
        (n as f64 - self.loss) / n as f64
    }

    pub fn clean(&self) -> bool {
        self.last_pass.is_some_and(|a| a >= CLEAN)
    }

    /// "4 bars · plays 6×".
    pub fn shape(&self) -> String {
        let bars = if self.len == 1 {
            "1 bar".to_string()
        } else {
            format!("{} bars", self.len)
        };
        if self.repeats() {
            format!("{bars}  ·  plays {}×", self.places.len())
        } else {
            bars
        }
    }
}

/// "1st", "2nd", ...
fn nth(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Spreadsheet letters: A..Z, AA, AB...
fn letter(i: usize) -> String {
    qrs_analysis::similar::letters(i)
}

/// The bars of a run, its parts (from `ideas`, in song order, each bar in one place of one) and the parts listed
/// (indices: the ones that lost BAD_PART or more, at most MAX_SPOTS of the worst, in song order), from its judged
/// notes (chart seconds), the part's bars (start, end, number) and its sections (start, end, label).
pub fn analyse(
    notes: &[(f64, Judgement)],
    measures: &[(f64, f64, i32)],
    ideas: &[Idea],
    sections: &[(f64, f64, String)],
) -> (Vec<Bar>, Vec<Part>, Vec<usize>) {
    let mut bars: Vec<Bar> = measures
        .iter()
        .map(|&(start, end, number)| Bar {
            start,
            end,
            number,
            hit: 0,
            half: 0,
            miss: 0,
            part: None,
        })
        .collect();
    if bars.is_empty() {
        return (bars, Vec::new(), Vec::new());
    }
    let starts: Vec<f64> = bars.iter().map(|b| b.start).collect();
    let bar_of = |tm: f64| starts.partition_point(|&s| s <= tm + 1e-6).saturating_sub(1);
    for &(tm, j) in notes {
        // (a note before the first bar goes in it, one after the last in the last)
        let b = &mut bars[bar_of(tm)];
        match j {
            Judgement::Hit => b.hit += 1,
            Judgement::Late | Judgement::Early => b.half += 1,
            Judgement::Miss => b.miss += 1,
        }
    }
    let section_at = |tm: f64| {
        sections
            .iter()
            .rev()
            .find(|(a, _, _)| *a <= tm + 0.05)
            .map(|(_, _, l)| l.clone())
            .unwrap_or_default()
    };
    // parts that play once are named by their section, numbered when a section holds more than one
    let mut once_in: std::collections::HashMap<String, usize> = Default::default();
    for d in ideas.iter().filter(|d| !d.repeats()) {
        *once_in.entry(section_at(bars[d.starts[0]].start)).or_default() += 1;
    }
    let mut once_seen: std::collections::HashMap<String, usize> = Default::default();
    let mut riffs = 0;
    let mut parts: Vec<Part> = Vec::new();
    for (pi, d) in ideas.iter().enumerate() {
        let mut places = Vec::new();
        for (k, r) in d.places().enumerate() {
            let (f, l) = (r.start, (r.end - 1).min(bars.len() - 1));
            if f >= bars.len() {
                continue;
            }
            let (start, end) = (bars[f].start, bars[l].end);
            let mut in_place: Vec<(f64, Judgement)> = notes
                .iter()
                .copied()
                .filter(|&(tm, _)| (f..=l).contains(&bar_of(tm)))
                .collect();
            in_place.sort_by(|a, b| a.0.total_cmp(&b.0));
            for b in &mut bars[f..=l] {
                b.part = Some((pi, k));
            }
            let (na, nb) = (bars[f].number, bars[l].number);
            places.push(Place {
                first: f,
                last: l,
                start,
                end,
                bars: if na == nb {
                    format!("bar {na}")
                } else {
                    format!("bars {na}–{nb}")
                },
                notes: in_place.into_iter().map(|(_, j)| j).collect(),
                loss: bars[f..=l].iter().map(Bar::loss).sum(),
                marks: Vec::new(),
            });
        }
        if places.is_empty() {
            continue;
        }
        let first_sec = section_at(places[0].start);
        let (name, letter_s, hint) = if d.repeats() {
            let l = letter(riffs);
            riffs += 1;
            let hint = if first_sec.is_empty() {
                String::new()
            } else {
                format!("first in {first_sec}")
            };
            (format!("Riff {l}"), l, hint)
        } else {
            let name = if first_sec.is_empty() {
                places[0].bars.clone()
            } else if once_in.get(&first_sec).copied().unwrap_or(0) > 1 {
                let n = once_seen.entry(first_sec.clone()).or_default();
                *n += 1;
                format!("{first_sec} · {}", n)
            } else {
                first_sec.clone()
            };
            (name, String::new(), String::new())
        };
        // the worst time, the earliest of equals
        let at = (0..places.len())
            .max_by(|&a, &b| places[a].loss.total_cmp(&places[b].loss).then(b.cmp(&a)))
            .unwrap_or(0);
        let loss = places.iter().map(|p| p.loss).sum();
        parts.push(Part {
            name,
            letter: letter_s,
            hint,
            len: d.len,
            places,
            at,
            loss,
            passes: 0,
            last_pass: None,
            best: None,
        });
    }
    // names (from the charts agent's look at real songs, P29.3): the repeating idea covering the most bars, if it
    // plays 3+ times, is the main riff; one whose places sit mostly (3 in 4) in one kind of section that names
    // something (not Intro / Verse / Chorus / Riff...: charters put the same riff under all of them) is named by it;
    // the rest keep their letter. The hint says where it plays.
    let base = |tm: f64| {
        let l = section_at(tm);
        l.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ')
            .to_string()
    };
    let main = (0..parts.len())
        .filter(|&i| parts[i].places.len() >= 3)
        .max_by_key(|&i| (parts[i].len * parts[i].places.len(), std::cmp::Reverse(i)));
    const GENERIC: [&str; 8] = [
        "",
        "Intro",
        "Verse",
        "Chorus",
        "Pre-chorus",
        "Riff",
        "Outro",
        "Bridge",
    ];
    let mut taken: Vec<String> = Vec::new();
    for i in 0..parts.len() {
        if !parts[i].repeats() {
            continue;
        }
        let bases: Vec<String> = parts[i].places.iter().map(|pl| base(pl.start)).collect();
        let mut kinds: Vec<(String, usize)> = Vec::new();
        for b in &bases {
            match kinds.iter_mut().find(|(k, _)| k == b) {
                Some(e) => e.1 += 1,
                None => kinds.push((b.clone(), 1)),
            }
        }
        let shown: Vec<&str> = kinds
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, _)| k.as_str())
            .take(3)
            .collect();
        parts[i].hint = if shown.is_empty() {
            String::new()
        } else {
            format!("in {}", shown.join(", "))
        };
        let top = kinds.iter().max_by_key(|(_, n)| *n).cloned().unwrap_or_default();
        let name = if Some(i) == main {
            "Main riff".to_string()
        } else if top.1 * 4 >= bases.len() * 3 && !GENERIC.contains(&top.0.as_str()) {
            format!("{} riff", top.0)
        } else {
            parts[i].name.clone()
        };
        parts[i].name = if taken.contains(&name) {
            format!("{name} {}", parts[i].letter)
        } else {
            name
        };
        taken.push(parts[i].name.clone());
    }
    // (indices into `parts` may have shifted from `ideas` if one was empty: point the bars at the parts again)
    for (pi, p) in parts.iter().enumerate() {
        for (k, pl) in p.places.iter().enumerate() {
            for b in &mut bars[pl.first..=pl.last] {
                b.part = Some((pi, k));
            }
        }
    }
    let mut order: Vec<usize> = (0..parts.len()).filter(|&i| parts[i].loss >= BAD_PART).collect();
    order.sort_by(|&a, &b| parts[b].loss.total_cmp(&parts[a].loss).then(a.cmp(&b)));
    order.truncate(MAX_SPOTS);
    order.sort_unstable();
    (bars, parts, order)
}

/// A run's review, and where you are in it.
pub struct Review {
    pub part_key: String,
    pub bars: Vec<Bar>,
    pub parts: Vec<Part>,
    /// The parts listed (indices into `parts`).
    pub list: Vec<usize>,
    /// The run: notes judged, hits, late, early, misses.
    pub judged: usize,
    pub hit: usize,
    pub late: usize,
    pub early: usize,
    pub miss: usize,
    /// Parts that lost a note or more but weren't listed (more than MAX_SPOTS).
    pub more: usize,
    /// A few lines on the run.
    pub write_up: String,
    /// The page is up.
    pub open: bool,
    /// The row picked with the arrows (or the pointer), in `list`.
    pub key: usize,
    pub scroll: f32,
    scroll_max: f32,
    /// The list's height as last drawn.
    view_h: f32,
    /// The part being practised (in `parts`).
    pub current: Option<usize>,
    /// Which saved run this is (newest first), or none (played now, not saved).
    pub which: Option<usize>,
    /// The runs' list is open.
    pub menu: bool,
}

impl Review {
    pub fn new(
        part_key: &str,
        notes: &[(f64, Judgement)],
        measures: &[(f64, f64, i32)],
        ideas: &[Idea],
        sections: &[(f64, f64, String)],
    ) -> Review {
        let (bars, parts, list) = analyse(notes, measures, ideas, sections);
        let count = |f: fn(&Judgement) -> bool| notes.iter().filter(|(_, j)| f(j)).count();
        let bad = parts.iter().filter(|p| p.loss >= BAD_PART).count();
        let mut r = Review {
            part_key: part_key.to_string(),
            more: bad.saturating_sub(list.len()),
            bars,
            parts,
            list,
            judged: notes.len(),
            hit: count(|j| *j == Judgement::Hit),
            late: count(|j| *j == Judgement::Late),
            early: count(|j| *j == Judgement::Early),
            miss: count(|j| *j == Judgement::Miss),
            write_up: String::new(),
            open: false,
            key: 0,
            scroll: 0.0,
            scroll_max: 0.0,
            view_h: 0.0,
            current: None,
            which: None,
            menu: false,
        };
        r.write_up = write_up(&r);
        r
    }

    pub fn half(&self) -> usize {
        self.late + self.early
    }

    pub fn accuracy(&self) -> f64 {
        if self.judged == 0 {
            return 0.0;
        }
        (self.hit as f64 + 0.5 * self.half() as f64) / self.judged as f64
    }

    /// Lost in all.
    pub fn lost(&self) -> f64 {
        self.miss as f64 + 0.5 * self.half() as f64
    }

    /// The run's accuracy had every listed part been played clean.
    pub fn potential(&self) -> f64 {
        if self.judged == 0 {
            return 0.0;
        }
        let lost: f64 = self.list.iter().map(|&i| self.parts[i].loss).sum();
        ((self.hit as f64 + 0.5 * self.half() as f64 + lost) / self.judged as f64).min(1.0)
    }

    /// In short, for Home (P34.6): the parts listed, the worst ("Breakdown riff, bars 41–44", or for one that
    /// repeats "Main riff, plays 6×"), the potential.
    pub fn gist(&self) -> super::history::Gist {
        let worst = self
            .list
            .iter()
            .max_by(|&&a, &&b| self.parts[a].loss.total_cmp(&self.parts[b].loss).then(b.cmp(&a)))
            .map(|&w| {
                let p = &self.parts[w];
                let bars = &p.place().bars;
                if p.repeats() {
                    format!("{}, plays {}×", p.name, p.places.len())
                } else if p.name.contains(bars.as_str()) {
                    p.name.clone()
                } else {
                    format!("{}, {bars}", p.name)
                }
            })
            .unwrap_or_default();
        super::history::Gist {
            spots: self.list.len(),
            worst,
            potential: self.potential(),
        }
    }
}

/// A few lines on the run (experimental, owner request): how it went, what cost the most, how the repeats went,
/// the timing, the song's halves, what cleaning up would make it.
pub fn write_up(r: &Review) -> String {
    let mut s: Vec<String> = Vec::new();
    if r.judged == 0 {
        return String::new();
    }
    let acc = r.accuracy();
    s.push(
        match acc {
            a if a >= 0.995 => "A perfect run.",
            a if a >= 0.95 => "Close to perfect.",
            a if a >= 0.85 => "A solid run.",
            a if a >= 0.7 => "A decent run with a few rough patches.",
            _ => "A tough run: plenty to work on.",
        }
        .to_string(),
    );
    let lost = r.lost();
    if let Some(&w) = r
        .list
        .iter()
        .max_by(|&&a, &&b| r.parts[a].loss.total_cmp(&r.parts[b].loss))
        && lost > 0.0
    {
        let p = &r.parts[w];
        let share = (p.loss / lost * 100.0).round() as i64;
        if p.repeats() {
            s.push(format!(
                "{} costs the most ({share}% of what you lost): it plays {} times, so learning it once pays off {} times.",
                p.name,
                p.places.len(),
                p.places.len()
            ));
            // how its times went: falling off, or getting better
            let first = p.places[0].accuracy();
            let last = p.places[p.places.len() - 1].accuracy();
            if first - last >= 0.25 {
                s.push(format!(
                    "You had it the first time ({}) but it slipped to {} by the last: stamina or focus late in the song.",
                    pct(first),
                    pct(last)
                ));
            } else if last - first >= 0.25 {
                s.push(format!(
                    "It got better each time ({} to {}).",
                    pct(first),
                    pct(last)
                ));
            }
        } else {
            s.push(format!(
                "{} ({}) costs the most: {share}% of what you lost, in one go.",
                p.name, p.places[0].bars
            ));
        }
    }
    let half = r.half();
    if half * 10 >= r.judged && half >= 6 {
        let (word, n) = if r.late >= r.early {
            ("late", r.late)
        } else {
            ("early", r.early)
        };
        let advice = if word == "late" {
            "you're behind the beat"
        } else {
            "you're rushing"
        };
        s.push(format!(
            "Timing: {half} notes off the beat, mostly {word} ({n}): {advice}; a slower loop (Ctrl =) helps."
        ));
    }
    // the song's halves
    let n = r.bars.len();
    if n >= 8 {
        let acc_of = |bs: &[Bar]| {
            let (mut got, mut all) = (0.0, 0.0);
            for b in bs {
                got += b.hit as f64 + 0.5 * b.half as f64;
                all += b.notes() as f64;
            }
            (all > 0.0).then(|| got / all)
        };
        if let (Some(a), Some(b)) = (acc_of(&r.bars[..n / 2]), acc_of(&r.bars[n / 2..])) {
            if a - b >= 0.15 {
                s.push(format!(
                    "The second half was weaker ({} against {}).",
                    pct(b),
                    pct(a)
                ));
            } else if b - a >= 0.15 {
                s.push(format!(
                    "You warmed up: the second half went {} against {}.",
                    pct(b),
                    pct(a)
                ));
            }
        }
    }
    if !r.list.is_empty() {
        s.push(format!("Clean these up and you'd be at {}.", pct(r.potential())));
    }
    s.join(" ")
}

/// The colour of a judgement, or of an accuracy.
fn judged_color(j: Option<Judgement>) -> Color32 {
    let th = t();
    match j {
        Some(Judgement::Hit) => th.hit,
        Some(Judgement::Late | Judgement::Early) => th.late,
        Some(Judgement::Miss) => th.miss,
        None => th.text_2,
    }
}

fn acc_color(a: f64) -> Color32 {
    let th = t();
    if a >= CLEAN {
        th.hit
    } else if a >= 0.6 {
        th.late
    } else {
        th.miss
    }
}

fn pct(a: f64) -> String {
    format!("{}%", (a * 100.0).round() as i64)
}

/// A muted colour per part letter, for the map's lane.
fn lane_color(i: usize) -> Color32 {
    const HUES: [(u8, u8, u8); 8] = [
        (94, 129, 172),
        (163, 120, 170),
        (88, 160, 150),
        (190, 140, 90),
        (120, 150, 90),
        (170, 100, 110),
        (100, 140, 190),
        (150, 130, 100),
    ];
    let (r, g, b) = HUES[i % HUES.len()];
    Color32::from_rgb(r, g, b)
}

/// A small tab of one place: its strings, its bar lines, its notes as frets (dots when too close), each in the
/// colour of how the run went.
fn mini_tab(p: &egui::Painter, rect: Rect, place: &Place, bars: &[Bar], n_strings: usize) {
    let th = t();
    paint::display(p, rect, Some(8.0), 2.0);
    let inner = rect.shrink2(vec2(px(10.0), px(7.0)));
    let n = n_strings.max(2);
    let gap = inner.height() / (n - 1) as f32;
    // the staff: a line a string, as the tab draws it
    for s in 0..n {
        let y = inner.min.y + s as f32 * gap;
        p.hline(inner.x_range(), y, egui::Stroke::new(px(1.0), th.line));
    }
    let span = (place.end - place.start).max(1e-3);
    let x_of = |tm: f64| inner.min.x + ((tm - place.start) / span) as f32 * inner.width();
    // bar lines: its ends and each bar inside
    let mut xs = vec![inner.min.x, inner.max.x];
    xs.extend(
        bars[place.first + 1..=place.last.min(bars.len() - 1)]
            .iter()
            .map(|b| x_of(b.start)),
    );
    for x in xs {
        p.vline(x, inner.y_range(), egui::Stroke::new(px(1.0), th.bar_line));
    }
    // fret numbers on knock-outs of the screen's colour (a dot only where two would touch)
    let st = Style::new(10.0, 800, th.text);
    let mut last_x = vec![f32::NEG_INFINITY; n];
    for m in &place.marks {
        let x = x_of(m.t);
        // (string 0 is the lowest: drawn at the bottom, as the tab does)
        let row = (n - 1).saturating_sub(m.string.max(0) as usize).min(n - 1);
        let y = inner.min.y + row as f32 * gap;
        let c = judged_color(m.judged);
        let label = m.fret.to_string();
        let w = text::width(p.ctx(), &label, st);
        if x - last_x[row] >= w + px(1.0) {
            p.rect_filled(
                Rect::from_center_size(pos2(x, y), vec2(w + px(3.0), gap.min(px(11.0)))),
                px(2.0),
                th.screen,
            );
            txt(p, &label, pos2(x, y), 10.0, c, 800, Align2::CENTER_CENTER, None);
            last_x[row] = x;
        } else {
            paint::dot(p, pos2(x, y), px(2.0), c);
        }
    }
}

impl PlayerView {
    /// The review of the notes judged now (the run that just ended), replacing the last one.
    pub(super) fn make_review(&mut self) {
        let notes = self.judged_now();
        self.review = self.review_from(&notes);
    }

    /// The notes judged now: (chart time, judgement).
    fn judged_now(&self) -> Vec<(f64, Judgement)> {
        self.scorer
            .groups
            .iter()
            .filter_map(|g| g.state.map(|j| (g.t, j)))
            .collect()
    }

    /// The key of this song's part in reviews.json.
    pub(super) fn history_key(&self) -> String {
        format!("{}|{}", self.sid, self.part_name)
    }

    /// A run ended: it's saved with the last ones (the oldest past four goes) and reviewed.
    pub(super) fn record_review_run(&mut self) {
        let notes = self.judged_now();
        if notes.len() < 20 {
            // (a few notes aren't a run to keep: reviewed, not saved)
            self.review = self.review_from(&notes);
            return;
        }
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let got: f64 = notes.iter().map(|(_, j)| j.points() as f64 / 100.0).sum();
        self.review = self.review_from(&notes);
        let run = super::history::SavedRun {
            at,
            acc: got / notes.len() as f64,
            notes,
            gist: self.review.as_ref().map(Review::gist),
        };
        match super::history::save(&self.reviews_path, &self.history_key(), run) {
            Ok(runs) => {
                self.runs = runs;
                self.runs_for = self.history_key();
                if let Some(r) = self.review.as_mut() {
                    r.which = Some(0);
                }
            }
            Err(e) => crate::applog::write(&format!("review: couldn't save the run: {e}")),
        }
    }

    /// The saved run `k` (newest first) as the review, the page up.
    pub(super) fn pick_run(&mut self, k: usize) {
        let Some(run) = self.runs.get(k).cloned() else {
            return;
        };
        self.review = self.review_from(&run.notes);
        if let Some(r) = self.review.as_mut() {
            r.which = Some(k);
            r.open = true;
        }
    }

    /// The review of these judged notes on the part as it's charted now.
    fn review_from(&self, notes: &[(f64, Judgement)]) -> Option<Review> {
        if notes.is_empty() {
            return None;
        }
        let bars = qrs_analysis::similar::Bars::new(&self.chart.arr);
        if bars.bars.is_empty() {
            return None;
        }
        let measures: Vec<(f64, f64, i32)> = bars.bars.iter().map(|b| (b.t0, b.t1, b.number)).collect();
        let sections: Vec<(f64, f64, String)> = self
            .chart
            .sections
            .iter()
            .map(|s| (s.start, s.end, section_label(s)))
            .collect();
        // the charter's marks (sections and phrases) as bars, where one starts within a hair of a bar
        let mut marks: Vec<f64> = self.chart.sections.iter().map(|s| s.start).collect();
        marks.extend(self.chart.arr.phrases.iter().copied());
        let mut boundaries: Vec<usize> = marks
            .iter()
            .filter_map(|&m| bars.bars.iter().position(|b| (b.t0 - m).abs() < 0.08))
            .collect();
        boundaries.sort_unstable();
        boundaries.dedup();
        let sim = qrs_analysis::parts::similarity(&bars);
        let has_notes: Vec<bool> = bars.prints.iter().map(|p| !p.events.is_empty()).collect();
        let ideas = qrs_analysis::parts::ideas(&sim, &has_notes, &boundaries);
        let mut r = Review::new(&self.part_key, notes, &measures, &ideas, &sections);
        let mut sorted = notes.to_vec();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let judged_at = |t: f64| {
            let k = sorted.partition_point(|n| n.0 < t - 0.016);
            sorted.get(k).filter(|n| (n.0 - t).abs() <= 0.016).map(|n| n.1)
        };
        // each place's notes for its small tab, judged as the run went
        for part in &mut r.parts {
            for pl in &mut part.places {
                let lo = self.chart.gtimes.partition_point(|&x| x < pl.start - 1e-3);
                let hi = self.chart.gtimes.partition_point(|&x| x < pl.end - 1e-3);
                pl.marks = self.chart.glyphs[lo..hi]
                    .iter()
                    .map(|g| Mark {
                        t: g.t,
                        string: g.string,
                        fret: g.fret,
                        judged: g
                            .group
                            .and_then(|k| self.scorer.groups.get(k))
                            .and_then(|x| judged_at(x.t)),
                    })
                    .collect();
            }
        }
        if std::env::var_os("QUICKRS_REVIEW_DUMP").is_some() {
            // (to check the parts on a real song: each part, its bars, its places)
            for p in &r.parts {
                let places: Vec<String> = p.places.iter().map(|pl| pl.bars.clone()).collect();
                eprintln!(
                    "{:<22} {:>2} bars x{:<2} {}",
                    p.name,
                    p.len,
                    p.places.len(),
                    places.join(", ")
                );
            }
        }
        Some(r)
    }

    /// Screenshot mode: a run judged with four rough stretches (the rest hit, a late note now and then), its review
    /// up; or its second spot looped, two passes played (and in continuous mode, nine).
    pub(super) fn stage_review(&mut self, what: &str) {
        let spot = !matches!(what, "review" | "review_history");
        let len = self.chart.length.max(1.0);
        let rough = [(0.12, 0.03), (0.33, 0.05), (0.55, 0.02), (0.8, 0.06)];
        for (i, g) in self.scorer.groups.iter_mut().enumerate() {
            let f = g.t / len;
            let bad = rough.iter().any(|&(at, w)| (f - at).abs() < w);
            g.state = Some(if bad && i % 3 != 0 {
                Judgement::Miss
            } else if i % 13 == 0 {
                Judgement::Late
            } else {
                Judgement::Hit
            });
        }
        self.make_review();
        if spot {
            let Some(i) = self.review.as_ref().and_then(|r| r.list.get(1).copied()) else {
                return;
            };
            self.practise_part(i, None);
            self.countdown_until = None;
            self.transport.pause();
            let set = |me: &mut PlayerView, n: u32, a: f64| {
                if let Some(s) = me.review.as_mut().and_then(|r| r.parts.get_mut(i)) {
                    s.passes = n;
                    s.last_pass = Some(a);
                    s.best = Some(a);
                }
            };
            set(self, 2, 0.78);
            self.loop_passes = 2;
            self.loop_acc = Some(0.78);
            self.toast = None;
            if what == "review_continuous"
                && let Some((a, b)) = self.loop_a.zip(self.loop_b)
            {
                // nine passes played, the tenth under way: held 40% in, those notes judged
                self.continuous = true;
                let list = vec![0.55, 0.62, 0.71, 0.68, 0.8, 0.86, 0.93, 0.91, 0.94];
                set(self, 9, 0.94);
                self.loop_passes = list.len() as u32;
                self.loop_acc = list.last().copied();
                self.pass_hist.insert(super::continuous::loop_key(a, b), list);
                let at = a + 0.4 * (b - a);
                for (i, g) in self.scorer.groups.iter_mut().enumerate() {
                    if a <= g.t && g.t < at {
                        g.state = Some(if i % 7 == 0 {
                            Judgement::Miss
                        } else {
                            Judgement::Hit
                        });
                    }
                }
                self.transport.seek(at + self.offset);
                self.arcade_stage();
            }
        } else {
            self.show_review();
            if what == "review_history" {
                // three saved runs (not written: screenshot mode), the newest this one, the list open
                let notes = self.judged_now();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_secs_f64());
                self.runs = [(0.0, 0.76), (3600.0 * 20.0, 0.71), (3600.0 * 50.0, 0.64)]
                    .map(|(ago, acc)| super::history::SavedRun {
                        at: now - ago,
                        acc,
                        notes: notes.clone(),
                        gist: None,
                    })
                    .to_vec();
                self.runs_for = self.history_key();
                if let Some(r) = self.review.as_mut() {
                    r.which = Some(0);
                    r.menu = true;
                }
            }
        }
    }

    /// The page is up.
    pub(super) fn review_open(&self) -> bool {
        self.review.as_ref().is_some_and(|r| r.open)
    }

    /// The part being practised, while the loop set is its place.
    pub(super) fn review_spot(&self) -> Option<(usize, &Part)> {
        let r = self.review.as_ref()?;
        let i = r.current.filter(|_| r.part_key == self.part_key)?;
        let s = r.parts.get(i)?;
        let pl = s.place();
        let on = self
            .loop_a
            .zip(self.loop_b)
            .is_some_and(|(a, b)| (a - pl.start).abs() < 1e-3 && (b - pl.end).abs() < 1e-3);
        on.then_some((i, s))
    }

    /// E (or the results card's Review): the page, made from what was played if there's no review yet.
    pub(crate) fn show_review(&mut self) {
        if !self.review_may_open() {
            return;
        }
        if self.review.as_ref().is_some_and(|r| r.part_key != self.part_key) {
            self.review = None; // (another part: its own notes)
        }
        if self.review.is_none() {
            // the newest saved run first; without any, what's been played
            self.runs = super::history::load(&self.reviews_path, &self.history_key());
            self.runs_for = self.history_key();
            if self.runs.is_empty() {
                self.make_review();
            } else {
                self.pick_run(0);
            }
        }
        let Some(r) = self.review.as_mut() else {
            self.say_for(
                "Nothing to review yet: ",
                "play the song (or some of it), then press E",
                3.0,
            );
            return;
        };
        r.open = true;
        if let Some(c) = r.current
            && let Some(k) = r.list.iter().position(|&i| i == c)
        {
            r.key = k;
        }
        self.review_opening();
        self.hold();
    }

    /// Practise part `i` (in `parts`) at its place `place` (or the one it's at): those bars looped, from a little
    /// before.
    pub(super) fn practise_part(&mut self, i: usize, place: Option<usize>) {
        if self.review.as_ref().is_none_or(|r| r.parts.get(i).is_none()) {
            return;
        }
        self.spot_entering();
        let Some(r) = self.review.as_mut() else { return };
        let Some(s) = r.parts.get_mut(i) else { return };
        if let Some(k) = place {
            s.at = k.min(s.places.len() - 1);
        }
        let pl = s.place();
        let (a, b) = (pl.start, pl.end);
        let title = if s.repeats() {
            format!("{}  ·  {} of {} times", s.name, nth(s.at + 1), s.places.len())
        } else {
            s.name.clone()
        };
        let head = match r.list.iter().position(|&x| x == i) {
            Some(k) => format!("Spot {} of {}: ", k + 1, r.list.len()),
            None => String::new(),
        };
        r.open = false;
        r.current = Some(i);
        if let Some(k) = r.list.iter().position(|&x| x == i) {
            r.key = k;
        }
        self.summary_dismissed = true; // (back on the song: the results card stays closed)
        self.backdrop.clear();
        self.set_range(a, b, RangeFrom::Spot);
        let playing = self.transport.playing();
        self.seek((a - LOOP_LEAD_IN).max(0.0));
        if !playing {
            // (stopped: a count-in; playing, the song just jumps there and plays on)
            self.count_in();
        }
        self.say_for(head, title, 2.5);
    }

    /// The page's "Play the whole song": no range, from the top, counted in.
    pub(super) fn whole_song(&mut self) {
        self.review_return = None;
        self.close_review();
        self.clear_range();
        self.summary_dismissed = true;
        self.seek(0.0);
        self.count_in();
    }

    /// Leave the part: your range back (or none), the review kept.
    fn leave_spot(&mut self) {
        self.spot_left();
        if self.range().is_some() {
            self.say("Back to your loop", "");
        } else {
            self.say("Loop cleared", "");
        }
    }

    /// A loop pass ended: the part's practice so far.
    pub(super) fn review_pass(&mut self, acc: Option<f64>) {
        let Some(acc) = acc else { return };
        let Some((i, _)) = self.review_spot() else { return };
        let Some(s) = self.review.as_mut().and_then(|r| r.parts.get_mut(i)) else {
            return;
        };
        let first_clean = !s.clean() && acc >= CLEAN;
        s.passes += 1;
        s.last_pass = Some(acc);
        s.best = Some(s.best.map_or(acc, |b| b.max(acc)));
        if first_clean && !self.continuous {
            self.say_for(
                "Clean: ",
                format!("{} at 90%+  ·  on to the next spot?", pct(acc)),
                3.0,
            );
        }
    }

    pub(super) fn review_act(&mut self, name: &str) -> bool {
        match name {
            "review" => {
                if self.review_open() {
                    self.close_review();
                } else {
                    self.show_review();
                }
            }
            "review:close" => self.close_review(),
            "review:whole" => self.whole_song(),
            "review:menu" => {
                if let Some(r) = self.review.as_mut() {
                    r.menu = !r.menu;
                }
            }
            "review:leave" => self.leave_spot(),
            "review:next" | "review:prev" => {
                let Some(r) = self.review.as_ref() else {
                    return true;
                };
                let n = r.list.len();
                if n == 0 {
                    return true;
                }
                let k = match r.current.and_then(|c| r.list.iter().position(|&x| x == c)) {
                    Some(k) if name == "review:next" => (k + 1) % n,
                    Some(k) => (k + n - 1) % n,
                    None => 0,
                };
                let i = r.list[k];
                self.practise_part(i, None);
            }
            _ => {
                if let Some(k) = super::footer_keys::parse_recent(name) {
                    self.open_recent(k);
                } else if let Some(k) = name
                    .strip_prefix("review:run:")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    self.pick_run(k);
                } else if let Some(k) = name
                    .strip_prefix("review:spot:")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    if let Some(i) = self.review.as_ref().and_then(|r| r.list.get(k).copied()) {
                        self.practise_part(i, None);
                    }
                } else if let Some((i, k)) = name.strip_prefix("review:place:").and_then(|s| {
                    let (a, b) = s.split_once(':')?;
                    Some((a.parse().ok()?, b.parse().ok()?))
                }) {
                    self.practise_part(i, Some(k));
                } else {
                    return false;
                }
            }
        }
        true
    }

    fn close_review(&mut self) {
        let was_open = self.review_open();
        if let Some(r) = self.review.as_mut() {
            r.open = false;
        }
        if !self.summary_up() {
            self.backdrop.clear();
        }
        if was_open {
            self.review_closed(); // (back where the song was, playing again if it was)
        }
    }

    /// Keys and the wheel while the page is up (it's modal): true if the event was its.
    pub(super) fn review_event(&mut self, ev: &Event) -> bool {
        if !self.review_open() {
            return false;
        }
        let Some(r) = self.review.as_mut() else {
            return false;
        };
        let n = r.list.len();
        if let Event::Key { key, modifiers, .. } = ev
            && (matches!(key, Key::Equals | Key::Plus) || (*key == Key::C && modifiers.shift))
        {
            return false; // (the speed keys and the count-in key work over the page: D120)
        }
        match ev {
            Event::MouseWheel { unit, delta, .. } => {
                let notches = crate::library::wheel_notches(*unit, delta.y);
                r.scroll = (r.scroll - notches * px(ROW)).clamp(0.0, r.scroll_max);
            }
            Event::Key {
                key, pressed: true, ..
            } => match key {
                Key::Escape if r.menu => r.menu = false,
                Key::Escape | Key::E => self.close_review(),
                Key::Enter | Key::Space if n > 0 => {
                    let i = r.list[r.key.min(n - 1)];
                    self.practise_part(i, None);
                }
                Key::ArrowUp | Key::ArrowDown if n > 0 => {
                    let up = *key == Key::ArrowUp;
                    r.key = if up {
                        r.key.saturating_sub(1)
                    } else {
                        (r.key + 1).min(n - 1)
                    };
                    // (the row picked in sight)
                    let top = r.key as f32 * px(ROW);
                    r.scroll = r
                        .scroll
                        .max(top + px(ROW) - r.view_h)
                        .min(top)
                        .clamp(0.0, r.scroll_max);
                }
                _ => {}
            },
            _ => {}
        }
        true
    }

    /// The page over the blurred player: the run in two numbers and a few lines, its bars as a map with its parts
    /// under them, the parts to practise as a list.
    pub(super) fn draw_review(&mut self, ui: &Ui, area: Rect) {
        let Some(r) = self.review.as_ref() else { return };
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let modal = interact(ui, area, "review-modal", Sense::click());
        let pw = pixels(to_px(900.0).min(ipx(area.width()) - to_px(40.0)).trunc() as f32);
        let ph = snap((area.height() - px(40.0)).min(px(820.0)));
        let panel = Rect::from_min_size(
            ipos(
                area.min.x + snap((area.width() - pw) / 2.0),
                area.min.y + snap((area.height() - ph) / 2.0),
            ),
            vec2(pw, ph),
        );
        paint::floating(&p, panel, Some(32.0));
        let mut acts: Vec<String> = Vec::new();
        if modal.clicked() && modal.interact_pointer_pos().is_some_and(|at| !panel.contains(at)) {
            acts.push("review:close".into());
        }
        let (pad_x, pad_y) = (px(34.0), px(26.0));
        let (lx, rx) = (snap(panel.min.x + pad_x), snap(panel.max.x - pad_x));
        let mut y = snap(panel.min.y + pad_y);
        // the close key, Esc beside it
        let close = Rect::from_min_size(ipos(rx - px(34.0), y), Vec2::splat(px(34.0)));
        let close_resp = interact(ui, close, "review-close", Sense::click());
        paint::inset(
            &p,
            close,
            None,
            if close_resp.is_pointer_button_down_on() {
                4.5
            } else {
                3.0
            },
            Fill::Base,
        );
        icons::icon(
            &p,
            Icon::X,
            mid(close),
            13.0,
            if close_resp.hovered() { th.text } else { th.text_2 },
        );
        if close_resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if close_resp.clicked() {
            acts.push("review:close".into());
        }
        let esc = keycap(
            &p,
            "Esc",
            ipos(close.min.x - px(8.0), mid_y(close)),
            Align2::RIGHT_CENTER,
            11.0,
            CapOn::Material,
        );
        caps(&p, "SESSION REVIEW", pos2(lx, y), 12.0, Align2::LEFT_TOP, None);
        // which run: a saved one (newest first) or the one just played, a list of the others under it
        let run_label = |k: Option<usize>| match k.and_then(|k| self.runs.get(k).map(|run| (k, run))) {
            Some((k, run)) => format!(
                "{}{}  ·  {}",
                if k == 0 { "Latest  ·  " } else { "" },
                crate::home::fmt_when(Some(run.at), false),
                pct(run.acc)
            ),
            None => "Played now  ·  not saved".to_string(),
        };
        let pick_text = run_label(r.which);
        let pick_w = snap(text::width(&ctx, &pick_text, Style::new(13.0, 700, th.text)) + px(48.0));
        let pick_r = Rect::from_min_size(ipos(esc.min.x - px(14.0) - pick_w, y), vec2(pick_w, px(34.0)));
        let pick_resp = button(
            ui,
            pick_r,
            "review-runs",
            &pick_text,
            "",
            ButtonStyle {
                icon: Some(if r.menu { Icon::Up } else { Icon::Down }),
                size: 13.0,
                r: Some(12.0),
                enabled: !self.runs.is_empty(),
                ..Default::default()
            },
        );
        if pick_resp.clicked() {
            acts.push("review:menu".into());
        }
        txt(
            &p,
            &format!("{}  ·  {}", self.entry.display_title(), self.part_name),
            ipos(lx, y + px(18.0)),
            20.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(snap(pick_r.min.x - px(16.0) - lx)),
        );
        y = snap(y + px(44.0) + px(14.0));
        // the run, and the run with the listed parts clean
        let acc = r.accuracy();
        let a1 = txt(
            &p,
            &pct(acc),
            ipos(lx, y),
            34.0,
            acc_color(acc),
            800,
            Align2::LEFT_TOP,
            None,
        );
        txt(
            &p,
            "this run",
            ipos(lx, a1.max.y),
            12.0,
            th.caption,
            700,
            Align2::LEFT_TOP,
            None,
        );
        let mut nums_right = a1.max.x;
        if !r.list.is_empty() {
            let pot = r.potential();
            let ar = txt(
                &p,
                "→",
                ipos(a1.max.x + px(18.0), a1.min.y + px(6.0)),
                24.0,
                th.text_2,
                700,
                Align2::LEFT_TOP,
                None,
            );
            let a2 = txt(
                &p,
                &pct(pot),
                ipos(ar.max.x + px(18.0), y),
                34.0,
                acc_color(pot),
                800,
                Align2::LEFT_TOP,
                None,
            );
            let l2 = txt(
                &p,
                &format!(
                    "with {} clean",
                    if r.list.len() == 1 {
                        "this part".to_string()
                    } else {
                        format!("these {} parts", r.list.len())
                    }
                ),
                ipos(a2.min.x, a2.max.y),
                12.0,
                th.caption,
                700,
                Align2::LEFT_TOP,
                None,
            );
            nums_right = a2.max.x.max(l2.max.x);
        }
        let counts = format!(
            "{} notes  ·  {} missed  ·  {} late  ·  {} early{}",
            r.judged,
            r.miss,
            r.late,
            r.early,
            if r.more > 0 {
                format!("  ·  {} more parts lost a little", r.more)
            } else {
                String::new()
            }
        );
        txt(
            &p,
            &counts,
            ipos(rx, y + px(4.0)),
            13.0,
            th.text_2,
            600,
            Align2::RIGHT_TOP,
            None,
        );
        // the write-up, beside the numbers (up to four lines)
        let wx = snap(nums_right + px(32.0));
        let ww = snap(rx - wx);
        let wst = Style::new(13.0, 600, th.text);
        let mut wy = snap(y + px(24.0));
        for line in text::wrap(&ctx, &r.write_up, wst, ww).into_iter().take(4) {
            txt(
                &p,
                &line,
                pos2(wx, wy),
                13.0,
                th.text,
                600,
                Align2::LEFT_TOP,
                None,
            );
            wy = snap(wy + px(17.0));
        }
        y = snap(y.max(wy - px(24.0)) + px(62.0) + px(12.0)).max(snap(wy + px(12.0)));
        // the map: a column per bar, coloured by how it went; the listed parts numbered over it; under it a lane of
        // the song's parts (one colour and letter per repeating idea); a click anywhere opens that part there
        let map = Rect::from_min_size(pos2(lx, y), vec2(snap(rx - lx), px(104.0)));
        paint::display(&p, map, Some(14.0), 4.0);
        let inner = map.shrink2(vec2(px(12.0), px(10.0)));
        let nb = r.bars.len().max(1) as f32;
        let bw = inner.width() / nb;
        let col_top = snap(inner.min.y + px(20.0));
        let lane_h = px(18.0);
        let col_bot = snap(inner.max.y - lane_h - px(6.0));
        let col_h = snap(col_bot - col_top);
        let hover_pos = ctx.input(|i| i.pointer.hover_pos());
        let map_resp = interact(ui, inner, "review-map", Sense::click());
        let hover_bar = hover_pos
            .filter(|h| inner.contains(*h))
            .map(|h| (((h.x - inner.min.x) / bw) as usize).min(r.bars.len().saturating_sub(1)));
        let hover_part = hover_bar.and_then(|b| r.bars.get(b)).and_then(|b| b.part);
        let key_part = r.list.get(r.key).copied();
        let mut map_hover: Option<usize> = None;
        // the hovered part's places (or the picked row's), lit behind the columns
        let lit = hover_part.map(|(pi, _)| pi).or(key_part);
        if let Some(pi) = lit
            && let Some(part) = r.parts.get(pi)
        {
            for (k, pl) in part.places.iter().enumerate() {
                let x0 = inner.min.x + pl.first as f32 * bw;
                let x1 = inner.min.x + (pl.last + 1) as f32 * bw;
                let strong = hover_part == Some((pi, k)) || hover_part.is_none() && k == part.at;
                p.rect_filled(
                    Rect::from_min_max(
                        pos2(x0, inner.min.y),
                        pos2(x1.max(x0 + px(3.0)), col_bot + px(2.0)),
                    ),
                    px(4.0),
                    th.accent.gamma_multiply(if strong { 0.3 } else { 0.12 }),
                );
            }
        }
        // the listed parts' numbers over their worst place
        for (k, &pi) in r.list.iter().enumerate() {
            let pl = r.parts[pi].place();
            let cx = inner.min.x + (pl.first + pl.last + 1) as f32 * bw / 2.0;
            let hot = lit == Some(pi);
            txt(
                &p,
                &(k + 1).to_string(),
                pos2(cx, inner.min.y + px(1.0)),
                12.0,
                if hot { th.accent_ink } else { th.text_2 },
                800,
                Align2::CENTER_TOP,
                None,
            );
        }
        for (i, b) in r.bars.iter().enumerate() {
            let x = inner.min.x + i as f32 * bw;
            let w = (bw - px(1.0)).max(px(1.0));
            let (h, c) = match b.accuracy() {
                Some(a) => (col_h * (0.35 + 0.65 * (1.0 - a as f32)).min(1.0), acc_color(a)),
                None => (px(2.0), th.unlit),
            };
            p.rect_filled(
                Rect::from_min_size(pos2(x, col_bot - h), vec2(w, h)),
                px(1.5).min(w / 2.0),
                c,
            );
        }
        // the lane: each place of each part, its letter when there's room
        let lane = Rect::from_min_max(pos2(inner.min.x, inner.max.y - lane_h), inner.max);
        for (pi, part) in r.parts.iter().enumerate() {
            let ci = if part.repeats() {
                part.letter_index()
            } else {
                usize::MAX
            };
            for (k, pl) in part.places.iter().enumerate() {
                let x0 = lane.min.x + pl.first as f32 * bw + px(0.5);
                let x1 = lane.min.x + (pl.last + 1) as f32 * bw - px(0.5);
                let seg = Rect::from_min_max(pos2(x0, lane.min.y), pos2(x1.max(x0 + px(2.0)), lane.max.y));
                let col = if ci == usize::MAX {
                    th.unlit
                } else {
                    lane_color(ci)
                };
                let hot = hover_part.is_some_and(|(h, _)| h == pi);
                p.rect_filled(seg, px(3.0), if hot { col } else { col.gamma_multiply(0.75) });
                if hover_part == Some((pi, k)) {
                    p.rect_stroke(
                        seg,
                        px(3.0),
                        egui::Stroke::new(px(1.5), th.text),
                        egui::StrokeKind::Outside,
                    );
                }
                let label = if part.repeats() { part.letter.as_str() } else { "" };
                if !label.is_empty() && seg.width() > px(12.0) {
                    txt(
                        &p,
                        label,
                        seg.center(),
                        10.0,
                        Color32::WHITE,
                        800,
                        Align2::CENTER_CENTER,
                        None,
                    );
                }
            }
        }
        if let Some((pi, k)) = hover_part {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            map_hover = r.list.iter().position(|&x| x == pi);
            let part = &r.parts[pi];
            let pl = &part.places[k];
            let what = if part.repeats() {
                format!(
                    "{}  ·  {}  ·  {} of {}",
                    part.name,
                    pl.bars,
                    nth(k + 1),
                    part.places.len()
                )
            } else {
                format!("{}  ·  {}", part.name, pl.bars)
            };
            if let Some(h) = hover_pos {
                self_tip(
                    &p,
                    &what,
                    &format!("{}  ·  click to play it", pct(pl.accuracy())),
                    h,
                    area,
                );
            }
            if map_resp.clicked() {
                acts.push(format!("review:place:{pi}:{k}"));
            }
        }
        y = snap(map.max.y + px(16.0));
        // the list
        let foot_h = px(54.0);
        let list = Rect::from_min_max(
            pos2(lx, y),
            pos2(rx, snap(panel.max.y - pad_y - foot_h - px(16.0))),
        );
        paint::well(&p, list, 18.0);
        let clip = list.shrink(px(6.0));
        let lp = p.with_clip_rect(clip);
        let rh = px(ROW);
        let full = r.list.len() as f32 * rh;
        let most = (full - clip.height()).max(0.0);
        let sc = r.scroll.clamp(0.0, most);
        let mut pick: Option<usize> = map_hover;
        if r.list.is_empty() {
            txt(
                &p,
                "Every part went to plan. Next: the same at full speed and difficulty, or a harder part.",
                list.center(),
                15.0,
                th.text_2,
                600,
                Align2::CENTER_CENTER,
                Some(snap(list.width() - px(40.0))),
            );
        }
        let n_strings = self.chart.n_strings;
        for (row_i, &pi) in r.list.iter().enumerate() {
            let s = &r.parts[pi];
            let pl = s.place();
            let ry = snap(clip.min.y + row_i as f32 * rh - sc);
            let row = Rect::from_min_size(
                pos2(clip.min.x, ry),
                vec2(
                    clip.width() - if most > 0.0 { px(12.0) } else { 0.0 },
                    rh - px(4.0),
                ),
            );
            if row.max.y < clip.min.y || row.min.y > clip.max.y {
                continue;
            }
            let vis = row.intersect(clip);
            let resp = interact(ui, vis, ("review-row", row_i), Sense::click());
            if resp.hovered() {
                pick = Some(row_i);
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                acts.push(format!("review:spot:{row_i}"));
            }
            if r.key == row_i || resp.hovered() {
                paint::raised(&lp, row, Some(14.0), 3.0, Fill::Base, 0.0);
            }
            let cy = mid_y(row);
            // its number, in its lane colour when it repeats
            let badge = Rect::from_center_size(pos2(row.min.x + px(30.0), cy), Vec2::splat(px(32.0)));
            paint::inset(&lp, badge, None, 2.0, Fill::Base);
            txt(
                &lp,
                &(row_i + 1).to_string(),
                badge.center(),
                15.0,
                th.accent_ink,
                800,
                Align2::CENTER_CENTER,
                None,
            );
            let x = snap(row.min.x + px(60.0));
            // on the right: this run's accuracy over every time it plays, then practice so far
            let right = snap(row.max.x - px(16.0));
            icons::icon(&lp, Icon::Right, pos2(right - px(6.0), cy), 12.0, th.text_2);
            let px_ = right - px(26.0);
            let (prac, prac_col) = match (s.last_pass, s.best) {
                (Some(last), Some(best)) => (
                    format!(
                        "{} pass{}  ·  last {}  ·  best {}",
                        s.passes,
                        if s.passes == 1 { "" } else { "es" },
                        pct(last),
                        pct(best)
                    ),
                    if s.clean() { th.hit } else { th.text_2 },
                ),
                _ => ("not practised yet".to_string(), th.caption),
            };
            let pr = txt(
                &lp,
                &prac,
                pos2(px_, cy + px(14.0)),
                12.0,
                prac_col,
                700,
                Align2::RIGHT_CENTER,
                None,
            );
            if s.clean() {
                icons::icon(
                    &lp,
                    Icon::Check,
                    pos2(pr.min.x - px(12.0), cy + px(14.0)),
                    12.0,
                    th.hit,
                );
            }
            let ar = txt(
                &lp,
                &pct(s.accuracy()),
                pos2(px_, cy - px(10.0)),
                22.0,
                acc_color(s.accuracy()),
                800,
                Align2::RIGHT_CENTER,
                None,
            );
            let stat_x = ar.min.x.min(pr.min.x - px(20.0)) - px(16.0);
            // the name, its shape, where its worst time is
            let text_w = px(250.0).min((stat_x - x) * 0.42);
            if s.repeats() {
                let chip = Rect::from_min_size(pos2(x, cy - px(30.0)), vec2(px(22.0), px(18.0)));
                p.with_clip_rect(clip)
                    .rect_filled(chip, px(4.0), lane_color(s.letter_index()));
                txt(
                    &lp,
                    &s.letter,
                    chip.center(),
                    11.0,
                    Color32::WHITE,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
            let name_x = if s.repeats() { x + px(30.0) } else { x };
            txt(
                &lp,
                &s.name,
                pos2(name_x, cy - px(21.0)),
                16.0,
                th.text,
                800,
                Align2::LEFT_CENTER,
                Some(snap(text_w - (name_x - x))),
            );
            txt(
                &lp,
                &s.shape(),
                pos2(x, cy - px(1.0)),
                12.0,
                th.text_2,
                700,
                Align2::LEFT_CENTER,
                Some(text_w),
            );
            let where_ = if s.repeats() {
                format!(
                    "worst: the {} time, {}  ·  {}",
                    nth(s.at + 1),
                    pl.bars,
                    pct(pl.accuracy())
                )
            } else {
                format!("{}  ·  {}–{}", pl.bars, fmt_time(pl.start), fmt_time(pl.end))
            };
            txt(
                &lp,
                &where_,
                pos2(x, cy + px(15.0)),
                12.0,
                th.caption,
                600,
                Align2::LEFT_CENTER,
                Some(text_w),
            );
            if !s.hint.is_empty() {
                txt(
                    &lp,
                    &s.hint,
                    pos2(x, cy + px(31.0)),
                    11.0,
                    th.caption,
                    600,
                    Align2::LEFT_CENTER,
                    Some(text_w),
                );
            }
            // its worst time as a small tab
            let tab_x0 = snap(x + text_w + px(14.0));
            if stat_x - tab_x0 > px(80.0) {
                let tab = Rect::from_min_max(
                    pos2(tab_x0, row.min.y + px(10.0)),
                    pos2(stat_x, row.max.y - px(10.0)),
                );
                mini_tab(&lp, tab, pl, &r.bars, n_strings);
            }
        }
        if most > 0.0 {
            let track = Rect::from_min_size(
                ipos(list.max.x - px(12.0), list.min.y + px(10.0)),
                vec2(px(4.0), snap(list.height() - px(20.0))),
            );
            let thh = px(26.0).max(snap(track.height() * clip.height() / full));
            paint::inset(&p, track, None, 2.0, Fill::Base);
            paint::pill(
                &p,
                Rect::from_min_size(
                    ipos(track.min.x, track.min.y + (track.height() - thh) * sc / most),
                    vec2(track.width(), thh),
                ),
                th.tick,
            );
        }
        // the foot: the whole song again, and how to use the page
        let fy = snap(panel.max.y - pad_y - foot_h);
        let whole_w = snap(button_w(&ctx, "Play the whole song", "", 15.0, true) + px(20.0));
        if button(
            ui,
            Rect::from_min_size(pos2(rx - whole_w, fy), vec2(whole_w, foot_h)),
            "review-whole",
            "Play the whole song",
            "",
            ButtonStyle {
                icon: Some(Icon::Restart),
                size: 15.0,
                r: Some(17.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            acts.push("review:whole".into());
        }
        let k1 = keycap(
            &p,
            "↑↓",
            ipos(lx, fy + foot_h / 2.0),
            Align2::LEFT_CENTER,
            11.0,
            CapOn::Material,
        );
        let k2 = keycap(
            &p,
            "Enter",
            ipos(k1.max.x + px(6.0), fy + foot_h / 2.0),
            Align2::LEFT_CENTER,
            11.0,
            CapOn::Material,
        );
        txt(
            &p,
            "loop a part  ·  click the map anywhere to play that part  ·  E brings this page back",
            ipos(k2.max.x + px(10.0), fy + foot_h / 2.0),
            13.0,
            th.caption,
            600,
            Align2::LEFT_CENTER,
            Some(snap(rx - whole_w - px(20.0) - k2.max.x)),
        );
        // the runs' list, over everything (a click outside closes it)
        if r.menu {
            let rh2 = px(36.0);
            let mw = px(300.0).max(pick_r.width());
            let menu = Rect::from_min_size(
                pos2(pick_r.max.x - mw, pick_r.max.y + px(6.0)),
                vec2(mw, rh2 * self.runs.len() as f32 + px(12.0)),
            );
            let catch = interact(ui, panel, "review-runs-catch", Sense::click());
            if catch.clicked() && catch.interact_pointer_pos().is_some_and(|at| !menu.contains(at)) {
                acts.push("review:menu".into());
            }
            paint::floating(&p, menu, Some(14.0));
            for k in 0..self.runs.len() {
                let row = Rect::from_min_size(
                    pos2(menu.min.x + px(6.0), menu.min.y + px(6.0) + k as f32 * rh2),
                    vec2(menu.width() - px(12.0), rh2 - px(2.0)),
                );
                let resp = interact(ui, row, ("review-run", k), Sense::click());
                if resp.hovered() || r.which == Some(k) {
                    paint::raised(&p, row, Some(10.0), 2.0, Fill::Base, 0.0);
                }
                if resp.hovered() {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                txt(
                    &p,
                    &run_label(Some(k)),
                    pos2(row.min.x + px(12.0), mid_y(row)),
                    13.0,
                    if r.which == Some(k) {
                        th.accent_ink
                    } else {
                        th.text
                    },
                    700,
                    Align2::LEFT_CENTER,
                    Some(row.width() - px(24.0)),
                );
                if resp.clicked() {
                    acts.push(format!("review:run:{k}"));
                }
            }
        }
        let n = r.list.len();
        if let Some(r) = self.review.as_mut() {
            r.scroll_max = most;
            r.scroll = sc;
            r.view_h = clip.height();
            if let Some(i) = pick {
                r.key = i;
            }
            r.key = r.key.min(n.saturating_sub(1));
        }
        for a in acts {
            if a == "review:whole" {
                self.whole_song();
            } else {
                self.review_act(&a);
            }
        }
    }

    /// The strip over the tab while a part is practised: which one, how it's going, the next one, the list. Its
    /// rect, for what sits beside or over it.
    pub(super) fn draw_review_strip(&mut self, ui: &Ui, well: Rect) -> Option<Rect> {
        let (i, s) = self.review_spot()?;
        let (k, n) = self
            .review
            .as_ref()
            .map_or((None, 0), |r| (r.list.iter().position(|&x| x == i), r.list.len()));
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let h = px(56.0);
        let pl = s.place();
        let title = if s.repeats() {
            format!("{}  ·  {} of {} times", s.name, nth(s.at + 1), s.places.len())
        } else {
            s.name.clone()
        };
        let line = match (s.last_pass, s.best) {
            (Some(last), Some(best)) => format!(
                "{}  ·  was {}  ·  last pass {}  ·  best {}",
                pl.bars,
                pct(pl.accuracy()),
                pct(last),
                pct(best)
            ),
            _ => format!(
                "{}  ·  was {} this run  ·  {} of {} missed",
                pl.bars,
                pct(pl.accuracy()),
                pl.missed(),
                pl.notes.len()
            ),
        };
        let line_col = if s.clean() { th.hit } else { th.text_2 };
        let tw = text::width(&ctx, &title, Style::new(14.0, 800, th.text)).max(text::width(
            &ctx,
            &line,
            Style::new(12.0, 700, th.text),
        ));
        let btn = px(34.0);
        let w = snap(
            (px(16.0) + px(92.0) + tw + px(16.0) + 5.0 * btn + 4.0 * px(6.0) + px(12.0))
                .min(well.width() * 0.6),
        );
        // (at the tab's foot: the row being played and the toolbar stay in sight)
        let plate = Rect::from_min_size(ipos(well.min.x + px(14.0), well.max.y - px(12.0) - h), vec2(w, h));
        paint::raised(&p, plate, Some(18.0), 6.0, Fill::Base, 0.0);
        let cy = mid_y(plate);
        let x = snap(plate.min.x + px(16.0));
        caps(
            &p,
            "REVIEW",
            pos2(x, cy - px(9.0)),
            11.0,
            Align2::LEFT_CENTER,
            None,
        );
        txt(
            &p,
            &match k {
                Some(k) => format!("{} / {n}", k + 1),
                None => "part".into(),
            },
            pos2(x, cy + px(9.0)),
            14.0,
            th.accent_ink,
            800,
            Align2::LEFT_CENTER,
            None,
        );
        let bx0 = snap(plate.max.x - px(12.0) - 5.0 * btn - 4.0 * px(6.0));
        let tx = snap(x + px(76.0));
        txt(
            &p,
            &title,
            pos2(tx, cy - px(9.0)),
            14.0,
            th.text,
            800,
            Align2::LEFT_CENTER,
            Some(snap(bx0 - tx - px(10.0))),
        );
        txt(
            &p,
            &line,
            pos2(tx, cy + px(10.0)),
            12.0,
            line_col,
            700,
            Align2::LEFT_CENTER,
            Some(snap(bx0 - tx - px(10.0))),
        );
        let keys: [(Icon, &str, &str); 5] = [
            (
                Icon::Restart,
                "continuous",
                "Continuous: plays on, every pass scored (O)",
            ),
            (Icon::Left, "review:prev", "The part before"),
            (Icon::Grid, "review", "Every part (E)"),
            (Icon::Right, "review:next", "The next part"),
            (Icon::X, "review:leave", "Stop looping it"),
        ];
        for (k, (icon, act, tip)) in keys.into_iter().enumerate() {
            let r = Rect::from_min_size(
                ipos(bx0 + k as f32 * (btn + px(6.0)), cy - btn / 2.0),
                Vec2::splat(btn),
            );
            // (P38.1, D151: continuous is the practice mode, so its key is amber glass, breathing until it's on)
            let resp = if act == "continuous" {
                let t_now = ctx.input(|i| i.time);
                super::footer_keys::continuous_key(ui, r, ("review-strip", k), self.continuous, t_now)
            } else {
                button(
                    ui,
                    r,
                    ("review-strip", k),
                    "",
                    "",
                    ButtonStyle {
                        icon: Some(icon),
                        size: 13.0,
                        r: Some(11.0),
                        ..Default::default()
                    },
                )
            };
            if resp.hovered() {
                let (label, key) = match (act, self.continuous) {
                    ("continuous", false) => (
                        "Continuous practice: the loop plays on without stopping, every pass scored"
                            .to_string(),
                        "O".to_string(),
                    ),
                    ("continuous", true) => (
                        "Continuous practice is on, every pass scored: click to stop".to_string(),
                        "O".to_string(),
                    ),
                    _ => (tip.to_string(), String::new()),
                };
                self.tip = Some((
                    label,
                    key,
                    pos2(r.center().x, r.min.y - px(8.0)),
                    Align2::CENTER_BOTTOM,
                ));
            }
            if resp.clicked() {
                self.acts.push(act.into());
            }
        }
        Some(plate)
    }
}

impl Part {
    /// Its letter as a number (A 0, B 1...), for its lane colour.
    fn letter_index(&self) -> usize {
        self.letter
            .bytes()
            .fold(0usize, |n, c| n * 26 + (c.saturating_sub(b'A') as usize + 1))
            .saturating_sub(1)
    }
}

/// A tooltip at the pointer: the part and how it went.
fn self_tip(p: &egui::Painter, label: &str, key: &str, at: egui::Pos2, area: Rect) {
    super::draw_tip(
        p,
        label,
        key,
        pos2(at.x, at.y - px(14.0)),
        Align2::CENTER_BOTTOM,
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use Judgement::{Hit, Late, Miss};

    /// Bars of 2 s from 0, numbered from 1.
    fn bars(n: usize) -> Vec<(f64, f64, i32)> {
        (0..n)
            .map(|i| (i as f64 * 2.0, i as f64 * 2.0 + 2.0, i as i32 + 1))
            .collect()
    }

    /// Four notes a bar, each bar's judgements given.
    fn run(per_bar: &[[Judgement; 4]]) -> Vec<(f64, Judgement)> {
        let mut v = Vec::new();
        for (b, js) in per_bar.iter().enumerate() {
            for (k, j) in js.iter().enumerate() {
                v.push((b as f64 * 2.0 + k as f64 * 0.5, *j));
            }
        }
        v
    }

    fn idea(len: usize, starts: &[usize]) -> Idea {
        Idea {
            len,
            starts: starts.to_vec(),
            likeness: 1.0,
        }
    }

    const OK: [Judgement; 4] = [Hit, Hit, Hit, Hit];
    const ONE: [Judgement; 4] = [Hit, Miss, Hit, Hit];
    const BAD: [Judgement; 4] = [Miss, Miss, Late, Hit];

    #[test]
    fn a_clean_run_lists_nothing() {
        let (b, p, l) = analyse(&run(&[OK, OK, OK, OK]), &bars(4), &[idea(2, &[0, 2])], &[]);
        assert_eq!(b.len(), 4);
        assert_eq!(p.len(), 1);
        assert!(l.is_empty());
    }

    #[test]
    fn a_riff_is_listed_once_at_its_worst_time() {
        // riff A (2 bars) three times: the second time bad; a solo of 2 bars, clean
        let per = [OK, OK, BAD, ONE, OK, OK, OK, OK];
        let ideas = [idea(2, &[0, 2, 4]), idea(2, &[6])];
        let (b, p, l) = analyse(&run(&per), &bars(8), &ideas, &[(12.0, 16.0, "Solo".into())]);
        assert_eq!(l, vec![0]);
        assert_eq!(p[0].name, "Main riff");
        assert_eq!(p[0].letter, "A");
        assert_eq!(p[0].at, 1);
        assert_eq!(p[0].place().bars, "bars 3–4");
        assert_eq!(p[0].shape(), "2 bars  ·  plays 3×");
        assert!((p[0].loss - 3.5).abs() < 1e-9);
        assert_eq!(p[1].name, "Solo");
        assert_eq!(b[3].part, Some((0, 1)));
        assert_eq!(b[7].part, Some((1, 0)));
    }

    #[test]
    fn parts_that_play_once_in_one_section_are_numbered() {
        let ideas = [idea(2, &[0]), idea(2, &[2])];
        let (_, p, _) = analyse(&run(&[OK; 4]), &bars(4), &ideas, &[(0.0, 8.0, "Solo".into())]);
        assert_eq!((p[0].name.as_str(), p[1].name.as_str()), ("Solo · 1", "Solo · 2"));
        let (_, p, _) = analyse(&run(&[OK; 2]), &bars(2), &[idea(2, &[0])], &[]);
        assert_eq!(p[0].name, "bars 1–2");
    }

    #[test]
    fn only_the_worst_parts_are_listed_in_song_order() {
        let mut per = Vec::new();
        let mut ideas = Vec::new();
        for i in 0..10 {
            per.push(if i % 2 == 0 { BAD } else { ONE });
            ideas.push(idea(1, &[i]));
        }
        let r = Review::new("lead", &run(&per), &bars(10), &ideas, &[]);
        assert_eq!(r.list.len(), MAX_SPOTS);
        assert_eq!(r.more, 2);
        assert!(r.list.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn the_potential_counts_the_listed_parts_clean() {
        let r = Review::new(
            "lead",
            &run(&[OK, BAD, OK, OK]),
            &bars(4),
            &[idea(2, &[0, 2])],
            &[],
        );
        assert_eq!(r.judged, 16);
        assert!((r.accuracy() - 13.5 / 16.0).abs() < 1e-9);
        assert!((r.potential() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_write_up_names_what_cost_the_most() {
        let per = [OK, OK, BAD, BAD, OK, OK, OK, OK];
        let r = Review::new("lead", &run(&per), &bars(8), &[idea(2, &[0, 2, 4, 6])], &[]);
        assert!(r.write_up.contains("Main riff costs the most"), "{}", r.write_up);
        assert!(r.write_up.contains("plays 4 times"));
        assert!(r.write_up.ends_with("you'd be at 100%."));
    }

    #[test]
    fn the_gist_names_the_worst_part() {
        let per = [OK, OK, BAD, BAD, OK, OK, OK, OK];
        let r = Review::new("lead", &run(&per), &bars(8), &[idea(2, &[0, 2, 4, 6])], &[]);
        let g = r.gist();
        assert_eq!((g.spots, g.worst.as_str()), (1, "Main riff, plays 4×"));
        assert!((g.potential - 1.0).abs() < 1e-9);
        // a part that plays once: its name and bars; one named by its bars isn't said twice
        let ideas = [idea(2, &[0]), idea(2, &[2])];
        let per = [OK, OK, BAD, ONE];
        let solo = [(4.0, 8.0, "Solo".to_string())];
        let r = Review::new("lead", &run(&per), &bars(4), &ideas, &solo);
        assert_eq!(r.gist().worst, "Solo, bars 3–4");
        let r = Review::new("lead", &run(&per), &bars(4), &ideas, &[]);
        assert_eq!(r.gist().worst, "bars 3–4");
        let r = Review::new("lead", &run(&[OK; 4]), &bars(4), &ideas, &[]);
        assert_eq!((r.gist().spots, r.gist().worst.as_str()), (0, ""));
    }

    #[test]
    fn ordinals() {
        assert_eq!(
            (nth(1), nth(2), nth(3), nth(4), nth(11), nth(22)),
            (
                "1st".into(),
                "2nd".into(),
                "3rd".into(),
                "4th".into(),
                "11th".into(),
                "22nd".into()
            )
        );
    }
}
