//! `quickrs-cli corpus`: a labelled set of short recordings to measure (and tune) note and chord detection on.
//!
//!   quickrs-cli corpus init DIR     write the exercises: a label file (NAME.json: what the chart wants and what is
//!                                   really played at each beat), a click track (NAME.click.wav) and SCRIPT.md (what
//!                                   to play, bar by bar)
//!   quickrs-cli corpus score DIR    score the recordings in DIR (NAME.wav, or NAME@TONE.wav for the same exercise at
//!                                   another amp tone) the way the app scores live: the pluck detector and the
//!                                   scorer, against the labels
//!
//! The question it answers is the one a threshold change has to: of the strikes that were right, how many does the
//! game accept, and of the ones that were not (a wrong chord, a held chord that was not struck again, a muted
//! strum, a note a fret off), how many does it accept anyway? Recordings are put on the chart's clock by finding
//! the shift that lines the first plucks up with the first labelled strikes, so they may start at any time (record,
//! wait two seconds, then start the click).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use qrs_analysis::detect::Judgement;
use serde_json::{Value, json};

use crate::chords_cmd::{self, Item, RATE, Shape};

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Write the exercises (labels, click tracks, SCRIPT.md) into a folder
    Init {
        dir: PathBuf,
    },
    /// Score the recordings in a folder against their labels
    Score(ScoreArgs),
}

#[derive(clap::Args)]
pub struct ScoreArgs {
    pub dir: PathBuf,
    /// list every strike the game got wrong, with the chord test's reason
    #[arg(long)]
    pub why: bool,
    /// write the results here (JSON), to compare with `--baseline` after a change
    #[arg(long)]
    pub json: Option<PathBuf>,
    /// a results file from an earlier run: print what changed
    #[arg(long)]
    pub baseline: Option<PathBuf>,
    /// use this shift (seconds, chart minus recording) instead of finding it
    #[arg(long)]
    pub shift: Option<f64>,
    /// only recordings whose name contains this
    #[arg(long)]
    pub only: Option<String>,
}

pub fn run(cmd: Cmd) -> anyhow::Result<()> {
    match cmd {
        Cmd::Init { dir } => init(&dir),
        Cmd::Score(a) => score_dir(&a),
    }
}

// -- the exercises ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Truth {
    /// Played as the chart wants it: should be accepted.
    Right,
    /// Something else was played (or a muted strum): should not be accepted.
    Wrong,
    /// Nothing struck (the last chord left ringing): should not be accepted.
    None,
}

impl Truth {
    fn name(self) -> &'static str {
        match self {
            Truth::Right => "right",
            Truth::Wrong => "wrong",
            Truth::None => "none",
        }
    }
    fn of(s: &str) -> Option<Truth> {
        Some(match s {
            "right" => Truth::Right,
            "wrong" => Truth::Wrong,
            "none" => Truth::None,
            _ => return None,
        })
    }
}

/// What is struck at an event.
#[derive(Clone, Debug, PartialEq)]
pub enum Played {
    /// The chart's shape.
    AsCharted,
    /// These (string, fret)s instead.
    Shape(Shape),
    /// A fret-hand muted strum.
    Muted,
    /// Nothing.
    Nothing,
}

#[derive(Clone, Debug)]
pub struct Ev {
    /// Beats from the click's first beat (the count-in is the first four).
    pub beat: f64,
    pub want: Shape,
    pub label: String,
    pub truth: Truth,
    pub tag: &'static str,
    pub played: Played,
    /// What to say in the script when it is not simply "strike LABEL".
    pub say: String,
}

pub struct Exercise {
    pub name: &'static str,
    pub bpm: f64,
    /// Click ticks per beat (4: a soft tick on every sixteenth, to play to).
    pub sub: u32,
    pub about: &'static str,
    pub events: Vec<Ev>,
}

impl Exercise {
    fn t(&self, beat: f64) -> f64 {
        beat * 60.0 / self.bpm
    }
}

const D5: [(i32, i32); 3] = [(1, 5), (2, 7), (3, 7)];
const A5: [(i32, i32); 3] = [(0, 5), (1, 7), (2, 7)];
const E5: [(i32, i32); 3] = [(0, 0), (1, 2), (2, 2)];
const G5: [(i32, i32); 3] = [(0, 3), (1, 5), (2, 5)];
const C5: [(i32, i32); 3] = [(1, 3), (2, 5), (3, 5)];

fn sh(s: &[(i32, i32)]) -> Shape {
    s.to_vec()
}

fn right(beat: f64, label: &str, want: &[(i32, i32)], tag: &'static str) -> Ev {
    Ev {
        beat,
        want: sh(want),
        label: label.into(),
        truth: Truth::Right,
        tag,
        played: Played::AsCharted,
        say: format!("strike {label}"),
    }
}

fn held(beat: f64, label: &str, want: &[(i32, i32)]) -> Ev {
    Ev {
        beat,
        want: sh(want),
        label: label.into(),
        truth: Truth::None,
        tag: "held",
        played: Played::Nothing,
        say: "do NOT strike (let it ring)".into(),
    }
}

fn wrong(beat: f64, label: &str, want: &[(i32, i32)], tag: &'static str, played: Played, say: String) -> Ev {
    Ev {
        beat,
        want: sh(want),
        label: label.into(),
        truth: Truth::Wrong,
        tag,
        played,
        say,
    }
}

fn open(frets: &str) -> Shape {
    chords_cmd::shape(frets)
}

pub fn exercises() -> Vec<Exercise> {
    let mut v = Vec::new();

    // 1. the song's riff: a chord struck three times a sixteenth apart, in time with a click at 95 bpm
    let seq = [
        ("D5", D5),
        ("A5", A5),
        ("E5", E5),
        ("D5", D5),
        ("D5", D5),
        ("A5", A5),
        ("A5", A5),
        ("E5", E5),
    ];
    let mut ev = Vec::new();
    for (i, (n, s)) in seq.iter().enumerate() {
        for (k, off) in [0.0, 0.25, 0.5].into_iter().enumerate() {
            ev.push(right(4.0 + i as f64 * 4.0 + off, n, s, if k == 0 { "first" } else { "repeat" }));
        }
    }
    v.push(Exercise {
        name: "repeat-fast",
        sub: 4,
        bpm: 95.0,
        about: "A power chord struck three times in a row, a sixteenth note apart (the Back in Black rhythm), then a rest.",
        events: ev,
    });

    // 2. eighth notes
    let mut ev = Vec::new();
    for (i, (n, s)) in [("D5", D5), ("A5", A5), ("E5", E5), ("G5", G5)].iter().enumerate() {
        for k in 0..8 {
            ev.push(right(4.0 + i as f64 * 4.0 + k as f64 * 0.5, n, s, if k == 0 { "first" } else { "eighths" }));
        }
    }
    v.push(Exercise {
        name: "repeat-eighths",
        sub: 2,
        bpm: 110.0,
        about: "A power chord struck on every eighth note for a bar, then the next chord.",
        events: ev,
    });

    // 3. chords struck, then left ringing where the chart asks for another strike
    let bars = [
        ("D5", D5, "RNN"),
        ("A5", A5, "RNR"),
        ("E5", E5, "RRN"),
        ("D5", D5, "RNN"),
        ("A5", A5, "RNR"),
        ("E5", E5, "RRN"),
        ("G5", G5, "RNR"),
        ("C5", C5, "RRN"),
    ];
    let mut ev = Vec::new();
    for (i, (n, s, pat)) in bars.iter().enumerate() {
        for (k, c) in pat.chars().enumerate() {
            let beat = 4.0 + i as f64 * 4.0 + k as f64 * 0.25;
            if c == 'R' {
                ev.push(right(beat, n, s, if k == 0 { "first" } else { "repeat" }));
            } else {
                ev.push(held(beat, n, s));
            }
        }
    }
    v.push(Exercise {
        name: "hold-skip",
        sub: 4,
        bpm: 95.0,
        about: "Like the riff, but where the card says \"do NOT strike\" you let the chord ring: the game must not give credit for a chord that was not struck again.",
        events: ev,
    });

    // 4. wrong shapes and muted strums among right ones, a quarter note apart
    let pairs: Vec<(&str, &[(i32, i32)], Played, &'static str, Truth, String)> = vec![
        ("D5", &D5, Played::AsCharted, "first", Truth::Right, "".into()),
        ("D5", &D5, Played::Shape(sh(&A5)), "wrong-shape", Truth::Wrong, "strike A5 (wrong on purpose)".into()),
        ("E5", &E5, Played::AsCharted, "first", Truth::Right, "".into()),
        ("E5", &E5, Played::Shape(sh(&D5)), "wrong-shape", Truth::Wrong, "strike D5 (wrong on purpose)".into()),
        ("A5", &A5, Played::AsCharted, "first", Truth::Right, "".into()),
        ("A5", &A5, Played::Muted, "muted", Truth::Wrong, "strum it MUTED (wrong on purpose: lay the fret hand across the strings for a dead \"chk\")".into()),
        ("D5", &D5, Played::AsCharted, "first", Truth::Right, "".into()),
        (
            "D5",
            &D5,
            Played::Shape(vec![(1, 6), (2, 8), (3, 8)]),
            "wrong-shape",
            Truth::Wrong,
            "strike the D5 shape ONE FRET HIGHER (frets 6, 8, 8; wrong on purpose)".into(),
        ),
        ("A5", &A5, Played::AsCharted, "first", Truth::Right, "".into()),
        (
            "A5",
            &A5,
            Played::Shape(vec![(0, 5)]),
            "single-for-chord",
            Truth::Wrong,
            "play only the low note of A5, one string (wrong on purpose)".into(),
        ),
        ("E5", &E5, Played::AsCharted, "first", Truth::Right, "".into()),
        (
            "E5",
            &E5,
            Played::Shape(vec![(1, 2), (2, 2)]),
            "partial",
            Truth::Right,
            "play only the top TWO strings of E5 (the game forgives one missing string)".into(),
        ),
    ];
    let mut ev = Vec::new();
    for round in 0..2 {
        for (i, (n, s, played, tag, truth, say)) in pairs.iter().enumerate() {
            let beat = 4.0 + (round * pairs.len() + i) as f64;
            let mut e = right(beat, n, s, tag);
            e.truth = *truth;
            e.played = played.clone();
            if !say.is_empty() {
                e.say = say.clone();
            }
            ev.push(e);
        }
    }
    v.push(Exercise {
        name: "wrong-shapes",
        sub: 1,
        bpm: 70.0,
        about: "Quarter notes: right chords mixed with wrong shapes, a muted strum, one string only, and a chord with one string left out.",
        events: ev,
    });

    // 5. single notes up and down the low strings
    let mut ev = Vec::new();
    let mut beat = 4.0;
    for (string, tag, name) in [(0, "single-low", "low E"), (1, "single-low", "A")] {
        for fret in (0..=7).chain((0..=6).rev()) {
            let label = format!("{name} string, fret {fret}");
            ev.push(right(beat, &label, &[(string, fret)], tag));
            beat += 1.0;
        }
    }
    v.push(Exercise {
        name: "singles-low",
        sub: 1,
        bpm: 80.0,
        about: "Single notes up and down frets 0-7 of the low E string, then the A string.",
        events: ev,
    });

    // 5b. the same low note struck again fast, as in the song's chromatic low-E run (B2 A2 B2 Bb2 B2 B2)
    let mut ev = Vec::new();
    for (i, (string, fret, name)) in [(0, 7, "B2"), (0, 5, "A2"), (0, 6, "Bb2"), (1, 2, "B2")].iter().enumerate() {
        for k in 0..3 {
            let label = format!("{name} (string {}, fret {fret})", if *string == 0 { "low E" } else { "A" });
            ev.push(right(4.0 + i as f64 * 4.0 + k as f64 * 0.25, &label, &[(*string, *fret)], if k == 0 { "single" } else { "single-repeat" }));
        }
    }
    let run = [(0, 7, "B2", 0.0), (0, 5, "A2", 0.25), (0, 7, "B2", 0.75), (0, 6, "Bb2", 1.0), (0, 7, "B2", 1.5), (0, 7, "B2", 1.75)];
    for r in 0..3 {
        for (string, fret, name, off) in run {
            let label = format!("{name} (low E, fret {fret})");
            let tag = if off == 1.75 || off == 0.25 || off == 1.0 { "single-repeat" } else { "single" };
            ev.push(right(20.0 + r as f64 * 4.0 + off, &label, &[(string, fret)], tag));
        }
    }
    v.push(Exercise {
        name: "single-repeat",
        sub: 4,
        bpm: 95.0,
        about: "Single low notes struck again fast: the same note three times, then the song's run B A B Bb B B on the low E string (frets 7 5 7 6 7 7).",
        events: ev,
    });

    // 6. single notes on the middle and high strings
    let mut ev = Vec::new();
    let mut beat = 4.0;
    for (string, name) in [(2, "D"), (3, "G"), (4, "B")] {
        for fret in (0..=5).chain((0..=4).rev()) {
            let label = format!("{name} string, fret {fret}");
            ev.push(right(beat, &label, &[(string, fret)], "single-mid"));
            beat += 1.0;
        }
    }
    v.push(Exercise {
        name: "singles-mid",
        sub: 1,
        bpm: 80.0,
        about: "Single notes up and down frets 0-5 of the D, G and B strings.",
        events: ev,
    });

    // 7. single notes a fret off
    let mut ev = Vec::new();
    let mut beat = 4.0;
    for round in 0..2 {
        for (string, fret, name) in [(1, 5, "A string"), (0, 5, "low E string"), (2, 7, "D string"), (3, 5, "G string"), (1, 9, "A string")] {
            let label = format!("{name}, fret {fret}");
            ev.push(right(beat, &label, &[(string, fret)], "single"));
            beat += 1.0;
            let off = if (round + fret as usize).is_multiple_of(2) { 1 } else { -1 };
            ev.push(wrong(
                beat,
                &label,
                &[(string, fret)],
                "single-semitone",
                Played::Shape(vec![(string, fret + off)]),
                format!("play fret {} on the same string (wrong on purpose)", fret + off),
            ));
            beat += 1.0;
        }
    }
    v.push(Exercise {
        name: "single-semitone",
        sub: 1,
        bpm: 70.0,
        about: "Single notes in pairs: the right note, then the note a fret above or below it on the same string.",
        events: ev,
    });

    // 8. open chords
    let chords = [
        ("C", "x32010"),
        ("G", "320003"),
        ("Em", "022000"),
        ("Am", "x02210"),
        ("D", "xx0232"),
        ("E", "022100"),
    ];
    let mut ev = Vec::new();
    for (i, (n, f)) in chords.iter().enumerate() {
        for k in 0..4 {
            let mut e = right(4.0 + i as f64 * 2.0 + k as f64 * 0.5, n, &open(f), if k == 0 { "first" } else { "open" });
            e.say = format!("strum {n} ({f})");
            ev.push(e);
        }
    }
    v.push(Exercise {
        name: "open-strums",
        sub: 2,
        bpm: 90.0,
        about: "Open chords strummed on four eighth notes each (down, down, up, up if you like).",
        events: ev,
    });
    v
}

// -- init -----------------------------------------------------------------------------------------------------------

fn shape_json(s: &Shape) -> Value {
    json!(s.iter().map(|&(a, b)| json!([a, b])).collect::<Vec<_>>())
}

fn shape_of(v: &Value) -> Shape {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| Some((p.get(0)?.as_i64()? as i32, p.get(1)?.as_i64()? as i32)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn labels_json(ex: &Exercise) -> Value {
    let events: Vec<Value> = ex
        .events
        .iter()
        .map(|e| {
            let played = match &e.played {
                Played::AsCharted => shape_json(&e.want),
                Played::Shape(s) => shape_json(s),
                Played::Muted => json!("muted"),
                Played::Nothing => Value::Null,
            };
            json!({
                "t": (ex.t(e.beat) * 1e4).round() / 1e4,
                "beat": e.beat,
                "label": e.label,
                "want": shape_json(&e.want),
                "truth": e.truth.name(),
                "tag": e.tag,
                "played": played,
            })
        })
        .collect();
    json!({
        "about": ex.about,
        "name": ex.name,
        "bpm": ex.bpm,
        "events": events,
    })
}

/// A play-along track: a soft tick on every beat (the bar's first a little higher), and a loud bright "tock" at
/// each time in `strikes`: strike when you hear one, and only then. The first four beats are a count-in.
pub fn click_track(bpm: f64, strikes: &[f64], end: f64) -> Vec<f32> {
    let n = (end * RATE as f64) as usize;
    let mut x = vec![0.0f32; n];
    let mut put = |t: f64, freqs: &[f64], amp: f64, decay: f64, len: f64| {
        let at = (t * RATE as f64) as usize;
        for i in 0..(len * RATE as f64) as usize {
            if at + i >= n {
                break;
            }
            let s = i as f64 / RATE as f64;
            let v: f64 = freqs.iter().map(|f| (std::f64::consts::TAU * f * s).sin()).sum();
            x[at + i] += (amp * (-s * decay).exp() * v / freqs.len() as f64) as f32;
        }
    };
    let mut k = 0;
    while k as f64 * 60.0 / bpm < end {
        let f = if k % 4 == 0 { 900.0 } else { 650.0 };
        put(k as f64 * 60.0 / bpm, &[f], 0.2, 90.0, 0.04);
        k += 1;
    }
    for &t in strikes {
        put(t, &[1800.0, 2700.0, 3300.0], 0.9, 45.0, 0.12);
    }
    x
}

fn beat_name(beat: f64) -> String {
    let bar = (beat / 4.0).floor();
    let b = beat - bar * 4.0;
    let whole = b.floor();
    let frac = ((b - whole) * 4.0).round() as i32;
    let sub = ["", "e", "&", "a"][(frac as usize).min(3)];
    format!("{}{}", whole as i32 + 1, sub)
}

fn script(exs: &[Exercise]) -> String {
    let mut s = String::new();
    s.push_str(
        "# Detection corpus: what to record\n\n\
         About 15 minutes at your usual tone, and again at a clean and a high-gain tone if you are willing\n\
         (the first four exercises matter most). Each exercise is one short recording.\n\n\
         ## How to record each exercise\n\n\
         1. Open a recorder that can record the guitar input (Audacity is free; use the Katana's USB input, mono,\n\
            48000 Hz if it lets you choose).\n\
         2. Play the exercise's play-along track (`NAME.click.wav`) through your speakers or headphones from any media\n\
            player. It has a soft tick on every beat and a LOUD bright **tock** wherever you must strike: **strike exactly on every tock and never otherwise**. It counts in four beats first; play nothing until the count-in is over.\n\
         3. Order: **start recording, wait two seconds in silence, then start the click.** Then play the bars below\n\
            along with the click.\n\
         4. Stop recording after the last bar. Export as WAV and save it in this folder as `NAME.wav`\n\
            (for another tone: `NAME@clean.wav`, `NAME@crunch.wav`, `NAME@metal.wav`).\n\n\
         Do not try to be perfect: a slightly early or late strike is fine (the scorer finds the offset). Do play\n\
         exactly what each card says, especially \"do NOT strike\" and the deliberately wrong ones: they are what\n\
         measures how easily the game is fooled. If you slip and play the wrong thing, redo that exercise.\n\n\
         Then run `quickrs-cli corpus score THIS-FOLDER`.\n\n\
         Shapes (low E string first; fret numbers):\n\
         D5 = A string 5, D string 7, G string 7. A5 = low E 5, A 7, D 7. E5 = low E 0, A 2, D 2.\n\
         G5 = low E 3, A 5, D 5. C5 = A string 3, D 5, G 5. Power chords: strike only those three strings.\n\n",
    );
    for (i, ex) in exs.iter().enumerate() {
        let bars = ex.events.last().map(|e| (e.beat / 4.0).floor() as i32).unwrap_or(1);
        let sub = ex.sub.max(1) as usize;
        s.push_str(&format!(
            "## {}. `{}` ({} bpm; {} bars of 4 beats, about {:.0} seconds after the count-in)

{}

",
            i + 1,
            ex.name,
            ex.bpm,
            bars,
            bars as f64 * 4.0 * 60.0 / ex.bpm,
            ex.about
        ));
        let names = ["1", "e", "&", "a"];
        let slot_names: Vec<&str> = match sub {
            4 => names.to_vec(),
            2 => vec!["1", "&"],
            _ => vec!["1"],
        };
        let mut head = format!("{:<14}", "beat");
        for beat in 0..4 {
            for (k, n) in slot_names.iter().enumerate() {
                let n = if k == 0 { (beat + 1).to_string() } else { n.to_string() };
                head.push_str(&format!("{n} "));
            }
            head.push_str("| ");
        }
        s.push_str("```
");
        s.push_str(head.trim_end());
        s.push('\n');
        for bar in 1..=bars {
            let mut line = format!("{:<14}", format!("bar {bar}"));
            for beat in 0..4 {
                for k in 0..sub {
                    let at = bar as f64 * 4.0 + beat as f64 + k as f64 / sub as f64;
                    let c = match ex.events.iter().find(|e| (e.beat - at).abs() < 1e-6) {
                        Some(e) if e.truth == Truth::Right => 'X',
                        Some(e) if e.truth == Truth::Wrong => 'W',
                        Some(_) => 'o',
                        None => '.',
                    };
                    line.push(c);
                    line.push(' ');
                }
                line.push_str("| ");
            }
            s.push_str(line.trim_end());
            s.push('\n');
        }
        s.push_str("```

");
        s.push_str("X = strike, o = do NOT strike (let the last chord ring), W = strike the deliberately wrong thing, . = silent. ");
        s.push_str("Each X (and W) is one tock you will hear; the silent slots have none, so you simply do not play there. The grid is only a map of the tocks, and the list under it says which chord goes with each. Each bar is four beats long.

");
        // what to play in each bar
        for bar in 1..=bars {
            let evs: Vec<&Ev> = ex.events.iter().filter(|e| (e.beat / 4.0).floor() as i32 == bar).collect();
            let mut parts: Vec<(Vec<String>, String)> = Vec::new();
            for e in evs {
                let pos = beat_name(e.beat);
                match parts.last_mut() {
                    Some((p, say)) if *say == e.say => p.push(pos),
                    _ => parts.push((vec![pos], e.say.clone())),
                }
            }
            let text: Vec<String> = parts.iter().map(|(p, say)| format!("[{}] {}", p.join(" "), say)).collect();
            s.push_str(&format!("- bar {bar}: {}
", text.join("; ")));
        }
        s.push('\n');
    }
    s
}

fn init(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let exs = exercises();
    for ex in &exs {
        let end = ex.t(ex.events.last().map(|e| e.beat).unwrap_or(4.0) + 4.0);
        std::fs::write(dir.join(format!("{}.json", ex.name)), serde_json::to_string_pretty(&labels_json(ex))?)?;
        qrs_analysis::take::write_wav_mono(&dir.join(format!("{}.click.wav", ex.name)), RATE, &click_track(ex.bpm, &ex.events.iter().filter(|e| e.truth != Truth::None).map(|e| ex.t(e.beat)).collect::<Vec<_>>(), end))?;
    }
    std::fs::write(dir.join("SCRIPT.md"), script(&exs))?;
    println!(
        "wrote {} exercises (labels + click tracks) and SCRIPT.md in {}",
        exs.len(),
        dir.display()
    );
    println!("record each as NAME.wav (see SCRIPT.md), then: quickrs-cli corpus score {}", dir.display());
    Ok(())
}

// -- scoring ---------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct EvResult {
    pub rec: String,
    pub tone: String,
    pub t: f64,
    pub label: String,
    pub tag: String,
    pub truth: Truth,
    pub accepted: bool,
    /// The pluck detector found a strike within 60 ms.
    pub found: bool,
    pub why: String,
}

struct Labelled {
    t: f64,
    label: String,
    want: Shape,
    truth: Truth,
    tag: String,
}

fn read_labels(path: &Path) -> anyhow::Result<Vec<Labelled>> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?).with_context(|| path.display().to_string())?;
    let mut out = Vec::new();
    for e in v["events"].as_array().context("no events")? {
        out.push(Labelled {
            t: e["t"].as_f64().context("event without t")?,
            label: e["label"].as_str().unwrap_or("").into(),
            want: shape_of(&e["want"]),
            truth: Truth::of(e["truth"].as_str().unwrap_or("")).context("bad truth")?,
            tag: e["tag"].as_str().unwrap_or("").into(),
        });
    }
    out.sort_by(|a, b| a.t.total_cmp(&b.t));
    Ok(out)
}

/// The shift (chart minus recording, seconds) that puts the labelled strikes on the recording's plucks: the first
/// plucks tried against the first strike, each refined by the number of strikes within 70 ms of a pluck.
pub fn find_shift(strikes: &[f64], plucks: &[f64]) -> Option<(f64, usize)> {
    let first = *strikes.first()?;
    let mut best: Option<(f64, usize, f64)> = None;
    for &p in plucks.iter().take(6) {
        let base = p - first;
        for k in -30..=30 {
            let shift = base + k as f64 * 0.005;
            let (mut hit, mut err) = (0usize, 0.0);
            for &t in strikes {
                let at = t + shift;
                let i = plucks.partition_point(|&q| q < at);
                let d = [i.checked_sub(1), Some(i)]
                    .into_iter()
                    .flatten()
                    .filter_map(|j| plucks.get(j))
                    .map(|q| (q - at).abs())
                    .fold(f64::INFINITY, f64::min);
                if d < 0.07 {
                    hit += 1;
                    err += d;
                }
            }
            if best.is_none_or(|(_, h, e)| hit > h || (hit == h && err < e)) {
                best = Some((shift, hit, err));
            }
        }
    }
    best.map(|(s, h, _)| (s, h))
}

/// Where each labelled strike really happened.
pub struct Snapped {
    /// Each event's time: the pluck it was struck at (nearest within 100 ms of its tock, the first of a strum's
    /// plucks), else the tock's time.
    pub times: Vec<f64>,
    /// A strike was found for it.
    pub found: Vec<bool>,
    /// A "do not strike" event with a strike (an unclaimed pluck) within 80 ms.
    pub struck_anyway: Vec<bool>,
}

pub fn snap_to_plucks(nominal: &[f64], truths: &[Truth], plucks: &[f64]) -> Snapped {
    let n = nominal.len();
    let mut used = vec![false; plucks.len()];
    let mut times = nominal.to_vec();
    let mut found = vec![false; n];
    let mut last = f64::NEG_INFINITY;
    for i in 0..n {
        if truths[i] == Truth::None {
            continue;
        }
        let best = (0..plucks.len())
            .filter(|&j| !used[j] && (plucks[j] - nominal[i]).abs() <= 0.10 && plucks[j] > last + 0.05)
            .min_by(|&a, &b| (plucks[a] - nominal[i]).abs().total_cmp(&(plucks[b] - nominal[i]).abs()));
        let Some(mut j) = best else { continue };
        while j > 0 && !used[j - 1] && plucks[j] - plucks[j - 1] < 0.07 {
            j -= 1; // the strum's first string
        }
        times[i] = plucks[j];
        found[i] = true;
        last = plucks[j];
        let mut k = j;
        while k < plucks.len() && plucks[k] - plucks[j] < 0.07 {
            used[k] = true; // (a strum's later strings)
            k += 1;
        }
    }
    let struck_anyway = (0..n)
        .map(|i| {
            truths[i] == Truth::None
                && (0..plucks.len()).any(|j| !used[j] && (plucks[j] - nominal[i]).abs() <= 0.08)
        })
        .collect();
    Snapped { times, found, struck_anyway }
}

fn score_one(wav: &Path, labels: &Path, a: &ScoreArgs) -> anyhow::Result<(Vec<EvResult>, String)> {
    let stem = wav.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let (name, tone) = match stem.split_once('@') {
        Some((n, t)) => (n.to_string(), t.to_string()),
        None => (stem.clone(), "usual".to_string()),
    };
    let ev = read_labels(labels)?;
    let (x, rate) = crate::take_cmd::load_mono(wav)?;
    let x = if rate == RATE { x } else { chords_cmd::resample(&x, rate, RATE) };
    // plucks as the live detector finds them
    let mut det = qrs_audio::rt::onset::OnsetDetector::new(RATE);
    let mut plucks = Vec::new();
    for (k, c) in x.chunks(128).enumerate() {
        det.feed(c, (k * 128) as u64, |i, _| plucks.push(i as f64 / RATE as f64));
    }
    let strikes: Vec<f64> = ev.iter().filter(|e| e.truth != Truth::None).map(|e| e.t).collect();
    let (shift, matched) = match a.shift {
        Some(s) => (s, 0),
        None => find_shift(&strikes, &plucks).context("no strikes or no plucks to line up")?,
    };
    // each strike on the time it was really struck (nearest pluck within 100 ms of the tock): a person is rarely
    // within a sixteenth note of a click, and the strike beside it must not be taken for it
    let nominal: Vec<f64> = ev.iter().map(|e| e.t + shift).collect();
    let truths: Vec<Truth> = ev.iter().map(|e| e.truth).collect();
    let snap = snap_to_plucks(&nominal, &truths, &plucks);
    let mut order: Vec<usize> = (0..ev.len()).collect();
    order.sort_by(|&i, &j| snap.times[i].total_cmp(&snap.times[j]));
    let items: Vec<Item> = order
        .iter()
        .map(|&i| Item {
            t: snap.times[i],
            want: ev[i].want.clone(),
            play: None,
            judged: true,
            palm: false,
        })
        .collect();
    let arr = chords_cmd::chart(&items);
    let (judged, plucks_i) = chords_cmd::score(&arr, &x, None);
    if judged.len() != ev.len() {
        bail!("{}: {} chart groups for {} labelled events", name, judged.len(), ev.len());
    }
    let len = x.len() as f64 / RATE as f64;
    let mut out = Vec::new();
    let mut struck = 0;
    for (g, &i) in order.iter().enumerate() {
        let e = &ev[i];
        let t = snap.times[i];
        if t < 0.3 || t > len - 0.5 {
            continue; // not in the recording
        }
        if snap.struck_anyway[i] {
            struck += 1; // the card said not to strike and a strike is there: not a test of the game
            continue;
        }
        let accepted = !matches!(judged[g], Some(Judgement::Miss) | None);
        let found = snap.found[i];
        let wrong = (e.truth == Truth::Right) != accepted;
        let why = if a.why && wrong {
            chords_cmd::why(&arr, &x, &plucks_i, g)
        } else {
            String::new()
        };
        out.push(EvResult {
            rec: name.clone(),
            tone: tone.clone(),
            t,
            label: e.label.clone(),
            tag: e.tag.clone(),
            truth: e.truth,
            accepted,
            found,
            why,
        });
    }
    let note = format!(
        "{stem}: shift {shift:+.3} s{}, {} plucks, {} strikes{}",
        if a.shift.is_some() { String::new() } else { format!(" ({matched} of {} strikes lined up)", strikes.len()) },
        plucks.len(),
        strikes.len(),
        if struck > 0 {
            format!("; {struck} \"do NOT strike\" slots left out because a strike is there (you struck where the card said not to)")
        } else {
            String::new()
        }
    );
    Ok((out, note))
}

/// (right accepted, right, wrong-or-none accepted, wrong-or-none, right with a pluck found)
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Tally {
    pub right_ok: u32,
    pub right: u32,
    pub false_ok: u32,
    pub should_not: u32,
    pub found: u32,
}

impl Tally {
    pub fn add(&mut self, r: &EvResult) {
        if r.truth == Truth::Right {
            self.right += 1;
            self.right_ok += r.accepted as u32;
            self.found += r.found as u32;
        } else {
            self.should_not += 1;
            self.false_ok += r.accepted as u32;
        }
    }
    fn line(&self) -> String {
        let pc = |a: u32, b: u32| if b == 0 { "   -  ".to_string() } else { format!("{:5.1}%", 100.0 * a as f64 / b as f64) };
        format!(
            "right accepted {:>3}/{:<3} {}  (strike found {})   |   wrong accepted {:>3}/{:<3} {}",
            self.right_ok,
            self.right,
            pc(self.right_ok, self.right),
            pc(self.found, self.right),
            self.false_ok,
            self.should_not,
            pc(self.false_ok, self.should_not)
        )
    }
}

pub fn tally_by<'a>(rs: &'a [EvResult], key: impl Fn(&'a EvResult) -> String) -> Vec<(String, Tally)> {
    let mut m: std::collections::BTreeMap<String, Tally> = std::collections::BTreeMap::new();
    for r in rs {
        m.entry(key(r)).or_default().add(r);
    }
    m.into_iter().collect()
}

fn score_dir(a: &ScoreArgs) -> anyhow::Result<()> {
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(&a.dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            (n.ends_with(".wav") || n.ends_with(".flac") || n.ends_with(".mp3")) && !n.contains(".click.")
        })
        .collect();
    wavs.sort();
    if let Some(o) = &a.only {
        wavs.retain(|p| p.to_string_lossy().contains(o.as_str()));
    }
    if wavs.is_empty() {
        bail!("no recordings in {} (record NAME.wav for the exercises written by `corpus init`)", a.dir.display());
    }
    let mut all: Vec<EvResult> = Vec::new();
    for w in &wavs {
        let stem = w.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let name = stem.split('@').next().unwrap_or(stem);
        let labels = a.dir.join(format!("{name}.json"));
        if !labels.exists() {
            eprintln!("skipping {}: no {name}.json beside it", w.display());
            continue;
        }
        match score_one(w, &labels, a) {
            Ok((rs, note)) => {
                println!("{note}");
                let mut t = Tally::default();
                rs.iter().for_each(|r| t.add(r));
                println!("    {}", t.line());
                all.extend(rs);
            }
            Err(e) => eprintln!("skipping {}: {e:#}", w.display()),
        }
    }
    println!("\n== by kind of strike");
    for (k, t) in tally_by(&all, |r| r.tag.clone()) {
        println!("  {k:<18} {}", t.line());
    }
    println!("\n== by amp tone");
    for (k, t) in tally_by(&all, |r| r.tone.clone()) {
        println!("  {k:<18} {}", t.line());
    }
    let mut total = Tally::default();
    all.iter().for_each(|r| total.add(r));
    println!("\n== everything\n  {:<18} {}", "all", total.line());
    if a.why {
        println!("\n== what the game got wrong");
        for r in all.iter().filter(|r| (r.truth == Truth::Right) != r.accepted) {
            println!(
                "  {}{} {:>7.2} s {:<22} {}: {}",
                r.rec,
                if r.tone == "usual" { String::new() } else { format!("@{}", r.tone) },
                r.t,
                r.label,
                if r.truth == Truth::Right { "REFUSED (was right)" } else { "ACCEPTED (was not right)" },
                r.why
            );
        }
    }
    if let Some(p) = &a.json {
        let v: Vec<Value> = all
            .iter()
            .map(|r| {
                json!({"rec": r.rec, "tone": r.tone, "t": r.t, "label": r.label, "tag": r.tag,
                       "truth": r.truth.name(), "accepted": r.accepted, "found": r.found})
            })
            .collect();
        std::fs::write(p, serde_json::to_string_pretty(&json!({ "results": v }))?)?;
        println!("\nresults written to {}", p.display());
    }
    if let Some(p) = &a.baseline {
        let old: Value = serde_json::from_str(&std::fs::read_to_string(p)?)?;
        let mut before: Vec<EvResult> = Vec::new();
        for r in old["results"].as_array().context("no results in the baseline")? {
            before.push(EvResult {
                rec: r["rec"].as_str().unwrap_or("").into(),
                tone: r["tone"].as_str().unwrap_or("").into(),
                t: r["t"].as_f64().unwrap_or(0.0),
                label: r["label"].as_str().unwrap_or("").into(),
                tag: r["tag"].as_str().unwrap_or("").into(),
                truth: Truth::of(r["truth"].as_str().unwrap_or("")).unwrap_or(Truth::Right),
                accepted: r["accepted"].as_bool().unwrap_or(false),
                found: r["found"].as_bool().unwrap_or(false),
                why: String::new(),
            });
        }
        println!("\n== against the baseline ({})", p.display());
        let (b, n) = (tally_by(&before, |r| r.tag.clone()), tally_by(&all, |r| r.tag.clone()));
        for (k, t) in &n {
            let o = b.iter().find(|(bk, _)| bk == k).map(|x| x.1).unwrap_or_default();
            println!(
                "  {k:<18} right accepted {:+} ({} -> {} of {})   wrong accepted {:+} ({} -> {} of {})",
                t.right_ok as i64 - o.right_ok as i64,
                o.right_ok,
                t.right_ok,
                t.right,
                t.false_ok as i64 - o.false_ok as i64,
                o.false_ok,
                t.false_ok,
                t.should_not
            );
        }
    }
    Ok(())
}

// -- tests -------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chords_cmd::Strum;
    use qrs_audio::synth::Rng;

    /// The recording an exercise would give if played as labelled, `lead` seconds after the recording starts
    /// (the click's first beat), on the synth.
    fn play(ex: &Exercise, lead: f64, seed: u64) -> Vec<f32> {
        let mut r = chords_cmd::Rand(Rng::new(seed));
        let mut strums = Vec::new();
        for e in &ex.events {
            let notes = match &e.played {
                Played::AsCharted => e.want.clone(),
                Played::Shape(s) => s.clone(),
                Played::Muted => e.want.clone(),
                Played::Nothing => continue,
            };
            strums.push(Strum {
                at: lead + ex.t(e.beat),
                notes,
                spread: 0.03,
                palm: false,
                dead: e.played == Played::Muted,
            });
        }
        let len = lead + ex.t(ex.events.last().unwrap().beat) + 2.0;
        chords_cmd::render(&strums, len, &mut r, true)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs-corpus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_exercise_is_well_formed() {
        let exs = exercises();
        assert!(exs.len() >= 8);
        for ex in &exs {
            assert!(!ex.events.is_empty());
            for w in ex.events.windows(2) {
                // groups closer than 100 ms would merge in the chart
                assert!(ex.t(w[1].beat) - ex.t(w[0].beat) > 0.12, "{}: {} -> {}", ex.name, w[0].beat, w[1].beat);
            }
            for e in &ex.events {
                assert!(!e.want.is_empty());
                assert_eq!(e.truth == Truth::None, e.played == Played::Nothing, "{}", ex.name);
            }
            let v = labels_json(ex);
            assert_eq!(v["events"].as_array().unwrap().len(), ex.events.len());
        }
        let s = script(&exs);
        assert!(s.contains("do NOT strike") && s.contains("repeat-fast"));
        assert_eq!(beat_name(4.25), "1e");
        assert_eq!(beat_name(6.5), "3&");
        let click = click_track(95.0, &[2.0, 2.158], 5.0);
        assert!(click.iter().any(|v| v.abs() > 0.3) && click.len() == 5 * RATE as usize);
    }

    #[test]
    fn strikes_are_snapped_to_the_plucks_beside_their_tocks() {
        // tocks at 1.0 (right), 1.158 (right), 1.316 (do not); the player struck at 0.93, 1.25 (late) and
        // 1.33 on the third tock's slot
        let nominal = [1.0, 1.158, 1.316, 3.0];
        let truths = [Truth::Right, Truth::Right, Truth::None, Truth::None];
        let s = snap_to_plucks(&nominal, &truths, &[0.93, 1.25, 1.33]);
        assert_eq!(s.found, vec![true, true, false, false]);
        assert!((s.times[0] - 0.93).abs() < 1e-9 && (s.times[1] - 1.25).abs() < 1e-9);
        // 1.33 is the third slot's own pluck, but the second strike was the nearer to take 1.25 first, so 1.33 is
        // left over: the player struck where the card said not to
        assert_eq!(s.struck_anyway, vec![false, false, true, false]);
        // a strum's later strings are not another strike
        let s = snap_to_plucks(&[1.0, 1.3], &[Truth::Right, Truth::None], &[1.02, 1.05, 1.08]);
        assert!(s.found[0] && !s.struck_anyway[1], "{:?}", s.struck_anyway);
    }

    #[test]
    fn the_shift_is_found_whatever_the_lead() {
        let strikes = [4.0 * 0.6316, 4.0 * 0.6316 + 0.158, 4.0 * 0.6316 + 0.316, 4.0 * 0.6316 + 2.526];
        for lead in [0.4, 2.0, 3.7] {
            // a stray pluck (a touched string) first
            let mut plucks = vec![0.2];
            plucks.extend(strikes.iter().map(|t| t + lead + 0.012));
            let (s, n) = find_shift(&strikes, &plucks).unwrap();
            assert_eq!(n, 4);
            assert!((s - (lead + 0.012)).abs() < 0.006, "{lead}: {s}");
        }
    }

    #[test]
    fn a_recording_is_scored_against_its_labels() {
        // a short exercise: one chord struck three times, left ringing once, a wrong chord, a muted strum
        let ex = Exercise {
            name: "t",
            sub: 1,
            bpm: 95.0,
            about: "",
            events: vec![
                right(4.0, "D5", &D5, "first"),
                right(4.25, "D5", &D5, "repeat"),
                held(4.5, "D5", &D5),
                right(6.0, "A5", &A5, "first"),
                wrong(7.0, "E5", &E5, "wrong-shape", Played::Shape(sh(&D5)), String::new()),
                wrong(8.0, "A5", &A5, "muted", Played::Muted, String::new()),
                right(9.0, "E5", &E5, "first"),
            ],
        };
        let dir = tmp("score");
        std::fs::write(dir.join("t.json"), serde_json::to_string(&labels_json(&ex)).unwrap()).unwrap();
        qrs_analysis::take::write_wav_mono(&dir.join("t@crunch.wav"), RATE, &play(&ex, 3.7, 5)).unwrap();
        let a = ScoreArgs {
            dir: dir.clone(),
            why: false,
            json: Some(dir.join("out.json")),
            baseline: None,
            shift: None,
            only: None,
        };
        let (rs, note) = score_one(&dir.join("t@crunch.wav"), &dir.join("t.json"), &a).unwrap();
        assert!(note.contains("shift +3.") || note.contains("shift +4."), "{note}");
        assert_eq!(rs.len(), 7);
        assert!(rs.iter().all(|r| r.tone == "crunch"));
        let mut t = Tally::default();
        rs.iter().for_each(|r| t.add(r));
        assert_eq!((t.right, t.should_not), (4, 3), "{t:?}");
        assert!(t.right_ok >= 3, "{t:?} {rs:?}");
        assert!(t.false_ok <= 1, "{t:?} {rs:?}");
        score_dir(&a).unwrap();
        assert!(dir.join("out.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
