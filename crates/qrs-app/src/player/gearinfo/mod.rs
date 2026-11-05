//! What the pedalboard's editor says about each model (PLAN.md P21.4): an About card (the real unit it's after,
//! its era, what it is, famous records), a plain-English tip for every knob with its sweet spot, and a few named
//! starting points. New code (no Python source). Data only; keyed by the catalogue's model ids and knob keys,
//! which never change (D26). `info` holds the amp room's "i" dot text (P37.14): the real unit each model is
//! inspired by (brand only in the `brands` build), its era, about, players and a line per front control.

#[cfg(test)]
use qrs_audio::board::{MODELS, Unit};
use qrs_audio::board::{ModelDef, catalog};

mod data;
mod info;

#[allow(unused_imports)] // (the room's "i" dot reads them, P37.14)
pub use info::{INFO, Info, info};

/// A knob's tip: what it does, where it usually sits.
#[derive(Clone, Copy, Debug)]
pub struct KnobTip {
    pub key: &'static str,
    /// What turning it does, in a sentence or two.
    pub text: &'static str,
    /// The sweet spot, in the knob's own units (0-10, dB, ms, Hz, %, or a switch's option index).
    pub sweet: (f32, f32),
    /// When the sweet spot holds, or when to leave it ("for crunch; 6-8 if the amp is dark").
    pub note: &'static str,
}

/// A record the real unit is heard on.
#[derive(Clone, Copy, Debug)]
pub struct Heard {
    pub song: &'static str,
    pub artist: &'static str,
    pub year: u16,
}

/// A named setting of a model's knobs (keys left out stay at the model's defaults).
#[derive(Clone, Copy, Debug)]
pub struct Start {
    pub name: &'static str,
    pub knobs: &'static [(&'static str, f32)],
}

/// Everything the editor says about a model.
#[derive(Clone, Copy, Debug)]
pub struct About {
    pub id: &'static str,
    /// The real unit, in full ("Marshall 1959 Super Lead"); shown only when `catalog::SHOW_BRANDS`.
    pub real: &'static str,
    /// The same without a brand ("a late-60s British 100-watt head"), for the public build.
    pub plain: &'static str,
    /// The year the real unit came out.
    pub year: u16,
    /// A short line of facts ("1967-69 · British · 100 W head"), no brand names.
    pub facts: &'static str,
    /// What it is and what it sounds like, two or three sentences, no brand names.
    pub what: &'static str,
    pub heard: &'static [Heard],
    /// Styles it suits ("Classic rock").
    pub try_for: &'static [&'static str],
    /// A tip for every knob of the model.
    pub tips: &'static [KnobTip],
    /// Three to five starting points.
    pub starts: &'static [Start],
}

/// The About card for a model id.
pub fn about(id: &str) -> Option<&'static About> {
    data::ABOUT.iter().find(|a| a.id == id)
}

/// A knob's tip (the controls behind the panel have theirs apart, in `data::BACK`).
pub fn tip(id: &str, key: &str) -> Option<&'static KnobTip> {
    about(id)?
        .tips
        .iter()
        .find(|t| t.key == key)
        .or_else(|| {
            data::BACK
                .iter()
                .find(|(m, _)| *m == id)
                .and_then(|(_, tips)| tips.iter().find(|t| t.key == key))
        })
        .or_else(|| {
            // (P37.2's controls: the model's own, then the ones every model with that knob shares)
            [id, "*"].into_iter().find_map(|m| {
                data::NEW_TIPS
                    .iter()
                    .find(|(x, _)| *x == m)
                    .and_then(|(_, tips)| tips.iter().find(|t| t.key == key))
            })
        })
}

/// The real unit as the build may show it: its name with brands on, else the plain description.
pub fn real_unit(a: &About) -> &'static str {
    if catalog::SHOW_BRANDS { a.real } else { a.plain }
}

/// A starting point's knob values for a model (the defaults where it doesn't say).
pub fn start_values(def: &ModelDef, s: &Start) -> [f32; qrs_audio::board::MAX_KNOBS] {
    let mut v = def.defaults();
    for &(k, x) in s.knobs {
        if let Some(i) = def.knob_index(k) {
            v[i] = def.knobs[i].clamp(x);
        }
    }
    v
}

/// The LFO range (Hz at 0 and at 10) behind a modulation knob that sets a rate, for tempo sync. These mirror the
/// DSP's `sweep(lo, hi, dial / 10)` in `qrs-audio/src/board/{modfx,delay}.rs`.
pub fn rate_range(id: &str, key: &str) -> Option<(f32, f32)> {
    match (id, key) {
        ("chorus", "rate") => Some((0.12, 6.0)),
        ("flanger", "rate") => Some((0.05, 4.0)),
        ("phaser", "speed") => Some((0.08, 8.0)),
        ("tremolo", "speed") => Some((1.0, 12.0)),
        ("vibe", "speed") => Some((0.6, 9.0)),
        ("mod_delay", "rate") => Some((0.1, 3.0)),
        _ => None,
    }
}

/// A rate knob's dial position for `hz` (clamped to the knob).
pub fn dial_for_hz(range: (f32, f32), hz: f32) -> f32 {
    let (lo, hi) = range;
    (10.0 * (hz / lo).ln() / (hi / lo).ln()).clamp(0.0, 10.0)
}

/// The rate in Hz at a dial position.
pub fn hz_for_dial(range: (f32, f32), dial: f32) -> f32 {
    let (lo, hi) = range;
    lo * (hi / lo).powf((dial / 10.0).clamp(0.0, 1.0))
}

/// Note values for tempo sync: (label, length in quarter notes).
pub const NOTES: [(&str, f32); 8] = [
    ("1/2", 2.0),
    ("1/4", 1.0),
    ("1/4 dotted", 1.5),
    ("1/4 triplet", 2.0 / 3.0),
    ("1/8", 0.5),
    ("1/8 dotted", 0.75),
    ("1/8 triplet", 1.0 / 3.0),
    ("1/16", 0.25),
];

/// A note value's length in ms at `bpm`.
pub fn note_ms(bpm: f32, quarters: f32) -> f32 {
    60_000.0 / bpm.max(1.0) * quarters
}

/// The note value a time in ms is (within 1.5%), at `bpm`.
pub fn note_of(bpm: f32, ms: f32) -> Option<usize> {
    NOTES
        .iter()
        .position(|(_, q)| (note_ms(bpm, *q) - ms).abs() <= ms.abs() * 0.015 + 0.5)
}

/// The models with no card.
#[cfg(test)]
fn missing() -> Vec<&'static str> {
    MODELS
        .iter()
        .filter(|m| about(m.id).is_none())
        .map(|m| m.id)
        .collect()
}

#[cfg(test)]
/// Whether a value is a sensible setting of a knob (in range; a switch at one of its positions).
pub fn fits(kd: &qrs_audio::board::KnobDef, v: f32) -> bool {
    match kd.unit {
        Unit::Choice(c) => v.fract() == 0.0 && v >= 0.0 && (v as usize) < c.len(),
        _ => v >= kd.lo - 1e-4 && v <= kd.hi + 1e-4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_has_its_card() {
        assert!(missing().is_empty(), "no About for {:?}", missing());
        let mut ids = std::collections::HashSet::new();
        for a in data::ABOUT {
            assert!(ids.insert(a.id), "{} twice", a.id);
            assert!(catalog::find(a.id).is_some(), "{} isn't a model", a.id);
            assert!(
                !a.real.is_empty() && !a.plain.is_empty() && !a.what.is_empty(),
                "{}",
                a.id
            );
            assert!((1920..=2030).contains(&a.year), "{}: year {}", a.id, a.year);
            assert!(!a.try_for.is_empty(), "{}: try it for", a.id);
        }
    }

    #[test]
    fn every_knob_has_a_tip_with_its_sweet_spot_in_range() {
        for m in MODELS {
            let a = about(m.id).unwrap();
            for kd in m.knobs {
                let t = tip(m.id, kd.key).unwrap_or_else(|| panic!("{}: no tip for {}", m.id, kd.key));
                assert!(!t.text.is_empty(), "{}: {}", m.id, kd.key);
                let (lo, hi) = t.sweet;
                assert!(
                    lo <= hi && fits(kd, lo) && fits(kd, hi),
                    "{}: {}'s sweet spot {lo}..{hi} is out of range",
                    m.id,
                    kd.key
                );
            }
            let back = data::BACK
                .iter()
                .filter(|(id, _)| *id == m.id)
                .flat_map(|(_, t)| t.iter());
            for t in a.tips.iter().chain(back) {
                assert!(
                    m.knob_index(t.key).is_some(),
                    "{}: a tip for no knob {}",
                    m.id,
                    t.key
                );
            }
        }
    }

    #[test]
    fn starting_points_are_in_range() {
        for m in MODELS {
            let a = about(m.id).unwrap();
            assert!(
                (3..=5).contains(&a.starts.len()),
                "{}: {} starting points",
                m.id,
                a.starts.len()
            );
            let mut names = std::collections::HashSet::new();
            for s in a.starts {
                assert!(names.insert(s.name), "{}: {} twice", m.id, s.name);
                assert!(!s.knobs.is_empty(), "{}: {} sets nothing", m.id, s.name);
                for &(k, v) in s.knobs {
                    let i = m
                        .knob_index(k)
                        .unwrap_or_else(|| panic!("{}: {} sets no knob {k}", m.id, s.name));
                    assert!(fits(&m.knobs[i], v), "{}: {} sets {k} to {v}", m.id, s.name);
                }
            }
        }
    }

    #[test]
    fn tempo_sync_maths() {
        assert!((note_ms(120.0, 0.75) - 375.0).abs() < 1e-3);
        assert_eq!(note_of(120.0, 375.0).map(|i| NOTES[i].0), Some("1/8 dotted"));
        assert_eq!(note_of(120.0, 300.0), None);
        let r = rate_range("tremolo", "speed").unwrap();
        let d = dial_for_hz(r, 4.0);
        assert!((hz_for_dial(r, d) - 4.0).abs() < 1e-3);
        for m in MODELS {
            for kd in m.knobs {
                if rate_range(m.id, kd.key).is_some() {
                    assert_eq!(kd.unit, Unit::Dial, "{}: {}", m.id, kd.key);
                }
            }
        }
    }
}
