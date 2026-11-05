//! `quickrs --nam-check DIR [N]`: the NAM picker checked end to end on the owner's library (P33.7, D82), with no
//! window. N songs spread across the library (default 60): each tone of each part becomes its board twice, ours
//! (`rs_gear::board_for`) and the NAM one (`captures::auto::nam_board`, what a song opens on), and a riff (a
//! guitar's or a bass's, at our input's level: peaks near -12 dBFS) is played through each by the board the engine
//! runs (`BoardProcessor`). Written to DIR: report.md (what was picked for each piece of gear, the level, peak and
//! brightness of both boards, CPU per board, and the tones that stand out: clipping, silence, far louder or
//! quieter than ours), picks.csv, and WAVs of the first tones (ours and NAM, for listening).

use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use qrs_audio::board::{Board, BoardProcessor, Kind, rs_gear};
use qrs_audio::rt::ToneProcessor;
use qrs_formats::Package;

const RATE: u32 = 48000;

/// Plucked notes (a decaying saw and sine), at about a guitar's level after our input trim.
fn riff(bass: bool) -> Vec<f32> {
    let notes: [i32; 8] = if bass {
        [28, 28, 35, 33, 31, 31, 26, 28]
    } else {
        [40, 47, 52, 55, 52, 47, 43, 45]
    };
    let step = RATE as usize / 4;
    let mut out = vec![0.0f32; step * notes.len() + RATE as usize];
    for (k, &m) in notes.iter().enumerate() {
        let f = 440.0 * 2f32.powf((m as f32 - 69.0) / 12.0);
        for i in 0..RATE as usize {
            let t = i as f32 / RATE as f32;
            let env = (-t * 3.0).exp() * 0.25;
            let ph = (t * f).fract();
            out[k * step + i] += env * (ph * 2.0 - 1.0 + 0.5 * (std::f32::consts::TAU * f * t).sin());
        }
    }
    out
}

struct Heard {
    rms_db: f32,
    peak_db: f32,
    /// The share of the energy above 2 kHz (dB under the total): brighter is nearer 0.
    bright_db: f32,
    /// CPU: seconds of work per second of audio.
    cpu: f32,
    audio: Vec<f32>,
}

fn hear(board: &Board, x: &[f32]) -> Heard {
    let mut p = BoardProcessor::new(board, RATE);
    let mut y = x.to_vec();
    let t = Instant::now();
    for c in y.chunks_mut(256) {
        p.process(c);
    }
    let cpu = t.elapsed().as_secs_f32() / (x.len() as f32 / RATE as f32);
    let e = y.iter().map(|v| v * v).sum::<f32>() / y.len().max(1) as f32;
    let peak = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    // a one-pole high-pass at 2 kHz for the bright share
    let k = (-std::f32::consts::TAU * 2000.0 / RATE as f32).exp();
    let (mut lp, mut eh) = (0.0f32, 0.0f32);
    for v in &y {
        lp = k * lp + (1.0 - k) * v;
        eh += (v - lp) * (v - lp);
    }
    let db = |v: f32| 10.0 * v.max(1e-12).log10();
    Heard {
        rms_db: db(e),
        peak_db: 20.0 * peak.max(1e-6).log10(),
        bright_db: db(eh / y.len().max(1) as f32) - db(e),
        cpu,
        audio: y,
    }
}

fn wav(path: &Path, y: &[f32]) {
    let pcm: Vec<i16> = y.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
    let _ = qrs_audio::decode::write_wav_i16(path, RATE, 1, &pcm);
}

pub fn run(out: &Path, n: usize) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let data = qrs_library::paths::DataDir::locate();
    let config = qrs_library::Config::load(data.config());
    let exe_dir: Vec<std::path::PathBuf> = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .into_iter()
        .collect();
    let (keys, _) = crate::keyfile::load(&crate::keyfile::candidates(&data.root, &exe_dir));
    crate::captures::init_quiet(&data.root, &config);
    crate::captures::ready();
    let lib_n = crate::captures::with(|l| l.items.len());
    let mut songs: Vec<qrs_library::scan::SongEntry> = qrs_library::scan::load_index(&data.index())
        .into_values()
        .flatten()
        .filter(|e| e.error.is_empty() && !e.needs_keys)
        .collect();
    songs.sort_by(|a, b| a.artist.cmp(&b.artist).then(a.title.cmp(&b.title)));
    songs.dedup_by(|a, b| a.path == b.path && a.song_key == b.song_key);
    let step = (songs.len() / n.max(1)).max(1);
    let picked: Vec<&qrs_library::scan::SongEntry> = songs.iter().step_by(step).take(n).collect();
    println!(
        "nam-check: {} captures in the library, {} songs of {}",
        lib_n,
        picked.len(),
        songs.len()
    );
    let mut md = String::new();
    let mut csv = String::from("song,part,tone,gear key,our model,capture\n");
    let _ = writeln!(
        md,
        "# NAM check\n\n{} captures; {} songs of {}.\n",
        lib_n,
        picked.len(),
        songs.len()
    );
    let _ = writeln!(
        md,
        "| song | part | tone | ours dB | NAM dB | NAM peak | bright ours / NAM | CPU NAM | captures |\n|---|---|---|---|---|---|---|---|---|"
    );
    let (mut tones, mut swapped, mut outliers) = (0usize, 0usize, Vec::new());
    let (mut cpu_max, mut cpu_sum) = (0.0f32, 0.0f32);
    let mut level_diffs: Vec<f32> = Vec::new();
    let mut wavs = 0;
    let gtr = riff(false);
    let bass_riff = riff(true);
    for e in picked {
        let key = (!e.song_key.is_empty()).then_some(e.song_key.as_str());
        let Ok(p) = Package::open(&e.path, &keys, key) else {
            continue;
        };
        let mut seen = std::collections::HashSet::new();
        for a in &p.arrangements {
            for t in &a.rig {
                if !seen.insert(t.key.clone()) {
                    continue;
                }
                tones += 1;
                let ours = rs_gear::board_for(t);
                let nam = crate::captures::with(|l| crate::captures::auto::nam_board(t, l));
                let bass = rs_gear::is_bass_tone(t);
                let x = if bass { &bass_riff } else { &gtr };
                let ho = hear(&ours, x);
                let hn = hear(&nam, x);
                let (_, gear_keys) = rs_gear::board_keyed(t);
                let caps: Vec<String> = nam
                    .blocks
                    .iter()
                    .filter(|b| b.def().file)
                    .map(|b| crate::captures::name_of(b.asset).unwrap_or_else(|| format!("#{}", b.asset)))
                    .collect();
                if !caps.is_empty() {
                    swapped += 1;
                }
                // which gear became which capture (by position: the NAM board keeps our board's order, minus a cab)
                for (b, k) in ours.blocks.iter().zip(gear_keys.iter()) {
                    if matches!(b.def().kind, Kind::Amp | Kind::Cab | Kind::Drive | Kind::Fuzz) {
                        let cap = crate::captures::with(|l| {
                            crate::captures::auto::pick(l, b, k.as_deref(), bass).map(|i| i.name.clone())
                        });
                        let _ = writeln!(
                            csv,
                            "\"{}\",{},{},{},{},\"{}\"",
                            e.title.replace('"', "'"),
                            a.name,
                            t.key,
                            k.as_deref().unwrap_or("-"),
                            b.def().id,
                            cap.unwrap_or_else(|| "(ours)".into())
                        );
                    }
                }
                cpu_max = cpu_max.max(hn.cpu);
                cpu_sum += hn.cpu;
                let diff = hn.rms_db - ho.rms_db;
                level_diffs.push(diff);
                let mut why = Vec::new();
                if hn.peak_db > -0.3 {
                    why.push("clips");
                }
                if hn.rms_db < -45.0 {
                    why.push("near silent");
                }
                if diff.abs() > 8.0 {
                    why.push(if diff > 0.0 {
                        "much louder than ours"
                    } else {
                        "much quieter than ours"
                    });
                }
                if hn.cpu > 0.5 {
                    why.push("heavy on the CPU");
                }
                let name = format!("{} - {}", e.artist, e.title);
                if !why.is_empty() {
                    outliers.push(format!("{name} / {} / {}: {}", a.name, t.key, why.join(", ")));
                }
                let _ = writeln!(
                    md,
                    "| {} | {} | {} | {:.1} | {:.1} | {:.1} | {:.1} / {:.1} | {:.0}% | {} |",
                    name.replace('|', "/"),
                    a.name,
                    t.key,
                    ho.rms_db,
                    hn.rms_db,
                    hn.peak_db,
                    ho.bright_db,
                    hn.bright_db,
                    hn.cpu * 100.0,
                    if caps.is_empty() {
                        "(none: ours)".into()
                    } else {
                        caps.join(" + ")
                    }
                );
                if wavs < 24 && !caps.is_empty() {
                    wavs += 1;
                    let base = format!(
                        "{wavs:02} {}",
                        crate::captures::safe_name(&format!("{name} {} {}", a.name, t.key))
                    );
                    wav(&out.join(format!("{base} ours.wav")), &ho.audio);
                    wav(&out.join(format!("{base} NAM.wav")), &hn.audio);
                }
            }
        }
    }
    level_diffs.sort_by(f32::total_cmp);
    let med = level_diffs.get(level_diffs.len() / 2).copied().unwrap_or(0.0);
    let mut head = String::new();
    let _ = writeln!(
        head,
        "## Summary\n\n- tones: {tones}, with captures: {swapped}\n- NAM level minus ours: median {med:+.1} dB, \
         range {:+.1} to {:+.1} dB\n- CPU per NAM board: mean {:.0}%, max {:.0}% of one core\n- stand out: {}\n",
        level_diffs.first().copied().unwrap_or(0.0),
        level_diffs.last().copied().unwrap_or(0.0),
        cpu_sum / tones.max(1) as f32 * 100.0,
        cpu_max * 100.0,
        outliers.len()
    );
    for o in &outliers {
        let _ = writeln!(head, "  - {o}");
    }
    std::fs::write(out.join("report.md"), format!("{head}\n{md}"))?;
    std::fs::write(out.join("picks.csv"), csv)?;
    println!("{head}");
    Ok(())
}

/// The level of `x` through these blocks (dB RMS).
fn level(blocks: Vec<qrs_audio::board::Block>, x: &[f32]) -> f32 {
    hear(&Board::new(blocks), x).rms_db
}

/// A capture's trim (dB) to play as loud as the model of ours it stands in for, at the capture's gain: an amp or
/// pedal against our model alone, an IR against our cab after one of our amps (P33). None without a skin.
/// Returns the trim and the level it brings the capture to (our model's at the capture's gain), dB.
fn measure(it: &crate::captures::Item) -> Option<(f32, f32)> {
    use qrs_audio::board::Block;
    let skin = Block::of(it.skin.as_deref()?)?;
    if skin.def().file {
        return None;
    }
    let mut ours = skin;
    if let Some(g) = it.gain {
        for k in ["gain", "drive", "dist", "fuzz", "sustain", "volume"] {
            if ours.get(k).is_some() {
                ours.set(k, g);
                break;
            }
        }
    }
    let mut cap = Block::of(it.kind.model())?;
    cap.asset = it.id;
    qrs_audio::board::nam::set_trim(it.id, 0.0);
    let x = riff(it.bass);
    let (a, b) = if it.kind == crate::captures::Kind::Ir {
        let amp = Block::of(if it.bass { "amp_bass" } else { "amp_crunch" })?;
        (level(vec![amp, ours], &x), level(vec![amp, cap], &x))
    } else {
        (level(vec![ours], &x), level(vec![cap], &x))
    };
    (a.is_finite() && b.is_finite() && b > -90.0).then(|| ((a - b).clamp(-24.0, 24.0), a))
}

/// A gain ladder's steps levelled against each other (P37.6c, D139): each capture trimmed to our model at its gain
/// made the steps jump as our model does (+4 to +8 LU across a Hot Rod's or an SVT's ladder); now the steps sit
/// round their mean level, rising 0.4 dB a gain point (a turned-up amp is a little louder), within 1.5 dB. `steps`:
/// (id, gain, trim, the level that trim reached). Returns (id, the new trim).
pub fn level_ladder(steps: &[(u32, f32, f32, f32)]) -> Vec<(u32, f32)> {
    if steps.len() < 2 {
        return steps.iter().map(|s| (s.0, s.2)).collect();
    }
    let n = steps.len() as f32;
    let (ml, mg) = (
        steps.iter().map(|s| s.3).sum::<f32>() / n,
        steps.iter().map(|s| s.1).sum::<f32>() / n,
    );
    steps
        .iter()
        .map(|&(id, g, t, l)| {
            let want = ml + (0.4 * (g - mg)).clamp(-1.5, 1.5);
            (id, (t + want - l).clamp(-24.0, 24.0))
        })
        .collect()
}

/// Measure the trims the library lacks (or all, `every`), keep them in captures.json and in force: how many.
pub fn calibrate(every: bool) -> usize {
    crate::captures::ready();
    let ladder = |i: &crate::captures::Item| {
        (i.kind != crate::captures::Kind::Ir && i.gain.is_some())
            .then_some(i.tone_id)
            .flatten()
            .map(|t| (t, i.kind.key()))
    };
    let todo: Vec<crate::captures::Item> = crate::captures::with(|l| {
        // (a capture needing it, and every step of a ladder one of whose steps needs it: they're levelled together)
        let need = |i: &crate::captures::Item| {
            every || i.trim.is_none() || (ladder(i).is_some() && i.trim_v < crate::captures::TRIM_V)
        };
        let ladders: Vec<_> = l.items.iter().filter(|i| need(i)).filter_map(ladder).collect();
        l.items
            .iter()
            .filter(|i| need(i) || ladder(i).is_some_and(|k| ladders.contains(&k)))
            .cloned()
            .collect()
    });
    let mut done = Vec::new();
    let mut levels: Vec<(u32, f32, f32, f32)> = Vec::new();
    for it in &todo {
        let (t, lv) = measure(it).unwrap_or((0.0, f32::NAN));
        if it.gain.is_some() && lv.is_finite() {
            levels.push((it.id, it.gain.unwrap_or(0.0), t, lv));
        }
        done.push((it.id, t));
    }
    // each ladder levelled
    let mut keys: Vec<_> = todo.iter().filter_map(ladder).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let steps: Vec<(u32, f32, f32, f32)> = levels
            .iter()
            .copied()
            .filter(|s| todo.iter().any(|i| i.id == s.0 && ladder(i) == Some(k)))
            .collect();
        for (id, t) in level_ladder(&steps) {
            if let Some(d) = done.iter_mut().find(|d| d.0 == id) {
                d.1 = t;
            }
        }
    }
    for &(id, t) in &done {
        qrs_audio::board::nam::set_trim(id, t);
    }
    if !done.is_empty() {
        crate::captures::with(|l| l.set_trims(&done));
    }
    done.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ladders_steps_sit_round_their_mean() {
        // an SVT ladder: trimmed to our model, the steps reached -24, -20, -16 (an 8 LU jump)
        let steps = [(1, 2.0, 0.0, -24.0), (2, 5.0, 1.0, -20.0), (3, 8.0, 2.0, -16.0)];
        let out = level_ladder(&steps);
        let level = |id: u32| {
            let s = steps.iter().find(|s| s.0 == id).unwrap();
            let t = out.iter().find(|o| o.0 == id).unwrap().1;
            s.3 + (t - s.2)
        };
        assert!(level(3) - level(1) <= 3.0 + 1e-4, "{} {}", level(1), level(3));
        assert!(level(3) > level(1), "a higher step is still a little louder");
        assert_eq!(level_ladder(&steps[..1]), vec![(1, 0.0)]);
    }
}
