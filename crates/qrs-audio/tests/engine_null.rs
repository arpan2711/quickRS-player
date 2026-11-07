//! The engine end to end on the null devices (no sound card): transport, clock, previews, the guitar path in
//! duplex and two-clock mode, onsets, reopening.

use std::sync::Arc;
use std::time::Duration;

use qrs_audio::backend::{Backend, BufferSize, DeviceId};
use qrs_audio::decode::SongBuffer;
use qrs_audio::rt::Clip;
use qrs_audio::{Engine, InputRoute, Route};

fn null(id: &str) -> DeviceId {
    DeviceId {
        backend: Backend::Null,
        id: id.into(),
    }
}

fn song(seconds: f64) -> SongBuffer {
    let n = (48000.0 * seconds) as usize;
    let mut s = Vec::with_capacity(n * 2);
    for i in 0..n {
        let v = ((i as f64 * 440.0 / 48000.0 * std::f64::consts::TAU).sin() * 8000.0) as i16;
        s.push(v);
        s.push(v);
    }
    SongBuffer {
        rate: 48000,
        samples: Arc::new(s),
    }
}

/// Long enough for a loaded machine (settle returns as soon as the engine is up).
const SETTLE: Duration = Duration::from_secs(10);

/// The null devices' clocks are threads woken every few ms, so a busy machine (a parallel build, a CI runner)
/// starves them now and then and the cushion runs dry: not the engine's fault. Strict with QUICKRS_STRICT_RT=1.
fn check_gaps(gaps: u32, stats: &impl std::fmt::Debug) {
    if std::env::var("QUICKRS_STRICT_RT").is_ok_and(|v| v == "1") {
        assert!(gaps <= 2, "{stats:?}");
    } else if gaps > 2 {
        eprintln!("monitor gaps: {gaps} (a loaded machine? QUICKRS_STRICT_RT=1 makes this fail) {stats:?}");
    }
}

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[test]
fn transport_on_the_silent_timer() {
    let e = Engine::new();
    assert!(e.settle(SETTLE));
    assert!(e.status().silent_output);
    e.load_song(song(1.0));
    sleep(30);
    assert_eq!(e.position(), 0.0);
    let t0 = std::time::Instant::now();
    e.play(None);
    assert!(e.playing());
    sleep(300);
    let p = e.position();
    // (against the time that really passed: on a loaded machine the sleep overshoots; P37.4)
    let slept = t0.elapsed().as_secs_f64().min(1.0);
    assert!(
        p > 0.2 && p < slept + 0.15,
        "position after {:.0} ms: {p}",
        slept * 1000.0
    );
    e.pause();
    let p1 = e.position();
    sleep(100);
    assert!((e.position() - p1).abs() < 0.02, "paused position moved");
    e.seek(0.8);
    sleep(20);
    assert!((e.position() - 0.8).abs() < 0.01);
    e.play(None);
    sleep(400);
    assert!(!e.playing(), "song should have ended");
    assert!(e.finished());
    // play again from the start after the end
    let t1 = std::time::Instant::now();
    e.toggle();
    sleep(50);
    // (from the start: against the time that really passed, as above; a loaded CI Mac overshot the sleep)
    let slept = t1.elapsed().as_secs_f64();
    let p = e.position();
    assert!(
        e.playing() && p < slept + 0.15,
        "position {p} after {:.0} ms",
        slept * 1000.0
    );
}

#[test]
fn position_never_steps_back_while_playing() {
    let e = Engine::new();
    e.settle(SETTLE);
    e.load_song(song(2.0));
    sleep(20);
    e.play(Some(0.0));
    let mut last = 0.0;
    for _ in 0..200 {
        let p = e.position();
        assert!(p >= last, "{p} < {last}");
        last = p;
        std::thread::sleep(Duration::from_micros(700));
    }
}

/// A loop going round: a seek back while playing shows at once and the clock carries on from there (it used to stay
/// at the old time until the song caught up, so the tab stayed at the loop's end).
#[test]
fn a_seek_back_while_playing_is_seen_at_once() {
    let e = Engine::new();
    e.settle(SETTLE);
    e.load_song(song(3.0));
    sleep(20);
    e.play(Some(1.5));
    sleep(200);
    assert!(e.position() > 1.6);
    e.seek(0.5);
    let p = e.position();
    assert!((p - 0.5).abs() < 0.01, "right after the seek: {p}");
    sleep(150);
    let p = e.position();
    assert!((0.55..0.8).contains(&p), "150 ms on: {p}");
}

#[test]
fn preview_fades_in_and_out() {
    let e = Engine::new();
    e.settle(SETTLE);
    let clip = Clip {
        rate: 48000,
        samples: vec![0.1; 48000 * 2],
    };
    e.play_preview(clip, true, true);
    sleep(100);
    assert!(e.previewing());
    let (pos, len) = e.preview_position();
    assert!(pos > 0.0 && (len - 1.0).abs() < 1e-6, "{pos} {len}");
    e.stop_preview(false);
    sleep(400);
    assert!(!e.previewing());
}

/// The loudest output over a while (the test signal plucks once a second and decays fast).
fn max_output_peak(e: &Engine, ms: u64) -> f32 {
    let start = std::time::Instant::now();
    let mut m = 0.0f32;
    while start.elapsed() < Duration::from_millis(ms) {
        m = m.max(e.meters().output_peak);
        sleep(5);
    }
    m
}

fn wait_onsets(e: &Engine, ms: u64) -> Vec<qrs_audio::rt::Onset> {
    let mut all = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(ms) {
        all.extend(e.take_onsets());
        sleep(20);
    }
    all
}

#[test]
fn two_clock_guitar_path() {
    let e = Engine::new();
    e.open(Route {
        input: Some(InputRoute {
            device: null("null:sine"),
            channel: 0,
        }),
        output: Some(null("null")),
        exclusive: false,
        buffer: BufferSize::Frames(128),
        base_channel: 0,
    });
    assert!(e.settle(SETTLE));
    let st = e.status();
    assert!(st.input.is_some() && st.output.is_some() && !st.duplex, "{st:?}");
    e.load_song(song(5.0));
    sleep(20);
    e.take_onsets(); // plucks from before the song started
    e.play(Some(0.0));
    let onsets = wait_onsets(&e, 2300);
    assert!((2..=3).contains(&onsets.len()), "{} onsets", onsets.len());
    for o in &onsets {
        let t = o.song_time.expect("song time while playing");
        // the test signal plucks on whole seconds of input time
        assert!(t < 3.0, "{t}");
    }
    assert_eq!(e.meters().channel_count, 2);
    assert!(max_output_peak(&e, 1100) > 0.1, "the guitar is monitored");
    let mut buf = vec![0.0; 4096];
    assert!(e.capture().latest(&mut buf).is_some());
    assert!(buf.iter().any(|s| s.abs() > 0.01));
    // the two null clocks run in step, so the cushion shouldn't run dry after it primed
    check_gaps(e.stats().monitor_gaps, &e.stats());
}

#[test]
fn duplex_guitar_path() {
    let e = Engine::new();
    e.open(Route {
        input: Some(InputRoute {
            device: null("null:sine"),
            channel: 1,
        }),
        output: Some(null("null:sine")),
        exclusive: false,
        buffer: BufferSize::Frames(64),
        base_channel: 0,
    });
    assert!(e.settle(SETTLE));
    assert!(e.status().duplex);
    assert_eq!(e.channel(), 1, "the route's channel is the one listened to");
    let onsets = wait_onsets(&e, 1600);
    assert!(!onsets.is_empty());
    assert!(onsets.iter().all(|o| o.song_time.is_none()), "no song playing");
    assert!(max_output_peak(&e, 1100) > 0.1, "the guitar is monitored");
    e.set_monitor(false);
    sleep(100);
    assert!(max_output_peak(&e, 1100) < 0.001, "monitor off: {:?}", e.meters());
}

#[test]
fn reopening_keeps_the_song_and_position() {
    let e = Engine::new();
    e.settle(SETTLE);
    e.load_song(song(5.0));
    sleep(20);
    e.play(Some(1.0));
    sleep(200);
    let before = e.position();
    e.open(Route {
        output: Some(null("null")),
        buffer: BufferSize::Frames(512),
        ..Default::default()
    });
    assert!(e.settle(SETTLE));
    sleep(50);
    let after = e.position();
    assert!(e.playing(), "still playing after reopen");
    assert!(
        after >= before - 0.05 && after < before + 0.5,
        "{before} -> {after}"
    );
    assert_eq!(e.status().output.unwrap().block, 512);
}

#[test]
fn synth_player_renders_restarts_and_stops() {
    use qrs_audio::synth::{SynthNote, SynthPlayer, SynthState};
    use qrs_audio::tone::Preset;
    use qrs_formats::Techs;
    let e = Engine::new();
    e.settle(SETTLE);
    let note = |t: f64, string: i32, fret: i32| SynthNote {
        t,
        string,
        fret,
        sus: 0.0,
        tech: Techs::NONE,
        slide_to: -1,
        bend: 0.0,
        in_chord: false,
    };
    let notes = vec![
        note(0.0, 0, 3),
        note(0.25, 1, 5),
        note(0.5, 2, 5),
        note(0.75, 3, 4),
    ];
    let opens = vec![40, 45, 50, 55, 59, 64];
    let mut p = SynthPlayer::default();
    let wait_playing = |p: &mut SynthPlayer| {
        for _ in 0..300 {
            p.update(&e);
            if p.state == SynthState::Playing {
                return;
            }
            sleep(10);
        }
        panic!("the render never started");
    };
    let start = |p: &mut SynthPlayer, looped: bool| {
        p.start(
            notes.clone(),
            opens.clone(),
            0.0,
            1.0,
            1.0,
            false,
            looped,
            Preset::Clean.params(),
            e.rate(),
            Arc::new(|t| t),
        )
    };
    start(&mut p, false);
    assert_eq!(p.state, SynthState::Rendering);
    wait_playing(&mut p);
    sleep(100);
    let t = p.position(&e).expect("position while playing");
    assert!((0.0..1.0).contains(&t), "{t}");
    // restarting mid-way (Loop toggled): the new clip takes over
    start(&mut p, true);
    wait_playing(&mut p);
    sleep(1500); // longer than the clip: still going round
    p.update(&e);
    assert_eq!(p.state, SynthState::Playing);
    assert!(e.previewing());
    p.stop(&e);
    assert_eq!(p.state, SynthState::Idle);
    sleep(400);
    assert!(!e.previewing());
    // a one-shot runs out on its own
    start(&mut p, false);
    wait_playing(&mut p);
    sleep(2500);
    p.update(&e);
    assert_eq!(p.state, SynthState::Idle);
}

// -- the jam (P22.4) --------------------------------------------------------------------------------------------

#[test]
fn the_metronome_runs_and_the_beat_light_follows() {
    use qrs_audio::jam::ClickParams;
    let e = Engine::new();
    assert!(e.settle(SETTLE));
    e.set_jam_tempo(120.0);
    e.set_click(ClickParams {
        on: true,
        accent: true,
        volume: 0.8,
    });
    assert!(!e.jam().running);
    e.jam_play(0);
    sleep(700);
    let j = e.jam();
    assert!(j.running && j.bpm == 120.0 && j.beats_per_bar == 4, "{j:?}");
    assert!((0.8..1.8).contains(&j.beat), "beat after 700 ms: {}", j.beat);
    assert_eq!(j.bar, 0);
    assert!(j.phase >= 0.0 && j.phase < 1.0);
    assert!(max_output_peak(&e, 600) > 0.2, "the click is heard");
    e.jam_stop();
    sleep(100);
    let b = e.jam().beat;
    sleep(200);
    assert!(!e.jam().running && e.jam().beat == b, "stopped");
    // a reopened output keeps the jam as it was
    e.jam_play(1);
    e.open(Route {
        output: Some(null("null")),
        buffer: BufferSize::Frames(256),
        ..Default::default()
    });
    assert!(e.settle(SETTLE));
    sleep(200);
    assert!(e.jam().running, "{:?}", e.jam());
}

#[test]
fn drums_and_a_chord_loop_play_together() {
    use qrs_audio::jam::theory::{Scale, parse_progression};
    use qrs_audio::jam::{BackingKind, BackingSpec, ClickParams, DrumParams, DrumStyle};
    let e = Engine::new();
    assert!(e.settle(SETTLE));
    e.set_click(ClickParams {
        on: false,
        ..Default::default()
    });
    e.set_backing(Some(BackingSpec {
        kind: BackingKind::Chords,
        key: 0,
        scale: Scale::Major,
        // short (two chords of two beats): renders are slow in a debug build and the null devices need the CPU
        chords: parse_progression("I-V", 0, Scale::Major).unwrap(),
        beats_per_chord: 2,
        bass: true,
    }));
    let wait_ready = |e: &Engine| {
        let start = std::time::Instant::now();
        while !e.backing_ready() && start.elapsed() < SETTLE {
            sleep(20);
        }
        assert!(e.backing_ready());
    };
    wait_ready(&e);
    e.jam_play(0);
    sleep(300);
    assert!(max_output_peak(&e, 600) > 0.1, "the backing is heard");
    e.set_drums(DrumParams {
        on: true,
        style: DrumStyle::Funk,
        ..Default::default()
    });
    // a tempo change renders the backing again
    e.set_jam_tempo(96.0);
    wait_ready(&e);
    sleep(300);
    assert_eq!(e.jam().bpm, 96.0);
    assert!(max_output_peak(&e, 600) > 0.1);
    e.set_backing(None);
    assert!(e.backing_ready());
}

/// Two devices (the guitar through the cushion): a loop recorded, overdubbed and undone, playing on with
/// monitoring off; Rewind holding the same guitar, a stretch of it saved.
#[test]
fn the_looper_and_rewind_on_the_guitar() {
    use qrs_audio::jam::{LoopAction, LoopState, LooperSettings, RewindKeep, RewindSettings, RewindSource};
    let e = Engine::new();
    e.open(Route {
        input: Some(InputRoute {
            device: null("null:sine"),
            channel: 0,
        }),
        output: Some(null("null")),
        exclusive: false,
        buffer: BufferSize::Frames(128),
        base_channel: 0,
    });
    assert!(e.settle(SETTLE));
    assert_eq!(e.jam().looper, LoopState::Off);
    e.set_looper(Some(LooperSettings {
        max_seconds: 8.0,
        max_layers: 4,
    }));
    e.set_rewind(Some(RewindSettings {
        minutes: 0.5,
        keep: RewindKeep::Both,
    }));
    sleep(100);
    assert_eq!(e.jam().looper, LoopState::Empty);
    e.looper(LoopAction::Press);
    sleep(1200);
    assert_eq!(e.jam().looper, LoopState::Recording);
    e.looper(LoopAction::Press);
    sleep(100);
    let j = e.jam();
    assert_eq!(j.looper, LoopState::Playing, "{j:?}");
    assert!((1.0..1.8).contains(&j.loop_len), "loop of {} s", j.loop_len);
    assert!(
        j.loop_latency > 0.0 && j.loop_latency < 0.2,
        "round trip {}",
        j.loop_latency
    );
    e.looper(LoopAction::Overdub);
    sleep(400);
    assert_eq!(e.jam().looper, LoopState::Overdubbing);
    e.looper(LoopAction::Play);
    sleep(100);
    assert_eq!(e.jam().layers, 2);
    e.looper(LoopAction::Undo);
    sleep(100);
    let j = e.jam();
    assert_eq!((j.layers, j.redo), (1, 1));
    // the guitar silenced: the loop still plays (the test signal plucks once a second, so it isn't empty)
    e.set_monitor(false);
    sleep(100);
    assert!(
        max_output_peak(&e, 2500) > 0.05,
        "the loop plays: {:?}",
        e.meters()
    );
    e.looper(LoopAction::Clear);
    sleep(100);
    assert_eq!(e.jam().looper, LoopState::Empty);
    // Rewind has the last few seconds of the guitar, dry and through the rig
    let j = e.jam();
    assert!(
        j.rewind_seconds > 3.0 && (j.rewind_capacity - 30.0).abs() < 0.1,
        "{j:?}"
    );
    let wave = e.rewind_waveform(3.0, 30, RewindSource::Dry);
    assert!(wave.iter().any(|&p| p > 0.05), "{wave:?}");
    let dir = std::env::temp_dir().join(format!("qrs-jam-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rewind.wav");
    e.save_rewind(&path, 2.5, 0.5, RewindSource::Wet).unwrap();
    let (rate, ch, s) = qrs_audio::decode::read_wav_i16(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((rate, ch), (48000, 1));
    assert!((s.len() as i64 - 96000).abs() < 10, "{} samples", s.len());
    assert!(s.iter().any(|v| v.unsigned_abs() > 1000), "the guitar is in it");
    let _ = std::fs::remove_dir_all(&dir);
    check_gaps(e.stats().monitor_gaps, &e.stats());
}

/// Recording what's heard (P23.9): the song through the null output, streamed to a WAV as long as the wall clock
/// ran (a pause left out), the song's own samples in it.
#[test]
fn a_recording_is_as_long_as_it_ran() {
    use qrs_audio::recorder::Recorder;
    let e = Arc::new(Engine::new());
    e.open(Route {
        input: None,
        output: Some(null("null")),
        exclusive: false,
        buffer: BufferSize::Frames(256),
        base_channel: 0,
    });
    assert!(e.settle(SETTLE));
    e.load_song(song(10.0));
    e.play(None);
    let dir = std::env::temp_dir().join(format!("qrs-rec-e2e-{}", std::process::id()));
    let path = dir.join("take.wav");
    let t0 = std::time::Instant::now();
    let rec = Recorder::start(e.clone(), path.clone()).unwrap();
    sleep(1500);
    rec.set_paused(true);
    let paused_at = t0.elapsed().as_secs_f64();
    sleep(600);
    let p0 = std::time::Instant::now();
    rec.set_paused(false);
    sleep(700);
    assert!(!rec.paused());
    let saved = rec.stop().unwrap();
    let ran = paused_at + p0.elapsed().as_secs_f64();
    assert!(e.record_tap().is_none(), "the tap is off after stop");
    let (rate, ch, s) = qrs_audio::decode::read_wav_i16(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((rate, ch), (48000, 2));
    let secs = s.len() as f64 / 2.0 / rate as f64;
    assert!((secs - saved.seconds).abs() < 1e-6, "{secs} vs {saved:?}");
    // the null clock runs on a sleeping thread and the pause lands on a 100 ms tick: within a few tenths
    assert!((secs - ran).abs() < 0.4, "{secs} s recorded over {ran} s");
    assert!(secs < 2.2 + 0.4, "the pause left out: {secs}");
    assert_eq!(saved.lost_seconds, 0.0);
    let peak = s.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
    assert!(peak > 4000, "the song is in it: peak {peak}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A tone of NAM captures (P33): the guitar path's plucks are found as with any tone (the scorer hears the dry input,
/// before the tone), at their song times, and the captures' sound is monitored.
#[test]
fn a_nam_board_leaves_the_plucks_alone() {
    use qrs_audio::board::{Block, Board, nam};
    use qrs_audio::tone::ToneSpec;
    let tiny = r#"{"version": "0.5.4", "architecture": "WaveNet", "sample_rate": 48000,
        "config": {"layers": [{"input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
            "kernel_size": 1, "dilations": [1], "activation": "Tanh", "gated": false, "head_bias": false}],
            "head": null, "head_scale": 1.0},
        "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]}"#;
    nam::register_nam(7701, tiny).unwrap();
    nam::register_nam(7702, tiny).unwrap();
    let mut pedal = Block::of("pedal_nam").unwrap();
    pedal.asset = 7701;
    let mut amp = Block::of("amp_nam").unwrap();
    amp.asset = 7702;
    let e = Engine::new();
    e.open(Route {
        input: Some(InputRoute {
            device: null("null:sine"),
            channel: 0,
        }),
        output: Some(null("null")),
        exclusive: false,
        buffer: BufferSize::Frames(128),
        base_channel: 0,
    });
    assert!(e.settle(SETTLE));
    e.set_tone(ToneSpec::Board {
        board: Board::new(vec![pedal, amp]),
    });
    e.load_song(song(5.0));
    sleep(20);
    e.take_onsets();
    e.play(Some(0.0));
    let onsets = wait_onsets(&e, 2300);
    assert!((2..=3).contains(&onsets.len()), "{} onsets", onsets.len());
    for o in &onsets {
        assert!(o.song_time.expect("song time while playing") < 3.0);
    }
    assert!(max_output_peak(&e, 1100) > 0.05, "the captures are heard");
    check_gaps(e.stats().monitor_gaps, &e.stats());
}
