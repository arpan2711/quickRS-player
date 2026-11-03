//! `quickrs-cli chordname`: a bench for the note widget's chord names (new in quickRS, P24.4).
//!
//! The listener (`qrs_analysis::chordname`) is fed the input as the widget feeds it (a 170 ms frame every 50 ms)
//! and the chord it shows is read 0.35 s after each strum. Chords are the synth's strums (chords_cmd.rs `strummed`:
//! clean, sloppy, and overdriven by a soft clip) or a take of the owner's (its WAV and the .json beside it, chord by
//! chord against the chart's notes). The chord wanted is the name of the notes the shape plays (so a miss is the
//! hearing's, not the dictionary's); the shape guessed for it is checked against the one played. Nothing is written.

use std::path::PathBuf;

use qrs_analysis::chordname::{self, Current, HeardNote, Listener};

#[derive(clap::Args)]
pub struct Args {
    /// a take (WAV, its .json beside it) to name chord by chord, instead of the synth's strums
    #[arg(long)]
    pub take: Option<PathBuf>,
    /// print every chord, not just the misses
    #[arg(long)]
    pub all: bool,
    #[arg(long, default_value_t = 1)]
    pub seed: u64,
}

const OPENS: [i32; 6] = [40, 45, 50, 55, 59, 64];
const HOP: f64 = 0.05;
/// When the chord shown is read, after the strum.
const LOOK: f64 = 0.35;

/// The synth's chords: open, barre, power, sevenths, sus and slash shapes (low string first, x = muted).
fn shapes() -> Vec<Vec<i32>> {
    [
        "x32010", "320003", "xx0232", "022000", "x02210", "022100", "x02220", "xx0231", "x02020", "020100",
        "x32000", "xx0233", "x02200", "200232", "x20003", "x02010", "xx0212", "133211", "x24432", "355433",
        "x35553", "577655", "x57765", "02xxxx", "355xxx", "x022xx", "577xxx", "x799xx", "x24232", "x35343",
    ]
    .iter()
    .map(|s| {
        s.chars()
            .map(|c| c.to_digit(10).map_or(-1, |d| d as i32))
            .collect()
    })
    .collect()
}

/// The name of exactly these notes.
fn ideal(midis: &[i32]) -> String {
    let heard: Vec<HeardNote> = midis
        .iter()
        .map(|&midi| HeardNote { midi, strength: 1.0 })
        .collect();
    chordname::name(&heard).map_or("-".into(), |g| g.name())
}

/// The listener over `x`, read at each of `checks` (seconds, ascending): the chord shown then.
fn listen(x: &[f32], rate: u32, checks: &[f64]) -> Vec<Option<Current>> {
    let mut l = Listener::default();
    let n = Listener::frame_len(rate);
    let mut out = Vec::new();
    let mut t = n as f64 / rate as f64;
    while out.len() < checks.len() {
        let end = (t * rate as f64) as usize;
        if end > x.len() {
            break;
        }
        l.feed(&x[end - n..end], rate, 40, t);
        while out.len() < checks.len() && checks[out.len()] <= t + 1e-9 {
            out.push(l.current.clone());
        }
        t += HOP;
    }
    out.resize(checks.len(), None);
    out
}

#[derive(Default)]
struct Tally {
    n: usize,
    named: usize,
    root: usize,
    shape: usize,
}

/// The root of a chord's name ("F#m7/A" -> "F#").
fn root(s: &str) -> &str {
    let n = s
        .char_indices()
        .nth(1)
        .filter(|(_, c)| *c == '#' || *c == 'b')
        .map_or(1, |(i, _)| i + 1);
    &s[..n.min(s.len())]
}

fn tab(s: &[i32]) -> String {
    s.iter()
        .map(|&f| if f < 0 { "x".into() } else { f.to_string() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Each chord wanted (its name and shape) against what was shown; prints the misses (or all).
fn report(wanted: &[(String, Vec<i32>)], got: &[Option<Current>], all: bool) -> Tally {
    let mut t = Tally::default();
    for ((want, frets), g) in wanted.iter().zip(got) {
        t.n += 1;
        let name = g.as_ref().map_or("-".to_string(), |c| c.guess.name());
        let ok = name == *want;
        t.named += ok as usize;
        t.root += (root(&name) == root(want)) as usize;
        let shape = g
            .as_ref()
            .and_then(|c| chordname::shape(&c.guess, &c.heard, &OPENS, None));
        t.shape += (ok && shape.as_ref() == Some(frets)) as usize;
        if all || !ok {
            let heard = g.as_ref().map_or(String::new(), |c| {
                c.heard
                    .iter()
                    .map(|h| {
                        format!(
                            "{}({:.2})",
                            qrs_analysis::pitch::note_label(h.midi as f64),
                            h.strength
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            println!(
                "  {} want {want:<7} ({})  got {name:<7} shape {:<12} heard {heard}",
                if ok { "ok  " } else { "MISS" },
                tab(frets),
                shape.map_or("-".into(), |s| tab(&s)),
            );
        }
    }
    t
}

fn line(label: &str, t: &Tally) {
    let pc = |a: usize| 100.0 * a as f64 / t.n.max(1) as f64;
    println!(
        "{label:<24} {:>4} chords  named {:>5.1}%  root {:>5.1}%  shape {:>5.1}%",
        t.n,
        pc(t.named),
        pc(t.root),
        pc(t.shape)
    );
}

pub fn run(args: &Args) -> anyhow::Result<()> {
    if let Some(p) = &args.take {
        return run_take(p, args);
    }
    let sh = shapes();
    let mut lines = Vec::new();
    for (case, sloppy, drive) in [
        ("clean", false, 0.0f32),
        ("sloppy", true, 0.0),
        ("overdriven", false, 6.0),
    ] {
        println!("{case}:");
        let (mut x, at) = crate::chords_cmd::strummed(&sh, 2.0, sloppy, args.seed);
        if drive > 0.0 {
            let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
            x.iter_mut().for_each(|v| *v = (*v * drive / peak).tanh() * 0.7);
        }
        let checks: Vec<f64> = at.iter().map(|(t, _)| t + LOOK).collect();
        let got = listen(&x, 48000, &checks);
        // what the first strum struck
        let w: Vec<(String, Vec<i32>)> = at
            .iter()
            .map(|(_, struck)| {
                let mut frets = vec![-1; 6];
                for &(s, f) in struck {
                    frets[s as usize] = f;
                }
                let m: Vec<i32> = struck.iter().map(|&(s, f)| OPENS[s as usize] + f).collect();
                (ideal(&m), frets)
            })
            .collect();
        lines.push((case, report(&w, &got, args.all)));
    }
    for (case, t) in &lines {
        line(case, t);
    }
    Ok(())
}

/// "G2", "C#4", "Eb3" as MIDI.
fn parse_note(s: &str) -> Option<i32> {
    let mut c = s.chars();
    let pc = match c.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = c.collect();
    let (acc, oct) = match rest.chars().next()? {
        '#' => (1, &rest[1..]),
        'b' => (-1, &rest[1..]),
        _ => (0, &rest[..]),
    };
    Some(pc + acc + 12 * (oct.parse::<i32>().ok()? + 1))
}

/// A take of the owner's: each chord of the chart in it, named from the take's audio.
fn run_take(path: &std::path::Path, args: &Args) -> anyhow::Result<()> {
    let (x, rate) = crate::take_cmd::load_mono(path)?;
    let side: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path.with_extension("json"))?)?;
    let timing = side
        .get("timing")
        .and_then(qrs_analysis::take::TakeTiming::from_json)
        .ok_or_else(|| anyhow::anyhow!("no timing in the take's .json"))?;
    let t0 = timing.chart_time(0.0);
    let mut checks = Vec::new();
    let mut wanted = Vec::new();
    for e in side["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["kind"] == "chord")
    {
        // (when it was played, if it was)
        let t = e["t"].as_f64().unwrap_or(0.0) - t0 + e["offset_ms"].as_f64().unwrap_or(0.0) / 1000.0;
        let mut frets = vec![-1; 6];
        let mut midis = Vec::new();
        for n in e["expected"].as_array().into_iter().flatten() {
            if let (Some(s), Some(f), Some(m)) = (
                n["string"].as_u64(),
                n["fret"].as_i64(),
                n["note"].as_str().and_then(parse_note),
            ) {
                frets[(s as usize).min(5)] = f as i32;
                midis.push(m);
            }
        }
        if t > 0.0 && (t + LOOK) * (rate as f64) < x.len() as f64 {
            checks.push(t + LOOK);
            wanted.push((ideal(&midis), frets));
        }
    }
    let got = listen(&x, rate, &checks);
    let t = report(&wanted, &got, args.all);
    let label = path.file_stem().and_then(|s| s.to_str()).unwrap_or("take");
    line(&label[label.len().saturating_sub(24)..], &t);
    Ok(())
}
