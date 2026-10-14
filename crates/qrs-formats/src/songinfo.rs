//! Facts for the Song info tile, from the chart itself: tempo, time signature, how many notes, chords and
//! techniques this part has, the highest fret, the most-used chord, the busiest passage, the song's structure.
//! Port of rsplay/songinfo.py.

use std::collections::HashSet;

use crate::model::{Arrangement, Techs};

const TECH_NAMES: [(Techs, &str); 9] = [
    (Techs::BEND, "bends"),
    (Techs::SLIDE, "slides"),
    (Techs::HAMMER_ON, "hammer-ons"),
    (Techs::PULL_OFF, "pull-offs"),
    (Techs::PALM_MUTE, "palm-muted notes"),
    (Techs::VIBRATO, "vibratos"),
    (Techs::HARMONIC, "harmonics"),
    (Techs::TAP, "tapped notes"),
    (Techs::FRET_HAND_MUTE, "muted strums"),
];

/// `f"{n:,}"`
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: usize, word: &str) -> String {
    format!(
        "{} {}",
        thousands(n),
        if n != 1 { word } else { word.trim_end_matches('s') }
    )
}

/// A Counter that keeps insertion order for ties, like Python's `Counter.most_common`.
#[derive(Default)]
struct Counter {
    items: Vec<(String, usize)>,
}

impl Counter {
    fn add(&mut self, k: &str) {
        match self.items.iter_mut().find(|(x, _)| x == k) {
            Some((_, n)) => *n += 1,
            None => self.items.push((k.to_string(), 1)),
        }
    }
    fn most_common(&self, n: usize) -> Vec<(String, usize)> {
        let mut v = self.items.clone();
        v.sort_by_key(|a| std::cmp::Reverse(a.1)); // stable: ties keep insertion order
        v.truncate(n);
        v
    }
    fn len(&self) -> usize {
        self.items.len()
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn mmss(t: f64) -> String {
    let s = t.max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Short, readable facts about this part, computed from the chart.
pub fn chart_facts(arr: &Arrangement, year: i32, album: &str) -> Vec<String> {
    let mut facts = Vec::new();
    let beats: Vec<f64> = arr.beats.iter().map(|b| b.time).collect();
    if beats.len() >= 8 {
        let gaps: Vec<f64> = beats
            .windows(2)
            .filter(|w| w[1] > w[0])
            .map(|w| w[1] - w[0])
            .collect();
        if !gaps.is_empty() {
            let bpm = 60.0 / median(gaps);
            facts.push(format!("Tempo: about {} BPM", bpm.round_ties_even() as i64));
        }
        let downs: Vec<usize> = arr
            .beats
            .iter()
            .enumerate()
            .filter(|(_, b)| b.is_downbeat())
            .map(|(k, _)| k)
            .collect();
        let bars: Vec<usize> = downs.windows(2).map(|w| w[1] - w[0]).collect();
        if !bars.is_empty() {
            let mut c = Counter::default();
            for b in &bars {
                c.add(&b.to_string());
            }
            let sig: usize = c.most_common(1)[0].0.parse().unwrap();
            let odd = bars.iter().filter(|&&b| b != sig).count();
            let extra = if odd > 0 {
                format!(", with {}", plural(odd, "odd bars"))
            } else {
                String::new()
            };
            facts.push(format!("Time signature: mostly {sig}/4{extra}"));
        }
    }
    let notes: Vec<_> = arr.notes.iter().filter(|n| !n.has(Techs::IGNORE)).collect();
    let chords = &arr.chords;
    if !notes.is_empty() || !chords.is_empty() {
        facts.push(format!(
            "This part: {} and {}",
            plural(notes.len(), "single notes"),
            plural(chords.len(), "chords")
        ));
    }
    let all_frets: Vec<i32> = notes
        .iter()
        .map(|n| n.fret)
        .chain(chords.iter().flat_map(|c| c.notes.iter().map(|n| n.fret)))
        .collect();
    let mut counts = Counter::default();
    let events: Vec<Techs> = notes
        .iter()
        .map(|n| n.techniques)
        .chain(
            chords
                .iter()
                .map(|c| c.notes.iter().fold(c.techniques, |acc, n| acc | n.techniques)),
        )
        .collect();
    for techs in events {
        // a chord counts once, not once per string
        for (tech, name) in TECH_NAMES {
            if techs.has(tech) {
                counts.add(name);
            }
        }
    }
    let top: Vec<String> = counts
        .most_common(3)
        .iter()
        .map(|(name, c)| plural(*c, name))
        .collect();
    if !top.is_empty() {
        facts.push(format!("Techniques: {}", top.join(", ")));
    }
    let frets: Vec<i32> = all_frets.into_iter().filter(|&f| f > 0).collect();
    if let Some(&hi) = frets.iter().max() {
        let up = if hi > 12 {
            " (above the 12th: up the neck)"
        } else {
            ""
        };
        facts.push(format!("Highest fret: {hi}{up}"));
    }
    let mut named = Counter::default();
    for c in chords.iter().filter(|c| !c.name.is_empty()) {
        named.add(&c.name);
    }
    if named.len() > 0 {
        let (name, k) = named.most_common(1)[0].clone();
        facts.push(format!(
            "Most-played chord: {name} ({}); {}",
            plural(k, "times"),
            plural(named.len(), "different chords")
        ));
    }
    let shapes: HashSet<&Vec<i32>> = arr.handshapes.iter().map(|h| &h.frets).collect();
    if shapes.len() > 1 {
        facts.push(format!(
            "{} for your fretting hand to learn",
            plural(shapes.len(), "chord shapes")
        ));
    }
    let mut times: Vec<f64> = notes
        .iter()
        .map(|n| n.time)
        .chain(chords.iter().map(|c| c.time))
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    if times.len() > 20 {
        let (mut best, mut j) = (0usize, 0usize);
        for (i, &t) in times.iter().enumerate() {
            while times[j] < t - 2.0 {
                j += 1;
            }
            best = best.max(i - j + 1);
        }
        let at = times
            .iter()
            .enumerate()
            .find(|&(i, &t)| i + 1 >= best && t - times[i + 1 - best] <= 2.0)
            .map(|(_, &t)| t)
            .unwrap_or(0.0);
        facts.push(format!(
            "Busiest passage: {:.1} notes per second, around {}",
            best as f64 / 2.0,
            mmss(at)
        ));
    }
    if !arr.sections.is_empty() {
        let mut kinds = Counter::default();
        for s in &arr.sections {
            kinds.add(
                &s.name
                    .trim_end_matches(|c: char| c.is_ascii_digit())
                    .to_lowercase(),
            );
        }
        let nice = |k: &str| -> Option<&str> {
            Some(match k {
                "verse" => "verses",
                "chorus" => "choruses",
                "solo" => "solos",
                "bridge" => "bridges",
                "riff" => "riffs",
                "intro" => "intros",
                "outro" => "outros",
                "prechorus" => "pre-choruses",
                "breakdown" => "breakdowns",
                _ => return None,
            })
        };
        let parts: Vec<String> = kinds
            .items
            .iter()
            .filter_map(|(k, n)| nice(k).map(|pl| format!("{n} {}", if *n > 1 { pl } else { k })))
            .collect();
        if !parts.is_empty() {
            facts.push(format!("Structure: {}", parts.join(", ")));
        }
    }
    if arr.song_length > 0.0 {
        let from = if !album.is_empty() && year != 0 {
            format!(", from {album} ({year})")
        } else {
            String::new()
        };
        facts.push(format!("Length: {}{from}", mmss(arr.song_length)));
    }
    let capo = if arr.capo != 0 {
        format!(", capo on fret {}", arr.capo)
    } else {
        String::new()
    };
    facts.push(format!("Tuning: {}{capo}", arr.tuning_name()));
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Beat, Chord, Note, Section};

    fn arr() -> Arrangement {
        let mut a = Arrangement::new("Lead");
        a.title = "Test".into();
        a.song_length = 125.0;
        a.beats = (0..64)
            .map(|i| Beat {
                time: i as f64 * 0.5,
                measure: if i % 4 == 0 { i / 4 } else { -1 },
            })
            .collect();
        a.notes = (0..30)
            .map(|i| Note::new(1.0 + i as f64 * 0.25, 1, 5 + i % 3))
            .collect();
        a.notes[3] = Note {
            techniques: Techs::BEND,
            ..Note::new(1.75, 1, 14)
        };
        a.chords = (0..4)
            .map(|i| Chord {
                time: 20.0 + i as f64,
                chord_id: 0,
                name: "Am".into(),
                notes: [1, 2, 3]
                    .iter()
                    .map(|&s| Note::new(20.0 + i as f64, s, 2))
                    .collect(),
                techniques: Techs::NONE,
                sustain: 0.0,
                frets: vec![],
                fingers: vec![],
            })
            .collect();
        a.sections = vec![
            Section {
                name: "verse".into(),
                number: 1,
                start: 0.0,
                end: 10.0,
            },
            Section {
                name: "chorus".into(),
                number: 1,
                start: 10.0,
                end: 20.0,
            },
            Section {
                name: "verse".into(),
                number: 2,
                start: 20.0,
                end: 30.0,
            },
        ];
        a
    }

    #[test]
    fn facts_cover_tempo_signature_counts_and_structure() {
        let text = chart_facts(&arr(), 1999, "Album").join("\n");
        for want in [
            "about 120 BPM",
            "mostly 4/4",
            "30 single notes and 4 chords",
            "Highest fret: 14 (above the 12th",
            "Most-played chord: Am (4 times)",
            "1 bend",
            "Structure: 2 verses, 1 chorus",
            "Length: 2:05, from Album (1999)",
        ] {
            assert!(text.contains(want), "{want} in\n{text}");
        }
    }

    #[test]
    fn empty_chart_still_works() {
        let facts = chart_facts(&Arrangement::new("Lead"), 0, "");
        assert!(facts.last().unwrap().starts_with("Tuning: E Standard"));
    }

    #[test]
    fn chord_techniques_count_once_per_strum() {
        let mut a = Arrangement::new("Rhythm");
        a.chords = (0..3)
            .map(|i| Chord {
                time: i as f64,
                chord_id: 0,
                name: String::new(),
                notes: (0..6)
                    .map(|s| Note {
                        techniques: Techs::FRET_HAND_MUTE,
                        ..Note::new(i as f64, s, 0)
                    })
                    .collect(),
                techniques: Techs::NONE,
                sustain: 0.0,
                frets: vec![],
                fingers: vec![],
            })
            .collect();
        assert!(
            chart_facts(&a, 0, "")
                .iter()
                .any(|f| f.contains("3 muted strums"))
        );
    }

    #[test]
    fn thousands_separator() {
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(999), "999");
    }
}
