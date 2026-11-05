//! The run in numbers for the results card and the score tile (new in quickRS, P37.6d after the RS veteran's
//! review): best runs with repeats folded together, accuracy per section, the techniques that lost the most notes,
//! and the change since the last run. Pure functions over the scorer's judgements and the chart.

use eframe::egui;
use egui::{Align2, Color32, Painter, Pos2, Rect, pos2, vec2};
use qrs_analysis::Judgement;
use qrs_formats::Techs;
use qrs_formats::ascii_tab::section_label;
use qrs_library::userdata::Run;
use qrs_ui::paint;
use qrs_ui::raster::mix;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;

use super::{PlayerView, ipos, snap};
use crate::library::{caps, txt};

/// A best run and how many times exactly that run (score and accuracy) was played.
#[derive(Clone, Debug, PartialEq)]
pub struct BestRun {
    pub run: Run,
    pub times: usize,
    /// Its first index in the list it came from (for "this run").
    pub first: usize,
    /// Every index it stands for.
    pub of: Vec<usize>,
}

/// The best runs with identical neighbours (same score and accuracy to the percent) folded into one row: three
/// "38,900 · 84%" rows said nothing the first didn't. The newest of the repeats gives the date.
pub fn collapse_runs(top: &[Run]) -> Vec<BestRun> {
    let pct = |r: &Run| r.acc.map(|a| (a * 100.0).round() as i64);
    let mut out: Vec<BestRun> = Vec::new();
    for (i, r) in top.iter().enumerate() {
        if let Some(last) = out.last_mut()
            && last.run.score == r.score
            && pct(&last.run) == pct(r)
        {
            last.times += 1;
            last.of.push(i);
            if r.at.unwrap_or(0.0) > last.run.at.unwrap_or(0.0) {
                last.run.at = r.at;
            }
            continue;
        }
        out.push(BestRun {
            run: r.clone(),
            times: 1,
            first: i,
            of: vec![i],
        });
    }
    out
}

/// "1st", "2nd", "3rd", "4th"...
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// How a stretch of the song went: notes judged and the points they made (a hit 1, late or early ½).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub judged: u32,
    pub missed: u32,
    pub points: f64,
}

impl Tally {
    fn add(&mut self, j: Judgement) {
        self.judged += 1;
        self.points += match j {
            Judgement::Hit => 1.0,
            Judgement::Miss => 0.0,
            _ => 0.5,
        };
        if j == Judgement::Miss {
            self.missed += 1;
        }
    }

    pub fn acc(&self) -> f64 {
        if self.judged == 0 {
            0.0
        } else {
            self.points / self.judged as f64
        }
    }
}

/// One section's accuracy: its name, start, end and tally (sections with no judged notes are left out).
#[derive(Clone, Debug, PartialEq)]
pub struct SectionAcc {
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub tally: Tally,
}

/// Accuracy per section: `sections` (its label as the tab shows it, start, end) in order, `judged` (note time,
/// judgement). Sections with no judged notes are left out.
pub fn by_section(sections: &[(String, f64, f64)], judged: &[(f64, Judgement)]) -> Vec<SectionAcc> {
    sections
        .iter()
        .filter_map(|(name, a, b)| {
            let mut t = Tally::default();
            for &(nt, j) in judged {
                if nt >= *a - 1e-3 && nt < *b - 1e-3 {
                    t.add(j);
                }
            }
            (t.judged > 0).then(|| SectionAcc {
                name: name.clone(),
                start: *a,
                end: *b,
                tally: t,
            })
        })
        .collect()
}

/// The techniques the results card reports on, in the order it lists ties: (flags, plural name).
pub const TECHS: [(Techs, &str); 9] = [
    (Techs(Techs::BEND.0), "bends"),
    (Techs(Techs::SLIDE.0 | Techs::UNPITCHED_SLIDE.0), "slides"),
    (
        Techs(Techs::HAMMER_ON.0 | Techs::PULL_OFF.0),
        "hammer-ons and pull-offs",
    ),
    (Techs(Techs::PALM_MUTE.0), "palm mutes"),
    (Techs(Techs::HARMONIC.0 | Techs::PINCH_HARMONIC.0), "harmonics"),
    (Techs(Techs::TREMOLO.0), "tremolo"),
    (Techs(Techs::TAP.0), "taps"),
    (Techs(Techs::SLAP.0 | Techs::POP.0), "slaps and pops"),
    (Techs(Techs::VIBRATO.0), "vibrato"),
];

/// A technique's tally over the run.
#[derive(Clone, Debug, PartialEq)]
pub struct TechMiss {
    pub name: &'static str,
    pub tally: Tally,
}

/// The techniques that lost notes, most missed first: `notes` (scorer group, techniques of one of its notes; a
/// chord's notes each), `state` of each group. Chords count as their own "technique". A group counts once per
/// technique however many of its notes have it.
pub fn tech_misses(notes: &[(usize, Techs, bool)], state: &[Option<Judgement>]) -> Vec<TechMiss> {
    let mut rows: Vec<(&'static str, Techs, Vec<usize>)> =
        TECHS.iter().map(|&(f, n)| (n, f, Vec::new())).collect();
    let mut chords: Vec<usize> = Vec::new();
    for &(g, tech, in_chord) in notes {
        if state.get(g).copied().flatten().is_none() {
            continue;
        }
        for row in rows.iter_mut() {
            if tech.has(row.1) && row.2.last() != Some(&g) {
                row.2.push(g);
            }
        }
        if in_chord && chords.last() != Some(&g) {
            chords.push(g);
        }
    }
    // (a group's notes are usually side by side; any that weren't are counted once here)
    for row in rows.iter_mut() {
        row.2.sort_unstable();
        row.2.dedup();
    }
    chords.sort_unstable();
    chords.dedup();
    let tally = |gs: &[usize]| {
        let mut t = Tally::default();
        for &g in gs {
            if let Some(j) = state.get(g).copied().flatten() {
                t.add(j);
            }
        }
        t
    };
    let mut out: Vec<TechMiss> = rows
        .iter()
        .map(|(n, _, gs)| TechMiss {
            name: n,
            tally: tally(gs),
        })
        .collect();
    out.push(TechMiss {
        name: "chords",
        tally: tally(&chords),
    });
    out.retain(|t| t.tally.missed > 0);
    // most missed first; equal: the larger share missed
    out.sort_by(|a, b| {
        b.tally.missed.cmp(&a.tally.missed).then(
            (b.tally.missed as f64 / b.tally.judged as f64)
                .total_cmp(&(a.tally.missed as f64 / a.tally.judged as f64)),
        )
    });
    out
}

/// The run by section and technique, and the run before it, for the results card.
pub struct RunStats {
    pub acc: f64,
    pub sections: Vec<SectionAcc>,
    pub techs: Vec<TechMiss>,
    /// The accuracy of the run saved before this one (reviews.json), if there's one.
    pub last_acc: Option<f64>,
}

impl PlayerView {
    /// This run in numbers (every frame the card is up: a pass over the notes, cheap next to drawing them).
    pub(super) fn run_stats(&self) -> RunStats {
        let judged: Vec<(f64, Judgement)> = self
            .scorer
            .groups
            .iter()
            .filter_map(|g| g.state.map(|j| (g.t, j)))
            .collect();
        let secs: Vec<(String, f64, f64)> = self
            .chart
            .sections
            .iter()
            .map(|s| (section_label(s), s.start, s.end))
            .collect();
        let states: Vec<Option<Judgement>> = self.scorer.groups.iter().map(|g| g.state).collect();
        let notes: Vec<(usize, Techs, bool)> = self
            .chart
            .glyphs
            .iter()
            .filter_map(|g| g.group.map(|k| (k, g.tech, g.in_chord)))
            .collect();
        // the runs saved for this part, newest first: this one is the first when it was saved (20 notes or more)
        let last_acc = (self.runs_for == self.history_key())
            .then(|| {
                let this_saved = self.runs.first().is_some_and(|r| r.notes.len() == judged.len());
                self.runs.get(usize::from(this_saved)).map(|r| r.acc)
            })
            .flatten()
            .filter(|_| !judged.is_empty());
        RunStats {
            acc: self.scorer.accuracy(),
            sections: by_section(&secs, &judged),
            techs: tech_misses(&notes, &states),
            last_acc,
        }
    }
}

/// The rows of the run's box (points): padding, caption, gap, the section strip, gap, the weakest line, gap, the
/// techniques line, padding.
pub const STATS_H: [f32; 9] = [14.0, 13.0, 8.0, 22.0, 10.0, 17.0, 5.0, 17.0, 12.0];

/// An accuracy's colour: the miss red below 50%, through the late orange to the hit green at 90%.
pub fn acc_color(acc: f64) -> Color32 {
    let th = t();
    let a = acc as f32;
    if a >= 0.9 {
        th.hit
    } else if a >= 0.7 {
        mix(th.late, th.hit, (a - 0.7) / 0.2)
    } else {
        mix(th.miss, th.late, ((a - 0.5) / 0.2).clamp(0.0, 1.0))
    }
}

/// A line of runs of text in their own colours and weights (13-14 pt), stopping before it would pass `max_w`.
pub fn summary_line(p: &Painter, parts: &[(String, Color32, u16)], at: Pos2, max_w: f32) -> f32 {
    let mut x = at.x;
    for (s, col, weight) in parts {
        let room = at.x + max_w - x;
        if room <= px(8.0) {
            break;
        }
        let r = txt(
            p,
            s,
            ipos(x, at.y),
            14.0,
            *col,
            *weight,
            Align2::LEFT_TOP,
            Some(room),
        );
        x = r.max.x;
    }
    x
}

/// The run's box on the results card (P37.6d): "BY SECTION", a strip of the sections as long as they last coloured by
/// their accuracy (the percentage in those wide enough), the weakest two named, then the techniques that lost the
/// most notes.
pub fn draw_run_stats(p: &Painter, ctx: &egui::Context, bx: Rect, stats: &RunStats) {
    let th = t();
    paint::display(p, bx, Some(16.0), 4.0);
    let x0 = snap(bx.min.x + px(18.0));
    let x1 = snap(bx.max.x - px(18.0));
    let mut y = snap(bx.min.y + px(STATS_H[0]));
    caps(
        p,
        "THIS RUN BY SECTION",
        ipos(x0, y),
        11.0,
        Align2::LEFT_TOP,
        None,
    );
    y = snap(y + px(STATS_H[1] + STATS_H[2]));
    // the strip: each section as wide as it lasts (at least a sliver), 2 px apart
    let secs = &stats.sections;
    let gap = px(2.0);
    let total: f64 = secs.iter().map(|s| (s.end - s.start).max(0.1)).sum();
    let room = (x1 - x0) - gap * (secs.len().saturating_sub(1)) as f32;
    let strip_h = px(STATS_H[3]);
    let mut x = x0;
    let ink = Color32::from_rgb(14, 16, 18);
    for (k, sec) in secs.iter().enumerate() {
        let w = if k + 1 == secs.len() {
            x1 - x
        } else {
            (room * ((sec.end - sec.start).max(0.1) / total) as f32).max(px(3.0))
        };
        let r = Rect::from_min_size(pos2(x, y), vec2(w.max(px(1.0)), strip_h));
        let col = acc_color(sec.tally.acc());
        p.rect_filled(r, px(4.0).min(w / 2.0), col.gamma_multiply(0.9));
        let label = format!("{}%", (sec.tally.acc() * 100.0).round() as i64);
        if text::width(ctx, &label, Style::new(11.0, 800, ink)) + px(8.0) <= w {
            txt(p, &label, r.center(), 11.0, ink, 800, Align2::CENTER_CENTER, None);
        }
        x = snap(x + w + gap);
    }
    y = snap(y + strip_h + px(STATS_H[4]));
    // the weakest two (below 95%), or that there's none
    let mut weak: Vec<&SectionAcc> = secs.iter().filter(|s| s.tally.acc() < 0.95).collect();
    weak.sort_by(|a, b| a.tally.acc().total_cmp(&b.tally.acc()));
    let mut parts: Vec<(String, Color32, u16)> = Vec::new();
    if weak.is_empty() {
        parts.push(("Every section at 95% or better".into(), th.hit, 700));
    } else {
        parts.push(("Weakest: ".into(), th.caption, 600));
        for (k, s) in weak.iter().take(2).enumerate() {
            if k > 0 {
                parts.push(("  ·  ".into(), th.caption, 500));
            }
            parts.push((format!("{} ", s.name), th.text_2, 700));
            parts.push((
                format!("{}%", (s.tally.acc() * 100.0).round() as i64),
                acc_color(s.tally.acc()),
                800,
            ));
        }
    }
    summary_line(p, &parts, pos2(x0, y), x1 - x0);
    y = snap(y + px(STATS_H[5] + STATS_H[6]));
    // the techniques that lost the most notes
    let mut parts: Vec<(String, Color32, u16)> = Vec::new();
    if stats.techs.is_empty() {
        parts.push(("No note missed on a technique or a chord".into(), th.text_2, 600));
    } else {
        parts.push(("Most missed: ".into(), th.caption, 600));
        for (k, tm) in stats.techs.iter().take(3).enumerate() {
            if k > 0 {
                parts.push(("  ·  ".into(), th.caption, 500));
            }
            parts.push((format!("{} ", tm.name), th.text_2, 700));
            let of = if k == 0 {
                format!("{} of {} missed", tm.tally.missed, tm.tally.judged)
            } else {
                format!("{} of {}", tm.tally.missed, tm.tally.judged)
            };
            parts.push((of, acc_color(tm.tally.acc()), 800));
        }
    }
    summary_line(p, &parts, pos2(x0, y), x1 - x0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(score: i64, acc: f64, at: f64) -> Run {
        Run {
            score,
            acc: Some(acc),
            speed: 1.0,
            at: Some(at),
        }
    }

    #[test]
    fn repeated_best_runs_fold_into_one_row() {
        let top = [
            run(38900, 0.84, 10.0),
            run(38900, 0.8401, 30.0),
            run(35100, 0.79, 20.0),
        ];
        let c = collapse_runs(&top);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].times, c[0].first, c[0].of.clone()), (2, 0, vec![0, 1]));
        assert_eq!(c[0].run.at, Some(30.0), "the newest repeat's date");
        assert_eq!((c[1].run.score, c[1].times, c[1].first), (35100, 1, 2));
        // the same score at another accuracy is another run
        assert_eq!(collapse_runs(&[run(100, 0.5, 0.0), run(100, 0.6, 0.0)]).len(), 2);
        assert!(collapse_runs(&[]).is_empty());
    }

    #[test]
    fn ordinals() {
        let got: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 102].map(ordinal).to_vec();
        assert_eq!(
            got,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "102nd"
            ]
        );
    }

    #[test]
    fn sections_tally_their_notes() {
        let (h, l, m) = (Judgement::Hit, Judgement::Late, Judgement::Miss);
        let secs = vec![
            ("Intro".to_string(), 0.0, 10.0),
            ("Chorus".to_string(), 10.0, 20.0),
            ("No guitar".to_string(), 20.0, 30.0),
            ("Chorus 2".to_string(), 30.0, 40.0),
        ];
        let judged = [(1.0, h), (2.0, l), (11.0, m), (12.0, h), (31.0, h), (39.999, h)];
        let s = by_section(&secs, &judged);
        let names: Vec<&str> = s.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(
            names,
            ["Intro", "Chorus", "Chorus 2"],
            "an empty section is left out"
        );
        assert!((s[0].tally.acc() - 0.75).abs() < 1e-9);
        assert_eq!((s[1].tally.judged, s[1].tally.missed), (2, 1));
        assert_eq!(s[2].tally.acc(), 1.0);
    }

    #[test]
    fn techniques_rank_by_notes_missed() {
        let (h, m) = (Some(Judgement::Hit), Some(Judgement::Miss));
        // groups: 0 bend hit, 1 bend missed, 2 slide missed, 3 slide missed, 4 a chord (two notes, palm muted)
        // missed, 5 not judged yet
        let notes = [
            (0, Techs::BEND, false),
            (1, Techs::BEND, false),
            (2, Techs::SLIDE, false),
            (3, Techs::UNPITCHED_SLIDE, false),
            (4, Techs::PALM_MUTE, true),
            (4, Techs::PALM_MUTE, true),
            (5, Techs::BEND, false),
        ];
        let state = [h, m, m, m, m, None];
        let t = tech_misses(&notes, &state);
        let got: Vec<(&str, u32, u32)> = t
            .iter()
            .map(|x| (x.name, x.tally.missed, x.tally.judged))
            .collect();
        assert_eq!(
            got,
            [
                ("slides", 2, 2),
                ("palm mutes", 1, 1),
                ("chords", 1, 1),
                ("bends", 1, 2)
            ]
        );
    }
}
