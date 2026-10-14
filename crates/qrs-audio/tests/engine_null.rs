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

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[test]
fn transport_on_the_silent_timer() {
    let e = Engine::new();
    assert!(e.settle(Duration::from_secs(2)));
    assert!(e.status().silent_output);
    e.load_song(song(1.0));
    sleep(30);
    assert_eq!(e.position(), 0.0);
    e.play(None);
    assert!(e.playing());
    sleep(300);
    let p = e.position();
    assert!((0.2..0.45).contains(&p), "position after 300 ms: {p}");
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
    e.toggle();
    sleep(50);
    assert!(e.playing() && e.position() < 0.2);
}

#[test]
fn position_never_steps_back_while_playing() {
    let e = Engine::new();
    e.settle(Duration::from_secs(2));
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

#[test]
fn preview_fades_in_and_out() {
    let e = Engine::new();
    e.settle(Duration::from_secs(2));
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
    assert!(e.settle(Duration::from_secs(2)));
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
    assert!(e.stats().monitor_gaps <= 2, "{:?}", e.stats());
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
    assert!(e.settle(Duration::from_secs(2)));
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
    e.settle(Duration::from_secs(2));
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
    assert!(e.settle(Duration::from_secs(2)));
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
    e.settle(Duration::from_secs(2));
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
