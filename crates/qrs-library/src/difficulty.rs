//! A part's difficulty from its chart (P37.6b, D137; new, no Python source): how many notes a second (on average and
//! in the busiest stretch), how much of it is chords, how high up the neck it goes, its techniques and its tempo,
//! rolled into a level from 1 (a first song) to 10. Worked out once when the library reads a song and kept in the
//! index with `LEVEL_VERSION`, so a change to the sums reads the songs again (shown meanwhile, D137).

use qrs_formats::model::{Arrangement, Techs};
use serde::{Deserialize, Serialize};

/// Bumped whenever the sums below change: songs indexed with an older one are read again (kept on the list meanwhile).
pub const LEVEL_VERSION: u32 = 1;

/// What the level was made of (kept in the index for the library's tooltip).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartLevel {
    /// 1 (easiest) to 10.
    pub level: u8,
    /// Notes and chords a second, over the stretch that's played (first note to last).
    pub nps: f32,
    /// The busiest ten seconds' notes and chords a second.
    pub peak: f32,
    /// Share of the events that are chords (0-1).
    pub chords: f32,
    /// The highest fret played.
    pub max_fret: i32,
    /// Share of the notes with a technique that's hard to start with (bends, slides, hammer-ons and pull-offs,
    /// taps, harmonics, tremolo, slap / pop).
    pub techs: f32,
    /// The techniques played at all, by JSON name (`bend`, `slide`, ...), the hardest ones only.
    #[serde(default)]
    pub tech_names: Vec<String>,
    /// Beats a minute (the median beat).
    pub bpm: f32,
    /// The chart has a difficulty ladder (RS's dynamic difficulty): it can be played easier.
    pub dd: bool,
}

/// The techniques that count (a beginner meets them as something new to learn).
const HARD: &[(Techs, &str)] = &[
    (Techs::BEND, "bend"),
    (Techs::SLIDE, "slide"),
    (Techs::UNPITCHED_SLIDE, "slide"),
    (Techs::HAMMER_ON, "hammer_on"),
    (Techs::PULL_OFF, "pull_off"),
    (Techs::TAP, "tap"),
    (Techs::HARMONIC, "harmonic"),
    (Techs::PINCH_HARMONIC, "pinch_harmonic"),
    (Techs::TREMOLO, "tremolo"),
    (Techs::SLAP, "slap"),
    (Techs::POP, "pop"),
];

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

impl PartLevel {
    /// The level's word: Easy (1-3), Medium (4-6), Hard (7-8), Expert (9-10).
    pub fn word(level: u8) -> &'static str {
        match level {
            0..=3 => "Easy",
            4..=6 => "Medium",
            7..=8 => "Hard",
            _ => "Expert",
        }
    }

    /// One line on what it's made of: "1.8 notes a second, up to fret 7, few chords, bends and slides".
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("{:.1} notes a second", self.nps),
            format!("up to fret {}", self.max_fret.max(0)),
        ];
        parts.push(
            match self.chords {
                c if c < 0.05 => "no chords to speak of",
                c if c < 0.25 => "a few chords",
                c if c < 0.6 => "many chords",
                _ => "mostly chords",
            }
            .into(),
        );
        if !self.tech_names.is_empty() {
            let words: Vec<String> = self
                .tech_names
                .iter()
                .map(|n| match n.as_str() {
                    "hammer_on" => "hammer-ons".to_string(),
                    "pull_off" => "pull-offs".into(),
                    "pinch_harmonic" => "pinch harmonics".into(),
                    "tremolo" => "tremolo picking".into(),
                    n => format!("{n}s"),
                })
                .collect();
            parts.push(words.join(", "));
        }
        if self.bpm > 0.0 {
            parts.push(format!("{:.0} bpm", self.bpm));
        }
        parts.join(", ")
    }
}

/// The level of a part, from its full chart (mastery 1.0).
pub fn part_level(arr: &Arrangement) -> PartLevel {
    // events: each note on its own and each chord once
    let mut times: Vec<f64> = arr
        .notes
        .iter()
        .filter(|n| !n.has(Techs::IGNORE))
        .map(|n| n.time)
        .collect();
    times.extend(arr.chords.iter().map(|c| c.time));
    times.sort_by(f64::total_cmp);
    let events = times.len();
    let mut out = PartLevel {
        dd: arr.has_levels,
        ..Default::default()
    };
    if events == 0 {
        out.level = 1;
        return out;
    }
    let span = (times[events - 1] - times[0]).max(10.0);
    out.nps = (events as f64 / span) as f32;
    // the busiest ten seconds
    let mut j = 0;
    let mut most = 0;
    for i in 0..events {
        while times[i] - times[j] > 10.0 {
            j += 1;
        }
        most = most.max(i - j + 1);
    }
    out.peak = most as f32 / 10.0;
    out.chords = arr.chords.len() as f32 / events as f32;
    let frets = arr
        .notes
        .iter()
        .map(|n| n.fret.max(n.slide_to))
        .chain(arr.chords.iter().flat_map(|c| c.frets.iter().copied()));
    out.max_fret = frets.max().unwrap_or(0).max(0);
    let notes: Vec<Techs> = arr
        .notes
        .iter()
        .map(|n| n.techniques)
        .chain(arr.chords.iter().map(|c| c.techniques))
        .collect();
    let hard = notes
        .iter()
        .filter(|t| HARD.iter().any(|(h, _)| t.has(*h)))
        .count();
    out.techs = hard as f32 / notes.len().max(1) as f32;
    for (h, name) in HARD {
        let n = notes.iter().filter(|t| t.has(*h)).count();
        // a technique counts when it's more than a passing one or two
        if n >= 3 && !out.tech_names.iter().any(|x| x == name) {
            out.tech_names.push((*name).to_string());
        }
    }
    let mut gaps: Vec<f64> = arr
        .beats
        .windows(2)
        .map(|w| w[1].time - w[0].time)
        .filter(|g| *g > 0.1 && *g < 2.0)
        .collect();
    if !gaps.is_empty() {
        gaps.sort_by(f64::total_cmp);
        out.bpm = (60.0 / gaps[gaps.len() / 2]) as f32;
    }
    out.level = level_of(&out);
    out
}

/// The sums (D137): points out of 10 for speed (4), the busiest stretch (1.5), chords (1.5), techniques (1.5), the
/// neck (1) and tempo (0.5); 1 + 9 x points / 10, rounded.
pub fn level_of(l: &PartLevel) -> u8 {
    let speed = clamp01((l.nps - 0.8) / 6.0) * 4.0;
    let peak = clamp01((l.peak - 2.0) / 10.0) * 1.5;
    let chords = clamp01(l.chords / 0.6) * 1.5;
    let techs = clamp01(l.techs / 0.35) * 1.5;
    let neck = clamp01((l.max_fret as f32 - 5.0) / 15.0);
    let tempo = clamp01((l.bpm - 90.0) / 100.0) * 0.5;
    let pts = speed + peak + chords + techs + neck + tempo;
    (1.0 + 9.0 * pts / 10.0).round().clamp(1.0, 10.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_formats::model::{Beat, Chord, Note};

    fn arr(notes: Vec<Note>, chords: Vec<Chord>, bpm: f64, len: f64) -> Arrangement {
        let step = 60.0 / bpm;
        Arrangement {
            beats: (0..((len / step) as usize))
                .map(|i| Beat {
                    time: i as f64 * step,
                    measure: -1,
                })
                .collect(),
            notes,
            chords,
            song_length: len,
            ..Default::default()
        }
    }

    fn chord(t: f64, frets: Vec<i32>) -> Chord {
        Chord {
            time: t,
            chord_id: 0,
            name: String::new(),
            notes: Vec::new(),
            techniques: Techs::NONE,
            sustain: 0.0,
            frets,
            fingers: Vec::new(),
            high_density: false,
        }
    }

    #[test]
    fn a_slow_open_string_riff_is_easy() {
        // one note a second on open strings and the first frets, at 80 bpm, for two minutes
        let notes = (0..120).map(|i| Note::new(i as f64, i % 3, i % 4)).collect();
        let l = part_level(&arr(notes, Vec::new(), 80.0, 120.0));
        assert!(l.level <= 2, "{l:?}");
        assert_eq!(PartLevel::word(l.level), "Easy");
        assert_eq!(l.max_fret, 3);
        assert!((l.nps - 1.0).abs() < 0.05, "{}", l.nps);
        assert!(l.summary().starts_with("1.0 notes a second, up to fret 3"));
    }

    #[test]
    fn a_fast_shred_up_the_neck_with_bends_is_expert() {
        // twelve notes a second up to fret 22, every fourth a bend, every third a hammer-on, at 180 bpm
        let notes = (0..1440)
            .map(|i| {
                let mut n = Note::new(i as f64 / 12.0, i % 6, 12 + i % 11);
                if i % 4 == 0 {
                    n.techniques.insert(Techs::BEND);
                }
                if i % 3 == 0 {
                    n.techniques.insert(Techs::HAMMER_ON);
                }
                n
            })
            .collect();
        let l = part_level(&arr(notes, Vec::new(), 180.0, 120.0));
        assert!(l.level >= 9, "{l:?}");
        assert_eq!(l.max_fret, 22);
        assert!(l.tech_names.contains(&"bend".to_string()));
        assert!(l.tech_names.contains(&"hammer_on".to_string()));
        assert!((l.bpm - 180.0).abs() < 1.0);
    }

    #[test]
    fn chords_and_speed_each_raise_the_level() {
        let slow: Vec<Note> = (0..60).map(|i| Note::new(i as f64 * 2.0, 1, 2)).collect();
        let base = part_level(&arr(slow.clone(), Vec::new(), 100.0, 120.0)).level;
        let chords: Vec<Chord> = (0..60)
            .map(|i| chord(i as f64 * 2.0 + 1.0, vec![3, 5, 5, 4, 3, 3]))
            .collect();
        let with_chords = part_level(&arr(slow, chords, 100.0, 120.0)).level;
        assert!(with_chords > base, "{with_chords} vs {base}");
        let fast: Vec<Note> = (0..480).map(|i| Note::new(i as f64 / 4.0, 1, 2)).collect();
        assert!(part_level(&arr(fast, Vec::new(), 100.0, 120.0)).level > base);
    }

    #[test]
    fn an_empty_chart_is_level_one_and_dd_is_kept() {
        let mut a = arr(Vec::new(), Vec::new(), 120.0, 60.0);
        a.has_levels = true;
        let l = part_level(&a);
        assert_eq!(l.level, 1);
        assert!(l.dd);
    }

    #[test]
    fn levels_stay_between_one_and_ten() {
        for nps in [0.0, 0.5, 3.0, 9.0, 40.0] {
            let l = PartLevel {
                nps,
                peak: nps * 2.0,
                chords: 1.0,
                techs: 1.0,
                max_fret: 24,
                bpm: 300.0,
                ..Default::default()
            };
            assert!((1..=10).contains(&level_of(&l)));
        }
        assert_eq!(level_of(&PartLevel::default()), 1);
    }
}
