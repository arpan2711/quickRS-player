//! The board's tests: every model at its defaults and at every knob's extremes stays finite, bounded and quiet on
//! silence; the levels; the running board's in-place commands; JSON; the RS mapping; speed.

use super::*;
use crate::rt::ToneProcessor;

const RATE: u32 = 48000;

/// A plucked-string-ish test signal: decaying harmonics on a few notes, peaks around -12 dBFS.
pub(crate) fn guitar(seconds: f32) -> Vec<f32> {
    let r = RATE as f32;
    let n = (seconds * r) as usize;
    let notes = [82.41f32, 110.0, 146.83, 196.0, 246.94, 329.63, 220.0, 164.81];
    let per = (0.5 * r) as usize;
    let mut x = vec![0.0f32; n];
    for (k, f) in notes.iter().cycle().take(n / per + 1).enumerate() {
        let start = k * per;
        for i in 0..per.min(n.saturating_sub(start)) {
            let t = i as f32 / r;
            let env = (-t * 3.0).exp() * (1.0 - (-t * 400.0).exp());
            let mut s = 0.0;
            for h in 1..8 {
                let fh = f * h as f32;
                if fh < r * 0.45 {
                    s += (std::f32::consts::TAU * fh * t).sin() / h as f32 * (-t * h as f32 * 0.8).exp();
                }
            }
            x[start + i] += 0.16 * env * s;
        }
    }
    x
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn run(model: usize, v: [f32; MAX_KNOBS], x: &[f32]) -> Vec<f32> {
    let mut b = Block::new(model);
    b.v = v;
    let mut p = make_pedal(&b, RATE);
    let mut y = x.to_vec();
    for c in y.chunks_mut(128) {
        p.process(c);
    }
    y
}

#[test]
fn every_model_is_finite_and_bounded_at_every_extreme() {
    let x = guitar(3.0);
    for (i, m) in MODELS.iter().enumerate() {
        let mut settings = vec![m.defaults()];
        for k in 0..m.knobs.len() {
            for end in [m.knobs[k].lo, m.knobs[k].hi] {
                let mut v = m.defaults();
                v[k] = end;
                settings.push(v);
            }
        }
        // everything at its top and its bottom at once
        let mut hi = m.defaults();
        let mut lo = m.defaults();
        for (k, kd) in m.knobs.iter().enumerate() {
            hi[k] = kd.hi;
            lo[k] = kd.lo;
        }
        settings.push(hi);
        settings.push(lo);
        for v in settings {
            let y = run(i, v, &x);
            let peak = y.iter().fold(0.0f32, |a, s| a.max(s.abs()));
            assert!(
                y.iter().all(|s| s.is_finite()),
                "{} {:?}: not finite",
                m.id,
                &v[..m.knobs.len()]
            );
            assert!(peak < 50.0, "{} {:?}: peak {peak}", m.id, &v[..m.knobs.len()]);
        }
    }
}

#[test]
fn every_model_goes_quiet_on_silence() {
    // a burst, then silence: after the tail, nothing (no DC, no self-oscillation at defaults)
    let mut x = guitar(1.0);
    x.extend(std::iter::repeat_n(0.0, (RATE * 12) as usize));
    for (i, m) in MODELS.iter().enumerate() {
        let y = run(i, m.defaults(), &x);
        let tail = &y[y.len() - RATE as usize..];
        assert!(rms(tail) < 1e-3, "{}: tail {}", m.id, rms(tail));
    }
}

#[test]
fn default_levels_sit_near_the_guitars() {
    // into a model at its defaults, out at roughly the same loudness (amps and drives level themselves); delays
    // and reverbs return only their echoes, which are quieter
    let x = guitar(4.0);
    let rin = rms(&x);
    let mut report = Vec::new();
    for (i, m) in MODELS.iter().enumerate() {
        let y = run(i, m.defaults(), &x);
        let db = 20.0 * (rms(&y[RATE as usize..]) / rin).log10();
        let wet_only = make_pedal(&Block::new(i), RATE).wet_only();
        let (lo, hi) = if wet_only { (-30.0, 3.0) } else { (-8.0, 7.0) };
        let ok = db > lo && db < hi;
        report.push((
            format!("{:>14} {:+6.1} dB{}", m.id, db, if ok { "" } else { "  <--" }),
            ok,
        ));
    }
    let text: Vec<&str> = report.iter().map(|r| r.0.as_str()).collect();
    eprintln!("{}", text.join("\n"));
    assert!(report.iter().all(|r| r.1), "levels off:\n{}", text.join("\n"));
}

#[test]
fn json_round_trips_and_unknown_models_are_dropped() {
    let mut b = presets()[2].1.clone();
    b.level_db = -3.5;
    b.blocks[0].on = false;
    let j = b.to_json();
    let back = Board::from_json(&j);
    assert_eq!(back.blocks.len(), b.blocks.len());
    assert_eq!(back.level_db, -3.5);
    for (x, y) in back.blocks.iter().zip(&b.blocks) {
        assert_eq!(x.model, y.model);
        assert_eq!(x.on, y.on);
        for k in 0..x.def().knobs.len() {
            assert!((x.v[k] - y.v[k]).abs() < 1e-3);
        }
    }
    let odd: serde_json::Value = serde_json::from_str(
        r#"{"blocks":[{"model":"green","knobs":{"drive":7,"nope":3}},{"model":"from_the_future"}],"level":1}"#,
    )
    .unwrap();
    let b = Board::from_json(&odd);
    assert_eq!(b.blocks.len(), 1);
    assert_eq!(b.blocks[0].get("drive"), Some(7.0));
    // and through serde, as the tone spec stores it
    let spec = crate::tone::ToneSpec::Board { board: b.clone() };
    let s = serde_json::to_string(&spec).unwrap();
    let back: crate::tone::ToneSpec = serde_json::from_str(&s).unwrap();
    match back {
        crate::tone::ToneSpec::Board { board } => assert_eq!(board.blocks[0].get("drive"), Some(7.0)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_running_board_follows_edits_in_place() {
    let base = presets()[0].1.clone();
    let mut p = BoardProcessor::new(&base, RATE);
    let x = guitar(0.5);
    let mut y = x.clone();
    p.process(&mut y);
    // turn a knob, switch a block off, add a block, move it, take one away
    let mut next = base.clone();
    next.blocks[1].set("gain", 8.0);
    next.blocks[0].on = false;
    let added = Block::of("phaser").unwrap();
    next.blocks.insert(1, added);
    let gone = next.blocks.remove(4);
    let cmds = diff(&base, &next, RATE).expect("an in-place edit");
    assert!(cmds.iter().any(|c| matches!(c, BoardCmd::Knob { .. })));
    assert!(
        cmds.iter()
            .any(|c| matches!(c, BoardCmd::Bypass { on: false, .. }))
    );
    assert!(cmds.iter().any(|c| matches!(c, BoardCmd::Insert { .. })));
    assert!(
        cmds.iter()
            .any(|c| matches!(c, BoardCmd::Remove { uid } if *uid == gone.uid))
    );
    for c in cmds {
        assert!(p.board(c).is_none());
    }
    // the reverb that was removed rings out, then is dropped
    let mut long = guitar(0.2);
    long.extend(std::iter::repeat_n(0.0, (RATE * 7) as usize));
    p.process(&mut long);
    let mut freed = 0;
    while p.trash().is_some() {
        freed += 1;
    }
    assert_eq!(freed, 1);
    let order: Vec<u32> = p.slots.iter().map(|s| s.uid).collect();
    let want: Vec<u32> = next.blocks.iter().map(|b| b.uid).collect();
    assert_eq!(order, want);
    assert!(!p.slots[0].on);
    let gain = next.blocks[2].def().knob_index("gain").expect("the amp's gain");
    assert!((p.slots[2].v[gain] - 8.0).abs() < 1e-6, "the amp's gain");
    // a whole new board is a swap, not an edit
    assert!(diff(&next, &presets()[3].1, RATE).is_none());
}

#[test]
fn a_switched_off_delay_lets_its_tail_ring() {
    let board = Board::new(vec![
        Block::of("digital_delay")
            .unwrap()
            .with("time", 200.0)
            .with("repeats", 6.0),
    ]);
    let mut p = BoardProcessor::new(&board, RATE);
    let mut x = guitar(0.3);
    p.process(&mut x);
    let uid = board.blocks[0].uid;
    p.board(BoardCmd::Bypass { uid, on: false });
    let mut after = vec![0.0f32; (RATE / 2) as usize];
    p.process(&mut after);
    assert!(rms(&after) > 1e-3, "the echoes go on");
    // and new playing gets no new echoes
    let mut fresh = guitar(0.2);
    let dry = fresh.clone();
    fresh.extend(std::iter::repeat_n(0.0, (RATE * 3) as usize));
    p.process(&mut fresh);
    let n = dry.len();
    let late = &fresh[n + (RATE as usize) * 2..];
    assert!(rms(late) < 1e-3, "{}", rms(late));
}

#[test]
fn bypass_fades_rather_than_cuts() {
    let board = Board::new(vec![Block::of("orange").unwrap()]);
    let mut p = BoardProcessor::new(&board, RATE);
    let x: Vec<f32> = (0..RATE as usize)
        .map(|i| (std::f32::consts::TAU * 110.0 * i as f32 / RATE as f32).sin() * 0.25)
        .collect();
    let mut y = x.clone();
    let half = RATE as usize / 2;
    p.process(&mut y[..half]);
    p.board(BoardCmd::Bypass {
        uid: board.blocks[0].uid,
        on: false,
    });
    p.process(&mut y[half..]);
    // still mostly the pedal 1 ms after the switch, all dry (bit for bit) from 15 ms on
    let ms = |m: f32| (m * 1e-3 * RATE as f32) as usize;
    let early = (half..half + ms(1.0))
        .filter(|&i| (y[i] - x[i]).abs() > 1e-6)
        .count();
    assert!(early > ms(0.5), "{early}");
    assert!((half + ms(15.0)..y.len()).all(|i| y[i] == x[i]));
}

#[test]
fn rs_gear_becomes_a_board() {
    use qrs_formats::rig::RigTone;
    let v: serde_json::Value = serde_json::from_str(
        r#"{"Key":"AudBe_cleanlead","Name":"AudBe_cleanlead","ToneDescriptors":["$[1]CLEAN","$[2]LEAD"],"Volume":-16,
            "GearList":{
              "PrePedal1":{"Key":"Pedal_ValveEcho","KnobValues":{"Pedal_ValveEcho_Time":450,"Pedal_ValveEcho_Feedback":35,"Pedal_ValveEcho_Mix":35}},
              "Amp":{"Key":"Amp_CA38","KnobValues":{"Amp_CA38_Gain":30,"Amp_CA38_Bass":35,"Amp_CA38_Mid":55,"Amp_CA38_Treble":95}},
              "PostPedal1":{"Key":"Pedal_SpringReverb","KnobValues":{"Pedal_SpringReverb_Time":72,"Pedal_SpringReverb_Mix":30,"Pedal_SpringReverb_Depth":64}},
              "Cabinet":{"Key":"Cab_CA215C_57_Cone"},
              "Rack1":{"Key":"Rack_StudioEQ","KnobValues":{"Rack_StudioEQ_HiMidFreq":2.0,"Rack_StudioEQ_TrebleFreq":5.0,"Rack_StudioEQ_Treble":10}},
              "Rack2":{"Key":"Rack_SomethingNew"}}}"#,
    )
    .unwrap();
    let t = RigTone::from_json(&v).unwrap();
    let b = rs_gear::board_for(&t);
    let ids: Vec<&str> = b.blocks.iter().map(|x| x.def().id).collect();
    assert_eq!(
        ids,
        vec!["tape_echo", "amp_clean", "spring", "cab_2x12", "studio_eq"]
    );
    assert_eq!(b.blocks[0].get("time"), Some(450.0));
    assert!((b.blocks[1].get("treble").unwrap() - 9.5).abs() < 1e-4);
    assert_eq!(b.blocks[2].get("mix"), Some(30.0));
    assert_eq!(b.blocks[3].get("mic"), Some(7.0));
    assert_eq!(b.blocks[4].get("hi_freq"), Some(2000.0));
    assert_eq!(b.blocks[4].get("treble_freq"), Some(5000.0));
    // (P41.1, D161: levelled to the song tones' loudness, then RS's volume within 3 dB)
    let at = super::level::board_lufs(&b, false) + b.level_db;
    assert!(
        (at - super::level::TARGET_LUFS - super::level::rs_trim(t.volume_db)).abs() < 0.3,
        "{at} LUFS at level {}",
        b.level_db
    );
    // a Marshall by name; a 4x12 by layout
    let jcm: serde_json::Value = serde_json::from_str(
        r#"{"Key":"x_dist","ToneDescriptors":["$[1]DISTORTION"],"GearList":{"Amp":{"Key":"Amp_MarshallJCM800","KnobValues":{"Amp_MarshallJCM800_Gain":35}},"Cabinet":{"Key":"Cab_Marshall1960a_57_Edge"}}}"#,
    )
    .unwrap();
    let b = rs_gear::board_for(&RigTone::from_json(&jcm).unwrap());
    let ids: Vec<&str> = b
        .blocks
        .iter()
        .map(|x| x.def().id)
        .filter(|id| *id != "gate")
        .collect();
    assert_eq!(ids, vec!["amp_lead", "cab_4x12"]);
    assert_eq!(
        b.blocks
            .iter()
            .find(|x| x.def().id == "cab_4x12")
            .unwrap()
            .get("mic"),
        Some(2.5)
    );
    // the clean tone hisses little and gets no gate; a cranked Marshall gets one, right after the amp (P41.1: keyed
    // on the input there, D162)
    assert!(t.descriptors.iter().any(|d| d == "CLEAN"));
    assert!(!rs_gear::board_for(&t).blocks.iter().any(|x| x.def().id == "gate"));
    let loud: serde_json::Value = serde_json::from_str(
        r#"{"Key":"x_lead","ToneDescriptors":["$[1]HIGH GAIN"],"GearList":{"Amp":{"Key":"Amp_MarshallJCM800","KnobValues":{"Amp_MarshallJCM800_Gain":90}},"Cabinet":{"Key":"Cab_Marshall1960a_57_Edge"}}}"#,
    )
    .unwrap();
    let b = rs_gear::board_for(&RigTone::from_json(&loud).unwrap());
    assert_eq!(
        b.blocks.iter().map(|x| x.def().id).collect::<Vec<_>>(),
        vec!["amp_lead", "gate", "cab_4x12"]
    );
    assert_eq!(b.blocks[1].get("key"), Some(0.0), "keyed on the input");
    assert_eq!(b.blocks[1].get("range"), Some(-60.0), "a firm gate on high gain");
    assert!(hiss_gain_db(&b) < 0.0, "the gate shuts on idle hiss");
}

#[test]
fn high_gain_starting_boards_are_gated_and_quiet_between_notes() {
    for (name, b) in presets() {
        let g = hiss_gain_db(&b);
        // (P38.2: 20, where idle noise is plainly heard: Crunch and Blues got a gentle gate)
        assert!(g < GATE_HISS_DB, "{name}: idle hiss raised {g:.0} dB");
    }
}

#[test]
fn every_model_starts_silent() {
    // a board starting (a preset, a song's tone change) mustn't thump: silence in, silence out from the first sample
    for (m, def) in MODELS.iter().enumerate() {
        let y = run(m, Block::new(m).v, &vec![0.0; 9600]);
        let peak = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(
            peak < 1e-4,
            "{}: {:.1} dBFS from silence",
            def.id,
            20.0 * peak.max(1e-9).log10()
        );
    }
}

#[test]
fn a_heavy_board_runs_much_faster_than_real_time() {
    // two drives, an amp, a cab, an EQ, chorus, delay, reverb: ~35x real time optimised here (about 3% of a core;
    // the amp's ADAA in f64 is most of it), so 20x leaves room for a slower machine
    let ids = [
        "green",
        "orange",
        "amp_lead",
        "cab_4x12",
        "eq8",
        "chorus",
        "tape_echo",
        "plate",
    ];
    let board = Board::new(ids.iter().map(|id| Block::of(id).unwrap()).collect());
    let mut p = BoardProcessor::new(&board, RATE);
    let mut x = guitar(5.0);
    let t0 = std::time::Instant::now();
    for c in x.chunks_mut(128) {
        p.process(c);
    }
    let took = t0.elapsed().as_secs_f64();
    let speed = 5.0 / took;
    eprintln!("heavy board: {speed:.0}x real time");
    // (a debug build's speed on a loaded machine only warns, unless QUICKRS_STRICT_RT=1; release always asserts)
    let strict = !cfg!(debug_assertions) || std::env::var("QUICKRS_STRICT_RT").is_ok_and(|v| v == "1");
    let limit = if cfg!(debug_assertions) { 2.0 } else { 20.0 };
    if strict {
        assert!(speed > limit, "{speed:.1}x");
    } else if speed <= limit {
        eprintln!(
            "heavy board only {speed:.1}x real time (a loaded machine? QUICKRS_STRICT_RT=1 makes this fail)"
        );
    }
}

// -- nothing allocates on the audio thread ----------------------------------------------------------------------------

#[test]
fn nothing_allocates_or_frees_on_the_audio_thread() {
    // every model: processing, each knob to its ends, bypass and back, level, reorder, a model swapped in, a block
    // added, one removed and dropped to the trash (the commands' pedals are built first, as the engine does)
    let x = guitar(0.25);
    for (m, def) in MODELS.iter().enumerate() {
        let board = Board::new(vec![Block::of("green").unwrap(), Block::new(m)]);
        let (first, uid) = (board.blocks[0].uid, board.blocks[1].uid);
        let mut p = BoardProcessor::new(&board, RATE);
        let mut swapped = Block::new(m);
        swapped.uid = first;
        let swap = make_pedal(&swapped, RATE);
        let added = Block::of("plate").unwrap();
        let add = make_pedal(&added, RATE);
        let mut order = [0u32; MAX_BLOCKS];
        order[..3].copy_from_slice(&[added.uid, uid, first]);
        let mut y = x.clone();
        let mut back: Vec<Box<dyn Pedal>> = Vec::with_capacity(8);
        crate::alloc_count::watch();
        let mut chunks = y.chunks_mut(128);
        let mut run = |p: &mut BoardProcessor, k: usize| {
            for _ in 0..k {
                if let Some(c) = chunks.next() {
                    p.process(c);
                }
            }
        };
        run(&mut p, 4);
        for (k, kd) in def.knobs.iter().enumerate() {
            for value in [kd.hi, kd.lo, kd.def] {
                p.board(BoardCmd::Knob { uid, knob: k, value });
                run(&mut p, 1);
            }
        }
        p.board(BoardCmd::Bypass { uid, on: false });
        run(&mut p, 2);
        p.board(BoardCmd::Bypass { uid, on: true });
        p.board(BoardCmd::Level(3.0));
        run(&mut p, 2);
        if let Some(old) = p.board(BoardCmd::Replace {
            block: swapped,
            pedal: swap,
        }) {
            back.push(old);
        }
        if let Some(old) = p.board(BoardCmd::Insert {
            block: added,
            pedal: add,
        }) {
            back.push(old);
        }
        p.board(BoardCmd::Order { uids: order, n: 3 });
        run(&mut p, 2);
        p.board(BoardCmd::Remove { uid });
        run(&mut p, 8);
        while let Some(old) = p.trash() {
            back.push(old);
        }
        let calls = crate::alloc_count::calls();
        assert_eq!(
            calls, 0,
            "{}: {calls} allocator calls on the audio thread",
            def.id
        );
        assert!(
            !back.is_empty(),
            "{}: the swapped-out pedal comes back for the trash",
            def.id
        );
    }
}

// -- P21.5: the amps' back panel, drive diodes, tape heads ------------------------------------------------------------

fn sine(f: f32, amp: f32, seconds: f32) -> Vec<f32> {
    let r = RATE as f32;
    (0..(seconds * r) as usize)
        .map(|i| amp * (std::f32::consts::TAU * f * i as f32 / r).sin())
        .collect()
}

/// The amplitude of harmonic `h` of `f` in `y`, over half a second from the 1 s mark (`f` a whole number of Hz,
/// so that's a whole number of cycles).
fn harmonic(y: &[f32], f: f32, h: usize) -> f32 {
    let r = RATE as f32;
    let n = RATE as usize / 2;
    let seg = &y[RATE as usize..RATE as usize + n];
    let w = std::f64::consts::TAU * (f * h as f32 / r) as f64;
    let (mut c, mut s) = (0.0f64, 0.0f64);
    for (i, &v) in seg.iter().enumerate() {
        c += v as f64 * (w * i as f64).cos();
        s += v as f64 * (w * i as f64).sin();
    }
    (2.0 * (c * c + s * s).sqrt() / n as f64) as f32
}

/// Total harmonic distortion (harmonics 2 to 9 against the fundamental).
fn thd(y: &[f32], f: f32) -> f32 {
    let h1 = harmonic(y, f, 1);
    let rest: f32 = (2..10).map(|h| harmonic(y, f, h).powi(2)).sum();
    rest.sqrt() / h1.max(1e-9)
}

fn with(id: &str, knobs: &[(&str, f32)]) -> (usize, [f32; MAX_KNOBS]) {
    let mut b = Block::of(id).unwrap();
    for &(k, v) in knobs {
        assert!(b.get(k).is_some(), "{id}: {k}");
        b.set(k, v);
    }
    (b.model, b.v)
}

fn ratio_db(a: f32, b: f32) -> f32 {
    20.0 * (a.max(1e-9) / b.max(1e-9)).log10()
}

#[test]
fn sag_pulls_the_level_down_after_a_hard_hit_and_a_tube_rectifier_more() {
    // a loud, held note: its level half a second in (the supply has drooped), against no sag
    let loud = sine(110.0, 0.5, 1.0);
    let held = |x: &[f32], knobs: &[(&str, f32)]| {
        let (m, v) = with("amp_crunch", knobs);
        let y = run(m, v, x);
        let r = RATE as usize;
        rms(&y[r * 4 / 10..r * 6 / 10])
    };
    let none = held(&loud, &[("sag", 0.0)]);
    let lots = ratio_db(held(&loud, &[("sag", 10.0), ("rectifier", 0.0)]), none);
    let solid = ratio_db(held(&loud, &[("sag", 10.0), ("rectifier", 1.0)]), none);
    eprintln!("held level against no sag: sag 10 tube {lots:+.1} dB, solid {solid:+.1} dB");
    assert!(lots < -1.5, "sag: {lots}");
    assert!(lots < solid - 0.5, "tube rectifier: {lots} vs solid {solid}");
    // a quiet note doesn't work the supply: hardly any droop
    let x = sine(110.0, 0.01, 1.0);
    let quiet = ratio_db(
        held(&x, &[("sag", 10.0), ("rectifier", 0.0)]),
        held(&x, &[("sag", 0.0)]),
    );
    eprintln!("quiet: {quiet:+.2} dB");
    assert!(quiet > lots * 0.3, "quiet note {quiet} vs loud {lots}");
}

#[test]
fn cold_bias_adds_crossover_distortion_on_quiet_notes_and_hot_bias_clips_sooner() {
    // the clean amp turned down: almost no distortion at noon, crossover grit when cold
    let quiet = sine(220.0, 0.01, 1.6);
    let at = |x: &[f32], bias: f32| {
        let (m, v) = with("amp_clean", &[("gain", 2.0), ("master", 3.0), ("bias", bias)]);
        thd(&run(m, v, x), 220.0)
    };
    let (cold, mid) = (at(&quiet, 0.0), at(&quiet, 5.0));
    eprintln!("quiet THD: cold {cold:.4}, noon {mid:.4}");
    assert!(cold > mid * 3.0 && cold > 0.01, "cold {cold} vs noon {mid}");
    // pushed harder: hot bias breaks up earlier
    let loud = sine(220.0, 0.2, 1.6);
    let (hot, mid) = (at(&loud, 10.0), at(&loud, 5.0));
    eprintln!("loud THD: hot {hot:.4}, noon {mid:.4}");
    assert!(hot > mid * 1.2, "hot {hot} vs noon {mid}");
}

#[test]
fn power_tubes_and_power_master_change_the_breakup() {
    let x = sine(196.0, 0.3, 1.6);
    let at = |knobs: &[(&str, f32)]| {
        let (m, v) = with("amp_clean", knobs);
        thd(&run(m, v, &x), 196.0)
    };
    let (el34, l6, kt88) = (
        at(&[("master", 7.0), ("tubes", 0.0)]),
        at(&[("master", 7.0), ("tubes", 1.0)]),
        at(&[("master", 7.0), ("tubes", 2.0)]),
    );
    eprintln!("THD: EL34 {el34:.4}, 6L6 {l6:.4}, KT88 {kt88:.4}");
    assert!(el34 > l6 && l6 > kt88, "EL34 {el34} > 6L6 {l6} > KT88 {kt88}");
    let (low, high) = (at(&[("power_master", 0.0)]), at(&[("power_master", 10.0)]));
    eprintln!("THD: power master 0 {low:.4}, 10 {high:.4}");
    assert!(high > low * 1.5, "power master {high} vs {low}");
}

#[test]
fn led_diodes_clip_later_and_louder_than_silicon_and_asymmetric_adds_evens() {
    let x = sine(196.0, 0.05, 1.6);
    // each at a moderate gain, where silicon clips and LEDs barely do
    for (id, gain) in [
        ("green", ("drive", 5.0)),
        ("orange", ("dist", 0.0)),
        ("rodent", ("distortion", 2.0)),
    ] {
        let at = |d: f32| {
            let (m, v) = with(id, &[gain, ("diodes", d)]);
            run(m, v, &x)
        };
        let (si, led, asym) = (at(0.0), at(2.0), at(1.0));
        let r = RATE as usize;
        let (t_si, t_led) = (thd(&si, 196.0), thd(&led, 196.0));
        eprintln!(
            "{id}: THD silicon {t_si:.3}, LED {t_led:.3}; LED {:+.1} dB",
            ratio_db(rms(&led[r..]), rms(&si[r..]))
        );
        assert!(t_led < t_si * 0.8, "{id}: LED {t_led} vs silicon {t_si}");
        assert!(rms(&led[r..]) > rms(&si[r..]), "{id}: LED louder");
        let (e_si, e_asym) = (
            harmonic(&si, 196.0, 2) / harmonic(&si, 196.0, 1),
            harmonic(&asym, 196.0, 2) / harmonic(&asym, 196.0, 1),
        );
        assert!(e_asym > e_si * 2.0, "{id}: 2nd harmonic {e_asym} vs {e_si}");
    }
}

#[test]
fn tape_heads_play_at_one_two_and_three_times_the_time() {
    let r = RATE as usize;
    let mut x = vec![0.0f32; r * 2];
    x[100] = 1.0;
    let taps = |heads: f32| {
        let (m, v) = with(
            "tape_echo",
            &[
                ("time", 200.0),
                ("repeats", 0.0),
                ("wow", 0.0),
                ("mix", 100.0),
                ("heads", heads),
            ],
        );
        let y = run(m, v, &x);
        // energy in a 20 ms window around each head's echo
        [1usize, 2, 3].map(|k| {
            let at = 100 + k * r / 5;
            rms(&y[at - r / 100..at + r / 100])
        })
    };
    let one = taps(0.0);
    let two = taps(1.0);
    let all = taps(6.0);
    let late = taps(5.0); // 2+3
    eprintln!("taps: 1 {one:?}; 2 {two:?}; 1+2+3 {all:?}; 2+3 {late:?}");
    let on = |e: f32| e > 1e-3;
    assert!(on(one[0]) && !on(one[1]) && !on(one[2]));
    assert!(!on(two[0]) && on(two[1]) && !on(two[2]));
    assert!(all.iter().all(|&e| on(e)));
    assert!(!on(late[0]) && on(late[1]) && on(late[2]));
}

#[test]
fn an_old_board_without_the_new_controls_loads_with_each_models_defaults() {
    // saved before P21.5: knobs by key, none of the back panel, diodes or heads
    let old: serde_json::Value = serde_json::from_str(
        r#"{"blocks":[
            {"model":"green","on":true,"knobs":{"drive":4,"tone":6,"level":5}},
            {"model":"amp_crunch","on":true,"knobs":{"gain":7,"bass":5,"mid":6,"treble":6,"presence":5,"master":5}},
            {"model":"amp_chime","on":true,"knobs":{"gain":5,"treble":6,"bass":5,"cut":3,"master":5}},
            {"model":"amp_bass","on":true,"knobs":{"gain":4,"bass":6,"mid":5,"freq":800,"treble":5,"master":5}},
            {"model":"tape_echo","on":true,"knobs":{"time":300,"repeats":4,"mix":35,"wow":3}}
        ],"level":0}"#,
    )
    .unwrap();
    let b = Board::from_json(&old);
    assert_eq!(b.blocks.len(), 5);
    assert_eq!(b.blocks[0].get("drive"), Some(4.0));
    assert_eq!(b.blocks[0].get("diodes"), Some(0.0), "Mean Green: silicon");
    let crunch = &b.blocks[1];
    assert_eq!(crunch.get("gain"), Some(7.0));
    assert_eq!(crunch.get("tubes"), Some(0.0), "Plexi: EL34");
    assert_eq!(crunch.get("rectifier"), Some(1.0), "Plexi: solid state");
    assert_eq!(crunch.get("sag"), Some(5.0));
    assert_eq!(crunch.get("bias"), Some(5.0));
    assert_eq!(crunch.get("power_master"), Some(5.0));
    assert_eq!(b.blocks[2].get("tubes"), Some(3.0), "AC30: EL84");
    assert_eq!(b.blocks[2].get("rectifier"), Some(0.0), "AC30: tube rectifier");
    assert_eq!(b.blocks[3].get("tubes"), Some(2.0), "SVT: 6550 as KT88");
    assert_eq!(b.blocks[4].get("heads"), Some(0.0), "one head, as before");
    // and a round trip keeps the new knobs
    let again = Board::from_json(&b.to_json());
    assert_eq!(again.blocks[1].get("sag"), Some(5.0));
}

// -- P37.2: guitar sound unchanged ------------------------------------------------------------------------------------

/// A fingerprint of a rendering: its energy and two weighted sums (catch a changed sample anywhere).
fn fingerprint(y: &[f32]) -> [f64; 3] {
    let e: f64 = y.iter().map(|&v| v as f64 * v as f64).sum();
    let a: f64 = y
        .iter()
        .enumerate()
        .map(|(i, &v)| v as f64 * ((i % 977) as f64 - 488.0))
        .sum();
    let b: f64 = y
        .iter()
        .enumerate()
        .map(|(i, &v)| v as f64 * ((i * 7919 % 1009) as f64))
        .sum();
    [e, a, b]
}

/// Every model of ours at its defaults and every starting board, on the test guitar, against the fingerprints taken
/// before P37.2 (`tests/data/guitar_fingerprints.json`; bless with `QRS_BLESS=1`). New knobs default to "as before",
/// so the old models and the guitar presets sound exactly as they did; models and presets new since have no entry.
#[test]
fn guitar_sound_is_unchanged_by_the_new_controls() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/guitar_fingerprints.json");
    let x = guitar(2.0);
    let mut now = serde_json::Map::new();
    for (i, m) in MODELS.iter().enumerate().filter(|(_, m)| !m.file) {
        now.insert(
            format!("model:{}", m.id),
            serde_json::json!(fingerprint(&run(i, m.defaults(), &x))),
        );
    }
    for (name, b) in presets() {
        now.insert(
            format!("preset:{name}"),
            serde_json::json!(fingerprint(&render(&b, RATE, &x))),
        );
    }
    if std::env::var("QRS_BLESS").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&now).unwrap()).unwrap();
        return;
    }
    let old: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fingerprints")).unwrap();
    // changed on purpose in P37.2 (bass): the SVT's and the Bass Driver's high-passes moved down for a low B (and the
    // board using the SVT), the octaver's tracker (22 Hz, follows the note, a floor)
    // (P38.2: Crunch and Blues got a gentle gate: their energy is within 0.05%, only the silences between notes moved)
    // (P41.1, D162: the gate's Range, -30 dB by default (it was all the way down); Lead, Metal and Fuzz have their gate
    // after the amp, keyed on the input, with a -60 dB range: energy within 0.05%, the silences between notes moved)
    // (P41.2, D166/D168: the Rectifier, the JCM800 and the tweed's low end revoiced toward the owner's captures and
    // the Rectifier quieter turned down, the acoustic sim revoiced; with the presets on them. The Twin's knobs were
    // put in its panel's order with the same sound: its fingerprint holds.)
    const CHANGED: [&str; 15] = [
        "model:gate",
        "preset:Fuzz",
        "model:amp_bass",
        "model:bass_drive",
        "model:octaver",
        "preset:Bass",
        "preset:Crunch",
        "preset:Blues",
        "model:amp_tweed",
        "model:amp_lead",
        "model:amp_modern",
        "model:acoustic",
        "preset:Lead",
        "preset:Metal",
        "preset:Acoustic",
    ];
    let mut bad = Vec::new();
    for (k, want) in old.iter().filter(|(k, _)| !CHANGED.contains(&k.as_str())) {
        let Some(got) = now.get(k) else {
            bad.push(format!("{k}: gone"));
            continue;
        };
        for j in 0..3 {
            let (w, g) = (want[j].as_f64().unwrap(), got[j].as_f64().unwrap());
            if (w - g).abs() > 1e-6 * (1.0 + w.abs()) {
                bad.push(format!("{k}[{j}]: {w} -> {g}"));
            }
        }
    }
    assert!(bad.is_empty(), "changed:\n{}", bad.join("\n"));
}

// -- P41.2: the new guitar voices --------------------------------------------------------------------------------------

/// Shares of a rendering's energy in six bands (under 150 Hz, 150-400, 400-1k, 1-2.5k, 2.5-5k, over 5k), from the
/// energy each band's filter passes.
fn shape(y: &[f32]) -> [f32; 6] {
    let edges = [150.0f32, 400.0, 1000.0, 2500.0, 5000.0];
    let band = |lo: Option<f32>, hi: Option<f32>| {
        let mut fs: Vec<dsp::Biquad> = Vec::new();
        if let Some(f) = lo {
            fs.push(dsp::Biquad::new(RATE as f32, dsp::Filter::HighPass { f, q: 0.7 }));
            fs.push(dsp::Biquad::new(RATE as f32, dsp::Filter::HighPass { f, q: 0.7 }));
        }
        if let Some(f) = hi {
            fs.push(dsp::Biquad::new(RATE as f32, dsp::Filter::LowPass { f, q: 0.7 }));
            fs.push(dsp::Biquad::new(RATE as f32, dsp::Filter::LowPass { f, q: 0.7 }));
        }
        y.iter()
            .map(|&v| {
                let mut s = v;
                for b in fs.iter_mut() {
                    s = b.tick(s);
                }
                s * s
            })
            .sum::<f32>()
    };
    let mut e = [0.0f32; 6];
    for (k, o) in e.iter_mut().enumerate() {
        let lo = (k > 0).then(|| edges[k - 1]);
        let hi = (k < 5).then(|| edges[k]);
        *o = band(lo, hi);
    }
    let tot: f32 = e.iter().sum::<f32>().max(1e-12);
    e.map(|v| v / tot)
}

#[test]
fn the_new_guitar_voices_are_their_own_and_their_panels_work() {
    // every guitar amp at the same gain on the test guitar: no two the same shape (summed band differences)
    let x = guitar(3.0);
    let ids = [
        "amp_crunch",
        "amp_lead",
        "amp_modern",
        "amp_cali",
        "amp_snake",
        "amp_tangerine",
        "amp_surrey",
    ];
    let shapes: Vec<[f32; 6]> = ids
        .iter()
        .map(|id| {
            let (m, v) = with(id, &[("gain", 7.0)]);
            shape(&run(m, v, &x))
        })
        .collect();
    for i in 0..ids.len() {
        eprintln!(
            "{:>14}: {:?}",
            ids[i],
            shapes[i].map(|v| (v * 1000.0).round() / 10.0)
        );
        for j in i + 1..ids.len() {
            let d: f32 = shapes[i].iter().zip(&shapes[j]).map(|(a, b)| (a - b).abs()).sum();
            assert!(d > 0.04, "{} and {} sound alike ({d:.3})", ids[i], ids[j]);
        }
    }
    // distortion on a held note at each one's default: the hot-rodded heads past the 800, the Hiwatt the cleanest
    let note = sine(196.0, 0.1, 1.6);
    let dist = |id: &str, knobs: &[(&str, f32)]| {
        let (m, v) = with(id, knobs);
        thd(&run(m, v, &note), 196.0)
    };
    let (lead, snake, cali, surrey) = (
        dist("amp_lead", &[("gain", 6.0)]),
        dist("amp_snake", &[("gain", 6.0)]),
        dist("amp_cali", &[("gain", 6.0)]),
        dist("amp_surrey", &[]),
    );
    eprintln!("THD: 800 {lead:.3}, SLO {snake:.3}, Mark {cali:.3}, Hiwatt {surrey:.3}");
    assert!(snake > lead, "SLO {snake} vs 800 {lead}");
    assert!(surrey < lead * 0.5 && surrey < cali * 0.5, "Hiwatt {surrey}");
    // each one's gain knob drives it harder (the Hiwatt's Normal and Brilliant volumes together), on a soft note
    let soft = sine(196.0, 0.01, 1.6);
    for (id, keys) in [
        ("amp_cali", &["gain"][..]),
        ("amp_snake", &["gain"][..]),
        ("amp_tangerine", &["gain"][..]),
        ("amp_surrey", &["gain", "brilliant"][..]),
    ] {
        let at = |g: f32| {
            let kv: Vec<(&str, f32)> = keys.iter().map(|k| (*k, g)).collect();
            let (m, v) = with(id, &kv);
            thd(&run(m, v, &soft), 196.0)
        };
        let (lo, hi) = (at(1.0), at(9.0));
        eprintln!("{id}: THD at 1 {lo:.3}, at 9 {hi:.3}");
        assert!(hi > lo * 1.5, "{id}: {lo} -> {hi}");
    }
    // the Mark's graphic EQ: each slider lifts its own band
    let band_of = |k: usize, db: f32| {
        let key = ["eq80", "eq240", "eq750", "eq2200", "eq6600"][k];
        let (m, v) = with("amp_cali", &[("gain", 3.0), (key, db)]);
        let f = amp::GEQ_HZ[k];
        let y = run(m, v, &x);
        let mut bp = dsp::Biquad::new(RATE as f32, dsp::Filter::Peak { f, q: 4.0, db: 24.0 });
        let all = rms(&y);
        let lifted: Vec<f32> = y.iter().map(|&s| bp.tick(s)).collect();
        rms(&lifted) / all.max(1e-9)
    };
    for k in 0..5 {
        let (down, up) = (band_of(k, -12.0), band_of(k, 12.0));
        eprintln!("Mark EQ band {k}: {down:.3} -> {up:.3}");
        assert!(up > down * 1.08, "EQ band {k}: {down} -> {up}");
    }
}

#[test]
fn the_guitar_4x10_is_brighter_and_lighter_than_the_4x12() {
    let x = guitar(3.0);
    let at = |id: &str| {
        let (m, v) = with(id, &[]);
        shape(&run(m, v, &x))
    };
    let (ten, twelve) = (at("cab_4x10_open"), at("cab_4x12"));
    eprintln!("4x10 {ten:?}\n4x12 {twelve:?}");
    assert!(ten[0] < twelve[0], "lows {} vs {}", ten[0], twelve[0]);
    assert!(ten[3] + ten[4] > twelve[3] + twelve[4], "upper mids");
}
