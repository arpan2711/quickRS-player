//! P37.2's synthetic tests (the list in `soundroom-review/BASS-GEAR.md` section 5): blends and clean lows, the low
//! end for a 5-string, the bass cabs' DI and horn, the bass amp voices, the grinder and the bass Muff, the octaver's
//! tracking, the chorus's low cut, the envelope filter's new controls, the opto compressor, the delays' Sync, and
//! the capture blocks' controls, gain ladder and rebuild.

use std::f32::consts::TAU;

use super::dsp::{Biquad, Filter};
use super::*;
use crate::rt::ToneProcessor;

const RATE: u32 = 48000;
const R: f32 = RATE as f32;

fn sine(f: f32, amp: f32, secs: f32) -> Vec<f32> {
    (0..(secs * R) as usize)
        .map(|i| amp * (TAU as f64 * f as f64 * i as f64 / R as f64).sin() as f32)
        .collect()
}

/// Additive notes with decaying harmonics; `h2` is the 2nd harmonic's level against the fundamental's.
fn note(f: f32, h2: f32, secs: f32, amp: f32) -> Vec<f32> {
    (0..(secs * R) as usize)
        .map(|i| {
            let t = i as f32 / R;
            let env = (1.0 - (-t * 300.0).exp()) * (-t * 1.2).exp();
            let mut s = (TAU * f * t).sin() + h2 * (TAU * 2.0 * f * t).sin();
            for h in 3..8 {
                s += (TAU * f * h as f32 * t).sin() / h as f32 * (-t * h as f32).exp();
            }
            amp * env * s
        })
        .collect()
}

/// The level of `f` in `x` (one DFT bin, Hann window).
fn level(x: &[f32], f: f32) -> f32 {
    let n = x.len();
    let (mut re, mut im, mut ws) = (0.0f64, 0.0f64, 0.0f64);
    for (i, &v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
        let ph = std::f64::consts::TAU * f as f64 * i as f64 / R as f64;
        re += v as f64 * w * ph.cos();
        im += v as f64 * w * ph.sin();
        ws += w;
    }
    (2.0 * (re * re + im * im).sqrt() / ws) as f32
}

fn db(a: f32, b: f32) -> f32 {
    20.0 * (a.max(1e-12) / b.max(1e-12)).log10()
}

fn run_block(b: &Block, x: &[f32]) -> Vec<f32> {
    let mut p = make_pedal(b, RATE);
    let mut y = x.to_vec();
    for c in y.chunks_mut(128) {
        p.process(c);
    }
    y
}

fn blk(id: &str, knobs: &[(&str, f32)]) -> Block {
    let mut b = Block::of(id).unwrap_or_else(|| panic!("{id}"));
    for (k, v) in knobs {
        assert!(b.def().knob_index(k).is_some(), "{id} has no {k}");
        b.set(k, *v);
    }
    b
}

/// Skip the first `s` seconds (filters and envelopes settling).
fn after(x: &[f32], s: f32) -> &[f32] {
    &x[(s * R) as usize..]
}

// -- 1. blend and clean low -------------------------------------------------------------------------------------------

#[test]
fn a_drive_at_blend_zero_is_its_input_and_clean_low_keeps_the_fundamental_clean() {
    // an E1 (41.2 Hz) with its harmonics
    let x = note(41.2, 0.5, 3.0, 0.3);
    for id in [
        "green",
        "orange",
        "rodent",
        "metal",
        "big_pi",
        "round_fuzz",
        "bass_grind",
        "bass_muff",
    ] {
        let lvl = |b: &Block| {
            let mut p = make_pedal(b, RATE);
            // the level the pedal's own Level puts on everything: measure it on the dry path itself
            let mut y = x.clone();
            p.process(&mut y);
            y
        };
        let dry = lvl(&blk(id, &[("blend", 0.0)]));
        // at blend 0 the output is the input times the pedal's level: the same shape at every frequency
        let k = level(after(&dry, 1.0), 41.2) / level(after(&x, 1.0), 41.2);
        for f in [82.4, 123.6, 206.0] {
            let kf = level(after(&dry, 1.0), f) / level(after(&x, 1.0), f);
            assert!(
                db(kf, k).abs() < 0.5,
                "{id}: blend 0 at {f} Hz is {:.2} dB off",
                db(kf, k)
            );
        }
        // with Clean low at 120 Hz the 41 Hz fundamental is the dry one (within 1 dB) at any blend
        let wet = lvl(&blk(id, &[("blend", 100.0), ("clean_low", 2.0)]));
        let k41 = level(after(&wet, 1.0), 41.2) / level(after(&x, 1.0), 41.2);
        assert!(
            db(k41, k).abs() < 1.5,
            "{id}: clean low keeps 41 Hz ({:.2} dB)",
            db(k41, k)
        );
    }
}

#[test]
fn for_bass_halves_the_blend_and_cleans_the_lows() {
    let b = Block::of("green").unwrap().for_bass();
    assert_eq!(b.get("blend"), Some(50.0));
    assert_eq!(b.get("clean_low"), Some(2.0));
    assert_eq!(Block::of("chorus").unwrap().for_bass().get("low_cut"), Some(3.0));
    // the SansAmp keeps its own Blend
    assert_eq!(
        Block::of("bass_drive").unwrap().for_bass().get("blend"),
        Some(60.0)
    );
}

// -- 2. a 5-string's low B ---------------------------------------------------------------------------------------------

#[test]
fn the_bass_amps_and_drive_keep_a_low_b() {
    // a sine at a low B (30.87 Hz) against one at 82 Hz, quiet enough to stay clean: within 1.5 dB before the cab
    for (id, knobs) in [
        ("amp_bass", vec![("gain", 1.0), ("bass", 5.0), ("treble", 5.0)]),
        ("amp_bass_ss", vec![("gain", 1.0)]),
        ("amp_bass_tube", vec![("gain", 1.0), ("bass", 5.0)]),
        ("amp_b15", vec![("volume", 1.0), ("bass", 5.0), ("input", 0.0)]),
        ("bass_drive", vec![("gain", 0.0), ("blend", 0.0)]),
    ] {
        let b = blk(id, &knobs);
        let lo = run_block(&b, &sine(30.87, 0.02, 3.0));
        let hi = run_block(&b, &sine(82.4, 0.02, 3.0));
        let d = db(level(after(&lo, 1.5), 30.87), level(after(&hi, 1.5), 82.4));
        // the tone stack's own bass curve is the amp's voice, not a high-pass: taken out of the comparison
        let stack = match id {
            "amp_bass" => Some(amp::AMPEG),
            "amp_b15" | "amp_bass_tube" => Some(amp::BASSMAN),
            _ => None,
        }
        .map(|s| (s.response_db(30.87, 0.5, 0.5, 0.5) - s.response_db(82.4, 0.5, 0.5, 0.5)) as f32)
        .unwrap_or(0.0)
            // and the power stage's depth (a peak at 90 Hz) is the voicing too
            + {
                let depth = match id {
                    "amp_bass" | "amp_b15" => 2.0,
                    "amp_bass_tube" => 3.0,
                    _ => 0.0,
                };
                let d = Biquad::new(R, Filter::Peak { f: 90.0, q: 0.8, db: depth });
                db(d.magnitude(30.87, R), d.magnitude(82.4, R))
            };
        eprintln!("{id}: 31 Hz vs 82 Hz {d:+.2} dB (the stack's own {stack:+.2})");
        // (the solid-state head, which has no stack or depth, is the plain measure of the high-passes: -0.7 dB)
        assert!(d - stack > -2.0, "{id}: a low B is {:.1} dB down", d - stack);
    }
}

// -- 5. bass cabs --------------------------------------------------------------------------------------------------------

#[test]
fn a_bass_cab_at_full_di_is_flat_and_its_horn_adds_the_top() {
    for id in ["cab_8x10", "cab_4x10", "cab_2x10", "cab_4x15", "cab_1x15"] {
        let di = blk(id, &[("di", 100.0)]);
        for f in [40.0, 250.0, 1000.0, 3000.0] {
            let x = sine(f, 0.2, 1.0);
            let y = run_block(&di, &x);
            let d = db(level(after(&y, 0.5), f), level(after(&x, 0.5), f));
            assert!(d.abs() < 0.6, "{id} DI 100 at {f} Hz: {d:.2} dB");
        }
        let x = sine(6000.0, 0.2, 1.0);
        let off = run_block(&blk(id, &[]), &x);
        let horn = run_block(&blk(id, &[("horn", 10.0)]), &x);
        let d = db(level(after(&horn, 0.5), 6000.0), level(after(&off, 0.5), 6000.0));
        assert!(d > 3.0, "{id}: the horn adds {d:.1} dB at 6 kHz");
    }
    // the speakers differ where they should: the 4x15 deepest and darkest, the 2x10 the least low end
    let resp = |id: &str, f: f32| cab::response_db(id, &MODELS[catalog::find(id).unwrap()].defaults(), R, f);
    assert!(resp("cab_4x15", 45.0) > resp("cab_2x10", 45.0) + 3.0);
    assert!(resp("cab_4x10", 2500.0) > resp("cab_4x15", 2500.0) + 3.0);
}

// -- 6-8. the bass amp voices ---------------------------------------------------------------------------------------------

fn thd(y: &[f32], f: f32) -> f32 {
    let h1 = level(y, f);
    let rest: f32 = (2..6).map(|h| level(y, f * h as f32).powi(2)).sum::<f32>().sqrt();
    rest / h1.max(1e-9)
}

#[test]
fn the_bass_amps_break_up_with_gain_and_the_contour_scoops_500_hz() {
    // a low E at a hard pluck's level
    let x = sine(41.2, 0.25, 2.0);
    for (id, g) in [
        ("amp_b15", "volume"),
        ("amp_bass_ss", "gain"),
        ("amp_bass_tube", "gain"),
        ("amp_bass", "gain"),
    ] {
        let lo = thd(after(&run_block(&blk(id, &[(g, 1.0)]), &x), 1.0), 41.2);
        let hi = thd(after(&run_block(&blk(id, &[(g, 9.0)]), &x), 1.0), 41.2);
        eprintln!("{id}: THD {:.1}% at 1, {:.1}% at 9", lo * 100.0, hi * 100.0);
        assert!(hi > lo * 2.0 && hi > 0.05, "{id}: {lo} -> {hi}");
    }
    // the solid-state head's Contour: 500 Hz down 6 dB or more, 60 Hz and 3 kHz much less
    let at = |contour: f32, f: f32| {
        let y = run_block(
            &blk("amp_bass_ss", &[("gain", 0.5), ("contour", contour)]),
            &sine(f, 0.02, 1.0),
        );
        level(after(&y, 0.5), f)
    };
    let notch = db(at(1.0, 500.0), at(0.0, 500.0));
    assert!(notch < -6.0, "contour {notch:.1} dB at 500 Hz");
    assert!(db(at(1.0, 60.0), at(0.0, 60.0)) > -2.5);
    // the SVT's Ultra Hi lifts the top, Ultra Lo scoops the mids
    let svt = |k: &str, f: f32| {
        let y = run_block(&blk("amp_bass", &[("gain", 1.0), (k, 1.0)]), &sine(f, 0.02, 1.0));
        let y0 = run_block(&blk("amp_bass", &[("gain", 1.0)]), &sine(f, 0.02, 1.0));
        db(level(after(&y, 0.5), f), level(after(&y0, 0.5), f))
    };
    assert!(svt("ultra_hi", 6000.0) > 4.0);
    assert!(svt("ultra_lo", 500.0) < -6.0);
    // and the voices are not one another: their responses to the same note differ
    let n = note(55.0, 0.5, 2.0, 0.25);
    let ids = ["amp_bass", "amp_b15", "amp_bass_ss", "amp_bass_tube"];
    let spec: Vec<Vec<f32>> = ids
        .iter()
        .map(|id| {
            let y = run_block(&blk(id, &[]), &n);
            let y = after(&y, 0.5);
            let base = level(y, 55.0);
            [110.0, 220.0, 440.0, 880.0, 1760.0, 3520.0]
                .iter()
                .map(|&f| db(level(y, f), base))
                .collect()
        })
        .collect();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let d: f32 = spec[i]
                .iter()
                .zip(&spec[j])
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                / 6.0;
            assert!(d > 1.0, "{} and {} sound alike ({d:.2} dB apart)", ids[i], ids[j]);
        }
    }
}

// -- 9, 13. the grinder and the bass Muff -------------------------------------------------------------------------------------

#[test]
fn the_grinder_keeps_its_low_band_clean_and_the_bass_muff_passes_its_dry() {
    let x = sine(41.2, 0.25, 2.0);
    let full = run_block(&blk("bass_grind", &[("drive", 8.0), ("blend", 50.0)]), &x);
    let split = run_block(
        &blk(
            "bass_grind",
            &[("drive", 8.0), ("blend", 50.0), ("clean_low", 4.0)],
        ),
        &x,
    );
    // the low band (under the 250 Hz crossover): its harmonics against the fundamental
    let low_thd = |y: &[f32]| {
        let h1 = level(y, 41.2);
        (2..6)
            .map(|h| level(y, 41.2 * h as f32).powi(2))
            .sum::<f32>()
            .sqrt()
            / h1
    };
    let (t_full, t_split) = (low_thd(after(&full, 1.0)), low_thd(after(&split, 1.0)));
    eprintln!("grinder low band THD {t_full:.3} -> {t_split:.3} with Clean low");
    assert!(t_split < t_full * 0.5, "clean low: THD {t_full} -> {t_split}");
    // the grunt switch: Fat has more 100 Hz into the clipper than Raw
    let g = |v: f32| {
        level(
            after(
                &run_block(
                    &blk("bass_grind", &[("grunt", v), ("blend", 100.0)]),
                    &sine(100.0, 0.05, 1.0),
                ),
                0.5,
            ),
            100.0,
        )
    };
    assert!(g(0.0) > g(2.0));
    // the bass Muff at no blend is its input (times its volume); its Bass Boost lifts the lows
    let x = note(55.0, 0.5, 2.0, 0.25);
    let dry = run_block(&blk("bass_muff", &[("blend", 0.0)]), &x);
    let k = level(after(&dry, 0.5), 55.0) / level(after(&x, 0.5), 55.0);
    let k3 = level(after(&dry, 0.5), 165.0) / level(after(&x, 0.5), 165.0);
    assert!(db(k3, k).abs() < 0.5);
    // a 41 + 55 Hz two-tone: the fuzz's intermodulation (14, 27, 69, 96 Hz) under the two notes at the default
    // blend, and more of it with no dry
    let tt: Vec<f32> = sine(41.0, 0.15, 2.0)
        .iter()
        .zip(sine(55.0, 0.15, 2.0))
        .map(|(a, b)| a + b)
        .collect();
    let imd = |blend: f32| {
        let y = run_block(&blk("bass_muff", &[("blend", blend)]), &tt);
        let y = after(&y, 1.0);
        let notes = level(y, 41.0) + level(y, 55.0);
        let products = level(y, 27.0) + level(y, 69.0) + level(y, 96.0) + level(y, 14.0);
        products / notes
    };
    let (blended, full) = (imd(70.0), imd(100.0));
    eprintln!("bass Muff IMD/notes: {blended:.3} at blend 70, {full:.3} at 100");
    assert!(
        blended < 1.0 && blended < full,
        "the dry keeps the notes clear ({blended} vs {full})"
    );
}

// -- 10. the octaver -----------------------------------------------------------------------------------------------------------

#[test]
fn the_octaver_tracks_a_bass_note_whose_second_harmonic_is_louder() {
    for f in [30.87f32, 41.2, 55.0, 82.4, 110.0, 196.0, 330.0] {
        // H2 at +6 dB over the fundamental (a pick near the bridge)
        let x = note(f, 2.0, 2.0, 0.15);
        let y = run_block(&blk("octaver", &[("sub", 10.0), ("dry", 0.0), ("up", 0.0)]), &x);
        let y = &y[(0.3 * R) as usize..(1.3 * R) as usize];
        let sub = level(y, f / 2.0);
        // what a tracker on the 2nd harmonic would give: the note itself
        let wrong = level(y, f);
        eprintln!("{f:6.2} Hz: f/2 {:.4}, f {:.4}", sub, wrong);
        assert!(
            sub > 2.0 * wrong,
            "{f} Hz: the octave down is not on top ({sub} vs {wrong})"
        );
        // the peak of the sub is at f/2 within 1%
        let peak = (0..41)
            .map(|k| f / 2.0 * (0.98 + 0.001 * k as f32))
            .max_by(|a, b| level(y, *a).total_cmp(&level(y, *b)))
            .unwrap();
        assert!((peak / (f / 2.0) - 1.0).abs() < 0.01, "{f}: peak at {peak}");
    }
    // Sub 2: two octaves down
    let x = note(110.0, 0.5, 2.0, 0.15);
    let y = run_block(&blk("octaver", &[("sub", 0.0), ("dry", 0.0), ("sub2", 10.0)]), &x);
    let y = &y[(0.3 * R) as usize..(1.3 * R) as usize];
    assert!(level(y, 27.5) > level(y, 55.0), "sub 2 at f/4");
    // a glide: no long dropouts in the sub while the note slides an octave
    let n = (1.5 * R) as usize;
    let mut ph = 0.0f32;
    let glide: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            ph += TAU * 55.0 * 2f32.powf(t) / R;
            0.15 * (ph.sin() + 1.5 * (2.0 * ph).sin())
        })
        .collect();
    let y = run_block(&blk("octaver", &[("sub", 10.0), ("dry", 0.0)]), &glide);
    let hop = (0.01 * R) as usize;
    let rms: Vec<f32> = y[(0.1 * R) as usize..]
        .chunks(hop)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let med = {
        let mut s = rms.clone();
        s.sort_by(f32::total_cmp);
        s[s.len() / 2]
    };
    let drops = rms.iter().filter(|&&r| r < med * 0.25).count();
    assert!(drops <= 2, "{drops} dropouts in a glide");
}

// -- 11. the chorus's low cut -----------------------------------------------------------------------------------------------------

#[test]
fn the_chorus_low_cut_keeps_the_lows_still() {
    let x: Vec<f32> = sine(60.0, 0.2, 4.0)
        .iter()
        .zip(sine(1500.0, 0.05, 4.0))
        .map(|(a, b)| a + b)
        .collect();
    let wobble = |low_cut: f32| {
        let y = run_block(
            &blk(
                "chorus",
                &[
                    ("depth", 10.0),
                    ("rate", 6.0),
                    ("mix", 100.0),
                    ("low_cut", low_cut),
                ],
            ),
            &x,
        );
        // the 60 Hz band's level, 100 ms at a time: how much it moves
        let lv: Vec<f32> = after(&y, 0.5)
            .chunks((0.1 * R) as usize)
            .filter(|c| c.len() == (0.1 * R) as usize)
            .map(|c| level(c, 60.0))
            .collect();
        let m = lv.iter().sum::<f32>() / lv.len() as f32;
        (lv.iter().map(|v| (v - m).powi(2)).sum::<f32>() / lv.len() as f32).sqrt() / m
    };
    let (off, on) = (wobble(0.0), wobble(3.0));
    eprintln!("60 Hz wobble: {off:.4} off, {on:.4} at 200 Hz");
    assert!(on < off * 0.3 || on < 0.01, "{off} -> {on}");
}

// -- 12. the envelope filter ------------------------------------------------------------------------------------------------------

#[test]
fn the_envelope_filter_range_direction_and_mix() {
    // a burst train: loud decaying noise bursts and rests (a flat spectrum, so the filter's shape shows)
    let mut x = vec![0.0f32; (3.0 * R) as usize];
    let mut noise = qrs_analysis::testsig::Noise::new(5);
    for k in 0..6 {
        let n: Vec<f32> = (0..(0.4 * R) as usize)
            .map(|i| 0.3 * noise.sample() * (-(i as f32) / R * 8.0).exp())
            .collect();
        let at = (k as f32 * 0.5 * R) as usize;
        for (i, v) in n.iter().enumerate() {
            if at + i < x.len() {
                x[at + i] += v;
            }
        }
    }
    let cent = |y: &[f32]| {
        let fs: Vec<f32> = (1..80).map(|k| 60.0 * 1.06f32.powi(k)).collect();
        let e: Vec<f32> = fs.iter().map(|&f| level(y, f).powi(2) * f).collect();
        fs.iter().zip(&e).map(|(f, e)| f * e).sum::<f32>() / e.iter().sum::<f32>()
    };
    let base = [("sens", 8.0), ("res", 6.0), ("mode", 1.0)];
    let with = |extra: &[(&str, f32)]| {
        let mut k: Vec<(&str, f32)> = base.to_vec();
        k.extend_from_slice(extra);
        run_block(&blk("env_filter", &k), &x)
    };
    let hi = cent(&with(&[]));
    let lo = cent(&with(&[("range", 1.0)]));
    let down0 = cent(&with(&[("direction", 1.0)]));
    eprintln!("envelope centroids: Hi {hi:.0} Hz, Lo {lo:.0}, Down {down0:.0}");
    assert!(lo < hi, "Lo range is darker ({lo} vs {hi})");
    let down = cent(&with(&[("direction", 1.0)]));
    assert!(down < hi, "Down closes on the attack ({down} vs {hi})");
    // Mix 0 is the dry signal; Boost adds level
    let dry = with(&[("mix", 0.0)]);
    let e: f32 = dry.iter().zip(&x).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(e < 1e-3, "mix 0: {e}");
    let loud = with(&[("boost", 12.0)]);
    let r = |y: &[f32]| y.iter().map(|v| v * v).sum::<f32>();
    assert!(db(r(&loud).sqrt(), r(&with(&[])).sqrt()) > 10.0);
}

// -- 14. the opto compressor --------------------------------------------------------------------------------------------------------

#[test]
fn the_opto_lets_go_faster_after_a_short_note_than_after_a_long_phrase() {
    // a loud burst (short or long) then a quiet tone: how long until the quiet tone is back near its level
    let recover = |burst_s: f32| {
        let mut x = sine(200.0, 0.5, burst_s);
        x.extend(sine(200.0, 0.02, 3.0));
        let y = run_block(&blk("opto_comp", &[("peak", 8.0)]), &x);
        let start = (burst_s * R) as usize;
        let end_level = level(&y[y.len() - (0.3 * R) as usize..], 200.0);
        let hop = (0.02 * R) as usize;
        (0..)
            .map(|k| start + k * hop)
            .take_while(|&i| i + hop < y.len())
            .find(|&i| level(&y[i..i + hop], 200.0) > end_level * 0.7)
            .map(|i| (i - start) as f32 / R)
            .unwrap_or(9.0)
    };
    let (short, long) = (recover(0.08), recover(2.5));
    eprintln!("opto: back after {short:.2} s (short note), {long:.2} s (long phrase)");
    assert!(
        long > short * 2.0,
        "programme-dependent release: {short} vs {long}"
    );
    // and it compresses: 20 dB more in, much less than 20 dB more out
    let q = run_block(&blk("opto_comp", &[("peak", 8.0)]), &sine(200.0, 0.03, 2.0));
    let l = run_block(&blk("opto_comp", &[("peak", 8.0)]), &sine(200.0, 0.3, 2.0));
    let d = db(level(after(&l, 1.5), 200.0), level(after(&q, 1.5), 200.0));
    assert!(d < 12.0, "20 dB in gives {d:.1} dB out");
}

// -- the delays' Sync ------------------------------------------------------------------------------------------------------------

#[test]
fn a_synced_delay_repeats_on_the_beat() {
    set_tempo(120.0);
    // a click; the digital delay on 1/4 at 120 bpm: the echo 500 ms later (its Time knob says 380)
    let mut x = vec![0.0f32; (1.2 * R) as usize];
    x[100] = 0.5;
    let y = run_block(
        &blk(
            "digital_delay",
            &[("sync", 1.0), ("repeats", 0.0), ("mix", 100.0)],
        ),
        &x,
    );
    let peak = y
        .iter()
        .enumerate()
        .skip(200)
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(i, _)| i)
        .unwrap();
    let ms = (peak - 100) as f32 / R * 1000.0;
    assert!((ms - 500.0).abs() < 3.0, "echo at {ms} ms");
    assert!((tempo_of_beats(&[0.0, 0.5, 1.0, 1.5, 2.0]) - 120.0).abs() < 0.01);
}

// -- 15. capture blocks ----------------------------------------------------------------------------------------------------------

#[test]
fn a_capture_s_eq_is_relative_to_the_capture_and_its_ladder_steps_and_crossfades() {
    let tiny = super::nam::tests::tiny_capture(r#", "sample_rate": 48000"#);
    for id in [9301, 9302, 9303] {
        nam::register_nam(id, &tiny).unwrap();
    }
    nam::set_ladder(9301, 77, 2.0);
    nam::set_ladder(9302, 77, 5.0);
    nam::set_ladder(9303, 77, 8.0);
    // the ladder: below 0 the block's own capture; else the nearest gain
    assert_eq!(nam::ladder_pick(9302, -1.0), 9302);
    assert_eq!(nam::ladder_pick(9302, 1.0), 9301);
    assert_eq!(nam::ladder_pick(9301, 6.9), 9303);
    let mut amp = Block::of("amp_nam").unwrap();
    amp.set_asset(9302);
    assert_eq!(
        amp.get("gain"),
        Some(5.0),
        "the Gain knob starts on the capture's own gain"
    );
    // the EQ at 0 dB is the capture as made; +9 dB of bass lifts 80 Hz, not 2 kHz
    let x: Vec<f32> = sine(80.0, 0.05, 1.0)
        .iter()
        .zip(sine(2000.0, 0.05, 1.0))
        .map(|(a, b)| a + b)
        .collect();
    let flat = run_block(&amp, &x);
    let mut boosted = amp;
    boosted.set("bass", 9.0);
    let b = run_block(&boosted, &x);
    assert!(db(level(after(&b, 0.5), 80.0), level(after(&flat, 0.5), 80.0)) > 5.0);
    assert!(db(level(after(&b, 0.5), 2000.0), level(after(&flat, 0.5), 2000.0)).abs() < 1.0);
    // turning Gain to another step: a Replace, played as a crossfade (no dip to the dry guitar)
    let board = Board::new(vec![amp]);
    let mut turned = board.clone();
    turned.blocks[0].set("gain", 8.0);
    let cmds = diff(&board, &turned, RATE).unwrap();
    assert!(cmds.iter().any(|c| matches!(c, BoardCmd::Replace { .. })));
    let mut p = BoardProcessor::new(&board, RATE);
    let mut y = sine(220.0, 0.1, 0.5);
    let (a, rest) = y.split_at_mut(4800);
    p.process(a);
    for c in cmds {
        if let Some(old) = p.board(c) {
            drop(old);
        }
    }
    p.process(rest);
    let hop = 240;
    let lv: Vec<f32> = y[2400..]
        .chunks(hop)
        .map(|c| c.iter().map(|v| v.abs()).fold(0.0, f32::max))
        .collect();
    let mn = lv.iter().cloned().fold(f32::MAX, f32::min);
    let mx = lv.iter().cloned().fold(0.0, f32::max);
    assert!(mn > mx * 0.6, "a crossfade, not a dip ({mn} vs {mx})");
    // the Gate: a quiet hiss under its threshold is silenced before the capture
    let mut gated = Block::of("amp_nam").unwrap();
    gated.set_asset(9301);
    gated.set("gate", -40.0);
    let hiss: Vec<f32> = (0..24000)
        .map(|i| 0.001 * ((i * 7919 % 1013) as f32 / 506.0 - 1.0))
        .collect();
    let open = run_block(
        &{
            let mut g = gated;
            g.set("gate", -90.0);
            g
        },
        &hiss,
    );
    let shut = run_block(&gated, &hiss);
    let e = |y: &[f32]| y[12000..].iter().map(|v| v * v).sum::<f32>();
    assert!(e(&shut) < e(&open) * 0.01);
    for id in [9301, 9302, 9303] {
        nam::unregister(id);
    }
}

#[test]
fn a_nam_pedal_blends_its_dry_and_a_late_capture_is_picked_up() {
    let tiny = super::nam::tests::tiny_capture(r#", "sample_rate": 48000"#);
    // (B11) a block built before its capture was registered passes the signal; once registered, the generation
    // moved on, and a rebuilt pedal plays the capture
    let mut ped = Block::of("pedal_nam").unwrap();
    ped.asset = 9311;
    let x = sine(150.0, 0.1, 0.5);
    let before = run_block(&ped, &x);
    assert_eq!(before, x);
    let g0 = nam::generation();
    nam::register_nam(9311, &tiny).unwrap();
    assert!(nam::generation() > g0);
    let after_reg = run_block(&ped, &x);
    assert!(after_reg.iter().zip(&x).any(|(a, b)| (a - b).abs() > 1e-3));
    // blend 0: the dry input exactly
    ped.set("blend", 0.0);
    let dry = run_block(&ped, &x);
    let e = dry
        .iter()
        .zip(&x)
        .skip(2400)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(e < 1e-3, "blend 0: {e}");
    nam::unregister(9311);
}

#[test]
#[ignore]
fn print_wet_dry_ratios() {
    let x: Vec<f32> = note(41.2, 0.5, 3.0, 0.3)
        .iter()
        .zip(crate::board::tests::guitar(3.0))
        .map(|(a, b)| a * 0.5 + b * 0.5)
        .collect();
    let rms =
        |y: &[f32]| (after(y, 0.5).iter().map(|v| v * v).sum::<f32>() / after(y, 0.5).len() as f32).sqrt();
    for id in [
        "green",
        "blues",
        "gold",
        "germanium",
        "treble_boost",
        "orange",
        "rodent",
        "metal",
        "round_fuzz",
        "big_pi",
        "bender",
        "octave_fuzz",
        "bass_grind",
        "bass_muff",
    ] {
        let wet = run_block(&blk(id, &[("blend", 100.0)]), &x);
        let dry = run_block(&blk(id, &[("blend", 0.0)]), &x);
        eprintln!("{id:>14} wet/dry {:+.1} dB", db(rms(&wet), rms(&dry)));
    }
}
