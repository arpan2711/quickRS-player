//! The take analysis (qrs-analysis take.rs) on the app's own synth guitar (P18.4): synthetic charts played clean
//! and with errors put in. Pins the accuracy measured when it was built (docs/PROGRESS.md, P18.4); the same checks
//! on real charts: `quickrs-cli take-check`.

use std::sync::atomic::AtomicBool;

use qrs_analysis::take::{self, Status, TakeInput, TakeReport, TakeTiming};
use qrs_audio::synth::render_mono;
use qrs_formats::Arrangement;
use qrs_formats::model::{Beat, Chord, Note, Techs};

const RATE: u32 = 48000;

fn grid(secs: f64, beat: f64) -> Vec<Beat> {
    (0..(secs / beat) as usize + 1)
        .map(|i| Beat {
            time: i as f64 * beat,
            measure: if i % 4 == 0 { (i / 4 + 1) as i32 } else { -1 },
        })
        .collect()
}

fn chord(t: f64, shape: &[(i32, i32)]) -> Chord {
    Chord {
        time: t,
        chord_id: 0,
        name: String::new(),
        notes: shape.iter().map(|&(s, f)| Note::new(t, s, f)).collect(),
        techniques: Techs::NONE,
        sustain: 0.0,
        frets: vec![],
        fingers: vec![],
        high_density: false,
    }
}

/// A lead part: a run of eighths at 120 bpm, double stops, a bend, a slide and a grace hammer-on.
fn lead() -> Arrangement {
    let mut a = Arrangement::new("Lead");
    a.title = "Synth lead".into();
    a.artist = "quickRS".into();
    a.beats = grid(20.0, 0.5);
    let run = [(3, 5), (3, 7), (4, 5), (4, 8), (5, 5), (5, 8), (4, 5), (4, 8)];
    let mut t = 1.0;
    for _ in 0..3 {
        for &(s, f) in &run {
            a.notes.push(Note::new(t, s, f));
            t += 0.25;
        }
        for k in 0..4 {
            a.chords.push(chord(t + 0.25 * k as f64, &[(3, 7), (4, 8)]));
        }
        t += 1.0;
    }
    a.notes.push(Note {
        bend: 2.0,
        sustain: 0.5,
        techniques: Techs::BEND | Techs::SUSTAIN,
        ..Note::new(t, 3, 7)
    });
    t += 1.0;
    a.notes.push(Note::new(t, 4, 5));
    a.notes.push(Note {
        techniques: Techs::HAMMER_ON,
        ..Note::new(t + 0.04, 4, 7)
    });
    t += 0.5;
    a.notes.push(Note {
        slide_to: 9,
        sustain: 0.4,
        techniques: Techs::SLIDE | Techs::SUSTAIN,
        ..Note::new(t, 3, 5)
    });
    a.notes.sort_by(|x, y| x.time.total_cmp(&y.time));
    a
}

/// A rhythm part: a shuffled boogie (double stops on the low strings) over four bars, then strummed chords.
fn rhythm() -> Arrangement {
    let mut a = Arrangement::new("Rhythm");
    a.title = "Synth rhythm".into();
    a.artist = "quickRS".into();
    a.beats = grid(20.0, 0.5);
    let tops = [2, 2, 4, 4, 5, 5, 4, 4];
    for bar in 0..4 {
        let (s, f) = if bar == 2 { (1, 5) } else { (0, 5) };
        for (k, top) in tops.iter().enumerate() {
            let off = if k % 2 == 0 { 0.0 } else { 1.0 / 3.0 };
            let t = 1.0 + bar as f64 * 2.0 + (k / 2) as f64 * 0.5 + off;
            a.chords.push(chord(t, &[(s, f), (s + 1, f + top)]));
        }
    }
    let shapes: [&[(i32, i32)]; 4] = [
        &[(0, 3), (1, 2), (2, 0), (3, 0), (4, 0), (5, 3)],
        &[(1, 3), (2, 2), (3, 0), (4, 1), (5, 0)],
        &[(0, 3), (1, 5), (2, 5)],
        &[(1, 0), (2, 2), (3, 2), (4, 2), (5, 0)],
    ];
    for (k, sh) in shapes.iter().enumerate() {
        a.chords.push(chord(9.0 + k as f64, sh));
    }
    a
}

fn analyse(chart: &Arrangement, x: Vec<f32>, from: f64, to: f64, speed: f64) -> TakeReport {
    take::analyse(
        &TakeInput {
            arr: chart.clone(),
            from,
            to,
            samples: x,
            timing: TakeTiming::linear(RATE, from, speed),
        },
        &|_| {},
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn play(a: &Arrangement, from: f64, to: f64, speed: f64) -> Vec<f32> {
    render_mono(a, from, to, speed, None, RATE)
}

/// Events in time order as (time, note index or chord index).
fn event_times(a: &Arrangement) -> Vec<f64> {
    let mut t: Vec<f64> = a
        .notes
        .iter()
        .map(|n| n.time)
        .chain(a.chords.iter().map(|c| c.time))
        .collect();
    t.sort_by(|x, y| x.total_cmp(y));
    t.dedup_by(|x, y| (*x - *y).abs() < 0.015);
    t
}

fn status_at(r: &TakeReport, t: f64) -> (Status, Option<f64>) {
    let e = r.events.iter().find(|e| (e.t - t).abs() < 0.001).unwrap();
    (e.status, e.offset_ms)
}

#[test]
fn clean_takes_are_all_right() {
    for (a, to) in [(lead(), 14.0), (rhythm(), 13.5)] {
        for speed in [1.0, 0.5] {
            let r = analyse(&a, play(&a, 0.5, to, speed), 0.5, to, speed);
            assert_eq!(
                r.count(Status::Hit),
                r.events.len(),
                "{} at {speed}: {}",
                a.name,
                r.text(120)
            );
            assert!(r.extras.is_empty(), "{:?}", r.extras);
            assert!(r.timing.median_ms.abs() < 6.0, "{:?}", r.timing);
        }
    }
}

#[test]
fn errors_put_in_are_found() {
    for (a, to) in [(lead(), 14.0), (rhythm(), 13.5)] {
        let times = event_times(&a);
        // a fret up on single notes and chords alike (every 5th event), every 7th dropped
        let mut played = a.clone();
        let mut wrong = Vec::new();
        let mut dropped = Vec::new();
        for (k, &t) in times.iter().enumerate() {
            let plain = |n: &Note| n.techniques.is_empty();
            // (not a grace note: hammered straight on, it's given the benefit)
            if times.get(k + 1).is_some_and(|&u| u - t < 0.06) {
                continue;
            }
            if k % 5 == 2 {
                if let Some(n) = played
                    .notes
                    .iter_mut()
                    .find(|n| (n.time - t).abs() < 1e-9 && plain(n))
                {
                    n.fret += 1;
                    wrong.push(t);
                } else if let Some(c) = played
                    .chords
                    .iter_mut()
                    .find(|c| (c.time - t).abs() < 1e-9)
                    .filter(|c| c.notes.iter().any(|n| n.string >= 3) && c.notes.len() <= 4)
                {
                    // (not shapes on the low strings only: a fret off there is a few Hz from the right note,
                    // under what the 85 ms window splits; nor full chords, where the top string is often an
                    // octave of a lower one and its harmonic hides the change; see PROGRESS P18.4)
                    // the top string (a fret off on a low string inside a full chord is a few Hz away from
                    // the right note, under what an 85 ms window can split: not pinned)
                    let top = c.notes.len() - 1;
                    c.notes[top].fret += 1;
                    wrong.push(t);
                }
            } else if k % 7 == 4
                && played
                    .notes
                    .iter()
                    .chain(played.chords.iter().flat_map(|c| &c.notes))
                    .any(|n| (n.time - t).abs() < 1e-9 && plain(n))
            {
                played.notes.retain(|n| (n.time - t).abs() > 1e-9);
                played.chords.retain(|c| (c.time - t).abs() > 1e-9);
                dropped.push(t);
            }
        }
        let r = analyse(&a, play(&played, 0.5, to, 1.0), 0.5, to, 1.0);
        for &t in &wrong {
            assert_eq!(
                status_at(&r, t).0,
                Status::Wrong,
                "{} wrong at {t}: {}",
                a.name,
                r.text(120)
            );
        }
        // a dropped beat of a repeated boogie shape is sometimes taken for the shape still ringing (on JBG's
        // rhythm 69 of 82 found; here 2 of 4 in the rhythm): pinned at half, all in the lead
        let found = dropped
            .iter()
            .filter(|&&t| status_at(&r, t).0 == Status::Missed)
            .count();
        let need = if a.name == "Lead" {
            dropped.len()
        } else {
            dropped.len().div_ceil(2)
        };
        assert!(
            found >= need,
            "{} dropped {found}/{}: {}",
            a.name,
            dropped.len(),
            r.text(120)
        );
        let others = r
            .events
            .iter()
            .filter(|e| !wrong.iter().chain(&dropped).any(|&t| (e.t - t).abs() < 0.001))
            .filter(|e| e.status != Status::Hit)
            .count();
        // (the rhythm's undetected drops leave a neighbour called wrong now and then)
        assert!(
            others <= 2,
            "{} right notes called wrong: {}",
            a.name,
            r.text(120)
        );
    }
}

#[test]
fn timing_shifts_and_a_late_take_are_measured() {
    let a = lead();
    let times = event_times(&a);
    let mut played = a.clone();
    let mut shifted = Vec::new();
    for (k, &t) in times.iter().enumerate().take(28) {
        if k % 4 == 1 {
            let ms = [40.0, -40.0, 80.0][(k / 4) % 3];
            for n in played.notes.iter_mut().filter(|n| (n.time - t).abs() < 1e-9) {
                n.time += ms / 1000.0;
            }
            for c in played.chords.iter_mut().filter(|c| (c.time - t).abs() < 1e-9) {
                c.time += ms / 1000.0;
                for n in &mut c.notes {
                    n.time += ms / 1000.0;
                }
            }
            shifted.push((t, ms));
        }
    }
    let r = analyse(&a, play(&played, 0.5, 14.0, 1.0), 0.5, 14.0, 1.0);
    for (t, ms) in shifted {
        let (st, off) = status_at(&r, t);
        assert_eq!(st, Status::Hit);
        assert!((off.unwrap() - ms).abs() <= 6.0, "{t}: {off:?} for {ms}");
    }
    // the whole take 30 ms late
    let mut x = vec![0.0f32; (0.03 * RATE as f64) as usize];
    x.extend(play(&a, 0.5, 14.0, 1.0));
    let r = analyse(&a, x, 0.5, 14.0, 1.0);
    assert_eq!(r.count(Status::Hit), r.events.len());
    assert!((r.timing.median_ms - 30.0).abs() < 4.0, "{:?}", r.timing);
    assert!(
        r.problems.iter().any(|p| p.contains("late (dragging)")),
        "{:?}",
        r.problems
    );
}

#[test]
fn extra_notes_noise_and_a_level_drop() {
    let a = lead();
    let mut played = a.clone();
    // two extra notes in the gaps after the double stops
    played.notes.push(Note::new(4.5 + 0.125, 2, 7));
    played.notes.push(Note::new(13.25, 5, 10));
    played.notes.sort_by(|x, y| x.time.total_cmp(&y.time));
    let r = analyse(&a, play(&played, 0.5, 14.0, 1.0), 0.5, 14.0, 1.0);
    assert_eq!(r.extras.len(), 2, "{}", r.text(120));
    assert_eq!(r.extras[0].played[0].midi, 57);
    assert_eq!(r.count(Status::Hit), r.events.len());

    let mut x = play(&a, 0.5, 14.0, 1.0);
    let n = x.len();
    let mut seed: u32 = 5;
    for (i, v) in x.iter_mut().enumerate() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let hiss = ((seed >> 8) as f32 / 16777216.0 - 0.5) * 0.004;
        let hum = 0.002 * (std::f32::consts::TAU * 50.0 * i as f32 / RATE as f32).sin();
        *v = *v * if i > n / 2 { 0.1 } else { 1.0 } + hiss + hum;
    }
    let r = analyse(&a, x, 0.5, 14.0, 1.0);
    assert!(r.count(Status::Hit) + 1 >= r.events.len(), "{}", r.text(120));
    assert!(r.extras.is_empty(), "{:?}", r.extras);
}

#[test]
fn a_three_minute_take_is_analysed_in_a_few_seconds() {
    // the rhythm part repeated to three minutes
    let base = rhythm();
    let mut a = base.clone();
    a.beats = grid(185.0, 0.5);
    for k in 1..14 {
        let dt = 13.0 * k as f64;
        a.chords.extend(base.chords.iter().map(|c| {
            let mut c = c.clone();
            c.time += dt;
            for n in &mut c.notes {
                n.time += dt;
            }
            c
        }));
    }
    let x = play(&a, 0.5, 180.0, 1.0);
    let t = std::time::Instant::now();
    let r = analyse(&a, x, 0.5, 180.0, 1.0);
    let secs = t.elapsed().as_secs_f64();
    assert!(r.accuracy() > 0.98, "{}", r.accuracy());
    // (a debug build is far slower than the app's release build: ~1 s there). A debug build's wall clock says more
    // about the machine's load than the code (other builds running: 60 s overrun, P37.4), so there it only warns,
    // unless QUICKRS_STRICT_RT=1; release always asserts.
    let strict = !cfg!(debug_assertions) || std::env::var("QUICKRS_STRICT_RT").is_ok_and(|v| v == "1");
    let limit = if cfg!(debug_assertions) { 60.0 } else { 5.0 };
    if strict {
        assert!(secs < limit, "{secs} s");
    } else if secs >= limit {
        eprintln!(
            "a three-minute take took {secs:.1} s (a loaded machine? QUICKRS_STRICT_RT=1 makes this fail)"
        );
    }
}

#[test]
fn a_whole_part_on_the_song_s_clock_is_all_right() {
    // the player's synth guitar (P23.11): the whole part in one render, the song 0.3 s ahead of the chart; every
    // note is where the chart has it, and it's as long as the song
    use qrs_audio::synth::{notes_of, render_part};
    use std::sync::atomic::AtomicU32;
    let offset = 0.3;
    for (a, to) in [(lead(), 14.0), (rhythm(), 13.5)] {
        let frames = 20 * RATE as usize;
        let (cancel, progress) = (AtomicBool::new(false), AtomicU32::new(0));
        let x = render_part(
            &notes_of(&a),
            &a.open_string_midi(),
            a.is_bass,
            None,
            RATE,
            frames,
            offset,
            &cancel,
            &progress,
        )
        .unwrap();
        assert_eq!(x.len(), frames);
        let i0 = ((0.5 + offset) * RATE as f64).round() as usize;
        let i1 = (((to + 1.0 + offset) * RATE as f64) as usize).min(frames);
        let r = analyse(&a, x[i0..i1].to_vec(), 0.5, to, 1.0);
        // every note there, none extra, on time; (one shape of the rhythm's boogie is now and then heard a fret
        // off, as in `errors_put_in_are_found`: the analyser's limit, not the synth's)
        assert_eq!(r.count(Status::Missed), 0, "{}: {}", a.name, r.text(120));
        assert!(r.extras.is_empty(), "{}: {:?}", a.name, r.extras);
        assert!(
            r.count(Status::Hit) + 1 >= r.events.len(),
            "{}: {}",
            a.name,
            r.text(120)
        );
        assert!(r.timing.median_ms.abs() < 6.0, "{:?}", r.timing);
    }
}
