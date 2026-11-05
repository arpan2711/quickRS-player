//! Song tones at one loudness (P41.1, D161; the guitar tech's review: "Teen Spirit's two cleans are 12 dB apart"). New
//! code. A song tone's board is measured on a short reference DI (open chords strummed, or a bass line; the synth's
//! plucked strings peaking at -10 dBFS, as `quickrs-cli board loudness` measures `AMP_LUFS`), its gate left out, and
//! its level set so it plays at `TARGET_LUFS`: the same loudness scale as `loudness::AMP_LUFS` (D139), so a song tone,
//! a starting board and an amp swap in the room all agree. Measured, not looked up: a drive into a clean amp, a
//! post-amp booster or an EQ pedal move the loudness as much as the amp's gain does. RS's own tone volume is then
//! added back within +-3 dB (`rs_trim`): what's left of RS's intent (a lead a little over the rhythm).
//!
//! Off the audio thread: renders about 1.8 s of audio (a few ms in a release build).

use std::sync::OnceLock;

use qrs_formats::model::Techs;

use super::{Board, render};
use crate::synth::{self, SynthNote};

const RATE: u32 = 48000;

/// Where a song tone's loudness lands (LUFS, on the reference DI): where the starting boards play on it (Clean,
/// Crunch, Lead, Jangle, Dotted 8th: -18.6 .. -18.1 LUFS, their levels matched by loudness in D26 and P37.2).
pub const TARGET_LUFS: f32 = -18.5;
/// RS's own tone volume, added back after levelling: this much a dB of it, within `RS_TRIM_DB`.
pub const RS_TRIM_PER_DB: f32 = 0.15;
pub const RS_TRIM_DB: f32 = 3.0;

fn note(t: f64, string: i32, fret: i32, sus: f64, in_chord: bool) -> SynthNote {
    SynthNote {
        t,
        string,
        fret,
        sus,
        tech: Techs::NONE,
        slide_to: -1,
        bend: 0.0,
        in_chord,
    }
}

fn dry(notes: &[SynthNote], bass: bool, end: f64) -> Vec<f32> {
    let opens: Vec<i32> = if bass {
        vec![28, 33, 38, 43]
    } else {
        vec![40, 45, 50, 55, 59, 64]
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let place = |t: f64| t;
    let o = synth::RenderOptions {
        rate: RATE,
        speed: 1.0,
        bass,
        tone: None,
        looped: false,
        seed: 7,
        place: &place,
        cancel: &cancel,
    };
    let st = synth::render(notes, &opens, 0.0, end, &o).unwrap_or_default();
    let n = (end * RATE as f64) as usize;
    let mono: Vec<f32> = st.chunks(2).map(|c| c[0]).take(n).collect();
    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    mono.iter().map(|v| v * 0.316 / peak).collect()
}

/// The reference DI: an E and an A chord strummed, then a short single-note line (guitar); a root-fifth bass line
/// (bass). Peaks at -10 dBFS.
pub fn reference(bass: bool) -> &'static [f32] {
    static GUITAR: OnceLock<Vec<f32>> = OnceLock::new();
    static BASS: OnceLock<Vec<f32>> = OnceLock::new();
    if bass {
        BASS.get_or_init(|| {
            let line = [(0, 0), (0, 0), (1, 0), (0, 3), (0, 0), (1, 2), (0, 5), (1, 0)];
            let n: Vec<SynthNote> = line
                .iter()
                .enumerate()
                .map(|(i, &(s, f))| note(i as f64 * 0.22, s, f, 0.18, false))
                .collect();
            dry(&n, true, 1.8)
        })
    } else {
        GUITAR.get_or_init(|| {
            let shapes: [&[(i32, i32)]; 2] = [
                &[(0, 0), (1, 2), (2, 2), (3, 1), (4, 0), (5, 0)],
                &[(1, 0), (2, 2), (3, 2), (4, 2), (5, 0)],
            ];
            let mut n = Vec::new();
            for (i, shape) in shapes.iter().enumerate() {
                let t = i as f64 * 0.55;
                for (k, &(s, f)) in shape.iter().enumerate() {
                    n.push(note(t + k as f64 * 0.012, s, f, 0.5, true));
                }
            }
            for (i, &(s, f)) in [(3, 2), (3, 4), (4, 2), (4, 3), (3, 4)].iter().enumerate() {
                n.push(note(1.1 + i as f64 * 0.14, s, f, 0.12, false));
            }
            dry(&n, false, 1.8)
        })
    }
}

/// Integrated loudness (ITU-R BS.1770-4 at 48 kHz: K-weighting, 400 ms blocks every 100 ms, the -70 LUFS and relative
/// -10 LU gates), as `quickrs-cli`'s listening set measures it.
pub fn lufs(x: &[f32]) -> f32 {
    let mut k = Vec::with_capacity(x.len());
    // the two K-weighting stages at 48 kHz, as the standard gives them
    let stages: [([f64; 3], [f64; 2]); 2] = [
        (
            [1.53512485958697, -2.69169618940638, 1.19839281085285],
            [-1.69065929318241, 0.73248077421585],
        ),
        ([1.0, -2.0, 1.0], [-1.99004745483398, 0.99007225036621]),
    ];
    let mut z = [[0.0f64; 4]; 2];
    for &v in x {
        let mut y = v as f64;
        for (st, (b, a)) in z.iter_mut().zip(stages.iter()) {
            let out = b[0] * y + b[1] * st[0] + b[2] * st[1] - a[0] * st[2] - a[1] * st[3];
            st[1] = st[0];
            st[0] = y;
            st[3] = st[2];
            st[2] = out;
            y = out;
        }
        k.push(y * y);
    }
    let (blk, hop) = ((0.4 * RATE as f64) as usize, (0.1 * RATE as f64) as usize);
    let mut ms = Vec::new();
    let mut i = 0;
    while i + blk <= k.len() {
        ms.push(k[i..i + blk].iter().sum::<f64>() / blk as f64);
        i += hop;
    }
    let l = |m: f64| -0.691 + 10.0 * m.max(1e-20).log10();
    let abs: Vec<f64> = ms.into_iter().filter(|m| l(*m) > -70.0).collect();
    if abs.is_empty() {
        return -70.0;
    }
    let gate = l(abs.iter().sum::<f64>() / abs.len() as f64) - 10.0;
    let rel: Vec<f64> = abs.into_iter().filter(|m| l(*m) > gate).collect();
    if rel.is_empty() {
        return -70.0;
    }
    l(rel.iter().sum::<f64>() / rel.len() as f64) as f32
}

/// A board's loudness on the reference DI (LUFS) at its level 0, its gates left out (they are open while you play).
/// Kept for the boards measured lately (a song opened again measures nothing).
pub fn board_lufs(board: &Board, bass: bool) -> f32 {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static SEEN: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);
    let mut b = board.clone();
    b.blocks.retain(|x| x.def().id != "gate");
    b.level_db = 0.0;
    let sig = format!("{bass}{}", b.to_json());
    if let Some(v) = SEEN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(&sig))
    {
        return *v;
    }
    let v = lufs(&render(&b, RATE, reference(bass)));
    let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() > 4096 {
        m.clear();
    }
    m.insert(sig, v);
    v
}

/// RS's tone volume as a trim (dB): 0.15 dB a dB from RS's usual -20, within +-3 dB.
pub fn rs_trim(volume_db: f32) -> f32 {
    if volume_db == 0.0 {
        return 0.0;
    }
    (RS_TRIM_PER_DB * (volume_db + 20.0)).clamp(-RS_TRIM_DB, RS_TRIM_DB)
}

/// The level (dB) that brings a song tone's board to `TARGET_LUFS`, then RS's own volume within +-3 dB; within the
/// board's -24..+12.
pub fn song_level(board: &Board, bass: bool, volume_db: f32) -> f32 {
    let l = board_lufs(board, bass);
    if l <= -69.0 {
        // (nothing comes out: leave it be)
        return rs_trim(volume_db);
    }
    let db = TARGET_LUFS - l + rs_trim(volume_db);
    ((db * 10.0).round() / 10.0).clamp(-24.0, 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Block, presets};

    #[test]
    fn the_reference_is_a_guitar_at_an_interfaces_level() {
        for bass in [false, true] {
            let x = reference(bass);
            let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - 0.316).abs() < 0.01, "{peak}");
            let l = lufs(x);
            assert!((-34.0..-20.0).contains(&l), "{bass}: {l}");
        }
    }

    #[test]
    fn a_board_comes_out_at_the_target() {
        let a = Board::new(vec![
            Block::of("amp_clean").unwrap().with("gain", 3.0),
            Block::of("cab_2x12").unwrap(),
        ]);
        let b = Board::new(vec![
            Block::of("amp_modern").unwrap().with("gain", 8.0),
            Block::of("cab_4x12").unwrap(),
        ]);
        let mut levels = Vec::new();
        for mut x in [a, b] {
            x.level_db = song_level(&x, false, -20.0);
            levels.push(board_lufs(&x, false) + x.level_db);
        }
        for l in &levels {
            assert!((l - TARGET_LUFS).abs() < 0.2, "{levels:?}");
        }
        // RS's volume moves it within 3 dB
        assert_eq!(rs_trim(-20.0), 0.0);
        assert_eq!(rs_trim(-60.0), -3.0);
        assert_eq!(rs_trim(0.0), 0.0);
    }

    /// The target sits among the starting guitar boards (their level matched by ear and by loudness, D26/P37.2).
    #[test]
    fn the_target_is_where_the_starting_boards_play() {
        let mut v: Vec<(String, f32)> = presets()
            .into_iter()
            .filter(|(n, _)| ["Clean", "Crunch", "Lead", "Jangle", "Dotted 8th"].contains(n))
            .map(|(n, b)| (n.to_string(), board_lufs(&b, false) + b.level_db))
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mid = v[v.len() / 2].1;
        eprintln!("starting boards on the reference: {v:?}");
        assert!((mid - TARGET_LUFS).abs() < 1.5, "{v:?}");
    }
}
