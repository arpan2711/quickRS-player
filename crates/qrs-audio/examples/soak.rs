//! Soak test, SILENT: the automatic route with a song playing at volume 0 (and monitoring off) for N minutes,
//! stats every minute.   cargo run --release -p qrs-audio --features asio --example soak -- CONFIG.json MINUTES [INPUT]
//! INPUT: part of an input device's name to use instead of the automatic choice (e.g. a webcam mic).
use std::time::{Duration, Instant};

use qrs_audio::decode::SongBuffer;
use qrs_audio::{Audio, AudioPrefs};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dirs: Vec<std::path::PathBuf> = std::fs::read_to_string(&a[1])
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["library_dirs"].as_array().cloned())
        .map(|v| v.iter().filter_map(|d| d.as_str().map(Into::into)).collect())
        .unwrap_or_default();
    let minutes: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);
    let input = match a.get(3) {
        Some(part) => {
            let p = part.to_lowercase();
            let dev = qrs_audio::backend::enumerate()
                .into_iter()
                .find(|d| d.input_channels > 0 && d.label().to_lowercase().contains(&p))
                .unwrap_or_else(|| panic!("no input device matching {part:?}"));
            qrs_audio::Choice::Device(qrs_audio::DevicePref::of(&dev))
        }
        None => qrs_audio::Choice::Auto,
    };
    let audio = Audio::start(
        AudioPrefs {
            input,
            ..Default::default()
        },
        dirs,
    );
    let e = &audio.engine;
    e.settle(Duration::from_secs(3));
    e.set_monitor(false);
    e.set_song_gain(0.0);
    let rate = e.rate();
    // a 10-minute "song" of low noise at volume 0: exercises the mixer without making a sound
    let n = rate as usize * 600;
    let mut s = Vec::with_capacity(n * 2);
    let mut x: u32 = 1;
    for _ in 0..n * 2 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        s.push(((x >> 16) as i16) >> 6);
    }
    e.load_song(SongBuffer {
        rate,
        samples: std::sync::Arc::new(s),
    });
    std::thread::sleep(Duration::from_millis(100));
    e.play(Some(0.0));
    let st = e.status();
    println!(
        "soak {minutes} min: in {:?} / out {:?}",
        st.input
            .as_ref()
            .map(|i| format!("{} {} block {}", i.device_name, i.mode, i.block)),
        st.output
            .as_ref()
            .map(|o| format!("{} {} block {}", o.device_name, o.mode, o.block))
    );
    e.reset_stats();
    let start = Instant::now();
    for m in 1..=minutes {
        std::thread::sleep(Duration::from_secs(60));
        if !e.playing() {
            e.play(Some(0.0));
        }
        let s = e.stats();
        println!(
            "{m:>3} min: dropouts {} cushion gaps {} worst callback {:.3} ms, callbacks in/out {}/{}, position {:.1}",
            s.dropouts,
            s.monitor_gaps,
            s.max_work_ns as f64 / 1e6,
            s.in_callbacks,
            s.out_callbacks,
            e.position()
        );
        for ev in audio.events() {
            println!("     event {ev:?}");
        }
    }
    println!("done after {:.0} s", start.elapsed().as_secs_f64());
}
