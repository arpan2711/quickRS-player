//! P41.1's tests (the guitar tech's and gigging guitarist's reviews): song tones at one loudness, the keyed gate after
//! the amp, the gate's threshold from the noise floor, and the mapping clean-ups.

use super::tests::guitar;
use super::*;
use qrs_formats::rig::{Gear, RigTone};

const RATE: u32 = 48000;

fn gear(slot: &str, key: &str, knobs: &[(&str, f32)]) -> Gear {
    Gear {
        slot: slot.into(),
        key: key.into(),
        knobs: knobs.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    }
}

fn tone(desc: &[&str], volume_db: f32, gear: Vec<Gear>) -> RigTone {
    RigTone {
        key: "x_tone".into(),
        name: "x_tone".into(),
        descriptors: desc.iter().map(|d| d.to_string()).collect(),
        volume_db,
        gear,
    }
}

fn ids(b: &Board) -> Vec<&'static str> {
    b.blocks.iter().map(|x| x.def().id).collect()
}

/// Teen Spirit's three tones as RS has them (the review's numbers: its two cleans 12 dB apart), Creep's clean and its
/// MT-2 into the tweed (11 dB), a hot bass: on a DI none of them was levelled on, all within 3 LU of each other.
#[test]
fn a_songs_tones_play_within_3_lu_on_the_same_di() {
    let cab = || gear("Cabinet", "Cab_CA412C_Ribbon_Cone", &[]);
    let songs: Vec<(&str, Vec<RigTone>)> = vec![
        (
            "teen spirit",
            vec![
                tone(
                    &["CLEAN"],
                    -18.0,
                    vec![
                        gear("Amp", "Amp_TW40", &[("Gain", 72.0), ("Treble", 60.0)]),
                        gear("PostPedal1", "Pedal_SpringReverb", &[("Mix", 25.0)]),
                        cab(),
                    ],
                ),
                tone(
                    &["CLEAN", "CHORUS"],
                    -22.0,
                    vec![
                        gear("PrePedal1", "Pedal_Chorus", &[("Mix", 50.0), ("Depth", 50.0)]),
                        gear("Amp", "Amp_CA85", &[("Gain", 12.0)]),
                        cab(),
                    ],
                ),
                tone(
                    &["DISTORTION"],
                    -20.0,
                    vec![
                        gear("PrePedal1", "Pedal_GermaniumDrive", &[("Gain", 72.0)]),
                        gear("Amp", "Amp_CA85", &[("Gain", 12.0)]),
                        cab(),
                    ],
                ),
            ],
        ),
        (
            "creep",
            vec![
                tone(
                    &["CLEAN"],
                    -20.0,
                    vec![
                        gear("Amp", "Amp_TW22", &[("Gain", 30.0)]),
                        gear("Cabinet", "Cab_TW112C_57_Edge", &[]),
                    ],
                ),
                tone(
                    &["DISTORTION"],
                    -20.0,
                    vec![
                        gear("PrePedal1", "Pedal_MetalDistortion", &[("Gain", 80.0)]),
                        gear("Amp", "Amp_TW22", &[("Gain", 30.0)]),
                        gear("Cabinet", "Cab_TW112C_57_Edge", &[]),
                    ],
                ),
            ],
        ),
    ];
    // a DI unlike the reference (single notes, -12 dBFS peaks)
    let x = guitar(3.0);
    for (song, tones) in songs {
        let levels: Vec<f32> = tones
            .iter()
            .map(|t| {
                let b = rs_gear::board_for(t);
                level::lufs(&render(&b, RATE, &x))
            })
            .collect();
        let spread =
            levels.iter().cloned().fold(f32::MIN, f32::max) - levels.iter().cloned().fold(f32::MAX, f32::min);
        eprintln!("{song}: {levels:?}");
        assert!(spread < 3.0, "{song}: {levels:?}");
    }
}

/// A bass tone lands at the same loudness as a guitar's (Hysteria's played 6 dB hot).
#[test]
fn a_hot_bass_tone_comes_down_to_the_target() {
    let t = tone(
        &["BASS", "DISTORTION"],
        -20.0,
        vec![
            gear("Amp", "Bass_Amp_CH350B", &[("Gain", 80.0)]),
            gear("PostPedal1", "Bass_Pedal_BassOverdrive", &[("Gain", 100.0)]),
            gear("Cabinet", "Bass_Cab_BT212BC_Tube_Cone", &[]),
        ],
    );
    let b = rs_gear::board_for(&t);
    let at = level::board_lufs(&b, true) + b.level_db;
    assert!((at - level::TARGET_LUFS).abs() < 0.3, "{at}");
    assert!(b.level_db < -1.0, "turned down: {}", b.level_db);
}

/// 5 ms RMS frames in dB.
fn frames_db(y: &[f32]) -> Vec<f32> {
    y.chunks(240)
        .map(|c| {
            10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32)
                .max(1e-14)
                .log10()
        })
        .collect()
}

/// A high-gain chain's tail (D162): the gate after the amp, keyed on the DI, turns the amp's sustain down from the
/// moment the note falls under the threshold, smoothly, to its range; the same gate in front of the amp turns the
/// input down, which the amp's clipping holds up, so the sound stays at full sustain and then falls late.
#[test]
fn a_high_gain_tail_closes_smoothly_after_the_amp() {
    // one note, decaying over a couple of seconds, over a single coil's buzz at -66 dBFS
    let n = RATE as usize * 3;
    let mut seed = 0x1234_5678u32;
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.0005 * 3f32.sqrt();
            let note = 0.25 * (-t * 2.5).exp() * (std::f32::consts::TAU * 110.0 * t).sin();
            note + noise
        })
        .collect();
    let gate = |key: f32| {
        Block::of("gate")
            .unwrap()
            .with("threshold", -50.0)
            .with("release", 120.0)
            .with("range", -60.0)
            .with("key", key)
    };
    let amp = || Block::of("amp_modern").unwrap().with("gain", 8.0);
    let cab = || Block::of("cab_4x12").unwrap();
    let before = Board::new(vec![gate(1.0), amp(), cab()]);
    let after = Board::new(vec![amp(), gate(0.0), cab()]);
    let open = Board::new(vec![amp(), cab()]);
    let (yb, ya, yo) = (
        frames_db(&render(&before, RATE, &x)),
        frames_db(&render(&after, RATE, &x)),
        frames_db(&render(&open, RATE, &x)),
    );
    // where the gate starts to close: the first frame either is 1 dB under the ungated sound
    let close = (20..yo.len())
        .find(|&i| yo[i] - ya[i] > 1.0 || yo[i] - yb[i] > 1.0)
        .unwrap_or(0)
        .max(1);
    eprintln!(
        "dB under the open sound from frame {close}: after {:?}
 before {:?}",
        (close..close + 80)
            .step_by(4)
            .map(|i| (yo[i] - ya[i]).round())
            .collect::<Vec<_>>(),
        (close..close + 80)
            .step_by(4)
            .map(|i| (yo[i] - yb[i]).round())
            .collect::<Vec<_>>()
    );
    // how far under the ungated sound each is, frame by frame, from there
    let under = |y: &[f32]| -> Vec<f32> { (close..yo.len().min(y.len())).map(|i| yo[i] - y[i]).collect() };
    let (ub, ua) = (under(&yb), under(&ya));
    let first = |u: &[f32], db: f32| u.iter().position(|v| *v > db).unwrap_or(u.len());
    // after the amp: 3 dB down within 40 ms of the close, and from there to 40 dB down without a step
    let a3 = first(&ua, 3.0);
    let b3 = first(&ub, 3.0);
    let a40 = first(&ua, 40.0);
    // the largest step (dB in 5 ms) in how far under the open sound it is, over the fade
    let step = |u: &[f32], to: usize| -> f32 {
        (1..to.min(u.len()))
            .map(|i| u[i] - u[i - 1])
            .fold(0.0f32, f32::max)
    };
    let (step_a, step_b) = (step(&ua, a40), step(&ub, first(&ub, 40.0)));
    eprintln!(
        "close at frame {close}: after the amp 3 dB down in {} ms, 40 dB in {} ms, largest step {step_a:.2} dB;          before the amp 3 dB down in {} ms, largest step {step_b:.2} dB",
        a3 * 5,
        a40 * 5,
        b3 * 5
    );
    assert!(a3 <= 8, "after the amp it turns down at once ({} ms)", a3 * 5);
    assert!(
        b3 >= a3 + 10,
        "in front of the amp the clipping holds the sound up ({} ms)",
        b3 * 5
    );
    assert!(a40 < ua.len(), "it reaches its range");
    assert!(a40 * 5 >= 150, "a fade, not a chop ({} ms)", a40 * 5);
    assert!(step_a < 1.5, "no step in the fade ({step_a:.2} dB in 5 ms)");
    // and it stops at its range: 60 dB under the open sound, not silence
    let tail = &ua[ua.len() - 40..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    assert!((mean - 60.0).abs() < 6.0, "{mean}");
}

#[test]
fn a_gate_saved_before_range_keeps_its_sound() {
    let old: serde_json::Value =
        serde_json::from_str(r#"{"model":"gate","on":true,"knobs":{"threshold":-54,"release":120}}"#)
            .unwrap();
    let b = Block::from_json(&old).unwrap();
    assert_eq!(b.get("range"), Some(-90.0));
    assert_eq!(b.get("key"), Some(1.0));
    // a new one is keyed on the input, gentle
    let n = Block::of("gate").unwrap();
    assert_eq!((n.get("range"), n.get("key")), (Some(-30.0), Some(0.0)));
    // and a round trip keeps them
    assert_eq!(Block::from_json(&b.to_json()).unwrap().get("range"), Some(-90.0));
}

#[test]
fn the_threshold_comes_from_the_noise_floor() {
    set_noise_floor(None);
    assert_eq!(gate_threshold(false), -62.0);
    assert_eq!(gate_threshold(true), -54.0);
    set_noise_floor(Some(-58.0));
    assert_eq!(gate_threshold(false), -50.0);
    assert_eq!(new_gate(true).get("threshold"), Some(-50.0));
    set_noise_floor(Some(-20.0));
    assert_eq!(gate_threshold(false), -30.0);
    set_noise_floor(None);
}

#[test]
fn a_new_gate_goes_after_the_amp() {
    let b = |id: &str| Block::of(id).unwrap();
    let mut off = b("amp_lead");
    off.on = false;
    let board = Board::new(vec![b("green"), b("amp_nam"), off, b("cab_4x12"), b("plate")]);
    assert_eq!(gate_slot(&board), 3);
    assert_eq!(gate_slot(&Board::new(vec![b("green"), b("plate")])), 1);
    assert_eq!(gate_slot(&Board::new(vec![b("plate")])), 0);
}

#[test]
fn clean_tones_get_no_gate_and_rs_s_post_amp_booster_is_a_clean_boost() {
    // Under the Bridge's lead: a JC into a 2x12, RS's booster after the amp
    let t = tone(
        &["CLEAN"],
        -20.0,
        vec![
            gear("Amp", "Amp_CS90", &[("Gain", 40.0)]),
            gear("PostPedal1", "Pedal_RangeBooster", &[("Boost", 80.0)]),
            gear("Cabinet", "Cab_Marshall1936_57_Edge", &[]),
        ],
    );
    let b = rs_gear::board_for(&t);
    assert_eq!(ids(&b), vec!["amp_clean", "boost", "cab_4x12"]);
    // in front of the amp it's still the treble booster
    let t = tone(
        &["LEAD"],
        -20.0,
        vec![
            gear("PrePedal1", "Pedal_RangeBooster", &[("Boost", 80.0)]),
            gear("Amp", "Amp_MarshallPlexi", &[("Gain", 70.0)]),
            gear("Cabinet", "Cab_Marshall1960TV_57_Edge", &[]),
        ],
    );
    assert_eq!(rs_gear::board_for(&t).blocks[0].def().id, "treble_boost");
}

#[test]
fn the_mapping_clean_ups() {
    // a Mark IIC+ at gain 1 is its clean channel even on a "driven" tone (Black Hole Sun's rotomod)
    let t = tone(
        &["OVERDRIVE"],
        -20.0,
        vec![
            gear("Amp", "Amp_CA85", &[("Gain", 1.0)]),
            gear("Cabinet", "Cab_CA412C_57_Cone", &[]),
        ],
    );
    let b = rs_gear::board_for(&t);
    assert_eq!(b.blocks[0].def().id, "amp_clean");
    assert!(
        b.blocks[0].get("gain").unwrap() >= 3.0,
        "its clean channel at a normal volume"
    );
    // a bass part on a guitar 1x12 (Another One Bites the Dust): a bass cab, half DI
    let t = tone(
        &["BASS"],
        -20.0,
        vec![
            gear("PrePedal1", "Pedal_LoFiFilter", &[("Mix", 1.0)]),
            gear("Amp", "Bass_Amp_CS75B", &[("Gain", 40.0)]),
            gear("Cabinet", "Cab_CS1120C_57_Cone", &[]),
        ],
    );
    let b = rs_gear::board_for(&t);
    assert_eq!(ids(&b), vec!["amp_bass_ss", "cab_2x10"], "the LoFi at 1% is gone");
    assert_eq!(b.blocks[1].get("di"), Some(50.0));
    // RS's acoustic emulator on an electric part is left out (Seven Nation Army)
    let t = tone(
        &["DISTORTION"],
        -20.0,
        vec![
            gear("PrePedal1", "Pedal_AcousticEmulator", &[]),
            gear("Amp", "Amp_AT20", &[("Gain", 50.0)]),
            gear("Cabinet", "Cab_AT1121C_57_Cone", &[]),
        ],
    );
    assert!(!ids(&rs_gear::board_for(&t)).contains(&"acoustic"));
    // an Octavia next to a fuzz is the octave fuzz (Purple Haze's solo)
    let t = tone(
        &["FUZZ"],
        -20.0,
        vec![
            gear("PrePedal1", "Pedal_OctaveUp", &[("Mix", 100.0)]),
            gear("PrePedal2", "Pedal_CaptFuzzle", &[("Gain", 85.0)]),
            gear("Amp", "Amp_MarshallPlexi", &[("Gain", 74.0)]),
            gear("Cabinet", "Cab_Marshall1960TV_Ribbon_Cone", &[]),
        ],
    );
    let b = rs_gear::board_for(&t);
    let v = ids(&b);
    assert!(
        v.contains(&"octave_fuzz") && !v.contains(&"octaver") && !v.contains(&"round_fuzz"),
        "{v:?}"
    );
}
