//! How loud each amp of ours plays (P37.6c, D139): measured, not modelled. `quickrs-cli board loudness` renders every
//! amp alone into its home cab, its gain (or volume) at 0..10 and the rest at defaults, on the listening set's guitar
//! chords or finger bass line, and prints `AMP_LUFS` (integrated BS.1770 loudness). The amp room reads it to keep the
//! level when an amp is swapped for another: the board's level moves by the outgoing amp's loudness minus the new
//! one's, at their gains (`swap_trim`), so a Twin at 3 swapped for a Rectifier at 3 doesn't jump 9 LU. New code.

use super::catalog::{Kind, ModelDef};
use super::{Block, MODELS};

/// The table `board loudness` printed (re-run it after an amp's voicing changes; a test checks every amp has a row).
pub const AMP_LUFS: &[(&str, [f32; 11])] = &[
    (
        "amp_clean",
        [
            -38.2, -36.5, -32.3, -27.9, -24.2, -21.0, -20.5, -20.6, -21.2, -22.1, -23.4,
        ],
    ),
    (
        "amp_tweed",
        [
            -28.2, -27.4, -25.4, -23.8, -22.7, -22.2, -22.1, -22.1, -22.3, -22.6, -22.9,
        ],
    ),
    (
        "amp_chime",
        [
            -32.2, -28.9, -26.8, -25.2, -24.1, -23.1, -22.6, -22.3, -22.2, -22.2, -22.4,
        ],
    ),
    (
        "amp_crunch",
        [
            -25.2, -25.4, -25.2, -24.8, -24.5, -24.2, -24.1, -24.1, -24.1, -24.2, -24.3,
        ],
    ),
    (
        "amp_lead",
        [
            -22.2, -22.3, -22.3, -22.3, -22.2, -22.1, -22.1, -22.1, -22.1, -22.3, -22.4,
        ],
    ),
    (
        "amp_modern",
        [
            -28.4, -27.7, -20.6, -18.9, -18.6, -18.5, -18.4, -18.3, -18.2, -18.2, -18.2,
        ],
    ),
    (
        "amp_bass",
        [
            -26.7, -23.8, -22.0, -20.8, -20.1, -19.6, -19.6, -19.6, -19.7, -19.8, -20.0,
        ],
    ),
    (
        "amp_b15",
        [
            -23.8, -23.8, -23.0, -22.2, -21.7, -21.4, -21.3, -21.4, -21.6, -21.9, -22.2,
        ],
    ),
    (
        "amp_bass_ss",
        [
            -28.1, -27.8, -26.3, -24.1, -21.8, -19.8, -18.4, -18.5, -19.9, -22.1, -24.8,
        ],
    ),
    (
        "amp_bass_tube",
        [
            -27.2, -25.9, -24.3, -22.9, -22.1, -21.9, -21.8, -21.9, -22.1, -22.4, -22.6,
        ],
    ),
    (
        "amp_cali",
        [
            -21.7, -21.6, -21.1, -20.7, -20.4, -20.2, -20.1, -20.0, -19.9, -19.9, -19.8,
        ],
    ),
    (
        "amp_snake",
        [
            -21.3, -21.3, -21.2, -21.1, -21.1, -20.9, -20.8, -20.8, -20.8, -20.7, -20.7,
        ],
    ),
    (
        "amp_tangerine",
        [
            -23.9, -23.9, -23.7, -23.4, -23.1, -22.9, -22.8, -22.9, -22.8, -22.9, -23.0,
        ],
    ),
    (
        "amp_surrey",
        [
            -29.7, -28.3, -26.9, -25.7, -24.6, -23.7, -23.1, -22.5, -22.2, -21.9, -21.8,
        ],
    ),
];

/// An amp for bass (measured on the bass line).
pub fn is_bass_amp(id: &str) -> bool {
    id.contains("bass") || id == "amp_b15"
}

/// The knob that sets an amp's gain ("gain", else "volume").
pub fn gain_key(m: &ModelDef) -> &'static str {
    if m.knob_index("gain").is_some() {
        "gain"
    } else {
        "volume"
    }
}

/// The cab an amp is measured into.
pub fn home_cab(id: &str) -> &'static str {
    match id {
        "amp_clean" | "amp_chime" => "cab_2x12",
        "amp_tweed" => "cab_1x12",
        "amp_surrey" => "cab_4x12",
        "amp_b15" => "cab_1x15",
        id if is_bass_amp(id) => "cab_8x10",
        _ => "cab_4x12",
    }
}

/// An amp's loudness (LUFS) with its gain at `g` (0-10, between the measured points), if it's in the table.
pub fn lufs_at(id: &str, g: f32) -> Option<f32> {
    let row = AMP_LUFS.iter().find(|r| r.0 == id)?.1;
    let g = g.clamp(0.0, 10.0);
    let i = (g.floor() as usize).min(9);
    let f = g - i as f32;
    Some(row[i] + (row[i + 1] - row[i]) * f)
}

/// A block's loudness as the table has it (an amp of ours; None for anything else).
pub fn block_lufs(b: &Block) -> Option<f32> {
    let d = b.def();
    if d.kind != Kind::Amp || d.file {
        return None;
    }
    lufs_at(d.id, b.get(gain_key(d)).unwrap_or(5.0))
}

/// The level change (dB) that keeps the sound as loud when amp `old` is swapped for `new` (0 when either isn't in
/// the table), within +-12 dB.
pub fn swap_trim(old: &Block, new: &Block) -> f32 {
    match (block_lufs(old), block_lufs(new)) {
        (Some(a), Some(b)) => (a - b).clamp(-12.0, 12.0),
        _ => 0.0,
    }
}

/// The amps of ours (for the table's test).
pub fn amps() -> impl Iterator<Item = &'static ModelDef> {
    MODELS.iter().filter(|m| m.kind == Kind::Amp && !m.file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_amp_has_its_loudness() {
        for m in amps() {
            let row = AMP_LUFS.iter().find(|r| r.0 == m.id);
            assert!(
                row.is_some(),
                "{}: no row (run `quickrs-cli board loudness`)",
                m.id
            );
            for v in row.unwrap().1 {
                assert!((-60.0..0.0).contains(&v), "{}: {v} LUFS", m.id);
            }
        }
    }

    #[test]
    fn a_swap_keeps_the_level() {
        let a = Block::of("amp_clean").unwrap().with("gain", 3.0);
        let b = Block::of("amp_modern").unwrap().with("gain", 3.0);
        let t = swap_trim(&a, &b);
        let (la, lb) = (block_lufs(&a).unwrap(), block_lufs(&b).unwrap());
        assert!((lb + t - la).abs() < 0.01 || t.abs() >= 12.0);
        assert_eq!(swap_trim(&a, &a), 0.0);
        assert_eq!(swap_trim(&a, &Block::of("green").unwrap()), 0.0);
    }
}
