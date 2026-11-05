//! `quickrs-cli board idle` (P38.2): what every starting board, the listening set's chains and the owner's NAM
//! captures put out while nobody plays. Three inputs, 3 s each (the first 0.5 s not counted: the gate and the
//! filters settle):
//! - **silence**: digital zero. Anything out is the board's own noise (self-noise, denormal crackle, DC, an
//!   oscillation or limit cycle); it should be nothing.
//! - **interface**: -90 dBFS of white noise and a -100 dBFS 50 Hz hum, an audio interface's floor with nothing
//!   plugged in.
//! - **single coil** / **humbucker**: a guitar plugged in, volume up, not played, at the owner's input level (strums
//!   peak near -12 dBFS after the input trim): the pickup's 50 Hz buzz with its harmonics (-70 dBFS for single coils,
//!   -88 for humbuckers, which cancel it) and the guitar's and interface's hiss (-84 dBFS, rolled off above 6 kHz).
//!   Estimates of a typical guitar; a real one near a monitor or a dimmer buzzes more.
//!
//! For each: the idle output (dBFS RMS) and how much louder than the input it is (dB). Then "after a note": a
//! chug, then 3 s of silence: the tail's peak at the end (a limit cycle or a stuck filter stays up) and whether the
//! time per block climbs (denormals). Writes `idle.md` to `--out` (default `board-idle`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use qrs_analysis::testsig;
use qrs_audio::board::{self, Board};

const RATE: u32 = 48000;

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-10).log10()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt() as f32
}

/// White noise at `noise_db` dBFS RMS, low-passed at `lp` Hz if given, and a 50 Hz buzz at `hum_db` dBFS RMS with
/// `harmonics` harmonics falling 3 dB each (a pickup's buzz is rich; an interface's hum is nearly a sine).
pub fn idle_input(
    secs: f32,
    noise_db: f32,
    lp: Option<f32>,
    hum_db: f32,
    harmonics: usize,
    seed: u32,
) -> Vec<f32> {
    let n = (RATE as f32 * secs) as usize;
    let mut x = testsig::hiss(n, 10f32.powf(noise_db / 20.0), seed);
    if let Some(f) = lp {
        let a = (-std::f32::consts::TAU * f / RATE as f32).exp();
        let mut z = 0.0f32;
        for v in x.iter_mut() {
            z = (1.0 - a) * *v + a * z;
            *v = z;
        }
        // back to the asked level after the roll-off
        let g = 10f32.powf(noise_db / 20.0) / rms(&x).max(1e-12);
        for v in x.iter_mut() {
            *v *= g;
        }
    }
    let mut hum = vec![0.0f32; n];
    for h in 1..=harmonics.max(1) {
        let a = 10f32.powf(-3.0 * (h - 1) as f32 / 20.0);
        for (i, v) in hum.iter_mut().enumerate() {
            *v += a * (std::f32::consts::TAU * 50.0 * h as f32 * i as f32 / RATE as f32).sin();
        }
    }
    let g = 10f32.powf(hum_db / 20.0) / rms(&hum).max(1e-12);
    for (v, h) in x.iter_mut().zip(&hum) {
        *v += h * g;
    }
    x
}

/// What a board puts out of `x`, after the first half second: (RMS dBFS, peak, |mean|).
fn idle_out(board: &Board, x: &[f32]) -> (f32, f32, f32) {
    let y = board::render(board, RATE, x);
    let tail = &y[RATE as usize / 2..];
    let peak = tail.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let mean = (tail.iter().map(|v| *v as f64).sum::<f64>() / tail.len().max(1) as f64).abs() as f32;
    (db(rms(tail)), peak, mean)
}

pub struct Inputs {
    pub silence: Vec<f32>,
    pub interface: Vec<f32>,
    pub single: Vec<f32>,
    pub humbucker: Vec<f32>,
}

pub fn inputs() -> Inputs {
    Inputs {
        silence: vec![0.0; RATE as usize * 3],
        interface: idle_input(3.0, -90.0, None, -100.0, 1, 3),
        single: idle_input(3.0, -84.0, Some(6000.0), -70.0, 12, 5),
        humbucker: idle_input(3.0, -84.0, Some(6000.0), -88.0, 12, 7),
    }
}

/// One board's row: silence out, then (out dBFS, gain dB) for the interface, single coil and humbucker inputs, the
/// tail after a note, and a verdict.
pub fn row(board: &Board, ins: &Inputs, note: &[f32]) -> (String, f32) {
    let (s_db, s_peak, s_dc) = idle_out(board, &ins.silence);
    let mut cells = vec![if s_peak == 0.0 {
        "0 (none)".to_string()
    } else {
        format!("{s_db:.0} (peak {:.0})", db(s_peak))
    }];
    let mut worst_single = -200.0f32;
    for (k, x) in [&ins.interface, &ins.single, &ins.humbucker].iter().enumerate() {
        let (o, _, _) = idle_out(board, x);
        let g = o - db(rms(&x[RATE as usize / 2..]));
        if k == 1 {
            worst_single = o;
        }
        cells.push(format!("{o:.0} / {g:+.0}"));
    }
    // after a note: the silence that follows, timed
    let mut p = board::make_board(board, RATE);
    let mut y = note.to_vec();
    for c in y.chunks_mut(128) {
        p.process(c);
    }
    let t0 = Instant::now();
    let mut quiet = vec![0.0f32; RATE as usize * 3];
    let mut first = 0.0f64;
    for (i, c) in quiet.chunks_mut(128).enumerate() {
        p.process(c);
        if i == 20 {
            first = t0.elapsed().as_secs_f64();
        }
    }
    let total = t0.elapsed().as_secs_f64();
    let blocks = quiet.len() / 128;
    let per_first = first / 21.0;
    let per_rest = (total - first) / (blocks - 21) as f64;
    let tail = &quiet[quiet.len() - RATE as usize / 2..];
    let tail_peak = tail.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let nan = quiet.iter().any(|v| !v.is_finite());
    cells.push(format!("{:.0}", db(tail_peak)));
    let mut why = Vec::new();
    if s_peak > 1e-7 {
        why.push("self-noise");
    }
    if s_dc > 1e-4 {
        why.push("DC");
    }
    if nan {
        why.push("NaN");
    }
    if per_rest > per_first * 3.0 && per_rest > 20e-6 {
        why.push("slow in silence (denormals?)");
    }
    // (a reverb or delay tail can run 3 s; a stuck one or a limit cycle stays well up)
    if tail_peak > 1e-3 {
        why.push("tail stays up");
    }
    if worst_single > -50.0 {
        why.push("loud hiss/hum idle: gate");
    } else if worst_single > -60.0 {
        why.push("audible idle noise");
    }
    cells.push(if why.is_empty() {
        "clean".into()
    } else {
        why.join(", ")
    });
    (cells.join(" | "), worst_single)
}

pub fn run(out: Option<&Path>, only: Option<&str>, captures: Option<PathBuf>) -> anyhow::Result<()> {
    let out = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("board-idle"));
    std::fs::create_dir_all(&out)?;
    let ins = inputs();
    let note = crate::board_cmd::dry(&crate::board_cmd::chug(), false);
    let mut md = String::new();
    let _ = writeln!(
        md,
        "# Idle noise (P38.2)\n\nInputs: silence (digital zero); interface (-90 dBFS white + -100 dBFS 50 Hz hum); single \
         coil (-70 dBFS 50 Hz buzz with harmonics + -84 dBFS hiss); humbucker (the buzz at -88). Cells: idle out dBFS RMS \
         / dB louder than the input. Tail: peak dBFS of the last 0.5 s of 3 s of silence after a chug.\n"
    );
    let header = "| chain | silence out | interface | single coil | humbucker | tail after a note | verdict |\n|---|---|---|---|---|---|---|";
    let mut chains = crate::listen_cmd::chains();
    let cap_dir = captures.or_else(|| {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("quickRS").join("data").join("captures"))
    });
    let low_dir = cap_dir.filter(|d| d.join("captures.json").exists());
    if let Some(dir) = &low_dir {
        chains.extend(crate::listen_cmd::nam_chains(dir).unwrap_or_default());
    }
    let mut group = "";
    for c in &chains {
        if only.is_some_and(|o| !c.name.contains(o)) {
            continue;
        }
        if c.group != group {
            group = c.group;
            let _ = writeln!(md, "\n## {group}\n\n{header}");
        }
        let (r, _) = row(&c.board, &ins, &note);
        let _ = writeln!(md, "| {} | {r} |", c.name);
        eprintln!("{} {}", c.name, r);
    }
    if let Some(dir) = low_dir {
        low_end(&mut md, &dir, only);
    }
    std::fs::write(out.join("idle.md"), &md)?;
    print!("{md}");
    println!("-> {}", out.join("idle.md").display());
    Ok(())
}

/// The low end of every amp capture in the library (P38.2, the "muddy" Dual Rec): the share of a palm-muted chug's
/// energy under 100 Hz out of the capture alone and into our 4x12, beside our model it stands in for at the same gain.
/// A capture with its cab in it (or one that sounds like it) shows its low end alone.
fn low_end(md: &mut String, dir: &Path, only: Option<&str>) {
    use crate::listen_cmd::{band, spectrum};
    use qrs_audio::board::{Block, nam};
    let Ok(text) = std::fs::read_to_string(dir.join("captures.json")) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let x = crate::board_cmd::dry(&crate::board_cmd::chug(), false);
    let low = |b: &Board| {
        let y = board::render(b, RATE, &x);
        100.0 * band(&spectrum(&y), 0.0, 100.0)
    };
    let _ = writeln!(
        md,
        "\n## Low end of the amp captures (share of the chug's energy under 100 Hz, %; the dry chug: {:.0})\n\n| capture \
         | cab in it | alone | into our 4x12 | our model alone | our model + 4x12 |\n|---|---|---|---|---|---|",
        100.0 * band(&spectrum(&x), 0.0, 100.0)
    );
    for it in v.get("items").and_then(|i| i.as_array()).into_iter().flatten() {
        let s = |k: &str| it.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        if s("kind") != "nam" || it.get("bass").and_then(|b| b.as_bool()) == Some(true) {
            continue;
        }
        let name = s("name");
        if only.is_some_and(|o| !name.to_lowercase().contains(&o.to_lowercase())) {
            continue;
        }
        let id = it.get("id").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
        let Ok(json) = std::fs::read_to_string(dir.join(s("file"))) else {
            continue;
        };
        if nam::register_nam(id, &json).is_err() {
            continue;
        }
        nam::set_trim(id, it.get("trim").and_then(|t| t.as_f64()).unwrap_or(0.0) as f32);
        let mut a = Block::of("amp_nam").expect("amp_nam");
        a.asset = id;
        let cab = || Block::of("cab_4x12").expect("cab");
        let gain = it.get("gain").and_then(|g| g.as_f64()).unwrap_or(5.0) as f32;
        let skin = s("skin");
        let ours = Block::of(if skin.is_empty() { "amp_lead" } else { &skin })
            .unwrap_or_else(|| Block::of("amp_lead").expect("amp"))
            .with("gain", gain);
        let _ = writeln!(
            md,
            "| {name} | {} | {:.0} | {:.0} | {:.0} ({skin} {gain:.1}) | {:.0} |",
            match it.get("cab").and_then(|c| c.as_bool()) {
                Some(true) => "yes",
                Some(false) => "no",
                None => "?",
            },
            low(&Board::new(vec![a])),
            low(&Board::new(vec![a, cab()])),
            low(&Board::new(vec![ours])),
            low(&Board::new(vec![ours, cab()])),
        );
        nam::unregister(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listen_cmd::{band, spectrum};
    use qrs_audio::board::Block;

    /// The share of the chug's energy under 100 Hz out of an amp of ours alone at a gain (%), as `low_end` measures
    /// the owner's captures.
    fn low(id: &str, gain: f32) -> f64 {
        let x = crate::board_cmd::dry(&crate::board_cmd::chug(), false);
        let m = Block::of(id).unwrap();
        let key = qrs_audio::board::loudness::gain_key(m.def());
        let y = board::render(&Board::new(vec![m.with(key, gain)]), RATE, &x);
        100.0 * band(&spectrum(&y), 0.0, 100.0)
    }

    /// One amp at a gain: the idle noise over a single coil's (dB).
    fn idle_gain(id: &str, gain: f32) -> f32 {
        let ins = inputs();
        let m = Block::of(id).unwrap();
        let key = qrs_audio::board::loudness::gain_key(m.def());
        let (o, _, _) = idle_out(&Board::new(vec![m.with(key, gain)]), &ins.interface);
        o - db(rms(&ins.interface[RATE as usize / 2..]))
    }

    /// P41.2 (D166): the low end of our high-gain and tweed models sits where the owner's captures of the same amps
    /// do (idle.md's last table: Dual Rec / Recto 10-20%, JCM800 2-7%, 5E3 / Bassman 15-30%), and the new voices
    /// sit where theirs do (Mark 3-11%, SLO / 5150 1-7%, Orange 3-6%).
    #[test]
    fn our_amps_low_end_is_near_the_captures() {
        let want: &[(&str, &[f32], f64, f64)] = &[
            ("amp_modern", &[6.5, 8.5], 9.0, 16.0),
            ("amp_lead", &[6.0, 9.0], 2.5, 7.0),
            ("amp_tweed", &[3.0, 5.0, 8.0], 14.0, 22.0),
            ("amp_cali", &[5.0, 8.0], 2.5, 11.0),
            ("amp_snake", &[5.0, 8.5], 1.0, 7.0),
            ("amp_tangerine", &[5.0, 8.0], 2.5, 8.0),
        ];
        let mut bad = Vec::new();
        for (id, gains, lo, hi) in want {
            for g in *gains {
                let l = low(id, *g);
                eprintln!("{id} {g}: {l:.1}% under 100 Hz");
                if l < *lo || l > *hi {
                    bad.push(format!("{id} {g}: {l:.1}% (want {lo}-{hi})"));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:?}");
    }

    /// P41.2 (D166): a Rectifier turned down is a quiet amp, as the real one's clean side is (the NAM Recto clean
    /// capture adds 3-8 dB of idle noise; ours added 48 at gain 3).
    #[test]
    fn the_rectifier_turned_down_is_quiet() {
        let g = idle_gain("amp_modern", 3.0);
        eprintln!("amp_modern 3: idle {g:+.1} dB");
        assert!(g < 30.0, "{g}");
    }

    /// The acoustic sim on strummed chords: (under 150 Hz, 200-300 Hz, 1-5 kHz shares %, centroid Hz).
    fn acoustic_shape(knobs: &[(&str, f32)]) -> (f64, f64, f64, f64) {
        let x = crate::board_cmd::dry(&crate::board_cmd::chords(), false);
        let mut a = Block::of("acoustic").unwrap();
        for (k, v) in knobs {
            a.set(k, *v);
        }
        let y = board::render(&Board::new(vec![a]), RATE, &x);
        let sp = spectrum(&y);
        let bin = RATE as f64 / 8192.0;
        let tot: f64 = sp.iter().sum();
        let c = sp
            .iter()
            .enumerate()
            .map(|(k, v)| k as f64 * bin * v)
            .sum::<f64>()
            / tot;
        (
            100.0 * band(&sp, 0.0, 150.0),
            100.0 * band(&sp, 200.0, 300.0),
            100.0 * band(&sp, 1000.0, 5000.0),
            c,
        )
    }

    /// P41.2 (D168, the guitar tech's #9): the acoustic sim at its defaults is brighter than the strummed electric
    /// it starts from, not boomier: less under 150 Hz than it was (35%), the 200-300 Hz low mids cut, the 1-5 kHz
    /// detail lifted (it was 3.5%).
    #[test]
    fn the_acoustic_sim_is_bright_not_boomy() {
        let x = crate::board_cmd::dry(&crate::board_cmd::chords(), false);
        let sp = spectrum(&x);
        let (low_mid_in, detail_in) = (100.0 * band(&sp, 200.0, 300.0), 100.0 * band(&sp, 1000.0, 5000.0));
        let (boom, low_mid, detail, centroid) = acoustic_shape(&[]);
        eprintln!(
            "acoustic: {boom:.1}% <150, {low_mid:.1}% 200-300, {detail:.1}% 1-5k, centroid {centroid:.0} Hz"
        );
        assert!(boom < 22.0, "{boom}");
        assert!(low_mid < low_mid_in * 0.75, "{low_mid} vs {low_mid_in}");
        assert!(detail > detail_in * 2.5, "{detail} vs {detail_in}");
        assert!(centroid > 500.0, "{centroid}");
    }

    #[test]
    #[ignore]
    fn print_acoustic() {
        let x = crate::board_cmd::dry(&crate::board_cmd::chords(), false);
        let sp = spectrum(&x);
        eprintln!(
            "dry: {:.1}% <150, {:.1}% 200-300, {:.1}% 1-5k",
            100.0 * band(&sp, 0.0, 150.0),
            100.0 * band(&sp, 200.0, 300.0),
            100.0 * band(&sp, 1000.0, 5000.0)
        );
        for body in [0.0, 6.0, 10.0] {
            let (a, b, c, d) = acoustic_shape(&[("body", body)]);
            eprintln!(
                "acoustic body {body}: {a:.1}% <150, {b:.1}% 200-300, {c:.1}% 1-5k, centroid {d:.0} Hz"
            );
        }
    }

    #[test]
    #[ignore]
    fn print_low_ends() {
        for id in [
            "amp_clean",
            "amp_tweed",
            "amp_chime",
            "amp_crunch",
            "amp_lead",
            "amp_modern",
            "amp_cali",
            "amp_snake",
            "amp_tangerine",
            "amp_surrey",
        ] {
            let row: Vec<String> = [2.0, 3.0, 5.0, 6.5, 8.5]
                .iter()
                .map(|g| format!("{:.1}% / {:+.0} dB", low(id, *g), idle_gain(id, *g)))
                .collect();
            eprintln!("{id}: {}", row.join("  "));
        }
    }
}
