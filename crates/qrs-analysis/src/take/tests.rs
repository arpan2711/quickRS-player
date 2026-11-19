//! Take analysis on synthetic takes: a small plucked-string renderer plays a chart (or a changed copy of it, the
//! "student"), and the analysis has to find each change.

use super::*;
use qrs_formats::model::{Beat, Chord};

const RATE: u32 = 48000;

/// One played note: chart-time `t`, string, fret, and how it's played.
#[derive(Clone, Debug)]
struct Played {
    t: f64,
    string: i32,
    fret: i32,
    bend: f64,
    /// Plucked (false: a hammer-on, no attack).
    pick: bool,
    amp: f32,
}

fn p(t: f64, string: i32, fret: i32) -> Played {
    Played {
        t,
        string,
        fret,
        bend: 0.0,
        pick: true,
        amp: 0.25,
    }
}

/// Render played notes into a take that starts at chart time `t0`, at `speed`, `late` seconds late overall.
fn render(notes: &[Played], t0: f64, len: f64, speed: f64, late: f64) -> Vec<f32> {
    let opens = [40, 45, 50, 55, 59, 64];
    let n = (len / speed * RATE as f64) as usize + RATE as usize;
    let mut x = vec![0.0f32; n];
    let mut order: Vec<&Played> = notes.iter().collect();
    order.sort_by(|a, b| a.t.total_cmp(&b.t));
    for (k, nt) in order.iter().enumerate() {
        // a note rings until the next note on its string
        let end = order[k + 1..]
            .iter()
            .find(|o| o.string == nt.string && o.t > nt.t + 1e-6)
            .map_or(nt.t + 2.0, |o| o.t);
        let start = ((nt.t - t0) / speed + late) * RATE as f64;
        let stop = ((end - t0) / speed + late) * RATE as f64;
        if start < 0.0 {
            continue;
        }
        let (s0, s1) = (start as usize, (stop as usize).min(n));
        let f0 = midi_to_hz((opens[nt.string as usize] + nt.fret) as f64);
        let mut phase = [0.0f64; 8];
        let mut last = 0.0f64;
        for i in s0..s1 {
            let t = (i - s0) as f64 / RATE as f64;
            // a bend rises over 120 ms from 50 ms in
            let up = if nt.bend > 0.0 {
                nt.bend * ((t - 0.05) / 0.12).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let f = f0 * 2f64.powf(up / 12.0);
            let mut v = 0.0;
            for (h, ph) in phase.iter_mut().enumerate() {
                let hh = (h + 1) as f64;
                *ph += std::f64::consts::TAU * f * hh / RATE as f64;
                v += ph.sin() / hh * (-t * (2.0 + 1.5 * hh)).exp();
            }
            let attack = if nt.pick {
                (t / 0.001).min(1.0)
            } else {
                (t / 0.004).min(1.0) * 0.7
            };
            let fade = ((s1 - i) as f64 / (0.004 * RATE as f64)).min(1.0);
            last = v * attack * fade;
            x[i] += nt.amp * last as f32;
        }
        let _ = last;
    }
    x
}

fn grid(a: &mut Arrangement, secs: f64) {
    a.beats = (0..(secs * 2.0) as usize + 1)
        .map(|i| Beat {
            time: i as f64 * 0.5,
            measure: if i % 4 == 0 { (i / 4 + 1) as i32 } else { -1 },
        })
        .collect();
}

/// The chart from played notes (single notes, or chords where several share a time).
fn chart(notes: &[Played], secs: f64) -> Arrangement {
    let mut a = Arrangement::new("Lead");
    a.title = "Test".into();
    a.artist = "Synth".into();
    grid(&mut a, secs);
    let mut by_t: BTreeMap<i64, Vec<&Played>> = BTreeMap::new();
    for n in notes {
        by_t.entry((n.t * 1e6) as i64).or_default().push(n);
    }
    for v in by_t.values() {
        let mk = |n: &Played| Note {
            bend: n.bend,
            techniques: if !n.pick {
                Techs::HAMMER_ON
            } else if n.bend > 0.0 {
                Techs::BEND
            } else {
                Techs::NONE
            },
            sustain: if n.bend > 0.0 { 0.5 } else { 0.0 },
            ..Note::new(n.t, n.string, n.fret)
        };
        if v.len() == 1 {
            a.notes.push(mk(v[0]));
        } else {
            a.chords.push(Chord {
                time: v[0].t,
                chord_id: 0,
                name: String::new(),
                notes: v.iter().map(|n| mk(n)).collect(),
                techniques: Techs::NONE,
                sustain: 0.0,
                frets: vec![],
                fingers: vec![],
                high_density: false,
            });
        }
    }
    a
}

/// A strummed chord: low string first, 8 ms apart.
fn strum(t: f64, shape: &[(i32, i32)]) -> Vec<Played> {
    shape
        .iter()
        .enumerate()
        .map(|(k, &(s, f))| Played {
            t: t + 0.008 * k as f64,
            ..p(t, s, f)
        })
        .collect()
}

fn run(chart: &Arrangement, take: Vec<f32>, from: f64, to: f64, speed: f64) -> TakeReport {
    let input = TakeInput {
        arr: chart.clone(),
        from,
        to,
        samples: take,
        timing: TakeTiming::linear(RATE, from, speed),
    };
    analyse(&input, &|_| {}, &AtomicBool::new(false)).unwrap()
}

/// A lick: eighth notes at 120 bpm over two bars on the top three strings.
fn lick() -> Vec<Played> {
    let frets = [
        (3, 5),
        (3, 7),
        (4, 5),
        (4, 8),
        (5, 5),
        (4, 8),
        (4, 5),
        (3, 7),
        (3, 5),
        (2, 7),
        (3, 5),
        (3, 7),
        (4, 5),
        (4, 6),
        (4, 5),
        (3, 7),
    ];
    frets
        .iter()
        .enumerate()
        .map(|(k, &(s, f))| p(0.5 + 0.25 * k as f64, s, f))
        .collect()
}

fn statuses(r: &TakeReport) -> Vec<Status> {
    r.events.iter().map(|e| e.status).collect()
}

#[test]
fn timing_maps_samples_to_chart_time_and_back() {
    let t = TakeTiming::linear(48000, 10.0, 0.5);
    assert!((t.chart_time(48000.0) - 10.5).abs() < 1e-9);
    assert!((t.sample_at(10.5) - 48000.0).abs() < 1e-6);
    // anchors: a pause (the song stood still for half a second) in the middle
    let t = TakeTiming {
        rate: 1000,
        speed: 1.0,
        anchors: vec![(0, 5.0), (1000, 6.0), (1500, 6.0), (2500, 7.0)],
    };
    assert!((t.chart_time(500.0) - 5.5).abs() < 1e-9);
    assert!((t.chart_time(1200.0) - 6.0).abs() < 1e-9);
    assert!((t.chart_time(3500.0) - 8.0).abs() < 1e-9);
    assert!((t.chart_time(-1000.0) - 4.0).abs() < 1e-9);
    assert!((t.sample_at(6.5) - 2000.0).abs() < 1e-9);
    assert!((t.sample_at(6.0) - 1000.0).abs() < 1e-9);
    assert!((t.sample_at(4.0) + 1000.0).abs() < 1e-9);
    let back = TakeTiming::from_json(&t.to_json()).unwrap();
    assert_eq!(back, t);
}

#[test]
fn string_and_fret_for_a_pitch() {
    let a = Arrangement::new("Lead");
    let neck = Neck::new(&a);
    assert_eq!(neck.labels, ["low E", "A", "D", "G", "B", "high E"]);
    // C4 (60) on the B string: fret 1
    assert_eq!(
        neck.place(60, Some(4), 2.0).unwrap(),
        Pos {
            string: 4,
            fret: 1,
            midi: 60
        }
    );
    // not on the preferred string (too low): nearest the hand at fret 5 -> G string fret 5
    assert_eq!(
        neck.place(60, Some(5), 5.0).unwrap(),
        Pos {
            string: 3,
            fret: 5,
            midi: 60
        }
    );
    // hand at fret 8: A string 15? D string 10? the D string's 10th is nearest
    assert_eq!(neck.place(60, None, 8.0).unwrap().string, 2);
    // open strings are near any low hand
    assert_eq!(
        neck.place(64, None, 1.0).unwrap(),
        Pos {
            string: 5,
            fret: 0,
            midi: 64
        }
    );
    assert_eq!(
        neck.describe(&Pos {
            string: 4,
            fret: 2,
            midi: 61
        }),
        "C# (B string, fret 2)"
    );
    assert_eq!(
        neck.describe(&Pos {
            string: 5,
            fret: 0,
            midi: 64
        }),
        "E (high E string, open)"
    );
    assert!(neck.place(30, None, 5.0).is_none(), "below the low E");
}

#[test]
fn a_clean_take_is_all_right_and_on_time() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let r = run(&a, render(&notes, 0.0, 5.0, 1.0, 0.0), 0.0, 5.0, 1.0);
    assert_eq!(r.events.len(), 16);
    assert_eq!(r.count(Status::Hit), 16, "{}", r.text(100));
    assert!(r.extras.is_empty(), "{:?}", r.extras);
    assert!(r.timing.median_ms.abs() < 5.0, "{:?}", r.timing);
    assert_eq!(r.timing.tendency, Tendency::OnTime);
    assert!(r.passed(0.9));
    assert_eq!(r.problems, ["Clean take: every note right and in time."]);
    assert_eq!(r.bars.len(), 3);
    assert_eq!(r.bars[0].hits, 6);
}

#[test]
fn a_wrong_fret_is_reported_with_what_was_played() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let mut played = notes.clone();
    played[4].fret = 4; // high E fret 4 (G#4) instead of fret 5 (A4)
    played[9].fret = 8; // G string fret 8 instead of 7
    let r = run(&a, render(&played, 0.0, 5.0, 1.0, 0.0), 0.0, 5.0, 1.0);
    assert_eq!(r.count(Status::Hit), 14, "{}", r.text(100));
    let w: Vec<&EventResult> = r.events.iter().filter(|e| e.status == Status::Wrong).collect();
    assert_eq!(w.len(), 2);
    assert_eq!(
        w[0].played,
        vec![Pos {
            string: 5,
            fret: 4,
            midi: 68
        }]
    );
    assert_eq!(
        w[1].played,
        vec![Pos {
            string: 2,
            fret: 8,
            midi: 58
        }]
    );
    assert!((r.accuracy() - 14.0 / 16.0).abs() < 1e-9);
    let text = r.problems.join("\n");
    assert!(
        text.contains("you played Ab (high E string, fret 4) where the chart has A (high E string, fret 5)"),
        "{text}"
    );
    assert!(text.starts_with("Bar 1 (1.5 s)"), "{text}");
}

#[test]
fn dropped_and_added_notes() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let mut played = notes.clone();
    played.remove(6); // drop the note at 2.0
    played.push(Played {
        amp: 0.25,
        ..p(3.375, 2, 9)
    }); // an extra B3 between two notes
    let r = run(&a, render(&played, 0.0, 5.0, 1.0, 0.0), 0.0, 5.0, 1.0);
    assert_eq!(r.events[6].status, Status::Missed, "{}", r.text(100));
    assert_eq!(r.count(Status::Hit), 15, "{}", r.text(100));
    assert_eq!(r.extras.len(), 1, "{:?}", r.extras);
    assert_eq!(r.extras[0].played[0].midi, 59);
    assert!((r.extras[0].t - 3.375).abs() < 0.01);
    let text = r.problems.join("\n");
    assert!(text.contains("you missed"), "{text}");
    assert!(text.contains("1 extra note"), "{text}");
}

#[test]
fn timing_offsets_and_a_late_take() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    // some notes 40 and 80 ms late, one 40 ms early
    let mut played = notes.clone();
    played[3].t += 0.04;
    played[8].t += 0.08;
    played[12].t -= 0.04;
    let r = run(&a, render(&played, 0.0, 5.0, 1.0, 0.0), 0.0, 5.0, 1.0);
    assert_eq!(r.count(Status::Hit), 16, "{}", r.text(100));
    let off = |i: usize| r.events[i].offset_ms.unwrap();
    assert!((off(3) - 40.0).abs() < 5.0, "{}", off(3));
    assert!((off(8) - 80.0).abs() < 5.0, "{}", off(8));
    assert!((off(12) + 40.0).abs() < 5.0, "{}", off(12));
    assert!(off(0).abs() < 5.0);
    // the whole take 30 ms late
    let r = run(&a, render(&notes, 0.0, 5.0, 1.0, 0.03), 0.0, 5.0, 1.0);
    assert_eq!(r.count(Status::Hit), 16);
    assert!((r.timing.median_ms - 30.0).abs() < 5.0, "{:?}", r.timing);
    assert_eq!(r.timing.tendency, Tendency::Dragging);
    assert!(
        r.problems[0].contains("ms late (dragging) overall"),
        "{:?}",
        r.problems
    );
    // 120 ms late: still lined up (the windows follow the take)
    let r = run(&a, render(&notes, 0.0, 5.0, 1.0, 0.12), 0.0, 5.0, 1.0);
    assert_eq!(r.count(Status::Hit), 16, "{}", r.text(100));
    assert!((r.shift_ms - 120.0).abs() < 8.0, "{}", r.shift_ms);
}

#[test]
fn half_speed_takes_are_mapped_and_offsets_are_wall_ms() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let mut played = notes.clone();
    played[5].t += 0.02; // 20 ms of chart time = 40 ms at half speed
    let r = run(&a, render(&played, 0.0, 5.0, 0.5, 0.0), 0.0, 5.0, 0.5);
    assert_eq!(r.count(Status::Hit), 16, "{}", r.text(100));
    assert!((r.events[5].offset_ms.unwrap() - 40.0).abs() < 5.0);
}

#[test]
fn double_stops_and_chords() {
    let mut notes = Vec::new();
    // double stops (Chuck Berry style) on G+B, then power chords and an open G chord
    for k in 0..4 {
        notes.extend(strum(0.5 + 0.5 * k as f64, &[(3, 8), (4, 8)]));
    }
    notes.extend(strum(2.5, &[(0, 3), (1, 5), (2, 5)]));
    notes.extend(strum(3.0, &[(0, 5), (1, 7), (2, 7)]));
    notes.extend(strum(3.5, &[(0, 3), (1, 2), (2, 0), (3, 0), (4, 0), (5, 3)]));
    // the chart has the notes at the strum's start
    let chart_notes: Vec<Played> = notes
        .iter()
        .map(|n| Played {
            t: ((n.t - 0.5) / 0.5).floor() * 0.5 + 0.5,
            ..n.clone()
        })
        .collect();
    let a = chart(&chart_notes, 5.0);
    let r = run(&a, render(&notes, 0.0, 4.5, 1.0, 0.0), 0.0, 4.5, 1.0);
    assert_eq!(r.events.len(), 7);
    assert_eq!(r.events[0].kind, EventKind::DoubleStop);
    assert_eq!(r.events[4].kind, EventKind::Chord);
    assert_eq!(r.count(Status::Hit), 7, "{}", r.text(100));
    assert!(r.extras.is_empty(), "{:?}", r.extras);

    // the student: a double stop with the B string a fret low, a power chord missing its top note, double stops
    // 40 ms late
    let mut played: Vec<Played> = Vec::new();
    for (k, n) in notes.iter().enumerate() {
        let mut n = n.clone();
        if k == 3 {
            n.fret = 7; // the second double stop's B string: fret 7
        }
        if n.t < 2.4 {
            n.t += 0.04;
        }
        if (n.t - 3.008).abs() < 1e-6 {
            continue; // the second power chord's fifth (A string); an octave can't be told from the root's harmonic
        }
        played.push(n);
    }
    let r = run(&a, render(&played, 0.0, 4.5, 1.0, 0.0), 0.0, 4.5, 1.0);
    let s = statuses(&r);
    assert_eq!(s[1], Status::Wrong, "{}", r.text(100));
    assert!(
        r.events[1].played.contains(&Pos {
            string: 4,
            fret: 7,
            midi: 66
        }),
        "{:?}",
        r.events[1]
    );
    assert_eq!(s[5], Status::Wrong, "{}", r.text(100));
    assert_eq!(
        r.events[5].missing,
        vec![Pos {
            string: 1,
            fret: 7,
            midi: 52
        }]
    );
    for i in [0, 2, 3] {
        assert_eq!(s[i], Status::Hit, "{i}: {}", r.text(100));
        assert!((r.events[i].offset_ms.unwrap() - 40.0).abs() < 6.0);
    }
    let text = r.problems.join("\n");
    assert!(
        text.contains("the double stop had F# (B string, fret 7) instead of G (B string, fret 8)"),
        "{text}"
    );
    assert!(text.contains("was missing E (A string, fret 7)"), "{text}");
    assert!(
        text.contains("ms late on the double stops"),
        "{text} {:?}",
        r.events.iter().map(|e| e.offset_ms).collect::<Vec<_>>()
    );
}

#[test]
fn noise_and_level_changes_dont_change_the_verdict() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let mut x = render(&notes, 0.0, 5.0, 1.0, 0.0);
    let mut seed: u32 = 9;
    let n = x.len();
    for (i, v) in x.iter_mut().enumerate() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((seed >> 8) as f32 / 16777216.0 - 0.5) * 0.006; // hiss about 45 dB under the notes
        let hum = 0.003 * (std::f32::consts::TAU * 50.0 * i as f32 / RATE as f32).sin();
        // the second half 20 dB quieter (the volume knob turned down)
        let gain = if i > n / 2 { 0.1 } else { 1.0 };
        *v = *v * gain + noise + hum;
    }
    let r = run(&a, x, 0.0, 5.0, 1.0);
    assert!(r.count(Status::Hit) >= 15, "{}", r.text(100));
    assert!(r.extras.len() <= 1, "{:?}", r.extras);
}

#[test]
fn a_hammer_on_is_heard_without_a_pluck_and_bends_are_followed() {
    let mut notes = vec![
        p(0.5, 3, 5),
        Played {
            pick: false,
            ..p(0.75, 3, 7)
        },
        p(1.5, 4, 8),
    ];
    notes.push(Played {
        bend: 2.0,
        ..p(2.0, 3, 7)
    });
    let a = chart(&notes, 4.0);
    let r = run(&a, render(&notes, 0.0, 3.5, 1.0, 0.0), 0.0, 3.5, 1.0);
    assert_eq!(statuses(&r), [Status::Hit; 4], "{}", r.text(100));
    assert!(r.events[3].note.is_none(), "{:?}", r.events[3].note);
    // the bend only reaches a semitone
    let mut played = notes.clone();
    played[3].bend = 1.0;
    let r = run(&a, render(&played, 0.0, 3.5, 1.0, 0.0), 0.0, 3.5, 1.0);
    assert_eq!(r.events[3].status, Status::Wrong, "{}", r.text(100));
    assert!(
        r.events[3]
            .note
            .as_ref()
            .unwrap()
            .starts_with("bend reached +1.0 of +2"),
        "{:?}",
        r.events[3]
    );
    assert!(
        r.problems.iter().any(|p| p.contains("the bend fell short")),
        "{:?}",
        r.problems
    );
    // the hammer-on not played: not called wrong
    let mut played = notes.clone();
    played.remove(1);
    let r = run(&a, render(&played, 0.0, 3.5, 1.0, 0.0), 0.0, 3.5, 1.0);
    assert_ne!(r.events[1].status, Status::Hit, "{}", r.text(100));
    assert_ne!(r.events[1].status, Status::Wrong);
}

#[test]
fn a_silent_take_misses_everything() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let r = run(&a, vec![0.0; RATE as usize * 6], 0.0, 5.0, 1.0);
    assert_eq!(r.count(Status::Missed), 16);
    assert_eq!(r.accuracy(), 0.0);
    assert!(!r.passed(0.5));
    assert!(
        r.problems.iter().any(|p| p.contains("missed")),
        "{:?}",
        r.problems
    );
}

#[test]
fn the_report_as_text_json_and_tabs() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let mut played = notes.clone();
    played[4].fret = 4;
    let r = run(&a, render(&played, 0.0, 5.0, 1.0, 0.0), 0.0, 5.0, 1.0);
    let tabs = r.ascii_tabs(200);
    let lines: Vec<&str> = tabs.lines().collect();
    assert_eq!(lines[0], "chart");
    assert!(lines[1].starts_with(" e|"), "{tabs}");
    assert!(lines[8].starts_with(" e|"), "{tabs}");
    assert!(lines[1].contains("-5-"), "{tabs}");
    assert!(
        lines[8].contains("-4-"),
        "the played fret 4 on the high E: {tabs}"
    );
    assert!(lines[14].contains('X'), "{tabs}");
    let j = r.to_json();
    assert_eq!(j["summary"]["wrong"], 1);
    assert_eq!(j["events"][4]["status"], "wrong");
    assert_eq!(j["events"][4]["played"][0]["fret"], 4);
    assert_eq!(j["events"][4]["played"][0]["string_name"], "high E");
    assert_eq!(j["events"][4]["expected"][0]["note"], "A4");
    assert!(j["about"].as_str().unwrap().contains("accuracy"));
    assert!(r.text(100).contains("accuracy 94%"));
}

#[test]
fn the_background_job_reports_progress_and_a_result() {
    let notes = lick();
    let a = chart(&notes, 6.0);
    let mut job = TakeJob::start(TakeInput {
        arr: a,
        from: 0.0,
        to: 5.0,
        samples: render(&notes, 0.0, 5.0, 1.0, 0.0),
        timing: TakeTiming::linear(RATE, 0.0, 1.0),
    });
    let t = Instant::now();
    while !job.finished() && t.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!((job.progress() - 1.0).abs() < 1e-6);
    let r = job.take().unwrap();
    assert_eq!(r.count(Status::Hit), 16);
    assert!(job.take().is_none(), "taken once");
}

#[test]
fn multi_pitch_finds_a_chords_notes() {
    let mut x = vec![0.0f32; RATE as usize];
    let notes = strum(0.0, &[(0, 3), (1, 5), (2, 5)]); // G2 D3 G3
    x = {
        let r = render(&notes, 0.0, 0.5, 1.0, 0.0);
        x[..].copy_from_slice(&r[..RATE as usize]);
        x
    };
    let mut sp = Spectra::new();
    let w = &x[(0.04 * RATE as f64) as usize..(0.14 * RATE as f64) as usize];
    let found: Vec<i32> = pitches(&mut sp, w, RATE, 28, 90, 4).iter().map(|p| p.0).collect();
    assert!(found.contains(&43) && found.contains(&50), "{found:?}");
    let x2 = render(&[p(0.0, 4, 1)], 0.0, 0.5, 1.0, 0.0);
    let w = &x2[(0.02 * RATE as f64) as usize..(0.12 * RATE as f64) as usize];
    let found: Vec<i32> = pitches(&mut sp, w, RATE, 28, 90, 2).iter().map(|p| p.0).collect();
    assert_eq!(found[0], 60, "{found:?}");
}

#[test]
fn takes_are_saved_with_their_timing_and_only_the_last_ten_kept() {
    // (in a takes folder of your choosing, P23.9: listed and pruned there)
    let root = std::env::temp_dir().join(format!("qrs-take-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let notes = lick();
    let a = chart(&notes, 6.0);
    let input = TakeInput {
        arr: a,
        from: 0.0,
        to: 5.0,
        samples: render(&notes, 0.0, 5.0, 1.0, 0.0),
        timing: TakeTiming::linear(RATE, 0.0, 1.0),
    };
    let r = analyse(&input, &|_| {}, &AtomicBool::new(false)).unwrap();
    let takes = root.join("my takes");
    let mut last = None;
    for k in 0..12 {
        last = Some(save_take(&takes, &input, Some(&r), &format!("20261015-1200{k:02}")).unwrap());
    }
    let path = last.unwrap();
    assert!(
        path.ends_with("my takes/Synth - Test/Lead-0.0-5.0-20261015-120011.json"),
        "{path:?}"
    );
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["summary"]["hit"], 16);
    assert_eq!(TakeTiming::from_json(&v["timing"]).unwrap(), input.timing);
    let saved = saved_takes(&takes, "Synth", "Test");
    assert_eq!(saved.len(), KEEP_PER_STRETCH);
    assert_eq!(saved[0], path);
    let wav = path.with_extension("wav");
    assert_eq!(
        std::fs::metadata(&wav).unwrap().len(),
        44 + 2 * input.samples.len() as u64
    );
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(stamp_now().len(), 15);
}

#[test]
fn anchors_from_a_recording_are_cleaned() {
    let pts: Vec<(u64, f64)> = (0..200).map(|k| (k * 480, 10.0 + k as f64 * 0.01)).collect();
    let mut noisy = pts.clone();
    noisy.insert(50, (100, 10.0)); // goes back: dropped
    let t = TakeTiming::from_anchors(48000, 1.0, &noisy);
    assert!(t.anchors.len() < 60, "{}", t.anchors.len());
    assert_eq!(t.anchors.last(), pts.last());
    assert!((t.chart_time(48000.0) - 11.0).abs() < 1e-6);
}

#[test]
fn nothing_checkable_is_not_zero_percent() {
    // only hammer-ons, none played: nothing could be checked
    let notes = vec![
        Played {
            pick: false,
            ..p(1.0, 3, 7)
        },
        Played {
            pick: false,
            ..p(1.5, 3, 9)
        },
    ];
    let a = chart(&notes, 3.0);
    let r = run(&a, vec![0.0; RATE as usize * 3], 0.0, 2.5, 1.0);
    assert_eq!(r.count(Status::NotChecked), 2);
    assert_eq!(r.checked(), 0);
    assert_eq!(r.accuracy_checked(), None);
    assert!(!r.passed(0.5));
    assert_eq!(r.to_json()["summary"]["checked"], 0);
    let notes = lick();
    let r = run(
        &chart(&notes, 6.0),
        render(&notes, 0.0, 5.0, 1.0, 0.0),
        0.0,
        5.0,
        1.0,
    );
    assert_eq!(r.checked(), 16);
    assert_eq!(r.accuracy_checked(), Some(1.0));
}

#[test]
fn a_job_whose_thread_died_is_finished_without_a_report() {
    let (tx, rx) = std::sync::mpsc::channel::<Option<TakeReport>>();
    drop(tx);
    let mut job = TakeJob {
        progress: Arc::new(AtomicU32::new(0)),
        cancel: Arc::new(AtomicBool::new(false)),
        rx,
        result: None,
    };
    assert!(job.finished());
    assert!(job.failed());
    assert!(job.take().is_none());
}

#[test]
fn problems_name_bars_by_the_charts_measures() {
    // the stretch starts at the chart's third bar (measure 3)
    let notes: Vec<Played> = lick().into_iter().map(|n| Played { t: n.t + 4.0, ..n }).collect();
    let a = chart(&notes, 10.0);
    let mut played = notes.clone();
    played[4].fret = 4;
    let r = run(&a, render(&played, 4.0, 5.0, 1.0, 0.0), 4.0, 9.0, 1.0);
    assert_eq!(r.bars[0].number, 3);
    assert!(r.problems[0].starts_with("Bar 3 (5.5 s)"), "{:?}", r.problems);
    let j = r.to_json();
    assert_eq!(j["events"][4]["measure"], 3);
    assert_eq!(j["events"][4]["bar"], 1);
}

/// The same power chord struck again a sixteenth later, its low string barely sounding (a quick strum's weakest
/// string): still the chord, as the scorer takes it (`detect::need`: two of three), with the string noted. Its fifth
/// missing is another sound (root and octave), and stays wrong.
#[test]
fn a_chord_struck_again_with_a_quiet_root_is_still_the_chord() {
    let shape = [(1, 0), (2, 2), (3, 2)];
    let mut notes = Vec::new();
    for t in [0.5, 0.66, 1.2] {
        notes.extend(strum(t, &shape));
    }
    // (the chart has the notes at the strum's start)
    let at = [0.5, 0.66, 1.2];
    let on_the_beat: Vec<Played> = notes
        .iter()
        .map(|n| Played {
            t: *at.iter().rfind(|&&t| t <= n.t + 1e-6).unwrap(),
            ..n.clone()
        })
        .collect();
    let a = chart(&on_the_beat, 2.5);
    let clean = run(&a, render(&notes, 0.0, 2.0, 1.0, 0.0), 0.0, 2.0, 1.0);
    assert_eq!(clean.count(Status::Hit), 3, "{}", clean.text(100));

    // the second strike's root at a twentieth of the level
    let quiet: Vec<Played> = notes
        .iter()
        .map(|n| Played {
            amp: if (n.t - 0.66).abs() < 0.001 && n.string == 1 { n.amp * 0.05 } else { n.amp },
            ..n.clone()
        })
        .collect();
    let r = run(&a, render(&quiet, 0.0, 2.0, 1.0, 0.0), 0.0, 2.0, 1.0);
    assert_eq!(statuses(&r), vec![Status::Hit; 3], "{}", r.text(100));

    // the fifth never played (root and octave: another sound), the strike repeated: its string is no peak
    let no_fifth: Vec<Played> = notes.iter().filter(|n| n.string != 2).cloned().collect();
    let r = run(&a, render(&no_fifth, 0.0, 2.0, 1.0, 0.0), 0.0, 2.0, 1.0);
    assert_eq!(statuses(&r)[1], Status::Wrong, "{}", r.text(100));
}

/// The same low note struck again a sixteenth later (the song's B2 B2): the note before still rings as loud as the
/// new one, so what "got louder" is only the attack's leftovers. Each is read as the note it is.
#[test]
fn a_low_note_struck_again_fast_is_read_as_that_note() {
    let notes: Vec<Played> = [0.5, 0.66, 0.82, 1.5, 1.66]
        .iter()
        .map(|&t| p(t, 0, 7))
        .collect();
    let a = chart(&notes, 2.5);
    let r = run(&a, render(&notes, 0.0, 2.0, 1.0, 0.0), 0.0, 2.0, 1.0);
    assert_eq!(statuses(&r), vec![Status::Hit; 5], "{}", r.text(100));
    assert!(r.events.iter().all(|e| e.played.iter().all(|p| p.midi == 47)), "{:?}", r.events);
}
